use super::*;
use copywraith_core::sync_protocol::*;

pub(crate) struct SyncCandidate {
    pub entry: ClipboardEntry,
    pub revision: i64,
    pub origin: String,
    pub remote_id: Option<String>,
}

pub(crate) struct PendingMutation {
    pub request: SyncMutation,
    pub local_id: String,
    pub revision: i64,
}

fn project_remote_state(
    db: &Connection,
    server: &str,
    local_id: &str,
) -> anyhow::Result<(bool, Option<String>)> {
    let state: Option<(bool, bool, Option<String>)> = db.query_row(
        "SELECT deleted, starred, updated_at FROM sync_links WHERE server_id = ?1 AND local_id = ?2 ORDER BY sequence DESC LIMIT 1",
        params![server, local_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))
    ).optional()?;
    let Some((deleted, starred, updated_at)) = state else {
        return Ok((false, None));
    };
    if !deleted {
        let changed = db.execute("UPDATE entries SET starred = ?1, updated_at = COALESCE(?2, updated_at) WHERE id = ?3 AND synced = 1 AND starred != ?1", params![starred, updated_at, local_id])?;
        return Ok((changed > 0, None));
    }

    let blob = db
        .query_row(
            "SELECT blob_hash FROM entries WHERE id = ?1",
            [local_id],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten();
    let changed = db.execute("DELETE FROM entries WHERE id = ?1", [local_id])?;
    db.execute(
        "DELETE FROM sync_blocked WHERE server_id = ?1 AND local_id = ?2",
        params![server, local_id],
    )?;
    db.execute(
        "DELETE FROM sync_outbox WHERE server_id = ?1 AND local_id = ?2 AND kind = 'star'",
        params![server, local_id],
    )?;
    Ok((changed > 0, blob))
}

pub(super) fn initialize(conn: &mut Connection) -> anyhow::Result<()> {
    let tx = conn.transaction()?;
    ensure_entries_column(&tx, "sync_revision", "INTEGER NOT NULL DEFAULT 1")?;
    ensure_entries_column(&tx, "sync_origin", "TEXT NOT NULL DEFAULT 'legacy'")?;
    tx.execute_batch(include_str!("sync_schema.sql"))?;
    tx.commit()?;
    Ok(())
}

fn enqueue(
    db: &Connection,
    server_id: &str,
    local_id: &str,
    revision: i64,
    action: SyncAction,
) -> anyhow::Result<()> {
    let kind = match action {
        SyncAction::Create { .. } => "create",
        SyncAction::Star { .. } => "star",
        SyncAction::Delete { .. } => "delete",
    };
    let request = SyncMutation {
        server_id: server_id.into(),
        operation_id: Ulid::generate().to_string(),
        action,
    };
    db.execute(
        "INSERT INTO sync_outbox VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            request.operation_id,
            server_id,
            local_id,
            revision,
            kind,
            serde_json::to_string(&request)?
        ],
    )?;
    Ok(())
}

