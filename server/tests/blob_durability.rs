// Exercise production stores and protocol commits without the desktop runtime.
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

use copywraith_core::content::{bytes_to_base64, hash_bytes};
use copywraith_core::models::{ClipboardFlavors, ContentType};

const PAYLOAD: &[u8] = b"complete validated blob payload";
const KEY: [u8; 32] = [7; 32];

#[test]
fn truncated_orphan_is_repaired_before_upload_acknowledgement() {
    let dir = tempfile::tempdir().unwrap();
    let db = server_storage::Storage::new(dir.path()).unwrap();
    let hash = hash_bytes(PAYLOAD);
    std::fs::write(dir.path().join("blobs").join(&hash), b"truncated").unwrap();
    db.create_entry(
        ContentType::Image,
        &ClipboardFlavors::default(),
        Some(&bytes_to_base64(PAYLOAD)),
        None,
        None,
        &hash,
        Some(&KEY),
    )
    .unwrap();
    let stored = db.get_blob(&hash).unwrap().unwrap();
    assert_eq!(crypto::decrypt_blob(&KEY, &stored).unwrap(), PAYLOAD);
}

#[test]
fn duplicate_server_row_repairs_corrupt_and_missing_blobs() {
    let dir = tempfile::tempdir().unwrap();
    let db = server_storage::Storage::new(dir.path()).unwrap();
    let hash = hash_bytes(PAYLOAD);
    let upload = bytes_to_base64(PAYLOAD);
    let (original, _) = db
        .create_entry(
            ContentType::Image,
            &ClipboardFlavors::default(),
            Some(&upload),
            None,
            Some(true),
            &hash,
            Some(&KEY),
        )
        .unwrap();
    let path = dir.path().join("blobs").join(&hash);
    for damage in [Some(b"ENCBbad".as_slice()), None] {
        match damage {
            Some(bytes) => std::fs::write(&path, bytes).unwrap(),
            None => std::fs::remove_file(&path).unwrap(),
        }
        let (entry, created) = db
            .create_entry(
                ContentType::Image,
                &ClipboardFlavors::default(),
                Some(&upload),
                None,
                Some(false),
                &hash,
                Some(&KEY),
            )
            .unwrap();
        assert!(!created);
        assert_eq!(entry.id, original.id);
        assert!(entry.starred);
        assert_eq!(
            crypto::decrypt_blob(&KEY, &db.get_blob(&hash).unwrap().unwrap()).unwrap(),
            PAYLOAD
        );
    }
}

#[test]
fn duplicate_local_row_repairs_corrupt_and_missing_blobs() {
    let dir = tempfile::tempdir().unwrap();
    let db = storage::LocalStorage::new(dir.path()).unwrap();
    let hash = hash_bytes(PAYLOAD);
    let entry = db
        .insert_entry(
            ContentType::Image,
            &ClipboardFlavors::default(),
            Some(PAYLOAD),
            &hash,
            None,
        )
        .unwrap()
        .unwrap();
    db.toggle_star(&entry.id).unwrap();
    let path = dir.path().join("blobs").join(&hash);
    for damage in [Some(b"truncated".as_slice()), None] {
        match damage {
            Some(bytes) => std::fs::write(&path, bytes).unwrap(),
            None => std::fs::remove_file(&path).unwrap(),
        }
        assert!(db
            .insert_entry(
                ContentType::Image,
                &ClipboardFlavors::default(),
                Some(PAYLOAD),
                &hash,
                None
            )
            .unwrap()
            .is_none());
        assert!(db.get_entry(&entry.id).unwrap().unwrap().starred);
        assert_eq!(db.get_blob(&hash).unwrap().unwrap(), PAYLOAD);
    }
}

#[cfg(unix)]
const ENCRYPTION_CHILD_DIR: &str = "COPYWRAITH_ENCRYPTION_FAILURE_DIR";
#[cfg(unix)]
const INTERRUPTED_WRITE_LIMIT: libc::rlim_t = 1024;
#[cfg(unix)]
const ENCRYPTION_PAYLOAD_SIZE: usize = 8192;

