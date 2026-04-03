use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::Utc;
use copywraith_core::content::{base64_to_bytes, hash_bytes};
use copywraith_core::models::{ClipboardEntry, ClipboardFlavors, ContentType};
use copywraith_core::sensitive::contains_sensitive_data;
use rusqlite::{params, Connection, OptionalExtension};
use ulid::Ulid;

use crate::crypto;

pub struct Storage {
    db: Mutex<Connection>,
    blob_dir: PathBuf,
}

const ENTRY_SELECT_COLUMNS: &str =
    "id, content_type, text_content, text_plain, text_html, text_rtf, blob_hash, blob_size, source_app, starred, sensitive, created_at, updated_at";

const ENTRIES_FTS_SCHEMA_VERSION_KEY: &str = "entries_fts_schema_version";
const ENTRIES_FTS_SCHEMA_VERSION: i64 = 2;
const ENCRYPTED_TEXT_PREFIX_LIKE: &str = "ENC:1:%";

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

    let mut stmt = conn.prepare(
        "SELECT id, content_type, text_content, text_plain, text_html, text_rtf, search_text FROM entries",
    )?;

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);

    for (id, content_type_raw, text_content, text_plain, text_html, text_rtf, search_text) in rows {
        let content_type = content_type_raw
            .parse::<ContentType>()
            .unwrap_or(ContentType::Text);

        let prev_text_plain = text_plain.clone();
        let prev_text_html = text_html.clone();
        let prev_text_rtf = text_rtf.clone();

        let flavors = ClipboardFlavors {
            text_plain,
            text_html,
            text_rtf,
            file_list: None,
        }
        .merge_legacy(content_type, text_content.as_deref());

        let next_text_content = flavors.to_legacy_text_content(content_type);
        let next_search_text = flavors.best_plain_text();
        let next_text_plain = flavors.text_plain.clone();
        let next_text_html = flavors.text_html.clone();
        let next_text_rtf = flavors.text_rtf.clone();

        if text_content != next_text_content
            || search_text != next_search_text
            || prev_text_plain != next_text_plain
            || prev_text_html != next_text_html
            || prev_text_rtf != next_text_rtf
        {
            conn.execute(
                "UPDATE entries
                 SET text_content = ?1,
                     text_plain = ?2,
                     text_html = ?3,
                     text_rtf = ?4,
                     search_text = ?5
                 WHERE id = ?6",
                params![
                    next_text_content,
                    next_text_plain,
                    next_text_html,
                    next_text_rtf,
                    next_search_text,
                    id
                ],
            )?;
        }
    }

    Ok(())
}

fn rebuild_entries_fts(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(
        &format!(
            "
        DROP TRIGGER IF EXISTS entries_ai;
        DROP TRIGGER IF EXISTS entries_ad;
        DROP TRIGGER IF EXISTS entries_au;
        DROP TABLE IF EXISTS entries_fts;

        CREATE VIRTUAL TABLE entries_fts USING fts5(
            search_text,
            content='entries',
            content_rowid='rowid'
        );

        CREATE TRIGGER entries_ai AFTER INSERT ON entries
        WHEN new.search_text IS NOT NULL AND new.search_text NOT LIKE '{encrypted_prefix}' BEGIN
            INSERT INTO entries_fts(rowid, search_text) VALUES (new.rowid, new.search_text);
        END;
        CREATE TRIGGER entries_ad AFTER DELETE ON entries BEGIN
            INSERT INTO entries_fts(entries_fts, rowid, search_text)
            SELECT 'delete', old.rowid, old.search_text
            WHERE old.search_text IS NOT NULL AND old.search_text NOT LIKE '{encrypted_prefix}';
        END;
        CREATE TRIGGER entries_au AFTER UPDATE ON entries BEGIN
            INSERT INTO entries_fts(entries_fts, rowid, search_text)
            SELECT 'delete', old.rowid, old.search_text
            WHERE old.search_text IS NOT NULL AND old.search_text NOT LIKE '{encrypted_prefix}';
            INSERT INTO entries_fts(rowid, search_text)
            SELECT new.rowid, new.search_text
            WHERE new.search_text IS NOT NULL AND new.search_text NOT LIKE '{encrypted_prefix}';
        END;
        ",
            encrypted_prefix = ENCRYPTED_TEXT_PREFIX_LIKE,
        ),
    )?;

    conn.execute(
        "INSERT INTO entries_fts(rowid, search_text)
         SELECT rowid, search_text
         FROM entries
         WHERE search_text IS NOT NULL
           AND search_text NOT LIKE ?1",
        params![ENCRYPTED_TEXT_PREFIX_LIKE],
    )?;

    conn.execute(
        "INSERT INTO entries_fts(entries_fts) VALUES('optimize')",
        [],
    )?;

    Ok(())
}

