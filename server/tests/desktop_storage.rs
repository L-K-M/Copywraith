// Exercise the actual desktop persistence layer without a GUI/display dependency.
#[allow(dead_code)]
#[path = "../../src-tauri/src/models.rs"]
mod models;
#[allow(dead_code)]
#[path = "../../src-tauri/src/storage.rs"]
mod storage;

use copywraith_core::models::{ClipboardEntry, ClipboardFlavors, ContentType};
use storage::LocalStorage;

#[test]
fn legacy_desktop_database_preserves_ids_and_sync_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("copywraith.db");
    std::fs::write(&path, include_bytes!("fixtures/legacy.db")).unwrap();
    // The shared legacy schema differs from the desktop schema by this column.
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("ALTER TABLE entries ADD COLUMN synced INTEGER DEFAULT 0;")
        .unwrap();
    drop(conn);
    let legacy: ClipboardEntry = serde_json::from_str(include_str!("fixtures/entry.json")).unwrap();
    let db = LocalStorage::new(dir.path()).unwrap();
    let old = db.get_entry(&legacy.id).unwrap().unwrap();
    assert_eq!(old.flavors.text_plain.as_deref(), Some("legacy row"));
    assert!(!old.sensitive);
    let flavors = ClipboardFlavors {
        text_plain: Some("new local row".into()),
        ..Default::default()
    };
    let new = db
        .insert_entry(ContentType::Text, &flavors, None, "new-local-hash", None)
        .unwrap()
        .unwrap();
    assert!(new.id.parse::<ulid::Ulid>().is_ok());
    assert_ne!(new.id, legacy.id);
    // A timestamp written by an older version must still match what it parses to.
    assert!(db
        .mark_synced_if_unchanged(&legacy.id, old.updated_at)
        .unwrap());
    drop(db);
    let db = LocalStorage::new(dir.path()).unwrap();
    let old = db.get_entry(&legacy.id).unwrap().unwrap();
    assert_eq!(old.id, legacy.id);
    assert!(!old.sensitive);
    assert_eq!(db.get_entry(&new.id).unwrap().unwrap().id, new.id);
    assert_eq!(db.get_unsynced_entries().unwrap().len(), 1);
    assert_eq!(db.get_unsynced_entries().unwrap()[0].id, new.id);
}

#[test]
fn migration_preserves_existing_sensitive_flags() {
    let legacy: ClipboardEntry = serde_json::from_str(include_str!("fixtures/entry.json")).unwrap();

    for sensitive in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("copywraith.db");
        std::fs::write(&path, include_bytes!("fixtures/legacy.db")).unwrap();
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "ALTER TABLE entries ADD COLUMN synced INTEGER DEFAULT 0;
             ALTER TABLE entries ADD COLUMN sensitive INTEGER DEFAULT 0;",
        )
        .unwrap();
        conn.execute(
            "UPDATE entries SET sensitive = ?1 WHERE id = ?2",
            rusqlite::params![sensitive as i32, legacy.id],
        )
        .unwrap();
        drop(conn);

        // Stored classifications survive migrations and repeated opens.
        for _ in 0..2 {
            let db = LocalStorage::new(dir.path()).unwrap();
            let old = db.get_entry(&legacy.id).unwrap().unwrap();
            assert_eq!(old.sensitive, sensitive);
        }
    }
}