#[cfg(unix)]
#[test]
fn interrupted_encryption_preserves_the_only_valid_copy() {
    let dir = tempfile::tempdir().unwrap();
    let payload = vec![42; ENCRYPTION_PAYLOAD_SIZE];
    let hash = hash_bytes(&payload);
    let db = server_storage::Storage::new(dir.path()).unwrap();
    db.create_entry(
        ContentType::Image,
        &ClipboardFlavors::default(),
        Some(&bytes_to_base64(&payload)),
        None,
        None,
        &hash,
        None,
    )
    .unwrap();
    drop(db);
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "encryption_write_failure_child", "--nocapture"])
        .env(ENCRYPTION_CHILD_DIR, dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "child: {:?}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let reopened = server_storage::Storage::new(dir.path()).unwrap();
    let stored = reopened.get_blob(&hash).unwrap().unwrap();
    assert_eq!(crypto::decrypt_blob(&KEY, &stored).unwrap(), payload);
    reopened.encrypt_all_blobs(&KEY).unwrap();
    assert_eq!(
        crypto::decrypt_blob(&KEY, &reopened.get_blob(&hash).unwrap().unwrap()).unwrap(),
        payload
    );
}

#[cfg(unix)]
#[test]
fn encryption_write_failure_child() {
    let Some(dir) = std::env::var_os(ENCRYPTION_CHILD_DIR) else {
        return;
    };
    let db = server_storage::Storage::new(std::path::Path::new(&dir)).unwrap();
    // Limit only this child after opening the DB. A real short write interrupts
    // replacement; the parent process and machine limits remain unchanged.
    let limit = libc::rlimit {
        rlim_cur: INTERRUPTED_WRITE_LIMIT,
        rlim_max: INTERRUPTED_WRITE_LIMIT,
    };
    unsafe {
        libc::signal(libc::SIGXFSZ, libc::SIG_IGN);
        assert_eq!(libc::setrlimit(libc::RLIMIT_FSIZE, &limit), 0);
    }
    assert!(db.encrypt_all_blobs(&KEY).is_err());
}

use copywraith_core::api_types::CreateEntryRequest;
use copywraith_core::blob_store::fault_injection::{self, Boundary};
use copywraith_core::sync_protocol::{SyncAction, SyncMutation};

const PUBLICATION_BOUNDARIES: [Boundary; 5] = [
    Boundary::TempCreated,
    Boundary::TempWritten,
    Boundary::FileSynced,
    Boundary::Published,
    Boundary::DirectorySynced,
];

fn upload(server: &str) -> SyncMutation {
    SyncMutation {
        server_id: server.into(),
        operation_id: "durability-upload".into(),
        action: SyncAction::Create {
            expected: None,
            payload: CreateEntryRequest {
                content_type: ContentType::Image,
                text_content: None,
                flavors: None,
                blob_base64: Some(bytes_to_base64(PAYLOAD)),
                source_app: None,
                starred: None,
                content_hash: hash_bytes(PAYLOAD),
            },
        },
    }
}

fn fail_at(selected: Boundary) -> fault_injection::Guard {
    fault_injection::install(move |point| {
        if point == selected {
            return Err(std::io::Error::other(format!("injected {point:?}")));
        }
        Ok(())
    })
}

