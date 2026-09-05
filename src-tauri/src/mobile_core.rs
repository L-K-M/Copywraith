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
