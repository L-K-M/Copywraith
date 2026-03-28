#[cfg(desktop)]
mod clipboard;
mod commands;
mod models;
#[cfg(desktop)]
mod paste;
mod storage;
mod sync;

use std::sync::Arc;
use std::time::Duration;
use tauri::{Emitter, Manager};

pub struct AppState {
    pub storage: Arc<storage::LocalStorage>,
    pub sync_client: Arc<sync::SyncClient>,
    #[cfg(desktop)]
    pub last_focused_app: std::sync::Mutex<Option<String>>,
    #[cfg(desktop)]
    pub popup_open: std::sync::atomic::AtomicBool,
    #[cfg(desktop)]
    pub last_popup_toggle_at: std::sync::Mutex<Option<std::time::Instant>>,
    #[cfg(desktop)]
    pub last_popup_opened_at: std::sync::Mutex<Option<std::time::Instant>>,
    /// When `true`, the next clipboard-monitor event should be ignored because
    /// it was triggered by our own clipboard write (paste preparation).
    #[cfg(desktop)]
    pub suppress_next_monitor_event: std::sync::atomic::AtomicBool,
    #[cfg(target_os = "macos")]
    pub popup_panel_initialized: std::sync::atomic::AtomicBool,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();

    // Desktop-only plugins
    #[cfg(desktop)]
    {
        builder = builder
            .plugin(tauri_plugin_global_shortcut::Builder::new().build())
            .plugin(tauri_plugin_clipboard::init());

        #[cfg(target_os = "macos")]
        {
            builder = builder.plugin(tauri_nspanel::init());
        }
    }

    // Android: official clipboard-manager plugin for read/write
    #[cfg(target_os = "android")]
    {
        builder = builder.plugin(tauri_plugin_clipboard_manager::init());
    }

    builder
        .setup(|app| {
            let app_handle = app.handle().clone();
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("failed to get app data dir");
            std::fs::create_dir_all(&data_dir).expect("failed to create data dir");

            let storage =
                Arc::new(storage::LocalStorage::new(&data_dir).expect("failed to init storage"));

            let sync_client = Arc::new(sync::SyncClient::new(&storage));

            let state = AppState {
                storage: storage.clone(),
                sync_client: sync_client.clone(),
                #[cfg(desktop)]
                last_focused_app: std::sync::Mutex::new(None),
                #[cfg(desktop)]
                popup_open: std::sync::atomic::AtomicBool::new(false),
                #[cfg(desktop)]
                last_popup_toggle_at: std::sync::Mutex::new(None),
                #[cfg(desktop)]
                last_popup_opened_at: std::sync::Mutex::new(None),
                #[cfg(desktop)]
                suppress_next_monitor_event: std::sync::atomic::AtomicBool::new(false),
                #[cfg(target_os = "macos")]
                popup_panel_initialized: std::sync::atomic::AtomicBool::new(false),
            };

            app.manage(state);

            // Desktop: start clipboard monitoring and register global shortcuts
            #[cfg(desktop)]
            {
                clipboard::start_monitoring(
                    app_handle.clone(),
                    storage.clone(),
                    sync_client.clone(),
                );

                let settings = storage.get_settings();
                register_shortcuts(&app_handle, &settings);
            }

            // Start periodic two-way sync loop (push unsynced + pull remote)
            start_sync_loop(app_handle.clone(), storage.clone(), sync_client.clone());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_entries,
            commands::get_entry_image,
            commands::toggle_star,
            commands::delete_entry,
            commands::paste_entry,
            commands::paste_entry_plaintext,
            commands::get_settings,
            commands::update_settings,
            commands::reregister_shortcuts,
            commands::capture_clipboard,
            commands::get_platform,
            commands::hide_popup,
        ])
        .run(tauri::generate_context!())
        .expect("error while running copywraith");
}

