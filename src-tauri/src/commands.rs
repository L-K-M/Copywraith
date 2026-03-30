use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use tauri::State;

use crate::models::{EntryForFrontend, Settings};
use crate::AppState;
use copywraith_core::models::ContentType;

#[cfg(desktop)]
use crate::paste;

#[tauri::command]
pub async fn get_entries(
    state: State<'_, AppState>,
    limit: Option<u32>,
    offset: Option<u32>,
    starred_only: Option<bool>,
    search: Option<String>,
) -> Result<Vec<EntryForFrontend>, String> {
    let entries = state
        .storage
        .get_entries(
            limit.unwrap_or(50),
            offset.unwrap_or(0),
            starred_only.unwrap_or(false),
            search.as_deref(),
        )
        .map_err(|e| e.to_string())?;

    let result: Vec<EntryForFrontend> = entries
        .into_iter()
        .map(|e| {
            let preview = e.preview(200);

            // For sensitive entries, mask the full text so we never send
            // secrets to the frontend JS context.
            let full_text = if e.sensitive {
                e.text_content.map(|t| {
                    let plain = match e.content_type {
                        ContentType::Html => copywraith_core::content::strip_html(&t),
                        ContentType::Rtf => copywraith_core::content::strip_rtf(&t),
                        _ => t.trim().to_string(),
                    };
                    copywraith_core::content::mask_sensitive(&plain, 200)
                })
            } else {
                e.text_content
            };

            EntryForFrontend {
                id: e.id,
                content_type: e.content_type,
                preview,
                full_text,
                has_image: e.content_type == ContentType::Image && e.blob_hash.is_some(),
                starred: e.starred,
                sensitive: e.sensitive,
                created_at: e.created_at.to_rfc3339(),
                updated_at: e.updated_at.to_rfc3339(),
                source_app: e.source_app,
            }
        })
        .collect();

    Ok(result)
}

