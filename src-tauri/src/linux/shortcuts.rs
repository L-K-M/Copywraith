//! Global shortcuts on Linux.
//!
//! `tauri-plugin-global-shortcut` only has an X11 backend on Linux, so the
//! in-process registration used on macOS and Windows works on an X11 session
//! and silently does nothing on a Wayland one (Wayland forbids clients from
//! grabbing keys globally). Worse, registration still *reports success* there,
//! so a shortcut typed into Settings looks bound but never fires.
//!
//! Ubuntu — like most current distributions — defaults to a Wayland session,
//! so this module routes the configured shortcuts to whatever the running
//! session can actually deliver:
//!
//! - **X11**: keep the in-process grabs (they work, and need no external
//!   configuration).
//! - **Wayland + GNOME** (the Ubuntu default): install GNOME custom
//!   keybindings via `gsettings` that run `copywraith --toggle` and friends.
//!   The single-instance guard in [`super`] forwards those to the running app.
//! - **KDE**: register native, user-assigned KGlobalAccel actions.
//! - **Anything else** (Sway, …): report the commands so the user can
//!   bind them in their own shortcut editor.

mod kde;

use std::collections::HashMap;
use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex, Once,
};
use std::time::Duration;

use crate::models::{Settings, ShortcutCommand, ShortcutStatus};

use super::which;

// ---------------------------------------------------------------------------
// Session detection
// ---------------------------------------------------------------------------

/// Display server backing the current session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionType {
    X11,
    Wayland,
    /// No session detected at all (headless, a container, a test runner).
    Unknown,
}

/// Detect the display server.
///
/// `XDG_SESSION_TYPE` wins because a GNOME Wayland session also exports
/// `DISPLAY` for XWayland, which would otherwise look like X11.
pub fn session_type() -> SessionType {
    match std::env::var("XDG_SESSION_TYPE").ok().as_deref() {
        Some("wayland") => return SessionType::Wayland,
        Some("x11") => return SessionType::X11,
        _ => {}
    }

    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        SessionType::Wayland
    } else if std::env::var_os("DISPLAY").is_some() {
        SessionType::X11
    } else {
        SessionType::Unknown
    }
}

/// Whether the session is GNOME (Ubuntu reports `ubuntu:GNOME`).
pub fn is_gnome_session() -> bool {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .or_else(|_| std::env::var("XDG_SESSION_DESKTOP"))
        .unwrap_or_default()
        .to_ascii_lowercase();
    desktop
        .split(':')
        .any(|part| part == "gnome" || part == "ubuntu")
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

/// The three shortcut-bindable actions, in the order Settings lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    TogglePopup,
    StarredPopup,
    PastePlaintext,
}

impl Action {
    pub const ALL: [Action; 3] = [
        Action::TogglePopup,
        Action::StarredPopup,
        Action::PastePlaintext,
    ];

