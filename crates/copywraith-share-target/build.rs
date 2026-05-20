fn main() {
    const COMMANDS: &[&str] = &[
        "collectPendingShare",
        "shizukuStatus",
        "startShizukuClipboardListener",
        "stopShizukuClipboardListener",
        "readShizukuClipboard",
    ];

    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .try_build()
        .unwrap();
}
