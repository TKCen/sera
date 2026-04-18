use std::sync::Arc;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::backend::QueueError;

/// Workspace-wide concurrency cap implemented as a counting semaphore.
pub struct GlobalThrottle {
    cap: usize,
    semaphore: Arc<Semaphore>,
}

impl GlobalThrottle {
    pub fn new(cap: usize) -> Self {
        Self {
            cap,
            semaphore: Arc::new(Semaphore::new(cap)),
        }
    }

    /// Try to acquire one permit without blocking.
    /// Returns `Err(QueueError::Unavailable)` when the cap is exhausted.
    pub fn try_acquire(&self) -> Result<OwnedSemaphorePermit, QueueError> {
        Arc::clone(&self.semaphore)
            .try_acquire_owned()
            .map_err(|_| QueueError::Unavailable {
                reason: format!("global throttle cap ({}) exhausted", self.cap),
            })
    }

    /// Number of permits still available.
    pub fn available(&self) -> usize {
        self.semaphore.available_permits()
    }
}
