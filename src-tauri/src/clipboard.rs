use std::path::Path;
use std::sync::Arc;

use copywraith_core::content::{hash_bytes, hash_text};
use copywraith_core::models::ContentType;
use tauri::{Emitter, Listener, Manager};
use tauri_plugin_clipboard::Clipboard;

use crate::storage::LocalStorage;
use crate::sync::SyncClient;

/// Start monitoring the clipboard for changes.
///
/// Uses the tauri-plugin-clipboard Rust API directly:
/// 1. Starts the native clipboard monitor via `Clipboard::start_monitor()`
/// 2. Listens for the single generic `plugin:clipboard://clipboard-monitor/update` event
/// 3. On each change, reads clipboard contents using has_*/read_* methods
/// 4. Stores new entries in local SQLite and triggers server sync
pub fn start_monitoring(
    app: tauri::AppHandle,
    storage: Arc<LocalStorage>,
    sync_client: Arc<SyncClient>,
) {
    // Start the native clipboard monitor from Rust (no JS dependency)
    let clipboard = app.state::<Clipboard>();
    if let Err(e) = clipboard.start_monitor(app.clone()) {
        log::error!("Failed to start clipboard monitor: {}", e);
        return;
    }
    log::info!("Clipboard monitor started");

    // Listen for the single generic clipboard change event
    // The plugin emits this for ALL clipboard changes (text, image, html, files, etc.)
    let app_clone = app.clone();
    app.listen(
        "plugin:clipboard://clipboard-monitor/update",
        move |_event| {
            let clipboard = app_clone.state::<Clipboard>();
            handle_clipboard_change(&app_clone, &clipboard, &storage, &sync_client);
        },
    );
}

/// Handle a clipboard change by reading current clipboard contents and storing them.
///
/// Priority order: Image > File > HTML > RTF > Text
/// We pick the richest content type available. If an image is present, we store
/// the image so the UI can render a thumbnail preview. If files are present, we
/// store them as a file list. For
/// text-based content, we prefer HTML > RTF > plain text, except Office-style
/// HTML wrappers where we prefer plain text to avoid storing raw `<html ...>`
/// markup for normal text copies from Word.
fn handle_clipboard_change(
    app: &tauri::AppHandle,
    clipboard: &Clipboard,
    storage: &Arc<LocalStorage>,
    sync_client: &Arc<SyncClient>,
) {
    let plain_text = read_plain_text(clipboard);
    let source_app = detect_source_app_name();

    // Check for image first so copied screenshots/files with image payload
    // are stored as image entries and shown with previews in the UI.
    if clipboard.has_image().unwrap_or(false) {
        if let Ok(b64) = clipboard.read_image_base64() {
            if !b64.is_empty() {
                if let Ok(bytes) = copywraith_core::content::base64_to_bytes(&b64) {
                    if !bytes.is_empty() {
                        let content_hash = hash_bytes(&bytes);
                        store_entry(
                            app,
                            storage,
                            sync_client,
                            ContentType::Image,
                            None,
                            Some(&bytes),
                            &content_hash,
                            source_app.as_deref(),
                        );
                        return;
                    }
                }
            }
        }
    }

    // Check for files
    if clipboard.has_files().unwrap_or(false) {
        if let Ok(files) = clipboard.read_files() {
            if !files.is_empty() {
                // Some apps copy image files without native image payload.
                // In that case, attempt to read the first image path and store
                // it as an image entry so previews work as expected.
                if let Some(bytes) = read_first_image_file(&files) {
                    let content_hash = hash_bytes(&bytes);
                    store_entry(
                        app,
                        storage,
                        sync_client,
                        ContentType::Image,
                        None,
                        Some(&bytes),
                        &content_hash,
                        source_app.as_deref(),
                    );
                    return;
                }

                let file_list = files.join("\n");
                let content_hash = hash_text(&file_list);
                store_entry(
                    app,
                    storage,
                    sync_client,
                    ContentType::File,
                    Some(&file_list),
                    None,
                    &content_hash,
                    source_app.as_deref(),
                );
                return;
            }
        }
    }

    // Check for HTML
    if clipboard.has_html().unwrap_or(false) {
        if let Ok(html) = clipboard.read_html() {
            if !html.trim().is_empty() {
                if should_prefer_plain_text_for_html(&html) {
                    if let Some(text) = plain_text.as_ref() {
                        let content_hash = hash_text(text);
                        store_entry(
                            app,
                            storage,
                            sync_client,
                            ContentType::Text,
                            Some(text),
                            None,
                            &content_hash,
                            source_app.as_deref(),
                        );
                        return;
                    }
                }

                let content_hash = hash_text(&html);
                store_entry(
                    app,
                    storage,
                    sync_client,
                    ContentType::Html,
                    Some(&html),
                    None,
                    &content_hash,
                    source_app.as_deref(),
                );
                return;
            }
        }
    }

    // Check for RTF
    if clipboard.has_rtf().unwrap_or(false) {
        if let Ok(rtf) = clipboard.read_rtf() {
            if !rtf.is_empty() {
                let content_hash = hash_text(&rtf);
                store_entry(
                    app,
                    storage,
                    sync_client,
                    ContentType::Rtf,
                    Some(&rtf),
                    None,
                    &content_hash,
                    source_app.as_deref(),
                );
                return;
            }
        }
    }

    // Fall back to plain text
    if let Some(text) = plain_text {
        let content_hash = hash_text(&text);
        store_entry(
            app,
            storage,
            sync_client,
            ContentType::Text,
            Some(&text),
            None,
            &content_hash,
            source_app.as_deref(),
        );
    }
}

