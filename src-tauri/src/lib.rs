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
    /// When `true`, the next clipboard-monitor event should be ignored because
    /// it was triggered by our own clipboard write (paste preparation).
    #[cfg(desktop)]
    pub suppress_next_monitor_event: std::sync::atomic::AtomicBool,
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
                suppress_next_monitor_event: std::sync::atomic::AtomicBool::new(false),
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

    if !shortcut_toggle.is_empty() {
        let app_handle = app.clone();
        app.global_shortcut()
            .on_shortcut(shortcut_toggle.as_str(), move |_app, _shortcut, event| {
                if event.state == ShortcutState::Pressed {
                    let _ = toggle_popup(&app_handle, false);
                }
            })
            .unwrap_or_else(|e| {
                log::warn!("Failed to register {}: {}", shortcut_toggle, e);
            });
    }

    if !shortcut_starred.is_empty() {
        let app_handle = app.clone();
        app.global_shortcut()
            .on_shortcut(shortcut_starred.as_str(), move |_app, _shortcut, event| {
                if event.state == ShortcutState::Pressed {
                    let _ = toggle_popup(&app_handle, true);
                }
            })
            .unwrap_or_else(|e| {
                log::warn!("Failed to register {}: {}", shortcut_starred, e);
            });
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
    if let Some(popup) = app.get_webview_window("popup") {
        let is_visible = popup.is_visible().unwrap_or(false);
        if is_visible {
            let _ = popup.hide();
        } else {
            paste::remember_frontmost_app(app);
            position_popup_near_cursor(&popup);
            let _ = popup.show();
            let _ = popup.set_focus();
            // Emit event to frontend to update filter mode
            let _ = popup.emit("popup-show", starred_only);
        }
    }
    Ok(())
}

#[cfg(desktop)]
fn position_popup_near_cursor(popup: &tauri::WebviewWindow) {
    const CURSOR_OFFSET_PX: i32 = 14;
    const SCREEN_MARGIN_PX: i32 = 8;

    let cursor = match popup.cursor_position() {
        Ok(pos) => pos,
        Err(e) => {
            log::debug!("Could not read cursor position: {}", e);
            let _ = popup.center();
            return;
        }
    };

    let window_size = match popup.outer_size() {
        Ok(size) => size,
        Err(e) => {
            log::debug!("Could not read popup size for positioning: {}", e);
            let _ = popup.center();
            return;
        }
    };

    let cursor_x = cursor.x.round() as i32;
    let cursor_y = cursor.y.round() as i32;
    let window_w = window_size.width as i32;
    let window_h = window_size.height as i32;

    // Start below-right of the pointer.
    let mut x = cursor_x + CURSOR_OFFSET_PX;
    let mut y = cursor_y + CURSOR_OFFSET_PX;

    let monitor = popup
        .monitor_from_point(cursor.x, cursor.y)
        .ok()
        .flatten()
        .or_else(|| popup.current_monitor().ok().flatten())
        .or_else(|| popup.primary_monitor().ok().flatten());

    if let Some(monitor) = monitor {
        let monitor_pos = monitor.position();
        let monitor_size = monitor.size();

        let monitor_left = monitor_pos.x;
        let monitor_top = monitor_pos.y;
        let monitor_right = monitor_left + monitor_size.width as i32;
        let monitor_bottom = monitor_top + monitor_size.height as i32;

        // If below-right overflows, try above-left first.
        if x + window_w > monitor_right - SCREEN_MARGIN_PX {
            x = cursor_x - window_w - CURSOR_OFFSET_PX;
        }
        if y + window_h > monitor_bottom - SCREEN_MARGIN_PX {
            y = cursor_y - window_h - CURSOR_OFFSET_PX;
        }

        // Clamp to keep the full popup inside monitor bounds.
        let min_x = monitor_left + SCREEN_MARGIN_PX;
        let min_y = monitor_top + SCREEN_MARGIN_PX;
        let max_x = monitor_right - window_w - SCREEN_MARGIN_PX;
        let max_y = monitor_bottom - window_h - SCREEN_MARGIN_PX;

        x = if max_x >= min_x {
            x.clamp(min_x, max_x)
        } else {
            monitor_left
        };

        y = if max_y >= min_y {
            y.clamp(min_y, max_y)
        } else {
            monitor_top
        };
    }

    let _ = popup.set_position(tauri::PhysicalPosition::new(x, y));
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
