use std::sync::Arc;

/// Trait seam for PTY output observation — decouples the reader thread from
/// NotifyState and Tauri's AppHandle.
pub trait OutputObserver: Send + Sync {
    fn on_output(&self, session_id: &str, byte_count: usize);
}

/// No-op observer for tests or when no notification system is configured.
pub struct NoopObserver;

impl OutputObserver for NoopObserver {
    fn on_output(&self, _session_id: &str, _byte_count: usize) {}
}

/// Dispatches to multiple observers.
#[allow(dead_code)]
pub struct CompositeObserver {
    observers: Vec<Arc<dyn OutputObserver>>,
}

impl CompositeObserver {
    #[allow(dead_code)]
    pub fn new(observers: Vec<Arc<dyn OutputObserver>>) -> Self {
        Self { observers }
    }
}

impl OutputObserver for CompositeObserver {
    fn on_output(&self, session_id: &str, byte_count: usize) {
        for o in &self.observers {
            o.on_output(session_id, byte_count);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct RecordingObserver {
        calls: AtomicUsize,
    }

    impl RecordingObserver {
        fn new() -> Self {
            Self {
                calls: AtomicUsize::new(0),
            }
        }
    }

    impl OutputObserver for RecordingObserver {
        fn on_output(&self, _session_id: &str, _byte_count: usize) {
            self.calls.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn noop_observer_does_not_panic() {
        let o = NoopObserver;
        o.on_output("test", 100);
    }

    #[test]
    fn composite_dispatches_to_all() {
        let a = Arc::new(RecordingObserver::new());
        let b = Arc::new(RecordingObserver::new());
        let composite = CompositeObserver::new(vec![a.clone(), b.clone()]);

        composite.on_output("s1", 42);
        composite.on_output("s1", 10);

        assert_eq!(a.calls.load(Ordering::Relaxed), 2);
        assert_eq!(b.calls.load(Ordering::Relaxed), 2);
    }
}
