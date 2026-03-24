use serde::{Deserialize, Serialize};

use crate::models::{ClipboardEntry, ContentType};

// --- Request types ---

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateEntryRequest {
    pub content_type: ContentType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob_base64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_app: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub starred: Option<bool>,
    pub content_hash: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateEntryRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub starred: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListEntriesParams {
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<ContentType>,
    #[serde(default)]
    pub starred_only: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
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

#[derive(Debug, Serialize, Deserialize)]
pub struct EntryResponse {
    #[serde(flatten)]
    pub entry: ClipboardEntry,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListEntriesResponse {
    pub entries: Vec<EntryResponse>,
    pub total: u64,
    pub has_more: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateEntryResponse {
    pub entry: EntryResponse,
    pub created: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub entries_count: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
}
