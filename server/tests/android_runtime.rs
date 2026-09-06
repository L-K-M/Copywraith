//! Real authenticated protocol fixture for host cancellation and APK instrumentation.
#![allow(dead_code, unexpected_cfgs)]
#[path = "../src/api.rs"]
mod api;
#[path = "../src/crypto.rs"]
mod crypto;
#[path = "../../src-tauri/src/mobile_core.rs"]
mod mobile_core;
#[path = "../../src-tauri/src/mobile_runtime.rs"]
mod mobile_runtime;
#[path = "../../src-tauri/src/models.rs"]
mod models;
#[path = "../src/storage.rs"]
mod server_storage;
#[path = "../../src-tauri/src/storage.rs"]
mod storage;
#[path = "../../src-tauri/src/sync.rs"]
mod sync;

use copywraith_core::sync_protocol::SyncInfo;
use mobile_core::CoreRegistry;
use mobile_runtime::MobileRuntime;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};

const PASSWORD: &str = "fixture-password";
const MAX_REQUEST_BYTES: usize = 64 * 1024 * 1024;
const PROBE_PORT: u16 = 18763;
const TEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
struct AppState {
    storage: server_storage::Storage,
    crypto: crypto::SharedCryptoState,
}

#[derive(Default)]
struct Evidence {
    operations: AtomicUsize,
    feeds: AtomicUsize,
    uploaded: AtomicBool,
    reply_gate: ReplyGate,
    held_replies: AtomicUsize,
    returned_replies: AtomicUsize,
    committed: tokio::sync::Notify,
    requests: Mutex<Vec<Vec<u8>>>,
    receipts: Mutex<Vec<Vec<u8>>>,
}

struct ReplyGate(tokio::sync::watch::Sender<bool>);

impl Default for ReplyGate {
    fn default() -> Self {
        Self(tokio::sync::watch::channel(false).0)
    }
}

impl Evidence {
    fn snapshot(&self) -> serde_json::Value {
        let requests = self.requests.lock().unwrap();
        let receipts = self.receipts.lock().unwrap();
        serde_json::json!({
            "operations": self.operations.load(Ordering::SeqCst),
            "feeds": self.feeds.load(Ordering::SeqCst),
            "uploaded": self.uploaded.load(Ordering::SeqCst),
            "heldReplies": self.held_replies.load(Ordering::SeqCst),
            "returnedReplies": self.returned_replies.load(Ordering::SeqCst),
            "requests": requests.len(),
            "identicalRequests": requests.len() == 2 && requests[0] == requests[1],
            "identicalReceipts": receipts.len() == 2 && receipts[0] == receipts[1],
        })
    }
}

struct Fixture {
    directory: tempfile::TempDir,
    url: String,
    task: tokio::task::JoinHandle<()>,
    evidence: Arc<Evidence>,
}

