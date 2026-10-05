//! Terminal tabs: the lifecycle of every tab a TaskWorkspace layout opens on a PTY, such as a
//! shell, a terminal editor or a provider handoff's program (ADR-0015).
//!
//! Each tab goes Reserved → Spawning → Live → Closing → Gone. Its index comes from a counter
//! persisted per session, so a pty key is never handed out twice, and every tab reports its
//! end exactly once as a [`TabEnded`], whoever caused it.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

/// A terminal tab's identity; its pty key is `<session id>:<index>`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TabId {
    session_id: String,
    index: u32,
}

impl TabId {
    pub fn new(session_id: impl Into<String>, index: u32) -> Self {
        Self {
            session_id: session_id.into(),
            index,
        }
    }

    /// The tab a pty key names; `None` for an agent's key, or any other resource's.
    pub fn parse(pty_key: &str) -> Option<Self> {
        let (session_id, index) = pty_key.split_once(':')?;
        let well_formed = !session_id.is_empty()
            && !index.starts_with('0')
            && !index.is_empty()
            && index.bytes().all(|byte| byte.is_ascii_digit());
        if !well_formed {
            return None;
        }
        Some(Self::new(session_id, index.parse().ok()?))
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn index(&self) -> u32 {
        self.index
    }

    pub fn key(&self) -> String {
        format!("{}:{}", self.session_id, self.index)
    }
}

/// What a tab runs when it starts.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TabSpec {
    /// The user's login shell.
    Shell,
    /// A command line run before the shell, such as a terminal editor.
    Command { command: String },
    /// A program run without a shell, such as a provider handoff's TUI: the tab ends with it.
    Program { argv: Vec<String> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TabEndReason {
    Closed,
    Exited,
    FailedToStart,
    SessionEnded,
}

/// The one report of a tab's end, emitted to the frontend as `tab-ended`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TabEnded {
    pub pty_key: String,
    pub reason: TabEndReason,
    /// Why it failed to start.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Tab identity across runs.
pub trait TabStore: Send + Sync {
    /// An index the session's tabs never had.
    fn allocate(&self, session_id: &str) -> Result<u32, String>;
    fn record_ended(&self, tab: &TabId) -> Result<(), String>;
    fn is_ended(&self, tab: &TabId) -> Result<bool, String>;
}

/// The processes behind tabs, one adapter for every session backend.
pub trait TabHost: Send + Sync {
    /// Where a tab's output goes.
    type Output: Send;

    /// Start the tab's process, or connect to the one already running it without starting
    /// another. A `reconnect` fails rather than start one when that process is gone.
    fn attach(
        &self,
        tab: &TabId,
        spec: &TabSpec,
        output: Self::Output,
        reconnect: bool,
    ) -> impl Future<Output = Result<(), String>> + Send;

    /// End the tab's process and release its terminal. Fails, releasing nothing, when the
    /// process may still be running.
    fn end(&self, tab: &TabId) -> impl Future<Output = Result<(), String>> + Send;

    /// Release the tab's terminal, its process being gone.
    fn release(&self, tab: &TabId);

    /// Whether a process runs behind the tab, as far as the host can tell.
    fn is_running(&self, tab: &TabId) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Not started in this run. A daemon or rmux may still run it (`may_run`) after a start
    /// that failed without confirming nothing runs; the next start adopts it.
    Reserved {
        may_run: bool,
    },
    /// Its process is starting, or a terminal is connecting to it.
    Spawning {
        exited: bool,
    },
    Live,
    Closing {
        exited: bool,
    },
}

struct Entry {
    spec: TabSpec,
    phase: Phase,
}

#[derive(Default)]
struct Registry {
    tabs: HashMap<TabId, Entry>,
    /// Tabs that ended in this run, so none can start again before the store records it.
    ended: HashSet<TabId>,
}

impl Registry {
    fn mark_ended(&mut self, tab: &TabId) {
        self.tabs.remove(tab);
        self.ended.insert(tab.clone());
    }

    /// What a tab without an entry is, given whether the store says it ended in an earlier run.
    fn unknown(&self, tab: &TabId, ended_earlier: Option<bool>) -> Unknown {
        if self.ended.contains(tab) || ended_earlier == Some(true) {
            Unknown::Ended
        } else if ended_earlier.is_none() {
            Unknown::Unchecked
        } else {
            Unknown::Fresh
        }
    }
}

enum Unknown {
    Ended,
    /// The store has not been asked yet.
    Unchecked,
    /// Never seen, as a tab saved before an app restart.
    Fresh,
}

/// How an attach went: a tab that ended, during it or before, says so through its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attached {
    Live,
    Ended,
}

enum AttachStep {
    Spawn { spec: TabSpec, reconnect: bool },
    Wait(watch::Receiver<u64>),
    CheckStore,
}

enum EndStep {
    Kill,
    Wait(watch::Receiver<u64>),
    CheckStore,
    /// Ended under the lock without a process to kill; only reporting it is left.
    Report,
}

enum ExitStep {
    Kill,
    CheckStore,
    Report,
}

pub struct TerminalTabs<H: TabHost> {
    host: H,
    store: Arc<dyn TabStore>,
    on_ended: Box<dyn Fn(&TabEnded) + Send + Sync>,
    registry: Mutex<Registry>,
    /// Bumped on every phase change, for operations waiting out another one on the same tab.
    changed: watch::Sender<u64>,
}

impl<H: TabHost> TerminalTabs<H> {
    pub fn new(
        host: H,
        store: Arc<dyn TabStore>,
        on_ended: impl Fn(&TabEnded) + Send + Sync + 'static,
    ) -> Self {
        Self {
            host,
            store,
            on_ended: Box::new(on_ended),
            registry: Mutex::new(Registry::default()),
            changed: watch::channel(0).0,
        }
    }

