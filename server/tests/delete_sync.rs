// Drive the production client and authenticated server over real HTTP, without a GUI.
#[allow(dead_code)]
#[path = "../src/api.rs"]
mod api;
#[allow(dead_code)]
#[path = "../src/crypto.rs"]
mod crypto;
#[allow(dead_code)]
#[path = "../../src-tauri/src/models.rs"]
mod models;
#[allow(dead_code)]
#[path = "../src/storage.rs"]
mod server_storage;
#[allow(dead_code)]
#[path = "../../src-tauri/src/storage.rs"]
mod storage;
#[allow(dead_code)]
#[path = "../../src-tauri/src/sync.rs"]
mod sync;

use axum::response::IntoResponse;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use copywraith_core::api_types::{CreateEntryRequest, ListEntriesResponse};
use copywraith_core::models::{ClipboardEntry, ClipboardFlavors, ContentType};
use copywraith_core::sync_protocol::*;
use reqwest::StatusCode;
use storage::LocalStorage;
use sync::SyncClient;

const PASSWORD: &str = "fixture-password";
const SERVER_TEXT_LIMIT: usize = 10 * 1024 * 1024;
const TEST_REQUEST_LIMIT: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy)]
enum OperationFault {
    Reject(StatusCode),
    CommitThenReject(StatusCode),
    CommitThenTimeout,
}

#[derive(Default)]
struct OperationFaults {
    faults: HashMap<String, OperationFault>,
    attempts: HashMap<String, Vec<String>>,
}

struct AppState {
    storage: server_storage::Storage,
    crypto: crypto::SharedCryptoState,
}

struct Server {
    _dir: tempfile::TempDir,
    url: String,
    task: tokio::task::JoinHandle<()>,
    discovery_hidden: Arc<AtomicBool>,
    operation_faults: Arc<Mutex<OperationFaults>>,
}

impl Server {
    fn fault(&self, operation_id: &str, fault: OperationFault) {
        self.operation_faults
            .lock()
            .unwrap()
            .faults
            .insert(operation_id.into(), fault);
    }

    fn clear_faults(&self) {
        self.operation_faults.lock().unwrap().faults.clear();
    }
    async fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("auth.json"),
            include_bytes!("fixtures/auth.json"),
        )
        .unwrap();
        let state = Arc::new(AppState {
            storage: server_storage::Storage::new(dir.path()).unwrap(),
            crypto: Mutex::new(crypto::CryptoState::load(dir.path()).unwrap()),
        });
        let discovery_hidden = Arc::new(AtomicBool::new(false));
        let hidden = discovery_hidden.clone();
        let operation_faults = Arc::new(Mutex::new(OperationFaults::default()));
        let faults = operation_faults.clone();
        let app = axum::Router::new()
            .nest("/api", api::router())
            .with_state(state)
            .layer(axum::middleware::from_fn(
                move |request: axum::extract::Request, next: axum::middleware::Next| {
                    let hidden = hidden.clone();
                    let faults = faults.clone();
                    async move {
                        if request.uri().path() == "/api/sync" && hidden.load(Ordering::Relaxed) {
                            return StatusCode::NOT_FOUND.into_response();
                        }
                        if request.uri().path().ends_with("/operations") {
                            let (parts, body) = request.into_parts();
                            let bytes = axum::body::to_bytes(body, TEST_REQUEST_LIMIT)
                                .await
                                .unwrap();
                            let mutation: SyncMutation = serde_json::from_slice(&bytes).unwrap();
                            let fault = {
                                let mut faults = faults.lock().unwrap();
                                faults
                                    .attempts
                                    .entry(mutation.operation_id.clone())
                                    .or_default()
                                    .push(copywraith_core::content::hash_bytes(&bytes));
                                faults.faults.get(&mutation.operation_id).copied()
                            };
                            if let Some(OperationFault::Reject(status)) = fault {
                                return status.into_response();
                            }
                            let response = next
                                .run(axum::extract::Request::from_parts(
                                    parts,
                                    axum::body::Body::from(bytes),
                                ))
                                .await;
                            return match fault {
                                Some(OperationFault::CommitThenReject(status)) => {
                                    status.into_response()
                                }
                                Some(OperationFault::CommitThenTimeout) => {
                                    std::future::pending().await
                                }
                                _ => response,
                            };
                        }
                        next.run(request).await
                    }
                },
            ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            _dir: dir,
            url,
            task,
            discovery_hidden,
            operation_faults,
        }
    }
}

#[tokio::test]
async fn liveness_oversized_text_does_not_wedge_ordinary_uploads() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let oversized = device.capture(&"x".repeat(SERVER_TEXT_LIMIT + 1));
    device.capture("ordinary after oversized");
    device.exchange().await;
    assert_eq!(
        server.entries().await.total,
        1,
        "a rejected entry must not stop unrelated uploads"
    );
    let server_id = server.info().await.server_id;
    let pending = device.storage.pending_mutations(&server_id).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].local_id, oversized.id);
    let frozen = serde_json::to_vec(&pending[0].request).unwrap();
    let device = device.restart();
    assert!(
        device.storage.sync_warning(&server_id).unwrap().is_some(),
        "rejection status must survive restart"
    );
    device.exchange().await;
    let pending = device.storage.pending_mutations(&server_id).unwrap();
    assert_eq!(serde_json::to_vec(&pending[0].request).unwrap(), frozen);
}

#[tokio::test]
async fn liveness_rejection_backlog_cannot_starve_healthy_frozen_work() {
    const FAILED_BACKLOG: usize = 55;
    let server = Server::start().await;
    let device = Device::new(&server.url);
    for index in 0..FAILED_BACKLOG {
        let text = format!("rejected backlog {index}");
        device.capture(&text);
        let frozen = device.freeze_create(&server, &text).await;
        server.fault(
            &frozen.operation_id,
            OperationFault::Reject(StatusCode::PAYLOAD_TOO_LARGE),
        );
    }
    device.capture("healthy after rejection backlog");
    for _ in 0..3 {
        device.exchange().await;
    }
    assert_eq!(
        server.entries().await.total,
        1,
        "failure selection must rotate past a full batch"
    );
    let server_id = server.info().await.server_id;
    assert!(device.storage.sync_warning(&server_id).unwrap().is_some());
}