#[cfg(desktop)]
pub fn register_shortcuts(app: &tauri::AppHandle, settings: &models::Settings) {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

    // Unregister all existing shortcuts first
    let _ = app.global_shortcut().unregister_all();

    let shortcut_toggle = &settings.shortcut_toggle_popup;
    let shortcut_starred = &settings.shortcut_starred_popup;
    let shortcut_plaintext = &settings.shortcut_paste_plaintext;
    let toggle_and_starred_conflict = !shortcut_toggle.is_empty()
        && !shortcut_starred.is_empty()
        && shortcut_toggle.eq_ignore_ascii_case(shortcut_starred);

    if !shortcut_toggle.is_empty() {
        let app_handle = app.clone();
        app.global_shortcut()
            .on_shortcut(shortcut_toggle.as_str(), move |_app, _shortcut, event| {
                // Use Released to avoid key-repeat firing open->close while
                // modifiers are still held.
                if event.state == ShortcutState::Released {
                    let _ = toggle_popup(&app_handle, false);
                }
            })
            .unwrap_or_else(|e| {
                log::warn!("Failed to register {}: {}", shortcut_toggle, e);
            });
    }

    if !shortcut_starred.is_empty() {
        if toggle_and_starred_conflict {
            log::warn!(
                "Skipping starred popup shortcut because it matches toggle shortcut ({})",
                shortcut_starred
            );
        } else {
            let app_handle = app.clone();
            app.global_shortcut()
                .on_shortcut(shortcut_starred.as_str(), move |_app, _shortcut, event| {
                    if event.state == ShortcutState::Released {
                        let _ = toggle_popup(&app_handle, true);
                    }
                })
                .unwrap_or_else(|e| {
                    log::warn!("Failed to register {}: {}", shortcut_starred, e);
                });
        }
    }

    if !shortcut_plaintext.is_empty() {
        let app_handle = app.clone();
        app.global_shortcut()
            .on_shortcut(
                shortcut_plaintext.as_str(),
                move |_app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        paste::paste_most_recent_plaintext(&app_handle);
                    }
                },
            )
            .unwrap_or_else(|e| {
                log::warn!("Failed to register {}: {}", shortcut_plaintext, e);
            });
    }
}

#[cfg(desktop)]
fn toggle_popup(app: &tauri::AppHandle, starred_only: bool) -> Result<(), String> {
    // Guard against key auto-repeat causing immediate open->close toggles.
    {
        let state = app.state::<AppState>();
        let lock_result = state.last_popup_toggle_at.lock();
        if let Ok(mut last_toggle_at) = lock_result {
            let now = std::time::Instant::now();
            if let Some(last) = *last_toggle_at {
                if now.duration_since(last) < Duration::from_millis(180) {
                    return Ok(());
                }
            }
            *last_toggle_at = Some(now);
        }
    }

    if let Some(popup) = app.get_webview_window("popup") {
        let state = app.state::<AppState>();

        let actually_visible = popup.is_visible().unwrap_or(false);
        let recorded_open = state.popup_open.load(std::sync::atomic::Ordering::SeqCst);

        let popup_open = if recorded_open && !actually_visible {
            log::debug!(
                "popup_open=true but window invisible; reconciling to closed"
            );
            state
                .popup_open
                .store(false, std::sync::atomic::Ordering::SeqCst);
            false
        } else if !recorded_open && actually_visible {
            log::debug!(
                "popup_open=false but window visible; reconciling to open"
            );
            state
                .popup_open
                .store(true, std::sync::atomic::Ordering::SeqCst);
            true
        } else {
            recorded_open
        };

        if popup_open {
            log::debug!("Toggle requested close for popup");
            let state = app.state::<AppState>();
            let lock_result = state.last_popup_opened_at.lock();
            if let Ok(last_opened_at) = lock_result {
                if let Some(last_opened) = *last_opened_at {
                    if std::time::Instant::now().duration_since(last_opened)
                        < Duration::from_millis(350)
                    {
                        // Likely key-repeat from same physical shortcut press.
                        return Ok(());
                    }
                }
            }

            hide_popup_window(app);
            return Ok(());
        }

        log::debug!("Toggle requested open for popup");
        paste::remember_frontmost_app(app);
        position_popup_near_cursor(&popup);
        let _ = popup.unminimize();

        #[cfg(target_os = "macos")]
        ensure_popup_panel_for_fullscreen_spaces(app, &popup);

        let _ = popup.show();

        #[cfg(target_os = "macos")]
        request_panel_show_on_main_thread(app, &popup);

        let _ = popup.set_focus();

        {
            let state = app.state::<AppState>();
            let lock_result = state.last_popup_opened_at.lock();
            if let Ok(mut last_opened_at) = lock_result {
                *last_opened_at = Some(std::time::Instant::now());
            }
            state
                .popup_open
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }

        // Emit event to frontend to update filter mode
        let _ = popup.emit("popup-show", starred_only);
    }
    Ok(())
}

