use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ulid::Ulid;
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct ClipboardFlavors {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_plain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_html: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_rtf: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_list: Option<Vec<String>>,
}

impl ClipboardFlavors {
    pub fn is_empty(&self) -> bool {
        self.text_plain.is_none()
            && self.text_html.is_none()
            && self.text_rtf.is_none()
            && self.file_list.is_none()
    }

    pub fn from_legacy(content_type: ContentType, text_content: Option<&str>) -> Self {
        match content_type {
            ContentType::Text => Self {
                text_plain: non_empty_text(text_content),
                ..Self::default()
            },
            ContentType::Html => Self {
                text_html: non_empty_text(text_content),
                ..Self::default()
            },
            ContentType::Rtf => Self {
                text_rtf: non_empty_text(text_content),
                ..Self::default()
            },
            ContentType::File => Self {
                file_list: parse_file_list(text_content),
                ..Self::default()
            },
            ContentType::Image => Self::default(),
        }
    }

    pub fn merge_legacy(mut self, content_type: ContentType, text_content: Option<&str>) -> Self {
        let legacy = Self::from_legacy(content_type, text_content);

        if self.text_plain.is_none() {
            self.text_plain = legacy.text_plain;
        }
        if self.text_html.is_none() {
            self.text_html = legacy.text_html;
        }
        if self.text_rtf.is_none() {
            self.text_rtf = legacy.text_rtf;
        }
        if self.file_list.is_none() {
            self.file_list = legacy.file_list;
        }

        self
    }

    pub fn to_legacy_text_content(&self, content_type: ContentType) -> Option<String> {
        match content_type {
            ContentType::Text => self.text_plain.clone().or_else(|| self.best_plain_text()),
            ContentType::Html => self.text_html.clone().or_else(|| self.text_plain.clone()),
            ContentType::Rtf => self.text_rtf.clone().or_else(|| self.text_plain.clone()),
            ContentType::File => self
                .file_list
                .as_ref()
                .map(|paths| paths.join("\n"))
                .filter(|s| !s.is_empty()),
            ContentType::Image => None,
        }
    }

    pub fn best_plain_text(&self) -> Option<String> {
        if let Some(text) = self.text_plain.as_ref().filter(|t| !t.trim().is_empty()) {
            return Some(text.trim().to_string());
        }

        if let Some(html) = self.text_html.as_ref().filter(|t| !t.trim().is_empty()) {
            let stripped = crate::content::strip_html(html);
            if !stripped.is_empty() {
                return Some(stripped);
            }
        }

        if let Some(rtf) = self.text_rtf.as_ref().filter(|t| !t.trim().is_empty()) {
            let stripped = crate::content::strip_rtf(rtf);
            if !stripped.is_empty() {
                return Some(stripped);
            }
        }

        self.file_list
            .as_ref()
            .filter(|paths| !paths.is_empty())
            .map(|paths| paths.join("\n"))
    }

    pub fn payload_hash(&self, content_type: ContentType, blob_hash: Option<&str>) -> String {
        if content_type == ContentType::Image {
            if let Some(hash) = blob_hash {
                return hash.to_string();
            }
        }

        let has_plain = self.text_plain.is_some();
        let has_html = self.text_html.is_some();
        let has_rtf = self.text_rtf.is_some();
        let has_files = self.file_list.is_some();

        // Keep legacy hash behavior for single-flavor entries so migration remains
        // stable with existing content_hash values.
        if blob_hash.is_none() {
            match content_type {
                ContentType::Text if has_plain && !has_html && !has_rtf && !has_files => {
                    if let Some(text) = self.text_plain.as_deref() {
                        return crate::content::hash_text(text);
                    }
                }
                ContentType::Html if !has_plain && has_html && !has_rtf && !has_files => {
                    if let Some(html) = self.text_html.as_deref() {
                        return crate::content::hash_text(html);
                    }
                }
                ContentType::Rtf if !has_plain && !has_html && has_rtf && !has_files => {
                    if let Some(rtf) = self.text_rtf.as_deref() {
                        return crate::content::hash_text(rtf);
                    }
                }
                ContentType::File if !has_plain && !has_html && !has_rtf && has_files => {
                    if let Some(file_list) = self.file_list.as_ref() {
                        return crate::content::hash_text(&file_list.join("\n"));
                    }
                }
                _ => {}
            }
        }

        #[derive(Serialize)]
        struct PayloadHashInput<'a> {
            content_type: &'a str,
            text_plain: Option<&'a str>,
            text_html: Option<&'a str>,
            text_rtf: Option<&'a str>,
            file_list: Option<&'a [String]>,
            blob_hash: Option<&'a str>,
        }

