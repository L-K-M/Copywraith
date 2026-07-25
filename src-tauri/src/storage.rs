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

/// The server's identity for an entry being pulled into local storage.
///
/// Pulled rows used to be inserted through the local capture path, which minted
/// a fresh ULID and stamped `created_at`/`updated_at` with the time of the pull.
/// Because the server returns newest-first and the client walks pages in that
/// order, the *oldest* server entry was inserted last and so received the
/// *newest* local timestamp — a fresh install displayed its history in reverse,
/// and "paste most recent" picked the wrong item.
///
/// Carrying the server's id and timestamps through keeps one entry the same
/// entry on every device, and keeps the list in true chronological order.
#[derive(Debug, Clone, Copy)]
pub struct RemoteEntryIdentity<'a> {
    pub id: &'a str,
    pub created_at: chrono::DateTime<Utc>,
    pub updated_at: chrono::DateTime<Utc>,
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
        // The live-entry reads all filter tombstones out, so anything reaching
        // this projection is by definition not deleted.
        deleted_at: None,
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
            -- WAL + NORMAL still survives a process crash; only a full OS/power
            -- loss can drop the most recent commit. Without this SQLite defaults
            -- to FULL, which fsyncs on every implicit transaction — on Android
            -- flash that is 5-40ms per statement and dominates sync wall time.
            PRAGMA synchronous=NORMAL;
            -- Wait instead of failing immediately when the sync loop and a UI
            -- command reach the database at the same time.
            PRAGMA busy_timeout=5000;

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
                updated_at TEXT NOT NULL,
                deleted_at TEXT
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
        ensure_entries_column(&conn, "deleted_at", "TEXT")?;

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

        let (blob_hash, blob_size) = self.write_blob(blob_data)?;

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
            deleted_at: None,
        }))
    }

    /// Insert an entry pulled from the server in a single transaction.
    ///
    /// The naive path (`insert_entry` + `set_starred` + `mark_synced`) issues
    /// three separate implicit transactions, each of which fsyncs. During a bulk
    /// pull that disk time dominates everything else, so the three writes are
    /// committed together here instead.
    ///
    /// Returns `Ok(true)` when a new row was created, or `Ok(false)` when the
    /// entry was already present — matched either by content hash or by the
    /// server id. Re-pulling a known id with different content keeps the
    /// existing row: entries are immutable apart from `starred`, which the
    /// caller reconciles separately.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_remote_entry(
        &self,
        remote: RemoteEntryIdentity<'_>,
        content_type: ContentType,
        flavors: &ClipboardFlavors,
        blob_data: Option<&[u8]>,
        content_hash: &str,
        source_app: Option<&str>,
        starred: bool,
    ) -> anyhow::Result<bool> {
        let resolved_flavors = flavors.clone().merge_legacy(content_type, None);
        let legacy_text_content = resolved_flavors.to_legacy_text_content(content_type);
        let search_text = resolved_flavors.best_plain_text();
        let sensitive = search_text
            .as_ref()
            .map(|t| contains_sensitive_data(t))
            .unwrap_or(false);

        // Write the blob before the row so a crash can never leave a row
        // pointing at a blob that does not exist.
        let (blob_hash, blob_size) = self.write_blob(blob_data)?;

        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;

        let existing_id: Option<String> = tx
            .query_row(
                "SELECT id FROM entries WHERE content_hash = ?1 OR id = ?2 LIMIT 1",
                params![content_hash, remote.id],
                |row| row.get(0),
            )
            .optional()?;

        if existing_id.is_some() {
            // Already present locally, either by payload or because this exact
            // server row was pulled before. The caller applies remote starred
            // state separately so unpushed local changes are not clobbered.
            tx.commit()?;
            return Ok(false);
        }

        tx.execute(
            "INSERT INTO entries (id, content_type, text_content, text_plain, text_html, text_rtf, search_text, blob_hash, blob_size, content_hash, source_app, starred, sensitive, synced, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 1, ?14, ?15)",
            params![
                remote.id,
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
                starred as i32,
                sensitive as i32,
                remote.created_at.to_rfc3339(),
                remote.updated_at.to_rfc3339(),
            ],
        )?;

        tx.commit()?;
        Ok(true)
    }

    /// Write blob bytes into the content-addressed blob directory.
    ///
    /// Returns the `(hash, size)` pair to store on the entry row, or
    /// `(None, None)` when there is no blob.
    fn write_blob(
        &self,
        blob_data: Option<&[u8]>,
    ) -> anyhow::Result<(Option<String>, Option<u64>)> {
        let Some(data) = blob_data else {
            return Ok((None, None));
        };

        let hash = copywraith_core::content::hash_bytes(data);
        // Validate hash before using as filename (defense in depth)
        if !copywraith_core::content::is_valid_hash(&hash) {
            anyhow::bail!("Generated invalid blob hash");
        }

        let blob_path = self.blob_dir.join(&hash);
        if !blob_path.exists() {
            std::fs::write(&blob_path, data)?;
        }

        Ok((Some(hash), Some(data.len() as u64)))
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

        // Tombstones are sync bookkeeping, never shown to the user.
        let mut conditions = vec!["deleted_at IS NULL".to_string()];
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

        let where_clause = format!("WHERE {}", conditions.join(" AND "));

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
            .query_map(param_refs.as_slice(), row_to_entry)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(entries)
    }

    pub fn get_entry(&self, id: &str) -> anyhow::Result<Option<ClipboardEntry>> {
        let db = self.db.lock().unwrap();
        let entry = db
            .query_row(
                &format!(
                    "SELECT {} FROM entries WHERE id = ?1 AND deleted_at IS NULL",
                    ENTRY_SELECT_COLUMNS
                ),
                params![id],
                row_to_entry,
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

    /// Delete an entry locally, leaving a tombstone for the server push.
    ///
    /// A local-only hard delete never reached the server: the row stayed there,
    /// and `Reset Sync Cursor` — offered to users as "Mobile Sync Repair" —
    /// re-pulled the full history and resurrected everything they had deleted.
    ///
    /// The row is kept as metadata with `synced = 0`, so the ordinary push loop
    /// picks it up and issues the server-side DELETE.
    pub fn delete_entry(&self, id: &str) -> anyhow::Result<bool> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;

        let blob_hash: Option<String> = tx
            .query_row(
                "SELECT blob_hash FROM entries WHERE id = ?1 AND deleted_at IS NULL",
                params![id],
                |row| row.get(0),
            )
            .optional()?
            .flatten();

        let now = Utc::now().to_rfc3339();
        let rows = tx.execute(
            "UPDATE entries
             SET deleted_at = ?1,
                 updated_at = ?1,
                 synced = 0,
                 text_content = NULL,
                 text_plain = NULL,
                 text_html = NULL,
                 text_rtf = NULL,
                 search_text = NULL,
                 blob_hash = NULL,
                 blob_size = NULL,
                 starred = 0
             WHERE id = ?2 AND deleted_at IS NULL",
            params![now, id],
        )?;

        let orphaned = if rows > 0 {
            match blob_hash {
                Some(hash) => {
                    let count: i64 = tx.query_row(
                        "SELECT COUNT(*) FROM entries WHERE blob_hash = ?1",
                        params![hash],
                        |row| row.get(0),
                    )?;
                    (count == 0).then_some(hash)
                }
                None => None,
            }
        } else {
            None
        };

        tx.commit()?;

        // Remove the blob only after the row change is durable, so a crash can
        // never leave a live row pointing at a missing file.
        if let Some(hash) = orphaned {
            let _ = std::fs::remove_file(self.blob_dir.join(hash));
        }

        Ok(rows > 0)
    }

    /// Apply a deletion that happened on another device.
    ///
    /// Marked `synced = 1`: the server already knows, so this must not be
    /// pushed back.
    pub fn apply_remote_deletion(&self, id: &str, deleted_at: &str) -> anyhow::Result<bool> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;

        let blob_hash: Option<String> = tx
            .query_row(
                "SELECT blob_hash FROM entries WHERE id = ?1 AND deleted_at IS NULL",
                params![id],
                |row| row.get(0),
            )
            .optional()?
            .flatten();

        let rows = tx.execute(
            "UPDATE entries
             SET deleted_at = ?1,
                 updated_at = ?1,
                 synced = 1,
                 text_content = NULL,
                 text_plain = NULL,
                 text_html = NULL,
                 text_rtf = NULL,
                 search_text = NULL,
                 blob_hash = NULL,
                 blob_size = NULL,
                 starred = 0
             WHERE id = ?2 AND deleted_at IS NULL",
            params![deleted_at, id],
        )?;

        let orphaned = if rows > 0 {
            match blob_hash {
                Some(hash) => {
                    let count: i64 = tx.query_row(
                        "SELECT COUNT(*) FROM entries WHERE blob_hash = ?1",
                        params![hash],
                        |row| row.get(0),
                    )?;
                    (count == 0).then_some(hash)
                }
                None => None,
            }
        } else {
            None
        };

        tx.commit()?;

        if let Some(hash) = orphaned {
            let _ = std::fs::remove_file(self.blob_dir.join(hash));
        }

        Ok(rows > 0)
    }

    /// Ids of locally deleted entries still waiting to be pushed to the server.
    pub fn get_pending_deletions(&self) -> anyhow::Result<Vec<String>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id FROM entries
             WHERE deleted_at IS NOT NULL AND synced = 0
             ORDER BY deleted_at ASC
             LIMIT 100",
        )?;
        let ids = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ids)
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
                    "SELECT {} FROM entries WHERE deleted_at IS NULL ORDER BY updated_at DESC LIMIT 1",
                    ENTRY_SELECT_COLUMNS
                ),
                [],
                row_to_entry,
            )
            .optional()?;
        Ok(entry)
    }

    pub fn get_unsynced_entries(&self) -> anyhow::Result<Vec<ClipboardEntry>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(&format!(
            "SELECT {} FROM entries WHERE synced = 0 AND deleted_at IS NULL ORDER BY created_at ASC LIMIT 50",
            ENTRY_SELECT_COLUMNS
        ))?;

        let entries = stmt
            .query_map([], row_to_entry)?
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

    /// Returns the persisted pull watermark as `(updated_at, id)`.
    ///
    /// The watermark is the newest `(updated_at, id)` we have fully pulled from
    /// the server. Storing both halves (rather than just an id) lets the pull
    /// loop stop at a stable position even when an entry's `updated_at` changes
    /// (e.g. it was re-copied or re-starred and moved back to the top).
    pub fn get_sync_watermark(&self) -> Option<(String, String)> {
        let db = self.db.lock().unwrap();
        let updated_at = db
            .query_row(
                "SELECT value FROM settings WHERE key = 'sync_watermark_updated_at'",
                [],
                |row| row.get::<_, String>(0),
            )
            .ok()
            .filter(|s| !s.is_empty())?;
        let id = db
            .query_row(
                "SELECT value FROM settings WHERE key = 'sync_watermark_id'",
                [],
                |row| row.get::<_, String>(0),
            )
            .ok()
            .filter(|s| !s.is_empty())?;
        Some((updated_at, id))
    }

    pub fn save_sync_watermark(&self, updated_at: &str, id: &str) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('sync_watermark_updated_at', ?1)",
            params![updated_at],
        )?;
        db.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('sync_watermark_id', ?1)",
            params![id],
        )?;
        Ok(())
    }

    pub fn clear_sync_watermark(&self) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "DELETE FROM settings WHERE key IN ('sync_watermark_updated_at', 'sync_watermark_id', 'sync_last_seen_server_id')",
            [],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_storage() -> (tempfile::TempDir, LocalStorage) {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalStorage::new(dir.path()).unwrap();
        (dir, storage)
    }

    /// Parse an RFC3339 timestamp for use in a RemoteEntryIdentity.
    fn ts(value: &str) -> chrono::DateTime<Utc> {
        chrono::DateTime::parse_from_rfc3339(value)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn text_flavors(text: &str) -> ClipboardFlavors {
        ClipboardFlavors {
            text_plain: Some(text.to_string()),
            ..ClipboardFlavors::default()
        }
    }

    #[test]
    fn insert_remote_entry_creates_a_synced_row() {
        let (_dir, storage) = temp_storage();
        let flavors = text_flavors("from another device");
        let hash = flavors.payload_hash(ContentType::Text, None);

        let inserted = storage
            .insert_remote_entry(
                RemoteEntryIdentity {
                    id: "01JQZ0R0000000000000000001",
                    created_at: ts("2026-07-01T10:00:00Z"),
                    updated_at: ts("2026-07-01T10:00:00Z"),
                },
                ContentType::Text,
                &flavors,
                None,
                &hash,
                None,
                false,
            )
            .unwrap();
        assert!(inserted);

        // A pulled entry must not be queued for push-back to the server.
        assert!(storage.get_unsynced_entries().unwrap().is_empty());
        assert!(storage.has_content_hash(&hash).unwrap());
    }

    #[test]
    fn insert_remote_entry_applies_starred_in_the_same_transaction() {
        let (_dir, storage) = temp_storage();
        let flavors = text_flavors("starred elsewhere");
        let hash = flavors.payload_hash(ContentType::Text, None);

        assert!(storage
            .insert_remote_entry(
                RemoteEntryIdentity {
                    id: "01JQZ0R0000000000000000002",
                    created_at: ts("2026-07-01T10:00:00Z"),
                    updated_at: ts("2026-07-01T10:00:00Z"),
                },
                ContentType::Text,
                &flavors,
                None,
                &hash,
                None,
                true,
            )
            .unwrap());

        let entries = storage.get_entries(10, 0, true, None).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].starred);
    }

    #[test]
    fn insert_remote_entry_is_idempotent_for_known_content() {
        let (_dir, storage) = temp_storage();
        let flavors = text_flavors("seen before");
        let hash = flavors.payload_hash(ContentType::Text, None);

        let identity = RemoteEntryIdentity {
            id: "01JQZ0R0000000000000000003",
            created_at: ts("2026-07-01T10:00:00Z"),
            updated_at: ts("2026-07-01T10:00:00Z"),
        };
        assert!(storage
            .insert_remote_entry(
                identity,
                ContentType::Text,
                &flavors,
                None,
                &hash,
                None,
                false
            )
            .unwrap());
        // Re-ingesting the same payload must not create a second row, so a
        // restarted or retried pull is safe to replay.
        assert!(!storage
            .insert_remote_entry(
                identity,
                ContentType::Text,
                &flavors,
                None,
                &hash,
                None,
                false
            )
            .unwrap());

        assert_eq!(storage.get_entries(10, 0, false, None).unwrap().len(), 1);
    }

    #[test]
    fn insert_remote_entry_does_not_clobber_unpushed_local_state() {
        let (_dir, storage) = temp_storage();
        let flavors = text_flavors("captured locally");
        let hash = flavors.payload_hash(ContentType::Text, None);

        let local = storage
            .insert_entry(ContentType::Text, &flavors, None, &hash, None)
            .unwrap()
            .expect("local capture inserts a row");
        storage.toggle_star(&local.id).unwrap();

        // The server still has this entry unstarred. Ingesting it must leave the
        // pending local star alone; push reconciles it later.
        assert!(!storage
            .insert_remote_entry(
                RemoteEntryIdentity {
                    id: "01JQZ0R0000000000000000004",
                    created_at: ts("2026-07-01T10:00:00Z"),
                    updated_at: ts("2026-07-01T10:00:00Z"),
                },
                ContentType::Text,
                &flavors,
                None,
                &hash,
                None,
                false,
            )
            .unwrap());
        assert!(!storage
            .apply_remote_star_state_by_content_hash(&hash, false)
            .unwrap());

        let entry = storage.get_entry(&local.id).unwrap().unwrap();
        assert!(entry.starred);
    }

    #[test]
    fn insert_remote_entry_preserves_server_id_and_timestamps() {
        let (_dir, storage) = temp_storage();
        let flavors = text_flavors("copied last week");
        let hash = flavors.payload_hash(ContentType::Text, None);

        assert!(storage
            .insert_remote_entry(
                RemoteEntryIdentity {
                    id: "01JQZ0R0000000000000000010",
                    created_at: ts("2026-07-01T09:00:00Z"),
                    updated_at: ts("2026-07-02T11:30:00Z"),
                },
                ContentType::Text,
                &flavors,
                None,
                &hash,
                None,
                false,
            )
            .unwrap());

        // An entry must be the same entry on every device, and must keep the
        // time it was actually copied rather than the time it was pulled.
        let entry = storage
            .get_entry("01JQZ0R0000000000000000010")
            .unwrap()
            .expect("row is stored under the server id");
        assert_eq!(entry.created_at, ts("2026-07-01T09:00:00Z"));
        assert_eq!(entry.updated_at, ts("2026-07-02T11:30:00Z"));
    }

    #[test]
    fn pulled_entries_keep_server_chronology() {
        let (_dir, storage) = temp_storage();

        // The server returns newest-first, so the pull loop ingests in that
        // order. Previously each insert stamped `now`, which meant the oldest
        // entry was written last and sorted to the top — a fresh install showed
        // its history reversed.
        let page = [
            (
                "01JQZ0R0000000000000000030",
                "newest",
                "2026-07-03T12:00:00Z",
            ),
            (
                "01JQZ0R0000000000000000020",
                "middle",
                "2026-07-02T12:00:00Z",
            ),
            (
                "01JQZ0R0000000000000000010",
                "oldest",
                "2026-07-01T12:00:00Z",
            ),
        ];

        for (id, text, at) in page {
            let flavors = text_flavors(text);
            let hash = flavors.payload_hash(ContentType::Text, None);
            assert!(storage
                .insert_remote_entry(
                    RemoteEntryIdentity {
                        id,
                        created_at: ts(at),
                        updated_at: ts(at),
                    },
                    ContentType::Text,
                    &flavors,
                    None,
                    &hash,
                    None,
                    false,
                )
                .unwrap());
        }

        let listed = storage.get_entries(10, 0, false, None).unwrap();
        let order: Vec<String> = listed
            .iter()
            .map(|e| e.best_plain_text().unwrap_or_default())
            .collect();
        assert_eq!(order, ["newest", "middle", "oldest"]);

        // "Paste most recent" must pick the genuinely newest entry.
        let most_recent = storage.get_most_recent_entry().unwrap().unwrap();
        assert_eq!(most_recent.best_plain_text().as_deref(), Some("newest"));
    }

    #[test]
    fn re_pulling_the_same_server_row_is_a_no_op() {
        let (_dir, storage) = temp_storage();
        let flavors = text_flavors("pulled twice");
        let hash = flavors.payload_hash(ContentType::Text, None);
        let identity = RemoteEntryIdentity {
            id: "01JQZ0R0000000000000000040",
            created_at: ts("2026-07-01T09:00:00Z"),
            updated_at: ts("2026-07-01T09:00:00Z"),
        };

        assert!(storage
            .insert_remote_entry(
                identity,
                ContentType::Text,
                &flavors,
                None,
                &hash,
                None,
                false
            )
            .unwrap());

        // Same id, different payload: the primary key must not collide-and-fail.
        let other = text_flavors("different payload");
        let other_hash = other.payload_hash(ContentType::Text, None);
        assert!(!storage
            .insert_remote_entry(
                identity,
                ContentType::Text,
                &other,
                None,
                &other_hash,
                None,
                false
            )
            .unwrap());
        assert_eq!(storage.get_entries(10, 0, false, None).unwrap().len(), 1);
    }

    #[test]
    fn deleting_leaves_a_tombstone_queued_for_the_server() {
        let (_dir, storage) = temp_storage();
        let flavors = text_flavors("delete me");
        let hash = flavors.payload_hash(ContentType::Text, None);
        let entry = storage
            .insert_entry(ContentType::Text, &flavors, None, &hash, None)
            .unwrap()
            .unwrap();
        storage.mark_synced(&entry.id).unwrap();

        assert!(storage.delete_entry(&entry.id).unwrap());

        // Gone from every user-facing read...
        assert!(storage.get_entries(10, 0, false, None).unwrap().is_empty());
        assert!(storage.get_entry(&entry.id).unwrap().is_none());
        assert!(storage.get_most_recent_entry().unwrap().is_none());

        // ...but retained as a deletion still owed to the server. Without this
        // the delete never propagated and a cursor reset resurrected it.
        assert_eq!(storage.get_pending_deletions().unwrap(), vec![entry.id]);

        // A tombstone is not content, so the create-push must not pick it up.
        assert!(storage.get_unsynced_entries().unwrap().is_empty());
    }

    #[test]
    fn a_pushed_deletion_stops_being_pending() {
        let (_dir, storage) = temp_storage();
        let flavors = text_flavors("delete me");
        let hash = flavors.payload_hash(ContentType::Text, None);
        let entry = storage
            .insert_entry(ContentType::Text, &flavors, None, &hash, None)
            .unwrap()
            .unwrap();

        storage.delete_entry(&entry.id).unwrap();
        storage.mark_synced(&entry.id).unwrap();

        assert!(storage.get_pending_deletions().unwrap().is_empty());
    }

    #[test]
    fn a_tombstone_blocks_the_entry_being_pulled_back() {
        let (_dir, storage) = temp_storage();
        let flavors = text_flavors("deleted here, still on the server");
        let hash = flavors.payload_hash(ContentType::Text, None);
        let entry = storage
            .insert_entry(ContentType::Text, &flavors, None, &hash, None)
            .unwrap()
            .unwrap();
        storage.delete_entry(&entry.id).unwrap();

        // The server has not been told yet, so a pull still offers the entry.
        // Re-ingesting it would undo the user's delete in front of them.
        assert!(!storage
            .insert_remote_entry(
                RemoteEntryIdentity {
                    id: &entry.id,
                    created_at: ts("2026-07-01T10:00:00Z"),
                    updated_at: ts("2026-07-01T10:00:00Z"),
                },
                ContentType::Text,
                &flavors,
                None,
                &hash,
                None,
                false,
            )
            .unwrap());
        assert!(storage.get_entries(10, 0, false, None).unwrap().is_empty());
    }

    #[test]
    fn a_remote_deletion_removes_the_local_copy_without_queueing_a_push() {
        let (_dir, storage) = temp_storage();
        let flavors = text_flavors("deleted on another device");
        let hash = flavors.payload_hash(ContentType::Text, None);
        let entry = storage
            .insert_entry(ContentType::Text, &flavors, None, &hash, None)
            .unwrap()
            .unwrap();
        storage.mark_synced(&entry.id).unwrap();

        assert!(storage
            .apply_remote_deletion(&entry.id, "2026-07-04T08:00:00Z")
            .unwrap());

        assert!(storage.get_entries(10, 0, false, None).unwrap().is_empty());
        // The server already knows; echoing it back would be pointless work.
        assert!(storage.get_pending_deletions().unwrap().is_empty());
    }

    #[test]
    fn deleting_removes_an_unreferenced_blob() {
        let (dir, storage) = temp_storage();
        let bytes = b"\x89PNG\r\n\x1a\n blob to drop";
        let hash = copywraith_core::content::hash_bytes(bytes);
        let entry = storage
            .insert_entry(
                ContentType::Image,
                &ClipboardFlavors::default(),
                Some(bytes),
                &hash,
                None,
            )
            .unwrap()
            .unwrap();

        assert!(dir.path().join("blobs").join(&hash).exists());
        storage.delete_entry(&entry.id).unwrap();
        assert!(!dir.path().join("blobs").join(&hash).exists());
    }

    #[test]
    fn insert_remote_entry_stores_blob_bytes_once() {
        let (dir, storage) = temp_storage();
        let bytes = b"\x89PNG\r\n\x1a\n fake image payload";
        let hash = copywraith_core::content::hash_bytes(bytes);

        assert!(storage
            .insert_remote_entry(
                RemoteEntryIdentity {
                    id: "01JQZ0R0000000000000000005",
                    created_at: ts("2026-07-01T10:00:00Z"),
                    updated_at: ts("2026-07-01T10:00:00Z"),
                },
                ContentType::Image,
                &ClipboardFlavors::default(),
                Some(bytes),
                &hash,
                None,
                false,
            )
            .unwrap());

        assert_eq!(
            storage.get_blob(&hash).unwrap().as_deref(),
            Some(&bytes[..])
        );
        assert!(dir.path().join("blobs").join(&hash).exists());
    }
}