#[tokio::test]
async fn liveness_missing_candidate_blob_does_not_abort_ordinary_capture() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let bytes = b"candidate image";
    let hash = copywraith_core::content::hash_bytes(bytes);
    let local = device
        .storage
        .insert_entry(
            ContentType::Image,
            &ClipboardFlavors::default(),
            Some(bytes),
            &hash,
            None,
        )
        .unwrap()
        .unwrap();
    std::fs::remove_file(device._dir.path().join("blobs").join(&hash)).unwrap();
    device.capture("healthy after missing image");
    device.exchange().await;
    assert_eq!(server.entries().await.total, 1);
    assert!(device
        .storage
        .get_unsynced_entries()
        .unwrap()
        .iter()
        .any(|entry| entry.id == local.id));
    assert!(device
        .storage
        .sync_warning(&server.info().await.server_id)
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn liveness_late_live_conflict_projects_consumed_canonical_star() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let local = device.capture("late live conflict");
    let frozen = device.freeze_create(&server, "late live conflict").await;
    let mut competing = server.create_request("late live conflict").await;
    if let SyncAction::Create { payload, .. } = &mut competing.action {
        payload.starred = Some(true);
    }
    let canonical = server.apply(&competing).await;
    let conflict = server.apply(&frozen).await;
    assert_eq!(conflict.outcome, SyncOutcome::Conflict);
    device.sync.pull_new_entries(&device.storage).await.unwrap();
    let pending = device
        .storage
        .pending_mutations(&frozen.server_id)
        .unwrap()
        .remove(0);
    device
        .storage
        .acknowledge_mutation(&pending, &conflict)
        .unwrap();
    let device = device.restart();
    device.exchange().await;
    assert!(
        device.entries()[0].starred,
        "a rejected capture default must yield to canonical stars"
    );
    assert_eq!(device.entries()[0].id, local.id);
    assert!(device
        .storage
        .sync_warning(&frozen.server_id)
        .unwrap()
        .is_none());
    assert_eq!(
        server.entries().await.entries[0].entry.id,
        canonical.generation.unwrap().id
    );
    assert!(server.entries().await.entries[0].entry.starred);
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct Device {
    _dir: tempfile::TempDir,
    storage: LocalStorage,
    sync: SyncClient,
}

impl Device {
    fn restart(self) -> Self {
        let Self {
            _dir,
            storage,
            sync,
        } = self;
        drop(sync);
        drop(storage);
        let storage = LocalStorage::new(_dir.path()).unwrap();
        let sync = SyncClient::new(&storage);
        Self {
            _dir,
            storage,
            sync,
        }
    }

    async fn freeze_create(&self, server: &Server, text: &str) -> SyncMutation {
        let create = server.create_request(text).await;
        let server_id = create.server_id;
        self.storage.bind_sync_server("test", &server_id).unwrap();
        let candidate = self.storage.sync_candidates(&server_id).unwrap().remove(0);
        self.storage
            .enqueue_sync_candidate(&server_id, &candidate, create.action)
            .unwrap();
        let db = rusqlite::Connection::open(self._dir.path().join("copywraith.db")).unwrap();
        let request: String = db
            .query_row(
                "SELECT request FROM sync_outbox WHERE server_id = ?1 AND local_id = ?2",
                rusqlite::params![server_id, candidate.entry.id],
                |row| row.get(0),
            )
            .unwrap();
        serde_json::from_str(&request).unwrap()
    }

    fn new(url: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalStorage::new(dir.path()).unwrap();
        storage
            .save_settings(&models::Settings {
                server_url_primary: url.into(),
                api_key: PASSWORD.into(),
                ..Default::default()
            })
            .unwrap();
        let sync = SyncClient::new(&storage);
        Self {
            _dir: dir,
            storage,
            sync,
        }
    }

    fn capture(&self, text: &str) -> ClipboardEntry {
        let flavors = ClipboardFlavors {
            text_plain: Some(text.into()),
            ..Default::default()
        };
        let hash = flavors.payload_hash(ContentType::Text, None);
        self.storage
            .insert_entry(ContentType::Text, &flavors, None, &hash, None)
            .unwrap()
            .unwrap()
    }

    async fn exchange(&self) {
        self.sync.sync_unsynced_entries(&self.storage).await;
        let result = self.sync.pull_new_entries(&self.storage).await.unwrap();
        assert_eq!(
            result.endpoint_status.state, "online",
            "{:?}",
            result.endpoint_status
        );
    }

    fn entries(&self) -> Vec<ClipboardEntry> {
        self.storage.get_entries(100, 0, false, None).unwrap()
    }
}

#[tokio::test]
async fn delete_reaches_independent_local_ids_and_survives_full_pull() {
    let server = Server::start().await;
    let a = Device::new(&server.url);
    let b = Device::new(&server.url);
    let local_a = a.capture("shared content");
    let local_b = b.capture("shared content");
    assert_ne!(local_a.id, local_b.id);
    a.exchange().await;
    b.exchange().await;
    assert_eq!(a.entries()[0].id, local_a.id);
    assert_eq!(b.entries()[0].id, local_b.id);

    a.storage.delete_entry(&local_a.id).unwrap();
    a.exchange().await;
    b.exchange().await;
    assert!(
        b.entries().is_empty(),
        "a deletion must reach the other device's local ID"
    );
    a.sync.reset_pull_cursor(&a.storage);
    a.exchange().await;
    assert!(
        a.entries().is_empty(),
        "resetting the cursor must not undo deletion"
    );
}

#[tokio::test]
async fn offline_peer_sees_recopy_without_reviving_the_deleted_generation() {
    let server = Server::start().await;
    let a = Device::new(&server.url);
    let b = Device::new(&server.url);
    let first = a.capture("copy again");
    a.exchange().await;
    b.exchange().await;
    a.storage.delete_entry(&first.id).unwrap();
    a.exchange().await;
    let second = a.capture("copy again");
    a.exchange().await;
    b.exchange().await;
    assert_eq!(b.entries().len(), 1);
    assert_ne!(b.entries()[0].id, first.id);
    assert_eq!(a.entries()[0].id, second.id);
}

impl Server {
    async fn info(&self) -> SyncInfo {
        reqwest::Client::new()
            .get(format!("{}/api/sync", self.url))
            .bearer_auth(PASSWORD)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap()
    }

    async fn create_request(&self, text: &str) -> SyncMutation {
        let info = self.info().await;
        let flavors = ClipboardFlavors {
            text_plain: Some(text.into()),
            ..Default::default()
        };
        let content_hash = flavors.payload_hash(ContentType::Text, None);
        let head: SyncHead = reqwest::Client::new()
            .get(format!(
                "{}/api/sync/{}/heads/{content_hash}",
                self.url, info.server_id
            ))
            .bearer_auth(PASSWORD)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        SyncMutation {
            server_id: info.server_id,
            operation_id: ulid::Ulid::generate().to_string(),
            action: SyncAction::Create {
                expected: head.generation,
                payload: CreateEntryRequest {
                    content_type: ContentType::Text,
                    text_content: Some(text.into()),
                    flavors: Some(flavors),
                    blob_base64: None,
                    source_app: None,
                    starred: Some(false),
                    content_hash,
                },
            },
        }
    }

    async fn apply(&self, request: &SyncMutation) -> SyncReceipt {
        reqwest::Client::new()
            .post(format!(
                "{}/api/sync/{}/operations",
                self.url, request.server_id
            ))
            .bearer_auth(PASSWORD)
            .json(request)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap()
    }

    async fn delete_generation(&self, id: &str) {
        let status = reqwest::Client::new()
            .delete(format!("{}/api/entries/{id}", self.url))
            .bearer_auth(PASSWORD)
            .send()
            .await
            .unwrap()
            .status();
        assert_eq!(status, StatusCode::NO_CONTENT);
    }

    async fn entries(&self) -> ListEntriesResponse {
        reqwest::Client::new()
            .get(format!("{}/api/entries", self.url))
            .bearer_auth(PASSWORD)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn lost_create_response_cannot_resurrect_deleted_content() {
    let server = Server::start().await;
    let create = server.create_request("lost response").await;
    let first = server.apply(&create).await;
    let id = first.generation.unwrap().id;
    server.delete_generation(&id).await;
    let replay = server.apply(&create).await;
    assert_eq!(replay.outcome, SyncOutcome::Applied);
    assert_eq!(replay.generation.unwrap().id, id);
    assert_eq!(server.entries().await.total, 0);
}

#[tokio::test]
async fn missing_generation_delete_does_not_block_unrelated_captures() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let server_id = server.info().await.server_id;
    device.storage.bind_sync_server("test", &server_id).unwrap();
    let obsolete = device.capture("obsolete generation");
    let candidate = device
        .storage
        .sync_candidates(&server_id)
        .unwrap()
        .remove(0);

    // Exercise an absent target through the real outbox and HTTP receipt replay.
    device
        .storage
        .enqueue_sync_candidate(
            &server_id,
            &candidate,
            SyncAction::Delete {
                target: DeleteTarget::Generation {
                    id: ulid::Ulid::generate().to_string(),
                },
            },
        )
        .unwrap();
    device.storage.delete_entry(&obsolete.id).unwrap();
    let pending = device
        .storage
        .pending_mutations(&server_id)
        .unwrap()
        .remove(0);
    assert_eq!(
        server.apply(&pending.request).await.outcome,
        SyncOutcome::Missing
    );
    let fresh = device.capture("unrelated capture");
    let device = device.restart();
    device.exchange().await;

    assert!(device
        .storage
        .pending_mutations(&server_id)
        .unwrap()
        .is_empty());
    assert_eq!(device.entries()[0].id, fresh.id);
    let entries = server.entries().await;
    assert_eq!(entries.total, 1);
    assert_eq!(
        entries.entries[0].entry.flavors.text_plain.as_deref(),
        Some("unrelated capture")
    );
}

#[tokio::test]
async fn missing_cancel_receipt_cannot_discard_a_pending_fence() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    const TEXT: &str = "cancel before delivery";
    let entry = device.capture(TEXT);
    let create = device.freeze_create(&server, TEXT).await;
    device.storage.delete_entry(&entry.id).unwrap();
    let pending = device
        .storage
        .pending_mutations(&create.server_id)
        .unwrap()
        .remove(0);
    let mut receipt = server.apply(&pending.request).await;

    // A nonconforming cancellation response must leave its durable intent intact.
    receipt.outcome = SyncOutcome::Missing;
    assert!(device
        .storage
        .acknowledge_mutation(&pending, &receipt)
        .is_err());
    assert_eq!(
        device
            .storage
            .pending_mutations(&create.server_id)
            .unwrap()
            .len(),
        1
    );
    device.exchange().await;
    assert!(device
        .storage
        .pending_mutations(&create.server_id)
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn cancellation_fences_a_create_that_has_not_arrived() {
    let server = Server::start().await;
    let create = server.create_request("delayed request").await;
    let cancel = SyncMutation {
        server_id: create.server_id.clone(),
        operation_id: ulid::Ulid::generate().to_string(),
        action: SyncAction::Delete {
            target: DeleteTarget::Create {
                operation_id: create.operation_id.clone(),
            },
        },
    };
    assert_eq!(server.apply(&cancel).await.outcome, SyncOutcome::Applied);
    assert_eq!(server.apply(&create).await.outcome, SyncOutcome::Cancelled);
    assert_eq!(server.entries().await.total, 0);
}

#[tokio::test]
async fn a_delayed_first_delivery_cannot_rebase_onto_a_new_generation() {
    let server = Server::start().await;
    let stale = server.create_request("same payload").await;
    let first = server.create_request("same payload").await;
    let first_id = server.apply(&first).await.generation.unwrap().id;
    server.delete_generation(&first_id).await;
    assert_eq!(server.apply(&stale).await.outcome, SyncOutcome::Conflict);
    assert_eq!(server.entries().await.total, 0);
    let second = server.create_request("same payload").await;
    let second_id = server.apply(&second).await.generation.unwrap().id;
    assert_ne!(first_id, second_id);
    let old_delete = SyncMutation {
        server_id: first.server_id,
        operation_id: ulid::Ulid::generate().to_string(),
        action: SyncAction::Delete {
            target: DeleteTarget::Generation { id: first_id },
        },
    };
    assert_eq!(
        server.apply(&old_delete).await.outcome,
        SyncOutcome::Applied
    );
    assert_eq!(server.entries().await.entries[0].entry.id, second_id);
}

#[tokio::test]
async fn legacy_client_cannot_ambiguously_recreate_a_deleted_hash() {
    let server = Server::start().await;
    let create = server.create_request("legacy retry").await;
    let id = server.apply(&create).await.generation.unwrap().id;
    server.delete_generation(&id).await;
    let SyncAction::Create { payload, .. } = create.action else {
        unreachable!()
    };
    let response = reqwest::Client::new()
        .post(format!("{}/api/entries", server.url))
        .bearer_auth(PASSWORD)
        .json(&payload)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(server.entries().await.total, 0);
}

#[tokio::test]
async fn different_fallback_servers_are_rejected_before_upload() {
    let first = Server::start().await;
    let second = Server::start().await;
    let device = Device::new(&first.url);
    let mut settings = device.storage.get_settings();
    settings.server_url_fallback = second.url.clone();
    device.storage.save_settings(&settings).unwrap();
    device.capture("must not cross server identity");
    device.sync.sync_unsynced_entries(&device.storage).await;
    assert!(device.sync.pull_new_entries(&device.storage).await.is_err());
    assert_eq!(first.entries().await.total, 0);
    assert_eq!(second.entries().await.total, 0);
}

#[tokio::test]
async fn push_side_pulls_still_notify_the_ui() {
    let server = Server::start().await;
    let a = Device::new(&server.url);
    let b = Device::new(&server.url);
    a.capture("remote update");
    a.exchange().await;
    b.sync.sync_unsynced_entries(&b.storage).await;
    let result = b.sync.pull_new_entries(&b.storage).await.unwrap();
    assert_eq!(
        result.pulled, 1,
        "a push-side pull must not consume the UI notification"
    );
}

#[tokio::test]
async fn lost_star_ack_preserves_a_newer_canonical_star() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let local = device.capture("star race");
    device.exchange().await;
    let server_id = server.info().await.server_id;
    device.storage.toggle_star(&local.id).unwrap();
    let candidate = device
        .storage
        .sync_candidates(&server_id)
        .unwrap()
        .remove(0);
    let remote_id = candidate.remote_id.clone().unwrap();
    device
        .storage
        .enqueue_sync_candidate(
            &server_id,
            &candidate,
            SyncAction::Star {
                generation_id: remote_id.clone(),
                starred: true,
            },
        )
        .unwrap();
    let pending = device
        .storage
        .pending_mutations(&server_id)
        .unwrap()
        .remove(0);
    let receipt = server.apply(&pending.request).await;
    // The upload committed, but its receipt has not reached local storage.
    let status = reqwest::Client::new()
        .patch(format!("{}/api/entries/{remote_id}", server.url))
        .bearer_auth(PASSWORD)
        .json(&serde_json::json!({"starred": false}))
        .send()
        .await
        .unwrap()
        .status();
    assert_eq!(status, StatusCode::OK);
    device.sync.pull_new_entries(&device.storage).await.unwrap();
    assert!(
        device.entries()[0].starred,
        "pending local intent remains visible"
    );
    device
        .storage
        .acknowledge_mutation(&pending, &receipt)
        .unwrap();
    device.sync.pull_new_entries(&device.storage).await.unwrap();
    assert!(
        !device.entries()[0].starred,
        "acknowledgment must project the newer canonical state"
    );
}

#[tokio::test]
async fn tombstone_before_create_receipt_is_not_forgotten() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let local = device.capture("unacknowledged capture");
    let create = server.create_request("unacknowledged capture").await;
    let server_id = create.server_id.clone();
    device.storage.bind_sync_server("test", &server_id).unwrap();
    let candidate = device
        .storage
        .sync_candidates(&server_id)
        .unwrap()
        .remove(0);
    device
        .storage
        .enqueue_sync_candidate(&server_id, &candidate, create.action)
        .unwrap();
    let pending = device
        .storage
        .pending_mutations(&server_id)
        .unwrap()
        .remove(0);
    let receipt = server.apply(&pending.request).await;
    server
        .delete_generation(&receipt.generation.as_ref().unwrap().id)
        .await;
    device.sync.pull_new_entries(&device.storage).await.unwrap();
    device
        .storage
        .acknowledge_mutation(&pending, &receipt)
        .unwrap();
    device.sync.pull_new_entries(&device.storage).await.unwrap();
    assert!(
        device.storage.get_entry(&local.id).unwrap().is_none(),
        "the earlier tombstone must apply when the delayed receipt reveals its identity"
    );
}

#[tokio::test]
async fn established_protocol_never_downgrades_to_unfenced_legacy_posts() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    device.capture("first");
    device.exchange().await;
    server.discovery_hidden.store(true, Ordering::Relaxed);
    device.capture("must remain pending");
    device.sync.sync_unsynced_entries(&device.storage).await;
    assert_eq!(
        server.entries().await.total,
        1,
        "a discovery 404 must not enable unfenced legacy POSTs"
    );
    assert!(device.sync.pull_new_entries(&device.storage).await.is_err());
}

