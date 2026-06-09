use std::path::Path;
use std::sync::Mutex;

use chrono::Utc;
use copywraith_core::models::{ClipboardEntry, ClipboardFlavors, ContentType};
use copywraith_core::sensitive::contains_sensitive_data;
use rusqlite::{params, Connection, OptionalExtension};
use ulid::Ulid;

use crate::models::Settings;

const ENTRY_SELECT_COLUMNS: &str =
    "id, content_type, text_content, text_plain, text_html, text_rtf, blob_hash, blob_size, source_app, starred, sensitive, created_at, updated_at";

pub struct LocalStorage {
    db: Mutex<Connection>,
    blob_dir: std::path::PathBuf,
}

/// Parse a SQLite row (with the standard entry SELECT columns) into a ClipboardEntry.
fn row_to_entry(row: &rusqlite::Row) -> rusqlite::Result<ClipboardEntry> {
    let content_type = row
        .get::<_, String>(1)?
        .parse::<ContentType>()
        .unwrap_or(ContentType::Text);
    let legacy_text_content: Option<String> = row.get(2)?;
    let flavors = ClipboardFlavors {
        text_plain: row.get(3)?,
        text_html: row.get(4)?,
        text_rtf: row.get(5)?,
        file_list: None,
    }
    .merge_legacy(content_type, legacy_text_content.as_deref());

    Ok(ClipboardEntry {
        id: row.get(0)?,
        content_type,
        text_content: legacy_text_content.or_else(|| flavors.to_legacy_text_content(content_type)),
        blob_hash: row.get(6)?,
        blob_size: row.get::<_, Option<i64>>(7)?.map(|s| s as u64),
        source_app: row.get(8)?,
        flavors,
        starred: row.get::<_, i32>(9)? != 0,
        sensitive: row.get::<_, i32>(10)? != 0,
        created_at: row
            .get::<_, String>(11)?
            .parse()
            .unwrap_or_else(|_| Utc::now()),
        updated_at: row
            .get::<_, String>(12)?
            .parse()
            .unwrap_or_else(|_| Utc::now()),
    })
}

