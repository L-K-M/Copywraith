use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use tauri::State;

use crate::models::{EntryForFrontend, Settings};
use crate::paste;
use crate::AppState;
use copywraith_core::models::ContentType;

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

    if let Some(text) = &entry.text_content {
        // Strip HTML tags for plaintext paste
        let plaintext = strip_html(text);
        paste::write_and_paste_text(&app, &plaintext);
    }

    Ok(())
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
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let settings = state.storage.get_settings();
    crate::register_shortcuts(&app, &settings);
    Ok(())
}

/// Simple HTML tag stripping
fn strip_html(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;

    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }

    result
}
