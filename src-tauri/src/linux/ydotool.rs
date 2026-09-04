//! Keep ydotool's command-line protocol out of the desktop paste flow.

use std::path::Path;
use std::process::{Command, Output};

/// Linux input event keycodes (`linux/input-event-codes.h`).
const KEY_LEFTCTRL: &str = "29";
const KEY_V: &str = "47";
const LEGACY_PASTE_KEYS: &str = "ctrl+v";
const LEGACY_HELP_EXIT_CODE: i32 = 1;
const SOCKET_OVERRIDE_ENV: &str = "YDOTOOL_SOCKET";

#[derive(Clone, Copy)]
enum KeySyntax {
    Symbolic,
    Evdev,
}

pub(super) fn paste() -> Result<(), String> {
    paste_with(Path::new("ydotool"))
}

fn paste_with(program: &Path) -> Result<(), String> {
    let keys = match key_syntax(program)? {
        KeySyntax::Symbolic => vec![LEGACY_PASTE_KEYS.to_string()],
        KeySyntax::Evdev => vec![
            format!("{KEY_LEFTCTRL}:1"),
            format!("{KEY_V}:1"),
            format!("{KEY_V}:0"),
            format!("{KEY_LEFTCTRL}:0"),
        ],
    };
    let output = Command::new(program)
        .arg("key")
        .args(keys)
        .output()
        .map_err(|e| format!("failed to run ydotool: {e}"))?;

    if output.status.success() {
        return Ok(());
    }

    let diagnostic = output_text(&output);
    Err(if diagnostic.is_empty() {
        "ydotool failed (is ydotoold running?)".to_string()
    } else {
        format!("ydotool failed: {diagnostic}")
    })
}

fn key_syntax(program: &Path) -> Result<KeySyntax, String> {
    // Top-level help needs neither a daemon nor /dev/uinput. 0.1.8 treats
    // numeric arguments as digit keys, so never try both syntaxes as fallback.
    let output = Command::new(program)
        .arg("help")
        .output()
        .map_err(|e| format!("failed to inspect ydotool: {e}"))?;
    let help = output_text(&output);
    let has_key_command = help.lines().any(|line| line.trim() == "key");
    if !help.contains("Usage: ydotool <cmd> <args>") || !has_key_command {
        return Err("unrecognized ydotool help; refusing to inject keys".to_string());
    }

    // The 1.x rewrite advertises its configurable socket; 0.1.8 has a fixed
    // socket and returns exit 1 for help. See upstream Client/ydotool{.cpp,.c}.
    if output.status.success() && help.contains(SOCKET_OVERRIDE_ENV) {
        return Ok(KeySyntax::Evdev);
    }
    if output.status.code() == Some(LEGACY_HELP_EXIT_CODE) && !help.contains(SOCKET_OVERRIDE_ENV) {
        return Ok(KeySyntax::Symbolic);
    }
    Err("unsupported ydotool help; refusing to inject keys".to_string())
}

fn output_text(output: &Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .trim()
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Mutex, MutexGuard};

    struct FakeYdotool {
        path: PathBuf,
        _lock: MutexGuard<'static, ()>,
    }

    impl FakeYdotool {
        fn new(script: &str) -> Self {
            static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
            static EXECUTABLE_LOCK: Mutex<()> = Mutex::new(());
            // A concurrent fork can inherit a writable fixture fd until exec,
            // causing ETXTBSY. Keep fixture creation and execution together.
            let lock = EXECUTABLE_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let directory = std::env::temp_dir().join(format!(
                "copywraith-ydotool-{}-{}",
                std::process::id(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&directory).unwrap();
            let path = directory.join("ydotool");
            std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
            Self { path, _lock: lock }
        }
    }

    impl Drop for FakeYdotool {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(self.path.parent().unwrap());
        }
    }

    #[test]
    fn fixture_recovers_after_a_test_panics() {
        let result = std::panic::catch_unwind(|| {
            let _program = FakeYdotool::new("exit 0");
            panic!("simulate a failed assertion while holding the fixture lock");
        });
        assert!(result.is_err());

        let _program = FakeYdotool::new("exit 0");
    }

    #[test]
    fn ubuntu_legacy_ydotool_uses_symbolic_keys() {
        // 0.1.8 prints top-level help on stderr and exits 1; key uses names.
        let program = FakeYdotool::new(
            r#"
case "$*" in
    help) printf 'Usage: ydotool <cmd> <args>\nAvailable commands:\n  key\n  type\n' >&2; exit 1 ;;
    'key ctrl+v') exit 0 ;;
    *) echo "unexpected arguments: $*" >&2; exit 2 ;;
esac
"#,
        );
        assert_eq!(paste_with(&program.path), Ok(()));
    }

    #[test]
    fn unknown_help_never_injects_keys() {
        let program = FakeYdotool::new(
            r#"
case "$*" in
    help) echo 'unknown implementation'; exit 0 ;;
    *) exit 0 ;;
esac
"#,
        );
        assert!(paste_with(&program.path)
            .unwrap_err()
            .contains("refusing to inject keys"));
    }

    #[test]
    fn modern_daemon_errors_on_stdout_are_reported() {
        let program = FakeYdotool::new(
            r#"
case "$*" in
    help) printf 'Usage: ydotool <cmd> <args>\n  key\nYDOTOOL_SOCKET\n' ;;
    *) echo 'failed to connect socket'; exit 2 ;;
esac
"#,
        );
        assert!(paste_with(&program.path)
            .unwrap_err()
            .contains("failed to connect socket"));
    }

    #[test]
    fn modern_ydotool_uses_evdev_keys() {
        let program = FakeYdotool::new(
            r#"
case "$*" in
    help) printf 'Usage: ydotool <cmd> <args>\nAvailable commands:\n  key\n  type\nUse environment variable YDOTOOL_SOCKET to specify daemon socket.\n' ;;
    'key 29:1 47:1 47:0 29:0') exit 0 ;;
    *) echo "unexpected arguments: $*" >&2; exit 2 ;;
esac
"#,
        );
        assert_eq!(paste_with(&program.path), Ok(()));
    }
}
