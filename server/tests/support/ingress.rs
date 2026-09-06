use super::*;
use storage::ingress::*;

const CLIPBOARD_DOMAIN: &str = "android:user0:device0:clipboard";

fn registration(device: &Device) -> CaptureRegistration {
    device.storage.begin_registration(CLIPBOARD_DOMAIN).unwrap()
}

fn envelope(
    registration: &CaptureRegistration,
    text: &str,
    sequence: u64,
    kind: ObservationKind,
) -> CaptureEnvelope {
    CaptureEnvelope {
        registration_id: registration.registration_id.clone(),
        sequence,
        kind,
        observation_stamp: Some(format!("stamp-{sequence}")),
        payload: CapturePayload {
            content_type: ContentType::Text,
            flavors: CaptureFlavors {
                text_plain: Some(text.into()),
                ..Default::default()
            },
            blob_base64: None,
            source_app: Some("fixture".into()),
        },
    }
}

fn accepted(receipt: &CaptureReceipt) -> (String, i64) {
    match &receipt.outcome {
        CaptureOutcome::Accepted {
            local_id,
            incarnation,
        } => (local_id.clone(), *incarnation),
        other => panic!("expected accepted capture, got {other:?}"),
    }
}

fn quarantined(receipt: &CaptureReceipt, reason: QuarantineReason) {
    assert_eq!(receipt.outcome, CaptureOutcome::Quarantined { reason });
}

#[tokio::test]
async fn ingress_lost_reply_delete_replay_never_reinserts() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let registration = registration(&device);
    let delivery = envelope(
        &registration,
        "ingress lost reply",
        1,
        ObservationKind::Explicit,
    );
    let receipt = device.storage.accept_capture(&delivery).unwrap();
    device.storage.delete_entry(&accepted(&receipt).0).unwrap();
    let device = device.restart();
    assert_eq!(device.storage.accept_capture(&delivery).unwrap(), receipt);
    assert!(device.entries().is_empty());
}

#[tokio::test]
async fn ingress_first_delivery_after_local_retirement_is_quarantined() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let registration = registration(&device);
    let prior = device.capture("delayed observation");
    device.storage.delete_entry(&prior.id).unwrap();
    let delivery = envelope(
        &registration,
        "delayed observation",
        1,
        ObservationKind::Explicit,
    );
    let receipt = device.storage.accept_capture(&delivery).unwrap();
    quarantined(&receipt, QuarantineReason::RetiredAfterRegistration);
    let device = device.restart();
    assert_eq!(device.storage.accept_capture(&delivery).unwrap(), receipt);
    assert!(device.entries().is_empty());
    assert_eq!(device.storage.capture_quarantines(10, 0).unwrap().len(), 1);
}

#[tokio::test]
async fn ingress_conservative_snapshot_cannot_restore_via_a_predecessor() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let prior = device.capture("unchanged clipboard snapshot");
    device.exchange().await;
    device.storage.delete_entry(&prior.id).unwrap();
    let registration = registration(&device);
    let delivery = envelope(
        &registration,
        "unchanged clipboard snapshot",
        1,
        ObservationKind::Snapshot,
    );
    let receipt = device.storage.accept_capture(&delivery).unwrap();
    quarantined(&receipt, QuarantineReason::UncertainObservation);
    device.exchange().await;
    let device = device.restart();
    assert_eq!(device.storage.accept_capture(&delivery).unwrap(), receipt);
    assert_eq!(server.entries().await.total, 0);
    assert!(device.entries().is_empty());
    let db = rusqlite::Connection::open(device._dir.path().join("copywraith.db")).unwrap();
    let retained: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM ingress_deletions d JOIN ingress_registrations r ON d.created_clock <= r.epoch AND d.resolved_clock > r.epoch WHERE r.registration_id = ?1)", [&registration.registration_id], |r| r.get(0)).unwrap();
    assert!(
        retained,
        "quarantine retains derivable exact predecessor evidence"
    );
}

