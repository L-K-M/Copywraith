mod clipboard;
mod commands;
mod models;
mod paste;
mod storage;
mod sync;

use std::sync::Arc;
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

pub struct AppState {
    pub storage: Arc<storage::LocalStorage>,
    pub sync_client: Arc<sync::SyncClient>,
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_clipboard::init())
        .setup(|app| {
            let app_handle = app.handle().clone();
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("failed to get app data dir");
            std::fs::create_dir_all(&data_dir).expect("failed to create data dir");

            let storage =
                Arc::new(storage::LocalStorage::new(&data_dir).expect("failed to init storage"));

            let sync_client = Arc::new(sync::SyncClient::new());

            let state = AppState {
                storage: storage.clone(),
                sync_client: sync_client.clone(),
            };

            app.manage(state);

            // Start clipboard monitoring
            clipboard::start_monitoring(app_handle.clone(), storage.clone(), sync_client.clone());

            // Register global shortcuts
            register_shortcuts(&app_handle);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_entries,
            commands::toggle_star,
            commands::delete_entry,
            commands::paste_entry,
            commands::paste_entry_plaintext,
            commands::get_settings,
            commands::update_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running copywraith");
}

fn register_shortcuts(app: &tauri::AppHandle) {
    use tauri_plugin_global_shortcut::ShortcutState;

    let app_handle = app.clone();
    app.global_shortcut().on_shortcut("CmdOrCtrl+Shift+V", move |_app, _shortcut, event| {
        if event.state == ShortcutState::Pressed {
            let _ = toggle_popup(&app_handle, false);
        }
    }).unwrap_or_else(|e| {
        log::warn!("Failed to register CmdOrCtrl+Shift+V: {}", e);
    });

    let app_handle = app.clone();
    app.global_shortcut().on_shortcut("CmdOrCtrl+Shift+B", move |_app, _shortcut, event| {
        if event.state == ShortcutState::Pressed {
            let _ = toggle_popup(&app_handle, true);
        }
    }).unwrap_or_else(|e| {
        log::warn!("Failed to register CmdOrCtrl+Shift+B: {}", e);
    });

    let app_handle = app.clone();
    app.global_shortcut().on_shortcut("CmdOrCtrl+Shift+Alt+V", move |_app, _shortcut, event| {
        if event.state == ShortcutState::Pressed {
            paste::paste_most_recent_plaintext(&app_handle);
        }
    }).unwrap_or_else(|e| {
        log::warn!("Failed to register CmdOrCtrl+Shift+Alt+V: {}", e);
    });
}

fn toggle_popup(app: &tauri::AppHandle, starred_only: bool) -> Result<(), String> {
    if let Some(popup) = app.get_webview_window("popup") {
        let is_visible = popup.is_visible().unwrap_or(false);
        if is_visible {
            let _ = popup.hide();
        } else {
            let _ = popup.show();
            let _ = popup.set_focus();
            // Emit event to frontend to update filter mode
            let _ = popup.emit("popup-show", starred_only);
        }
    }
    Ok(())
}
