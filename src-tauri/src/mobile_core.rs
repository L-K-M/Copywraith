use crate::{storage::LocalStorage, sync::SyncClient};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[cfg(target_os = "android")]
pub(crate) fn shared_core(data_dir: &Path) -> anyhow::Result<Arc<MobileCore>> {
    static REGISTRY: std::sync::OnceLock<CoreRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(CoreRegistry::default).open(data_dir)
}

/// Activity and service initialization must share the same storage and protocol lock.
#[derive(Default)]
pub(crate) struct CoreRegistry {
    core: Mutex<Option<Arc<MobileCore>>>,
}

pub(crate) struct MobileCore {
    data_dir: PathBuf,
    storage: Arc<LocalStorage>,
    sync_client: Arc<SyncClient>,
}

impl CoreRegistry {
    pub(crate) fn open(&self, data_dir: &Path) -> anyhow::Result<Arc<MobileCore>> {
        let data_dir = data_dir.canonicalize()?;
        let mut slot = self
            .core
            .lock()
            .map_err(|_| anyhow::anyhow!("Mobile core initialization was interrupted"))?;

        if let Some(core) = slot.as_ref() {
            anyhow::ensure!(
                core.data_dir == data_dir,
                "Mobile core data directory changed"
            );
            return Ok(Arc::clone(core));
        }

        // Publish only a complete core; a failed open remains retryable.
        let storage = Arc::new(LocalStorage::new(&data_dir)?);
        let sync_client = Arc::new(SyncClient::new(&storage));
        let core = Arc::new(MobileCore {
            data_dir,
            storage,
            sync_client,
        });
        *slot = Some(Arc::clone(&core));
        Ok(core)
    }
}

impl MobileCore {
    pub(crate) fn storage(&self) -> Arc<LocalStorage> {
        Arc::clone(&self.storage)
    }

    pub(crate) fn sync_client(&self) -> Arc<SyncClient> {
        Arc::clone(&self.sync_client)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;

    #[test]
    fn activity_and_service_share_one_core_during_concurrent_startup() {
        let directory = tempfile::tempdir().unwrap();
        let registry = Arc::new(CoreRegistry::default());
        let barrier = Arc::new(Barrier::new(2));
        let mut threads = Vec::new();

        for _ in ["activity", "service"] {
            let registry = Arc::clone(&registry);
            let barrier = Arc::clone(&barrier);
            let path = directory.path().to_path_buf();
            threads.push(std::thread::spawn(move || {
                barrier.wait();
                registry.open(&path).unwrap()
            }));
        }

        let cores: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        assert!(Arc::ptr_eq(&cores[0], &cores[1]));
        assert!(Arc::ptr_eq(&cores[0].storage(), &cores[1].storage()));
        assert!(Arc::ptr_eq(
            &cores[0].sync_client(),
            &cores[1].sync_client()
        ));
    }

    #[test]
    fn failed_initialization_does_not_poison_a_later_open() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("copywraith.db");
        std::fs::write(&database, b"not a SQLite database").unwrap();
        let registry = CoreRegistry::default();
        assert!(registry.open(directory.path()).is_err());

        std::fs::remove_file(database).unwrap();
        assert!(registry.open(directory.path()).is_ok());
    }

    #[test]
    fn path_aliases_reuse_the_core_but_another_directory_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let registry = CoreRegistry::default();
        let core = registry.open(directory.path()).unwrap();
        let alias = registry.open(&directory.path().join(".")).unwrap();

        assert!(Arc::ptr_eq(&core, &alias));
        assert!(registry.open(other.path()).is_err());
    }
}

#[cfg(any(test, feature = "android-runtime-probe"))]
impl MobileCore {
    pub(crate) fn prepare_probe(&self, endpoint: &str) -> anyhow::Result<()> {
        // Only a fresh debug installation may receive fixture settings.
        let mut settings = self.storage.get_settings();
        anyhow::ensure!(
            settings.server_url_primary.is_empty(),
            "Probe requires cleared app data"
        );
        settings.server_url_primary = endpoint.into();
        settings.api_key = "fixture-password".into();
        self.storage.save_settings(&settings)?;
        self.capture_probe("android-headless-upload")
    }

    pub(crate) fn capture_probe(&self, text: &str) -> anyhow::Result<()> {
        use copywraith_core::models::{ClipboardFlavors, ContentType};
        let flavors = ClipboardFlavors {
            text_plain: Some(text.into()),
            ..Default::default()
        };
        let hash = flavors.payload_hash(ContentType::Text, None);
        self.storage
            .insert_entry(ContentType::Text, &flavors, None, &hash, None)?;
        Ok(())
    }

    pub(crate) async fn exchange(&self) -> anyhow::Result<()> {
        self.sync_client.sync_unsynced_entries(&self.storage).await;
        self.sync_client.pull_new_entries(&self.storage).await?;
        Ok(())
    }

    pub(crate) fn probe_contains(&self, text: &str) -> anyhow::Result<bool> {
        use copywraith_core::models::{ClipboardFlavors, ContentType};
        let flavors = ClipboardFlavors {
            text_plain: Some(text.into()),
            ..Default::default()
        };
        self.storage
            .has_content_hash(&flavors.payload_hash(ContentType::Text, None))
    }
}
