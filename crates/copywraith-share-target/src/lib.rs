use tauri::{
    plugin::{Builder, TauriPlugin},
    Runtime,
};

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("copywraith-share-target")
        .setup(|_app, _api| {
            #[cfg(target_os = "android")]
            _api.register_android_plugin("ch.lkmc.copywraith.share", "CopywraithSharePlugin")?;

            Ok(())
        })
        .build()
}
