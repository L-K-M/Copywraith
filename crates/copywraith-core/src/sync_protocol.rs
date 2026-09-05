//! Generation-aware replication. Content equality is not operation identity.
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::api_types::{CreateEntryRequest, EntryResponse};

pub const SYNC_PROTOCOL_VERSION: u32 = 2;
pub const SYNC_PAGE_SIZE: u32 = 100;

#[derive(Debug, thiserror::Error)]
pub enum SyncProtocolError {
    #[error("Sync server identity mismatch")]
    ServerMismatch,
    #[error("Operation ID reused with different contents")]
    OperationReuse,
    #[error("Cancellation target is not a create operation")]
    WrongOperationKind,
    #[error("Upgrade the client to re-copy deleted content")]
    LegacyRecreation,
    #[error("Sync cursor is ahead of this server; restore requires a new server identity")]
    InvalidCursor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    Create,
    Star,
    Delete,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SyncInfo {
    pub version: u32,
    pub server_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum GenerationState {
    Live,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct GenerationHead {
    pub id: String,
    pub state: GenerationState,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SyncHead {
    pub server_id: String,
    pub content_hash: String,
    pub generation: Option<GenerationHead>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SyncMutation {
    pub server_id: String,
    pub operation_id: String,
    pub action: SyncAction,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SyncAction {
    Create {
        expected: Option<GenerationHead>,
        payload: CreateEntryRequest,
    },
    Star {
        generation_id: String,
        starred: bool,
    },
    Delete {
        target: DeleteTarget,
    },
}

impl SyncAction {
    pub fn kind(&self) -> OperationKind {
        match self {
            Self::Create { .. } => OperationKind::Create,
            Self::Star { .. } => OperationKind::Star,
            Self::Delete { .. } => OperationKind::Delete,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DeleteTarget {
    Generation { id: String },
    Create { operation_id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SyncOutcome {
    Applied,
    Cancelled,
    Conflict,
    Missing,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SyncReceipt {
    pub server_id: String,
    pub sequence: u64,
    pub operation_id: String,
    pub outcome: SyncOutcome,
    pub generation: Option<GenerationHead>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SyncChange {
    pub sequence: u64,
    pub generation: GenerationHead,
    pub content_hash: String,
    // Deleted generations retain identity only, never clipboard payloads.
    pub entry: Option<EntryResponse>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SyncChanges {
    pub server_id: String,
    pub changes: Vec<SyncChange>,
    pub cursor: u64,
    pub has_more: bool,
}
