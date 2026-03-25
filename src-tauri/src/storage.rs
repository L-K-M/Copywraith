use std::path::Path;
use std::sync::Mutex;

use chrono::Utc;
use copywraith_core::models::{ClipboardEntry, ContentType};
use copywraith_core::sensitive::contains_sensitive_data;
use rusqlite::{params, Connection, OptionalExtension};
use ulid::Ulid;

use crate::models::Settings;

pub struct LocalStorage {
    db: Mutex<Connection>,
    blob_dir: std::path::PathBuf,
}

/// Parse a SQLite row (with the standard 10-column SELECT) into a ClipboardEntry.
fn row_to_entry(row: &rusqlite::Row) -> rusqlite::Result<ClipboardEntry> {
    Ok(ClipboardEntry {
        id: row.get(0)?,
        content_type: row
            .get::<_, String>(1)?
            .parse::<ContentType>()
            .unwrap_or(ContentType::Text),
        text_content: row.get(2)?,
        blob_hash: row.get(3)?,
        blob_size: row.get::<_, Option<i64>>(4)?.map(|s| s as u64),
        source_app: row.get(5)?,
        starred: row.get::<_, i32>(6)? != 0,
        sensitive: row.get::<_, i32>(7)? != 0,
        created_at: row
            .get::<_, String>(8)?
            .parse()
            .unwrap_or_else(|_| Utc::now()),
        updated_at: row
            .get::<_, String>(9)?
            .parse()
            .unwrap_or_else(|_| Utc::now()),
    })
}

impl LocalStorage {
    pub fn new(data_dir: &Path) -> anyhow::Result<Self> {
        let db_path = data_dir.join("copywraith.db");
        let blob_dir = data_dir.join("blobs");
        std::fs::create_dir_all(&blob_dir)?;

        let conn = Connection::open(&db_path)?;
        conn.execute_batch(
            "
            PRAGMA journal_mode=WAL;

            CREATE TABLE IF NOT EXISTS entries (
                id TEXT PRIMARY KEY,
                content_type TEXT NOT NULL,
                text_content TEXT,
                blob_hash TEXT,
                blob_size INTEGER,
                content_hash TEXT NOT NULL,
                source_app TEXT,
                starred INTEGER DEFAULT 0,
                sensitive INTEGER DEFAULT 0,
                synced INTEGER DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_entries_updated_at ON entries(updated_at DESC);
            CREATE INDEX IF NOT EXISTS idx_entries_starred ON entries(starred) WHERE starred = 1;
            CREATE UNIQUE INDEX IF NOT EXISTS idx_entries_content_hash ON entries(content_hash);
            CREATE INDEX IF NOT EXISTS idx_entries_synced ON entries(synced) WHERE synced = 0;

            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            ",
        )?;

        // Migration: add sensitive column if missing (existing databases)
        let has_sensitive: bool = conn
            .prepare("SELECT sensitive FROM entries LIMIT 0")
            .is_ok();
        if !has_sensitive {
            conn.execute_batch("ALTER TABLE entries ADD COLUMN sensitive INTEGER DEFAULT 0;")?;
        }

        Ok(Self {
            db: Mutex::new(conn),
            blob_dir,
        })
    }

    pub fn insert_entry(
        &self,
        content_type: ContentType,
        text_content: Option<&str>,
        blob_data: Option<&[u8]>,
        content_hash: &str,
        source_app: Option<&str>,
    ) -> anyhow::Result<Option<ClipboardEntry>> {
        let db = self.db.lock().unwrap();

        // Check for duplicate
        let existing_id: Option<String> = db
            .query_row(
                "SELECT id FROM entries WHERE content_hash = ?1",
                params![content_hash],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(id) = existing_id {
            let now = Utc::now();
            db.execute(
                "UPDATE entries SET updated_at = ?1 WHERE id = ?2",
                params![now.to_rfc3339(), id],
            )?;
            return Ok(None); // Duplicate, moved to top
        }

        let (blob_hash, blob_size) = if let Some(data) = blob_data {
            let hash = copywraith_core::content::hash_bytes(data);
            let size = data.len() as u64;

            // Validate hash before using as filename (defense in depth)
            if !copywraith_core::content::is_valid_hash(&hash) {
                anyhow::bail!("Generated invalid blob hash");
            }
            let blob_path = self.blob_dir.join(&hash);
            if !blob_path.exists() {
                std::fs::write(&blob_path, data)?;
            }

            (Some(hash), Some(size))
        } else {
            (None, None)
        };

        let now = Utc::now();
        let id = Ulid::new().to_string();

        let sensitive = text_content
            .map(|t| contains_sensitive_data(t))
            .unwrap_or(false);

        db.execute(
            "INSERT INTO entries (id, content_type, text_content, blob_hash, blob_size, content_hash, source_app, starred, sensitive, synced, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, 0, ?9, ?10)",
            params![
                id,
                content_type.as_str(),
                text_content,
                blob_hash,
                blob_size.map(|s| s as i64),
                content_hash,
                source_app,
                sensitive as i32,
                now.to_rfc3339(),
                now.to_rfc3339(),
            ],
        )?;

        Ok(Some(ClipboardEntry {
            id,
            content_type,
            text_content: text_content.map(|s| s.to_string()),
            blob_hash,
            blob_size,
            source_app: source_app.map(|s| s.to_string()),
            starred: false,
            sensitive,
            created_at: now,
            updated_at: now,
        }))
    }

    pub fn has_content_hash(&self, content_hash: &str) -> anyhow::Result<bool> {
        let db = self.db.lock().unwrap();
        let exists: i64 = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM entries WHERE content_hash = ?1)",
            params![content_hash],
            |row| row.get(0),
        )?;
        Ok(exists != 0)
    }

