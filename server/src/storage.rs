use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::Utc;
use copywraith_core::content::{base64_to_bytes, hash_bytes};
use copywraith_core::models::{ClipboardEntry, ContentType};
use rusqlite::{params, Connection};
use ulid::Ulid;

pub struct Storage {
    db: Mutex<Connection>,
    blob_dir: PathBuf,
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
                blob_hash TEXT,
                blob_size INTEGER,
                content_hash TEXT NOT NULL,
                source_app TEXT,
                starred INTEGER DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_entries_created_at ON entries(created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_entries_starred ON entries(starred) WHERE starred = 1;
            CREATE INDEX IF NOT EXISTS idx_entries_content_type ON entries(content_type);
            CREATE UNIQUE INDEX IF NOT EXISTS idx_entries_content_hash ON entries(content_hash);

            CREATE VIRTUAL TABLE IF NOT EXISTS entries_fts USING fts5(
                text_content,
                content='entries',
                content_rowid='rowid'
            );

            CREATE TRIGGER IF NOT EXISTS entries_ai AFTER INSERT ON entries BEGIN
                INSERT INTO entries_fts(rowid, text_content) VALUES (new.rowid, new.text_content);
            END;
            CREATE TRIGGER IF NOT EXISTS entries_ad AFTER DELETE ON entries BEGIN
                INSERT INTO entries_fts(entries_fts, rowid, text_content) VALUES('delete', old.rowid, old.text_content);
            END;
            CREATE TRIGGER IF NOT EXISTS entries_au AFTER UPDATE ON entries BEGIN
                INSERT INTO entries_fts(entries_fts, rowid, text_content) VALUES('delete', old.rowid, old.text_content);
                INSERT INTO entries_fts(rowid, text_content) VALUES (new.rowid, new.text_content);
            END;
            ",
        )?;