#[tokio::test]
async fn ingress_complete_envelope_reuse_rejects_every_metadata_change() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let registration = registration(&device);
    let original = envelope(&registration, "fingerprint", 1, ObservationKind::Explicit);
    let receipt = device.storage.accept_capture(&original).unwrap();
    for field in [
        "source_app",
        "text_html",
        "stamp",
        "kind",
        "blob",
        "content_type",
    ] {
        let mut changed = original.clone();
        match field {
            "source_app" => changed.payload.source_app = Some("different producer".into()),
            "text_html" => changed.payload.flavors.text_html = Some("<b>fingerprint</b>".into()),
            "stamp" => changed.observation_stamp = Some("other".into()),
            "kind" => changed.kind = ObservationKind::Snapshot,
            "blob" => changed.payload.blob_base64 = Some("AA==".into()),
            "content_type" => changed.payload.content_type = ContentType::Html,
            _ => unreachable!(),
        }
        assert!(device
            .storage
            .accept_capture(&changed)
            .unwrap_err()
            .to_string()
            .contains("reused"));
    }
    assert_eq!(device.storage.accept_capture(&original).unwrap(), receipt);
}

#[tokio::test]
async fn ingress_epoch_does_not_learn_a_later_tombstone() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let old = registration(&device);
    let create = server.create_request("new knowledge").await;
    let generation = server.apply(&create).await.generation.unwrap();
    server.delete_generation(&generation.id).await;
    device.sync.pull_new_entries(&device.storage).await.unwrap();
    let stale = envelope(&old, "new knowledge", 1, ObservationKind::Explicit);
    let first = device.storage.accept_capture(&stale).unwrap();
    device.exchange().await;
    assert_eq!(server.entries().await.total, 0);
    let fresh = registration(&device);
    let recovery = envelope(&fresh, "new knowledge", 1, ObservationKind::Explicit);
    let recovered = device.storage.accept_capture(&recovery).unwrap();
    assert_eq!(accepted(&first).0, accepted(&recovered).0);
    assert!(accepted(&recovered).1 > accepted(&first).1);
    device.exchange().await;
    assert_eq!(server.entries().await.total, 1);
    assert_eq!(device.storage.accept_capture(&stale).unwrap(), first);
    assert_eq!(device.entries()[0].id, accepted(&recovered).0);
}

#[tokio::test]
async fn ingress_pending_predecessor_at_epoch_survives_later_resolution() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let prior = device.capture("eligible predecessor");
    device.exchange().await;
    device.storage.delete_entry(&prior.id).unwrap();
    let registration = registration(&device);
    let deletes = device
        .storage
        .pending_mutations(&server.info().await.server_id)
        .unwrap();
    device.exchange().await;
    let capture = envelope(
        &registration,
        "eligible predecessor",
        1,
        ObservationKind::Explicit,
    );
    let receipt = device.storage.accept_capture(&capture).unwrap();
    let db = rusqlite::Connection::open(device._dir.path().join("copywraith.db")).unwrap();
    for delete in deletes {
        let retained: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM sync_capture_predecessors WHERE local_id = ?1 AND operation_id = ?2)", rusqlite::params![accepted(&receipt).0, delete.request.operation_id], |r| r.get(0)).unwrap();
        assert!(retained);
    }
    device.exchange().await;
    assert_eq!(server.entries().await.total, 1);
}

#[tokio::test]
async fn ingress_known_observation_reuses_outcome_after_delete_and_rotation() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let original = envelope(
        &registration(&device),
        "classification replay",
        1,
        ObservationKind::Explicit,
    );
    let receipt = device.storage.accept_capture(&original).unwrap();
    device.storage.delete_entry(&accepted(&receipt).0).unwrap();
    let mut repeated = envelope(
        &registration(&device),
        "classification replay",
        2,
        ObservationKind::Event,
    );
    repeated.observation_stamp = original.observation_stamp.clone();
    assert_eq!(
        device.storage.accept_capture(&repeated).unwrap().outcome,
        receipt.outcome
    );
    assert!(device.entries().is_empty());
    repeated.sequence = 3;
    repeated.payload.flavors.text_plain = Some("different content, colliding timestamp".into());
    quarantined(
        &device.storage.accept_capture(&repeated).unwrap(),
        QuarantineReason::ObservationCollision,
    );
    assert!(device.entries().is_empty());
}