        let payload = PayloadHashInput {
            content_type: content_type.as_str(),
            text_plain: self.text_plain.as_deref(),
            text_html: self.text_html.as_deref(),
            text_rtf: self.text_rtf.as_deref(),
            file_list: self.file_list.as_deref(),
            blob_hash,
        };

        let serialized = serde_json::to_vec(&payload)
            .unwrap_or_else(|_| format!("{:?}", content_type).into_bytes());
        crate::content::hash_bytes(&serialized)
    }
}

fn non_empty_text(text: Option<&str>) -> Option<String> {
    text.filter(|t| !t.trim().is_empty()).map(|t| t.to_string())
}

fn parse_file_list(text: Option<&str>) -> Option<Vec<String>> {
    let value = text?;
    let files: Vec<String> = value
        .split('\n')
        .map(|line| line.trim_end_matches('\r'))
        .filter(|line| !line.is_empty())
        .map(|line| line.to_string())
        .collect();
    if files.is_empty() {
        None
    } else {
        Some(files)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
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
    #[serde(default, skip_serializing_if = "ClipboardFlavors::is_empty")]
    pub flavors: ClipboardFlavors,
    pub starred: bool,
    #[serde(default)]
    pub sensitive: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ClipboardEntry {
    pub fn resolved_flavors(&self) -> ClipboardFlavors {
        self.flavors
            .clone()
            .merge_legacy(self.content_type, self.text_content.as_deref())
    }

    pub fn best_plain_text(&self) -> Option<String> {
        self.resolved_flavors().best_plain_text()
    }

    pub fn new_text(text: String) -> Self {
        let now = Utc::now();
        let sensitive = crate::sensitive::contains_sensitive_data(&text);
        let flavors = ClipboardFlavors {
            text_plain: Some(text.clone()),
            ..ClipboardFlavors::default()
        };
        Self {
            id: Ulid::new().to_string(),
            content_type: ContentType::Text,
            text_content: Some(text),
            blob_hash: None,
            blob_size: None,
            source_app: None,
            flavors,
            starred: false,
            sensitive,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn new_html(html: String) -> Self {
        let now = Utc::now();
        let sensitive = crate::sensitive::contains_sensitive_data(&html);
        let flavors = ClipboardFlavors {
            text_html: Some(html.clone()),
            ..ClipboardFlavors::default()
        };
        Self {
            id: Ulid::new().to_string(),
            content_type: ContentType::Html,
            text_content: Some(html),
            blob_hash: None,
            blob_size: None,
            source_app: None,
            flavors,
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
            flavors: ClipboardFlavors::default(),
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
            if let Some(plain) = self.best_plain_text() {
                return crate::content::mask_sensitive(&plain, max_len);
            }
            return "[Sensitive]".to_string();
        }

        if let Some(plain) = self.best_plain_text() {
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
        } else {
            match self.content_type {
                ContentType::Image => "[Image]".to_string(),
                ContentType::File => "[File]".to_string(),
                _ => "[Empty]".to_string(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_hash_keeps_legacy_single_text_behavior() {
        let flavors = ClipboardFlavors {
            text_plain: Some("hello".to_string()),
            ..ClipboardFlavors::default()
        };

        assert_eq!(
            flavors.payload_hash(ContentType::Text, None),
            crate::content::hash_text("hello")
        );
    }

    #[test]
    fn payload_hash_changes_for_multi_flavor_payloads() {
        let plain_only = ClipboardFlavors {
            text_plain: Some("hello".to_string()),
            ..ClipboardFlavors::default()
        };

        let with_html = ClipboardFlavors {
            text_plain: Some("hello".to_string()),
            text_html: Some("<b>hello</b>".to_string()),
            ..ClipboardFlavors::default()
        };

        assert_ne!(
            plain_only.payload_hash(ContentType::Text, None),
            with_html.payload_hash(ContentType::Text, None)
        );
    }
}