    fn registry(&self) -> MutexGuard<'_, Registry> {
        self.registry.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn notify(&self) {
        self.changed
            .send_modify(|version| *version = version.wrapping_add(1));
    }

    /// Whether the tab ended in an earlier run, read off the registry lock.
    async fn ended_in_earlier_run(&self, tab: &TabId) -> Result<bool, String> {
        let (store, tab) = (self.store.clone(), tab.clone());
        off_thread(move || store.is_ended(&tab)).await
    }

    /// Reserve a new tab running `spec`; it starts when a terminal attaches to it.
    pub fn open(&self, session_id: &str, spec: TabSpec) -> Result<TabId, String> {
        if matches!(&spec, TabSpec::Program { argv } if argv.is_empty()) {
            return Err("a tab's program cannot be empty".to_string());
        }
        let tab = TabId::new(session_id, self.store.allocate(session_id)?);
        self.registry().tabs.insert(
            tab.clone(),
            Entry {
                spec,
                phase: Phase::Reserved { may_run: false },
            },
        );
        Ok(tab)
    }

    /// Start a reserved tab, or connect a terminal to a live one. A tab from a saved layout
    /// that this run never opened, as after an app restart, starts as a shell. Fails when the
    /// tab stays: a reconnect failing while its process runs, or a start failing while a daemon
    /// or rmux may still run it; any other tab that cannot start reports it through its end.
    pub async fn attach(&self, tab: &TabId, output: H::Output) -> Result<Attached, String> {
        let mut ended_earlier = None;
        let (spec, reconnect) = loop {
            let step = {
                let mut guard = self.registry();
                let registry = &mut *guard;
                match registry.tabs.get_mut(tab) {
                    Some(entry) => match entry.phase {
                        Phase::Reserved { .. } | Phase::Live => {
                            let reconnect = entry.phase == Phase::Live;
                            entry.phase = Phase::Spawning { exited: false };
                            AttachStep::Spawn {
                                spec: entry.spec.clone(),
                                reconnect,
                            }
                        }
                        Phase::Spawning { .. } | Phase::Closing { .. } => {
                            AttachStep::Wait(self.changed.subscribe())
                        }
                    },
                    None => match registry.unknown(tab, ended_earlier) {
                        Unknown::Ended => return Ok(Attached::Ended),
                        Unknown::Unchecked => AttachStep::CheckStore,
                        Unknown::Fresh => {
                            registry.tabs.insert(
                                tab.clone(),
                                Entry {
                                    spec: TabSpec::Shell,
                                    phase: Phase::Spawning { exited: false },
                                },
                            );
                            AttachStep::Spawn {
                                spec: TabSpec::Shell,
                                reconnect: false,
                            }
                        }
                    },
                }
            };
            match step {
                AttachStep::Spawn { spec, reconnect } => break (spec, reconnect),
                AttachStep::Wait(mut changed) => {
                    let _ = changed.changed().await;
                }
                AttachStep::CheckStore => {
                    ended_earlier = Some(self.ended_in_earlier_run(tab).await?);
                }
            }
        };

        let attached = self.host.attach(tab, &spec, output, reconnect).await;
        let still_runs = attached.is_ok() || self.host.is_running(tab);
        let ends = {
            let mut registry = self.registry();
            let Some(entry) = registry.tabs.get_mut(tab) else {
                return attached.map(|()| Attached::Ended);
            };
            let Phase::Spawning { exited } = entry.phase else {
                return attached.map(|()| Attached::Ended);
            };
            let ends = match (&attached, exited, reconnect) {
                (_, true, _) => Some(TabEndReason::Exited),
                (Err(_), false, false) => Some(TabEndReason::FailedToStart),
                // A reconnect found its process gone, its exit not reported yet.
                (Err(_), false, true) if !still_runs => Some(TabEndReason::Exited),
                // A terminal failed to reconnect; the process behind it runs on.
                (Err(_), false, true) | (Ok(()), false, _) => None,
            };
            entry.phase = match ends {
                Some(_) => Phase::Closing { exited: true },
                None => Phase::Live,
            };
            ends
        };
        let Some(reason) = ends else {
            self.notify();
            return attached.map(|()| Attached::Live);
        };
        if reason != TabEndReason::FailedToStart {
            self.end_process(tab).await;
        } else if self.host.end(tab).await.is_err() {
            // A daemon or rmux the host could not reach may still run it: kept, to be closed or
            // started again.
            if let Some(entry) = self.registry().tabs.get_mut(tab) {
                entry.phase = Phase::Reserved { may_run: true };
            }
            self.notify();
            return attached.map(|()| Attached::Live);
        }
        self.registry().mark_ended(tab);
        let error = attached
            .err()
            .filter(|_| reason == TabEndReason::FailedToStart);
        self.report_end(tab, reason, error).await;
        Ok(Attached::Ended)
    }

    /// Close a tab whatever its phase. It stays live when ending its process fails.
    pub async fn close(&self, tab: &TabId) -> Result<(), String> {
        self.end_tab(tab, TabEndReason::Closed).await
    }