#[tokio::test]
async fn cancelling_a_star_operation_cannot_delete_its_generation() {
    let server = Server::start().await;
    let create = server.create_request("not a create receipt").await;
    let id = server.apply(&create).await.generation.unwrap().id;
    let star = SyncMutation {
        server_id: create.server_id.clone(),
        operation_id: ulid::Ulid::generate().to_string(),
        action: SyncAction::Star {
            generation_id: id,
            starred: true,
        },
    };
    server.apply(&star).await;
    let cancel = SyncMutation {
        server_id: create.server_id,
        operation_id: ulid::Ulid::generate().to_string(),
        action: SyncAction::Delete {
            target: DeleteTarget::Create {
                operation_id: star.operation_id,
            },
        },
    };
    let response = reqwest::Client::new()
        .post(format!(
            "{}/api/sync/{}/operations",
            server.url, cancel.server_id
        ))
        .bearer_auth(PASSWORD)
        .json(&cancel)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(server.entries().await.total, 1);
}

#[tokio::test]
async fn provenance_cancelled_create_cannot_delete_a_recopy_pulled_before_cancellation() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let first = device.capture("receiptless predecessor");
    let create = device
        .freeze_create(&server, "receiptless predecessor")
        .await;
    let receipt = server.apply(&create).await;
    // The server committed, but the client never received the create receipt.
    device.storage.delete_entry(&first.id).unwrap();
    let replacement = device.capture("receiptless predecessor");
    let device = device.restart();
    device.sync.pull_new_entries(&device.storage).await.unwrap();
    assert_eq!(device.entries()[0].id, replacement.id);
    device.exchange().await;
    assert_eq!(
        device.entries().len(),
        1,
        "cancelling the predecessor must preserve the re-copy"
    );
    assert_eq!(device.entries()[0].id, replacement.id);
    assert_ne!(
        server.entries().await.entries[0].entry.id,
        receipt.generation.unwrap().id
    );
}