fn row_count(dir: &std::path::Path, table: &str) -> i64 {
    rusqlite::Connection::open(dir.join("copywraith.db"))
        .unwrap()
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

#[test]
#[ignore = "Protocol owner must repair retained live blobs before cached receipt replay"]
fn cached_upload_receipt_repairs_its_missing_live_blob() {
    let dir = tempfile::tempdir().unwrap();
    let db = server_storage::Storage::new(dir.path()).unwrap();
    let request = upload(&db.sync_info().unwrap().server_id);
    let receipt = db.apply_sync_mutation(&request, &KEY).unwrap();
    let frozen = serde_json::to_vec(&request).unwrap();
    let hash = hash_bytes(PAYLOAD);
    std::fs::remove_file(dir.path().join("blobs").join(&hash)).unwrap();
    drop(db);

    let db = server_storage::Storage::new(dir.path()).unwrap();
    let replay = serde_json::from_slice(&frozen).unwrap();
    let repeated = db.apply_sync_mutation(&replay, &KEY).unwrap();
    assert_eq!(
        serde_json::to_vec(&repeated).unwrap(),
        serde_json::to_vec(&receipt).unwrap()
    );
    let repaired = db
        .get_blob(&hash)
        .unwrap()
        .expect("Cached receipt acknowledged a missing blob");
    assert_eq!(crypto::decrypt_blob(&KEY, &repaired).unwrap(), PAYLOAD);
}

#[test]
fn server_receipt_waits_for_every_publication_boundary() {
    for point in PUBLICATION_BOUNDARIES {
        let dir = tempfile::tempdir().unwrap();
        let db = server_storage::Storage::new(dir.path()).unwrap();
        let server = db.sync_info().unwrap().server_id;
        let request = upload(&server);
        let frozen = serde_json::to_vec(&request).unwrap();
        let fault = fail_at(point);
        assert!(db.apply_sync_mutation(&request, &KEY).is_err(), "{point:?}");
        drop(fault);
        assert_eq!(row_count(dir.path(), "entries"), 0, "{point:?}");
        assert_eq!(row_count(dir.path(), "sync_receipts"), 0, "{point:?}");
        drop(db);
        let db = server_storage::Storage::new(dir.path()).unwrap();
        let replay = serde_json::from_slice(&frozen).unwrap();
        db.apply_sync_mutation(&replay, &KEY).unwrap();
        assert_eq!(serde_json::to_vec(&replay).unwrap(), frozen);
        assert_eq!(row_count(dir.path(), "sync_receipts"), 1);
        assert_eq!(
            crypto::decrypt_blob(&KEY, &db.get_blob(&hash_bytes(PAYLOAD)).unwrap().unwrap())
                .unwrap(),
            PAYLOAD
        );
    }
}

#[test]
fn client_cursor_waits_for_publication_and_duplicate_repair() {
    let origin = tempfile::tempdir().unwrap();
    let server = server_storage::Storage::new(origin.path()).unwrap();
    let server_id = server.sync_info().unwrap().server_id;
    server
        .apply_sync_mutation(&upload(&server_id), &KEY)
        .unwrap();
    let mut changes = server.sync_changes(&server_id, 0, 10, &KEY).unwrap();
    let mut change = changes.changes.remove(0);
    for point in PUBLICATION_BOUNDARIES {
        let dir = tempfile::tempdir().unwrap();
        let db = storage::LocalStorage::new(dir.path()).unwrap();
        db.bind_sync_server("profile", &server_id).unwrap();
        let fault = fail_at(point);
        assert!(db
            .apply_sync_live(&server_id, &change, Some(PAYLOAD))
            .is_err());
        drop(fault);
        assert_eq!(db.sync_cursor(&server_id).unwrap(), 0);
        assert_eq!(row_count(dir.path(), "entries"), 0);
        drop(db);
        let db = storage::LocalStorage::new(dir.path()).unwrap();
        db.apply_sync_live(&server_id, &change, Some(PAYLOAD))
            .unwrap();
        assert_eq!(db.sync_cursor(&server_id).unwrap(), change.sequence);

        let id = &change.generation.id;
        db.toggle_star(id).unwrap();
        let candidate = db.sync_candidates(&server_id).unwrap().remove(0);
        db.enqueue_sync_candidate(
            &server_id,
            &candidate,
            SyncAction::Star {
                generation_id: id.clone(),
                starred: true,
            },
        )
        .unwrap();
        let frozen = db.pending_mutations(&server_id).unwrap().remove(0).request;
        let frozen = serde_json::to_vec(&frozen).unwrap();
        std::fs::write(
            dir.path().join("blobs").join(hash_bytes(PAYLOAD)),
            b"damaged",
        )
        .unwrap();
        assert!(db.get_blob(&hash_bytes(PAYLOAD)).unwrap().is_none());
        let previous_cursor = change.sequence;
        change.sequence += 1;
        let fault = fail_at(point);
        assert!(db
            .apply_sync_live(&server_id, &change, Some(PAYLOAD))
            .is_err());
        drop(fault);
        assert_eq!(db.sync_cursor(&server_id).unwrap(), previous_cursor);
        drop(db);
        let db = storage::LocalStorage::new(dir.path()).unwrap();
        db.apply_sync_live(&server_id, &change, Some(PAYLOAD))
            .unwrap();
        assert_eq!(db.sync_cursor(&server_id).unwrap(), change.sequence);
        assert!(db.get_entry(id).unwrap().unwrap().starred);
        assert_eq!(
            serde_json::to_vec(&db.pending_mutations(&server_id).unwrap().remove(0).request)
                .unwrap(),
            frozen
        );
        assert_eq!(db.get_blob(&hash_bytes(PAYLOAD)).unwrap().unwrap(), PAYLOAD);
    }
}

#[test]
fn initial_directories_are_synchronized_before_database_creation() {
    for point in [Boundary::DirectoryCreated, Boundary::ParentSynced] {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("new").join("data");
        let fault = fail_at(point);
        assert!(storage::LocalStorage::new(&dir).is_err());
        drop(fault);
        assert!(!dir.join("copywraith.db").exists());
        storage::LocalStorage::new(&dir).unwrap();
    }
}

#[test]
fn encrypted_replacement_keeps_old_or_complete_new_bytes_at_each_boundary() {
    for point in PUBLICATION_BOUNDARIES {
        let dir = tempfile::tempdir().unwrap();
        let db = server_storage::Storage::new(dir.path()).unwrap();
        let hash = hash_bytes(PAYLOAD);
        db.create_entry(
            ContentType::Image,
            &ClipboardFlavors::default(),
            Some(&bytes_to_base64(PAYLOAD)),
            None,
            None,
            &hash,
            None,
        )
        .unwrap();
        let fault = fail_at(point);
        assert!(db.encrypt_all_blobs(&KEY).is_err());
        drop(fault);
        let stored = db.get_blob(&hash).unwrap().unwrap();
        assert_eq!(
            crypto::decrypt_blob(&KEY, &stored).unwrap(),
            PAYLOAD,
            "{point:?}"
        );
        drop(db);
        let db = server_storage::Storage::new(dir.path()).unwrap();
        db.encrypt_all_blobs(&KEY).unwrap();
        assert!(crypto::is_encrypted_blob(
            &db.get_blob(&hash).unwrap().unwrap()
        ));
    }
}

#[test]
fn failed_database_write_leaves_only_a_reusable_orphan() {
    let dir = tempfile::tempdir().unwrap();
    let db = server_storage::Storage::new(dir.path()).unwrap();
    let request = upload(&db.sync_info().unwrap().server_id);
    let inspect = rusqlite::Connection::open(dir.path().join("copywraith.db")).unwrap();
    inspect.execute_batch("CREATE TRIGGER reject_entry BEFORE INSERT ON entries BEGIN SELECT RAISE(FAIL, 'injected DB failure'); END;").unwrap();
    assert!(db.apply_sync_mutation(&request, &KEY).is_err());
    assert_eq!(row_count(dir.path(), "sync_receipts"), 0);
    assert_eq!(row_count(dir.path(), "entries"), 0);
    assert_eq!(
        crypto::decrypt_blob(&KEY, &db.get_blob(&hash_bytes(PAYLOAD)).unwrap().unwrap()).unwrap(),
        PAYLOAD
    );
    inspect.execute_batch("DROP TRIGGER reject_entry").unwrap();
    // Adoption of a complete orphan must still synchronize its directory.
    let fault = fail_at(Boundary::DirectorySynced);
    assert!(db.apply_sync_mutation(&request, &KEY).is_err());
    drop(fault);
    assert_eq!(row_count(dir.path(), "sync_receipts"), 0);
    db.apply_sync_mutation(&request, &KEY).unwrap();
}

#[test]
fn invalid_upload_cannot_repair_a_different_identity_or_downgrade_encryption() {
    let dir = tempfile::tempdir().unwrap();
    let db = server_storage::Storage::new(dir.path()).unwrap();
    let hash = hash_bytes(PAYLOAD);
    let upload = bytes_to_base64(PAYLOAD);
    let (entry, _) = db
        .create_entry(
            ContentType::Image,
            &ClipboardFlavors::default(),
            Some(&upload),
            None,
            Some(true),
            &hash,
            Some(&KEY),
        )
        .unwrap();
    let before = db.get_blob(&hash).unwrap().unwrap();
    for key in [None, Some(&KEY)] {
        assert!(db
            .create_entry(
                ContentType::Image,
                &ClipboardFlavors::default(),
                Some(&bytes_to_base64(b"wrong bytes")),
                None,
                None,
                &hash,
                key
            )
            .is_err());
    }
    assert!(db
        .create_entry(
            ContentType::Image,
            &ClipboardFlavors::default(),
            Some(&upload),
            None,
            None,
            &hash,
            None
        )
        .is_err());
    assert_eq!(db.get_blob(&hash).unwrap().unwrap(), before);
    assert!(
        db.get_entry(&entry.id, Some(&KEY))
            .unwrap()
            .unwrap()
            .starred
    );
    std::fs::write(dir.path().join("blobs").join(&hash), b"ENCBinvalid").unwrap();
    assert!(db.encrypt_all_blobs(&KEY).is_err());
    assert_eq!(db.get_blob(&hash).unwrap().unwrap(), b"ENCBinvalid");
}

#[test]
fn client_repairs_truncated_orphans_and_server_repairs_plaintext_duplicates() {
    let local_dir = tempfile::tempdir().unwrap();
    let local = storage::LocalStorage::new(local_dir.path()).unwrap();
    let hash = hash_bytes(PAYLOAD);
    std::fs::write(local_dir.path().join("blobs").join(&hash), b"truncated").unwrap();
    local
        .insert_entry(
            ContentType::Image,
            &ClipboardFlavors::default(),
            Some(PAYLOAD),
            &hash,
            None,
        )
        .unwrap();
    assert_eq!(local.get_blob(&hash).unwrap().unwrap(), PAYLOAD);

    let dir = tempfile::tempdir().unwrap();
    let db = server_storage::Storage::new(dir.path()).unwrap();
    let upload = bytes_to_base64(PAYLOAD);
    let (original, _) = db
        .create_entry(
            ContentType::Image,
            &ClipboardFlavors::default(),
            Some(&upload),
            None,
            Some(true),
            &hash,
            None,
        )
        .unwrap();
    std::fs::remove_file(dir.path().join("blobs").join(&hash)).unwrap();
    let (repaired, _) = db
        .create_entry(
            ContentType::Image,
            &ClipboardFlavors::default(),
            Some(&upload),
            None,
            Some(false),
            &hash,
            None,
        )
        .unwrap();
    assert_eq!(repaired.id, original.id);
    assert!(repaired.starred);
    assert_eq!(db.get_blob(&hash).unwrap().unwrap(), PAYLOAD);
}

#[test]
fn replacement_serializes_with_readers_and_post_commit_deletion() {
    use std::sync::{mpsc, Arc};
    use std::time::Duration;
    const WAIT: Duration = Duration::from_secs(5);
    const BLOCKED_WINDOW: Duration = Duration::from_millis(50);
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(server_storage::Storage::new(dir.path()).unwrap());
    let hash = hash_bytes(PAYLOAD);
    let (entry, _) = db
        .create_entry(
            ContentType::Image,
            &ClipboardFlavors::default(),
            Some(&bytes_to_base64(PAYLOAD)),
            None,
            None,
            &hash,
            None,
        )
        .unwrap();
    let (ready, waiting) = mpsc::channel();
    let (resume, released) = mpsc::channel();
    let writer = db.clone();
    let writer = std::thread::spawn(move || {
        let _hook = fault_injection::install(move |point| {
            if point == Boundary::TempWritten {
                ready.send(()).unwrap();
                released.recv_timeout(WAIT).unwrap();
            }
            Ok(())
        });
        writer.encrypt_all_blobs(&KEY).unwrap();
    });
    waiting.recv_timeout(WAIT).unwrap();
    // A reader already using the file path still sees the old complete bytes.
    assert_eq!(
        std::fs::read(dir.path().join("blobs").join(&hash)).unwrap(),
        PAYLOAD
    );
    let reader = db.clone();
    let reader_hash = hash.clone();
    let reader = std::thread::spawn(move || reader.get_blob(&reader_hash).unwrap());
    let deleter = db.clone();
    let (done, deleted) = mpsc::channel();
    let deleter = std::thread::spawn(move || {
        done.send(deleter.delete_entry(&entry.id).unwrap()).unwrap();
    });
    assert!(deleted.recv_timeout(BLOCKED_WINDOW).is_err());
    assert_eq!(row_count(dir.path(), "entries"), 1);
    resume.send(()).unwrap();
    writer.join().unwrap();
    if let Some(bytes) = reader.join().unwrap() {
        assert_eq!(crypto::decrypt_blob(&KEY, &bytes).unwrap(), PAYLOAD);
    }
    assert!(deleted.recv_timeout(WAIT).unwrap());
    deleter.join().unwrap();
    assert_eq!(row_count(dir.path(), "entries"), 0);
    assert!(db.get_blob(&hash).unwrap().is_none());
}