    /// Human-readable label; also the identity of our own GNOME keybinding rows.
    fn label(self) -> &'static str {
        match self {
            Action::TogglePopup => "Toggle popup",
            Action::StarredPopup => "Starred popup",
            Action::PastePlaintext => "Paste as plain text",
        }
    }

    /// The flag [`crate::dispatch_cli_command`] understands.
    pub(crate) fn cli_flag(self) -> &'static str {
        match self {
            Action::TogglePopup => "--toggle",
            Action::StarredPopup => "--starred",
            Action::PastePlaintext => "--paste-plaintext",
        }
    }

    fn accelerator(self, settings: &Settings) -> &str {
        match self {
            Action::TogglePopup => settings.shortcut_toggle_popup.trim(),
            Action::StarredPopup => settings.shortcut_starred_popup.trim(),
            Action::PastePlaintext => settings.shortcut_paste_plaintext.trim(),
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// What [`sync`] decided for the current session.
pub struct SyncOutcome {
    pub status: ShortcutStatus,
    /// Whether the caller should still register the shortcuts in-process.
    pub use_in_process: bool,
}

/// Bind the configured shortcuts using whatever the current session supports.
pub fn sync(settings: &Settings, on_activate: impl Fn(Action) + Send + 'static) -> SyncOutcome {
    // KDE owns its assignments on both X11 and Wayland; never also grab keys.
    if is_kde_session() {
        start_kde(on_activate);
        return SyncOutcome {
            status: kde_status(manual_commands(settings)),
            use_in_process: false,
        };
    }

    let session = session_type();
    let gnome = is_gnome_session() && gsettings_usable();

    // On X11 the in-process grabs work. Drop any GNOME keybindings left over
    // from a previous Wayland session, or every shortcut would fire twice.
    if session != SessionType::Wayland {
        if gnome {
            if let Err(e) = remove_gnome_bindings() {
                log::debug!("Could not clean up GNOME keybindings: {e}");
            }
        }
        return SyncOutcome {
            status: ShortcutStatus {
                mechanism: "in_process".to_string(),
                message: "Shortcuts are grabbed directly by Copywraith (X11 session).".to_string(),
                commands: Vec::new(),
            },
            use_in_process: true,
        };
    }

    if gnome {
        match install_gnome_bindings(settings) {
            Ok(installed) => {
                let message = if installed.commands.is_empty() {
                    "No shortcuts configured. Wayland forbids in-app key grabs, so \
                     Copywraith registers the ones you set as GNOME custom keybindings."
                        .to_string()
                } else if installed.problems.is_empty() {
                    "Registered as GNOME custom keybindings (Settings → Keyboard → \
                     View and Customize Shortcuts → Custom Shortcuts)."
                        .to_string()
                } else {
                    format!(
                        "Registered as GNOME custom keybindings, except: {}.",
                        installed.problems.join("; ")
                    )
                };
                return SyncOutcome {
                    status: ShortcutStatus {
                        mechanism: "gnome".to_string(),
                        message,
                        commands: installed.commands,
                    },
                    use_in_process: false,
                };
            }
            Err(e) => {
                log::warn!("Could not install GNOME keybindings: {e}");
                return SyncOutcome {
                    status: ShortcutStatus {
                        mechanism: "manual".to_string(),
                        message: format!(
                            "Could not write GNOME keybindings ({e}). Bind these commands \
                             yourself in your keyboard settings."
                        ),
                        commands: manual_commands(settings),
                    },
                    use_in_process: false,
                };
            }
        }
    }

    SyncOutcome {
        status: ShortcutStatus {
            mechanism: "manual".to_string(),
            message: "Wayland forbids in-app key grabs. Bind these commands in your \
                      desktop's keyboard settings instead."
                .to_string(),
            commands: manual_commands(settings),
        },
        use_in_process: false,
    }
}

static KDE_STARTED: Once = Once::new();
static KDE_STOP: AtomicBool = AtomicBool::new(false);
static KDE_WORKER: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);
static KDE_HEALTH: Mutex<Option<Result<(), String>>> = Mutex::new(None);
const KDE_RETRY: Duration = Duration::from_secs(2);

fn is_kde_session() -> bool {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .or_else(|_| std::env::var("XDG_SESSION_DESKTOP"))
        .unwrap_or_default();
    desktop_is_kde(&desktop)
}

fn desktop_is_kde(desktop: &str) -> bool {
    desktop
        .split(':')
        .any(|part| part.eq_ignore_ascii_case("kde") || part.eq_ignore_ascii_case("plasma"))
}

fn start_kde(on_activate: impl Fn(Action) + Send + 'static) {
    KDE_STARTED.call_once(|| {
        let result = std::thread::Builder::new()
            .name("kde-shortcuts".into())
            .spawn(move || loop {
                if KDE_STOP.load(Ordering::Acquire) {
                    break;
                }
                // Reconnect after bus failure; each new connection restores presence
                // without changing assignments saved by System Settings.
                let result = kde::Session::connect().and_then(|mut session| loop {
                    if KDE_STOP.load(Ordering::Acquire) {
                        return session.deactivate();
                    }
                    let activations = session.poll()?;
                    *KDE_HEALTH.lock().unwrap() = Some(Ok(()));
                    for activation in activations {
                        on_activate(match activation {
                            kde::Activation::Toggle => Action::TogglePopup,
                            kde::Activation::Starred => Action::StarredPopup,
                            kde::Activation::Plaintext => Action::PastePlaintext,
                        });
                    }
                });
                *KDE_HEALTH.lock().unwrap() = Some(result);
                if KDE_STOP.load(Ordering::Acquire) {
                    break;
                }
                std::thread::sleep(KDE_RETRY);
            });
        match result {
            Ok(worker) => *KDE_WORKER.lock().unwrap() = Some(worker),
            Err(error) => *KDE_HEALTH.lock().unwrap() = Some(Err(error.to_string())),
        }
    });
}

/// Stop the worker before exit so KDE releases our keys but keeps assignments.
pub(crate) fn shutdown() {
    KDE_STOP.store(true, Ordering::Release);
    if let Some(worker) = KDE_WORKER.lock().unwrap().take() {
        if worker.join().is_err() {
            log::warn!("KDE shortcut worker failed during shutdown");
        }
    }
}

fn kde_status(commands: Vec<ShortcutCommand>) -> ShortcutStatus {
    status_for_kde_health(KDE_HEALTH.lock().unwrap().as_ref(), commands)
}

fn status_for_kde_health(
    health: Option<&Result<(), String>>,
    commands: Vec<ShortcutCommand>,
) -> ShortcutStatus {
    let (mechanism, message) = match health {
        Some(Ok(())) => ("kde", "Registered with KDE. Assign or disable keys in System Settings → Keyboard → Shortcuts → Copywraith. Existing KDE assignments are preserved; new actions start unbound.".to_string()),
        Some(Err(error)) => ("kde_unavailable", format!("KDE shortcuts unavailable ({error}). Retrying automatically. You can bind these commands in System Settings instead; avoid assigning the same key to both a command and a native action.")),
        None => ("kde_connecting", "Connecting to KDE shortcuts. Assign keys in System Settings → Keyboard → Shortcuts → Copywraith once connected.".to_string()),
    };
    ShortcutStatus {
        mechanism: mechanism.into(),
        message,
        commands: if mechanism == "kde_unavailable" {
            commands
        } else {
            Vec::new()
        },
    }
}

/// Refresh asynchronous backend health when Settings asks for status.
pub(crate) fn current_status(status: ShortcutStatus) -> ShortcutStatus {
    if !status.mechanism.starts_with("kde") {
        return status;
    }
    // Reconstruct command fallbacks even when the initial state was connecting.
    let mut commands = manual_commands(&Settings::default());
    for command in &mut commands {
        command.accelerator.clear();
    }
    kde_status(commands)
}

/// The commands to bind by hand, for sessions we cannot configure ourselves.
fn manual_commands(settings: &Settings) -> Vec<ShortcutCommand> {
    let launcher = launch_command().unwrap_or_else(|_| "copywraith".to_string());
    Action::ALL
        .iter()
        .map(|action| ShortcutCommand {
            label: action.label().to_string(),
            accelerator: action.accelerator(settings).to_string(),
            command: format!("{launcher} {}", action.cli_flag()),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// GNOME custom keybindings (gsettings)
// ---------------------------------------------------------------------------

const MEDIA_KEYS_SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys";
const CUSTOM_SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding";
const CUSTOM_PATH_PREFIX: &str =
    "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/";
/// Every row we own is named `Copywraith: <label>`; that is how we find our own
/// rows again on a re-sync instead of piling up duplicates.
const NAME_PREFIX: &str = "Copywraith: ";

/// Whether `gsettings` exists and the media-keys schema is installed.
fn gsettings_usable() -> bool {
    which("gsettings").is_some()
        && gsettings(&["get", MEDIA_KEYS_SCHEMA, "custom-keybindings"]).is_ok()
}

fn gsettings(args: &[&str]) -> Result<String, String> {
    let output = Command::new("gsettings")
        .args(args)
        .output()
        .map_err(|e| format!("failed to run gsettings: {e}"))?;

    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr = stderr.trim();
    Err(if stderr.is_empty() {
        format!("gsettings {} failed", args.join(" "))
    } else {
        stderr.to_string()
    })
}

struct Installed {
    commands: Vec<ShortcutCommand>,
    /// Accelerators we could not translate, described for the user.
    problems: Vec<String>,
}

/// Create/update/remove the GNOME custom keybindings for the current settings.
fn install_gnome_bindings(settings: &Settings) -> Result<Installed, String> {
    let launcher = launch_command()?;
    let mut paths = parse_string_array(&gsettings(&[
        "get",
        MEDIA_KEYS_SCHEMA,
        "custom-keybindings",
    ])?);

    // Our existing rows, keyed by the label embedded in their name.
    let mut owned: HashMap<String, String> = HashMap::new();
    for path in &paths {
        if let Some(name) = custom_get(path, "name") {
            if let Some(label) = name.strip_prefix(NAME_PREFIX) {
                owned.insert(label.to_string(), path.clone());
            }
        }
    }

    let mut commands = Vec::new();
    let mut problems = Vec::new();
    let mut list_changed = false;

    for action in Action::ALL {
        let label = action.label();
        let accelerator = action.accelerator(settings);

        // An empty (or untranslatable) accelerator means "no binding": drop the
        // row so a cleared field in Settings actually clears the shortcut.
        let binding = if accelerator.is_empty() {
            None
        } else {
            match to_gtk_accelerator(accelerator) {
                Ok(binding) => Some(binding),
                Err(e) => {
                    log::warn!("Cannot bind {label} ({accelerator}): {e}");
                    problems.push(format!("{label} ({accelerator}): {e}"));
                    None
                }
            }
        };

        let Some(binding) = binding else {
            if let Some(path) = owned.remove(label) {
                paths.retain(|p| p != &path);
                let _ = gsettings(&["reset-recursively", &format!("{CUSTOM_SCHEMA}:{path}")]);
                list_changed = true;
            }
            continue;
        };

        let path = match owned.remove(label) {
            Some(path) => path,
            None => {
                let path = next_free_path(&paths);
                paths.push(path.clone());
                list_changed = true;
                path
            }
        };

        let command = format!("{launcher} {}", action.cli_flag());
        custom_set(&path, "name", &format!("{NAME_PREFIX}{label}"))?;
        custom_set(&path, "command", &command)?;
        custom_set(&path, "binding", &binding)?;

        commands.push(ShortcutCommand {
            label: label.to_string(),
            accelerator: binding,
            command,
        });
    }

    // Rows named like ours but no longer matching an action (e.g. left by an
    // older version) are still ours to clean up.
    for path in owned.into_values() {
        paths.retain(|p| p != &path);
        let _ = gsettings(&["reset-recursively", &format!("{CUSTOM_SCHEMA}:{path}")]);
        list_changed = true;
    }

    if list_changed {
        gsettings(&[
            "set",
            MEDIA_KEYS_SCHEMA,
            "custom-keybindings",
            &format_string_array(&paths),
        ])?;
    }

    Ok(Installed { commands, problems })
}

/// Remove every GNOME custom keybinding Copywraith owns.
fn remove_gnome_bindings() -> Result<(), String> {
    let mut paths = parse_string_array(&gsettings(&[
        "get",
        MEDIA_KEYS_SCHEMA,
        "custom-keybindings",
    ])?);
    let mut removed = false;

    paths.retain(|path| {
        let ours = custom_get(path, "name")
            .map(|name| name.starts_with(NAME_PREFIX))
            .unwrap_or(false);
        if ours {
            let _ = gsettings(&["reset-recursively", &format!("{CUSTOM_SCHEMA}:{path}")]);
            removed = true;
        }
        !ours
    });

    if removed {
        gsettings(&[
            "set",
            MEDIA_KEYS_SCHEMA,
            "custom-keybindings",
            &format_string_array(&paths),
        ])?;
    }
    Ok(())
}

fn custom_get(path: &str, key: &str) -> Option<String> {
    gsettings(&["get", &format!("{CUSTOM_SCHEMA}:{path}"), key])
        .ok()
        .map(|value| unquote(&value))
}

fn custom_set(path: &str, key: &str, value: &str) -> Result<(), String> {
    gsettings(&[
        "set",
        &format!("{CUSTOM_SCHEMA}:{path}"),
        key,
        &gvariant_string(value),
    ])
    .map(|_| ())
}

/// The lowest `customN` slot not already taken (other apps share this list).
fn next_free_path(paths: &[String]) -> String {
    (0..)
        .map(|n| format!("{CUSTOM_PATH_PREFIX}custom{n}/"))
        .find(|candidate| !paths.iter().any(|p| p == candidate))
        .expect("an unused custom keybinding slot always exists")
}

/// The absolute command that starts (or wakes) Copywraith.
///
/// Inside an AppImage `current_exe` points into a temporary mount that
/// disappears on exit, so the AppImage path itself is the durable one.
fn launch_command() -> Result<String, String> {
    let exe = match std::env::var_os("APPIMAGE") {
        Some(appimage) if !appimage.is_empty() => std::path::PathBuf::from(appimage),
        _ => std::env::current_exe().map_err(|e| format!("cannot resolve our own path: {e}"))?,
    };
    Ok(shell_quote(&exe.to_string_lossy()))
}

// ---------------------------------------------------------------------------
// Quoting / GVariant helpers
// ---------------------------------------------------------------------------

/// Quote a path for `g_shell_parse_argv`, which is how GNOME runs the command.
fn shell_quote(value: &str) -> String {
    let safe = !value.is_empty()
        && value.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':' | '@' | '+')
        });
    if safe {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', r"'\''"))
    }
}

/// Render a Rust string as a GVariant string literal.
fn gvariant_string(value: &str) -> String {
    format!("'{}'", value.replace('\\', r"\\").replace('\'', r"\'"))
}

/// Parse `gsettings get` output for a string array (`['a', 'b']` or `@as []`).
fn parse_string_array(raw: &str) -> Vec<String> {
    let raw = raw.trim();
    let raw = raw.strip_prefix("@as").map(str::trim).unwrap_or(raw);
    let raw = raw
        .strip_prefix('[')
        .and_then(|r| r.strip_suffix(']'))
        .unwrap_or(raw);
    raw.split(',')
        .map(unquote)
        .filter(|item| !item.is_empty())
        .collect()
}

fn format_string_array(items: &[String]) -> String {
    let rendered: Vec<String> = items.iter().map(|item| gvariant_string(item)).collect();
    format!("[{}]", rendered.join(", "))
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    let stripped = value
        .strip_prefix('\'')
        .and_then(|v| v.strip_suffix('\''))
        .or_else(|| value.strip_prefix('"').and_then(|v| v.strip_suffix('"')))
        .unwrap_or(value);
    stripped.replace(r"\'", "'").replace(r"\\", "\\")
}

// ---------------------------------------------------------------------------
// Accelerator translation
// ---------------------------------------------------------------------------

/// Translate a Tauri accelerator (`CmdOrCtrl+Shift+V`) into the GTK form GNOME
/// stores (`<Control><Shift>v`).
pub fn to_gtk_accelerator(shortcut: &str) -> Result<String, String> {
    let parts: Vec<&str> = shortcut
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();

    let Some((key, modifiers)) = parts.split_last() else {
        return Err("empty shortcut".to_string());
    };

    let (mut ctrl, mut alt, mut shift, mut super_) = (false, false, false, false);
    for modifier in modifiers {
        match modifier.to_ascii_lowercase().as_str() {
            "cmdorctrl" | "commandorcontrol" | "ctrl" | "control" => ctrl = true,
            "alt" | "option" => alt = true,
            "shift" => shift = true,
            "super" | "meta" | "cmd" | "command" | "win" => super_ = true,
            other => return Err(format!("unsupported modifier `{other}`")),
        }
    }

    let key = gtk_key_name(key)?;

    let mut accelerator = String::new();
    if ctrl {
        accelerator.push_str("<Control>");
    }
    if alt {
        accelerator.push_str("<Alt>");
    }
    if shift {
        accelerator.push_str("<Shift>");
    }
    if super_ {
        accelerator.push_str("<Super>");
    }
    accelerator.push_str(&key);
    Ok(accelerator)
}

/// Map a Tauri key name to the GDK key name GNOME expects.
fn gtk_key_name(key: &str) -> Result<String, String> {
    let lower = key.to_ascii_lowercase();

    // `KeyV` / `Digit1` are the code-style names the plugin also accepts.
    let lower = ["key", "digit"]
        .iter()
        .find_map(|prefix| lower.strip_prefix(prefix))
        .filter(|rest| rest.chars().count() == 1)
        .map(str::to_string)
        .unwrap_or(lower);

    if let Some(c) = lower.chars().next().filter(|_| lower.chars().count() == 1) {
        if c.is_ascii_alphanumeric() {
            return Ok(c.to_string());
        }
    }

    if let Some(number) = lower.strip_prefix('f') {
        if let Ok(n) = number.parse::<u8>() {
            if (1..=24).contains(&n) {
                return Ok(format!("F{n}"));
            }
        }
    }

    let named = match lower.as_str() {
        "space" => "space",
        "enter" | "return" => "Return",
        "tab" => "Tab",
        "escape" | "esc" => "Escape",
        "backspace" => "BackSpace",
        "delete" | "del" => "Delete",
        "insert" => "Insert",
        "home" => "Home",
        "end" => "End",
        "pageup" => "Page_Up",
        "pagedown" => "Page_Down",
        "up" | "arrowup" => "Up",
        "down" | "arrowdown" => "Down",
        "left" | "arrowleft" => "Left",
        "right" | "arrowright" => "Right",
        "comma" => "comma",
        "period" | "dot" => "period",
        "slash" => "slash",
        "backslash" => "backslash",
        "minus" => "minus",
        "equal" => "equal",
        "plus" => "plus",
        "semicolon" => "semicolon",
        "quote" => "apostrophe",
        "backquote" | "grave" => "grave",
        "bracketleft" => "bracketleft",
        "bracketright" => "bracketright",
        "printscreen" => "Print",
        other => return Err(format!("unsupported key `{other}`")),
    };
    Ok(named.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kde_detection_leaves_ubuntu_and_other_desktops_alone() {
        for desktop in ["KDE", "plasma", "KDE:Plasma"] {
            assert!(desktop_is_kde(desktop));
        }
        for desktop in ["ubuntu:GNOME", "GNOME", "sway", "", "not-kde"] {
            assert!(!desktop_is_kde(desktop));
        }
    }

    #[test]
    fn kde_status_distinguishes_connecting_ready_and_failed() {
        let connecting = status_for_kde_health(None, Vec::new());
        assert_eq!(connecting.mechanism, "kde_connecting");
        let ready = status_for_kde_health(Some(&Ok(())), Vec::new());
        assert_eq!(ready.mechanism, "kde");
        assert!(ready.message.contains("System Settings"));
        let failure = status_for_kde_health(
            Some(&Err("bus disconnected".into())),
            manual_commands(&Settings::default()),
        );
        assert_eq!(failure.mechanism, "kde_unavailable");
        assert!(failure.message.contains("Retrying"));
        assert_eq!(failure.commands.len(), Action::ALL.len());
        assert!(failure.commands[0].command.ends_with(" --toggle"));
    }

    #[test]
    fn translates_the_default_shortcuts() {
        let defaults = Settings::default();
        assert_eq!(
            to_gtk_accelerator(&defaults.shortcut_toggle_popup).unwrap(),
            "<Control><Shift>v"
        );
        assert_eq!(
            to_gtk_accelerator(&defaults.shortcut_starred_popup).unwrap(),
            "<Control><Shift>b"
        );
        assert_eq!(
            to_gtk_accelerator(&defaults.shortcut_paste_plaintext).unwrap(),
            "<Control><Alt><Shift>v"
        );
    }

    #[test]
    fn translates_modifier_aliases_and_key_spellings() {
        assert_eq!(to_gtk_accelerator("Super+V").unwrap(), "<Super>v");
        assert_eq!(to_gtk_accelerator("Meta+KeyV").unwrap(), "<Super>v");
        assert_eq!(to_gtk_accelerator("control+Digit1").unwrap(), "<Control>1");
        assert_eq!(to_gtk_accelerator("Alt+Space").unwrap(), "<Alt>space");
        assert_eq!(to_gtk_accelerator("CmdOrCtrl+F12").unwrap(), "<Control>F12");
        assert_eq!(
            to_gtk_accelerator("Ctrl+ Shift + V").unwrap(),
            "<Control><Shift>v"
        );
        assert_eq!(to_gtk_accelerator("F5").unwrap(), "F5");
    }

    #[test]
    fn rejects_shortcuts_it_cannot_express() {
        assert!(to_gtk_accelerator("").is_err());
        assert!(to_gtk_accelerator("Hyper+V").is_err());
        assert!(to_gtk_accelerator("Ctrl+Numpad0").is_err());
        assert!(to_gtk_accelerator("Ctrl+F99").is_err());
    }

    #[test]
    fn parses_gsettings_string_arrays() {
        assert_eq!(parse_string_array("@as []"), Vec::<String>::new());
        assert_eq!(parse_string_array("[]"), Vec::<String>::new());
        assert_eq!(
            parse_string_array("['/a/custom0/', '/a/custom1/']"),
            vec!["/a/custom0/".to_string(), "/a/custom1/".to_string()]
        );
    }

    #[test]
    fn round_trips_string_arrays() {
        let items = vec!["/a/custom0/".to_string(), "/a/custom3/".to_string()];
        assert_eq!(
            format_string_array(&items),
            "['/a/custom0/', '/a/custom3/']"
        );
        assert_eq!(parse_string_array(&format_string_array(&items)), items);
    }

    #[test]
    fn allocates_the_lowest_unused_slot() {
        let taken = vec![
            format!("{CUSTOM_PATH_PREFIX}custom0/"),
            format!("{CUSTOM_PATH_PREFIX}custom2/"),
        ];
        assert_eq!(
            next_free_path(&taken),
            format!("{CUSTOM_PATH_PREFIX}custom1/")
        );
        assert_eq!(next_free_path(&[]), format!("{CUSTOM_PATH_PREFIX}custom0/"));
    }

    #[test]
    fn quotes_paths_with_spaces_for_the_shell() {
        assert_eq!(shell_quote("/usr/bin/copywraith"), "/usr/bin/copywraith");
        assert_eq!(
            shell_quote("/home/me/My Apps/Copywraith.AppImage"),
            "'/home/me/My Apps/Copywraith.AppImage'"
        );
    }

    /// Exercises the real `gsettings` round-trip.
    ///
    /// Opt-in, because it rewrites GNOME custom keybindings in whatever dconf
    /// database the process can reach — never run it against a desktop session
    /// you care about. Use a throwaway one:
    ///
    /// ```sh
    /// XDG_CONFIG_HOME=$(mktemp -d) COPYWRAITH_TEST_GSETTINGS=1 \
    ///   dbus-run-session -- cargo test -p copywraith-tauri gnome_keybinding
    /// ```
    #[test]
    fn gnome_keybinding_round_trip() {
        if std::env::var_os("COPYWRAITH_TEST_GSETTINGS").is_none() {
            eprintln!("skipped: set COPYWRAITH_TEST_GSETTINGS=1 to run this test");
            return;
        }
        assert!(
            gsettings_usable(),
            "gsettings and the GNOME schemas are required"
        );

        let mut settings = Settings {
            shortcut_starred_popup: String::new(),
            ..Default::default()
        };

        let installed = install_gnome_bindings(&settings).expect("install");
        assert!(installed.problems.is_empty(), "{:?}", installed.problems);
        assert_eq!(installed.commands.len(), 2);

        // The rows are discoverable through the same list GNOME reads.
        let paths = parse_string_array(
            &gsettings(&["get", MEDIA_KEYS_SCHEMA, "custom-keybindings"]).unwrap(),
        );
        let ours: Vec<&String> = paths
            .iter()
            .filter(|path| {
                custom_get(path, "name")
                    .map(|name| name.starts_with(NAME_PREFIX))
                    .unwrap_or(false)
            })
            .collect();
        assert_eq!(ours.len(), 2);

        let toggle = ours
            .iter()
            .find(|path| custom_get(path, "name").as_deref() == Some("Copywraith: Toggle popup"))
            .expect("toggle row");
        assert_eq!(
            custom_get(toggle, "binding").as_deref(),
            Some("<Control><Shift>v")
        );
        assert!(custom_get(toggle, "command")
            .expect("command")
            .ends_with(" --toggle"));

        // Re-syncing updates in place instead of piling up duplicates.
        settings.shortcut_toggle_popup = "Super+V".to_string();
        let reinstalled = install_gnome_bindings(&settings).expect("re-install");
        assert_eq!(reinstalled.commands.len(), 2);
        assert_eq!(custom_get(toggle, "binding").as_deref(), Some("<Super>v"));

        // And clearing every accelerator removes every row we own.
        remove_gnome_bindings().expect("remove");
        let remaining = parse_string_array(
            &gsettings(&["get", MEDIA_KEYS_SCHEMA, "custom-keybindings"]).unwrap(),
        );
        assert!(remaining.iter().all(|path| custom_get(path, "name")
            .map(|name| !name.starts_with(NAME_PREFIX))
            .unwrap_or(true)));
    }

    #[test]
    fn escapes_gvariant_strings() {
        assert_eq!(gvariant_string("plain"), "'plain'");
        assert_eq!(gvariant_string("it's"), r"'it\'s'");
        assert_eq!(unquote(&gvariant_string("it's")), "it's");
    }
}
