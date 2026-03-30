#[cfg(desktop)]
use clipboard_rs::{Clipboard as ClipboardRs, ClipboardContent};
use copywraith_core::models::ClipboardFlavors;
use tauri::Manager;

#[cfg(target_os = "macos")]
use tauri::Emitter;

#[cfg(target_os = "macos")]
pub fn remember_frontmost_app(app: &tauri::AppHandle) {
    if let Some(name) = detect_frontmost_app_name() {
        let state = app.state::<crate::AppState>();
        let lock_result = state.last_focused_app.lock();
        if let Ok(mut slot) = lock_result {
            *slot = Some(name);
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn remember_frontmost_app(_app: &tauri::AppHandle) {}

#[cfg(target_os = "macos")]
pub fn start_frontmost_app_cache(app: &tauri::AppHandle) {
    let app_handle = app.clone();
    std::thread::Builder::new()
        .name("frontmost-app-cache".into())
        .spawn(move || {
            loop {
                if let Some(name) = detect_frontmost_app_name() {
                    let state = app_handle.state::<crate::AppState>();
                    let lock_result = state.last_focused_app.lock();
                    if let Ok(mut slot) = lock_result {
                        *slot = Some(name);
                    }
                }
                // detect_frontmost_app_name returns None when Copywraith itself
                // is frontmost (filtered by sanitize_target_app_name), so the
                // cache naturally retains the last real app during popup display.
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        })
        .expect("failed to spawn frontmost-app-cache thread");
}

#[cfg(not(target_os = "macos"))]
pub fn start_frontmost_app_cache(_app: &tauri::AppHandle) {}

#[cfg(desktop)]
pub fn restore_previous_focus(app: &tauri::AppHandle) {
    let target_app = {
        let state = app.state::<crate::AppState>();
        let lock_result = state.last_focused_app.lock();
        match lock_result {
            Ok(slot) => slot.clone(),
            Err(_) => None,
        }
    };

    #[cfg(target_os = "macos")]
    if let Some(name) = target_app {
        std::thread::spawn(move || {
            let escaped = name.replace('\\', "\\\\").replace('"', "\\\"");
            let _ = std::process::Command::new("osascript")
                .arg("-e")
                .arg("try")
                .arg("-e")
                .arg(format!("tell application \"{}\" to activate", escaped))
                .arg("-e")
                .arg("end try")
                .output();
        });
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = target_app;
    }
}

/// Paste the most recent clipboard entry as plaintext
pub fn paste_most_recent_plaintext(app: &tauri::AppHandle) {
    let state = app.state::<crate::AppState>();
    if let Ok(Some(entry)) = state.storage.get_most_recent_entry() {
        if let Some(text) = entry.best_plain_text() {
            // Write plaintext to clipboard and simulate paste
            write_and_paste_text(app, &text);
        }
    }
}

/// Write multiple text flavors (plain/html/rtf) in one transaction and paste.
pub fn write_and_paste_flavors(app: &tauri::AppHandle, flavors: &ClipboardFlavors) {
    let target_app = preferred_paste_target(app);

    let state = app.state::<crate::AppState>();
    if let Ok(mut guard) = state.suppress_monitor_until.lock() {
        *guard = Some(std::time::Instant::now() + std::time::Duration::from_millis(500));
    }

    let mut contents: Vec<ClipboardContent> = Vec::new();

    let plain = flavors
        .text_plain
        .clone()
        .or_else(|| {
            flavors
                .text_html
                .as_ref()
                .map(|html| copywraith_core::content::strip_html(html))
        })
        .or_else(|| {
            flavors
                .text_rtf
                .as_ref()
                .map(|rtf| copywraith_core::content::strip_rtf(rtf))
        })
        .filter(|text| !text.trim().is_empty());

    if let Some(text) = plain {
        contents.push(ClipboardContent::Text(text));
    }

    if let Some(html) = flavors
        .text_html
        .as_ref()
        .filter(|html| !html.trim().is_empty())
    {
        contents.push(ClipboardContent::Html(html.clone()));
    }

    if let Some(rtf) = flavors
        .text_rtf
        .as_ref()
        .filter(|rtf| !rtf.trim().is_empty())
    {
        contents.push(ClipboardContent::Rtf(rtf.clone()));
    }

    if contents.is_empty() {
        if let Ok(mut guard) = state.suppress_monitor_until.lock() {
            *guard = None;
        }
        return;
    }

    let clipboard = app.state::<tauri_plugin_clipboard::Clipboard>();
    let write_result = clipboard
        .clipboard
        .lock()
        .map_err(|err| err.to_string())
        .and_then(|ctx| ctx.set(contents).map_err(|err| err.to_string()));

    if let Err(e) = write_result {
        if let Ok(mut guard) = state.suppress_monitor_until.lock() {
            *guard = None;
        }
        log::error!("Failed to write flavors to clipboard: {}", e);
        return;
    }

    crate::hide_popup_window_for_paste(app);
    simulate_paste(app.clone(), target_app);
}

/// Write file-list clipboard content and simulate paste.
pub fn write_and_paste_files(app: &tauri::AppHandle, files: &[String]) {
    if files.is_empty() {
        return;
    }

    let target_app = preferred_paste_target(app);

    let state = app.state::<crate::AppState>();
    if let Ok(mut guard) = state.suppress_monitor_until.lock() {
        *guard = Some(std::time::Instant::now() + std::time::Duration::from_millis(500));
    }

    let clipboard = app.state::<tauri_plugin_clipboard::Clipboard>();
    let files_for_clipboard = files
        .iter()
        .map(|path| normalize_file_uri_for_write(path))
        .collect::<Vec<_>>();

    if let Err(e) = clipboard.write_files_uris(files_for_clipboard) {
        if let Ok(mut guard) = state.suppress_monitor_until.lock() {
            *guard = None;
        }
        log::error!("Failed to write files to clipboard: {}", e);
        return;
    }

    crate::hide_popup_window_for_paste(app);
    simulate_paste(app.clone(), target_app);
}

fn normalize_file_uri_for_write(path: &str) -> String {
    #[cfg(target_os = "windows")]
    {
        return path.trim_start_matches("file://").to_string();
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        if path.starts_with("file://") {
            return path.to_string();
        }
        return format!("file://{}", path);
    }

    #[allow(unreachable_code)]
    path.to_string()
}

/// Write text to clipboard and simulate Cmd+V / Ctrl+V
pub fn write_and_paste_text(app: &tauri::AppHandle, text: &str) {
    let target_app = preferred_paste_target(app);

    let state = app.state::<crate::AppState>();

    // Suppress the clipboard monitor for a 500ms window starting NOW.
    // The window approach handles multiple rapid monitor events from a single
    // paste write (e.g. duplicate system notifications) — every event that
    // arrives before the deadline is suppressed.
    if let Ok(mut guard) = state.suppress_monitor_until.lock() {
        *guard = Some(std::time::Instant::now() + std::time::Duration::from_millis(500));
    }

    let clipboard = app.state::<tauri_plugin_clipboard::Clipboard>();
    if let Err(e) = clipboard.write_text(text.to_string()) {
        if let Ok(mut guard) = state.suppress_monitor_until.lock() {
            *guard = None;
        }
        log::error!("Failed to write to clipboard: {}", e);
        return;
    }

    crate::hide_popup_window_for_paste(app);

    simulate_paste(app.clone(), target_app);
}

/// Write image data to clipboard and simulate paste
pub fn write_and_paste_image(app: &tauri::AppHandle, image_data: &[u8]) {
    let target_app = preferred_paste_target(app);

    let state = app.state::<crate::AppState>();

    if let Ok(mut guard) = state.suppress_monitor_until.lock() {
        *guard = Some(std::time::Instant::now() + std::time::Duration::from_millis(500));
    }

    let clipboard = app.state::<tauri_plugin_clipboard::Clipboard>();
    let b64 = copywraith_core::content::bytes_to_base64(image_data);
    if let Err(e) = clipboard.write_image_base64(b64) {
        if let Ok(mut guard) = state.suppress_monitor_until.lock() {
            *guard = None;
        }
        log::error!("Failed to write image to clipboard: {}", e);
        return;
    }

    crate::hide_popup_window_for_paste(app);

    simulate_paste(app.clone(), target_app);
}

/// Simulate a paste keystroke.
///
/// On macOS we use AppleScript/System Events because it is more reliable than
/// synthetic key events for the popup flow.
/// Other platforms currently only write to clipboard and log a warning.
fn simulate_paste(app: tauri::AppHandle, target_app: Option<String>) {
    #[cfg(target_os = "macos")]
    std::thread::spawn(move || {
        let accessibility_trusted = is_accessibility_trusted();

        // Warn early if Accessibility permission is missing.  We do NOT bail
        // out — the osascript must still run so the `activate` line restores
        // focus to the target app.  Only the `keystroke` line requires
        // Accessibility; it will fail and the stderr-capture below will
        // surface the error to the user.
        if !accessibility_trusted {
            log::warn!(
                "Accessibility permission not granted — the paste keystroke \
                 will likely fail.  Grant access in System Settings > Privacy \
                 & Security > Accessibility."
            );
        }

        // Give macOS time to complete the popup hide and begin refocusing the
        // previous app.  100 ms is sufficient for the window-server
        // round-trip on modern hardware.
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Primary path: keystroke "v" with command modifier.
        let primary_result = run_macos_paste_script(target_app.as_deref(), false);

        match primary_result {
            Ok(output) if output.status.success() => {
                log::debug!("Paste simulation succeeded");
            }
            Ok(output) => {
                let stderr = stderr_text(&output);
                log::error!(
                    "osascript paste simulation failed (status {}): {}",
                    output.status,
                    stderr
                );

                // Secondary fallback: key-code based paste can succeed in apps
                // where literal keystroke simulation is flaky.
                if accessibility_trusted {
                    match run_macos_paste_script(target_app.as_deref(), true) {
                        Ok(fallback_output) if fallback_output.status.success() => {
                            log::warn!(
                                "Primary paste simulation failed, but key-code fallback succeeded"
                            );
                            return;
                        }
                        Ok(fallback_output) => {
                            let fallback_stderr = stderr_text(&fallback_output);
                            log::error!(
                                "osascript key-code fallback failed (status {}): {}",
                                fallback_output.status,
                                fallback_stderr
                            );
                        }
                        Err(e) => {
                            log::error!("Failed to run osascript fallback: {}", e);
                        }
                    }
                }

                let msg = classify_macos_paste_error(&stderr);
                emit_paste_failed(&app, &msg);
            }
            Err(e) => {
                log::error!("Failed to run osascript: {}", e);
                emit_paste_failed(&app, &format!("Failed to run paste simulation: {}", e));
            }
        }
    });

    #[cfg(not(target_os = "macos"))]
    {
        let _ = target_app;
        let _ = app;
        log::warn!("Simulated paste is not implemented on this platform");
    }
}

#[cfg(target_os = "macos")]
fn run_macos_paste_script(
    target_app: Option<&str>,
    use_key_code: bool,
) -> std::io::Result<std::process::Output> {
    let mut command = std::process::Command::new("osascript");

    if let Some(target_app_name) = target_app {
        let escaped_name = target_app_name.replace('\\', "\\\\").replace('"', "\\\"");

        // Keep activation in a `try` block so name-resolution failures do not
        // abort the script before we even attempt Cmd+V.
        command
            .arg("-e")
            .arg("try")
            .arg("-e")
            .arg(format!("tell application \"{}\" to activate", escaped_name))
            .arg("-e")
            .arg("end try")
            .arg("-e")
            .arg("delay 0.14");
    }

    let paste_line = if use_key_code {
        "tell application \"System Events\" to key code 9 using {command down}"
    } else {
        "tell application \"System Events\" to keystroke \"v\" using {command down}"
    };

    command.arg("-e").arg(paste_line).output()
}

#[cfg(target_os = "macos")]
fn stderr_text(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        "<no stderr>".to_string()
    } else {
        stderr
    }
}

#[cfg(target_os = "macos")]
fn classify_macos_paste_error(stderr: &str) -> String {
    let lower = stderr.to_ascii_lowercase();

    if lower.contains("assistive")
        || lower.contains("-1719")
        || lower.contains("1002")
        || lower.contains("not allowed")
    {
        return "Accessibility permission required. Open System Settings -> Privacy & Security -> Accessibility and enable Copywraith.".to_string();
    }

    if lower.contains("can't get application") || lower.contains("can't get process") {
        return "Could not target the previously focused app for paste. Refocus the destination field and try again.".to_string();
    }

    if stderr == "<no stderr>" {
        return "Paste simulation failed for an unknown reason.".to_string();
    }

    format!("Paste simulation failed: {}", stderr)
}

#[cfg(target_os = "macos")]
fn emit_paste_failed(app: &tauri::AppHandle, message: &str) {
    if let Some(popup) = app.get_webview_window("popup") {
        crate::show_popup_and_panel_on_main_thread(app, &popup);
    }

    let _ = app.emit("paste-failed", message.to_string());
}

/// Check whether this process has Accessibility (assistive access) permission.
///
/// Uses the `AXIsProcessTrusted()` function from the ApplicationServices
/// framework.  Returns `true` when the app is listed and enabled in
/// System Settings > Privacy & Security > Accessibility.
#[cfg(target_os = "macos")]
fn is_accessibility_trusted() -> bool {
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> u8;
    }
    unsafe { AXIsProcessTrusted() != 0 }
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

    // Keep this path fast: avoid synchronous osascript fallback while handling
    // a user paste action.
    remembered.or_else(detect_frontmost_app_name_native)
}

#[cfg(not(target_os = "macos"))]
fn preferred_paste_target(_app: &tauri::AppHandle) -> Option<String> {
    None
}

#[cfg(target_os = "macos")]
fn detect_frontmost_app_name() -> Option<String> {
    detect_frontmost_app_name_native().or_else(detect_frontmost_app_name_via_osascript)
}

#[cfg(target_os = "macos")]
fn detect_frontmost_app_name_native() -> Option<String> {
    use tauri_nspanel::objc::rc::autoreleasepool;
    use tauri_nspanel::objc::runtime::Object;
    use tauri_nspanel::objc::{class, msg_send, sel, sel_impl};
    use tauri_nspanel::objc_foundation::{INSString, NSString};

    autoreleasepool(|| unsafe {
        let workspace: *mut Object = msg_send![class!(NSWorkspace), sharedWorkspace];
        if workspace.is_null() {
            return None;
        }

        let frontmost_application: *mut Object = msg_send![workspace, frontmostApplication];
        if frontmost_application.is_null() {
            return None;
        }

        let localized_name: *mut Object = msg_send![frontmost_application, localizedName];
        if localized_name.is_null() {
            return None;
        }

        let app_name = (&*(localized_name as *mut NSString))
            .as_str()
            .trim()
            .to_string();
        sanitize_target_app_name(app_name)
    })
}

#[cfg(target_os = "macos")]
fn detect_frontmost_app_name_via_osascript() -> Option<String> {
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
