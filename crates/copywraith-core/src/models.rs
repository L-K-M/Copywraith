use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ulid::Ulid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentType {
    Text,
    Html,
    Rtf,
    Image,
    File,
}

impl ContentType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ContentType::Text => "text",
            ContentType::Html => "html",
            ContentType::Rtf => "rtf",
            ContentType::Image => "image",
            ContentType::File => "file",
        }
    }
}

impl std::fmt::Display for ContentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardEntry {
    pub id: String,
    pub content_type: ContentType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob_size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_app: Option<String>,
    pub starred: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ClipboardEntry {
    pub fn new_text(text: String) -> Self {
        let now = Utc::now();
        Self {
            id: Ulid::new().to_string(),
            content_type: ContentType::Text,
            text_content: Some(text),
            blob_hash: None,
            blob_size: None,
            source_app: None,
            starred: false,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn new_html(html: String) -> Self {
        let now = Utc::now();
        Self {
            id: Ulid::new().to_string(),
            content_type: ContentType::Html,
            text_content: Some(html),
            blob_hash: None,
            blob_size: None,
            source_app: None,
            starred: false,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn new_image(blob_hash: String, blob_size: u64) -> Self {
        let now = Utc::now();
        Self {
            id: Ulid::new().to_string(),
            content_type: ContentType::Image,
            text_content: None,
            blob_hash: Some(blob_hash),
            blob_size: Some(blob_size),
            source_app: None,
            starred: false,
            created_at: now,
            updated_at: now,
        }
    }

    /// Returns a short preview of the content for display
    pub fn preview(&self, max_len: usize) -> String {
        match &self.text_content {
            Some(text) => {
                let trimmed = text.trim();
                if trimmed.len() <= max_len {
                    trimmed.to_string()
                } else {
                    format!("{}...", &trimmed[..max_len])
                }
            }
            None => match self.content_type {
                ContentType::Image => "[Image]".to_string(),
                ContentType::File => "[File]".to_string(),
                _ => "[Empty]".to_string(),
            },
        }
    }
}
