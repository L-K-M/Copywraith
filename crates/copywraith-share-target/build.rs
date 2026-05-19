fn main() {
    const COMMANDS: &[&str] = &[];

    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .try_build()
        .unwrap();
}