fn read_plain_text(clipboard: &Clipboard) -> Option<String> {
    if !clipboard.has_text().unwrap_or(false) {
        return None;
    }

    let text = clipboard.read_text().ok()?;
    if text.trim().is_empty() {
        return None;
    }

    Some(text)
}

fn should_prefer_plain_text_for_html(html: &str) -> bool {
    let lower = html.to_ascii_lowercase();
    lower.contains("urn:schemas-microsoft-com:office:office")
        || lower.contains("urn:schemas-microsoft-com:office:word")
        || lower.contains("class=\"msonormal\"")
        || lower.contains("mso-")
}

#[cfg(target_os = "macos")]
fn detect_source_app_name() -> Option<String> {
    use std::process::Command;

    let output = Command::new("osascript")
        .arg("-e")
        .arg("tell application \"System Events\" to get name of first process whose frontmost is true")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let app_name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if app_name.is_empty() {
        None
    } else {
        Some(app_name)
    }
}

#[cfg(not(target_os = "macos"))]
fn detect_source_app_name() -> Option<String> {
    None
}

/// Store a clipboard entry in local storage and trigger server sync.
fn store_entry(
    app: &tauri::AppHandle,
    storage: &Arc<LocalStorage>,
    sync_client: &Arc<SyncClient>,
    content_type: ContentType,
    text_content: Option<&str>,
    blob_content: Option<&[u8]>,
    content_hash: &str,
    source_app: Option<&str>,
) {
    match storage.insert_entry(content_type, text_content, blob_content, content_hash, source_app) {
        Ok(Some(entry)) => {
            let _ = app.emit("clipboard-updated", &entry);
            // Trigger background sync
            let sync = sync_client.clone();
            let storage = storage.clone();
            tauri::async_runtime::spawn(async move {
                sync.sync_entry(&entry, &storage).await;
            });
        }
        Ok(None) => {
            // Duplicate content — still notify frontend of potential reorder
            let _ = app.emit("clipboard-reordered", ());
        }
        Err(e) => {
            log::error!(
                "Failed to store clipboard entry ({:?}): {}",
                content_type,
                e
            );
        }
    }
}

fn read_first_image_file(files: &[String]) -> Option<Vec<u8>> {
    const MAX_IMAGE_FILE_BYTES: u64 = 32 * 1024 * 1024;

    for file_path in files {
        let path = Path::new(file_path);
        if !is_supported_image_path(path) {
            continue;
        }

        let Ok(metadata) = std::fs::metadata(path) else {
            continue;
        };
        if !metadata.is_file() || metadata.len() > MAX_IMAGE_FILE_BYTES {
            continue;
        }

        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        if bytes.is_empty() {
            continue;
        }

        if copywraith_core::content::detect_image_format(&bytes).is_some() {
            return Some(bytes);
        }
    }

    None
}

fn is_supported_image_path(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };

    matches!(
        ext.to_ascii_lowercase().as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff"
    )
}
