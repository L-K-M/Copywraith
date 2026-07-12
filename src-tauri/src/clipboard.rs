use std::path::Path;
use std::sync::Arc;

use copywraith_core::content::hash_bytes;
use copywraith_core::models::{ClipboardFlavors, ContentType};
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
            {
                let state = app_clone.state::<crate::AppState>();
                let suppress_guard = state.suppress_monitor_until.lock();
                if let Ok(guard) = suppress_guard {
                    if let Some(deadline) = *guard {
                        if std::time::Instant::now() < deadline {
                            log::debug!(
                                "Skipping self-triggered clipboard monitor event (suppress window active)"
                            );
                            return;
                        }
                    }
                };
            }

            let clipboard = app_clone.state::<Clipboard>();
            handle_clipboard_change(&app_clone, &clipboard, &storage, &sync_client);
        },
    );
}

/// Handle a clipboard change by reading current clipboard contents and storing them.
///
/// Priority order for primary entry type: Image > File > Text/HTML/RTF bundle.
/// If an image is present, we store the image so the UI can render a thumbnail
/// preview. If files are present, we store them as a file-list entry. For
/// text-based payloads we capture all available standard flavors together
/// (`text/plain`, `text/html`, `text/rtf`) in one logical entry.
fn handle_clipboard_change(
    app: &tauri::AppHandle,
    clipboard: &Clipboard,
    storage: &Arc<LocalStorage>,
    sync_client: &Arc<SyncClient>,
) {
    let source_app = read_cached_source_app(app);

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
                            &ClipboardFlavors::default(),
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
                        &ClipboardFlavors::default(),
                        Some(&bytes),
                        &content_hash,
                        source_app.as_deref(),
                    );
                    return;
                }

                let flavors = ClipboardFlavors {
                    file_list: Some(files),
                    ..ClipboardFlavors::default()
                };
                let content_hash = flavors.payload_hash(ContentType::File, None);
                store_entry(
                    app,
                    storage,
                    sync_client,
                    ContentType::File,
                    &flavors,
                    None,
                    &content_hash,
                    source_app.as_deref(),
                );
                return;
            }
        }
    }

    let flavors = read_text_flavors(clipboard);
    if !flavors.is_empty() {
        let content_type = if flavors.text_plain.is_some() {
            ContentType::Text
        } else if flavors.text_html.is_some() {
            ContentType::Html
        } else {
            ContentType::Rtf
        };

        let content_hash = flavors.payload_hash(content_type, None);
        store_entry(
            app,
            storage,
            sync_client,
            content_type,
            &flavors,
            None,
            &content_hash,
            source_app.as_deref(),
        );
    }
}

fn read_text_flavors(clipboard: &Clipboard) -> ClipboardFlavors {
    let text_plain = if clipboard.has_text().unwrap_or(false) {
        clipboard
            .read_text()
            .ok()
            .filter(|text| !text.trim().is_empty())
    } else {
        None
    };

    let text_html = if clipboard.has_html().unwrap_or(false) {
        clipboard
            .read_html()
            .ok()
            .filter(|html| !html.trim().is_empty())
    } else {
        None
    };

    let text_rtf = if clipboard.has_rtf().unwrap_or(false) {
        clipboard
            .read_rtf()
            .ok()
            .filter(|rtf| !rtf.trim().is_empty())
    } else {
        None
    };

    ClipboardFlavors {
        text_plain,
        text_html,
        text_rtf,
        file_list: None,
    }
}

#[cfg(desktop)]
fn read_cached_source_app(app: &tauri::AppHandle) -> Option<String> {
    let state = app.state::<crate::AppState>();
    state
        .last_focused_app
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
}

#[cfg(not(desktop))]
fn read_cached_source_app(_app: &tauri::AppHandle) -> Option<String> {
    None
}

/// Store a clipboard entry in local storage and trigger server sync.
#[allow(clippy::too_many_arguments)]
fn store_entry(
    app: &tauri::AppHandle,
    storage: &Arc<LocalStorage>,
    sync_client: &Arc<SyncClient>,
    content_type: ContentType,
    flavors: &ClipboardFlavors,
    blob_content: Option<&[u8]>,
    content_hash: &str,
    source_app: Option<&str>,
) {
    match storage.insert_entry(
        content_type,
        flavors,
        blob_content,
        content_hash,
        source_app,
    ) {
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
