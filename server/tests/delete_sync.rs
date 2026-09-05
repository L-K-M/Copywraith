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

use std::sync::{Arc, Mutex};

use copywraith_core::api_types::{CreateEntryRequest, ListEntriesResponse};
use copywraith_core::models::{ClipboardEntry, ClipboardFlavors, ContentType};
use copywraith_core::sync_protocol::*;
use reqwest::StatusCode;
use storage::LocalStorage;
use sync::SyncClient;

const PASSWORD: &str = "fixture-password";

struct AppState {
    storage: server_storage::Storage,
    crypto: crypto::SharedCryptoState,
}

struct Server {
    _dir: tempfile::TempDir,
    url: String,
    task: tokio::task::JoinHandle<()>,
}

impl Server {
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
        let app = axum::Router::new()
            .nest("/api", api::router())
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            _dir: dir,
            url,
            task,
        }
    }
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
