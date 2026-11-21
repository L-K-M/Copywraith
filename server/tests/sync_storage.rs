//! Storage-level regression tests for generation-aware synchronization.
//!
//! These exercise the server storage and protocol contracts directly, without
//! the unfinished HTTP endpoints or the desktop client.
#![allow(dead_code)]
#[path = "../src/crypto.rs"]
mod crypto;
#[path = "../src/storage.rs"]
mod server_storage;

use copywraith_core::api_types::CreateEntryRequest;
use copywraith_core::models::{ClipboardFlavors, ContentType};
use copywraith_core::sync_protocol::*;
use server_storage::Storage;

const DEK: [u8; 32] = [7u8; 32];
const LEGACY_ID: &str = "01M1RKPBQ7JNSQ9DWFTYZT8QYZ";
const LEGACY_HASH: &str = "legacy-hash";

fn flavors(text: &str) -> ClipboardFlavors {
    ClipboardFlavors {
        text_plain: Some(text.into()),
        ..Default::default()
    }
}

fn payload(text: &str) -> CreateEntryRequest {
    let flavors = flavors(text);
    CreateEntryRequest {
        content_type: ContentType::Text,
        text_content: Some(text.into()),
        flavors: Some(flavors.clone()),
        blob_base64: None,
        source_app: Some("fixture".into()),
        starred: Some(false),
        content_hash: flavors.payload_hash(ContentType::Text, None),
    }
}

fn operation_id() -> String {
    ulid::Ulid::generate().to_string()
}

fn create(
    server_id: &str,
    operation_id: &str,
    expected: Option<GenerationHead>,
    text: &str,
) -> SyncMutation {
    SyncMutation {
        server_id: server_id.into(),
        operation_id: operation_id.into(),
        action: SyncAction::Create {
            expected,
            payload: payload(text),
        },
    }
}

fn delete_generation(server_id: &str, id: &str) -> SyncMutation {
    SyncMutation {
        server_id: server_id.into(),
        operation_id: operation_id(),
        action: SyncAction::Delete {
            target: DeleteTarget::Generation { id: id.into() },
        },
    }
}

fn cancel_create(server_id: &str, target: &str) -> SyncMutation {
    SyncMutation {
        server_id: server_id.into(),
        operation_id: operation_id(),
        action: SyncAction::Delete {
            target: DeleteTarget::Create {
                operation_id: target.into(),
            },
        },
    }
}

fn star(server_id: &str, generation_id: &str, starred: bool) -> SyncMutation {
    SyncMutation {
        server_id: server_id.into(),
        operation_id: operation_id(),
        action: SyncAction::Star {
            generation_id: generation_id.into(),
            starred,
        },
    }
}

fn assert_protocol_error(error: &anyhow::Error, expected: &SyncProtocolError) {
    match error.downcast_ref::<SyncProtocolError>() {
        Some(actual) => assert!(
            std::mem::discriminant(actual) == std::mem::discriminant(expected),
            "expected {expected}, got {actual}"
        ),
        None => panic!("expected sync protocol error {expected}, got {error:?}"),
    }
}

#[test]
fn legacy_database_upgrade_adopts_generations_and_feed() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::copy("tests/fixtures/legacy.db", dir.path().join("copywraith.db")).unwrap();

    let storage = Storage::new(dir.path()).unwrap();
    let info = storage.sync_info().unwrap();
    assert_eq!(info.version, SYNC_PROTOCOL_VERSION);
    assert!(!info.server_id.is_empty());

    let head = storage.sync_head(LEGACY_HASH).unwrap();
    assert_eq!(head.server_id, info.server_id);
    assert_eq!(head.content_hash, LEGACY_HASH);
    assert_eq!(head.generation.as_ref().unwrap().id, LEGACY_ID);
    assert_eq!(
        head.generation.as_ref().unwrap().state,
        GenerationState::Live
    );

    let changes = storage.sync_changes(&info.server_id, 0, 100, &DEK).unwrap();
    assert_eq!(changes.server_id, info.server_id);
    assert_eq!(changes.changes.len(), 1);
    assert_eq!(changes.cursor, 1);
    assert!(!changes.has_more);
    let change = &changes.changes[0];
    assert_eq!(change.generation.id, LEGACY_ID);
    assert_eq!(change.content_hash, LEGACY_HASH);
    assert_eq!(
        change
            .entry
            .as_ref()
            .unwrap()
            .entry
            .flavors
            .text_plain
            .as_deref(),
        Some("legacy row")
    );

    // Reopening adopts nothing again and preserves the server identity.
    drop(storage);
    let reopened = Storage::new(dir.path()).unwrap();
    assert_eq!(reopened.sync_info().unwrap().server_id, info.server_id);
    assert_eq!(reopened.count_entries().unwrap(), 1);
    let changes = reopened
        .sync_changes(&info.server_id, 0, 100, &DEK)
        .unwrap();
    assert_eq!(changes.changes.len(), 1);
    assert_eq!(changes.cursor, 1);
    assert_eq!(changes.changes[0].generation.id, LEGACY_ID);
}

