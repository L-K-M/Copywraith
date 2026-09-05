#[cfg(test)]
mod migration_fixture_generator {
    use super::*;
    #[test]
    fn generate_legacy_fixtures() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let mut state = CryptoState::load(&path).unwrap();
        state.setup_password("fixture-password").unwrap();
        let dek = state.get_dek().unwrap();
        std::fs::write(path.join("dek.bin"), dek).unwrap();
        std::fs::write(path.join("text.enc"), encrypt_text(&dek, "legacy ciphertext 🦀").unwrap()).unwrap();
        std::fs::write(path.join("blob.enc"), encrypt_blob(&dek, b"\x00\xfflegacy blob").unwrap()).unwrap();
        let conn = rusqlite::Connection::open(path.join("legacy.db")).unwrap();
        conn.execute_batch("CREATE TABLE entries (
            id TEXT PRIMARY KEY, content_type TEXT NOT NULL, text_content TEXT,
            blob_hash TEXT, blob_size INTEGER, content_hash TEXT NOT NULL,
            source_app TEXT, starred INTEGER DEFAULT 0,
            created_at TEXT NOT NULL, updated_at TEXT NOT NULL);").unwrap();
        let entry = copywraith_core::models::ClipboardEntry::new_text("legacy row".into());
        conn.execute("INSERT INTO entries (id,content_type,text_content,content_hash,created_at,updated_at) VALUES (?1,'text','legacy row','legacy-hash',?2,?2)", rusqlite::params![entry.id, entry.created_at.to_rfc3339()]).unwrap();
        std::fs::write(path.join("entry.json"), serde_json::to_vec_pretty(&entry).unwrap()).unwrap();
    }
}