fn sqlite_object_exists(
    conn: &Connection,
    object_type: &str,
    object_name: &str,
) -> anyhow::Result<bool> {
    let exists: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = ?1 AND name = ?2 LIMIT 1",
            params![object_type, object_name],
            |row| row.get(0),
        )
        .optional()?;

    Ok(exists.is_some())
}

fn ensure_entries_fts_schema(conn: &Connection) -> anyhow::Result<()> {
    let stored_version = conn
        .query_row(
            "SELECT value FROM metadata WHERE key = ?1",
            params![ENTRIES_FTS_SCHEMA_VERSION_KEY],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .and_then(|value| value.parse::<i64>().ok());

    let fts_objects_present = sqlite_object_exists(conn, "table", "entries_fts")?
        && sqlite_object_exists(conn, "trigger", "entries_ai")?
        && sqlite_object_exists(conn, "trigger", "entries_ad")?
        && sqlite_object_exists(conn, "trigger", "entries_au")?;

    if stored_version == Some(ENTRIES_FTS_SCHEMA_VERSION) && fts_objects_present {
        return Ok(());
    }

    rebuild_entries_fts(conn)?;
    conn.execute(
        "INSERT INTO metadata (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![
            ENTRIES_FTS_SCHEMA_VERSION_KEY,
            ENTRIES_FTS_SCHEMA_VERSION.to_string()
        ],
    )?;

    Ok(())
}

fn encrypt_optional_text(dek: &[u8; 32], value: Option<String>) -> anyhow::Result<Option<String>> {
    match value {
        Some(text) if crypto::is_encrypted_text(&text) => Ok(Some(text)),
        Some(text) => Ok(Some(crypto::encrypt_text(dek, &text)?)),
        None => Ok(None),
    }
}

fn decrypt_optional_text(dek: &[u8; 32], value: Option<String>) -> anyhow::Result<Option<String>> {
    match value {
        Some(text) if !crypto::is_encrypted_text(&text) => Ok(Some(text)),
        Some(text) => Ok(Some(crypto::decrypt_text(dek, &text)?)),
        None => Ok(None),
    }
}

fn decrypt_entry_text_fields(entry: &mut ClipboardEntry, dek: &[u8; 32]) -> anyhow::Result<()> {
    entry.text_content = decrypt_optional_text(dek, entry.text_content.take())?;
    entry.flavors.text_plain = decrypt_optional_text(dek, entry.flavors.text_plain.take())?;
    entry.flavors.text_html = decrypt_optional_text(dek, entry.flavors.text_html.take())?;
    entry.flavors.text_rtf = decrypt_optional_text(dek, entry.flavors.text_rtf.take())?;

    entry.flavors = entry
        .flavors
        .clone()
        .merge_legacy(entry.content_type, entry.text_content.as_deref());
    entry.text_content = entry
        .text_content
        .clone()
        .or_else(|| entry.flavors.to_legacy_text_content(entry.content_type));

    Ok(())
}

