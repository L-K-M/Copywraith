use serde::{Deserialize, Serialize};

use copywraith_core::models::ContentType;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryForFrontend {
    pub id: String,
    pub content_type: ContentType,
    pub preview: String,
    pub full_text: Option<String>,
    pub image_base64: Option<String>,
    pub starred: bool,
    pub created_at: String,
    pub updated_at: String,
    pub source_app: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub server_url: String,
    pub api_key: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            server_url: String::new(),
            api_key: String::new(),
        }
    }
}