#[test]
fn database_without_sync_schema_is_upgraded_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::new(dir.path()).unwrap();
    let flavors = flavors("pre-upgrade row");
    let hash = flavors.payload_hash(ContentType::Text, None);
    let (entry, _) = storage
        .create_entry(ContentType::Text, &flavors, None, None, None, &hash, None)
        .unwrap();
    drop(storage);

    // Simulate a database written by a build that predates generation sync.
    let conn = rusqlite::Connection::open(dir.path().join("copywraith.db")).unwrap();
    conn.execute_batch(
        "
        DROP TRIGGER entries_sync_insert;
        DROP TRIGGER entries_sync_update;
        DROP TRIGGER entries_sync_delete;
        DROP TABLE sync_receipts;
        DROP TABLE sync_heads;
        DROP TABLE sync_generations;
        DROP TABLE sync_clock;
        DELETE FROM metadata WHERE key IN ('sync_server_id', 'sync_schema_version');
        ",
    )
    .unwrap();
    drop(conn);

    let upgraded = Storage::new(dir.path()).unwrap();
    let info = upgraded.sync_info().unwrap();
    assert_eq!(info.version, SYNC_PROTOCOL_VERSION);
    assert_eq!(upgraded.count_entries().unwrap(), 1);
    let head = upgraded.sync_head(&hash).unwrap();
    assert_eq!(head.generation.as_ref().unwrap().id, entry.id);
    assert_eq!(
        head.generation.as_ref().unwrap().state,
        GenerationState::Live
    );
    let changes = upgraded
        .sync_changes(&info.server_id, 0, 100, &DEK)
        .unwrap();
    assert_eq!(changes.changes.len(), 1);
    assert_eq!(changes.changes[0].generation.id, entry.id);
    assert_eq!(
        changes.changes[0]
            .entry
            .as_ref()
            .unwrap()
            .entry
            .flavors
            .text_plain
            .as_deref(),
        Some("pre-upgrade row")
    );
}

#[test]
fn legacy_create_of_deleted_hash_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::new(dir.path()).unwrap();
    let recopy_flavors = flavors("recopy");
    let hash = recopy_flavors.payload_hash(ContentType::Text, None);
    let (entry, created) = storage
        .create_entry(
            ContentType::Text,
            &recopy_flavors,
            None,
            None,
            None,
            &hash,
            None,
        )
        .unwrap();
    assert!(created);
    assert!(storage.delete_entry(&entry.id).unwrap());

    let error = storage
        .create_entry(
            ContentType::Text,
            &recopy_flavors,
            None,
            None,
            None,
            &hash,
            None,
        )
        .unwrap_err();
    assert_protocol_error(&error, &SyncProtocolError::LegacyRecreation);
    assert_eq!(storage.count_entries().unwrap(), 0);
    let head = storage.sync_head(&hash).unwrap();
    assert_eq!(
        head.generation.as_ref().unwrap().state,
        GenerationState::Deleted
    );

    // A different hash still travels the legacy path and enters the feed.
    let other_flavors = flavors("new content");
    let other_hash = other_flavors.payload_hash(ContentType::Text, None);
    let (_, created) = storage
        .create_entry(
            ContentType::Text,
            &other_flavors,
            None,
            None,
            None,
            &other_hash,
            None,
        )
        .unwrap();
    assert!(created);

    let server_id = storage.sync_info().unwrap().server_id;
    let changes = storage.sync_changes(&server_id, 0, 100, &DEK).unwrap();
    // A generation appears once, at its latest state: the deleted original,
    // then the unrelated new generation.
    assert_eq!(changes.changes.len(), 2);
    assert_eq!(changes.changes[0].generation.id, entry.id);
    assert_eq!(
        changes.changes[0].generation.state,
        GenerationState::Deleted
    );
    assert!(changes.changes[0].entry.is_none());
    assert_eq!(changes.changes[1].generation.state, GenerationState::Live);
    assert_eq!(
        changes.changes[1]
            .entry
            .as_ref()
            .unwrap()
            .entry
            .flavors
            .text_plain
            .as_deref(),
        Some("new content")
    );
}

