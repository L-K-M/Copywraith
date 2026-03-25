use tauri::Manager;

#[cfg(target_os = "macos")]
pub fn remember_frontmost_app(app: &tauri::AppHandle) {
    let target_app = detect_frontmost_app_name();
    let state = app.state::<crate::AppState>();
    if let Ok(mut slot) = state.last_focused_app.lock() {
        *slot = target_app;
    };
}

#[cfg(not(target_os = "macos"))]
pub fn remember_frontmost_app(_app: &tauri::AppHandle) {}

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
    let target_app = preferred_paste_target(app);

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
    simulate_paste(target_app);
}

/// Write image data to clipboard and simulate paste
pub fn write_and_paste_image(app: &tauri::AppHandle, image_data: &[u8]) {
    let target_app = preferred_paste_target(app);

    let clipboard = app.state::<tauri_plugin_clipboard::Clipboard>();
    let b64 = copywraith_core::content::bytes_to_base64(image_data);
    if let Err(e) = clipboard.write_image_base64(b64) {
        log::error!("Failed to write image to clipboard: {}", e);
        return;
    }

    if let Some(popup) = app.get_webview_window("popup") {
        let _ = popup.hide();
    }

    simulate_paste(target_app);
}

/// Simulate a paste keystroke.
///
/// On macOS we use AppleScript/System Events because it is more reliable than
/// synthetic key events from a background thread in the popup flow.
/// Other platforms currently only write to clipboard and log a warning.
fn simulate_paste(target_app: Option<String>) {
    #[cfg(target_os = "macos")]
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(100));

        let mut command = std::process::Command::new("osascript");

        if let Some(target_app_name) = target_app {
            let escaped_name = target_app_name.replace('\\', "\\\\").replace('"', "\\\"");

            command
                .arg("-e")
                .arg(format!("tell application \"{}\" to activate", escaped_name))
                .arg("-e")
                .arg("delay 0.08");
        }

        let status = command
            .arg("-e")
            .arg("tell application \"System Events\" to keystroke \"v\" using command down")
            .status();

        match status {
            Ok(exit_status) if exit_status.success() => {}
            Ok(exit_status) => {
                log::error!(
                    "osascript paste simulation exited with status: {}",
                    exit_status
                );
            }
            Err(e) => {
                log::error!("Failed to run osascript paste simulation: {}", e);
            }
        }
    });

    #[cfg(not(target_os = "macos"))]
    {
        let _ = target_app;
        log::warn!("Simulated paste is not implemented on this platform");
    }
}

#[cfg(target_os = "macos")]
fn preferred_paste_target(app: &tauri::AppHandle) -> Option<String> {
    let remembered = {
        let state = app.state::<crate::AppState>();
        let lock_result = state.last_focused_app.lock();
        let remembered_value = match lock_result {
            Ok(slot) => slot.clone(),
            Err(_) => None,
        };
        remembered_value
    };

    remembered.or_else(detect_frontmost_app_name)
}

#[cfg(not(target_os = "macos"))]
fn preferred_paste_target(_app: &tauri::AppHandle) -> Option<String> {
    None
}

#[cfg(target_os = "macos")]
fn detect_frontmost_app_name() -> Option<String> {
    let output = std::process::Command::new("osascript")
        .arg("-e")
        .arg("tell application \"System Events\" to get name of first process whose frontmost is true")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let app_name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    sanitize_target_app_name(app_name)
}

#[cfg(target_os = "macos")]
fn sanitize_target_app_name(app_name: String) -> Option<String> {
    let trimmed = app_name.trim();
    if trimmed.is_empty() {
        return None;
    }

    if trimmed.to_ascii_lowercase().contains("copywraith") {
        return None;
    }

    Some(trimmed.to_string())
}