    async fn end_tab(&self, tab: &TabId, reason: TabEndReason) -> Result<(), String> {
        let mut ended_earlier = None;
        loop {
            let step = {
                let mut guard = self.registry();
                let registry = &mut *guard;
                match registry.tabs.get_mut(tab) {
                    Some(entry) => match entry.phase {
                        Phase::Reserved { may_run: false } => {
                            registry.mark_ended(tab);
                            EndStep::Report
                        }
                        Phase::Reserved { may_run: true } | Phase::Live => {
                            entry.phase = Phase::Closing { exited: false };
                            EndStep::Kill
                        }
                        // A close during a spawn waits for it, then ends what it started.
                        Phase::Spawning { .. } | Phase::Closing { .. } => {
                            EndStep::Wait(self.changed.subscribe())
                        }
                    },
                    None => match registry.unknown(tab, ended_earlier) {
                        Unknown::Ended => return Ok(()),
                        Unknown::Unchecked => EndStep::CheckStore,
                        // Not opened in this run, but its process may outlive the app.
                        Unknown::Fresh => {
                            registry.tabs.insert(
                                tab.clone(),
                                Entry {
                                    spec: TabSpec::Shell,
                                    phase: Phase::Closing { exited: false },
                                },
                            );
                            EndStep::Kill
                        }
                    },
                }
            };
            match step {
                EndStep::Kill => break,
                EndStep::Wait(mut changed) => {
                    let _ = changed.changed().await;
                }
                // Unsure, it goes on: ending a process that is gone does no harm.
                EndStep::CheckStore => {
                    let ended = self.ended_in_earlier_run(tab).await;
                    ended_earlier = Some(ended.unwrap_or_else(|error| {
                        tracing::warn!(pty_key = %tab.key(), %error, "failed to look up a tab's end");
                        false
                    }));
                }
                EndStep::Report => {
                    self.report_end(tab, reason, None).await;
                    return Ok(());
                }
            }
        }
        self.notify();

        let result = self.host.end(tab).await;
        let ends = {
            let mut registry = self.registry();
            let Some(entry) = registry.tabs.get_mut(tab) else {
                return result;
            };
            let Phase::Closing { exited, .. } = entry.phase else {
                return result;
            };
            let ends = match (&result, exited) {
                (Ok(()), _) => Some(reason),
                (Err(_), true) => Some(TabEndReason::Exited),
                (Err(_), false) => None,
            };
            match ends {
                Some(_) => registry.mark_ended(tab),
                None => entry.phase = Phase::Live,
            }
            ends
        };
        let Some(reason) = ends else {
            self.notify();
            return result;
        };
        if result.is_err() {
            // Its process is gone anyway, but its terminal is still held.
            self.host.release(tab);
        }
        self.report_end(tab, reason, None).await;
        Ok(())
    }

    /// A tab's process exited by itself.
    pub async fn exited(&self, tab: &TabId) {
        let mut ended_earlier = None;
        loop {
            let step = {
                let mut guard = self.registry();
                let registry = &mut *guard;
                match registry.tabs.get_mut(tab).map(|entry| &mut entry.phase) {
                    Some(phase @ Phase::Live) => {
                        *phase = Phase::Closing { exited: true };
                        ExitStep::Kill
                    }
                    Some(Phase::Spawning { exited, .. } | Phase::Closing { exited, .. }) => {
                        *exited = true;
                        return;
                    }
                    Some(Phase::Reserved { .. }) => return,
                    None => match registry.unknown(tab, ended_earlier) {
                        Unknown::Ended => return,
                        Unknown::Unchecked => ExitStep::CheckStore,
                        // Never attached in this run, as a daemon shell exiting after an app restart.
                        Unknown::Fresh => {
                            registry.mark_ended(tab);
                            ExitStep::Report
                        }
                    },
                }
            };
            match step {
                ExitStep::Kill => break,
                ExitStep::CheckStore => match self.ended_in_earlier_run(tab).await {
                    Ok(ended) => ended_earlier = Some(ended),
                    Err(error) => {
                        tracing::warn!(pty_key = %tab.key(), %error, "failed to look up a tab's end");
                        return;
                    }
                },
                ExitStep::Report => {
                    self.report_end(tab, TabEndReason::Exited, None).await;
                    return;
                }
            }
        }
        self.notify();
        self.end_process(tab).await;
        self.registry().mark_ended(tab);
        self.report_end(tab, TabEndReason::Exited, None).await;
    }

    /// A session ended: the programs in its tabs end with it rather than drive its
    /// conversation unseen. Its shells keep their own lifecycle.
    pub async fn session_ended(&self, session_id: &str) {
        let programs: Vec<TabId> = self
            .registry()
            .tabs
            .iter()
            .filter(|(tab, entry)| {
                tab.session_id() == session_id && matches!(entry.spec, TabSpec::Program { .. })
            })
            .map(|(tab, _)| tab.clone())
            .collect();
        for tab in programs {
            if let Err(error) = self.end_tab(&tab, TabEndReason::SessionEnded).await {
                tracing::warn!(pty_key = %tab.key(), %error, "failed to end a program tab");
            }
        }
    }

    /// Sessions with a program in one of their tabs.
    pub fn program_owners(&self) -> Vec<String> {
        let mut owners: Vec<String> = self
            .registry()
            .tabs
            .iter()
            .filter(|(_, entry)| matches!(entry.spec, TabSpec::Program { .. }))
            .map(|(tab, _)| tab.session_id().to_string())
            .collect();
        owners.sort();
        owners.dedup();
        owners
    }

    /// Whether the tab runs its program, or is about to; after an app restart its tab is a
    /// plain shell.
    pub fn is_program_running(&self, tab: &TabId) -> bool {
        let phase =
            self.registry().tabs.get(tab).and_then(|entry| {
                matches!(entry.spec, TabSpec::Program { .. }).then_some(entry.phase)
            });
        match phase {
            Some(Phase::Reserved { .. } | Phase::Spawning { .. }) => true,
            Some(Phase::Live) => self.host.is_running(tab),
            Some(Phase::Closing { .. }) | None => false,
        }
    }