/// Escape SQL LIKE wildcards so user input matches literally.
fn escape_like_pattern(query: &str) -> String {
    query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn has_entries_column(conn: &Connection, column: &str) -> bool {
    conn.prepare(&format!("SELECT {} FROM entries LIMIT 0", column))
        .is_ok()
}

fn ensure_entries_column(
    conn: &Connection,
    column: &str,
    definition_sql: &str,
) -> anyhow::Result<()> {
    if has_entries_column(conn, column) {
        return Ok(());
    }

    conn.execute_batch(&format!(
        "ALTER TABLE entries ADD COLUMN {} {};",
        column, definition_sql
    ))?;
    Ok(())
}

fn backfill_flavor_columns(conn: &Connection) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE entries
         SET text_plain = text_content
         WHERE text_plain IS NULL AND content_type = 'text' AND text_content IS NOT NULL",
        [],
    )?;
    conn.execute(
        "UPDATE entries
         SET text_html = text_content
         WHERE text_html IS NULL AND content_type = 'html' AND text_content IS NOT NULL",
        [],
    )?;
    conn.execute(
        "UPDATE entries
         SET text_rtf = text_content
         WHERE text_rtf IS NULL AND content_type = 'rtf' AND text_content IS NOT NULL",
        [],
    )?;

    // Keep startup migrations lightweight: avoid loading and rewriting the entire
    // history table on every launch. We only fill clearly-missing search_text
    // values here; new and updated entries always compute search_text at write
    // time.
    conn.execute(
        "UPDATE entries
         SET search_text = COALESCE(text_plain, text_content)
         WHERE search_text IS NULL
           AND content_type = 'text'
           AND (text_plain IS NOT NULL OR text_content IS NOT NULL)",
        [],
    )?;

    conn.execute(
        "UPDATE entries
         SET search_text = COALESCE(text_html, text_content)
         WHERE search_text IS NULL
           AND content_type = 'html'
           AND (text_html IS NOT NULL OR text_content IS NOT NULL)",
        [],
    )?;

    conn.execute(
        "UPDATE entries
         SET search_text = COALESCE(text_rtf, text_content)
         WHERE search_text IS NULL
           AND content_type = 'rtf'
           AND (text_rtf IS NOT NULL OR text_content IS NOT NULL)",
        [],
    )?;

    Ok(())
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
                text_plain TEXT,
                text_html TEXT,
                text_rtf TEXT,
                search_text TEXT,
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

        ensure_entries_column(&conn, "text_plain", "TEXT")?;
        ensure_entries_column(&conn, "text_html", "TEXT")?;
        ensure_entries_column(&conn, "text_rtf", "TEXT")?;
        ensure_entries_column(&conn, "search_text", "TEXT")?;

        backfill_flavor_columns(&conn)?;

        Ok(Self {
            db: Mutex::new(conn),
            blob_dir,
        })
    }

    pub fn insert_entry(
        &self,
        content_type: ContentType,
        flavors: &ClipboardFlavors,
        blob_data: Option<&[u8]>,
        content_hash: &str,
        source_app: Option<&str>,
    ) -> anyhow::Result<Option<ClipboardEntry>> {
        let db = self.db.lock().unwrap();

        let resolved_flavors = flavors.clone().merge_legacy(content_type, None);
        let legacy_text_content = resolved_flavors.to_legacy_text_content(content_type);
        let search_text = resolved_flavors.best_plain_text();

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

        let sensitive = search_text
            .as_ref()
            .map(|t| contains_sensitive_data(t))
            .unwrap_or(false);

        db.execute(
            "INSERT INTO entries (id, content_type, text_content, text_plain, text_html, text_rtf, search_text, blob_hash, blob_size, content_hash, source_app, starred, sensitive, synced, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0, ?12, 0, ?13, ?14)",
            params![
                id,
                content_type.as_str(),
                legacy_text_content,
                resolved_flavors.text_plain,
                resolved_flavors.text_html,
                resolved_flavors.text_rtf,
                search_text,
                blob_hash,
                blob_size.map(|s| s as i64),
                content_hash,
                source_app,
                sensitive as i32,
                now.to_rfc3339(),
                now.to_rfc3339(),
            ],
        )?;

        let entry_flavors = flavors.clone().merge_legacy(content_type, None);
        let entry_text_content = entry_flavors.to_legacy_text_content(content_type);

        Ok(Some(ClipboardEntry {
            id,
            content_type,
            text_content: entry_text_content,
            blob_hash,
            blob_size,
            source_app: source_app.map(|s| s.to_string()),
            flavors: entry_flavors,
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
                conditions.push(format!(
                    "search_text LIKE ?{} ESCAPE '\\'",
                    param_values.len() + 1
                ));
                param_values.push(Box::new(format!("%{}%", escape_like_pattern(q))));
            }
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let sql = format!(
            "SELECT {}
             FROM entries {}
             ORDER BY updated_at DESC
             LIMIT ?{} OFFSET ?{}",
            ENTRY_SELECT_COLUMNS,
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
                &format!("SELECT {} FROM entries WHERE id = ?1", ENTRY_SELECT_COLUMNS),
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
                &format!(
                    "SELECT {} FROM entries ORDER BY updated_at DESC LIMIT 1",
                    ENTRY_SELECT_COLUMNS
                ),
                [],
                |row| row_to_entry(row),
            )
            .optional()?;
        Ok(entry)
    }

    pub fn get_unsynced_entries(&self) -> anyhow::Result<Vec<ClipboardEntry>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(&format!(
            "SELECT {} FROM entries WHERE synced = 0 ORDER BY created_at ASC LIMIT 50",
            ENTRY_SELECT_COLUMNS
        ))?;

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
        let shizuku_clipboard_enabled = db
            .query_row(
                "SELECT value FROM settings WHERE key = 'shizuku_clipboard_enabled'",
                [],
                |row| row.get::<_, String>(0),
            )
            .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
            .unwrap_or(defaults.shizuku_clipboard_enabled);
        Settings {
            server_url_primary,
            server_url_fallback,
            api_key,
            shortcut_toggle_popup,
            shortcut_starred_popup,
            shortcut_paste_plaintext,
            shizuku_clipboard_enabled,
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
            db.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES ('shizuku_clipboard_enabled', ?1)",
                params![if settings.shizuku_clipboard_enabled { "1" } else { "0" }],
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

    pub fn clear_sync_cursor(&self) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "DELETE FROM settings WHERE key = 'sync_last_seen_server_id'",
            [],
        )?;
        Ok(())
    }
}