#[test]
fn local_insertion_preserves_text_payloads_and_metadata() {
    let cases = [
        (
            ContentType::Text,
            serde_json::json!({ "text_plain": " \tHello, world!\n" }),
            Some(" \tHello, world!\n"),
            false,
        ),
        (
            ContentType::Text,
            serde_json::json!({ "text_plain": "", "text_html": "", "text_rtf": "" }),
            Some(""),
            false,
        ),
        (ContentType::Text, serde_json::json!({}), None, false),
        (
            ContentType::Text,
            serde_json::json!({
                "text_plain": "Hello world",
                "text_html": "<b>Hello world</b>",
                "text_rtf": r"{\rtf1\ansi Hello world}"
            }),
            Some("Hello world"),
            false,
        ),
        (
            ContentType::Html,
            serde_json::json!({ "text_html": "<p>Grüezi, world!</p>" }),
            Some("<p>Grüezi, world!</p>"),
            false,
        ),
        (
            ContentType::Rtf,
            serde_json::json!({ "text_rtf": r"{\rtf1\ansi Hello world}" }),
            Some(r"{\rtf1\ansi Hello world}"),
            false,
        ),
        (
            ContentType::File,
            serde_json::json!({ "file_list": ["/tmp/café notes.txt", "/tmp/report.pdf"] }),
            Some("/tmp/café notes.txt\n/tmp/report.pdf"),
            false,
        ),
        (
            ContentType::Text,
            serde_json::json!({ "text_plain": "password=hunter2" }),
            Some("password=hunter2"),
            true,
        ),
    ];

    for (content_type, expected_flavors, expected_text, expected_sensitive) in cases {
        let dir = tempfile::tempdir().unwrap();
        let db = LocalStorage::new(dir.path()).unwrap();
        let flavors: ClipboardFlavors = serde_json::from_value(expected_flavors.clone()).unwrap();
        let hash = flavors.payload_hash(content_type, None);
        let entry = db
            .insert_entry(content_type, &flavors, None, &hash, Some("Clipboard tests"))
            .unwrap()
            .unwrap();

        assert_eq!(entry.content_type, content_type);
        assert_eq!(entry.text_content.as_deref(), expected_text);
        assert_eq!(
            serde_json::to_value(&entry.flavors).unwrap(),
            expected_flavors
        );
        assert_eq!(serde_json::to_value(&flavors).unwrap(), expected_flavors);
        assert_eq!(entry.sensitive, expected_sensitive);
        assert!(!entry.starred);
        assert_eq!(entry.source_app.as_deref(), Some("Clipboard tests"));
        assert_eq!(entry.created_at, entry.updated_at);
        assert!(entry.blob_hash.is_none());
        assert!(entry.blob_size.is_none());

        // Returned and persisted payloads must agree, including whitespace.
        let expected_entry = serde_json::to_value(&entry).unwrap();
        let stored = db.get_entry(&entry.id).unwrap().unwrap();
        assert_eq!(serde_json::to_value(stored).unwrap(), expected_entry);
        let unsynced = db.get_unsynced_entries().unwrap();
        assert_eq!(unsynced.len(), 1);
        assert_eq!(unsynced[0].id, entry.id);
        drop(db);

        let reopened = LocalStorage::new(dir.path()).unwrap();
        let stored = reopened.get_entry(&entry.id).unwrap().unwrap();
        assert_eq!(serde_json::to_value(stored).unwrap(), expected_entry);
    }
}