pub(super) fn queue_deletion(db: &Connection, id: &str) -> anyhow::Result<()> {
    let row: Option<(String, String)> = db
        .query_row(
            "SELECT content_hash, sync_origin FROM entries WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((hash, origin)) = row else {
        return Ok(());
    };
    let links = db
        .prepare("SELECT server_id, remote_id FROM sync_links WHERE local_id = ?1 AND deleted = 0")?
        .query_map([id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let creates = db.prepare("SELECT server_id, operation_id FROM sync_outbox WHERE local_id = ?1 AND kind = 'create'")?
        .query_map([id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?.collect::<Result<Vec<_>, _>>()?;

    // Never guess a historical generation from today's matching content hash.
    let configured: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM settings WHERE key IN ('server_url', 'server_url_primary', 'server_url_fallback') AND TRIM(value) != '')", [], |r| r.get(0))?;
    if origin != "capture" && links.is_empty() && creates.is_empty() && configured {
        db.execute(
            "INSERT OR IGNORE INTO sync_unresolved_deletes VALUES (?1, ?2)",
            params![id, hash],
        )?;
    }

    // Retire frozen payloads, but retain cancellation barriers for ambiguous POSTs.
    db.execute(
        "DELETE FROM sync_outbox WHERE local_id = ?1 AND kind != 'delete'",
        [id],
    )?;
    db.execute("DELETE FROM sync_blocked WHERE local_id = ?1", [id])?;
    for (server, remote) in links {
        enqueue(
            db,
            &server,
            id,
            0,
            SyncAction::Delete {
                target: DeleteTarget::Generation { id: remote },
            },
        )?;
    }
    for (server, operation_id) in creates {
        enqueue(
            db,
            &server,
            id,
            0,
            SyncAction::Delete {
                target: DeleteTarget::Create { operation_id },
            },
        )?;
    }
    Ok(())
}

impl LocalStorage {
    pub(crate) fn has_generation_sync_state(&self) -> anyhow::Result<bool> {
        Ok(self.db.lock().unwrap().query_row(
            "SELECT EXISTS(SELECT 1 FROM sync_peers)",
            [],
            |row| row.get(0),
        )?)
    }

    pub(crate) fn bind_sync_server(&self, profile: &str, server_id: &str) -> anyhow::Result<()> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let bound: Option<String> = tx
            .query_row(
                "SELECT server_id FROM sync_profiles WHERE profile = ?1",
                [profile],
                |r| r.get(0),
            )
            .optional()?;
        anyhow::ensure!(
            bound.as_deref().is_none_or(|id| id == server_id),
            "Configured endpoints identify different servers; synchronization stopped"
        );
        tx.execute(
            "INSERT OR IGNORE INTO sync_profiles VALUES (?1, ?2)",
            params![profile, server_id],
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO sync_peers (server_id) VALUES (?1)",
            [server_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn sync_cursor(&self, server: &str) -> anyhow::Result<u64> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        // A reset requested during an older pull survives that pull and a restart.
        tx.execute("UPDATE sync_peers SET cursor = 0, reset_requested = 0 WHERE server_id = ?1 AND reset_requested = 1", [server])?;
        let cursor: i64 = tx.query_row(
            "SELECT cursor FROM sync_peers WHERE server_id = ?1",
            [server],
            |r| r.get(0),
        )?;
        tx.commit()?;
        Ok(u64::try_from(cursor)?)
    }

    pub(crate) fn clear_protocol_cursors(&self) -> anyhow::Result<()> {
        self.db
            .lock()
            .unwrap()
            .execute("UPDATE sync_peers SET cursor = 0, reset_requested = 1", [])?;
        Ok(())
    }

    pub(crate) fn pending_mutations(&self, server: &str) -> anyhow::Result<Vec<PendingMutation>> {
        let db = self.db.lock().unwrap();
        let rows = db.prepare("SELECT request, local_id, revision FROM sync_outbox WHERE server_id = ?1 ORDER BY CASE kind WHEN 'delete' THEN 0 ELSE 1 END, operation_id LIMIT 50")?
            .query_map([server], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))?.collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(json, local_id, revision)| {
                Ok(PendingMutation {
                    request: serde_json::from_str(&json)?,
                    local_id,
                    revision,
                })
            })
            .collect()
    }

    pub(crate) fn sync_candidates(&self, server: &str) -> anyhow::Result<Vec<SyncCandidate>> {
        let db = self.db.lock().unwrap();
        let entries = db.prepare(&format!("SELECT {ENTRY_SELECT_COLUMNS} FROM entries e WHERE
            (synced = 0 OR NOT EXISTS(SELECT 1 FROM sync_links l WHERE l.local_id = e.id AND l.server_id = ?1 AND l.deleted = 0))
            AND NOT EXISTS(SELECT 1 FROM sync_outbox o WHERE o.local_id = e.id AND o.server_id = ?1)
            AND NOT EXISTS(SELECT 1 FROM sync_blocked b WHERE b.local_id = e.id AND b.server_id = ?1)
            ORDER BY created_at LIMIT 50"))?.query_map([server], row_to_entry)?.collect::<Result<Vec<_>, _>>()?;
        entries.into_iter().map(|entry| {
            let (revision, origin) = db.query_row("SELECT sync_revision, sync_origin FROM entries WHERE id = ?1", [&entry.id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            let remote_id = db.query_row("SELECT remote_id FROM sync_links WHERE server_id = ?1 AND local_id = ?2 AND deleted = 0 ORDER BY sequence DESC LIMIT 1", params![server, entry.id], |r| r.get(0)).optional()?;
            Ok(SyncCandidate { entry, revision, origin, remote_id })
        }).collect()
    }

    pub(crate) fn enqueue_sync_candidate(
        &self,
        server: &str,
        candidate: &SyncCandidate,
        action: SyncAction,
    ) -> anyhow::Result<()> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let current: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM entries WHERE id = ?1 AND sync_revision = ?2)",
            params![candidate.entry.id, candidate.revision],
            |r| r.get(0),
        )?;
        if !current {
            return Ok(());
        }
        let pending: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sync_outbox WHERE local_id = ?1 AND server_id = ?2)",
            params![candidate.entry.id, server],
            |r| r.get(0),
        )?;
        if pending {
            return Ok(());
        }
        enqueue(&tx, server, &candidate.entry.id, candidate.revision, action)?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn block_sync_candidate(
        &self,
        server: &str,
        id: &str,
        reason: &str,
    ) -> anyhow::Result<()> {
        self.db.lock().unwrap().execute(
            "INSERT OR REPLACE INTO sync_blocked VALUES (?1, ?2, ?3)",
            params![server, id, reason],
        )?;
        Ok(())
    }

    pub(crate) fn sync_warning(&self, server: &str) -> anyhow::Result<Option<String>> {
        let db = self.db.lock().unwrap();
        let unresolved: i64 =
            db.query_row("SELECT COUNT(*) FROM sync_unresolved_deletes", [], |r| {
                r.get(0)
            })?;
        if unresolved > 0 {
            return Ok(Some(format!(
                "{unresolved} legacy deletions have no known server identity; they were not sent."
            )));
        }
        Ok(db
            .query_row(
                "SELECT reason FROM sync_blocked WHERE server_id = ?1 LIMIT 1",
                [server],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub(crate) fn acknowledge_mutation(
        &self,
        sent: &PendingMutation,
        receipt: &SyncReceipt,
    ) -> anyhow::Result<bool> {
        anyhow::ensure!(
            receipt.server_id == sent.request.server_id
                && receipt.operation_id == sent.request.operation_id,
            "Sync receipt identity mismatch"
        );
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let removed = tx.execute(
            "DELETE FROM sync_outbox WHERE operation_id = ?1",
            [&receipt.operation_id],
        )?;
        if removed == 0 {
            return Ok(false);
        }
        if matches!(sent.request.action, SyncAction::Delete { .. }) {
            anyhow::ensure!(
                receipt.outcome == SyncOutcome::Applied,
                "Server did not acknowledge the requested deletion"
            );
            if let Some(g) = &receipt.generation {
                tx.execute(
                    "UPDATE sync_links SET deleted = 1, sequence = MAX(sequence, ?3) WHERE server_id = ?1 AND remote_id = ?2",
                    params![receipt.server_id, g.id, i64::try_from(receipt.sequence)?],
                )?;
            }
            tx.commit()?;
            return Ok(false);
        }
        if receipt.outcome != SyncOutcome::Applied {
            tx.execute("INSERT OR REPLACE INTO sync_blocked VALUES (?1, ?2, ?3)", params![receipt.server_id, sent.local_id, "An older capture or star update targets a retired generation; copy again after synchronization."])?;
            tx.commit()?;
            return Ok(false);
        }
        if let Some(g) = &receipt.generation {
            let hash: Option<String> = tx
                .query_row(
                    "SELECT content_hash FROM entries WHERE id = ?1",
                    [&sent.local_id],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(hash) = hash {
                let starred = match &sent.request.action {
                    SyncAction::Create { payload, .. } => payload.starred.unwrap_or(false),
                    SyncAction::Star { starred, .. } => *starred,
                    SyncAction::Delete { .. } => unreachable!(),
                };
                tx.execute("INSERT INTO sync_links (server_id, remote_id, local_id, content_hash, sequence, starred) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                    ON CONFLICT(server_id, remote_id) DO UPDATE SET local_id = excluded.local_id,
                    starred = CASE WHEN excluded.sequence > sequence THEN excluded.starred ELSE starred END,
                    sequence = MAX(sequence, excluded.sequence)", params![receipt.server_id, g.id, sent.local_id, hash, i64::try_from(receipt.sequence)?, starred])?;
            }
        }
        // Only the sent revision is clean. A newer star/delete remains pending.
        tx.execute(
            "UPDATE entries SET synced = 1 WHERE id = ?1 AND sync_revision = ?2",
            params![sent.local_id, sent.revision],
        )?;
        let (changed, blob) = project_remote_state(&tx, &receipt.server_id, &sent.local_id)?;
        tx.commit()?;
        self.remove_unreferenced_blob(&db, blob.as_deref())?;
        Ok(changed)
    }

    pub(crate) fn apply_sync_deletion(
        &self,
        server: &str,
        change: &SyncChange,
    ) -> anyhow::Result<bool> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let local: Option<String> = tx
            .query_row(
                "SELECT local_id FROM sync_links WHERE server_id = ?1 AND remote_id = ?2",
                params![server, change.generation.id],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten();
        let mut changed = false;
        let mut blob = None;
        if let Some(id) = local {
            // Other generations with the same payload are not this deletion's target.
            let another: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM sync_links WHERE server_id = ?1 AND local_id = ?2 AND remote_id != ?3 AND deleted = 0)", params![server, id, change.generation.id], |r| r.get(0))?;
            if !another {
                blob = tx
                    .query_row("SELECT blob_hash FROM entries WHERE id = ?1", [&id], |r| {
                        r.get::<_, Option<String>>(0)
                    })
                    .optional()?
                    .flatten();
                changed = tx.execute("DELETE FROM entries WHERE id = ?1", [&id])? > 0;
                tx.execute(
                    "DELETE FROM sync_blocked WHERE server_id = ?1 AND local_id = ?2",
                    params![server, id],
                )?;
                tx.execute("DELETE FROM sync_outbox WHERE server_id = ?1 AND local_id = ?2 AND kind = 'star'", params![server, id])?;
            }
        }
        // Unknown tombstones must survive until a delayed create receipt supplies its local ID.
        tx.execute("INSERT INTO sync_links (server_id, remote_id, content_hash, deleted, sequence) VALUES (?1, ?2, ?3, 1, ?4)
            ON CONFLICT(server_id, remote_id) DO UPDATE SET deleted = 1, sequence = MAX(sequence, excluded.sequence)", params![server, change.generation.id, change.content_hash, i64::try_from(change.sequence)?])?;
        tx.execute(
            "UPDATE sync_peers SET cursor = MAX(cursor, ?1) WHERE server_id = ?2",
            params![i64::try_from(change.sequence)?, server],
        )?;
        tx.commit()?;
        self.remove_unreferenced_blob(&db, blob.as_deref())?;
        Ok(changed)
    }

    pub(crate) fn apply_sync_live(
        &self,
        server: &str,
        change: &SyncChange,
        blob: Option<&[u8]>,
    ) -> anyhow::Result<bool> {
        let remote = change
            .entry
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Live generation has no payload"))?;
        anyhow::ensure!(
            remote.entry.id == change.generation.id,
            "Generation ID differs from its payload"
        );
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let retired_here: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM sync_links l WHERE l.server_id = ?1 AND l.remote_id = ?2 AND (l.deleted = 1 OR EXISTS(SELECT 1 FROM sync_outbox o WHERE o.server_id = l.server_id AND o.local_id = l.local_id AND o.kind = 'delete')))", params![server, change.generation.id], |r| r.get(0))?;
        if retired_here {
            tx.execute(
                "UPDATE sync_peers SET cursor = MAX(cursor, ?1) WHERE server_id = ?2",
                params![i64::try_from(change.sequence)?, server],
            )?;
            tx.commit()?;
            return Ok(false);
        }
        let entry = &remote.entry;
        let flavors = entry.resolved_flavors();
        let inserted = self.insert_remote_entry_in(
            &tx,
            RemoteEntryIdentity {
                id: &entry.id,
                created_at: entry.created_at,
                updated_at: entry.updated_at,
            },
            entry.content_type,
            &flavors,
            blob,
            &change.content_hash,
            entry.source_app.as_deref(),
            entry.starred,
        )?;
        let updated = tx.execute("UPDATE entries SET starred = ?1, updated_at = ?2 WHERE content_hash = ?3 AND synced = 1 AND starred != ?1", params![entry.starred, entry.updated_at.to_rfc3339(), change.content_hash])?;
        let id: String = tx.query_row(
            "SELECT id FROM entries WHERE content_hash = ?1",
            [&change.content_hash],
            |r| r.get(0),
        )?;
        // Keep confirmed stars even while pending local edits hide their projection.
        tx.execute("INSERT INTO sync_links (server_id, remote_id, local_id, content_hash, sequence, starred, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(server_id, remote_id) DO UPDATE SET local_id = excluded.local_id, sequence = excluded.sequence, deleted = 0, starred = excluded.starred, updated_at = excluded.updated_at", params![server, change.generation.id, id, change.content_hash, i64::try_from(change.sequence)?, entry.starred, entry.updated_at.to_rfc3339()])?;
        tx.execute(
            "DELETE FROM sync_blocked WHERE server_id = ?1 AND local_id = ?2",
            params![server, id],
        )?;
        tx.execute(
            "UPDATE sync_peers SET cursor = MAX(cursor, ?1) WHERE server_id = ?2",
            params![i64::try_from(change.sequence)?, server],
        )?;
        tx.commit()?;
        Ok(inserted || updated > 0)
    }
}
