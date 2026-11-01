//! Trailing-edge debouncer: the `clearTimeout(t); t = setTimeout(fn, ms)`
//! idiom the Electron main processes use for auto-commit and auto-push.

use std::future::Future;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use tokio::runtime::Handle;
use tokio::task::JoinHandle;

static RUNTIME: OnceLock<Handle> = OnceLock::new();

/// Runtime used when `schedule` is called from a thread with no tokio
/// context (e.g. a GUI toolkit's event callback).
pub fn set_runtime(handle: Handle) {
    let _ = RUNTIME.set(handle);
}

fn spawn<F: Future<Output = ()> + Send + 'static>(f: F) -> JoinHandle<()> {
    match Handle::try_current() {
        Ok(h) => h.spawn(f),
        Err(_) => RUNTIME
            .get()
            .expect("laankit_core::debounce::set_runtime not called")
            .spawn(f),
    }
}

#[derive(Default)]
pub struct Debouncer {
    pending: Mutex<Option<JoinHandle<()>>>,
}

impl Debouncer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Cancel any pending run and schedule `f` to run after `delay`.
    pub fn schedule<F, Fut>(&self, delay: Duration, f: F)
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let handle = spawn(async move {
            tokio::time::sleep(delay).await;
            f().await;
        });
        if let Some(prev) = self.pending.lock().unwrap().replace(handle) {
            prev.abort();
        }
    }

    /// Cancel a pending run. Returns true if one was pending.
    pub fn cancel(&self) -> bool {
        match self.pending.lock().unwrap().take() {
            Some(h) if !h.is_finished() => {
                h.abort();
                true
            }
            _ => false,
        }
    }

    pub fn is_pending(&self) -> bool {
        self.pending
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|h| !h.is_finished())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn only_last_scheduled_run_fires() {
        let d = Debouncer::new();
        let hits = Arc::new(AtomicUsize::new(0));
        for _ in 0..5 {
            let h = hits.clone();
            d.schedule(Duration::from_millis(30), move || async move {
                h.fetch_add(1, Ordering::SeqCst);
            });
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }
}
