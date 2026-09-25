use std::path::Path;
use std::sync::Arc;

use copywraith_core::content::hash_bytes;
use copywraith_core::models::{ClipboardFlavors, ContentType};
use tauri::{Emitter, Manager};

use crate::native_clipboard::{ClipboardPayload, NativeClipboard};

use crate::storage::LocalStorage;
use crate::sync::SyncClient;

/// Subscribe before starting the native watcher; capture runs on its worker.
pub fn start_monitoring(
    app: tauri::AppHandle,
    storage: Arc<LocalStorage>,
    sync_client: Arc<SyncClient>,
) {
    let callback_app = app.clone();
    let clipboard = app.state::<NativeClipboard>();
    if let Err(error) = clipboard.start_monitor(
        move || {
            let state = callback_app.state::<crate::AppState>();
            if let Ok(guard) = state.suppress_monitor_until.lock() {
                if guard.is_some_and(|deadline| std::time::Instant::now() < deadline) {
                    return;
                }
            }
            // Paused: do not even read the clipboard.
            if state
                .capture_pause
                .lock()
                .is_ok_and(|pause| pause.is_paused_at(chrono::Utc::now()))
            {
                return;
            }
            let clipboard = callback_app.state::<NativeClipboard>();
            handle_clipboard_change(&callback_app, &clipboard, &storage, &sync_client);
        },
        |error| log::error!("Clipboard monitor failed: {error}"),
    ) {
        log::error!("Failed to start clipboard monitor: {error}");
        return;
    }
    log::info!("Clipboard monitor started");
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
    clipboard: &NativeClipboard,
    storage: &Arc<LocalStorage>,
    sync_client: &Arc<SyncClient>,
) {
    let source_app = read_cached_source_app(app);

    let payload = match clipboard.read() {
        Ok(payload) => payload,
        Err(error) => {
            log::error!("Failed to capture clipboard: {error}");
            return;
        }
    };
    let flavors = match payload {
        ClipboardPayload::Image(bytes) => {
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
        ClipboardPayload::Files(files) => {
            if let Some(bytes) = single_image_file(&files) {
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
                ..Default::default()
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
        ClipboardPayload::Flavors(flavors) => flavors,
        ClipboardPayload::Private => {
            // Never log the content or its source app: both can identify a secret.
            log::debug!("Skipping clipboard content marked private by its source");
            return;
        }
        ClipboardPayload::Empty => return,
    };
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

/// The image bytes when exactly one image file was copied.
///
/// Copying one screenshot in a file manager is stored as an image so it can be
/// previewed and pasted as a picture. With several files, converting would keep
/// one image and silently drop the rest of the selection, so the file list is
/// kept instead.
fn single_image_file(files: &[String]) -> Option<Vec<u8>> {
    const MAX_IMAGE_FILE_BYTES: u64 = 32 * 1024 * 1024;

    let [file_path] = files else {
        return None;
    };
    let path = Path::new(file_path);
    if !is_supported_image_path(path) {
        return None;
    }

    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_IMAGE_FILE_BYTES {
        return None;
    }

    let bytes = std::fs::read(path).ok()?;
    copywraith_core::content::detect_image_format(&bytes).map(|_| bytes)
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

#[cfg(test)]
mod tests {
    use super::single_image_file;

    const PNG: &[u8] = include_bytes!("../icons/32x32.png");

    fn file(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> String {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn one_copied_image_file_becomes_an_image() {
        let dir = tempfile::tempdir().unwrap();
        let image = file(&dir, "shot.png", PNG);

        assert_eq!(single_image_file(&[image]).as_deref(), Some(PNG));
    }

    #[test]
    fn several_copied_files_stay_a_file_list() {
        let dir = tempfile::tempdir().unwrap();
        let report = file(&dir, "report.pdf", b"%PDF-1.7");
        let image = file(&dir, "photo.png", PNG);

        assert_eq!(single_image_file(&[report.clone(), image.clone()]), None);
        assert_eq!(single_image_file(&[image.clone(), image]), None);
        assert_eq!(single_image_file(&[report]), None);
    }

    #[test]
    fn a_file_named_like_an_image_must_contain_one() {
        let dir = tempfile::tempdir().unwrap();
        let fake = file(&dir, "not-really.png", b"plain text");

        assert_eq!(single_image_file(&[fake]), None);
    }
}
