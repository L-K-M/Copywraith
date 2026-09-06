//! Durable admission of observations, independent of the Android process layout.
use super::*;
use copywraith_core::content::{base64_to_bytes, hash_bytes};
use serde::{Deserialize, Serialize};

const MAX_RECOVERY_PAGE: u32 = 100;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CaptureRegistration {
    pub registration_id: String,
    pub domain: String,
    pub knowledge_clock: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ObservationKind {
    Snapshot,
    Event,
    Explicit,
    Legacy,
}

// Reject unknown flavors instead of acknowledging material the row cannot retain.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CaptureFlavors {
    pub text_plain: Option<String>,
    pub text_html: Option<String>,
    pub text_rtf: Option<String>,
    pub file_list: Option<Vec<String>>,
}

impl CaptureFlavors {
    fn clipboard(&self) -> ClipboardFlavors {
        ClipboardFlavors {
            text_plain: self.text_plain.clone(),
            text_html: self.text_html.clone(),
            text_rtf: self.text_rtf.clone(),
            file_list: self.file_list.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CapturePayload {
    pub content_type: ContentType,
    pub flavors: CaptureFlavors,
    pub blob_base64: Option<String>,
    pub source_app: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CaptureEnvelope {
    pub registration_id: String,
    pub sequence: u64,
    pub kind: ObservationKind,
    pub observation_stamp: Option<String>,
    pub payload: CapturePayload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum QuarantineReason {
    UncertainObservation,
    ObservationCollision,
    RetiredAfterRegistration,
    RegistrationRevoked,
    UnsupportedPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(crate) enum CaptureOutcome {
    Accepted { local_id: String, incarnation: i64 },
    Quarantined { reason: QuarantineReason },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CaptureReceipt {
    pub registration_id: String,
    pub sequence: u64,
    pub outcome: CaptureOutcome,
}

#[derive(Debug, Serialize)]
pub(crate) struct QuarantinedCapture {
    pub receipt: CaptureReceipt,
    pub envelope: CaptureEnvelope,
}

pub(super) fn initialize(conn: &mut Connection) -> anyhow::Result<()> {
    let tx = conn.transaction()?;
    tx.execute_batch(include_str!("ingress_schema.sql"))?;
    let backfilled: bool =
        tx.query_row("SELECT backfilled FROM ingress_clock", [], |r| r.get(0))?;
    if !backfilled {
        // Existing facts become known now; no historical retirement time is invented.
        tx.execute(
            "UPDATE ingress_clock SET value = value + 1, backfilled = 1",
            [],
        )?;
        tx.execute("INSERT OR IGNORE INTO ingress_tombstones SELECT server_id, remote_id, content_hash, (SELECT value FROM ingress_clock) FROM sync_links WHERE deleted = 1", [])?;
        tx.execute("INSERT OR IGNORE INTO ingress_deletions SELECT operation_id, server_id, local_id, incarnation, content_hash, remote_id, (SELECT value FROM ingress_clock), CASE WHEN resolved = 1 THEN (SELECT value FROM ingress_clock) END FROM sync_operation_provenance WHERE kind = 'delete'", [])?;
    }
    tx.commit()?;
    Ok(())
}

pub(super) fn associate_capture(
    db: &Connection,
    id: &str,
    hash: &str,
    registration: &str,
) -> anyhow::Result<()> {
    db.execute("INSERT INTO ingress_capture_authority SELECT id, sync_incarnation, ?2 FROM entries WHERE id = ?1", params![id, registration])?;
    // Retain the exact dependencies that were pending at registration, even if resolved now.
    db.execute("INSERT INTO sync_capture_predecessors SELECT e.id, e.sync_incarnation, d.operation_id FROM entries e JOIN ingress_registrations r ON r.registration_id = ?2 JOIN ingress_deletions d ON d.content_hash = ?3 AND d.created_clock <= r.epoch AND (d.resolved_clock IS NULL OR d.resolved_clock > r.epoch) WHERE e.id = ?1", params![id, registration, hash])?;
    Ok(())
}

pub(super) fn can_restore(
    db: &Connection,
    server: &str,
    id: &str,
    incarnation: i64,
    generation: &str,
) -> anyhow::Result<Option<bool>> {
    // Only admitted event/explicit intent obtains this association. Quarantine never does.
    let epoch: Option<i64> = db.query_row("SELECT r.epoch FROM ingress_capture_authority a JOIN ingress_registrations r ON r.registration_id = a.registration_id WHERE a.local_id = ?1 AND a.incarnation = ?2", params![id, incarnation], |r| r.get(0)).optional()?;
    let Some(epoch) = epoch else {
        return Ok(None);
    };
    let allowed = db.query_row("SELECT
        EXISTS(SELECT 1 FROM ingress_tombstones t JOIN entries e ON e.content_hash = t.content_hash WHERE e.id = ?1 AND t.server_id = ?2 AND t.remote_id = ?3 AND t.first_known <= ?4)
        OR EXISTS(SELECT 1 FROM ingress_deletions d JOIN entries e ON e.content_hash = d.content_hash WHERE e.id = ?1 AND d.server_id = ?2 AND d.remote_id = ?3 AND d.created_clock <= ?4 AND d.resolved_clock > ?4)",
        params![id, server, generation, epoch], |r| r.get(0))?;
    Ok(Some(allowed))
}

impl LocalStorage {
    /// Freeze authority before arming the listener. Registration does not scan history.
    pub(crate) fn begin_registration(&self, domain: &str) -> anyhow::Result<CaptureRegistration> {
        anyhow::ensure!(!domain.trim().is_empty(), "Clipboard domain is required");
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let epoch: i64 = tx.query_row("SELECT value FROM ingress_clock", [], |r| r.get(0))?;
        let registration_id = Ulid::generate().to_string();
        tx.execute(
            "INSERT INTO ingress_registrations VALUES (?1, ?2, ?3)",
            params![registration_id, domain, epoch],
        )?;
        tx.commit()?;
        Ok(CaptureRegistration {
            registration_id,
            domain: domain.into(),
            knowledge_clock: u64::try_from(epoch)?,
        })
    }

    /// The driver can rotate readiness on authority changes, without rotating on every star/feed poll.
    pub(crate) fn registration_needs_rotation(&self, registration: &str) -> anyhow::Result<bool> {
        Ok(self.db.lock().unwrap().query_row("SELECT r.epoch != c.value OR EXISTS(SELECT 1 FROM ingress_revocations WHERE registration_id = r.registration_id) FROM ingress_registrations r CROSS JOIN ingress_clock c WHERE r.registration_id = ?1", [registration], |r| r.get(0))?)
    }

    pub(crate) fn revoke_registration(&self, registration: &str) -> anyhow::Result<()> {
        self.db.lock().unwrap().execute("INSERT OR IGNORE INTO ingress_revocations SELECT registration_id FROM ingress_registrations WHERE registration_id = ?1", [registration])?;
        Ok(())
    }

    pub(crate) fn registration_ready(&self, registration: &str) -> anyhow::Result<bool> {
        Ok(self.db.lock().unwrap().query_row("SELECT r.epoch = c.value AND EXISTS(SELECT 1 FROM ingress_baselines WHERE registration_id = r.registration_id) AND NOT EXISTS(SELECT 1 FROM ingress_revocations WHERE registration_id = r.registration_id) FROM ingress_registrations r CROSS JOIN ingress_clock c WHERE r.registration_id = ?1", [registration], |r| r.get(0))?)
    }

    pub(crate) fn accept_capture(
        &self,
        envelope: &CaptureEnvelope,
    ) -> anyhow::Result<CaptureReceipt> {
        let sequence = i64::try_from(envelope.sequence)?;
        anyhow::ensure!(sequence > 0, "Delivery sequence must be positive");
        let json = serde_json::to_string(envelope)?;
        let fingerprint = hash_bytes(json.as_bytes());
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        // Receipt replay precedes every changing admission condition, including local deletion.
        let prior: Option<(String, String)> = tx.query_row("SELECT fingerprint, outcome FROM ingress_receipts WHERE registration_id = ?1 AND sequence = ?2", params![envelope.registration_id, sequence], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
        if let Some((original, outcome)) = prior {
            anyhow::ensure!(
                original == fingerprint,
                "Capture identity reused with different contents"
            );
            return Ok(CaptureReceipt {
                registration_id: envelope.registration_id.clone(),
                sequence: envelope.sequence,
                outcome: serde_json::from_str(&outcome)?,
            });
        }
        let (domain, epoch): (String, i64) = tx.query_row(
            "SELECT domain, epoch FROM ingress_registrations WHERE registration_id = ?1",
            [&envelope.registration_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let payload = &envelope.payload;
        let flavors = payload.flavors.clipboard();
        let bytes = payload
            .blob_base64
            .as_deref()
            .map(base64_to_bytes)
            .transpose()?;
        let blob_hash = bytes.as_deref().map(hash_bytes);
        let content_hash = flavors.payload_hash(payload.content_type, blob_hash.as_deref());
        let payload_fingerprint = hash_bytes(&serde_json::to_vec(payload)?);
        let known: Option<(String, String)> = if envelope.kind != ObservationKind::Explicit {
            tx.query_row("SELECT a.payload_fingerprint, r.outcome FROM ingress_observations a JOIN ingress_receipts r ON r.registration_id = a.registration_id AND r.sequence = a.sequence WHERE a.domain = ?1 AND a.stamp = ?2", params![domain, envelope.observation_stamp], |r| Ok((r.get(0)?, r.get(1)?))).optional()?
        } else {
            None
        };
        let outcome = if let Some((observed, outcome)) = known {
            if observed == payload_fingerprint {
                serde_json::from_str(&outcome)?
            } else {
                CaptureOutcome::Quarantined {
                    reason: QuarantineReason::ObservationCollision,
                }
            }
        } else if let Some(reason) = quarantine_reason(&tx, envelope, &content_hash, epoch)? {
            CaptureOutcome::Quarantined { reason }
        } else {
            self.insert_entry_in(
                &tx,
                payload.content_type,
                &flavors,
                None,
                &content_hash,
                payload.source_app.as_deref(),
                CaptureAuthority::Registration(&envelope.registration_id),
            )?;
            let (local_id, incarnation) = tx.query_row(
                "SELECT id, sync_incarnation FROM entries WHERE content_hash = ?1",
                [&content_hash],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            CaptureOutcome::Accepted {
                local_id,
                incarnation,
            }
        };
        let reason = match &outcome {
            CaptureOutcome::Quarantined { reason } => Some(serde_json::to_string(reason)?),
            CaptureOutcome::Accepted { .. } => None,
        };
        // Settled receipts retain identity, not live payload. Only quarantine needs recovery bytes.
        let retained_payload = reason.as_ref().map(|_| json.as_str());
        tx.execute(
            "INSERT INTO ingress_receipts VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                envelope.registration_id,
                sequence,
                fingerprint,
                retained_payload,
                serde_json::to_string(&outcome)?,
                reason,
                content_hash
            ],
        )?;
        if envelope.kind == ObservationKind::Snapshot {
            tx.execute(
                "INSERT OR IGNORE INTO ingress_baselines VALUES (?1)",
                [&envelope.registration_id],
            )?;
        }
        if let Some(stamp) = &envelope.observation_stamp {
            // A timestamp collision never replaces the original observation association.
            tx.execute(
                "INSERT OR IGNORE INTO ingress_observations VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    domain,
                    stamp,
                    payload_fingerprint,
                    envelope.registration_id,
                    sequence
                ],
            )?;
        }
        tx.commit()?;
        Ok(CaptureReceipt {
            registration_id: envelope.registration_id.clone(),
            sequence: envelope.sequence,
            outcome,
        })
    }

    /// Capture again uses the retained payload under a fresh registration and Explicit envelope.
    /// Inspection never upgrades or clears the original quarantine receipt.
    pub(crate) fn capture_quarantines(
        &self,
        limit: u32,
        offset: u32,
    ) -> anyhow::Result<Vec<QuarantinedCapture>> {
        let db = self.db.lock().unwrap();
        let rows = db.prepare("SELECT envelope, outcome FROM ingress_receipts WHERE reason IS NOT NULL ORDER BY registration_id, sequence LIMIT ?1 OFFSET ?2")?
            .query_map(params![limit.min(MAX_RECOVERY_PAGE), offset], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?.collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(envelope, outcome)| {
                let envelope: CaptureEnvelope = serde_json::from_str(&envelope)?;
                Ok(QuarantinedCapture {
                    receipt: CaptureReceipt {
                        registration_id: envelope.registration_id.clone(),
                        sequence: envelope.sequence,
                        outcome: serde_json::from_str(&outcome)?,
                    },
                    envelope,
                })
            })
            .collect()
    }
}

fn quarantine_reason(
    db: &Connection,
    envelope: &CaptureEnvelope,
    hash: &str,
    epoch: i64,
) -> anyhow::Result<Option<QuarantineReason>> {
    let revoked: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM ingress_revocations WHERE registration_id = ?1)",
        [&envelope.registration_id],
        |r| r.get(0),
    )?;
    if revoked {
        return Ok(Some(QuarantineReason::RegistrationRevoked));
    }
    let retired: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM ingress_retirements WHERE content_hash = ?1 AND clock > ?2)",
        params![hash, epoch],
        |r| r.get(0),
    )?;
    if retired {
        return Ok(Some(QuarantineReason::RetiredAfterRegistration));
    }
    let baseline: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM ingress_baselines WHERE registration_id = ?1)",
        [&envelope.registration_id],
        |r| r.get(0),
    )?;
    if matches!(
        envelope.kind,
        ObservationKind::Snapshot | ObservationKind::Legacy
    ) || (envelope.kind == ObservationKind::Event
        && (!baseline
            || envelope
                .observation_stamp
                .as_ref()
                .is_none_or(|s| s.is_empty())))
    {
        return Ok(Some(QuarantineReason::UncertainObservation));
    }
    let payload = &envelope.payload;
    // Retain unsupported bytes in SQLite quarantine; ordinary blob files are not durable yet.
    if matches!(payload.content_type, ContentType::Image | ContentType::File)
        || payload.blob_base64.is_some()
        || payload.flavors.file_list.is_some()
        || payload.flavors.clipboard().is_empty()
    {
        return Ok(Some(QuarantineReason::UnsupportedPayload));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_insertion_and_provenance_use_the_callers_transaction() {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalStorage::new(dir.path()).unwrap();
        let flavors = ClipboardFlavors {
            text_plain: Some("transactional capture".into()),
            ..Default::default()
        };
        let hash = flavors.payload_hash(ContentType::Text, None);
        let mut db = storage.db.lock().unwrap();
        db.execute("INSERT INTO sync_links (server_id, remote_id, content_hash) VALUES ('server', 'prior', ?1)", [&hash]).unwrap();
        let observer = Connection::open(dir.path().join("copywraith.db")).unwrap();
        let counts = |db: &Connection| -> (i64, i64) {
            db.query_row(
                "SELECT (SELECT COUNT(*) FROM entries), (SELECT COUNT(*) FROM sync_capture_heads)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
        };
        let tx = db.transaction().unwrap();
        storage
            .insert_entry_in(
                &tx,
                ContentType::Text,
                &flavors,
                None,
                &hash,
                None,
                CaptureAuthority::Current,
            )
            .unwrap()
            .unwrap();
        assert_eq!(counts(&tx), (1, 1));
        assert_eq!(counts(&observer), (0, 0));
        tx.rollback().unwrap();
        assert_eq!(counts(&observer), (0, 0));
        let tx = db.transaction().unwrap();
        let entry = storage
            .insert_entry_in(
                &tx,
                ContentType::Text,
                &flavors,
                None,
                &hash,
                None,
                CaptureAuthority::Current,
            )
            .unwrap()
            .unwrap();
        tx.execute("UPDATE entries SET starred = 1 WHERE id = ?1", [&entry.id])
            .unwrap();
        tx.commit().unwrap();
        assert_eq!(counts(&observer), (1, 1));
        let tx = db.transaction().unwrap();
        tx.execute("UPDATE entries SET starred = 0 WHERE id = ?1", [&entry.id])
            .unwrap();
        assert!(storage
            .insert_entry_in(
                &tx,
                ContentType::Text,
                &flavors,
                None,
                &hash,
                None,
                CaptureAuthority::Current
            )
            .unwrap()
            .is_none());
        tx.rollback().unwrap();
        assert!(observer
            .query_row(
                "SELECT starred FROM entries WHERE id = ?1",
                [&entry.id],
                |r| r.get::<_, bool>(0)
            )
            .unwrap());
    }
}
