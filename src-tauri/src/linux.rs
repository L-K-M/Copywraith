//! Linux / KDE Plasma integration.
//!
//! Targets Plasma 6 (Wayland by default), where synthetic key injection is
//! restricted. Paste works through `ydotool` (a uinput-based key injector) when
//! it is installed and its daemon is running; otherwise the entry is left on the
//! clipboard and the user is notified to press Ctrl+V themselves — the same
//! graceful degradation the Android app uses.
//!
//! This module is only compiled on Linux and is kept entirely separate from the
//! macOS paste path.

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::Command;
use std::time::Duration;

/// Linux input event keycodes (see `linux/input-event-codes.h`).
const KEY_LEFTCTRL: &str = "29";
const KEY_V: &str = "47";

/// Simulate Ctrl+V into whatever window regains focus after the popup hides.
///
/// On Wayland we cannot target a specific window, so we rely on the compositor
/// refocusing the previously-active window once our popup is hidden, then inject
/// the keystroke globally. `target_app` is unused on Linux (kept for a uniform
/// signature with the macOS path).
pub fn simulate_paste(app: tauri::AppHandle, target_app: Option<String>) {
    let _ = target_app;

    std::thread::spawn(move || {
        // Give the compositor time to hide our popup and refocus the previous
        // window before the keystroke is delivered.
        std::thread::sleep(Duration::from_millis(140));

        match run_ydotool_paste() {
            Ok(()) => log::debug!("Paste simulation via ydotool succeeded"),
            Err(e) => {
                log::warn!("Automatic paste unavailable ({e}); leaving content on the clipboard");
                notify_manual_paste(&app);
            }
        }
    });
}

/// Run `ydotool` to press and release Ctrl+V.
fn run_ydotool_paste() -> Result<(), String> {
    if which("ydotool").is_none() {
        return Err("ydotool is not installed".to_string());
    }

    // key syntax: <keycode>:<1=press|0=release>
    let output = Command::new("ydotool")
        .args([
            "key",
            &format!("{KEY_LEFTCTRL}:1"),
            &format!("{KEY_V}:1"),
            &format!("{KEY_V}:0"),
            &format!("{KEY_LEFTCTRL}:0"),
        ])
        .output()
        .map_err(|e| format!("failed to run ydotool: {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        // The most common cause is a missing/unreachable ydotoold daemon.
        Err(if stderr.is_empty() {
            "ydotool failed (is ydotoold running?)".to_string()
        } else {
            format!("ydotool failed: {stderr}")
        })
    }
}

/// Tell the user the content is on the clipboard and they should paste manually.
fn notify_manual_paste(app: &tauri::AppHandle) {
    // Best-effort desktop notification via libnotify.
    if which("notify-send").is_some() {
        let _ = Command::new("notify-send")
            .args([
                "--app-name=Copywraith",
                "--icon=copywraith",
                "Copied to clipboard",
                "Press Ctrl+V to paste. Install ydotool for automatic paste.",
            ])
            .spawn();
    }

    // Also surface it in-app if the popup happens to be visible.
    use tauri::Emitter;
    let _ = app.emit(
        "paste-manual",
        "Copied to clipboard. Press Ctrl+V to paste.".to_string(),
    );
}

/// Resolve an executable on `PATH`, returning its full path.
fn which(program: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}

// ---------------------------------------------------------------------------
// Single instance (Unix socket)
// ---------------------------------------------------------------------------
//
// A dependency-free single-instance guard. The first process owns a Unix socket
// and listens for forwarded command-line arguments; later invocations connect,
// send their argv, and exit. This is how KDE global shortcuts drive the running
// app: bind a shortcut to `copywraith --toggle` and the second launch forwards
// `--toggle` to the instance already in the tray.

fn single_instance_socket_path() -> std::path::PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(std::env::temp_dir);
    // Scope by uid so multiple users on one machine don't collide in /tmp.
    let uid = current_uid();
    dir.join(format!("copywraith-{uid}.sock"))
}

// Avoid pulling in the `libc` crate just for getuid(); link it directly.
fn current_uid() -> u32 {
    extern "C" {
        fn getuid() -> u32;
    }
    unsafe { getuid() }
}

/// If another instance is already running, forward our argv to it and return
/// `true` (the caller should exit). Returns `false` if we are the first instance.
pub fn forward_to_running_instance() -> bool {
    let path = single_instance_socket_path();

    match UnixStream::connect(&path) {
        Ok(mut stream) => {
            // Send the full argv (including argv[0]); the receiver's dispatcher
            // skips the program name, matching the first-launch path.
            let argv: Vec<String> = std::env::args().collect();
            let payload = argv.join("\n");
            let _ = stream.write_all(payload.as_bytes());
            let _ = stream.flush();
            true
        }
        Err(_) => {
            // No listener (socket absent or stale). Remove any stale file so the
            // listener can bind cleanly later.
            let _ = std::fs::remove_file(&path);
            false
        }
    }
}

/// Start listening for forwarded commands from later invocations.
///
/// `on_command` receives the forwarded argv (without the program name) and is
/// invoked on the listener thread; callers should marshal back to the main
/// thread for any UI work.
pub fn start_single_instance_listener<F>(on_command: F)
where
    F: Fn(Vec<String>) + Send + 'static,
{
    let path = single_instance_socket_path();
    let _ = std::fs::remove_file(&path);

    let listener = match UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(e) => {
            log::warn!("Single-instance listener disabled (bind failed): {e}");
            return;
        }
    };

    std::thread::Builder::new()
        .name("single-instance".into())
        .spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(mut stream) => {
                        let mut buf = String::new();
                        if stream.read_to_string(&mut buf).is_ok() {
                            let argv: Vec<String> = buf
                                .split('\n')
                                .filter(|s| !s.is_empty())
                                .map(|s| s.to_string())
                                .collect();
                            if !argv.is_empty() {
                                on_command(argv);
                            }
                        }
                    }
                    Err(e) => log::debug!("Single-instance accept error: {e}"),
                }
            }
        })
        .expect("failed to spawn single-instance listener thread");
}

// ---------------------------------------------------------------------------
// Autostart (XDG ~/.config/autostart/copywraith.desktop)
// ---------------------------------------------------------------------------

const AUTOSTART_FILE: &str = "copywraith.desktop";

fn autostart_dir() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::Path::new(&home).join(".config"))
        })?;
    Some(base.join("autostart"))
}

fn autostart_path() -> Option<std::path::PathBuf> {
    autostart_dir().map(|dir| dir.join(AUTOSTART_FILE))
}

/// Whether Copywraith is configured to start on login.
pub fn autostart_is_enabled() -> bool {
    autostart_path().map(|p| p.exists()).unwrap_or(false)
}

/// Enable or disable starting Copywraith on login.
pub fn autostart_set_enabled(enabled: bool) -> anyhow::Result<()> {
    let path = autostart_path()
        .ok_or_else(|| anyhow::anyhow!("could not resolve the XDG autostart directory"))?;

    if !enabled {
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        return Ok(());
    }

    let dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid autostart path"))?;
    std::fs::create_dir_all(dir)?;

    let exe = std::env::current_exe()?;
    let exe = exe.to_string_lossy();
    let desktop = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Copywraith\n\
         Comment=Local-first clipboard manager\n\
         Exec={exe}\n\
         Icon=copywraith\n\
         Terminal=false\n\
         Categories=Utility;\n\
         X-GNOME-Autostart-enabled=true\n"
    );
    std::fs::write(&path, desktop)?;
    Ok(())
}
