//! Process leases cover initialization, work, cancellation, and teardown.
use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

#[derive(Clone, Copy)]
pub(crate) enum LeaseKind {
    Service,
    Job,
}

#[derive(Default)]
pub(crate) struct MobileRuntime {
    next_id: AtomicU64,
    leases: Mutex<HashMap<u64, LeaseKind>>,
    job: Mutex<Option<Job>>,
    completed: AtomicU64,
}

struct Job {
    id: u64,
    cancel: Option<oneshot::Sender<()>>,
}

pub(crate) struct Lease {
    owner: Arc<MobileRuntime>,
    id: u64,
}

impl Drop for Lease {
    fn drop(&mut self) {
        self.owner.leases.lock().unwrap().remove(&self.id);
    }
}

impl MobileRuntime {
    pub(crate) fn acquire(self: &Arc<Self>, kind: LeaseKind) -> Lease {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst) + 1;
        self.leases.lock().unwrap().insert(id, kind);
        Lease {
            owner: self.clone(),
            id,
        }
    }

    pub(crate) fn prevents_exit(&self) -> bool {
        !self.leases.lock().unwrap().is_empty()
    }

    pub(crate) fn lease_count(&self) -> usize {
        self.leases.lock().unwrap().len()
    }

    pub(crate) fn completed(&self) -> u64 {
        self.completed.load(Ordering::SeqCst)
    }

    pub(crate) fn job_id(&self) -> Option<u64> {
        self.job.lock().unwrap().as_ref().map(|job| job.id)
    }

    /// Reserve before spawning so startup and overlapping jobs cannot race exit.
    pub(crate) fn start_job<F>(
        self: &Arc<Self>,
        handle: &tokio::runtime::Handle,
        work: F,
    ) -> Option<u64>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let mut slot = self.job.lock().unwrap();
        if slot.is_some() {
            return None;
        }

        let lease = self.acquire(LeaseKind::Job);
        let id = lease.id;
        let (cancel, stopped) = oneshot::channel();
        *slot = Some(Job {
            id,
            cancel: Some(cancel),
        });
        let guard = JobGuard {
            owner: self.clone(),
            lease: Some(lease),
            id,
        };
        handle.spawn(async move {
            // Dropping the HTTP future retains the protocol's durable frozen request.
            let _guard = guard;
            tokio::select! {
                biased;
                _ = stopped => {}
                _ = work => {}
            }
        });
        Some(id)
    }

    pub(crate) fn stop_job(&self, id: u64) {
        let mut slot = self.job.lock().unwrap();
        let Some(job) = slot.as_mut().filter(|job| job.id == id) else {
            return;
        };
        if let Some(cancel) = job.cancel.take() {
            let _ = cancel.send(());
        }
        // The worker releases its reservation only after its future is dropped.
    }
}

struct JobGuard {
    owner: Arc<MobileRuntime>,
    lease: Option<Lease>,
    id: u64,
}

impl Drop for JobGuard {
    fn drop(&mut self) {
        let mut slot = self.owner.job.lock().unwrap();
        if slot.as_ref().map(|job| job.id) != Some(self.id) {
            return;
        }
        self.lease.take();
        *slot = None;
        self.owner.completed.store(self.id, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_lease_covers_failed_initialization_and_teardown() {
        let owner = Arc::new(MobileRuntime::default());
        let service = owner.acquire(LeaseKind::Service);
        assert!(owner.prevents_exit());
        let registry = crate::mobile_core::CoreRegistry::default();
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("copywraith.db"), b"invalid").unwrap();
        assert!(registry.open(directory.path()).is_err());
        assert!(owner.prevents_exit());
        drop(service);
        assert!(!owner.prevents_exit());
    }

    #[tokio::test]
    async fn cancellation_holds_lease_until_future_teardown_and_rejects_overlap() {
        let owner = Arc::new(MobileRuntime::default());
        let handle = tokio::runtime::Handle::current();
        let id = owner.start_job(&handle, std::future::pending()).unwrap();
        assert!(owner.prevents_exit());
        assert!(owner.start_job(&handle, async {}).is_none());
        owner.stop_job(id + 1);
        assert_eq!(owner.job_id(), Some(id));
        owner.stop_job(id);
        assert!(owner.prevents_exit());
        assert!(owner.start_job(&handle, async {}).is_none());
        tokio::task::yield_now().await;
        assert_eq!(owner.completed(), id);
        assert!(!owner.prevents_exit());

        let next = owner.start_job(&handle, std::future::pending()).unwrap();
        owner.stop_job(id);
        assert_eq!(owner.job_id(), Some(next));
        owner.stop_job(next);
        tokio::task::yield_now().await;
        assert!(!owner.prevents_exit());
    }

    #[tokio::test]
    async fn job_initialization_failure_releases_only_after_worker_returns() {
        let owner = Arc::new(MobileRuntime::default());
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("copywraith.db"), b"invalid").unwrap();
        let worker_owner = owner.clone();
        let id = owner
            .start_job(&tokio::runtime::Handle::current(), async move {
                assert!(worker_owner.prevents_exit());
                let registry = crate::mobile_core::CoreRegistry::default();
                assert!(registry.open(directory.path()).is_err());
                assert!(worker_owner.prevents_exit());
            })
            .unwrap();
        assert!(owner.prevents_exit());
        tokio::task::yield_now().await;
        assert_eq!(owner.completed(), id);
        assert!(!owner.prevents_exit());
    }

    #[tokio::test]
    async fn job_completion_does_not_release_service_lease() {
        let owner = Arc::new(MobileRuntime::default());
        let service = owner.acquire(LeaseKind::Service);
        let id = owner
            .start_job(&tokio::runtime::Handle::current(), async {})
            .unwrap();
        tokio::task::yield_now().await;
        assert_eq!(owner.completed(), id);
        assert_eq!(owner.lease_count(), 1);
        drop(service);
        assert!(!owner.prevents_exit());
    }
}
