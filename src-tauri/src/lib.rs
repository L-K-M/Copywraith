#[cfg(desktop)]
mod clipboard;
mod commands;
#[cfg(target_os = "linux")]
mod linux;
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
    /// When set, all clipboard-monitor events whose arrival time is before this
    /// instant are suppressed.  This handles multiple rapid monitor events from a
    /// single paste write (e.g. duplicate system notifications) without requiring
    /// per-event counter management.
    #[cfg(desktop)]
    pub suppress_monitor_until: std::sync::Mutex<Option<std::time::Instant>>,
    #[cfg(target_os = "macos")]
    pub popup_panel_initialized: std::sync::atomic::AtomicBool,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Linux/KDE: if Copywraith is already running, forward this invocation's
    // arguments (e.g. a KDE global shortcut bound to `copywraith --toggle`) to
    // the running instance over a Unix socket and exit. Done before building the
    // app so the second process stays cheap and short-lived.
    #[cfg(target_os = "linux")]
    {
        if linux::forward_to_running_instance() {
            return;
        }
    }

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
        builder = builder
            .plugin(tauri_plugin_clipboard_manager::init())
            .plugin(copywraith_share_target::init());
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
                suppress_monitor_until: std::sync::Mutex::new(None),
                #[cfg(target_os = "macos")]
                popup_panel_initialized: std::sync::atomic::AtomicBool::new(false),
            };

            app.manage(state);

            // Desktop: start clipboard monitoring and register global shortcuts
            #[cfg(desktop)]
            {
                paste::start_frontmost_app_cache(&app_handle);

                let settings = storage.get_settings();
                register_shortcuts(&app_handle, &settings);

                // Start clipboard monitoring shortly after setup completes so
                // startup remains responsive even if monitor startup is slow.
                let monitor_app = app_handle.clone();
                let monitor_storage = storage.clone();
                let monitor_sync_client = sync_client.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    clipboard::start_monitoring(monitor_app, monitor_storage, monitor_sync_client);
                });
            }

            // Start periodic two-way sync loop (push unsynced + pull remote)
            start_sync_loop(app_handle.clone(), storage.clone(), sync_client.clone());

            // Linux/KDE: system tray, single-instance listener, and a command
            // passed on first launch (e.g. `copywraith --toggle` bound to a KDE
            // global shortcut).
            #[cfg(target_os = "linux")]
            {
                if let Err(e) = build_tray(&app_handle) {
                    log::warn!("Failed to build system tray: {}", e);
                }

                // Listen for forwarded commands from later invocations.
                let listener_app = app_handle.clone();
                linux::start_single_instance_listener(move |argv| {
                    let dispatch_app = listener_app.clone();
                    let _ = listener_app.run_on_main_thread(move || {
                        dispatch_cli_command(&dispatch_app, &argv);
                    });
                });

                let argv: Vec<String> = std::env::args().collect();
                dispatch_cli_command(&app_handle, &argv);
            }

            #[cfg(target_os = "android")]
            if storage.get_settings().shizuku_clipboard_enabled {
                use copywraith_share_target::ShareTargetExt;
                let settings = storage.get_settings();
                let config = copywraith_share_target::ShizukuClipboardConfig {
                    server_url_primary: settings.server_url_primary,
                    server_url_fallback: settings.server_url_fallback,
                    api_key: settings.api_key,
                };

                if let Err(e) = app_handle
                    .share_target()
                    .start_shizuku_clipboard_listener(config)
                {
                    log::debug!("Shizuku clipboard listener not started: {}", e);
                }
            }

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
            commands::has_pending_shares,
            commands::import_pending_shares,
            commands::sync_now,
            commands::reset_sync_cursor,
            commands::shizuku_clipboard_status,
            commands::set_shizuku_clipboard_enabled,
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
                    paste::remember_frontmost_app(&app_handle);
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
                        paste::remember_frontmost_app(&app_handle);
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
                        paste::remember_frontmost_app(&app_handle);
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
    // On macOS the global-shortcut callback may fire on an arbitrary thread,
    // but NSWindow operations (is_visible, set_position, unminimize, show,
    // set_focus, hide, panel.show / order_out) must run on the main thread.
    // Dispatch the entire toggle body there; on non-macOS platforms the
    // callback thread is safe for window operations.
    #[cfg(target_os = "macos")]
    {
        let app = app.clone();
        if let Some(popup) = app.get_webview_window("popup") {
            let _ = popup.run_on_main_thread(move || {
                let _ = toggle_popup_impl(&app, starred_only);
            });
        }
        return Ok(());
    }

    #[cfg(not(target_os = "macos"))]
    {
        toggle_popup_impl(app, starred_only)
    }
}