#[tokio::test]
async fn ingress_event_requires_baseline_and_unambiguous_stamp() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let registration = registration(&device);
    let early = envelope(
        &registration,
        "event before baseline",
        1,
        ObservationKind::Event,
    );
    quarantined(
        &device.storage.accept_capture(&early).unwrap(),
        QuarantineReason::UncertainObservation,
    );
    let baseline = envelope(&registration, "baseline", 2, ObservationKind::Snapshot);
    quarantined(
        &device.storage.accept_capture(&baseline).unwrap(),
        QuarantineReason::UncertainObservation,
    );
    let event = envelope(&registration, "eligible event", 3, ObservationKind::Event);
    accepted(&device.storage.accept_capture(&event).unwrap());
    let mut unstamped = envelope(&registration, "API 24 event", 4, ObservationKind::Event);
    unstamped.observation_stamp = None;
    quarantined(
        &device.storage.accept_capture(&unstamped).unwrap(),
        QuarantineReason::UncertainObservation,
    );
}

#[tokio::test]
async fn ingress_revocation_keeps_receipts_but_stops_unseen_admission() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let registration = registration(&device);
    let original = envelope(&registration, "before stop", 1, ObservationKind::Explicit);
    let receipt = device.storage.accept_capture(&original).unwrap();
    device
        .storage
        .revoke_registration(&registration.registration_id)
        .unwrap();
    assert!(device
        .storage
        .registration_needs_rotation(&registration.registration_id)
        .unwrap());
    assert_eq!(device.storage.accept_capture(&original).unwrap(), receipt);
    let new = envelope(&registration, "after stop", 2, ObservationKind::Explicit);
    quarantined(
        &device.storage.accept_capture(&new).unwrap(),
        QuarantineReason::RegistrationRevoked,
    );
}

#[tokio::test]
async fn ingress_receipt_and_insertion_roll_back_together() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let registration = registration(&device);
    let original = envelope(
        &registration,
        "atomic admission",
        1,
        ObservationKind::Explicit,
    );
    let db = rusqlite::Connection::open(device._dir.path().join("copywraith.db")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_ingress_receipt BEFORE INSERT ON ingress_receipts BEGIN SELECT RAISE(ABORT, 'injected receipt failure'); END;").unwrap();
    assert!(device.storage.accept_capture(&original).is_err());
    let counts: (i64, i64, i64) = db.query_row("SELECT (SELECT COUNT(*) FROM entries), (SELECT COUNT(*) FROM ingress_receipts), (SELECT COUNT(*) FROM ingress_capture_authority)", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
    assert_eq!(counts, (0, 0, 0));
    db.execute_batch("DROP TRIGGER fail_ingress_receipt;")
        .unwrap();
    let device = device.restart();
    let receipt = device.storage.accept_capture(&original).unwrap();
    let device = device.restart();
    assert_eq!(device.storage.accept_capture(&original).unwrap(), receipt);
    assert_eq!(device.entries().len(), 1);
}

#[tokio::test]
async fn ingress_unsupported_material_remains_durable_quarantine() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let registration = registration(&device);
    let mut original = envelope(
        &registration,
        "image metadata",
        1,
        ObservationKind::Explicit,
    );
    original.payload.content_type = ContentType::Image;
    original.payload.blob_base64 = Some("AAECAw==".into());
    let receipt = device.storage.accept_capture(&original).unwrap();
    quarantined(&receipt, QuarantineReason::UnsupportedPayload);
    let device = device.restart();
    let retained = device.storage.capture_quarantines(1, 0).unwrap().remove(0);
    assert_eq!(
        serde_json::to_value(retained.envelope).unwrap(),
        serde_json::to_value(original).unwrap()
    );
    assert!(device.entries().is_empty());
}

#[tokio::test]
async fn ingress_profile_lookup_is_read_only_and_keeps_binding() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    assert_eq!(
        device.storage.sync_server_for_profile("unknown").unwrap(),
        None
    );
    device.storage.bind_sync_server("profile", "first").unwrap();
    assert_eq!(
        device
            .storage
            .sync_server_for_profile("profile")
            .unwrap()
            .as_deref(),
        Some("first")
    );
    assert!(device.storage.bind_sync_server("profile", "other").is_err());
    assert_eq!(
        device
            .storage
            .sync_server_for_profile("profile")
            .unwrap()
            .as_deref(),
        Some("first")
    );
}