    /// The tabs among `tabs` that ended, so a saved layout still holding them drops them.
    pub async fn ended_among(&self, tabs: Vec<TabId>) -> Result<Vec<TabId>, String> {
        let (mut ended, unknown): (Vec<TabId>, Vec<TabId>) = {
            let registry = self.registry();
            tabs.into_iter()
                .filter(|tab| !registry.tabs.contains_key(tab))
                .partition(|tab| registry.ended.contains(tab))
        };
        let store = self.store.clone();
        let ended_earlier = off_thread(move || {
            let mut ended = Vec::new();
            for tab in unknown {
                if store.is_ended(&tab)? {
                    ended.push(tab);
                }
            }
            Ok(ended)
        })
        .await?;
        ended.extend(ended_earlier);
        Ok(ended)
    }

    /// End a tab's process whose end is already decided, releasing its terminal regardless.
    async fn end_process(&self, tab: &TabId) {
        if let Err(error) = self.host.end(tab).await {
            tracing::warn!(pty_key = %tab.key(), %error, "failed to end a tab's process");
            self.host.release(tab);
        }
    }

    /// Record and report a tab already marked ended in the registry.
    async fn report_end(&self, tab: &TabId, reason: TabEndReason, error: Option<String>) {
        self.notify();
        let (store, stored) = (self.store.clone(), tab.clone());
        if let Err(error) = off_thread(move || store.record_ended(&stored)).await {
            tracing::warn!(pty_key = %tab.key(), %error, "failed to record a tab's end");
        }
        (self.on_ended)(&TabEnded {
            pty_key: tab.key(),
            reason,
            error,
        });
    }
}

/// Run blocking store work off the async workers.
async fn off_thread<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| e.to_string())?
}

// ─── Persistence ─────────────────────────────────────────────────────────────

pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS terminal_tab_counters (
             session_id TEXT PRIMARY KEY,
             last_index INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS ended_terminal_tabs (
             session_id TEXT NOT NULL,
             tab_index INTEGER NOT NULL,
             ended_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
             PRIMARY KEY (session_id, tab_index)
         );",
    )
}

pub struct SqliteTabStore(pub Arc<Mutex<Connection>>);

