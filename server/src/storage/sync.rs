use super::*;
use copywraith_core::api_types::EntryResponse;
use copywraith_core::sync_protocol::*;

const SERVER_ID_KEY: &str = "sync_server_id";
const SCHEMA_VERSION_KEY: &str = "sync_schema_version";
const SCHEMA_VERSION: &str = "1";

pub(super) fn legacy_create_is_retired(db: &Connection, hash: &str) -> anyhow::Result<bool> {
    Ok(head(db, hash)?.is_some_and(|g| g.state == GenerationState::Deleted))
}

pub(super) fn initialize(conn: &mut Connection) -> anyhow::Result<()> {
    let tx = conn.transaction()?;
    let version: Option<String> = tx
        .query_row(
            "SELECT value FROM metadata WHERE key = ?1",
            [SCHEMA_VERSION_KEY],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(version) = version {
        anyhow::ensure!(
            version == SCHEMA_VERSION,
            "Unsupported sync schema {version}"
        );
        return Ok(());
    }

    // Schema, existing-generation adoption and its version commit together.
    tx.execute_batch(include_str!("sync_schema.sql"))?;
    tx.execute(
        "INSERT INTO metadata VALUES (?1, ?2)",
        params![SERVER_ID_KEY, Ulid::generate().to_string()],
    )?;
    tx.execute(
        "INSERT INTO metadata VALUES (?1, ?2)",
        params![SCHEMA_VERSION_KEY, SCHEMA_VERSION],
    )?;
    tx.commit()?;
    Ok(())
}

fn server_id(db: &Connection) -> anyhow::Result<String> {
    Ok(db.query_row(
        "SELECT value FROM metadata WHERE key = ?1",
        [SERVER_ID_KEY],
        |r| r.get(0),
    )?)
}

fn head(db: &Connection, hash: &str) -> anyhow::Result<Option<GenerationHead>> {
    Ok(db.query_row(
        "SELECT g.id, g.deleted FROM sync_heads h JOIN sync_generations g ON g.id = h.generation_id WHERE h.content_hash = ?1",
        [hash], |r| Ok(GenerationHead { id: r.get(0)?, state: state(r.get(1)?) })
    ).optional()?)
}

fn sequence(db: &Connection) -> anyhow::Result<u64> {
    Ok(u64::try_from(db.query_row(
        "SELECT sequence FROM sync_clock",
        [],
        |r| r.get::<_, i64>(0),
    )?)?)
}

fn state(deleted: bool) -> GenerationState {
    if deleted {
        GenerationState::Deleted
    } else {
        GenerationState::Live
    }
}

fn generation(db: &Connection, id: &str) -> anyhow::Result<Option<GenerationHead>> {
    Ok(db
        .query_row(
            "SELECT deleted FROM sync_generations WHERE id = ?1",
            [id],
            |r| {
                Ok(GenerationHead {
                    id: id.into(),
                    state: state(r.get(0)?),
                })
            },
        )
        .optional()?)
}

struct StoredReceipt {
    fingerprint: Option<String>,
    kind: OperationKind,
    value: SyncReceipt,
}

fn receipt(db: &Connection, operation_id: &str) -> anyhow::Result<Option<StoredReceipt>> {
    let row: Option<(Option<String>, String, String)> = db
        .query_row(
            "SELECT fingerprint, kind, receipt FROM sync_receipts WHERE operation_id = ?1",
            [operation_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    row.map(|(fingerprint, kind, json)| {
        Ok(StoredReceipt {
            fingerprint,
            kind: serde_json::from_str(&kind)?,
            value: serde_json::from_str(&json)?,
        })
    })
    .transpose()
}

fn save_receipt(
    db: &Connection,
    fingerprint: Option<&str>,
    kind: OperationKind,
    value: &SyncReceipt,
) -> anyhow::Result<()> {
    db.execute(
        "INSERT INTO sync_receipts VALUES (?1, ?2, ?3, ?4)",
        params![
            value.operation_id,
            fingerprint,
            serde_json::to_string(&kind)?,
            serde_json::to_string(value)?
        ],
    )?;
    Ok(())
}

impl Storage {
    pub fn sync_info(&self) -> anyhow::Result<SyncInfo> {
        Ok(SyncInfo {
            version: SYNC_PROTOCOL_VERSION,
            server_id: server_id(&self.db.lock().unwrap())?,
        })
    }

    pub fn sync_head(&self, hash: &str) -> anyhow::Result<SyncHead> {
        let db = self.db.lock().unwrap();
        Ok(SyncHead {
            server_id: server_id(&db)?,
            content_hash: hash.into(),
            generation: head(&db, hash)?,
        })
    }

    pub fn sync_changes(
        &self,
        expected_server: &str,
        cursor: u64,
        limit: u32,
        dek: &[u8; 32],
    ) -> anyhow::Result<SyncChanges> {
        let db = self.db.lock().unwrap();
        let server_id = server_id(&db)?;
        anyhow::ensure!(
            expected_server == server_id,
            SyncProtocolError::ServerMismatch
        );
        let current = u64::try_from(db.query_row("SELECT sequence FROM sync_clock", [], |r| {
            r.get::<_, i64>(0)
        })?)?;
        anyhow::ensure!(cursor <= current, SyncProtocolError::InvalidCursor);
        let limit = copywraith_core::api_types::clamp_limit(limit) as usize;
        let rows = db.prepare("SELECT id, content_hash, deleted, sequence FROM sync_generations WHERE sequence > ?1 ORDER BY sequence LIMIT ?2")?
            .query_map(params![i64::try_from(cursor)?, (limit + 1) as i64], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, bool>(2)?, r.get::<_, i64>(3)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        let has_more = rows.len() > limit;
        let mut changes = Vec::new();
        for (id, content_hash, deleted, sequence) in rows.into_iter().take(limit) {
            let entry = if deleted {
                None
            } else {
                let mut entry = db.query_row(
                    &format!("SELECT {ENTRY_SELECT_COLUMNS} FROM entries WHERE id = ?1"),
                    [&id],
                    row_to_entry,
                )?;
                decrypt_entry_text_fields(&mut entry, dek)?;
                let blob_url = entry
                    .blob_hash
                    .as_ref()
                    .map(|_| format!("/api/sync/{server_id}/entries/{id}/blob"));
                Some(EntryResponse { entry, blob_url })
            };
            changes.push(SyncChange {
                sequence: u64::try_from(sequence)?,
                generation: GenerationHead {
                    id,
                    state: state(deleted),
                },
                content_hash,
                entry,
            });
        }
        let cursor = changes.last().map(|c| c.sequence).unwrap_or(current);
        Ok(SyncChanges {
            server_id,
            changes,
            cursor,
            has_more,
        })
    }

    pub fn apply_sync_mutation(
        &self,
        request: &SyncMutation,
        dek: &[u8; 32],
    ) -> anyhow::Result<SyncReceipt> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let server_id = server_id(&tx)?;
        anyhow::ensure!(
            request.server_id == server_id,
            SyncProtocolError::ServerMismatch
        );
        let fingerprint = hash_bytes(&serde_json::to_vec(request)?);
        if let Some(stored) = receipt(&tx, &request.operation_id)? {
            // Only a typed cancellation reservation may accept an unseen create body.
            let cancelled_create = stored.kind == OperationKind::Create
                && stored.value.outcome == SyncOutcome::Cancelled
                && request.action.kind() == OperationKind::Create;
            anyhow::ensure!(
                cancelled_create || stored.fingerprint.as_deref() == Some(&fingerprint),
                SyncProtocolError::OperationReuse
            );
            return Ok(stored.value);
        }

        let mut retired_blob = None;
        let (outcome, generation) = match &request.action {
            SyncAction::Create { expected, payload } => {
                let current = head(&tx, &payload.content_hash)?;
                if &current != expected {
                    (SyncOutcome::Conflict, current)
                } else {
                    let flavors = payload
                        .flavors
                        .clone()
                        .unwrap_or_default()
                        .merge_legacy(payload.content_type, payload.text_content.as_deref());
                    let (entry, _) = self.create_entry_in(
                        &tx,
                        payload.content_type,
                        &flavors,
                        payload.blob_base64.as_deref(),
                        payload.source_app.as_deref(),
                        payload.starred,
                        &payload.content_hash,
                        Some(dek),
                    )?;
                    (
                        SyncOutcome::Applied,
                        Some(GenerationHead {
                            id: entry.id,
                            state: GenerationState::Live,
                        }),
                    )
                }
            }
            SyncAction::Star {
                generation_id,
                starred,
            } => {
                let target = generation(&tx, generation_id)?;
                if target
                    .as_ref()
                    .is_some_and(|g| g.state == GenerationState::Live)
                {
                    tx.execute(
                        "UPDATE entries SET starred = ?1, updated_at = ?2 WHERE id = ?3",
                        params![starred, Utc::now().to_rfc3339(), generation_id],
                    )?;
                    (SyncOutcome::Applied, target)
                } else {
                    (SyncOutcome::Missing, target)
                }
            }
            SyncAction::Delete { target } => {
                let target = match target {
                    DeleteTarget::Generation { id } => generation(&tx, id)?,
                    DeleteTarget::Create { operation_id } => {
                        anyhow::ensure!(
                            operation_id != &request.operation_id,
                            SyncProtocolError::WrongOperationKind
                        );
                        match receipt(&tx, operation_id)? {
                            Some(prior) => {
                                anyhow::ensure!(
                                    prior.kind == OperationKind::Create,
                                    SyncProtocolError::WrongOperationKind
                                );
                                match prior.value.outcome {
                                    SyncOutcome::Applied => prior.value.generation,
                                    _ => None,
                                }
                            }
                            None => {
                                save_receipt(
                                    &tx,
                                    None,
                                    OperationKind::Create,
                                    &SyncReceipt {
                                        server_id: server_id.clone(),
                                        sequence: sequence(&tx)?,
                                        operation_id: operation_id.clone(),
                                        outcome: SyncOutcome::Cancelled,
                                        generation: None,
                                    },
                                )?;
                                None
                            }
                        }
                    }
                };
                if let Some(mut target) = target {
                    retired_blob = self.delete_entry_in(&tx, &target.id)?.1;
                    target.state = GenerationState::Deleted;
                    (SyncOutcome::Applied, Some(target))
                } else if matches!(
                    request.action,
                    SyncAction::Delete {
                        target: DeleteTarget::Create { .. }
                    }
                ) {
                    (SyncOutcome::Applied, None)
                } else {
                    (SyncOutcome::Missing, None)
                }
            }
        };
        let result = SyncReceipt {
            server_id,
            sequence: sequence(&tx)?,
            operation_id: request.operation_id.clone(),
            outcome,
            generation,
        };
        save_receipt(&tx, Some(&fingerprint), request.action.kind(), &result)?;
        tx.commit()?;
        self.remove_unreferenced_blob(&db, retired_blob.as_deref())?;
        Ok(result)
    }
}