#[tokio::test]
async fn ingress_pre_epoch_tombstone_authorizes_only_eligible_event_or_explicit() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let create = server.create_request("eligible recopy").await;
    let generation = server.apply(&create).await.generation.unwrap();
    server.delete_generation(&generation.id).await;
    device.sync.pull_new_entries(&device.storage).await.unwrap();
    let reg = registration(&device);
    assert!(!device
        .storage
        .registration_ready(&reg.registration_id)
        .unwrap());
    let baseline = envelope(&reg, "old baseline", 1, ObservationKind::Snapshot);
    device.storage.accept_capture(&baseline).unwrap();
    assert!(device
        .storage
        .registration_ready(&reg.registration_id)
        .unwrap());
    let snapshot = envelope(&reg, "eligible recopy", 2, ObservationKind::Snapshot);
    quarantined(
        &device.storage.accept_capture(&snapshot).unwrap(),
        QuarantineReason::UncertainObservation,
    );
    device.exchange().await;
    assert_eq!(server.entries().await.total, 0);
    let event = envelope(&reg, "eligible recopy", 3, ObservationKind::Event);
    let receipt = device.storage.accept_capture(&event).unwrap();
    accepted(&receipt);
    device.exchange().await;
    assert_eq!(server.entries().await.total, 1);
}

#[tokio::test]
async fn ingress_exact_epoch_authority_keeps_multiple_generations_and_servers() {
    let a = Server::start().await;
    let b = Server::start().await;
    let device = Device::new(&a.url);
    let mut generations = Vec::new();
    for server in [&a, &a, &b] {
        let create = server.create_request("multiple exact tombstones").await;
        let generation = server.apply(&create).await.generation.unwrap();
        server.delete_generation(&generation.id).await;
        let peer = Device::new(&server.url);
        peer.sync.pull_new_entries(&peer.storage).await.unwrap();
        // Feed the real server's canonical tombstone into this store's actual ingestion.
        let client = reqwest::Client::new();
        let page: SyncChanges = client
            .get(format!(
                "{}/api/sync/{}/changes?after=0",
                server.url, create.server_id
            ))
            .bearer_auth(PASSWORD)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        device
            .storage
            .bind_sync_server(&create.server_id, &create.server_id)
            .unwrap();
        for change in &page.changes {
            device
                .storage
                .apply_sync_deletion(&create.server_id, change)
                .unwrap();
        }
        generations.push((create.server_id, generation.id));
    }
    let reg = registration(&device);
    let delivery = envelope(
        &reg,
        "multiple exact tombstones",
        1,
        ObservationKind::Explicit,
    );
    device.storage.accept_capture(&delivery).unwrap();
    for (server, generation) in &generations {
        let candidate = device.storage.sync_candidates(server).unwrap().remove(0);
        assert!(device
            .storage
            .capture_can_restore(server, &candidate, generation)
            .unwrap());
        assert!(!device
            .storage
            .capture_can_restore(server, &candidate, "unknown-generation")
            .unwrap());
    }
    // A later head must not inherit authority from any of those historical tombstones.
    let newer = a.create_request("multiple exact tombstones").await;
    let later = a.apply(&newer).await.generation.unwrap();
    a.delete_generation(&later.id).await;
    device.exchange().await;
    assert_eq!(a.entries().await.total, 0);
}

#[tokio::test]
async fn ingress_incarnation_replacement_retires_an_old_registration() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let local = device.capture("retired incarnation");
    let frozen = device.freeze_create(&server, "retired incarnation").await;
    let competing = server.create_request("retired incarnation").await;
    let generation = server.apply(&competing).await.generation.unwrap();
    server.delete_generation(&generation.id).await;
    let conflict = server.apply(&frozen).await;
    let pending = device
        .storage
        .pending_mutations(&frozen.server_id)
        .unwrap()
        .remove(0);
    device
        .storage
        .acknowledge_mutation(&pending, &conflict)
        .unwrap();
    device.sync.pull_new_entries(&device.storage).await.unwrap();
    let old = registration(&device);
    let flavors = local.resolved_flavors();
    device
        .storage
        .insert_entry(
            ContentType::Text,
            &flavors,
            None,
            &flavors.payload_hash(ContentType::Text, None),
            None,
        )
        .unwrap();
    let before = device.entries()[0].clone();
    let late = envelope(&old, "retired incarnation", 1, ObservationKind::Explicit);
    quarantined(
        &device.storage.accept_capture(&late).unwrap(),
        QuarantineReason::RetiredAfterRegistration,
    );
    assert_eq!(device.entries()[0].id, local.id);
    assert_eq!(device.entries()[0].updated_at, before.updated_at);
    assert!(device
        .storage
        .registration_needs_rotation(&old.registration_id)
        .unwrap());
}

