use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use tauri::{Emitter, State};

#[cfg(target_os = "android")]
use tauri::Manager;

use crate::models::{EntryForFrontend, Settings};
use crate::sync;
use crate::AppState;
use copywraith_core::models::ContentType;

#[cfg(mobile)]
use copywraith_core::models::ClipboardFlavors;

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
            let plain_text = e.best_plain_text();

            // For sensitive entries, mask the full text so we never send
            // secrets to the frontend JS context.
            let full_text = if e.sensitive {
                plain_text
                    .as_ref()
                    .map(|plain| copywraith_core::content::mask_sensitive(plain, 200))
            } else {
                plain_text
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
            ContentType::File => {
                let flavors = entry.resolved_flavors();
                if let Some(files) = flavors.file_list.as_deref() {
                    paste::write_and_paste_files(&app, files);
                }
            }
            _ => {
                paste::write_and_paste_flavors(&app, &entry.resolved_flavors());
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
        if let Some(plaintext) = entry.best_plain_text() {
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

    if entry.content_type == ContentType::Image {
        // Android clipboard-manager only supports text; image copy not available
        return Ok(());
    }

    let text = if force_plaintext {
        entry.best_plain_text()
    } else {
        let flavors = entry.resolved_flavors();
        flavors
            .text_plain
            .clone()
            .or_else(|| flavors.best_plain_text())
            .or_else(|| entry.text_content.clone())
    }
    .unwrap_or_default();

    if text.trim().is_empty() {
        return Ok(());
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

        let flavors = ClipboardFlavors {
            text_plain: Some(text.clone()),
            ..ClipboardFlavors::default()
        };
        let content_hash = flavors.payload_hash(ContentType::Text, None);
        match state
            .storage
            .insert_entry(ContentType::Text, &flavors, None, &content_hash, None)
        {
            Ok(Some(entry)) => {
                let _ = app.emit("clipboard-updated", entry.clone());
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

#[cfg(target_os = "android")]
#[derive(Debug, serde::Deserialize)]
struct PendingShareBatch {
    #[serde(default)]
    items: Vec<PendingShareItem>,
}

#[cfg(target_os = "android")]
#[derive(Debug, serde::Deserialize)]
struct PendingShareItem {
    #[serde(rename = "type")]
    item_type: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    mime_type: Option<String>,
    #[serde(default)]
    file_name: Option<String>,
    #[serde(default)]
    stored_path: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct ImportPendingSharesResult {
    pub imported: usize,
    pub skipped: usize,
}

/// Import Android share-sheet payloads persisted by the native Activity.
#[tauri::command]
pub async fn import_pending_shares(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<ImportPendingSharesResult, String> {
    #[cfg(not(target_os = "android"))]
    {
        let _ = &app;
        let _ = &state;
        return Ok(ImportPendingSharesResult {
            imported: 0,
            skipped: 0,
        });
    }

    #[cfg(target_os = "android")]
    {
        let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
        let pending_dir = data_dir.join("pending-shares");
        let processed_dir = pending_dir.join("processed");
        let failed_dir = pending_dir.join("failed");
        std::fs::create_dir_all(&processed_dir).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&failed_dir).map_err(|e| e.to_string())?;

        let mut imported = 0usize;
        let mut skipped = 0usize;

        let entries = match std::fs::read_dir(&pending_dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ImportPendingSharesResult { imported, skipped });
            }
            Err(e) => return Err(e.to_string()),
        };

        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }

            let import_result = import_pending_share_file(&app, &state, &path);
            let target_dir = if import_result.is_ok() {
                &processed_dir
            } else {
                &failed_dir
            };
            let target = target_dir.join(
                path.file_name()
                    .unwrap_or_else(|| std::ffi::OsStr::new("share.json")),
            );
            let _ = std::fs::rename(&path, target);

            match import_result {
                Ok((new_imports, new_skips)) => {
                    imported += new_imports;
                    skipped += new_skips;
                }
                Err(e) => {
                    skipped += 1;
                    log::warn!("Failed to import pending Android share {:?}: {}", path, e);
                }
            }
        }

        if imported > 0 {
            let _ = app.emit("clipboard-updated", ());
        }

        Ok(ImportPendingSharesResult { imported, skipped })
    }
}

#[cfg(target_os = "android")]
fn import_pending_share_file(
    app: &tauri::AppHandle,
    state: &State<'_, AppState>,
    path: &std::path::Path,
) -> anyhow::Result<(usize, usize)> {
    let data = std::fs::read_to_string(path)?;
    let batch: PendingShareBatch = serde_json::from_str(&data)?;
    let mut imported = 0usize;
    let mut skipped = 0usize;

    for item in batch.items {
        match import_pending_share_item(state, &item) {
            Ok(Some(entry)) => {
                imported += 1;
                let _ = app.emit("clipboard-updated", &entry);
                let sync = state.sync_client.clone();
                let storage = state.storage.clone();
                tauri::async_runtime::spawn(async move {
                    sync.sync_entry(&entry, &storage).await;
                });
            }
            Ok(None) => skipped += 1,
            Err(e) => {
                skipped += 1;
                log::warn!("Failed to import Android share item from {:?}: {}", path, e);
            }
        }
    }

    Ok((imported, skipped))
}

#[cfg(target_os = "android")]
fn import_pending_share_item(
    state: &State<'_, AppState>,
    item: &PendingShareItem,
) -> anyhow::Result<Option<copywraith_core::models::ClipboardEntry>> {
    match item.item_type.as_str() {
        "text" => import_pending_text_share(state, item),
        "file" => import_pending_file_share(state, item),
        other => {
            log::warn!("Skipping unsupported Android share item type: {}", other);
            Ok(None)
        }
    }
}

#[cfg(target_os = "android")]
fn import_pending_text_share(
    state: &State<'_, AppState>,
    item: &PendingShareItem,
) -> anyhow::Result<Option<copywraith_core::models::ClipboardEntry>> {
    let Some(text) = item.text.as_ref().filter(|text| !text.trim().is_empty()) else {
        return Ok(None);
    };
    let flavors = ClipboardFlavors {
        text_plain: Some(text.clone()),
        ..ClipboardFlavors::default()
    };
    let content_hash = flavors.payload_hash(ContentType::Text, None);
    state.storage.insert_entry(
        ContentType::Text,
        &flavors,
        None,
        &content_hash,
        Some("Android share sheet"),
    )
}

#[cfg(target_os = "android")]
fn import_pending_file_share(
    state: &State<'_, AppState>,
    item: &PendingShareItem,
) -> anyhow::Result<Option<copywraith_core::models::ClipboardEntry>> {
    const MAX_SHARED_BLOB_BYTES: u64 = 64 * 1024 * 1024;

    let Some(stored_path) = item.stored_path.as_deref() else {
        return Ok(None);
    };
    let path = std::path::Path::new(stored_path);
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_SHARED_BLOB_BYTES {
        return Ok(None);
    }

    let bytes = std::fs::read(path)?;
    let _ = std::fs::remove_file(path);
    if bytes.is_empty() {
        return Ok(None);
    }

    if item
        .mime_type
        .as_deref()
        .is_some_and(|mime| mime.starts_with("image/"))
        || copywraith_core::content::detect_image_format(&bytes).is_some()
    {
        let content_hash = copywraith_core::content::hash_bytes(&bytes);
        return state.storage.insert_entry(
            ContentType::Image,
            &ClipboardFlavors::default(),
            Some(&bytes),
            &content_hash,
            Some("Android share sheet"),
        );
    }

    let display_name = item
        .file_name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| {
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("shared-file")
        });
    let flavors = ClipboardFlavors {
        file_list: Some(vec![display_name.to_string()]),
        ..ClipboardFlavors::default()
    };
    let blob_hash = copywraith_core::content::hash_bytes(&bytes);
    let content_hash = flavors.payload_hash(ContentType::File, Some(&blob_hash));
    state.storage.insert_entry(
        ContentType::File,
        &flavors,
        Some(&bytes),
        &content_hash,
        Some("Android share sheet"),
    )
}

#[tauri::command]
pub async fn sync_now(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<sync::PullSyncResult, String> {
    let storage = state.storage.clone();
    let sync_client = state.sync_client.clone();
    let configured_endpoint = sync::first_configured_endpoint(&storage);

    let _ = app.emit(
        "sync-endpoint-status",
        sync::checking_status_for_configured_endpoint(
            configured_endpoint.as_ref(),
            "Manual sync check started.",
        ),
    );

    let _ = app.emit(
        "sync-endpoint-status",
        sync::checking_status_for_configured_endpoint(
            configured_endpoint.as_ref(),
            "Pushing local unsynced entries to the server.",
        ),
    );

    if tokio::time::timeout(
        std::time::Duration::from_secs(35),
        sync_client.sync_unsynced_entries(&storage),
    )
    .await
    .is_err()
    {
        let status = configured_endpoint
            .as_ref()
            .map(|endpoint| {
                sync::SyncEndpointStatus::unreachable_endpoint(
                    endpoint,
                    "Timed out while pushing local unsynced entries.",
                )
            })
            .unwrap_or_else(|| sync::checking_status(&storage, "No configured server endpoint."));
        let _ = app.emit("sync-endpoint-status", &status);
        return Ok(sync::PullSyncResult {
            pulled: 0,
            endpoint_status: status,
        });
    }

    let _ = app.emit(
        "sync-endpoint-status",
        sync::checking_status_for_configured_endpoint(
            configured_endpoint.as_ref(),
            "Pulling remote entries from the server.",
        ),
    );

    let pull_result = tokio::time::timeout(
        std::time::Duration::from_secs(35),
        sync_client.pull_new_entries(&storage),
    )
    .await;

    match pull_result {
        Err(_) => {
            let status = configured_endpoint
                .as_ref()
                .map(|endpoint| {
                    sync::SyncEndpointStatus::unreachable_endpoint(
                        endpoint,
                        "Timed out while pulling remote entries.",
                    )
                })
                .unwrap_or_else(|| sync::checking_status(&storage, "No configured server endpoint."));
            let _ = app.emit("sync-endpoint-status", &status);
            Ok(sync::PullSyncResult {
                pulled: 0,
                endpoint_status: status,
            })
        }
        Ok(Ok(result)) => {
            let _ = app.emit("sync-endpoint-status", &result.endpoint_status);
            if result.pulled > 0 {
                let _ = app.emit("clipboard-updated", ());
            }
            Ok(result)
        }
        Ok(Err(e)) => {
            let fallback_status = configured_endpoint
                .as_ref()
                .map(|endpoint| {
                    sync::SyncEndpointStatus::unreachable_endpoint(endpoint, e.to_string())
                })
                .unwrap_or_else(|| sync::SyncEndpointStatus {
                    state: "unreachable".to_string(),
                    role: None,
                    url: None,
                    message: Some(e.to_string()),
                    checked_at: Some(chrono::Utc::now().to_rfc3339()),
                });
            let _ = app.emit(
                "sync-endpoint-status",
                &fallback_status,
            );
            Err(e.to_string())
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
