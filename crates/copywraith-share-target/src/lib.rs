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
}

#[derive(Debug, serde::Deserialize)]
pub struct PendingShareStatus {
    pub staged: bool,
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("copywraith-share-target")
        .setup(|_app, _api| {
            #[cfg(target_os = "android")]
            {
                let handle = _api.register_android_plugin(
                    PLUGIN_IDENTIFIER,
                    "CopywraithSharePlugin",
                )?;
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
