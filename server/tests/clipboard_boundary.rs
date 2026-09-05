#[test]
fn desktop_clipboard_uses_one_private_adapter() {
    const BACKEND: &str = "name = \"clipboard-rs\"";
    const REMOVED_PLUGIN: &str = "name = \"tauri-plugin-clipboard\"";

    // Resolved package names cover aliases without rejecting compatible bumps
    // or Android's distinct clipboard-manager plugin.
    let lockfile = include_str!("../../Cargo.lock");
    assert_eq!(lockfile.lines().filter(|line| *line == BACKEND).count(), 1);
    assert!(
        !lockfile.lines().any(|line| line == REMOVED_PLUGIN),
        "the plugin exposes an incompatible 0.2 context"
    );
    let paste = include_str!("../../src-tauri/src/paste.rs");
    assert!(!paste.contains("ClipboardContent"));
    assert!(!paste.contains("clipboard_rs"));
    assert!(!include_str!("../../src-tauri/capabilities/default.json").contains("\"clipboard:"));
}