impl Storage {
    pub fn new(data_dir: &Path) -> anyhow::Result<Self> {
        let db_path = data_dir.join("copywraith.db");
        let blob_dir = data_dir.join("blobs");
        std::fs::create_dir_all(&blob_dir)?;

        let conn = Connection::open(&db_path)?;
        conn.execute_batch(
            "
            PRAGMA journal_mode=WAL;
            PRAGMA foreign_keys=ON;

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
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS metadata (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_entries_created_at ON entries(created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_entries_updated_at ON entries(updated_at DESC);
            CREATE INDEX IF NOT EXISTS idx_entries_starred ON entries(starred) WHERE starred = 1;
            CREATE INDEX IF NOT EXISTS idx_entries_content_type ON entries(content_type);
            CREATE UNIQUE INDEX IF NOT EXISTS idx_entries_content_hash ON entries(content_hash);

            CREATE VIRTUAL TABLE IF NOT EXISTS entries_fts USING fts5(
                search_text,
                content='entries',
                content_rowid='rowid'
            );

            CREATE TRIGGER IF NOT EXISTS entries_ai AFTER INSERT ON entries
            WHEN new.search_text IS NOT NULL AND new.search_text NOT LIKE 'ENC:1:%' BEGIN
                INSERT INTO entries_fts(rowid, search_text) VALUES (new.rowid, new.search_text);
            END;
            CREATE TRIGGER IF NOT EXISTS entries_ad AFTER DELETE ON entries BEGIN
                INSERT INTO entries_fts(entries_fts, rowid, search_text)
                SELECT 'delete', old.rowid, old.search_text
                WHERE old.search_text IS NOT NULL AND old.search_text NOT LIKE 'ENC:1:%';
            END;
            CREATE TRIGGER IF NOT EXISTS entries_au AFTER UPDATE ON entries BEGIN
                INSERT INTO entries_fts(entries_fts, rowid, search_text)
                SELECT 'delete', old.rowid, old.search_text
                WHERE old.search_text IS NOT NULL AND old.search_text NOT LIKE 'ENC:1:%';
                INSERT INTO entries_fts(rowid, search_text)
                SELECT new.rowid, new.search_text
                WHERE new.search_text IS NOT NULL AND new.search_text NOT LIKE 'ENC:1:%';
            END;
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
        ensure_entries_fts_schema(&conn)?;

        Ok(Self {
            db: Mutex::new(conn),
            blob_dir,
        })
    }

    pub fn create_entry(
        &self,
        content_type: ContentType,
        flavors: &ClipboardFlavors,
        blob_base64: Option<&str>,
        source_app: Option<&str>,
        starred: Option<bool>,
        content_hash: &str,
        dek: Option<&[u8; 32]>,
    ) -> anyhow::Result<(ClipboardEntry, bool)> {
        let db = self.db.lock().unwrap();

        // Check for existing entry with same content hash
        let existing: Option<ClipboardEntry> = db
            .query_row(
                &format!(
                    "SELECT {} FROM entries WHERE content_hash = ?1",
                    ENTRY_SELECT_COLUMNS
                ),
                params![content_hash],
                |row| row_to_entry(row),
            )
            .ok();

        if let Some(mut entry) = existing {
            // Update timestamp to bring to top
            let now = Utc::now();
            let next_starred = starred.unwrap_or(entry.starred);
            entry.starred = next_starred;
            entry.updated_at = now;
            db.execute(
                "UPDATE entries SET updated_at = ?1, starred = ?2 WHERE id = ?3",
                params![now.to_rfc3339(), next_starred as i32, entry.id],
            )?;
            // Decrypt text flavors for the response
            if let Some(dek) = dek {
                decrypt_entry_text_fields(&mut entry, dek)?;
            }
            return Ok((entry, false));
        }

        let resolved_flavors = flavors.clone().merge_legacy(content_type, None);
        let legacy_text_content = resolved_flavors.to_legacy_text_content(content_type);
        let search_text = resolved_flavors.best_plain_text();

        // Process blob data
        let (blob_hash, blob_size) = if let Some(b64) = blob_base64 {
            let bytes = base64_to_bytes(b64)?;
            let hash = hash_bytes(&bytes);
            let size = bytes.len() as u64;

            // Encrypt blob if DEK is available, then write to disk
            if !copywraith_core::content::is_valid_hash(&hash) {
                anyhow::bail!("Generated invalid blob hash");
            }
            let blob_path = self.blob_dir.join(&hash);
            if !blob_path.exists() {
                let to_write = if let Some(dek) = dek {
                    crypto::encrypt_blob(dek, &bytes)?
                } else {
                    bytes
                };
                std::fs::write(&blob_path, &to_write)?;
            }

            (Some(hash), Some(size))
        } else {
            (None, None)
        };

        let now = Utc::now();
        let id = Ulid::new().to_string();
        let starred = starred.unwrap_or(false);

        let sensitive = search_text
            .as_ref()
            .map(|t| contains_sensitive_data(t))
            .unwrap_or(false);

        // Encrypt text fields if DEK is available
        let (
            stored_text_content,
            stored_text_plain,
            stored_text_html,
            stored_text_rtf,
            stored_search_text,
        ) = if let Some(dek) = dek {
            (
                encrypt_optional_text(dek, legacy_text_content.clone())?,
                encrypt_optional_text(dek, resolved_flavors.text_plain.clone())?,
                encrypt_optional_text(dek, resolved_flavors.text_html.clone())?,
                encrypt_optional_text(dek, resolved_flavors.text_rtf.clone())?,
                encrypt_optional_text(dek, search_text.clone())?,
            )
        } else {
            (
                legacy_text_content.clone(),
                resolved_flavors.text_plain.clone(),
                resolved_flavors.text_html.clone(),
                resolved_flavors.text_rtf.clone(),
                search_text.clone(),
            )
        };

        db.execute(
            "INSERT INTO entries (id, content_type, text_content, text_plain, text_html, text_rtf, search_text, blob_hash, blob_size, content_hash, source_app, starred, sensitive, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                id,
                content_type.as_str(),
                stored_text_content,
                stored_text_plain,
                stored_text_html,
                stored_text_rtf,
                stored_search_text,
                blob_hash,
                blob_size.map(|s| s as i64),
                content_hash,
                source_app,
                starred as i32,
                sensitive as i32,
                now.to_rfc3339(),
                now.to_rfc3339(),
            ],
        )?;

        let entry = ClipboardEntry {
            id,
            content_type,
            text_content: legacy_text_content,
            blob_hash,
            blob_size,
            source_app: source_app.map(|s| s.to_string()),
            flavors: resolved_flavors,
            starred,
            sensitive,
            created_at: now,
            updated_at: now,
        };

        Ok((entry, true))
    }

    pub fn get_entry(
        &self,
        id: &str,
        dek: Option<&[u8; 32]>,
    ) -> anyhow::Result<Option<ClipboardEntry>> {
        let db = self.db.lock().unwrap();
        let entry = db
            .query_row(
                &format!("SELECT {} FROM entries WHERE id = ?1", ENTRY_SELECT_COLUMNS),
                params![id],
                |row| row_to_entry(row),
            )
            .ok();

        match (entry, dek) {
            (Some(mut e), Some(dek)) => {
                decrypt_entry_text_fields(&mut e, dek)?;
                Ok(Some(e))
            }
            (e, _) => Ok(e),
        }
    }

    pub fn list_entries(
        &self,
        limit: u32,
        offset: u32,
        content_type: Option<ContentType>,
        starred_only: bool,
        search: Option<&str>,
        dek: Option<&[u8; 32]>,
    ) -> anyhow::Result<(Vec<ClipboardEntry>, u64)> {
        let db = self.db.lock().unwrap();

        // When encryption is active and a search term is provided, we can't use
        // FTS on ciphertext. Fall back to in-memory substring search.
        let encryption_active = dek.is_some();
        let use_fts = search.is_some() && !encryption_active;
        let memory_search = search.is_some() && encryption_active;
        let search_term = search.map(|s| s.to_lowercase());

        let mut conditions = Vec::new();
        let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(ct) = content_type {
            conditions.push(format!("e.content_type = ?{}", params_vec.len() + 1));
            params_vec.push(Box::new(ct.as_str().to_string()));
        }

        if starred_only {
            conditions.push("e.starred = 1".to_string());
        }

        if use_fts {
            if let Some(q) = search {
                conditions.push(format!("f.search_text MATCH ?{}", params_vec.len() + 1));
                params_vec.push(Box::new(q.to_string()));
            }
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let fts_join = if use_fts {
            "JOIN entries_fts f ON f.rowid = e.rowid"
        } else {
            ""
        };

        if memory_search {
            // Encrypted search: load all matching entries, decrypt, filter, paginate in memory
            let query_sql = format!(
                "SELECT {}
                 FROM entries e {} {}
                 ORDER BY e.updated_at DESC",
                ENTRY_SELECT_COLUMNS, fts_join, where_clause,
            );

            let mut stmt = db.prepare(&query_sql)?;
            let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                params_vec.iter().map(|p| p.as_ref()).collect();

            let all_entries = stmt
                .query_map(param_refs.as_slice(), |row| row_to_entry(row))?
                .collect::<Result<Vec<_>, _>>()?;

            let dek = dek.unwrap(); // safe: memory_search implies dek.is_some()
            let mut filtered = Vec::new();
            for mut entry in all_entries {
                if decrypt_entry_text_fields(&mut entry, dek).is_err() {
                    continue;
                }

                if let Some(plain) = entry.best_plain_text() {
                    if plain
                        .to_lowercase()
                        .contains(search_term.as_deref().unwrap_or(""))
                    {
                        filtered.push(entry);
                    }
                }
            }

            let total = filtered.len() as u64;
            let start = offset as usize;
            let end = (start + limit as usize).min(filtered.len());
            let page = if start < filtered.len() {
                filtered[start..end].to_vec()
            } else {
                Vec::new()
            };

            Ok((page, total))
        } else {
            // Normal path (plaintext FTS or no search)
            let count_sql = format!(
                "SELECT COUNT(*) FROM entries e {} {}",
                fts_join, where_clause
            );
            let total: u64 = {
                let mut stmt = db.prepare(&count_sql)?;
                let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                    params_vec.iter().map(|p| p.as_ref()).collect();
                stmt.query_row(param_refs.as_slice(), |row| row.get::<_, i64>(0))? as u64
            };

            let query_sql = format!(
                "SELECT {}
                 FROM entries e {} {}
                 ORDER BY e.updated_at DESC
                 LIMIT ?{} OFFSET ?{}",
                ENTRY_SELECT_COLUMNS,
                fts_join,
                where_clause,
                params_vec.len() + 1,
                params_vec.len() + 2,
            );

            params_vec.push(Box::new(limit as i64));
            params_vec.push(Box::new(offset as i64));

            let mut stmt = db.prepare(&query_sql)?;
            let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                params_vec.iter().map(|p| p.as_ref()).collect();

            let mut entries = stmt
                .query_map(param_refs.as_slice(), |row| row_to_entry(row))?
                .collect::<Result<Vec<_>, _>>()?;

            // Decrypt text_content if DEK is available
            if let Some(dek) = dek {
                for entry in &mut entries {
                    decrypt_entry_text_fields(entry, dek)?;
                }
            }

            Ok((entries, total))
        }
    }

    pub fn update_entry_starred(&self, id: &str, starred: bool) -> anyhow::Result<bool> {
        let db = self.db.lock().unwrap();
        let now = Utc::now();
        let rows = db.execute(
            "UPDATE entries SET starred = ?1, updated_at = ?2 WHERE id = ?3",
            params![starred as i32, now.to_rfc3339(), id],
        )?;
        Ok(rows > 0)
    }

    pub fn delete_entry(&self, id: &str) -> anyhow::Result<bool> {
        let db = self.db.lock().unwrap();

        // Get blob_hash before deleting to clean up blob file
        let blob_hash: Option<String> = db
            .query_row(
                "SELECT blob_hash FROM entries WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .ok()
            .flatten();

        let rows = db.execute("DELETE FROM entries WHERE id = ?1", params![id])?;

        // Clean up blob file if no other entries reference it
        if let Some(hash) = blob_hash {
            let count: i64 = db.query_row(
                "SELECT COUNT(*) FROM entries WHERE blob_hash = ?1",
                params![hash],
                |row| row.get(0),
            )?;
            if count == 0 {
                let blob_path = self.blob_dir.join(&hash);
                let _ = std::fs::remove_file(blob_path);
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

    pub fn count_entries(&self) -> anyhow::Result<u64> {
        let db = self.db.lock().unwrap();
        let count: i64 = db.query_row("SELECT COUNT(*) FROM entries", [], |row| row.get(0))?;
        Ok(count as u64)
    }

    // -----------------------------------------------------------------------
    // Migration: encrypt existing plaintext data
    // -----------------------------------------------------------------------

    /// Encrypt all existing unencrypted text flavor values in place.
    pub fn encrypt_all_entries(&self, dek: &[u8; 32]) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();

        let mut stmt = db.prepare(
            "SELECT id, text_content, text_plain, text_html, text_rtf, search_text FROM entries",
        )?;

        let rows: Vec<(
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        )> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        for (id, text_content, text_plain, text_html, text_rtf, search_text) in rows {
            let mut changed = false;

            let encrypted_text_content = match text_content {
                Some(value) if !crypto::is_encrypted_text(&value) => {
                    changed = true;
                    Some(crypto::encrypt_text(dek, &value)?)
                }
                other => other,
            };

            let encrypted_text_plain = match text_plain {
                Some(value) if !crypto::is_encrypted_text(&value) => {
                    changed = true;
                    Some(crypto::encrypt_text(dek, &value)?)
                }
                other => other,
            };

            let encrypted_text_html = match text_html {
                Some(value) if !crypto::is_encrypted_text(&value) => {
                    changed = true;
                    Some(crypto::encrypt_text(dek, &value)?)
                }
                other => other,
            };

            let encrypted_text_rtf = match text_rtf {
                Some(value) if !crypto::is_encrypted_text(&value) => {
                    changed = true;
                    Some(crypto::encrypt_text(dek, &value)?)
                }
                other => other,
            };

            let encrypted_search_text = match search_text {
                Some(value) if !crypto::is_encrypted_text(&value) => {
                    changed = true;
                    Some(crypto::encrypt_text(dek, &value)?)
                }
                other => other,
            };

            if changed {
                db.execute(
                    "UPDATE entries
                     SET text_content = ?1,
                         text_plain = ?2,
                         text_html = ?3,
                         text_rtf = ?4,
                         search_text = ?5
                     WHERE id = ?6",
                    params![
                        encrypted_text_content,
                        encrypted_text_plain,
                        encrypted_text_html,
                        encrypted_text_rtf,
                        encrypted_search_text,
                        id
                    ],
                )?;
            }
        }

        Ok(())
    }

    /// Encrypt all existing unencrypted blob files in place.
    pub fn encrypt_all_blobs(&self, dek: &[u8; 32]) -> anyhow::Result<()> {
        let entries = std::fs::read_dir(&self.blob_dir)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let data = std::fs::read(&path)?;
            if crypto::is_encrypted_blob(&data) {
                continue; // already encrypted
            }
            let encrypted = crypto::encrypt_blob(dek, &data)?;
            std::fs::write(&path, &encrypted)?;
        }
        Ok(())
    }
}

/// Helper: parse a SQLite row into a ClipboardEntry.
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