#[test]
fn duplicate_operations_replay_receipts_and_reject_reuse() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::new(dir.path()).unwrap();
    let server_id = storage.sync_info().unwrap().server_id;
    let operation = operation_id();

    let first = storage
        .apply_sync_mutation(&create(&server_id, &operation, None, "duplicate"), &DEK)
        .unwrap();
    assert_eq!(first.outcome, SyncOutcome::Applied);
    let generation = first.generation.clone().unwrap();

    let replay = storage
        .apply_sync_mutation(&create(&server_id, &operation, None, "duplicate"), &DEK)
        .unwrap();
    assert_eq!(replay.server_id, first.server_id);
    assert_eq!(replay.sequence, first.sequence);
    assert_eq!(replay.operation_id, first.operation_id);
    assert_eq!(replay.outcome, first.outcome);
    assert_eq!(replay.generation, first.generation);
    assert_eq!(storage.count_entries().unwrap(), 1);

    // The same operation ID with a different body is corruption, not a replay.
    let changed = create(&server_id, &operation, None, "different");
    let error = storage.apply_sync_mutation(&changed, &DEK).unwrap_err();
    assert_protocol_error(&error, &SyncProtocolError::OperationReuse);

    // A distinct operation for the same content is not a duplicate, but it
    // must acknowledge the live generation it re-copies.
    let expected = GenerationHead {
        id: generation.id.clone(),
        state: generation.state,
    };
    let distinct = create(&server_id, &operation_id(), Some(expected), "duplicate");
    let receipt = storage.apply_sync_mutation(&distinct, &DEK).unwrap();
    assert_eq!(receipt.outcome, SyncOutcome::Applied);
    assert_eq!(receipt.generation.unwrap().id, generation.id);
    assert_eq!(storage.count_entries().unwrap(), 1);

    // Identity is checked before any receipt replay.
    let mut wrong_server = create(&server_id, &operation_id(), None, "duplicate");
    wrong_server.server_id = ulid::Ulid::generate().to_string();
    let error = storage
        .apply_sync_mutation(&wrong_server, &DEK)
        .unwrap_err();
    assert_protocol_error(&error, &SyncProtocolError::ServerMismatch);
}