impl Fixture {
    async fn start(port: u16) -> Self {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("auth.json"),
            include_bytes!("fixtures/auth.json"),
        )
        .unwrap();
        let state = Arc::new(AppState {
            storage: server_storage::Storage::new(directory.path()).unwrap(),
            crypto: Mutex::new(crypto::CryptoState::load(directory.path()).unwrap()),
        });
        let evidence = Arc::new(Evidence::default());
        let observed = evidence.clone();
        let report = evidence.clone();
        let hold = evidence.clone();
        let release = evidence.clone();
        let app = axum::Router::new()
            .nest("/api", api::router())
            .with_state(state)
            .route(
                "/probe/evidence",
                axum::routing::get(move || {
                    let report = report.clone();
                    async move { axum::Json(report.snapshot()) }
                }),
            )
            // These controls exist only in the loopback test fixture.
            .route(
                "/probe/hold",
                axum::routing::post(move || {
                    hold.reply_gate.0.send_replace(true);
                    async { axum::http::StatusCode::NO_CONTENT }
                }),
            )
            .route(
                "/probe/release",
                axum::routing::post(move || {
                    release.reply_gate.0.send_replace(false);
                    async { axum::http::StatusCode::NO_CONTENT }
                }),
            )
            .layer(axum::middleware::from_fn(
                move |request: axum::extract::Request, next: axum::middleware::Next| {
                    let observed = observed.clone();
                    async move {
                        if request.uri().path().ends_with("/changes") {
                            observed.feeds.fetch_add(1, Ordering::SeqCst);
                        }
                        if !request.uri().path().ends_with("/operations") {
                            return next.run(request).await;
                        }
                        let (parts, body) = request.into_parts();
                        let bytes = axum::body::to_bytes(body, MAX_REQUEST_BYTES).await.unwrap();
                        observed.requests.lock().unwrap().push(bytes.to_vec());
                        let upload =
                            String::from_utf8_lossy(&bytes).contains("android-headless-upload");
                        let response = next
                            .run(axum::extract::Request::from_parts(
                                parts,
                                axum::body::Body::from(bytes),
                            ))
                            .await;
                        assert!(response.status().is_success());
                        let (parts, body) = response.into_parts();
                        let receipt = axum::body::to_bytes(body, MAX_REQUEST_BYTES).await.unwrap();
                        observed.receipts.lock().unwrap().push(receipt.to_vec());
                        observed.operations.fetch_add(1, Ordering::SeqCst);
                        if upload {
                            observed.uploaded.store(true, Ordering::SeqCst);
                        }
                        observed.committed.notify_one();
                        let mut gate = observed.reply_gate.0.subscribe();
                        if *gate.borrow_and_update() {
                            observed.held_replies.fetch_add(1, Ordering::SeqCst);
                            gate.wait_for(|held| !held).await.unwrap();
                        }
                        observed.returned_replies.fetch_add(1, Ordering::SeqCst);
                        axum::response::Response::from_parts(parts, axum::body::Body::from(receipt))
                    }
                },
            ));
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
            .await
            .unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            directory,
            url,
            task,
            evidence,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[tokio::test]
async fn cancelled_job_replays_identical_frozen_request_after_server_commit() {
    let fixture = Fixture::start(0).await;
    let directory = tempfile::tempdir().unwrap();
    let core = CoreRegistry::default().open(directory.path()).unwrap();
    core.prepare_probe(&fixture.url).unwrap();
    control(&fixture, "hold").await;
    let runtime = Arc::new(MobileRuntime::default());
    let worker = core.clone();
    let id = runtime
        .start_job(&tokio::runtime::Handle::current(), async move {
            worker.exchange().await.unwrap();
        })
        .unwrap();
    tokio::time::timeout(TEST_TIMEOUT, fixture.evidence.committed.notified())
        .await
        .unwrap();
    let info: SyncInfo = reqwest::Client::new()
        .get(format!("{}/api/sync", fixture.url))
        .bearer_auth(PASSWORD)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let pending = core.storage().pending_mutations(&info.server_id).unwrap();
    assert_eq!(pending.len(), 1);
    let frozen = serde_json::to_vec(&pending[0].request).unwrap();
    assert_eq!(core.probe_unsynced_count().unwrap(), 1);
    assert_eq!(fixture.evidence.snapshot()["heldReplies"], 1);
    assert_eq!(fixture.evidence.snapshot()["returnedReplies"], 0);
    runtime.stop_job(id);
    tokio::time::timeout(TEST_TIMEOUT, async {
        while runtime.job_id().is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(!runtime.prevents_exit());
    let retained = core.storage().pending_mutations(&info.server_id).unwrap();
    assert_eq!(serde_json::to_vec(&retained[0].request).unwrap(), frozen);
    control(&fixture, "release").await;
    let worker = core.clone();
    runtime
        .start_job(&tokio::runtime::Handle::current(), async move {
            worker.exchange().await.unwrap();
        })
        .unwrap();
    tokio::time::timeout(TEST_TIMEOUT, async {
        while runtime.job_id().is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(core
        .storage()
        .pending_mutations(&info.server_id)
        .unwrap()
        .is_empty());
    let requests = fixture.evidence.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0], requests[1]);
    drop(requests);
    assert_eq!(fixture.evidence.snapshot()["identicalReceipts"], true);
    assert_eq!(core.probe_unsynced_count().unwrap(), 0);
}

async fn control(fixture: &Fixture, action: &str) {
    let response = reqwest::Client::new()
        .post(format!("{}/probe/{action}", fixture.url))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::NO_CONTENT);
}

#[tokio::test]
#[ignore = "Standalone fixture; run only with the APK runner"]
async fn serve_android_probe() {
    let fixture = Fixture::start(PROBE_PORT).await;
    let seed_dir = tempfile::tempdir().unwrap();
    let seed = CoreRegistry::default().open(seed_dir.path()).unwrap();
    seed.storage()
        .save_settings(&models::Settings {
            server_url_primary: fixture.url.clone(),
            api_key: PASSWORD.into(),
            ..Default::default()
        })
        .unwrap();
    seed.capture_probe("android-headless-download").unwrap();
    seed.exchange().await.unwrap();
    fixture.evidence.operations.store(0, Ordering::SeqCst);
    fixture.evidence.feeds.store(0, Ordering::SeqCst);
    fixture.evidence.returned_replies.store(0, Ordering::SeqCst);
    fixture.evidence.requests.lock().unwrap().clear();
    fixture.evidence.receipts.lock().unwrap().clear();
    println!("ANDROID_PROBE_FIXTURE_READY");
    std::future::pending::<()>().await;
}
