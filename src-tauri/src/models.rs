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

/// Whether clipboard capture is paused ("the ghost sleeps").
///
/// Kept in memory only: a restart always resumes capture, so a forgotten
/// indefinite pause cannot silently stop history for good.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CapturePause {
    #[default]
    Active,
    Until(chrono::DateTime<chrono::Utc>),
    Indefinite,
}

impl CapturePause {
    pub fn is_paused_at(self, now: chrono::DateTime<chrono::Utc>) -> bool {
        match self {
            CapturePause::Active => false,
            CapturePause::Until(until) => now < until,
            CapturePause::Indefinite => true,
        }
    }

    /// The frontend view at `now`; an expired timed pause reads as active.
    pub fn status_at(self, now: chrono::DateTime<chrono::Utc>) -> CapturePauseStatus {
        match self {
            CapturePause::Until(until) if now < until => CapturePauseStatus {
                paused: true,
                until: Some(until.to_rfc3339()),
            },
            CapturePause::Indefinite => CapturePauseStatus {
                paused: true,
                until: None,
            },
            _ => CapturePauseStatus {
                paused: false,
                until: None,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CapturePauseStatus {
    pub paused: bool,
    /// When a timed pause ends (RFC 3339); `None` while paused means until resumed.
    pub until: Option<String>,
}

#[cfg(test)]
mod capture_pause_tests {
    use super::*;

    fn at(value: &str) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339(value)
            .unwrap()
            .with_timezone(&chrono::Utc)
    }

    #[test]
    fn a_timed_pause_ends_by_itself() {
        let pause = CapturePause::Until(at("2026-09-25T12:05:00Z"));

        assert!(pause.is_paused_at(at("2026-09-25T12:04:59Z")));
        assert!(!pause.is_paused_at(at("2026-09-25T12:05:00Z")));
        assert_eq!(
            pause.status_at(at("2026-09-25T12:06:00Z")),
            CapturePauseStatus {
                paused: false,
                until: None
            }
        );
        assert_eq!(
            pause.status_at(at("2026-09-25T12:00:00Z")).until.as_deref(),
            Some("2026-09-25T12:05:00+00:00")
        );
    }

    #[test]
    fn an_indefinite_pause_lasts_until_resumed() {
        let now = at("2030-01-01T00:00:00Z");

        assert!(CapturePause::Indefinite.is_paused_at(now));
        assert!(!CapturePause::Active.is_paused_at(now));
        assert_eq!(
            CapturePause::Indefinite.status_at(now),
            CapturePauseStatus {
                paused: true,
                until: None
            }
        );
    }
}