#[cfg(desktop)]
fn position_popup_near_cursor(popup: &tauri::WebviewWindow) {
    const CURSOR_OFFSET_PX: f64 = 14.0;

    let cursor = match popup.cursor_position() {
        Ok(pos) => pos,
        Err(e) => {
            log::debug!("Could not read cursor position: {}", e);
            let _ = popup.center();
            return;
        }
    };

    let final_x = (cursor.x + CURSOR_OFFSET_PX).round() as i32;
    let final_y = (cursor.y + CURSOR_OFFSET_PX).round() as i32;

    log::debug!(
        "Popup cursor=({}, {}), final_position=({}, {})",
        cursor.x.round() as i32,
        cursor.y.round() as i32,
        final_x,
        final_y
    );

    // Use physical coordinates because cursor_position() is physical.
    let _ = popup.set_position(tauri::PhysicalPosition::new(final_x, final_y));
}

#[cfg(desktop)]
pub(crate) fn hide_popup_window(app: &tauri::AppHandle) {
    if let Some(popup) = app.get_webview_window("popup") {
        #[cfg(target_os = "macos")]
        request_panel_hide_on_main_thread(app, &popup);

        let _ = popup.hide();

        let state = app.state::<AppState>();
        state
            .popup_open
            .store(false, std::sync::atomic::Ordering::SeqCst);

        paste::restore_previous_focus(app);
    }
}

#[cfg(target_os = "macos")]
fn request_panel_show_on_main_thread(app: &tauri::AppHandle, popup: &tauri::WebviewWindow) {
    use tauri_nspanel::ManagerExt as NSPanelManagerExt;

    let app_for_task = app.clone();
    if let Err(e) = popup.run_on_main_thread(move || {
        if let Ok(panel) = app_for_task.get_webview_panel("popup") {
            log::debug!("Running panel.show on main thread");
            panel.show();
        }
    }) {
        log::debug!("Failed to schedule panel.show on main thread: {}", e);
    }
}

#[cfg(target_os = "macos")]
fn request_panel_hide_on_main_thread(app: &tauri::AppHandle, popup: &tauri::WebviewWindow) {
    use tauri_nspanel::ManagerExt as NSPanelManagerExt;

    let app_for_task = app.clone();
    if let Err(e) = popup.run_on_main_thread(move || {
        if let Ok(panel) = app_for_task.get_webview_panel("popup") {
            log::debug!("Running panel.order_out on main thread");
            panel.order_out(None);
        }
    }) {
        log::debug!("Failed to schedule panel.order_out on main thread: {}", e);
    }
}

#[cfg(target_os = "macos")]
fn ensure_popup_panel_for_fullscreen_spaces(app: &tauri::AppHandle, popup: &tauri::WebviewWindow) {
    let state = app.state::<AppState>();
    if state
        .popup_panel_initialized
        .swap(true, std::sync::atomic::Ordering::SeqCst)
    {
        return;
    }
    let app_for_task = app.clone();

    let run_result = popup.run_on_main_thread(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            configure_popup_panel_for_fullscreen_spaces_now(&app_for_task)
        }));

        match result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                log::warn!("NSPanel fullscreen-space setup failed (will retry next open): {}", e);
                app_for_task
                    .state::<AppState>()
                    .popup_panel_initialized
                    .store(false, std::sync::atomic::Ordering::SeqCst);
            }
            Err(panic_payload) => {
                log::warn!(
                    "NSPanel setup panicked; disabling panel mode for this run: {}",
                    panic_payload_to_string(&panic_payload)
                );
            }
        }
    });

    if let Err(e) = run_result {
        state
            .popup_panel_initialized
            .store(false, std::sync::atomic::Ordering::SeqCst);
        log::warn!("Failed to run NSPanel setup on main thread: {}", e);
    }
}