#[test]
fn cancellation_receipt_fences_a_later_create() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::new(dir.path()).unwrap();
    let server_id = storage.sync_info().unwrap().server_id;
    let operation = operation_id();
    let create_request = create(&server_id, &operation, None, "fenced");

    let cancel = cancel_create(&server_id, &operation);
    let receipt = storage.apply_sync_mutation(&cancel, &DEK).unwrap();
    assert_eq!(receipt.outcome, SyncOutcome::Applied);
    assert!(receipt.generation.is_none());
    let replay = storage.apply_sync_mutation(&cancel, &DEK).unwrap();
    assert_eq!(replay.sequence, receipt.sequence);
    assert_eq!(replay.outcome, receipt.outcome);

    let fenced = storage.apply_sync_mutation(&create_request, &DEK).unwrap();
    assert_eq!(fenced.outcome, SyncOutcome::Cancelled);
    assert!(fenced.generation.is_none());
    assert_eq!(storage.count_entries().unwrap(), 0);

    // A typed cancellation reservation accepts an unseen create body.
    let unseen = create(&server_id, &operation, None, "different payload");
    let accepted = storage.apply_sync_mutation(&unseen, &DEK).unwrap();
    assert_eq!(accepted.outcome, SyncOutcome::Cancelled);
    assert!(accepted.generation.is_none());
    assert_eq!(storage.count_entries().unwrap(), 0);

    // Cancelling a non-create receipt is rejected.
    let target_operation = operation_id();
    let target = create(&server_id, &target_operation, None, "star target");
    let generation = storage
        .apply_sync_mutation(&target, &DEK)
        .unwrap()
        .generation
        .unwrap();
    let star_operation = operation_id();
    let star = SyncMutation {
        server_id: server_id.clone(),
        operation_id: star_operation.clone(),
        action: SyncAction::Star {
            generation_id: generation.id,
            starred: true,
        },
    };
    let receipt = storage.apply_sync_mutation(&star, &DEK).unwrap();
    assert_eq!(receipt.outcome, SyncOutcome::Applied);
    let cancel_star = cancel_create(&server_id, &star_operation);
    let error = storage.apply_sync_mutation(&cancel_star, &DEK).unwrap_err();
    assert_protocol_error(&error, &SyncProtocolError::WrongOperationKind);
}

#[test]
fn protocol_recreation_uses_a_new_generation_after_deletion() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::new(dir.path()).unwrap();
    let server_id = storage.sync_info().unwrap().server_id;
    let hash = payload("recreate").content_hash;

    let first = storage
        .apply_sync_mutation(&create(&server_id, &operation_id(), None, "recreate"), &DEK)
        .unwrap();
    let first_id = first.generation.as_ref().unwrap().id.clone();

    let delete = delete_generation(&server_id, &first_id);
    let deleted = storage.apply_sync_mutation(&delete, &DEK).unwrap();
    assert_eq!(deleted.outcome, SyncOutcome::Applied);
    assert_eq!(
        deleted.generation.as_ref().unwrap().state,
        GenerationState::Deleted
    );
    assert_eq!(storage.count_entries().unwrap(), 0);

    // A create that ignores the tombstone conflicts.
    let blind = storage
        .apply_sync_mutation(&create(&server_id, &operation_id(), None, "recreate"), &DEK)
        .unwrap();
    assert_eq!(blind.outcome, SyncOutcome::Conflict);
    assert_eq!(blind.generation.as_ref().unwrap().id, first_id);
    assert_eq!(
        blind.generation.as_ref().unwrap().state,
        GenerationState::Deleted
    );
    assert_eq!(storage.count_entries().unwrap(), 0);

    // A create that acknowledges the tombstone starts a new generation.
    let expected = GenerationHead {
        id: first_id.clone(),
        state: GenerationState::Deleted,
    };
    let second = storage
        .apply_sync_mutation(
            &create(&server_id, &operation_id(), Some(expected), "recreate"),
            &DEK,
        )
        .unwrap();
    assert_eq!(second.outcome, SyncOutcome::Applied);
    let second_id = second.generation.as_ref().unwrap().id.clone();
    assert_ne!(second_id, first_id);
    assert_eq!(storage.count_entries().unwrap(), 1);
    let head = storage.sync_head(&hash).unwrap();
    assert_eq!(head.generation.as_ref().unwrap().id, second_id);
    assert_eq!(
        head.generation.as_ref().unwrap().state,
        GenerationState::Live
    );

    // A late delete of the old generation neither replays nor removes the new one.
    let late_delete = delete_generation(&server_id, &first_id);
    let late = storage.apply_sync_mutation(&late_delete, &DEK).unwrap();
    assert_eq!(late.outcome, SyncOutcome::Applied);
    assert_eq!(late.generation.as_ref().unwrap().id, first_id);
    assert_eq!(storage.count_entries().unwrap(), 1);
    assert_eq!(
        storage
            .get_entry(&second_id, Some(&DEK))
            .unwrap()
            .unwrap()
            .id,
        second_id
    );
}

