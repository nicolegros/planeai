//! Every plugin sidecar process the app spawned, so the moment before the app process exits can
//! kill the ones still running. Exiting never drops their `Child` handles, so `kill_on_drop`
//! cannot do it, and a sidecar that ignores stdin EOF would otherwise outlive the app.

use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use tokio::process::Child;
use tokio::sync::Mutex as AsyncMutex;

pub(crate) type SharedChild = Arc<AsyncMutex<Child>>;

/// Holders of a child's lock keep it for a single non-blocking call, so a short wait suffices.
const LOCK_WAIT: Duration = Duration::from_millis(200);

#[derive(Default)]
pub(crate) struct PluginChildren(Mutex<Vec<Weak<AsyncMutex<Child>>>>);

impl PluginChildren {
    pub(crate) fn track(&self, child: Child) -> SharedChild {
        let child = Arc::new(AsyncMutex::new(child));
        let mut tracked = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        tracked.retain(|tracked| tracked.strong_count() > 0);
        tracked.push(Arc::downgrade(&child));
        child
    }

    /// Kills every tracked child that is still running and returns how many it killed. Blocking,
    /// so it can run where no async runtime drives tasks anymore.
    pub(crate) fn kill_all(&self) -> usize {
        let children = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .filter_map(Weak::upgrade)
            .collect::<Vec<_>>();
        let deadline = Instant::now() + LOCK_WAIT;
        children
            .iter()
            .filter(|child| kill_if_running(child, deadline))
            .count()
    }
}

fn kill_if_running(child: &AsyncMutex<Child>, deadline: Instant) -> bool {
    let mut child = loop {
        match child.try_lock() {
            Ok(child) => break child,
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(1)),
            Err(_) => {
                tracing::warn!("plugin process stayed locked; it may outlive the app");
                return false;
            }
        }
    };
    if !matches!(child.try_wait(), Ok(None)) {
        return false;
    }
    match child.start_kill() {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(%error, "failed to kill plugin process before exit");
            false
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::process::Stdio;
    use tokio::process::Command;

    /// Stands in for a wedged sidecar: it never reads stdin and ignores the hangup a parent's
    /// exit can send, so only a kill stops it.
    fn spawn_wedged_sidecar() -> Child {
        Command::new("/bin/sh")
            .args(["-c", "trap '' HUP PIPE TERM; while :; do sleep 1; done"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap()
    }

    async fn exits_within(child: &SharedChild, limit: Duration) -> bool {
        tokio::time::timeout(limit, async { child.lock().await.wait().await })
            .await
            .is_ok()
    }

    #[tokio::test]
    async fn kills_a_running_sidecar_that_ignores_eof_and_hangup() {
        let children = PluginChildren::default();
        let child = children.track(spawn_wedged_sidecar());
        child.lock().await.stdin.take();

        assert!(
            !exits_within(&child, Duration::from_millis(300)).await,
            "the sidecar must survive EOF, as a wedged plugin does"
        );
        assert_eq!(children.kill_all(), 1);
        assert!(exits_within(&child, Duration::from_secs(5)).await);
    }

    #[tokio::test]
    async fn waits_out_a_brief_lock_holder() {
        let children = PluginChildren::default();
        let child = children.track(spawn_wedged_sidecar());
        let holder = child.clone().try_lock_owned().unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            drop(holder);
        });

        assert_eq!(children.kill_all(), 1);
        release.join().unwrap();
        assert!(exits_within(&child, Duration::from_secs(5)).await);
    }

    #[tokio::test]
    async fn skips_children_that_exited_or_were_dropped() {
        let children = PluginChildren::default();
        let exited = children.track(
            Command::new("/bin/sh")
                .args(["-c", "exit 0"])
                .spawn()
                .unwrap(),
        );
        exited.lock().await.wait().await.unwrap();
        drop(children.track(spawn_wedged_sidecar()));

        assert_eq!(children.kill_all(), 0);
        let tracked = children.0.lock().unwrap();
        assert_eq!(tracked.iter().filter(|c| c.strong_count() > 0).count(), 1);
    }
}
