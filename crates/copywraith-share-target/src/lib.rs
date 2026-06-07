use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};

#[cfg(target_os = "android")]
use tauri::plugin::PluginHandle;

#[cfg(target_os = "android")]
const PLUGIN_IDENTIFIER: &str = "ch.lkmc.copywraith.share";

pub trait ShareTargetExt<R: Runtime> {
    fn share_target(&self) -> &ShareTarget<R>;
}

impl<R: Runtime, T: Manager<R>> ShareTargetExt<R> for T {
    fn share_target(&self) -> &ShareTarget<R> {
        self.state::<ShareTarget<R>>().inner()
    }
}

pub struct ShareTarget<R: Runtime> {
    #[cfg(target_os = "android")]
    handle: PluginHandle<R>,
    #[cfg(not(target_os = "android"))]
    _marker: std::marker::PhantomData<fn() -> R>,
}

impl<R: Runtime> ShareTarget<R> {
    pub fn collect_pending_share(&self) -> tauri::Result<PendingShareStatus> {
        #[cfg(target_os = "android")]
        {
            self.handle
                .run_mobile_plugin("collectPendingShare", ())
                .map_err(Into::into)
        }

        #[cfg(not(target_os = "android"))]
        {
            Ok(PendingShareStatus { staged: false })
        }
    }

    pub fn shizuku_status(&self) -> tauri::Result<ShizukuClipboardStatus> {
        #[cfg(target_os = "android")]
        {
            self.handle
                .run_mobile_plugin("shizukuStatus", ())
                .map_err(Into::into)
        }

        #[cfg(not(target_os = "android"))]
        {
            Ok(ShizukuClipboardStatus::unavailable(
                "Shizuku is only available on Android.",
            ))
        }
    }

    pub fn start_shizuku_clipboard_listener(
        &self,
        config: ShizukuClipboardConfig,
    ) -> tauri::Result<ShizukuClipboardStatus> {
        #[cfg(target_os = "android")]
        {
            self.handle
                .run_mobile_plugin("startShizukuClipboardListener", config)
                .map_err(Into::into)
        }

        #[cfg(not(target_os = "android"))]
        {
            let _ = config;
            Ok(ShizukuClipboardStatus::unavailable(
                "Shizuku is only available on Android.",
            ))
        }
    }

    pub fn stop_shizuku_clipboard_listener(&self) -> tauri::Result<ShizukuClipboardStatus> {
        #[cfg(target_os = "android")]
        {
            self.handle
                .run_mobile_plugin("stopShizukuClipboardListener", ())
                .map_err(Into::into)
        }

        #[cfg(not(target_os = "android"))]
        {
            Ok(ShizukuClipboardStatus {
                state: "disabled".to_string(),
                message: "Shizuku is only available on Android.".to_string(),
                available: false,
                enabled: false,
                listening: false,
                started: None,
                backend_uid: None,
                last_clipboard_text_at: None,
                text: None,
            })
        }
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct PendingShareStatus {
    pub staged: bool,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ShizukuClipboardConfig {
    pub server_url_primary: String,
    pub server_url_fallback: String,
    pub api_key: String,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ShizukuClipboardStatus {
    pub state: String,
    pub message: String,
    #[serde(default)]
    pub available: bool,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub listening: bool,
    #[serde(default)]
    pub started: Option<bool>,
    #[serde(default)]
    pub backend_uid: Option<i32>,
    #[serde(default)]
    pub last_clipboard_text_at: Option<i64>,
    #[serde(default)]
    pub text: Option<String>,
}

#[cfg(not(target_os = "android"))]
impl ShizukuClipboardStatus {
    fn unavailable(message: impl Into<String>) -> Self {
        Self {
            state: "unavailable".to_string(),
            message: message.into(),
            available: false,
            enabled: false,
            listening: false,
            started: None,
            backend_uid: None,
            last_clipboard_text_at: None,
            text: None,
        }
    }
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("copywraith-share-target")
        .setup(|_app, _api| {
            #[cfg(target_os = "android")]
            {
                let handle =
                    _api.register_android_plugin(PLUGIN_IDENTIFIER, "CopywraithSharePlugin")?;
                _app.manage(ShareTarget { handle });
            }

            #[cfg(not(target_os = "android"))]
            _app.manage(ShareTarget::<R> {
                _marker: std::marker::PhantomData,
            });

            Ok(())
        })
        .build()
}
