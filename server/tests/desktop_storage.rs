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
    db.mark_synced(&legacy.id).unwrap();
    drop(db);
    let db = LocalStorage::new(dir.path()).unwrap();
    assert_eq!(db.get_entry(&legacy.id).unwrap().unwrap().id, legacy.id);
    assert_eq!(db.get_entry(&new.id).unwrap().unwrap().id, new.id);
    assert_eq!(db.get_unsynced_entries().unwrap().len(), 1);
    assert_eq!(db.get_unsynced_entries().unwrap()[0].id, new.id);
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