#[tokio::test]
async fn provenance_never_prepared_capture_cannot_restore_a_later_deletion() {
    let server = Server::start().await;
    let offline = Device::new(&server.url);
    let stale = offline.capture("offline before deletion");
    let online = Device::new(&server.url);
    let original = online.capture("offline before deletion");
    online.exchange().await;
    online.storage.delete_entry(&original.id).unwrap();
    online.exchange().await;
    let offline = offline.restart();
    offline.exchange().await;
    assert_eq!(
        server.entries().await.total,
        0,
        "sync-time discovery must not authorize restoration"
    );
    assert_eq!(offline.entries()[0].id, stale.id);
    assert!(offline
        .storage
        .sync_warning(&server.info().await.server_id)
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn provenance_fresh_duplicate_capture_recovers_a_conflict_without_replacing_its_key() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let local = device.capture("blocked duplicate");
    let frozen = device.freeze_create(&server, "blocked duplicate").await;
    let competing = server.create_request("blocked duplicate").await;
    let generation = server.apply(&competing).await.generation.unwrap();
    server.delete_generation(&generation.id).await;
    let conflict = server.apply(&frozen).await;
    assert_eq!(conflict.outcome, SyncOutcome::Conflict);
    let pending = device
        .storage
        .pending_mutations(&frozen.server_id)
        .unwrap()
        .remove(0);
    device
        .storage
        .acknowledge_mutation(&pending, &conflict)
        .unwrap();
    device.sync.pull_new_entries(&device.storage).await.unwrap();
    assert!(device
        .storage
        .sync_warning(&frozen.server_id)
        .unwrap()
        .is_some());
    // A new explicit capture follows observation of the tombstone; it is not a retry.
    let flavors = ClipboardFlavors {
        text_plain: Some("blocked duplicate".into()),
        ..Default::default()
    };
    device
        .storage
        .insert_entry(
            ContentType::Text,
            &flavors,
            None,
            &flavors.payload_hash(ContentType::Text, None),
            None,
        )
        .unwrap();
    let device = device.restart();
    device.exchange().await;
    assert_eq!(
        server.entries().await.total,
        1,
        "a fresh duplicate must recover blocked capture intent"
    );
    assert_eq!(device.entries()[0].id, local.id);
    assert!(device
        .storage
        .sync_warning(&frozen.server_id)
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn provenance_recopy_survives_cancellation_before_the_original_post_arrives() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let original = device.capture("cancel before delivery");
    let frozen = device
        .freeze_create(&server, "cancel before delivery")
        .await;
    let device = device.restart();
    let retried = device
        .storage
        .pending_mutations(&frozen.server_id)
        .unwrap()
        .remove(0)
        .request;
    assert_eq!(
        serde_json::to_value(&frozen).unwrap(),
        serde_json::to_value(&retried).unwrap()
    );
    device.storage.delete_entry(&original.id).unwrap();
    let replacement = device.capture("cancel before delivery");
    device.sync.pull_new_entries(&device.storage).await.unwrap();
    device.exchange().await;
    assert_eq!(server.apply(&frozen).await.outcome, SyncOutcome::Cancelled);
    assert_eq!(device.entries()[0].id, replacement.id);
    assert_eq!(server.entries().await.total, 1);
}

#[tokio::test]
async fn provenance_deferred_new_generation_is_replayed_after_predecessor_resolution() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let original = device.capture("deferred newer generation");
    let frozen = device
        .freeze_create(&server, "deferred newer generation")
        .await;
    let first = server.apply(&frozen).await.generation.unwrap();
    device.storage.delete_entry(&original.id).unwrap();
    let replacement = device.capture("deferred newer generation");
    server.delete_generation(&first.id).await;
    let competing = server.create_request("deferred newer generation").await;
    let second = server.apply(&competing).await.generation.unwrap();
    device.sync.pull_new_entries(&device.storage).await.unwrap();
    let device = device.restart();
    device.exchange().await;
    assert_eq!(device.entries()[0].id, replacement.id);
    assert_eq!(server.entries().await.total, 1);
    assert_eq!(server.entries().await.entries[0].entry.id, second.id);
    assert!(device
        .storage
        .pending_mutations(&frozen.server_id)
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn provenance_observed_tombstone_does_not_authorize_a_newer_generation() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let first = server.create_request("generation-scoped authority").await;
    let first_id = server.apply(&first).await.generation.unwrap().id;
    server.delete_generation(&first_id).await;
    device.sync.pull_new_entries(&device.storage).await.unwrap();
    let local = device.capture("generation-scoped authority");
    let second = server.create_request("generation-scoped authority").await;
    let second_id = server.apply(&second).await.generation.unwrap().id;
    server.delete_generation(&second_id).await;
    device.exchange().await;
    assert_eq!(server.entries().await.total, 0);
    assert_eq!(device.entries()[0].id, local.id);
    assert!(device
        .storage
        .sync_warning(&first.server_id)
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn provenance_upgrade_with_a_discarded_create_hash_keeps_the_recopy_unresolved() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let original = device.capture("old build discarded provenance");
    let frozen = device
        .freeze_create(&server, "old build discarded provenance")
        .await;
    server.apply(&frozen).await;
    device.storage.delete_entry(&original.id).unwrap();
    let replacement = device.capture("old build discarded provenance");
    // The previous build retained only cancel-create's operation ID, not its hash.
    let db = rusqlite::Connection::open(device._dir.path().join("copywraith.db")).unwrap();
    db.execute_batch("DROP TABLE sync_capture_predecessors; DROP TABLE sync_capture_heads; DROP TABLE sync_operation_provenance;").unwrap();
    drop(db);
    let device = device.restart();
    device.sync.pull_new_entries(&device.storage).await.unwrap();
    device.exchange().await;
    assert_eq!(
        device.entries().len(),
        1,
        "missing historical provenance must not delete the new local copy"
    );
    assert_eq!(device.entries()[0].id, replacement.id);
    assert_eq!(server.entries().await.total, 0);
    assert!(device
        .storage
        .sync_warning(&frozen.server_id)
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn provenance_old_delete_receipt_cannot_replace_the_capture_time_known_head() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let original = device.capture("latest observed head");
    device.exchange().await;
    let first_id = server.entries().await.entries[0].entry.id.clone();
    device.storage.delete_entry(&original.id).unwrap();
    server.delete_generation(&first_id).await;
    let second = server.create_request("latest observed head").await;
    let second_id = server.apply(&second).await.generation.unwrap().id;
    server.delete_generation(&second_id).await;
    device.sync.pull_new_entries(&device.storage).await.unwrap();
    // A no-op delete receipt carries the server clock, not a new change to the old generation.
    device.exchange().await;
    let replacement = device.capture("latest observed head");
    device.exchange().await;
    assert_eq!(
        server.entries().await.total,
        1,
        "the new capture observed the newer tombstone"
    );
    assert_eq!(device.entries()[0].id, replacement.id);
}

#[tokio::test]
async fn liveness_conflict_before_feed_preserves_explicit_star_intent() {
    for explicit_edit in [false, true] {
        let server = Server::start().await;
        let device = Device::new(&server.url);
        let local = device.capture("conflict before feed");
        let frozen = device.freeze_create(&server, "conflict before feed").await;
        let mut competing = server.create_request("conflict before feed").await;
        if let SyncAction::Create { payload, .. } = &mut competing.action {
            payload.starred = Some(true);
        }
        server.apply(&competing).await;
        let conflict = server.apply(&frozen).await;
        if explicit_edit {
            device.storage.toggle_star(&local.id).unwrap();
            device.storage.toggle_star(&local.id).unwrap();
        }
        let pending = device
            .storage
            .pending_mutations(&frozen.server_id)
            .unwrap()
            .remove(0);
        device
            .storage
            .acknowledge_mutation(&pending, &conflict)
            .unwrap();
        let device = device.restart();
        device.exchange().await;
        assert_eq!(device.entries()[0].starred, !explicit_edit);
        assert_eq!(device.entries()[0].id, local.id);
        assert_eq!(
            server.entries().await.entries[0].entry.starred,
            !explicit_edit
        );
        assert!(device
            .storage
            .sync_warning(&frozen.server_id)
            .unwrap()
            .is_none());
    }
}

#[tokio::test]
async fn liveness_late_conflict_obeys_later_deletion_and_recreation() {
    for recreate in [false, true] {
        let server = Server::start().await;
        let device = Device::new(&server.url);
        let local = device.capture("late conflict retired");
        let frozen = device.freeze_create(&server, "late conflict retired").await;
        let competing = server.create_request("late conflict retired").await;
        let generation = server.apply(&competing).await.generation.unwrap();
        let conflict = server.apply(&frozen).await;
        server.delete_generation(&generation.id).await;
        if recreate {
            let mut replacement = server.create_request("late conflict retired").await;
            if let SyncAction::Create { payload, .. } = &mut replacement.action {
                payload.starred = Some(true);
            }
            server.apply(&replacement).await;
        }
        device.sync.pull_new_entries(&device.storage).await.unwrap();
        let pending = device
            .storage
            .pending_mutations(&frozen.server_id)
            .unwrap()
            .remove(0);
        device
            .storage
            .acknowledge_mutation(&pending, &conflict)
            .unwrap();
        let device = device.restart();
        device.exchange().await;
        assert_eq!(device.entries().len(), usize::from(recreate));
        if recreate {
            assert_eq!(device.entries()[0].id, local.id);
            assert!(device.entries()[0].starred);
        }
        assert_eq!(server.entries().await.total, usize::from(recreate) as u64);
        assert!(device
            .storage
            .sync_warning(&frozen.server_id)
            .unwrap()
            .is_none());
    }
}

#[tokio::test]
async fn liveness_later_rejection_cannot_erase_ambiguous_delivery_or_cancel_fences() {
    for rejection in [StatusCode::BAD_REQUEST, StatusCode::PAYLOAD_TOO_LARGE] {
        let server = Server::start().await;
        let device = Device::new(&server.url);
        let original = device.capture("ambiguous then rejected");
        let frozen = device
            .freeze_create(&server, "ambiguous then rejected")
            .await;
        server.fault(
            &frozen.operation_id,
            OperationFault::CommitThenReject(StatusCode::BAD_GATEWAY),
        );
        device.exchange().await;
        assert_eq!(server.entries().await.total, 1);
        server.fault(&frozen.operation_id, OperationFault::Reject(rejection));
        device.exchange().await;
        let device = device.restart();
        let pending = device
            .storage
            .pending_mutations(&frozen.server_id)
            .unwrap()
            .remove(0);
        assert_eq!(
            serde_json::to_vec(&pending.request).unwrap(),
            serde_json::to_vec(&frozen).unwrap()
        );
        assert!(device
            .storage
            .sync_warning(&frozen.server_id)
            .unwrap()
            .is_some());
        device.storage.delete_entry(&original.id).unwrap();
        let replacement = device.capture("ambiguous then rejected");
        let cancellations = device.storage.pending_mutations(&frozen.server_id).unwrap();
        for cancellation in &cancellations {
            server.fault(
                &cancellation.request.operation_id,
                OperationFault::Reject(rejection),
            );
        }
        device.exchange().await;
        assert_eq!(device.entries()[0].id, replacement.id);
        assert_eq!(server.entries().await.total, 1);
        assert_eq!(
            device
                .storage
                .pending_mutations(&frozen.server_id)
                .unwrap()
                .len(),
            cancellations.len()
        );
        server.clear_faults();
        device
            .storage
            .retry_sync_failures(&frozen.server_id)
            .unwrap();
        let device = device.restart();
        device.exchange().await;
        assert_eq!(device.entries()[0].id, replacement.id);
        assert_eq!(server.entries().await.total, 1);
        // Replay returns the original receipt, even after its generation was retired.
        assert_ne!(
            server.apply(&frozen).await.generation.unwrap().id,
            server.entries().await.entries[0].entry.id
        );
        assert!(device
            .storage
            .sync_warning(&frozen.server_id)
            .unwrap()
            .is_none());
    }
}

#[tokio::test]
async fn liveness_manual_retry_preserves_the_complete_request() {
    for rejection in [StatusCode::BAD_REQUEST, StatusCode::PAYLOAD_TOO_LARGE] {
        let server = Server::start().await;
        let device = Device::new(&server.url);
        device.capture("manual recovery");
        let frozen = device.freeze_create(&server, "manual recovery").await;
        server.fault(&frozen.operation_id, OperationFault::Reject(rejection));
        device.exchange().await;
        let device = device.restart();
        device.exchange().await;
        assert_eq!(
            server.operation_faults.lock().unwrap().attempts[&frozen.operation_id].len(),
            1
        );
        server.clear_faults();
        device
            .storage
            .retry_sync_failures(&frozen.server_id)
            .unwrap();
        device.exchange().await;
        assert_eq!(server.entries().await.total, 1);
        assert!(device
            .storage
            .sync_warning(&frozen.server_id)
            .unwrap()
            .is_none());
        let faults = server.operation_faults.lock().unwrap();
        let attempts = &faults.attempts[&frozen.operation_id];
        assert_eq!(attempts.len(), 2);
        assert!(attempts
            .iter()
            .all(|fingerprint| fingerprint == &attempts[0]));
    }
}

#[tokio::test]
async fn liveness_auth_failure_stops_the_session_without_quarantining_operations() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    device.capture("authorization failure");
    let frozen = device.freeze_create(&server, "authorization failure").await;
    server.fault(
        &frozen.operation_id,
        OperationFault::Reject(StatusCode::UNAUTHORIZED),
    );
    device.capture("waiting for authorization");
    device.sync.sync_unsynced_entries(&device.storage).await;
    assert_eq!(server.entries().await.total, 0);
    let device = device.restart();
    assert!(device
        .storage
        .sync_warning(&frozen.server_id)
        .unwrap()
        .is_some());
    assert_eq!(
        device
            .storage
            .pending_mutations(&frozen.server_id)
            .unwrap()
            .len(),
        1
    );
    server.clear_faults();
    device.exchange().await;
    assert_eq!(server.entries().await.total, 2);
    assert!(device
        .storage
        .sync_warning(&frozen.server_id)
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn liveness_operation_reuse_reports_corruption_without_blocking_other_work() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    device.capture("operation reuse");
    let frozen = device.freeze_create(&server, "operation reuse").await;
    let mut changed: SyncMutation =
        serde_json::from_value(serde_json::to_value(&frozen).unwrap()).unwrap();
    if let SyncAction::Create { payload, .. } = &mut changed.action {
        payload.starred = Some(true);
    }
    server.apply(&changed).await;
    device.capture("healthy despite corruption");
    device.exchange().await;
    assert_eq!(server.entries().await.total, 2);
    let device = device.restart();
    assert!(device
        .storage
        .sync_warning(&frozen.server_id)
        .unwrap()
        .unwrap()
        .contains("Operation ID reused"));
    let pending = device
        .storage
        .pending_mutations(&frozen.server_id)
        .unwrap()
        .remove(0);
    assert_eq!(
        serde_json::to_vec(&pending.request).unwrap(),
        serde_json::to_vec(&frozen).unwrap()
    );
}

#[tokio::test]
async fn liveness_transient_backlog_rotates_without_retargeting() {
    const FAILED_BACKLOG: usize = 55;
    let server = Server::start().await;
    let device = Device::new(&server.url);
    for index in 0..FAILED_BACKLOG {
        let text = format!("transient backlog {index}");
        device.capture(&text);
        let frozen = device.freeze_create(&server, &text).await;
        server.fault(
            &frozen.operation_id,
            OperationFault::Reject(StatusCode::INTERNAL_SERVER_ERROR),
        );
    }
    device.capture("healthy after transient backlog");
    for _ in 0..3 {
        device.exchange().await;
    }
    assert_eq!(server.entries().await.total, 1);
    server.clear_faults();
    let device = device.restart();
    for _ in 0..3 {
        device.exchange().await;
    }
    assert_eq!(server.entries().await.total, (FAILED_BACKLOG + 1) as u64);
    assert!(device
        .storage
        .sync_warning(&server.info().await.server_id)
        .unwrap()
        .is_none());
    for attempts in server.operation_faults.lock().unwrap().attempts.values() {
        assert!(attempts
            .iter()
            .all(|fingerprint| fingerprint == &attempts[0]));
    }
}

#[tokio::test]
async fn liveness_timeout_after_commit_replays_the_retained_receipt() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    device.capture("timeout after commit");
    let frozen = device.freeze_create(&server, "timeout after commit").await;
    server.fault(&frozen.operation_id, OperationFault::CommitThenTimeout);
    device.exchange().await;
    let generation = server.entries().await.entries[0].entry.id.clone();
    let device = device.restart();
    assert!(device
        .storage
        .sync_warning(&frozen.server_id)
        .unwrap()
        .is_some());
    server.clear_faults();
    device.exchange().await;
    assert_eq!(server.entries().await.total, 1);
    assert_eq!(server.entries().await.entries[0].entry.id, generation);
    assert!(device
        .storage
        .sync_warning(&frozen.server_id)
        .unwrap()
        .is_none());
    let faults = server.operation_faults.lock().unwrap();
    let attempts = &faults.attempts[&frozen.operation_id];
    assert_eq!(attempts.len(), 2);
    assert_eq!(attempts[0], attempts[1]);
}

#[tokio::test]
async fn liveness_late_conflict_preserves_a_newer_explicit_star_edit() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let local = device.capture("late explicit star");
    let frozen = device.freeze_create(&server, "late explicit star").await;
    let mut competing = server.create_request("late explicit star").await;
    if let SyncAction::Create { payload, .. } = &mut competing.action {
        payload.starred = Some(true);
    }
    server.apply(&competing).await;
    let conflict = server.apply(&frozen).await;
    device.sync.pull_new_entries(&device.storage).await.unwrap();
    device.storage.toggle_star(&local.id).unwrap();
    device.storage.toggle_star(&local.id).unwrap();
    let device = device.restart();
    let pending = device
        .storage
        .pending_mutations(&frozen.server_id)
        .unwrap()
        .remove(0);
    device
        .storage
        .acknowledge_mutation(&pending, &conflict)
        .unwrap();
    device.exchange().await;
    assert_eq!(device.entries()[0].id, local.id);
    assert!(!device.entries()[0].starred);
    assert!(!server.entries().await.entries[0].entry.starred);
    assert!(device
        .storage
        .sync_warning(&frozen.server_id)
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn liveness_missing_blob_recovery_preserves_local_identity() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let bytes = b"repair candidate";
    let hash = copywraith_core::content::hash_bytes(bytes);
    let local = device
        .storage
        .insert_entry(
            ContentType::Image,
            &ClipboardFlavors::default(),
            Some(bytes),
            &hash,
            None,
        )
        .unwrap()
        .unwrap();
    let path = device._dir.path().join("blobs").join(&hash);
    std::fs::remove_file(&path).unwrap();
    device.exchange().await;
    let device = device.restart();
    let server_id = server.info().await.server_id;
    assert!(device.storage.sync_warning(&server_id).unwrap().is_some());
    assert_eq!(device.storage.get_unsynced_entries().unwrap().len(), 1);
    // A later star edit must not make the repaired candidate permanently unretryable.
    device.storage.toggle_star(&local.id).unwrap();
    // Fixture restores the missing source; explicit recovery prepares its first immutable request.
    std::fs::write(path, bytes).unwrap();
    device.storage.retry_sync_failures(&server_id).unwrap();
    device.exchange().await;
    assert_eq!(server.entries().await.total, 1);
    assert_eq!(device.entries()[0].id, local.id);
    assert!(device.storage.get_unsynced_entries().unwrap().is_empty());
    assert!(device.storage.sync_warning(&server_id).unwrap().is_none());
}

#[tokio::test]
async fn liveness_upgrade_preserves_post_freeze_star_on_recovered_capture() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let local = device.capture("recovered pre-upgrade star");
    let first = device
        .freeze_create(&server, "recovered pre-upgrade star")
        .await;
    let competing = server.create_request("recovered pre-upgrade star").await;
    let generation = server.apply(&competing).await.generation.unwrap();
    server.delete_generation(&generation.id).await;
    let conflict = server.apply(&first).await;
    let pending = device
        .storage
        .pending_mutations(&first.server_id)
        .unwrap()
        .remove(0);
    device
        .storage
        .acknowledge_mutation(&pending, &conflict)
        .unwrap();
    device.sync.pull_new_entries(&device.storage).await.unwrap();
    let flavors = local.resolved_flavors();
    device
        .storage
        .insert_entry(
            ContentType::Text,
            &flavors,
            None,
            &flavors.payload_hash(ContentType::Text, None),
            None,
        )
        .unwrap();
    let frozen = device
        .freeze_create(&server, "recovered pre-upgrade star")
        .await;
    let mut competing = server.create_request("recovered pre-upgrade star").await;
    if let SyncAction::Create { payload, .. } = &mut competing.action {
        payload.starred = Some(true);
    }
    server.apply(&competing).await;
    let conflict = server.apply(&frozen).await;
    // The previous build tracked revisions, but had no explicit-star journal.
    let db = rusqlite::Connection::open(device._dir.path().join("copywraith.db")).unwrap();
    db.execute_batch("DROP TRIGGER entries_sync_star_intent; DROP TABLE sync_star_intents;")
        .unwrap();
    drop(db);
    device.storage.toggle_star(&local.id).unwrap();
    device.storage.toggle_star(&local.id).unwrap();
    let device = device.restart();
    device.sync.pull_new_entries(&device.storage).await.unwrap();
    let pending = device
        .storage
        .pending_mutations(&frozen.server_id)
        .unwrap()
        .remove(0);
    device
        .storage
        .acknowledge_mutation(&pending, &conflict)
        .unwrap();
    device.exchange().await;
    assert_eq!(device.entries()[0].id, local.id);
    assert!(!device.entries()[0].starred);
    assert!(!server.entries().await.entries[0].entry.starred);
}

#[path = "support/ingress.rs"]
mod ingress_tests;