#[test]
fn star_and_missing_generation_operations() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::new(dir.path()).unwrap();
    let server_id = storage.sync_info().unwrap().server_id;

    let created = storage
        .apply_sync_mutation(&create(&server_id, &operation_id(), None, "star me"), &DEK)
        .unwrap();
    let id = created.generation.unwrap().id;

    let missing = star(&server_id, &ulid::Ulid::generate().to_string(), true);
    let receipt = storage.apply_sync_mutation(&missing, &DEK).unwrap();
    assert_eq!(receipt.outcome, SyncOutcome::Missing);
    assert!(receipt.generation.is_none());

    let receipt = storage
        .apply_sync_mutation(&star(&server_id, &id, true), &DEK)
        .unwrap();
    assert_eq!(receipt.outcome, SyncOutcome::Applied);
    assert_eq!(receipt.generation.unwrap().id, id);
    assert!(storage.get_entry(&id, Some(&DEK)).unwrap().unwrap().starred);

    let delete = delete_generation(&server_id, &id);
    storage.apply_sync_mutation(&delete, &DEK).unwrap();
    let receipt = storage
        .apply_sync_mutation(&star(&server_id, &id, true), &DEK)
        .unwrap();
    assert_eq!(receipt.outcome, SyncOutcome::Missing);
    assert_eq!(
        receipt.generation.as_ref().unwrap().state,
        GenerationState::Deleted
    );
}

#[test]
fn sync_changes_checks_server_and_cursor() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::new(dir.path()).unwrap();
    let server_id = storage.sync_info().unwrap().server_id;
    storage
        .apply_sync_mutation(&create(&server_id, &operation_id(), None, "cursor"), &DEK)
        .unwrap();
    let second = create(&server_id, &operation_id(), None, "cursor two");
    let second_sequence = storage.apply_sync_mutation(&second, &DEK).unwrap().sequence;

    let error = storage
        .sync_changes("not-the-server", 0, 100, &DEK)
        .unwrap_err();
    assert_protocol_error(&error, &SyncProtocolError::ServerMismatch);

    let error = storage.sync_changes(&server_id, 99, 100, &DEK).unwrap_err();
    assert_protocol_error(&error, &SyncProtocolError::InvalidCursor);

    let first = storage.sync_changes(&server_id, 0, 1, &DEK).unwrap();
    assert_eq!(first.changes.len(), 1);
    assert!(first.has_more);
    assert_eq!(first.cursor, 1);

    let next = storage
        .sync_changes(&server_id, first.cursor, 1, &DEK)
        .unwrap();
    assert_eq!(next.changes.len(), 1);
    assert!(!next.has_more);
    assert_eq!(next.cursor, second_sequence);

    let empty = storage
        .sync_changes(&server_id, next.cursor, 1, &DEK)
        .unwrap();
    assert!(empty.changes.is_empty());
    assert!(!empty.has_more);
    assert_eq!(empty.cursor, second_sequence);
}

#[test]
fn failed_duplicate_create_rolls_back_its_star_and_sequence() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::new(dir.path()).unwrap();
    let text = flavors("encrypted duplicate");
    let hash = text.payload_hash(ContentType::Text, None);
    let (entry, _) = storage
        .create_entry(
            ContentType::Text,
            &text,
            None,
            None,
            Some(false),
            &hash,
            Some(&DEK),
        )
        .unwrap();
    let server = storage.sync_info().unwrap();
    let original = storage
        .sync_changes(&server.server_id, 0, 100, &DEK)
        .unwrap();

    let wrong_key = [2; 32];
    assert!(storage
        .create_entry(
            ContentType::Text,
            &text,
            None,
            None,
            Some(true),
            &hash,
            Some(&wrong_key)
        )
        .is_err());
    assert!(
        !storage
            .get_entry(&entry.id, Some(&DEK))
            .unwrap()
            .unwrap()
            .starred
    );
    assert!(storage
        .sync_changes(&server.server_id, original.cursor, 100, &DEK)
        .unwrap()
        .changes
        .is_empty());
}