#[tokio::test]
async fn ingress_replayed_knowledge_does_not_rotate_readiness() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let local = device.capture("immutable first knowledge");
    device.exchange().await;
    device.storage.delete_entry(&local.id).unwrap();
    let pending = device
        .storage
        .pending_mutations(&server.info().await.server_id)
        .unwrap()
        .remove(0);
    let receipt = server.apply(&pending.request).await;
    device
        .storage
        .acknowledge_mutation(&pending, &receipt)
        .unwrap();
    device.exchange().await;
    let known = registration(&device);
    device
        .storage
        .acknowledge_mutation(&pending, &receipt)
        .unwrap();
    device.sync.reset_pull_cursor(&device.storage);
    device.exchange().await;
    assert!(!device
        .storage
        .registration_needs_rotation(&known.registration_id)
        .unwrap());
    assert_eq!(registration(&device).knowledge_clock, known.knowledge_clock);
}

#[tokio::test]
async fn ingress_upgrade_backfills_present_facts_but_keeps_legacy_staging_conservative() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let create = server.create_request("legacy staged clipboard").await;
    let generation = server.apply(&create).await.generation.unwrap();
    server.delete_generation(&generation.id).await;
    device.sync.pull_new_entries(&device.storage).await.unwrap();
    // Simulate a pre-ingress database without changing retained protocol facts.
    let db = rusqlite::Connection::open(device._dir.path().join("copywraith.db")).unwrap();
    let objects: Vec<(String, String)> = db.prepare("SELECT type, name FROM sqlite_master WHERE name LIKE 'ingress_%' AND type IN ('table', 'trigger') ORDER BY type DESC").unwrap().query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<Result<_, _>>().unwrap();
    for (kind, name) in objects {
        db.execute_batch(&format!("DROP {kind} \"{name}\";"))
            .unwrap();
    }
    drop(db);
    let device = device.restart();
    let reg = device
        .storage
        .begin_registration("legacy:share-target")
        .unwrap();
    let mut staged = envelope(&reg, "legacy staged clipboard", 1, ObservationKind::Legacy);
    staged.observation_stamp = Some("persistent-batch-id/item-1".into());
    let receipt = device.storage.accept_capture(&staged).unwrap();
    quarantined(&receipt, QuarantineReason::UncertainObservation);
    let device = device.restart();
    assert_eq!(device.storage.accept_capture(&staged).unwrap(), receipt);
    let next = device
        .storage
        .begin_registration("legacy:share-target")
        .unwrap();
    let mut redelivered = staged.clone();
    redelivered.registration_id = next.registration_id;
    assert_eq!(
        device.storage.accept_capture(&redelivered).unwrap().outcome,
        receipt.outcome
    );
    device.exchange().await;
    assert_eq!(server.entries().await.total, 0);
    let recovery = envelope(
        &registration(&device),
        "legacy staged clipboard",
        1,
        ObservationKind::Explicit,
    );
    accepted(&device.storage.accept_capture(&recovery).unwrap());
    device.exchange().await;
    assert_eq!(server.entries().await.total, 1);
    assert_eq!(device.storage.accept_capture(&staged).unwrap(), receipt);
}

#[tokio::test]
async fn ingress_observation_replay_does_not_promote_or_cross_domains() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let first = envelope(
        &registration(&device),
        "domain-specific observation",
        1,
        ObservationKind::Explicit,
    );
    let receipt = device.storage.accept_capture(&first).unwrap();
    let original = device.entries()[0].clone();
    let mut classification = first.clone();
    classification.registration_id = registration(&device).registration_id;
    classification.kind = ObservationKind::Snapshot;
    assert_eq!(
        device
            .storage
            .accept_capture(&classification)
            .unwrap()
            .outcome,
        receipt.outcome
    );
    assert_eq!(device.entries()[0].updated_at, original.updated_at);
    device.storage.delete_entry(&original.id).unwrap();
    let other = device
        .storage
        .begin_registration("android:user10:clipboard")
        .unwrap();
    let baseline = envelope(
        &other,
        "other clipboard baseline",
        2,
        ObservationKind::Snapshot,
    );
    device.storage.accept_capture(&baseline).unwrap();
    let mut actual_other = envelope(
        &other,
        "domain-specific observation",
        3,
        ObservationKind::Event,
    );
    actual_other.observation_stamp = first.observation_stamp;
    let distinct = device.storage.accept_capture(&actual_other).unwrap();
    assert_ne!(
        accepted(&distinct).0,
        original.id,
        "a domain change is not delivery replay"
    );
}

