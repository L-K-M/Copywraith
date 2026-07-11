use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::models::{ClipboardEntry, ClipboardFlavors, ContentType};

// --- Request types ---

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CreateEntryRequest {
    pub content_type: ContentType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flavors: Option<ClipboardFlavors>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob_base64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_app: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub starred: Option<bool>,
    pub content_hash: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct UpdateEntryRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub starred: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, IntoParams, ToSchema)]
pub struct ListEntriesParams {
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
    /// Cursor bound for descending `(updated_at, id)` pagination.
    ///
    /// When set (optionally with `before_id`), results include only rows older
    /// than this boundary. This avoids offset drift while rows are being
    /// inserted/updated concurrently.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_updated_at: Option<String>,
    /// Tiebreaker cursor for rows sharing `before_updated_at`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<ContentType>,
    #[serde(default)]
    pub starred_only: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
    /// Return original sensitive payloads instead of presentation-safe masks.
    /// This is intended for authenticated native synchronization clients.
    #[serde(default)]
    pub include_sensitive: bool,
}

fn default_limit() -> u32 {
    50
}

const MAX_LIMIT: u32 = 200;

/// Clamp a requested limit to the allowed maximum.
pub fn clamp_limit(limit: u32) -> u32 {
    limit.min(MAX_LIMIT).max(1)
}

// --- Response types ---

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct EntryResponse {
    #[serde(flatten)]
    pub entry: ClipboardEntry,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ListEntriesResponse {
    pub entries: Vec<EntryResponse>,
    pub total: u64,
    pub has_more: bool,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CreateEntryResponse {
    pub entry: EntryResponse,
    pub created: bool,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entries_count: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ErrorResponse {
    pub error: String,
}

#[cfg(test)]
mod tests {
    use super::ListEntriesParams;

    #[test]
    fn list_entries_masks_sensitive_content_by_default() {
        let params: ListEntriesParams = serde_json::from_str("{}").unwrap();
        assert!(!params.include_sensitive);
    }

    #[test]
    fn list_entries_can_explicitly_include_sensitive_content() {
        let params: ListEntriesParams =
            serde_json::from_str(r#"{"include_sensitive":true}"#).unwrap();
        assert!(params.include_sensitive);
    }
}
