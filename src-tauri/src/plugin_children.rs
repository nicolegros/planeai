//! Every plugin sidecar process the app spawned, so the moment before the app process exits can
//! kill the ones still running. Exiting never drops their `Child` handles, so `kill_on_drop`
//! cannot do it, and a sidecar that ignores stdin EOF would otherwise outlive the app.

use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use tokio::process::{Child, Command};
use tokio::sync::Mutex as AsyncMutex;

pub(crate) type SharedChild = Arc<AsyncMutex<Child>>;

/// Holders of a child's lock keep it for a single non-blocking call, so a short wait suffices.
const LOCK_WAIT: Duration = Duration::from_millis(200);
const EXIT_POLL_INTERVAL: Duration = Duration::from_millis(20);

#[derive(Default)]
pub(crate) struct PluginChildren(Mutex<Registry>);

#[derive(Default)]
struct Registry {
    /// Set by `kill_all`: a child spawned after it would never be killed.
    closed: bool,
    children: Vec<Weak<AsyncMutex<Child>>>,
}

impl PluginChildren {
    /// Spawns under the registry lock, so no child can start between `kill_all`'s sweep and exit.
    pub(crate) fn spawn(&self, command: &mut Command) -> std::io::Result<SharedChild> {
        let mut registry = self.lock();
        if registry.closed {
            return Err(std::io::Error::other("the app is exiting"));
        }
        let child = Arc::new(AsyncMutex::new(command.spawn()?));
        registry
            .children
            .retain(|tracked| tracked.strong_count() > 0);
        registry.children.push(Arc::downgrade(&child));
        Ok(child)
    }

    /// Kills every tracked child that is still running, refuses later spawns, and returns how
    /// many it killed. Blocking, so it can run where no async runtime drives tasks anymore.
    pub(crate) fn kill_all(&self) -> usize {
        let children = {
            let mut registry = self.lock();
            registry.closed = true;
            registry
                .children
                .iter()
                .filter_map(Weak::upgrade)
                .collect::<Vec<_>>()
        };
        let deadline = Instant::now() + LOCK_WAIT;
        children
            .iter()
            .filter(|child| kill_if_running(child, deadline))
            .count()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Registry> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Polls rather than awaiting `wait()`, which would hold the child's lock until exit and block
/// `kill_all`.
pub(crate) async fn wait_for_exit(child: &SharedChild, deadline: Instant) -> std::io::Result<bool> {
    loop {
        if child.lock().await.try_wait()?.is_some() {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        tokio::time::sleep(EXIT_POLL_INTERVAL).await;
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

    /// Stands in for a wedged sidecar: it never reads stdin and ignores the hangup a parent's
    /// exit can send, so only a kill stops it.
    fn wedged_sidecar() -> Command {
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "trap '' HUP PIPE TERM; while :; do sleep 1; done"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        command
    }

    fn exiting_child() -> Command {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "exit 0"]);
        command
    }

    async fn exits_within(child: &SharedChild, limit: Duration) -> bool {
        wait_for_exit(child, Instant::now() + limit).await.unwrap()
    }

    #[tokio::test]
    async fn kills_a_running_sidecar_that_ignores_eof_and_hangup() {
        let children = PluginChildren::default();
        let child = children.spawn(&mut wedged_sidecar()).unwrap();
        child.lock().await.stdin.take();

        assert!(
            !exits_within(&child, Duration::from_millis(300)).await,
            "the sidecar must survive EOF, as a wedged plugin does"
        );
        assert_eq!(children.kill_all(), 1);
        assert!(exits_within(&child, Duration::from_secs(5)).await);
    }

    #[tokio::test]
    async fn kills_a_sidecar_while_a_graceful_stop_waits_for_it() {
        let children = Arc::new(PluginChildren::default());
        let child = children.spawn(&mut wedged_sidecar()).unwrap();
        let waiting = {
            let child = child.clone();
            tokio::spawn(async move {
                wait_for_exit(&child, Instant::now() + Duration::from_secs(10)).await
            })
        };
        tokio::time::sleep(Duration::from_millis(100)).await;

        let killer = children.clone();
        let killed = tokio::task::spawn_blocking(move || killer.kill_all())
            .await
            .unwrap();
        assert_eq!(killed, 1);
        let exited = tokio::time::timeout(Duration::from_secs(5), waiting).await;
        assert!(matches!(exited, Ok(Ok(Ok(true)))), "{exited:?}");
    }

    #[tokio::test]
    async fn waits_out_a_brief_lock_holder() {
        let children = PluginChildren::default();
        let child = children.spawn(&mut wedged_sidecar()).unwrap();
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
        let exited = children.spawn(&mut exiting_child()).unwrap();
        assert!(exits_within(&exited, Duration::from_secs(5)).await);
        drop(children.spawn(&mut wedged_sidecar()).unwrap());
        let _running = children.spawn(&mut wedged_sidecar()).unwrap();

        assert_eq!(
            children.lock().children.len(),
            2,
            "the dropped child is pruned"
        );
        assert_eq!(children.kill_all(), 1);
    }

    #[tokio::test]
    async fn refuses_to_spawn_once_killed() {
        let children = PluginChildren::default();
        children.kill_all();

        let error = children.spawn(&mut exiting_child()).unwrap_err();
        assert_eq!(error.to_string(), "the app is exiting");
    }
}
