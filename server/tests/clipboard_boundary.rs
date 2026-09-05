#[test]
fn desktop_clipboard_uses_one_private_adapter() {
    let manifest = include_str!("../../src-tauri/Cargo.toml");
    assert!(
        !manifest.contains("tauri-plugin-clipboard ="),
        "the plugin exposes an incompatible 0.2 context"
    );
    assert!(manifest.contains("clipboard-rs = \"0.3.5\""));
    let paste = include_str!("../../src-tauri/src/paste.rs");
    assert!(!paste.contains("ClipboardContent"));
    assert!(!paste.contains("clipboard_rs"));
    assert!(!include_str!("../../src-tauri/capabilities/default.json").contains("\"clipboard:"));
}