#[cfg(desktop)]
fn toggle_popup_impl(app: &tauri::AppHandle, starred_only: bool) -> Result<(), String> {
    // Guard against key auto-repeat causing immediate open->close toggles.
    {
        let state = app.state::<AppState>();
        let lock_result = state.last_popup_toggle_at.lock();
        if let Ok(mut last_toggle_at) = lock_result {
            let now = std::time::Instant::now();
            if let Some(last) = *last_toggle_at {
                if now.duration_since(last) < Duration::from_millis(100) {
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
            log::debug!("popup_open=true but window invisible; reconciling to closed");
            state
                .popup_open
                .store(false, std::sync::atomic::Ordering::SeqCst);
            false
        } else if !recorded_open && actually_visible {
            log::debug!("popup_open=false but window visible; reconciling to open");
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
                        < Duration::from_millis(200)
                    {
                        return Ok(());
                    }
                }
            }

            hide_popup_window(app);
            return Ok(());
        }

        log::debug!("Toggle requested open for popup");
        position_popup_near_cursor(&popup);
        #[cfg(target_os = "macos")]
        ensure_popup_panel_for_fullscreen_spaces(app, &popup);

        #[cfg(target_os = "macos")]
        show_popup_and_panel_on_main_thread(app, &popup);

        #[cfg(not(target_os = "macos"))]
        {
            let _ = popup.set_always_on_top(true);
            let _ = popup.unminimize();
            let _ = popup.show();
            let _ = popup.set_focus();
        }

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
    const CURSOR_OFFSET_LOGICAL_PX: f64 = 14.0;

    let cursor = match popup.cursor_position() {
        Ok(pos) => pos,
        Err(e) => {
            log::debug!("Could not read cursor position: {}", e);
            let _ = popup.center();
            return;
        }
    };

    let monitor = match resolve_monitor_for_cursor(popup, cursor) {
        Some(monitor) => monitor,
        None => {
            log::debug!("Could not resolve a monitor for cursor position; centering popup");
            let _ = popup.center();
            return;
        }
    };

    let popup_size = popup
        .outer_size()
        .or_else(|_| popup.inner_size())
        .unwrap_or_else(|_| tauri::PhysicalSize::new(560_u32, 480_u32));

    let cursor_logical = cursor.to_logical::<f64>(monitor.scale_factor());
    let candidate_logical = tauri::LogicalPosition::new(
        cursor_logical.x + CURSOR_OFFSET_LOGICAL_PX,
        cursor_logical.y + CURSOR_OFFSET_LOGICAL_PX,
    );
    let candidate_position = candidate_logical.to_physical::<i32>(monitor.scale_factor());

    let work_area = monitor.work_area();
    let final_x = clamp_popup_axis(
        candidate_position.x,
        popup_size.width,
        work_area.position.x,
        work_area.size.width,
    );
    let final_y = clamp_popup_axis(
        candidate_position.y,
        popup_size.height,
        work_area.position.y,
        work_area.size.height,
    );

    log::debug!(
        "Popup cursor=({}, {}), candidate_position=({}, {}), work_area=({}, {}, {}x{}), popup_size={}x{}, final_position=({}, {})",
        cursor.x.round() as i32,
        cursor.y.round() as i32,
        candidate_position.x,
        candidate_position.y,
        work_area.position.x,
        work_area.position.y,
        work_area.size.width,
        work_area.size.height,
        popup_size.width,
        popup_size.height,
        final_x,
        final_y
    );

    let _ = popup.set_position(tauri::PhysicalPosition::new(final_x, final_y));
}

#[cfg(desktop)]
fn resolve_monitor_for_cursor(
    popup: &tauri::WebviewWindow,
    cursor: tauri::PhysicalPosition<f64>,
) -> Option<tauri::window::Monitor> {
    popup
        .available_monitors()
        .ok()
        .and_then(|monitors| {
            monitors
                .into_iter()
                .find(|monitor| monitor_contains_cursor(monitor, cursor))
        })
        .or_else(|| popup.current_monitor().ok().flatten())
}

#[cfg(desktop)]
fn monitor_contains_cursor(
    monitor: &tauri::window::Monitor,
    cursor: tauri::PhysicalPosition<f64>,
) -> bool {
    let monitor_position = monitor.position();
    let monitor_size = monitor.size();

    let left = f64::from(monitor_position.x);
    let top = f64::from(monitor_position.y);
    let right = left + f64::from(monitor_size.width);
    let bottom = top + f64::from(monitor_size.height);

    cursor.x >= left && cursor.x < right && cursor.y >= top && cursor.y < bottom
}

#[cfg(desktop)]
fn clamp_popup_axis(candidate: i32, popup_extent: u32, work_start: i32, work_extent: u32) -> i32 {
    let candidate = i64::from(candidate);
    let popup_extent = i64::from(popup_extent);
    let work_start = i64::from(work_start);
    let work_extent = i64::from(work_extent);

    let max_origin = if popup_extent >= work_extent {
        work_start
    } else {
        work_start + work_extent - popup_extent
    };

    candidate.clamp(work_start, max_origin) as i32
}

#[cfg(desktop)]
pub(crate) fn hide_popup_window(app: &tauri::AppHandle) {
    hide_popup_window_impl(app, true);
}

#[cfg(desktop)]
pub(crate) fn hide_popup_window_for_paste(app: &tauri::AppHandle) {
    hide_popup_window_impl(app, false);
}

#[cfg(desktop)]
fn hide_popup_window_impl(app: &tauri::AppHandle, restore_focus: bool) {
    if let Some(popup) = app.get_webview_window("popup") {
        #[cfg(target_os = "macos")]
        hide_popup_and_panel_on_main_thread(app, &popup);

        #[cfg(not(target_os = "macos"))]
        let _ = popup.hide();

        let state = app.state::<AppState>();
        state
            .popup_open
            .store(false, std::sync::atomic::Ordering::SeqCst);

        if restore_focus {
            paste::restore_previous_focus(app);
        }
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn show_popup_and_panel_on_main_thread(
    app: &tauri::AppHandle,
    popup: &tauri::WebviewWindow,
) {
    use tauri_nspanel::ManagerExt as NSPanelManagerExt;

    let app_for_task = app.clone();
    if let Err(e) = popup.run_on_main_thread(move || {
        if let Some(p) = app_for_task.get_webview_window("popup") {
            let _ = p.unminimize();
            let _ = p.show();
        }
        if let Ok(panel) = app_for_task.get_webview_panel("popup") {
            log::debug!("Running panel.show on main thread (atomic with window.show)");
            panel.show();
        }
        if let Some(p) = app_for_task.get_webview_window("popup") {
            let _ = p.set_focus();
        }
    }) {
        log::debug!("Failed to schedule atomic popup show on main thread: {}", e);
    }
}

#[cfg(target_os = "macos")]
fn hide_popup_and_panel_on_main_thread(app: &tauri::AppHandle, popup: &tauri::WebviewWindow) {
    use tauri_nspanel::ManagerExt as NSPanelManagerExt;

    let app_for_task = app.clone();
    if let Err(e) = popup.run_on_main_thread(move || {
        if let Ok(panel) = app_for_task.get_webview_panel("popup") {
            log::debug!("Running panel.order_out on main thread (atomic with window.hide)");
            panel.order_out(None);
        }
        if let Some(p) = app_for_task.get_webview_window("popup") {
            let _ = p.hide();
        }
    }) {
        log::debug!("Failed to schedule atomic popup hide on main thread: {}", e);
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
                log::warn!(
                    "NSPanel fullscreen-space setup failed (will retry next open): {}",
                    e
                );
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
    use tauri_nspanel::objc::{msg_send, sel, sel_impl};
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
    const NSWindowStyleMaskResizable: i32 = 1 << 3;
    #[allow(non_upper_case_globals)]
    const NSWindowStyleMaskNonActivatingPanel: i32 = 1 << 7;

    let target_level = NSMainMenuWindowLevel + 1;
    log::info!(
        "NSPanel: setting window level to {} (NSMainMenuWindowLevel + 1)",
        target_level
    );
    panel.set_level(target_level);

    let current_style_mask_raw: usize = unsafe { msg_send![&*panel, styleMask] };
    let current_style_mask = current_style_mask_raw as i32;
    let target_style_mask =
        current_style_mask | NSWindowStyleMaskResizable | NSWindowStyleMaskNonActivatingPanel;
    log::info!(
        "NSPanel: setting style mask raw={:#x} -> {:#x} (non-activating + resizable)",
        current_style_mask,
        target_style_mask
    );
    panel.set_style_mask(target_style_mask);

    let target_behavior = NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenAuxiliary
        | NSWindowCollectionBehavior::NSWindowCollectionBehaviorCanJoinAllSpaces;
    log::info!("NSPanel: setting collection behavior = FullScreenAuxiliary | CanJoinAllSpaces");
    panel.set_collection_behaviour(target_behavior);

    let actual_behavior: NSWindowCollectionBehavior =
        unsafe { msg_send![&*panel, collectionBehavior] };
    let has_fullscreen_auxiliary = actual_behavior
        .contains(NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenAuxiliary);
    let has_can_join_all_spaces = actual_behavior
        .contains(NSWindowCollectionBehavior::NSWindowCollectionBehaviorCanJoinAllSpaces);
    if has_fullscreen_auxiliary && has_can_join_all_spaces {
        log::info!(
            "NSPanel: collection behavior verified (raw={:#x})",
            actual_behavior.bits()
        );
    } else {
        log::warn!(
            "NSPanel: collection behavior verification failed (raw={:#x}, has_fullscreen_auxiliary={}, has_can_join_all_spaces={})",
            actual_behavior.bits(),
            has_fullscreen_auxiliary,
            has_can_join_all_spaces
        );
    }

    panel.set_hides_on_deactivate(false);
    log::info!("NSPanel: set hides_on_deactivate = false");

    log::info!(
        "NSPanel: popup window fully configured for macOS fullscreen Spaces (level={})",
        target_level
    );
    Ok(())
}

/// Dispatch a command-line action to the running instance.
///
/// Used both at first launch and (via the single-instance plugin) when a second
/// process is started — typically by a KDE global shortcut bound to
/// `copywraith --toggle` / `--starred` / `--paste-plaintext`.
#[cfg(target_os = "linux")]
fn dispatch_cli_command(app: &tauri::AppHandle, argv: &[String]) {
    for arg in argv.iter().skip(1) {
        match arg.as_str() {
            "--toggle" | "toggle" => {
                let _ = toggle_popup(app, false);
            }
            "--starred" | "starred" => {
                let _ = toggle_popup(app, true);
            }
            "--paste-plaintext" | "paste-plaintext" => {
                paste::paste_most_recent_plaintext(app);
            }
            _ => {}
        }
    }
}

/// Build the KDE/Plasma system-tray icon and menu.
#[cfg(target_os = "linux")]
fn build_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri::menu::{CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder};
    use tauri::tray::TrayIconBuilder;

    let show = MenuItemBuilder::with_id("tray_show", "Show Copywraith").build(app)?;
    let starred = MenuItemBuilder::with_id("tray_starred", "Show Starred").build(app)?;
    let paste_plain =
        MenuItemBuilder::with_id("tray_paste_plain", "Paste last entry as plain text")
            .build(app)?;
    let autostart = CheckMenuItemBuilder::with_id("tray_autostart", "Start at login")
        .checked(linux::autostart_is_enabled())
        .build(app)?;
    let quit = MenuItemBuilder::with_id("tray_quit", "Quit Copywraith").build(app)?;

    let menu = MenuBuilder::new(app)
        .item(&show)
        .item(&starred)
        .item(&paste_plain)
        .separator()
        .item(&autostart)
        .separator()
        .item(&quit)
        .build()?;

    let autostart_item = autostart.clone();
    let mut tray = TrayIconBuilder::with_id("copywraith-tray")
        .tooltip("Copywraith")
        .menu(&menu)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "tray_show" => {
                let _ = toggle_popup(app, false);
            }
            "tray_starred" => {
                let _ = toggle_popup(app, true);
            }
            "tray_paste_plain" => {
                paste::paste_most_recent_plaintext(app);
            }
            "tray_autostart" => {
                let enable = !linux::autostart_is_enabled();
                match linux::autostart_set_enabled(enable) {
                    Ok(()) => {
                        let _ = autostart_item.set_checked(enable);
                    }
                    Err(e) => {
                        log::warn!("Failed to update autostart: {}", e);
                        let _ = autostart_item.set_checked(linux::autostart_is_enabled());
                    }
                }
            }
            "tray_quit" => app.exit(0),
            _ => {}
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        tray = tray.icon(icon);
    }

    tray.build(app)?;
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
                            message: Some(e.to_string()),
                            checked_at: Some(chrono::Utc::now().to_rfc3339()),
                        },
                    );
                    // Exponential backoff on failure, capped at MAX_INTERVAL_SECS
                    current_interval = (current_interval * 2).min(MAX_INTERVAL_SECS);
                }
            }
        }
    });
}