#[tauri::command]
pub async fn get_entry_image(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<String>, String> {
    let entry = state
        .storage
        .get_entry(&id)
        .map_err(|e| e.to_string())?
        .ok_or("Entry not found")?;

    if entry.content_type != ContentType::Image {
        return Ok(None);
    }

    let image_base64 = entry
        .blob_hash
        .as_ref()
        .and_then(|hash| state.storage.get_blob(hash).ok().flatten())
        .map(|data| BASE64.encode(&data));

    Ok(image_base64)
}

#[tauri::command]
pub async fn toggle_star(state: State<'_, AppState>, id: String) -> Result<bool, String> {
    let starred = state.storage.toggle_star(&id).map_err(|e| e.to_string())?;

    if let Some(entry) = state.storage.get_entry(&id).map_err(|e| e.to_string())? {
        state.sync_client.sync_entry(&entry, &state.storage).await;
    }

    Ok(starred)
}

#[tauri::command]
pub async fn delete_entry(state: State<'_, AppState>, id: String) -> Result<bool, String> {
    state.storage.delete_entry(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn paste_entry(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let entry = state
        .storage
        .get_entry(&id)
        .map_err(|e| e.to_string())?
        .ok_or("Entry not found")?;

    #[cfg(desktop)]
    {
        match entry.content_type {
            ContentType::Image => {
                if let Some(hash) = &entry.blob_hash {
                    if let Some(data) = state.storage.get_blob(hash).map_err(|e| e.to_string())? {
                        paste::write_and_paste_image(&app, &data);
                    }
                }
            }
            ContentType::Html => {
                if let Some(text) = &entry.text_content {
                    let plaintext = strip_html(text);
                    paste::write_and_paste_text(&app, &plaintext);
                }
            }
            _ => {
                if let Some(text) = &entry.text_content {
                    paste::write_and_paste_text(&app, text);
                }
            }
        }
    }

    // On mobile, just write to clipboard (no paste simulation)
    #[cfg(mobile)]
    {
        write_to_clipboard_mobile(&app, &entry, false)?;
    }

    Ok(())
}

#[tauri::command]
pub async fn paste_entry_plaintext(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let entry = state
        .storage
        .get_entry(&id)
        .map_err(|e| e.to_string())?
        .ok_or("Entry not found")?;

    #[cfg(desktop)]
    {
        if let Some(text) = &entry.text_content {
            // Strip HTML tags for plaintext paste
            let plaintext = strip_html(text);
            paste::write_and_paste_text(&app, &plaintext);
        }
    }

    #[cfg(mobile)]
    {
        write_to_clipboard_mobile(&app, &entry, true)?;
    }

    Ok(())
}

/// On mobile, write entry content to the system clipboard.
/// The official clipboard-manager plugin supports plain text on Android.
#[cfg(mobile)]
fn write_to_clipboard_mobile(
    app: &tauri::AppHandle,
    entry: &copywraith_core::models::ClipboardEntry,
    force_plaintext: bool,
) -> Result<(), String> {
    use tauri_plugin_clipboard_manager::ClipboardExt;

    let text = match &entry.text_content {
        Some(t) => {
            if force_plaintext || entry.content_type == ContentType::Html {
                strip_html(t)
            } else {
                t.clone()
            }
        }
        None => {
            if entry.content_type == ContentType::Image {
                // Android clipboard-manager only supports text; image copy not available
                return Ok(());
            }
            return Ok(());
        }
    };

    app.clipboard()
        .write_text(text)
        .map_err(|e| format!("Failed to write to clipboard: {}", e))
}

/// Capture the current system clipboard and save it as a new entry.
/// Called when the mobile app opens or resumes to persist whatever the user
/// last copied in another app.
#[tauri::command]
pub async fn capture_clipboard(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    // On desktop, clipboard monitoring handles this automatically
    #[cfg(desktop)]
    {
        let _ = &app;
        let _ = &state;
        return Ok(false);
    }

    #[cfg(mobile)]
    {
        use tauri::Emitter;
        use tauri_plugin_clipboard_manager::ClipboardExt;

        let text = app
            .clipboard()
            .read_text()
            .map_err(|e| format!("Failed to read clipboard: {}", e))?;

        if text.trim().is_empty() {
            return Ok(false);
        }

        let content_hash = copywraith_core::content::hash_text(&text);
        match state
            .storage
            .insert_entry(ContentType::Text, Some(&text), None, &content_hash, None)
        {
            Ok(Some(entry)) => {
                let _ = app.emit("clipboard-updated", &entry);
                // Trigger background sync for the new entry
                let sync = state.sync_client.clone();
                let storage = state.storage.clone();
                tauri::async_runtime::spawn(async move {
                    sync.sync_entry(&entry, &storage).await;
                });
                Ok(true)
            }
            Ok(None) => Ok(false), // duplicate content
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Returns the current platform so the frontend can adapt its UI.
/// Returns "android", "ios", "macos", "windows", or "linux".
#[tauri::command]
pub async fn get_platform() -> String {
    #[cfg(target_os = "android")]
    return "android".to_string();
    #[cfg(target_os = "ios")]
    return "ios".to_string();
    #[cfg(target_os = "macos")]
    return "macos".to_string();
    #[cfg(target_os = "windows")]
    return "windows".to_string();
    #[cfg(target_os = "linux")]
    return "linux".to_string();
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<Settings, String> {
    Ok(state.storage.get_settings())
}

#[tauri::command]
pub async fn update_settings(state: State<'_, AppState>, settings: Settings) -> Result<(), String> {
    state
        .storage
        .save_settings(&settings)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn reregister_shortcuts(
    #[allow(unused_variables)] app: tauri::AppHandle,
    #[allow(unused_variables)] state: State<'_, AppState>,
) -> Result<(), String> {
    #[cfg(desktop)]
    {
        let settings = state.storage.get_settings();
        crate::register_shortcuts(&app, &settings);
    }
    Ok(())
}

#[tauri::command]
pub async fn hide_popup(app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(desktop)]
    {
        crate::hide_popup_window(&app);
    }

    #[cfg(mobile)]
    {
        let _ = app;
    }

    Ok(())
}

/// Simple HTML tag stripping
fn strip_html(html: &str) -> String {
    copywraith_core::content::strip_html(html)
}
