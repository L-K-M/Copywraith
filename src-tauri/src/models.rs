use serde::{Deserialize, Serialize};

use copywraith_core::models::ContentType;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryForFrontend {
    pub id: String,
    pub content_type: ContentType,
    pub preview: String,
    pub full_text: Option<String>,
    pub has_image: bool,
    pub starred: bool,
    pub created_at: String,
    pub updated_at: String,
    pub source_app: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub server_url: String,
    pub api_key: String,
    pub shortcut_toggle_popup: String,
    pub shortcut_starred_popup: String,
    pub shortcut_paste_plaintext: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            server_url: String::new(),
            api_key: String::new(),
            shortcut_toggle_popup: "CmdOrCtrl+Shift+V".to_string(),
            shortcut_starred_popup: "CmdOrCtrl+Shift+B".to_string(),
            shortcut_paste_plaintext: "CmdOrCtrl+Shift+Alt+V".to_string(),
        }
    }
}