        Ok(Self {
            db: Mutex::new(conn),
            blob_dir,
        })
    }

    pub fn create_entry(
        &self,
        content_type: ContentType,
        text_content: Option<&str>,
        blob_base64: Option<&str>,
        source_app: Option<&str>,
        content_hash: &str,
    ) -> anyhow::Result<(ClipboardEntry, bool)> {
        let db = self.db.lock().unwrap();

        // Check for existing entry with same content hash
        let existing: Option<ClipboardEntry> = db
            .query_row(
                "SELECT id, content_type, text_content, blob_hash, blob_size, source_app, starred, created_at, updated_at
                 FROM entries WHERE content_hash = ?1",
                params![content_hash],
                |row| {
                    Ok(ClipboardEntry {
                        id: row.get(0)?,
                        content_type: serde_json::from_str::<ContentType>(&format!("\"{}\"", row.get::<_, String>(1)?)).unwrap_or(ContentType::Text),
                        text_content: row.get(2)?,
                        blob_hash: row.get(3)?,
                        blob_size: row.get::<_, Option<i64>>(4)?.map(|s| s as u64),
                        source_app: row.get(5)?,
                        starred: row.get::<_, i32>(6)? != 0,
                        created_at: row.get::<_, String>(7)?.parse().unwrap_or_else(|_| Utc::now()),
                        updated_at: row.get::<_, String>(8)?.parse().unwrap_or_else(|_| Utc::now()),
                    })
                },
            )
            .ok();

        if let Some(mut entry) = existing {
            // Update timestamp to bring to top
            let now = Utc::now();
            entry.updated_at = now;
            db.execute(
                "UPDATE entries SET updated_at = ?1 WHERE id = ?2",
                params![now.to_rfc3339(), entry.id],
            )?;
            return Ok((entry, false));
        }

        // Process blob data
        let (blob_hash, blob_size) = if let Some(b64) = blob_base64 {
            let bytes = base64_to_bytes(b64)?;
            let hash = hash_bytes(&bytes);
            let size = bytes.len() as u64;

            // Write blob to disk
            let blob_path = self.blob_dir.join(&hash);
            if !blob_path.exists() {
                std::fs::write(&blob_path, &bytes)?;
            }

            (Some(hash), Some(size))
        } else {
            (None, None)
        };

        let now = Utc::now();
        let id = Ulid::new().to_string();

        db.execute(
            "INSERT INTO entries (id, content_type, text_content, blob_hash, blob_size, content_hash, source_app, starred, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9)",
            params![
                id,
                content_type.as_str(),
                text_content,
                blob_hash,
                blob_size.map(|s| s as i64),
                content_hash,
                source_app,
                now.to_rfc3339(),
                now.to_rfc3339(),
            ],
        )?;

        let entry = ClipboardEntry {
            id,
            content_type,
            text_content: text_content.map(|s| s.to_string()),
            blob_hash,
            blob_size,
            source_app: source_app.map(|s| s.to_string()),
            starred: false,
            created_at: now,
            updated_at: now,
        };

        Ok((entry, true))
    }

    pub fn get_entry(&self, id: &str) -> anyhow::Result<Option<ClipboardEntry>> {
        let db = self.db.lock().unwrap();
        let entry = db
            .query_row(
                "SELECT id, content_type, text_content, blob_hash, blob_size, source_app, starred, created_at, updated_at
                 FROM entries WHERE id = ?1",
                params![id],
                |row| {
                    Ok(ClipboardEntry {
                        id: row.get(0)?,
                        content_type: serde_json::from_str::<ContentType>(&format!("\"{}\"", row.get::<_, String>(1)?)).unwrap_or(ContentType::Text),
                        text_content: row.get(2)?,
                        blob_hash: row.get(3)?,
                        blob_size: row.get::<_, Option<i64>>(4)?.map(|s| s as u64),
                        source_app: row.get(5)?,
                        starred: row.get::<_, i32>(6)? != 0,
                        created_at: row.get::<_, String>(7)?.parse().unwrap_or_else(|_| Utc::now()),
                        updated_at: row.get::<_, String>(8)?.parse().unwrap_or_else(|_| Utc::now()),
                    })
                },
            )
            .ok();
        Ok(entry)
    }

    pub fn list_entries(
        &self,
        limit: u32,
        offset: u32,
        content_type: Option<ContentType>,
        starred_only: bool,
        search: Option<&str>,
    ) -> anyhow::Result<(Vec<ClipboardEntry>, u64)> {
        let db = self.db.lock().unwrap();

        let mut conditions = Vec::new();
        let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(ct) = content_type {
            conditions.push(format!("e.content_type = ?{}", params_vec.len() + 1));
            params_vec.push(Box::new(ct.as_str().to_string()));
        }

        if starred_only {
            conditions.push("e.starred = 1".to_string());
        }

        let join_fts = search.is_some();
        if let Some(q) = search {
            conditions.push(format!("f.text_content MATCH ?{}", params_vec.len() + 1));
            params_vec.push(Box::new(q.to_string()));
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let fts_join = if join_fts {
            "JOIN entries_fts f ON f.rowid = e.rowid"
        } else {
            ""
        };

        // Count total
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

        // Fetch entries
        let query_sql = format!(
            "SELECT e.id, e.content_type, e.text_content, e.blob_hash, e.blob_size, e.source_app, e.starred, e.created_at, e.updated_at
             FROM entries e {} {}
             ORDER BY e.updated_at DESC
             LIMIT ?{} OFFSET ?{}",
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

        let entries = stmt
            .query_map(param_refs.as_slice(), |row| {
                Ok(ClipboardEntry {
                    id: row.get(0)?,
                    content_type: serde_json::from_str::<ContentType>(&format!(
                        "\"{}\"",
                        row.get::<_, String>(1)?
                    ))
                    .unwrap_or(ContentType::Text),
                    text_content: row.get(2)?,
                    blob_hash: row.get(3)?,
                    blob_size: row.get::<_, Option<i64>>(4)?.map(|s| s as u64),
                    source_app: row.get(5)?,
                    starred: row.get::<_, i32>(6)? != 0,
                    created_at: row
                        .get::<_, String>(7)?
                        .parse()
                        .unwrap_or_else(|_| Utc::now()),
                    updated_at: row
                        .get::<_, String>(8)?
                        .parse()
                        .unwrap_or_else(|_| Utc::now()),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok((entries, total))
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
}
