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

/// Write text to clipboard and simulate Cmd+V / Ctrl+V
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

    // Simulate Cmd+V / Ctrl+V keystroke
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

    simulate_paste();
}

/// Simulate a paste keystroke (Cmd+V on macOS, Ctrl+V on Windows/Linux).
///
/// Uses the `enigo` crate for cross-platform input simulation. A short delay
/// is inserted to allow the window manager to process the hide before the
/// keystroke is sent.
fn simulate_paste() {
    use enigo::{Enigo, Keyboard, Settings};

    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(100));

        let mut enigo = match Enigo::new(&Settings::default()) {
            Ok(e) => e,
            Err(e) => {
                log::error!("Failed to initialise input simulator: {}", e);
                return;
            }
        };

        #[cfg(target_os = "macos")]
        let modifier = enigo::Key::Meta;
        #[cfg(not(target_os = "macos"))]
        let modifier = enigo::Key::Control;

        if let Err(e) = enigo.key(modifier, enigo::Direction::Press) {
            log::error!("Failed to press modifier key: {}", e);
            return;
        }
        if let Err(e) = enigo.key(enigo::Key::Unicode('v'), enigo::Direction::Click) {
            log::error!("Failed to send 'v' key: {}", e);
        }
        if let Err(e) = enigo.key(modifier, enigo::Direction::Release) {
            log::error!("Failed to release modifier key: {}", e);
        }
    });
}
