use tauri::Manager;

/// Paste the most recent clipboard entry as plaintext
pub fn paste_most_recent_plaintext(app: &tauri::AppHandle) {
    let state = app.state::<crate::AppState>();
    if let Ok(Some(entry)) = state.storage.get_most_recent_entry() {
        if let Some(text) = &entry.text_content {
            // Write plaintext to clipboard and simulate paste
            write_and_paste_text(app, text);
        }
    }
}

/// Write text to clipboard and simulate Cmd+V
pub fn write_and_paste_text(app: &tauri::AppHandle, text: &str) {
    // Use the clipboard plugin to write text
    let clipboard = app.state::<tauri_plugin_clipboard::Clipboard>();
    if let Err(e) = clipboard.write_text(text.to_string()) {
        log::error!("Failed to write to clipboard: {}", e);
        return;
    }

    // Hide the popup window
    if let Some(popup) = app.get_webview_window("popup") {
        let _ = popup.hide();
    }

    // Simulate Cmd+V keystroke
    // On macOS we use CGEvent APIs through a helper
    #[cfg(target_os = "macos")]
    simulate_paste();
}

/// Write image data to clipboard and simulate paste
pub fn write_and_paste_image(app: &tauri::AppHandle, image_data: &[u8]) {
    let clipboard = app.state::<tauri_plugin_clipboard::Clipboard>();
    let b64 = copywraith_core::content::bytes_to_base64(image_data);
    if let Err(e) = clipboard.write_image_base64(b64) {
        log::error!("Failed to write image to clipboard: {}", e);
        return;
    }

    if let Some(popup) = app.get_webview_window("popup") {
        let _ = popup.hide();
    }

    #[cfg(target_os = "macos")]
    simulate_paste();
}

#[cfg(target_os = "macos")]
fn simulate_paste() {
    use std::process::Command;
    // Use osascript to simulate Cmd+V
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let _ = Command::new("osascript")
            .arg("-e")
            .arg("tell application \"System Events\" to keystroke \"v\" using command down")
            .output();
    });
}

#[cfg(not(target_os = "macos"))]
fn simulate_paste() {
    log::warn!("Paste simulation not implemented for this platform");
}