    pub fn set_starred(&self, id: &str, starred: bool) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "UPDATE entries SET starred = ?1 WHERE id = ?2",
            params![if starred { 1 } else { 0 }, id],
        )?;
        Ok(())
    }

    pub fn get_entries(
        &self,
        limit: u32,
        offset: u32,
        starred_only: bool,
        search: Option<&str>,
    ) -> anyhow::Result<Vec<ClipboardEntry>> {
        let db = self.db.lock().unwrap();

        let mut conditions = Vec::new();
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if starred_only {
            conditions.push("starred = 1".to_string());
        }

        if let Some(q) = search {
            if !q.is_empty() {
                conditions.push(format!("text_content LIKE ?{}", param_values.len() + 1));
                param_values.push(Box::new(format!("%{}%", q)));
            }
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let sql = format!(
            "SELECT id, content_type, text_content, blob_hash, blob_size, source_app, starred, sensitive, created_at, updated_at
             FROM entries {}
             ORDER BY updated_at DESC
             LIMIT ?{} OFFSET ?{}",
            where_clause,
            param_values.len() + 1,
            param_values.len() + 2,
        );

        param_values.push(Box::new(limit as i64));
        param_values.push(Box::new(offset as i64));

        let mut stmt = db.prepare(&sql)?;
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        let entries = stmt
            .query_map(param_refs.as_slice(), |row| row_to_entry(row))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(entries)
    }

    pub fn get_entry(&self, id: &str) -> anyhow::Result<Option<ClipboardEntry>> {
        let db = self.db.lock().unwrap();
        let entry = db
            .query_row(
                "SELECT id, content_type, text_content, blob_hash, blob_size, source_app, starred, sensitive, created_at, updated_at
                 FROM entries WHERE id = ?1",
                params![id],
                |row| row_to_entry(row),
            )
            .optional()?;
        Ok(entry)
    }

    pub fn toggle_star(&self, id: &str) -> anyhow::Result<bool> {
        let db = self.db.lock().unwrap();
        let current: i32 = db.query_row(
            "SELECT starred FROM entries WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )?;
        let new_value = if current == 0 { 1 } else { 0 };
        let now = Utc::now();
        db.execute(
            "UPDATE entries SET starred = ?1, synced = 0, updated_at = ?2 WHERE id = ?3",
            params![new_value, now.to_rfc3339(), id],
        )?;
        Ok(new_value == 1)
    }

    pub fn apply_remote_star_state_by_content_hash(
        &self,
        content_hash: &str,
        starred: bool,
    ) -> anyhow::Result<bool> {
        let db = self.db.lock().unwrap();
        let target = if starred { 1 } else { 0 };

        let row: Option<(i32, i32)> = db
            .query_row(
                "SELECT starred, synced FROM entries WHERE content_hash = ?1",
                params![content_hash],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;

        let Some((current_starred, synced)) = row else {
            return Ok(false);
        };

        // Keep local pending changes if they have not been pushed yet.
        if synced == 0 || current_starred == target {
            return Ok(false);
        }

        let now = Utc::now();
        let updated = db.execute(
            "UPDATE entries SET starred = ?1, updated_at = ?2 WHERE content_hash = ?3 AND synced = 1",
            params![target, now.to_rfc3339(), content_hash],
        )?;

        Ok(updated > 0)
    }

    pub fn delete_entry(&self, id: &str) -> anyhow::Result<bool> {
        let db = self.db.lock().unwrap();

        // Read blob_hash before deleting so we can clean up the file
        let blob_hash: Option<String> = db
            .query_row(
                "SELECT blob_hash FROM entries WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?
            .flatten();

        let rows = db.execute("DELETE FROM entries WHERE id = ?1", params![id])?;

        // Remove blob file if no other entry references the same hash
        if rows > 0 {
            if let Some(ref hash) = blob_hash {
                let count: i64 = db.query_row(
                    "SELECT COUNT(*) FROM entries WHERE blob_hash = ?1",
                    params![hash],
                    |row| row.get(0),
                )?;
                if count == 0 {
                    let blob_path = self.blob_dir.join(hash);
                    let _ = std::fs::remove_file(blob_path);
                }
            }
        }

        Ok(rows > 0)
    }

    pub fn get_blob(&self, hash: &str) -> anyhow::Result<Option<Vec<u8>>> {
        if !copywraith_core::content::is_valid_hash(hash) {
            anyhow::bail!("Invalid blob hash: {}", hash);
        }
        let blob_path = self.blob_dir.join(hash);
        if blob_path.exists() {
            Ok(Some(std::fs::read(&blob_path)?))
        } else {
            Ok(None)
        }
    }

    pub fn get_most_recent_entry(&self) -> anyhow::Result<Option<ClipboardEntry>> {
        let db = self.db.lock().unwrap();
        let entry = db
            .query_row(
                "SELECT id, content_type, text_content, blob_hash, blob_size, source_app, starred, sensitive, created_at, updated_at
                 FROM entries ORDER BY updated_at DESC LIMIT 1",
                [],
                |row| row_to_entry(row),
            )
            .optional()?;
        Ok(entry)
    }

    pub fn get_unsynced_entries(&self) -> anyhow::Result<Vec<ClipboardEntry>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, content_type, text_content, blob_hash, blob_size, source_app, starred, sensitive, created_at, updated_at
             FROM entries WHERE synced = 0 ORDER BY created_at ASC LIMIT 50",
        )?;

        let entries = stmt
            .query_map([], |row| row_to_entry(row))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(entries)
    }

    pub fn mark_synced(&self, id: &str) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute("UPDATE entries SET synced = 1 WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn get_settings(&self) -> Settings {
        let db = self.db.lock().unwrap();
        let defaults = Settings::default();
        let server_url_primary = db
            .query_row(
                "SELECT value FROM settings WHERE key = 'server_url_primary'",
                [],
                |row| row.get::<_, String>(0),
            )
            .or_else(|_| {
                // Backward compatibility for older clients that only stored one server URL.
                db.query_row(
                    "SELECT value FROM settings WHERE key = 'server_url'",
                    [],
                    |row| row.get::<_, String>(0),
                )
            })
            .unwrap_or_default();
        let server_url_fallback = db
            .query_row(
                "SELECT value FROM settings WHERE key = 'server_url_fallback'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap_or_default();
        let api_key = db
            .query_row(
                "SELECT value FROM settings WHERE key = 'api_key'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap_or_default();
        let shortcut_toggle_popup = db
            .query_row(
                "SELECT value FROM settings WHERE key = 'shortcut_toggle_popup'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap_or(defaults.shortcut_toggle_popup);
        let shortcut_starred_popup = db
            .query_row(
                "SELECT value FROM settings WHERE key = 'shortcut_starred_popup'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap_or(defaults.shortcut_starred_popup);
        let shortcut_paste_plaintext = db
            .query_row(
                "SELECT value FROM settings WHERE key = 'shortcut_paste_plaintext'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap_or(defaults.shortcut_paste_plaintext);
        Settings {
            server_url_primary,
            server_url_fallback,
            api_key,
            shortcut_toggle_popup,
            shortcut_starred_popup,
            shortcut_paste_plaintext,
        }
    }

    pub fn save_settings(&self, settings: &Settings) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute_batch("BEGIN")?;
        let result = (|| -> anyhow::Result<()> {
            db.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES ('server_url_primary', ?1)",
                params![settings.server_url_primary],
            )?;
            db.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES ('server_url_fallback', ?1)",
                params![settings.server_url_fallback],
            )?;
            db.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES ('server_url', ?1)",
                params![settings.server_url_primary],
            )?;
            db.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES ('api_key', ?1)",
                params![settings.api_key],
            )?;
            db.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES ('shortcut_toggle_popup', ?1)",
                params![settings.shortcut_toggle_popup],
            )?;
            db.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES ('shortcut_starred_popup', ?1)",
                params![settings.shortcut_starred_popup],
            )?;
            db.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES ('shortcut_paste_plaintext', ?1)",
                params![settings.shortcut_paste_plaintext],
            )?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                db.execute_batch("COMMIT")?;
                Ok(())
            }
            Err(e) => {
                let _ = db.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    pub fn get_sync_cursor(&self) -> Option<String> {
        let db = self.db.lock().unwrap();
        db.query_row(
            "SELECT value FROM settings WHERE key = 'sync_last_seen_server_id'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .filter(|s| !s.is_empty())
    }

    pub fn save_sync_cursor(&self, cursor: &str) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('sync_last_seen_server_id', ?1)",
            params![cursor],
        )?;
        Ok(())
    }
}