#[tokio::test]
async fn ingress_failed_recovery_rolls_back_incarnation_and_retirement_clock() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let stale = registration(&device);
    let create = server.create_request("rollback recovery").await;
    let generation = server.apply(&create).await.generation.unwrap();
    server.delete_generation(&generation.id).await;
    device.sync.pull_new_entries(&device.storage).await.unwrap();
    let original = envelope(&stale, "rollback recovery", 1, ObservationKind::Explicit);
    let first = device.storage.accept_capture(&original).unwrap();
    device.exchange().await;
    let fresh = registration(&device);
    let recovery = envelope(&fresh, "rollback recovery", 2, ObservationKind::Explicit);
    let before = device.entries()[0].clone();
    let db = rusqlite::Connection::open(device._dir.path().join("copywraith.db")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_recovery BEFORE INSERT ON ingress_receipts BEGIN SELECT RAISE(ABORT, 'injected recovery failure'); END;").unwrap();
    assert!(device.storage.accept_capture(&recovery).is_err());
    assert_eq!(device.entries()[0].updated_at, before.updated_at);
    assert!(!device
        .storage
        .registration_needs_rotation(&fresh.registration_id)
        .unwrap());
    assert_eq!(device.storage.accept_capture(&original).unwrap(), first);
    db.execute_batch("DROP TRIGGER fail_recovery;").unwrap();
    let recovered = device.storage.accept_capture(&recovery).unwrap();
    assert_eq!(accepted(&recovered).0, accepted(&first).0);
    assert!(accepted(&recovered).1 > accepted(&first).1);
    assert!(device
        .storage
        .registration_needs_rotation(&fresh.registration_id)
        .unwrap());
}

#[test]
fn ingress_wire_rejects_unknown_payload_material() {
    let json = serde_json::json!({
        "registration_id": "example", "sequence": 1, "kind": "explicit", "observation_stamp": null,
        "payload": { "content_type": "text", "source_app": null, "blob_base64": null,
            "flavors": { "text_plain": "visible", "unhandled_clip_items": ["must not disappear"] } }
    });
    assert!(serde_json::from_value::<CaptureEnvelope>(json).is_err());
}

#[tokio::test]
async fn ingress_accepted_receipt_keeps_identity_without_retaining_deleted_payload() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let delivery = envelope(
        &registration(&device),
        "discard live admission payload",
        1,
        ObservationKind::Explicit,
    );
    let receipt = device.storage.accept_capture(&delivery).unwrap();
    device.storage.delete_entry(&accepted(&receipt).0).unwrap();
    let db = rusqlite::Connection::open(device._dir.path().join("copywraith.db")).unwrap();
    let retained: Option<String> = db
        .query_row(
            "SELECT envelope FROM ingress_receipts WHERE registration_id = ?1 AND sequence = ?2",
            rusqlite::params![
                delivery.registration_id,
                i64::try_from(delivery.sequence).unwrap()
            ],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        retained.is_none(),
        "settled receipts need only their complete fingerprint, not deleted payload bytes"
    );
    assert_eq!(device.storage.accept_capture(&delivery).unwrap(), receipt);
}

#[tokio::test]
async fn ingress_recopy_cannot_bind_old_generation_before_cancel_ack() {
    let server = Server::start().await;
    let device = Device::new(&server.url);
    let original = device.capture("ingress cancellation fence");
    let frozen = device
        .freeze_create(&server, "ingress cancellation fence")
        .await;
    let old_generation = server.apply(&frozen).await.generation.unwrap();
    device.storage.delete_entry(&original.id).unwrap();
    let reg = registration(&device);
    let replacement = envelope(
        &reg,
        "ingress cancellation fence",
        1,
        ObservationKind::Explicit,
    );
    let receipt = device.storage.accept_capture(&replacement).unwrap();
    device.sync.pull_new_entries(&device.storage).await.unwrap();
    assert_eq!(device.entries()[0].id, accepted(&receipt).0);
    let device = device.restart();
    device.exchange().await;
    assert_eq!(device.entries()[0].id, accepted(&receipt).0);
    assert_ne!(
        server.entries().await.entries[0].entry.id,
        old_generation.id
    );
    assert_eq!(server.entries().await.total, 1);
    assert_eq!(
        device.storage.accept_capture(&replacement).unwrap(),
        receipt
    );
}