impl SqliteTabStore {
    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl TabStore for SqliteTabStore {
    fn allocate(&self, session_id: &str) -> Result<u32, String> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let last: Option<u32> = tx
            .query_row(
                "SELECT last_index FROM terminal_tab_counters WHERE session_id = ?1",
                params![session_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let last = match last {
            Some(last) => last,
            None => highest_known_index(&tx, session_id).map_err(|e| e.to_string())?,
        };
        let next = last
            .checked_add(1)
            .ok_or("the session has no tab index left")?;
        tx.execute(
            "INSERT INTO terminal_tab_counters (session_id, last_index) VALUES (?1, ?2)
             ON CONFLICT(session_id) DO UPDATE SET last_index = excluded.last_index",
            params![session_id, next],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(next)
    }

    fn record_ended(&self, tab: &TabId) -> Result<(), String> {
        self.conn()
            .execute(
                "INSERT OR IGNORE INTO ended_terminal_tabs (session_id, tab_index) VALUES (?1, ?2)",
                params![tab.session_id(), tab.index()],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn is_ended(&self, tab: &TabId) -> Result<bool, String> {
        self.conn()
            .query_row(
                "SELECT 1 FROM ended_terminal_tabs WHERE session_id = ?1 AND tab_index = ?2",
                params![tab.session_id(), tab.index()],
                |_| Ok(()),
            )
            .optional()
            .map(|row| row.is_some())
            .map_err(|e| e.to_string())
    }
}

/// The highest index a session's tabs had before its counter existed: those in saved layouts,
/// in rmux panes, and among its ended tabs.
fn highest_known_index(conn: &Connection, session_id: &str) -> rusqlite::Result<u32> {
    let prefix = format!("{session_id}:");
    let mut statement = conn.prepare(
        "SELECT layout_json FROM session_layouts WHERE instr(layout_json, ?1) > 0
         UNION ALL SELECT layout_json FROM task_workspaces WHERE instr(layout_json, ?1) > 0
         UNION ALL SELECT pty_key FROM rmux_resources WHERE session_id = ?2",
    )?;
    let texts = statement.query_map(params![prefix, session_id], |row| row.get::<_, String>(0))?;
    let mut highest = 0;
    for text in texts {
        highest = highest.max(highest_index_in(&text?, session_id));
    }
    let ended: Option<u32> = conn.query_row(
        "SELECT MAX(tab_index) FROM ended_terminal_tabs WHERE session_id = ?1",
        params![session_id],
        |row| row.get(0),
    )?;
    Ok(highest.max(ended.unwrap_or(0)))
}

/// The highest tab index of `session_id` among the pty keys in `text`, whole or quoted.
fn highest_index_in(text: &str, session_id: &str) -> u32 {
    let prefix = format!("{session_id}:");
    let mut highest = 0;
    for (start, _) in text.match_indices(&prefix) {
        if start > 0 && text.as_bytes()[start - 1] != b'"' {
            continue;
        }
        let rest = &text[start + prefix.len()..];
        let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        let ends_key = matches!(rest.as_bytes().get(digits), None | Some(b'"'));
        if digits > 0 && ends_key {
            if let Ok(index) = rest[..digits].parse::<u32>() {
                highest = highest.max(index);
            }
        }
    }
    highest
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::sync::Notify;

    #[derive(Default)]
    struct MemoryStore {
        counters: Mutex<HashMap<String, u32>>,
        ended: Mutex<HashSet<TabId>>,
    }

    impl TabStore for MemoryStore {
        fn allocate(&self, session_id: &str) -> Result<u32, String> {
            let mut counters = self.counters.lock().unwrap();
            let next = counters.entry(session_id.to_string()).or_default();
            *next += 1;
            Ok(*next)
        }
        fn record_ended(&self, tab: &TabId) -> Result<(), String> {
            self.ended.lock().unwrap().insert(tab.clone());
            Ok(())
        }
        fn is_ended(&self, tab: &TabId) -> Result<bool, String> {
            Ok(self.ended.lock().unwrap().contains(tab))
        }
    }

    /// A host whose spawns and kills can be held open, to land other operations mid-flight.
    #[derive(Default)]
    struct FakeHost {
        running: Mutex<HashSet<TabId>>,
        spawned: Mutex<Vec<(TabId, TabSpec)>>,
        ended: Mutex<Vec<TabId>>,
        released: Mutex<Vec<TabId>>,
        reconnects: Mutex<Vec<bool>>,
        fail_attach: AtomicBool,
        fail_end: AtomicBool,
        hold_attach: Mutex<Option<Arc<Notify>>>,
        hold_end: Mutex<Option<Arc<Notify>>>,
    }

    impl TabHost for Arc<FakeHost> {
        type Output = ();

        async fn attach(
            &self,
            tab: &TabId,
            spec: &TabSpec,
            _output: (),
            reconnect: bool,
        ) -> Result<(), String> {
            self.reconnects.lock().unwrap().push(reconnect);
            let hold = self.hold_attach.lock().unwrap().clone();
            if let Some(hold) = hold {
                hold.notified().await;
            }
            if self.fail_attach.load(Ordering::SeqCst) {
                return Err("spawn failed".to_string());
            }
            self.spawned
                .lock()
                .unwrap()
                .push((tab.clone(), spec.clone()));
            self.running.lock().unwrap().insert(tab.clone());
            Ok(())
        }

        async fn end(&self, tab: &TabId) -> Result<(), String> {
            let hold = self.hold_end.lock().unwrap().clone();
            if let Some(hold) = hold {
                hold.notified().await;
            }
            if self.fail_end.load(Ordering::SeqCst) {
                return Err("kill failed".to_string());
            }
            self.running.lock().unwrap().remove(tab);
            self.ended.lock().unwrap().push(tab.clone());
            Ok(())
        }

        fn release(&self, tab: &TabId) {
            self.released.lock().unwrap().push(tab.clone());
        }

        fn is_running(&self, tab: &TabId) -> bool {
            self.running.lock().unwrap().contains(tab)
        }
    }

    struct Harness {
        tabs: Arc<TerminalTabs<Arc<FakeHost>>>,
        host: Arc<FakeHost>,
        store: Arc<MemoryStore>,
        events: Arc<Mutex<Vec<TabEnded>>>,
    }

    fn harness() -> Harness {
        harness_with(Arc::new(MemoryStore::default()))
    }

    fn harness_with(store: Arc<MemoryStore>) -> Harness {
        let host = Arc::new(FakeHost::default());
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        let tabs = Arc::new(TerminalTabs::new(
            host.clone(),
            store.clone(),
            move |ended: &TabEnded| sink.lock().unwrap().push(ended.clone()),
        ));
        Harness {
            tabs,
            host,
            store,
            events,
        }
    }

    impl Harness {
        fn events(&self) -> Vec<TabEnded> {
            self.events.lock().unwrap().clone()
        }

        fn hold_attach(&self) -> Arc<Notify> {
            let hold = Arc::new(Notify::new());
            *self.host.hold_attach.lock().unwrap() = Some(hold.clone());
            hold
        }

        fn hold_end(&self) -> Arc<Notify> {
            let hold = Arc::new(Notify::new());
            *self.host.hold_end.lock().unwrap() = Some(hold.clone());
            hold
        }

        fn release_holds(&self) {
            *self.host.hold_attach.lock().unwrap() = None;
            *self.host.hold_end.lock().unwrap() = None;
        }
    }

    fn ended(tab: &TabId, reason: TabEndReason) -> TabEnded {
        TabEnded {
            pty_key: tab.key(),
            reason,
            error: None,
        }
    }

    fn failed_to_start(tab: &TabId) -> TabEnded {
        TabEnded {
            error: Some("spawn failed".to_string()),
            ..ended(tab, TabEndReason::FailedToStart)
        }
    }

    fn program() -> TabSpec {
        TabSpec::Program {
            argv: vec!["claude".into(), "--resume".into()],
        }
    }

    /// Lets spawned operations run up to their next await.
    async fn settle() {
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
    }

    #[test]
    fn a_tab_id_round_trips_its_pty_key_and_rejects_other_keys() {
        let tab = TabId::parse("s1:12").unwrap();
        assert_eq!((tab.session_id(), tab.index()), ("s1", 12));
        assert_eq!(tab.key(), "s1:12");
        for key in [
            "s1",
            "s1:diff",
            "s1:editor:/a",
            "s1:0",
            "s1:01",
            "s1:+1",
            ":1",
            "s1:",
        ] {
            assert_eq!(TabId::parse(key), None, "{key}");
        }
    }

    #[tokio::test]
    async fn indices_are_never_handed_out_twice() {
        let h = harness();
        let first = h.tabs.open("s1", TabSpec::Shell).unwrap();
        h.tabs.close(&first).await.unwrap();
        let second = h.tabs.open("s1", TabSpec::Shell).unwrap();
        assert_eq!((first.index(), second.index()), (1, 2));
        assert_eq!(h.tabs.open("s2", TabSpec::Shell).unwrap().index(), 1);
    }

    #[tokio::test]
    async fn a_tab_runs_its_spec_once_attached() {
        let h = harness();
        let tab = h.tabs.open("s1", program()).unwrap();
        // About to run it: a webview reloading now still adopts the handoff.
        assert!(h.tabs.is_program_running(&tab));
        h.tabs.attach(&tab, ()).await.unwrap();
        assert_eq!(*h.host.spawned.lock().unwrap(), [(tab.clone(), program())]);
        assert!(h.tabs.is_program_running(&tab));
        assert_eq!(h.tabs.program_owners(), ["s1"]);
        h.host.running.lock().unwrap().clear();
        assert!(!h.tabs.is_program_running(&tab));
    }

    #[tokio::test]
    async fn a_live_tab_reattaches_with_its_spec() {
        let h = harness();
        let command = TabSpec::Command {
            command: "nvim a.rs".into(),
        };
        let tab = h.tabs.open("s1", command.clone()).unwrap();
        h.tabs.attach(&tab, ()).await.unwrap();
        h.tabs.attach(&tab, ()).await.unwrap();
        assert_eq!(
            *h.host.spawned.lock().unwrap(),
            [(tab.clone(), command.clone()), (tab.clone(), command)]
        );
        // So a host never starts a second process for it.
        assert_eq!(*h.host.reconnects.lock().unwrap(), [false, true]);
        assert!(h.events().is_empty());
    }

    #[tokio::test]
    async fn closing_a_reserved_tab_ends_it_without_touching_a_process() {
        let h = harness();
        let tab = h.tabs.open("s1", TabSpec::Shell).unwrap();
        h.tabs.close(&tab).await.unwrap();
        assert_eq!(h.events(), [ended(&tab, TabEndReason::Closed)]);
        assert!(h.host.ended.lock().unwrap().is_empty());
        assert_eq!(h.tabs.attach(&tab, ()).await, Ok(Attached::Ended));
        assert!(h.host.spawned.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn closing_a_live_tab_ends_its_process_and_reports_once() {
        let h = harness();
        let tab = h.tabs.open("s1", TabSpec::Shell).unwrap();
        h.tabs.attach(&tab, ()).await.unwrap();
        h.tabs.close(&tab).await.unwrap();
        // The exit its own close caused, and any later close, are no news.
        h.tabs.exited(&tab).await;
        h.tabs.close(&tab).await.unwrap();
        assert_eq!(h.events(), [ended(&tab, TabEndReason::Closed)]);
        assert_eq!(*h.host.ended.lock().unwrap(), std::slice::from_ref(&tab));
        assert_eq!(h.tabs.attach(&tab, ()).await, Ok(Attached::Ended));
    }

    #[tokio::test]
    async fn a_failed_close_leaves_the_tab_live_and_a_retry_ends_it() {
        let h = harness();
        let tab = h.tabs.open("s1", TabSpec::Shell).unwrap();
        h.tabs.attach(&tab, ()).await.unwrap();
        h.host.fail_end.store(true, Ordering::SeqCst);
        assert_eq!(h.tabs.close(&tab).await, Err("kill failed".to_string()));
        assert!(h.events().is_empty());
        h.host.fail_end.store(false, Ordering::SeqCst);
        h.tabs.close(&tab).await.unwrap();
        assert_eq!(h.events(), [ended(&tab, TabEndReason::Closed)]);
    }

    #[tokio::test]
    async fn a_tab_exiting_during_a_failed_close_ends_as_exited() {
        let h = harness();
        let tab = h.tabs.open("s1", TabSpec::Shell).unwrap();
        h.tabs.attach(&tab, ()).await.unwrap();
        h.host.fail_end.store(true, Ordering::SeqCst);
        let hold = h.hold_end();
        let close = tokio::spawn({
            let (tabs, tab) = (h.tabs.clone(), tab.clone());
            async move { tabs.close(&tab).await }
        });
        settle().await;
        h.tabs.exited(&tab).await;
        hold.notify_one();
        close.await.unwrap().unwrap();
        assert_eq!(h.events(), [ended(&tab, TabEndReason::Exited)]);
    }

    #[tokio::test]
    async fn a_tab_exiting_by_itself_ends_once() {
        let h = harness();
        let tab = h.tabs.open("s1", TabSpec::Shell).unwrap();
        h.tabs.attach(&tab, ()).await.unwrap();
        h.tabs.exited(&tab).await;
        h.tabs.exited(&tab).await;
        assert_eq!(h.events(), [ended(&tab, TabEndReason::Exited)]);
        // Its daemon or rmux pane is released too.
        assert_eq!(*h.host.ended.lock().unwrap(), [tab]);
    }

    #[tokio::test]
    async fn a_tab_failing_to_start_ends_and_is_cleaned_up() {
        let h = harness();
        let tab = h.tabs.open("s1", TabSpec::Shell).unwrap();
        h.host.fail_attach.store(true, Ordering::SeqCst);
        // Reported once, by its end, which carries why.
        h.tabs.attach(&tab, ()).await.unwrap();
        assert_eq!(h.events(), [failed_to_start(&tab)]);
        assert_eq!(*h.host.ended.lock().unwrap(), [tab]);
    }

    #[tokio::test]
    async fn a_tab_failing_to_start_whose_process_may_run_on_stays() {
        let h = harness();
        // Saved before an app restart, its daemon unreachable for now.
        let tab = TabId::new("s1", 4);
        h.host.fail_attach.store(true, Ordering::SeqCst);
        h.host.fail_end.store(true, Ordering::SeqCst);
        assert_eq!(
            h.tabs.attach(&tab, ()).await,
            Err("spawn failed".to_string())
        );
        assert!(h.events().is_empty());
        // Started again, it adopts the process a daemon may still run.
        h.host.fail_attach.store(false, Ordering::SeqCst);
        assert_eq!(h.tabs.attach(&tab, ()).await, Ok(Attached::Live));
        assert_eq!(*h.host.reconnects.lock().unwrap(), [false, false]);
        assert!(h.events().is_empty());
    }

    #[tokio::test]
    async fn closing_a_tab_whose_start_failed_unconfirmed_ends_its_process() {
        let h = harness();
        let tab = TabId::new("s1", 4);
        h.host.fail_attach.store(true, Ordering::SeqCst);
        h.host.fail_end.store(true, Ordering::SeqCst);
        assert!(h.tabs.attach(&tab, ()).await.is_err());
        h.host.fail_end.store(false, Ordering::SeqCst);
        h.tabs.close(&tab).await.unwrap();
        assert_eq!(h.events(), [ended(&tab, TabEndReason::Closed)]);
        assert_eq!(*h.host.ended.lock().unwrap(), [tab]);
    }

    #[tokio::test]
    async fn a_failed_reconnect_leaves_the_tab_live() {
        let h = harness();
        let tab = h.tabs.open("s1", TabSpec::Shell).unwrap();
        h.tabs.attach(&tab, ()).await.unwrap();
        h.host.fail_attach.store(true, Ordering::SeqCst);
        assert_eq!(
            h.tabs.attach(&tab, ()).await,
            Err("spawn failed".to_string())
        );
        assert!(h.events().is_empty());
        assert!(h.host.is_running(&tab));
        h.host.fail_attach.store(false, Ordering::SeqCst);
        h.tabs.attach(&tab, ()).await.unwrap();
    }

    #[tokio::test]
    async fn a_reconnect_finding_its_process_gone_ends_the_tab() {
        let h = harness();
        let tab = h.tabs.open("s1", program()).unwrap();
        h.tabs.attach(&tab, ()).await.unwrap();
        // It exited, and its exit is not reported yet.
        h.host.running.lock().unwrap().clear();
        h.host.fail_attach.store(true, Ordering::SeqCst);
        assert_eq!(h.tabs.attach(&tab, ()).await, Ok(Attached::Ended));
        assert_eq!(h.events(), [ended(&tab, TabEndReason::Exited)]);
        h.tabs.exited(&tab).await;
        assert_eq!(h.events().len(), 1);
    }

    #[tokio::test]
    async fn an_exited_tab_whose_process_cannot_be_ended_still_releases_its_terminal() {
        let h = harness();
        let tab = h.tabs.open("s1", TabSpec::Shell).unwrap();
        h.tabs.attach(&tab, ()).await.unwrap();
        h.host.fail_end.store(true, Ordering::SeqCst);
        h.tabs.exited(&tab).await;
        assert_eq!(h.events(), [ended(&tab, TabEndReason::Exited)]);
        assert_eq!(*h.host.released.lock().unwrap(), [tab]);
    }

    #[tokio::test]
    async fn a_tab_exiting_before_this_run_attached_it_ends() {
        let h = harness();
        let tab = TabId::new("s1", 4);
        h.tabs.exited(&tab).await;
        h.tabs.exited(&tab).await;
        assert_eq!(h.events(), [ended(&tab, TabEndReason::Exited)]);
        assert_eq!(h.tabs.attach(&tab, ()).await, Ok(Attached::Ended));
    }

    #[tokio::test]
    async fn a_program_exiting_while_it_spawns_ends_as_exited() {
        let h = harness();
        let tab = h.tabs.open("s1", program()).unwrap();
        let hold = h.hold_attach();
        let attach = tokio::spawn({
            let (tabs, tab) = (h.tabs.clone(), tab.clone());
            async move { tabs.attach(&tab, ()).await }
        });
        settle().await;
        h.tabs.exited(&tab).await;
        hold.notify_one();
        attach.await.unwrap().unwrap();
        assert_eq!(h.events(), [ended(&tab, TabEndReason::Exited)]);
        assert!(!h.tabs.is_program_running(&tab));
    }

    #[tokio::test]
    async fn a_close_during_a_spawn_waits_for_it_then_ends_what_it_started() {
        let h = harness();
        let tab = h.tabs.open("s1", program()).unwrap();
        let hold = h.hold_attach();
        let attach = tokio::spawn({
            let (tabs, tab) = (h.tabs.clone(), tab.clone());
            async move { tabs.attach(&tab, ()).await }
        });
        settle().await;
        let close = tokio::spawn({
            let (tabs, tab) = (h.tabs.clone(), tab.clone());
            async move { tabs.close(&tab).await }
        });
        settle().await;
        assert!(h.events().is_empty());
        h.release_holds();
        hold.notify_one();
        attach.await.unwrap().unwrap();
        close.await.unwrap().unwrap();
        assert_eq!(h.events(), [ended(&tab, TabEndReason::Closed)]);
        assert!(!h.host.is_running(&tab));
    }

    #[tokio::test]
    async fn a_close_during_a_failing_spawn_reports_the_failure_once() {
        let h = harness();
        let tab = h.tabs.open("s1", TabSpec::Shell).unwrap();
        h.host.fail_attach.store(true, Ordering::SeqCst);
        let hold = h.hold_attach();
        let attach = tokio::spawn({
            let (tabs, tab) = (h.tabs.clone(), tab.clone());
            async move { tabs.attach(&tab, ()).await }
        });
        settle().await;
        let close = tokio::spawn({
            let (tabs, tab) = (h.tabs.clone(), tab.clone());
            async move { tabs.close(&tab).await }
        });
        settle().await;
        h.release_holds();
        hold.notify_one();
        attach.await.unwrap().unwrap();
        close.await.unwrap().unwrap();
        assert_eq!(h.events(), [failed_to_start(&tab)]);
    }

    #[tokio::test]
    async fn concurrent_closes_end_the_process_once() {
        let h = harness();
        let tab = h.tabs.open("s1", TabSpec::Shell).unwrap();
        h.tabs.attach(&tab, ()).await.unwrap();
        let hold = h.hold_end();
        let closes: Vec<_> = (0..2)
            .map(|_| {
                let (tabs, tab) = (h.tabs.clone(), tab.clone());
                tokio::spawn(async move { tabs.close(&tab).await })
            })
            .collect();
        settle().await;
        h.release_holds();
        hold.notify_one();
        for close in closes {
            close.await.unwrap().unwrap();
        }
        assert_eq!(*h.host.ended.lock().unwrap(), std::slice::from_ref(&tab));
        assert_eq!(h.events(), [ended(&tab, TabEndReason::Closed)]);
    }

    #[tokio::test]
    async fn an_attach_during_a_close_is_refused_once_it_ends() {
        let h = harness();
        let tab = h.tabs.open("s1", TabSpec::Shell).unwrap();
        h.tabs.attach(&tab, ()).await.unwrap();
        let hold = h.hold_end();
        let close = tokio::spawn({
            let (tabs, tab) = (h.tabs.clone(), tab.clone());
            async move { tabs.close(&tab).await }
        });
        settle().await;
        let attach = tokio::spawn({
            let (tabs, tab) = (h.tabs.clone(), tab.clone());
            async move { tabs.attach(&tab, ()).await }
        });
        settle().await;
        hold.notify_one();
        close.await.unwrap().unwrap();
        assert_eq!(attach.await.unwrap(), Ok(Attached::Ended));
        assert_eq!(h.host.spawned.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_saved_tab_this_run_never_opened_starts_as_a_shell() {
        let h = harness();
        let tab = TabId::new("s1", 4);
        h.tabs.attach(&tab, ()).await.unwrap();
        assert_eq!(*h.host.spawned.lock().unwrap(), [(tab, TabSpec::Shell)]);
    }

    #[tokio::test]
    async fn closing_a_saved_tab_this_run_never_opened_ends_its_process() {
        let h = harness();
        let tab = TabId::new("s1", 4);
        h.tabs.close(&tab).await.unwrap();
        assert_eq!(*h.host.ended.lock().unwrap(), std::slice::from_ref(&tab));
        assert_eq!(h.events(), [ended(&tab, TabEndReason::Closed)]);
    }

    #[tokio::test]
    async fn a_tab_that_ended_in_an_earlier_run_stays_ended() {
        let store = Arc::new(MemoryStore::default());
        let tab = {
            let h = harness_with(store.clone());
            let tab = h.tabs.open("s1", TabSpec::Shell).unwrap();
            h.tabs.close(&tab).await.unwrap();
            tab
        };
        let h = harness_with(store);
        assert_eq!(h.tabs.attach(&tab, ()).await, Ok(Attached::Ended));
        let live = TabId::new("s1", 9);
        assert_eq!(
            h.tabs.ended_among(vec![tab.clone(), live]).await.unwrap(),
            std::slice::from_ref(&tab)
        );
        assert!(h.store.is_ended(&tab).unwrap());
    }

    #[tokio::test]
    async fn an_ended_session_ends_its_programs_but_not_its_shells() {
        let h = harness();
        let shell = h.tabs.open("s1", TabSpec::Shell).unwrap();
        let live = h.tabs.open("s1", program()).unwrap();
        let reserved = h.tabs.open("s1", program()).unwrap();
        let other = h.tabs.open("s2", program()).unwrap();
        for tab in [&shell, &live, &other] {
            h.tabs.attach(tab, ()).await.unwrap();
        }
        h.tabs.session_ended("s1").await;
        let mut events = h.events();
        events.sort_by(|a, b| a.pty_key.cmp(&b.pty_key));
        assert_eq!(
            events,
            [
                ended(&live, TabEndReason::SessionEnded),
                ended(&reserved, TabEndReason::SessionEnded)
            ]
        );
        assert!(h.host.is_running(&shell));
        assert!(h.tabs.is_program_running(&other));
        assert_eq!(h.tabs.program_owners(), ["s2"]);
    }

    #[tokio::test]
    async fn an_empty_program_is_refused() {
        let h = harness();
        assert!(h
            .tabs
            .open("s1", TabSpec::Program { argv: Vec::new() })
            .is_err());
    }

    fn sqlite_store() -> SqliteTabStore {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        planeai_tasks::sqlite::migrate(&conn).unwrap();
        SqliteTabStore(Arc::new(Mutex::new(conn)))
    }

    #[test]
    fn the_sqlite_store_counts_on_from_tabs_saved_before_it() {
        let store = sqlite_store();
        {
            let conn = store.conn();
            let project = crate::db::create_project(&conn, "app", "/tmp").unwrap();
            crate::db::create_session_with_id(
                &conn,
                "s1",
                &project.id,
                "chat",
                None,
                "main",
                None,
                None,
                "local",
                false,
                None,
                None,
                None,
            )
            .unwrap();
            conn.execute(
                "INSERT INTO session_layouts (session_id, layout_json, updated_at) VALUES ('s1', ?1, 'now')",
                params![r#"{"tabs":[{"ptyKey":"s1"},{"ptyKey":"s1:3"},{"ptyKey":"s11:9"},{"ptyKey":"s1:diff"}]}"#],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO ended_terminal_tabs (session_id, tab_index) VALUES ('s2', 5)",
                [],
            )
            .unwrap();
        }
        assert_eq!(store.allocate("s1").unwrap(), 4);
        assert_eq!(store.allocate("s1").unwrap(), 5);
        assert_eq!(store.allocate("s2").unwrap(), 6);
        assert_eq!(store.allocate("s3").unwrap(), 1);

        let tab = TabId::new("s1", 4);
        assert!(!store.is_ended(&tab).unwrap());
        store.record_ended(&tab).unwrap();
        store.record_ended(&tab).unwrap();
        assert!(store.is_ended(&tab).unwrap());
    }

    #[test]
    fn saved_indices_are_read_from_whole_pty_keys_only() {
        let text = r#"["s1:2","xs1:40","s1:7x","s1:editor:/a:9","s1:5"] s1:6"#;
        assert_eq!(highest_index_in(text, "s1"), 5);
        assert_eq!(highest_index_in("s1:6", "s1"), 6);
    }
}