#[test]
fn local_insertion_preserves_image_and_shared_file_blobs() {
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgba8(1, 1)
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let bytes = encoded.into_inner();

    for (content_type, expected_flavors, expected_text, bytes) in [
        (
            ContentType::Image,
            serde_json::json!({}),
            None,
            bytes.as_slice(),
        ),
        (
            ContentType::File,
            serde_json::json!({ "file_list": ["shared.bin"] }),
            Some("shared.bin"),
            b"\0shared file\xff".as_slice(),
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let db = LocalStorage::new(dir.path()).unwrap();
        let flavors: ClipboardFlavors = serde_json::from_value(expected_flavors.clone()).unwrap();
        let blob_hash = copywraith_core::content::hash_bytes(bytes);
        let hash = flavors.payload_hash(content_type, Some(&blob_hash));
        let entry = db
            .insert_entry(content_type, &flavors, Some(bytes), &hash, None)
            .unwrap()
            .unwrap();

        assert_eq!(entry.content_type, content_type);
        assert_eq!(entry.text_content.as_deref(), expected_text);
        assert_eq!(
            serde_json::to_value(&entry.flavors).unwrap(),
            expected_flavors
        );
        assert_eq!(serde_json::to_value(&flavors).unwrap(), expected_flavors);
        assert_eq!(entry.blob_hash.as_deref(), Some(blob_hash.as_str()));
        assert_eq!(entry.blob_size, Some(bytes.len() as u64));
        assert!(entry.source_app.is_none());
        assert!(!entry.sensitive);
        assert_eq!(db.get_blob(&blob_hash).unwrap().unwrap(), bytes);

        let expected_entry = serde_json::to_value(&entry).unwrap();
        let stored = db.get_entry(&entry.id).unwrap().unwrap();
        assert_eq!(serde_json::to_value(stored).unwrap(), expected_entry);
        drop(db);

        let reopened = LocalStorage::new(dir.path()).unwrap();
        let stored = reopened.get_entry(&entry.id).unwrap().unwrap();
        assert_eq!(serde_json::to_value(stored).unwrap(), expected_entry);
        assert_eq!(reopened.get_blob(&blob_hash).unwrap().unwrap(), bytes);
    }
}

#[test]
fn duplicate_local_insertion_preserves_metadata_and_bypasses_blob_writes() {
    let dir = tempfile::tempdir().unwrap();
    let db = LocalStorage::new(dir.path()).unwrap();
    let flavors = ClipboardFlavors {
        file_list: Some(vec!["stored.bin".into()]),
        ..Default::default()
    };
    let bytes = b"stored binary payload";
    let blob_hash = copywraith_core::content::hash_bytes(bytes);
    let hash = flavors.payload_hash(ContentType::File, Some(&blob_hash));
    let entry = db
        .insert_entry(
            ContentType::File,
            &flavors,
            Some(bytes),
            &hash,
            Some("Original app"),
        )
        .unwrap()
        .unwrap();
    db.set_starred(&entry.id, true).unwrap();

    // A fixed older timestamp makes the recopy update observable.
    let conn = rusqlite::Connection::open(dir.path().join("copywraith.db")).unwrap();
    conn.execute(
        "UPDATE entries SET created_at = ?1, updated_at = ?1 WHERE id = ?2",
        rusqlite::params!["2000-01-01T00:00:00Z", entry.id],
    )
    .unwrap();
    drop(conn);
    let before = db.get_entry(&entry.id).unwrap().unwrap();
    assert!(db
        .mark_synced_if_unchanged(&entry.id, before.updated_at)
        .unwrap());

    // A duplicate must bypass blob I/O, even when its directory is unusable.
    let blob_dir = dir.path().join("blobs");
    let saved_blobs = dir.path().join("saved-blobs");
    std::fs::rename(&blob_dir, &saved_blobs).unwrap();
    std::fs::write(&blob_dir, b"not a directory").unwrap();
    let duplicate = db
        .insert_entry(
            ContentType::File,
            &flavors,
            Some(bytes),
            &hash,
            Some("Other app"),
        )
        .unwrap();
    assert!(duplicate.is_none());

    let after = db.get_entry(&entry.id).unwrap().unwrap();
    assert!(after.updated_at > before.updated_at);
    let mut expected = before;
    expected.updated_at = after.updated_at;
    assert_eq!(
        serde_json::to_value(after).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert_eq!(db.get_entries(10, 0, false, None).unwrap().len(), 1);
    assert!(db.get_unsynced_entries().unwrap().is_empty());

    std::fs::remove_file(&blob_dir).unwrap();
    std::fs::rename(&saved_blobs, &blob_dir).unwrap();
    assert_eq!(db.get_blob(&blob_hash).unwrap().unwrap(), bytes);
}

#[test]
fn all_entry_generators_preserve_identifier_json_format() {
    let entries = [
        ClipboardEntry::new_text("text".into()),
        ClipboardEntry::new_html("<b>html</b>".into()),
        ClipboardEntry::new_image("blob-hash".into(), 1),
    ];
    let mut ids = std::collections::HashSet::new();
    for entry in entries {
        let id: ulid::Ulid = entry.id.parse().unwrap();
        assert_eq!(id.to_string(), entry.id);
        assert!(ids.insert(entry.id.clone()));
        let json = serde_json::to_string(&entry).unwrap();
        let restored: ClipboardEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.id, entry.id);
        assert_eq!(
            serde_json::to_value(id).unwrap(),
            serde_json::Value::String(entry.id)
        );
    }
}
