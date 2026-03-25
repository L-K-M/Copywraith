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

impl std::str::FromStr for ContentType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "text" => Ok(ContentType::Text),
            "html" => Ok(ContentType::Html),
            "rtf" => Ok(ContentType::Rtf),
            "image" => Ok(ContentType::Image),
            "file" => Ok(ContentType::File),
            other => Err(format!("unknown content type: {}", other)),
        }
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
    #[serde(default)]
    pub sensitive: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ClipboardEntry {
    pub fn new_text(text: String) -> Self {
        let now = Utc::now();
        let sensitive = crate::sensitive::contains_sensitive_data(&text);
        Self {
            id: Ulid::new().to_string(),
            content_type: ContentType::Text,
            text_content: Some(text),
            blob_hash: None,
            blob_size: None,
            source_app: None,
            starred: false,
            sensitive,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn new_html(html: String) -> Self {
        let now = Utc::now();
        let sensitive = crate::sensitive::contains_sensitive_data(&html);
        Self {
            id: Ulid::new().to_string(),
            content_type: ContentType::Html,
            text_content: Some(html),
            blob_hash: None,
            blob_size: None,
            source_app: None,
            starred: false,
            sensitive,
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
            sensitive: false,
            created_at: now,
            updated_at: now,
        }
    }

    /// Returns a short preview of the content for display.
    /// `max_len` is measured in characters, not bytes.
    /// HTML and RTF content is stripped to plain text.
    /// Sensitive entries show first 3 chars + bullet characters.
    pub fn preview(&self, max_len: usize) -> String {
        if self.sensitive {
            if let Some(text) = &self.text_content {
                let plain = match self.content_type {
                    ContentType::Html => crate::content::strip_html(text),
                    ContentType::Rtf => crate::content::strip_rtf(text),
                    _ => text.trim().to_string(),
                };
                return crate::content::mask_sensitive(&plain, max_len);
            }
            return "[Sensitive]".to_string();
        }

        match &self.text_content {
            Some(text) => {
                // Strip markup for display
                let plain = match self.content_type {
                    ContentType::Html => crate::content::strip_html(text),
                    ContentType::Rtf => crate::content::strip_rtf(text),
                    _ => text.trim().to_string(),
                };
                let char_count = plain.chars().count();
                if char_count <= max_len {
                    plain
                } else {
                    // Find the byte index of the max_len-th character boundary
                    let byte_end = plain
                        .char_indices()
                        .nth(max_len)
                        .map(|(i, _)| i)
                        .unwrap_or(plain.len());
                    format!("{}...", &plain[..byte_end])
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