#[cfg(target_os = "macos")]
fn panic_payload_to_string(panic_payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = panic_payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = panic_payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

#[cfg(target_os = "macos")]
#[allow(deprecated)]
fn configure_popup_panel_for_fullscreen_spaces_now(app: &tauri::AppHandle) -> Result<(), String> {
    use tauri_nspanel::cocoa::appkit::{NSMainMenuWindowLevel, NSWindowCollectionBehavior};
    use tauri_nspanel::{ManagerExt as NSPanelManagerExt, WebviewWindowExt};

    let popup = app
        .get_webview_window("popup")
        .ok_or_else(|| "Popup window not found".to_string())?;

    popup
        .ns_window()
        .map_err(|e| format!("Popup NSWindow handle unavailable: {}", e))?;

    let panel = match app.get_webview_panel("popup") {
        Ok(existing) => {
            log::info!("NSPanel: reusing existing panel for popup window");
            existing
        }
        Err(_) => {
            log::info!(
                "NSPanel: converting popup NSWindow to NSPanel for fullscreen-space support"
            );
            popup
                .to_panel()
                .map_err(|e| format!("to_panel conversion failed: {}", e))?
        }
    };

    #[allow(non_upper_case_globals)]
    const NSWindowStyleMaskNonActivatingPanel: i32 = 1 << 7;

    let target_level = NSMainMenuWindowLevel + 1;
    log::info!(
        "NSPanel: setting window level to {} (NSMainMenuWindowLevel + 1)",
        target_level
    );
    panel.set_level(target_level);

    log::info!("NSPanel: setting non-activating style mask");
    panel.set_style_mask(NSWindowStyleMaskNonActivatingPanel);

    let target_behavior =
        NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenAuxiliary
            | NSWindowCollectionBehavior::NSWindowCollectionBehaviorCanJoinAllSpaces;
    log::info!(
        "NSPanel: setting collection behavior = FullScreenAuxiliary | CanJoinAllSpaces"
    );
    panel.set_collection_behaviour(target_behavior);

    log::info!("NSPanel: verifying collection behavior was applied");
    let actual = panel.collection_behaviour();
    let expected_fullscreen_aux =
        (actual as usize)
            & (NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenAuxiliary as usize)
            != 0;
    let expected_all_spaces =
        (actual as usize)
            & (NSWindowCollectionBehavior::NSWindowCollectionBehaviorCanJoinAllSpaces as usize)
            != 0;
    if expected_fullscreen_aux && expected_all_spaces {
        log::info!(
            "NSPanel: collection behavior verified — FullScreenAuxiliary and CanJoinAllSpaces both present"
        );
    } else {
        log::warn!(
            "NSPanel: collection behavior verification: FullScreenAuxiliary={}, CanJoinAllSpaces={} (raw=0x{:X})",
            expected_fullscreen_aux,
            expected_all_spaces,
            actual as usize
        );
    }

    panel.set_hides_on_deactivate(false);
    log::info!("NSPanel: set hides_on_deactivate = false");

    log::info!(
        "NSPanel: popup window fully configured for macOS fullscreen Spaces (level={}, collection_behavior verified)",
        target_level
    );
    Ok(())
}

fn start_sync_loop(
    app: tauri::AppHandle,
    storage: Arc<storage::LocalStorage>,
    sync_client: Arc<sync::SyncClient>,
) {
    tauri::async_runtime::spawn(async move {
        const BASE_INTERVAL_SECS: u64 = 5;
        const MAX_INTERVAL_SECS: u64 = 120;

        let mut current_interval = BASE_INTERVAL_SECS;

        loop {
            tokio::time::sleep(Duration::from_secs(current_interval)).await;

            // Push local unsynced entries first
            sync_client.sync_unsynced_entries(&storage).await;

            // Then pull entries created on other devices
            match sync_client.pull_new_entries(&storage).await {
                Ok(result) => {
                    let _ = app.emit("sync-endpoint-status", &result.endpoint_status);

                    if result.pulled > 0 {
                        let _ = app.emit("clipboard-updated", ());
                        log::info!("Applied {} updates from server", result.pulled);
                    }

                    if result.endpoint_status.state == "unreachable" {
                        current_interval = (current_interval * 2).min(MAX_INTERVAL_SECS);
                    } else {
                        current_interval = BASE_INTERVAL_SECS;
                    }
                }
                Err(e) => {
                    log::debug!("Pull sync failed: {}", e);
                    let _ = app.emit(
                        "sync-endpoint-status",
                        sync::SyncEndpointStatus {
                            state: "unreachable".to_string(),
                            role: None,
                            url: None,
                        },
                    );
                    // Exponential backoff on failure, capped at MAX_INTERVAL_SECS
                    current_interval = (current_interval * 2).min(MAX_INTERVAL_SECS);
                }
            }
        }
    });
}
