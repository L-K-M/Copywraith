use serde::{Deserialize, Serialize};

use copywraith_core::models::ContentType;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryForFrontend {
    pub id: String,
    pub content_type: ContentType,
    pub preview: String,
    /// Plain text for the preview dialog, bounded so the list stays cheap to
    /// send over IPC. When `full_text_truncated` is set this is only a prefix.
    pub full_text: Option<String>,
    /// True when `full_text` was cut short and the complete text must be
    /// fetched with `get_entry_text`.
    pub full_text_truncated: bool,
    pub has_image: bool,
    pub starred: bool,
    pub sensitive: bool,
    pub created_at: String,
    pub updated_at: String,
    pub source_app: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Settings {
    pub server_url_primary: String,
    pub server_url_fallback: String,
    pub api_key: String,
    pub shortcut_toggle_popup: String,
    pub shortcut_starred_popup: String,
    pub shortcut_paste_plaintext: String,
    #[serde(default)]
    pub shizuku_clipboard_enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            server_url_primary: String::new(),
            server_url_fallback: String::new(),
            api_key: String::new(),
            shortcut_toggle_popup: "CmdOrCtrl+Shift+V".to_string(),
            shortcut_starred_popup: "CmdOrCtrl+Shift+B".to_string(),
            shortcut_paste_plaintext: "CmdOrCtrl+Shift+Alt+V".to_string(),
            shizuku_clipboard_enabled: false,
        }
    }
}

/// How Copywraith's global shortcuts are bound, for display in Settings.
///
/// Only Linux has more than one answer here: Wayland forbids in-app key grabs,
/// so the shortcuts are handed to the desktop environment instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortcutStatus {
    /// `in_process`, `gnome`, `kde`, `kde_connecting`, `kde_unavailable`, `manual`, or `unsupported`.
    pub mechanism: String,
    /// One sentence explaining the mechanism to the user.
    pub message: String,
    /// Commands to bind by hand when native registration is unavailable.
    pub commands: Vec<ShortcutCommand>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortcutCommand {
    pub label: String,
    pub accelerator: String,
    pub command: String,
}

impl Default for ShortcutStatus {
    fn default() -> Self {
        Self {
            mechanism: "in_process".to_string(),
            message: String::new(),
            commands: Vec::new(),
        }
    }
}
