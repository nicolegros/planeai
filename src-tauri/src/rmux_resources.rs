//! Persistence for the `pty_key` → rmux pane mapping.
//!
//! PlaneAI's `pty_key` is the canonical identity for a terminal resource; the
//! rmux pane id is a renewable handle valid only for a daemon lifetime
//! (ADR-0012). Storing the pair lets attach, resize, and cleanup find the right
//! pane without depending on window indices, which rmux recompresses.
//!
//! Rows are deliberately cheap to discard: if the daemon restarts, every pane id
//! is stale and the affected sessions are reconciled to `exited`.

use rusqlite::{params, Connection};

use planeai_rmux::{ResourceHandle, WorkspaceName};

/// A recorded resource: which workspace hosts it and which pane it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceRecord {
    pub pty_key: String,
    pub session_id: String,
    pub workspace_key: String,
    pub pane_id: u32,
}

impl ResourceRecord {
    pub fn handle(&self) -> ResourceHandle {
        ResourceHandle::from_u32(self.pane_id)
    }

    /// The workspace hosting this resource, if the stored name is still one of
    /// PlaneAI's own.
    pub fn workspace(&self) -> Option<WorkspaceName> {
        WorkspaceName::from_stored(&self.workspace_key)
    }
}

/// Create the mapping table. Safe to call repeatedly.
pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS rmux_resources (
             pty_key TEXT PRIMARY KEY,
             session_id TEXT NOT NULL,
             workspace_key TEXT NOT NULL,
             pane_id INTEGER NOT NULL,
             updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         );
         CREATE INDEX IF NOT EXISTS rmux_resources_session
             ON rmux_resources(session_id);",
    )
}

/// Record (or replace) the pane hosting a resource.
pub fn put(
    conn: &Connection,
    pty_key: &str,
    session_id: &str,
    workspace: &WorkspaceName,
    handle: ResourceHandle,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO rmux_resources (pty_key, session_id, workspace_key, pane_id, updated_at)
         VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         ON CONFLICT(pty_key) DO UPDATE SET
             session_id = excluded.session_id,
             workspace_key = excluded.workspace_key,
             pane_id = excluded.pane_id,
             updated_at = excluded.updated_at",
        params![pty_key, session_id, workspace.as_str(), handle.as_u32()],
    )?;
    Ok(())
}

/// Look up the pane recorded for a resource.
pub fn get(conn: &Connection, pty_key: &str) -> rusqlite::Result<Option<ResourceRecord>> {
    let mut statement = conn.prepare(
        "SELECT pty_key, session_id, workspace_key, pane_id
         FROM rmux_resources WHERE pty_key = ?1",
    )?;
    let mut rows = statement.query_map(params![pty_key], |row| {
        Ok(ResourceRecord {
            pty_key: row.get(0)?,
            session_id: row.get(1)?,
            workspace_key: row.get(2)?,
            pane_id: row.get::<_, i64>(3)? as u32,
        })
    })?;
    rows.next().transpose()
}

/// Every resource belonging to a session, agent pane and shell tabs alike.
pub fn list_for_session(
    conn: &Connection,
    session_id: &str,
) -> rusqlite::Result<Vec<ResourceRecord>> {
    let mut statement = conn.prepare(
        "SELECT pty_key, session_id, workspace_key, pane_id
         FROM rmux_resources WHERE session_id = ?1 ORDER BY pty_key",
    )?;
    let rows = statement.query_map(params![session_id], |row| {
        Ok(ResourceRecord {
            pty_key: row.get(0)?,
            session_id: row.get(1)?,
            workspace_key: row.get(2)?,
            pane_id: row.get::<_, i64>(3)? as u32,
        })
    })?;
    rows.collect()
}

/// Forget a single resource.
pub fn remove(conn: &Connection, pty_key: &str) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM rmux_resources WHERE pty_key = ?1",
        params![pty_key],
    )?;
    Ok(())
}

/// Forget every resource of a session.
pub fn remove_for_session(conn: &Connection, session_id: &str) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM rmux_resources WHERE session_id = ?1",
        params![session_id],
    )?;
    Ok(())
}

/// Forget every resource recorded in a workspace, whichever session owns it.
///
/// Task deletion removes a whole workspace, and its panes can belong to several
/// agent sessions, so the records cannot be found by session id alone.
// Only task deletion needs this, which the Tauri binary's copy of the module does
// not reach.
#[allow(dead_code)]
pub fn remove_for_workspace(
    conn: &Connection,
    workspace: &WorkspaceName,
) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM rmux_resources WHERE workspace_key = ?1",
        params![workspace.as_str()],
    )
}

/// Drop records whose pane is no longer on the daemon.
///
/// Returns the session ids that lost at least one resource, so the caller can
/// reconcile their status.
pub fn prune_missing(
    conn: &Connection,
    live_pane_ids: &std::collections::HashSet<u32>,
) -> rusqlite::Result<Vec<String>> {
    let mut statement =
        conn.prepare("SELECT pty_key, session_id, workspace_key, pane_id FROM rmux_resources")?;
    let records: Vec<ResourceRecord> = statement
        .query_map([], |row| {
            Ok(ResourceRecord {
                pty_key: row.get(0)?,
                session_id: row.get(1)?,
                workspace_key: row.get(2)?,
                pane_id: row.get::<_, i64>(3)? as u32,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(statement);

    let mut affected = Vec::new();
    for record in records {
        if live_pane_ids.contains(&record.pane_id) {
            continue;
        }
        remove(conn, &record.pty_key)?;
        if !affected.contains(&record.session_id) {
            affected.push(record.session_id);
        }
    }
    Ok(affected)
}

/// What an orphan sweep found: live panes to close and records to forget.
///
/// The two are reported separately because they are different leaks. A pane with
/// no record at all leaked from a launch that spawned it and then failed to
/// persist it; a record whose session row is gone leaked from a session deleted
/// while its pane ran. Only the first needs a pane closed, but both need the
/// mapping cleaned.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct OrphanSweep {
    /// Live panes with no owning session, in daemon report order.
    pub panes: Vec<planeai_rmux::LivePane>,
    /// `pty_key`s whose session row no longer exists.
    pub stale_records: Vec<String>,
}

impl OrphanSweep {
    pub fn is_empty(&self) -> bool {
        self.panes.is_empty() && self.stale_records.is_empty()
    }

    /// Keep only what a second, independent pass also judged orphaned.
    ///
    /// A pane is legitimately unaccounted for in the window between its spawn and
    /// the write that records it, so a single snapshot cannot distinguish a leak
    /// from a launch in progress. Reconciliation runs on a background thread — it
    /// must not stall app startup — which means a launch really can overlap it.
    /// Requiring two passes to agree makes the in-flight case resolve itself
    /// (the record lands, the pane stops looking orphaned) while a genuine leak,
    /// which is permanent, still gets collected.
    pub fn confirmed_by(&self, later: &OrphanSweep) -> OrphanSweep {
        let still_orphaned: std::collections::HashSet<u32> =
            later.panes.iter().map(|pane| pane.pane_id).collect();
        let still_stale: std::collections::HashSet<&str> =
            later.stale_records.iter().map(String::as_str).collect();
        OrphanSweep {
            panes: self
                .panes
                .iter()
                .filter(|pane| still_orphaned.contains(&pane.pane_id))
                .cloned()
                .collect(),
            stale_records: self
                .stale_records
                .iter()
                .filter(|pty_key| still_stale.contains(pty_key.as_str()))
                .cloned()
                .collect(),
        }
    }
}

/// Find live panes PlaneAI can no longer account for.
///
/// The inverse of [`prune_missing`]: that drops records whose pane died, this
/// finds panes that outlived their record. Both leaks open in the same window
/// between spawning a pane and committing the row that owns it.
///
/// `known_session_ids` is supplied by the caller rather than read here so this
/// stays a decision over two sets, testable without the sessions schema. Panes
/// are matched by `pane_id`: the daemon names windows after the `pty_key`, but
/// that name is cosmetic and best-effort, so it is not identity.
pub fn orphaned_panes(
    conn: &Connection,
    live: &[planeai_rmux::LivePane],
    known_session_ids: &std::collections::HashSet<String>,
) -> rusqlite::Result<OrphanSweep> {
    let mut statement =
        conn.prepare("SELECT pty_key, session_id, workspace_key, pane_id FROM rmux_resources")?;
    let records: Vec<ResourceRecord> = statement
        .query_map([], |row| {
            Ok(ResourceRecord {
                pty_key: row.get(0)?,
                session_id: row.get(1)?,
                workspace_key: row.get(2)?,
                pane_id: row.get::<_, i64>(3)? as u32,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(statement);

    let owned: std::collections::HashMap<u32, &ResourceRecord> = records
        .iter()
        .map(|record| (record.pane_id, record))
        .collect();

    let mut sweep = OrphanSweep::default();
    for pane in live {
        match owned.get(&pane.pane_id) {
            // Recorded and owned by a session that still exists: leave it alone.
            Some(record) if known_session_ids.contains(&record.session_id) => {}
            _ => sweep.panes.push(pane.clone()),
        }
    }
    for record in &records {
        if !known_session_ids.contains(&record.session_id) {
            sweep.stale_records.push(record.pty_key.clone());
        }
    }
    Ok(sweep)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        conn
    }

    fn workspace() -> WorkspaceName {
        planeai_rmux::WorkspaceKey::for_session("proj-1", Some("PLA-42"), "session-a").name()
    }

    #[test]
    fn migrate_is_idempotent() {
        let conn = setup();
        migrate(&conn).unwrap();
        assert!(get(&conn, "anything").unwrap().is_none());
    }

    #[test]
    fn a_recorded_resource_can_be_looked_up() {
        let conn = setup();
        put(
            &conn,
            "session-a",
            "session-a",
            &workspace(),
            ResourceHandle::from_u32(7),
        )
        .unwrap();

        let record = get(&conn, "session-a").unwrap().unwrap();
        assert_eq!(record.pane_id, 7);
        assert_eq!(record.session_id, "session-a");
        assert_eq!(record.workspace_key, workspace().as_str());
        assert_eq!(record.workspace(), Some(workspace()));
        assert_eq!(record.handle(), ResourceHandle::from_u32(7));
    }

    #[test]
    fn respawning_replaces_the_pane_rather_than_duplicating_it() {
        let conn = setup();
        put(
            &conn,
            "session-a",
            "session-a",
            &workspace(),
            ResourceHandle::from_u32(7),
        )
        .unwrap();
        put(
            &conn,
            "session-a",
            "session-a",
            &workspace(),
            ResourceHandle::from_u32(9),
        )
        .unwrap();

        assert_eq!(get(&conn, "session-a").unwrap().unwrap().pane_id, 9);
        assert_eq!(list_for_session(&conn, "session-a").unwrap().len(), 1);
    }

    #[test]
    fn shell_tabs_are_listed_with_their_agent_session() {
        let conn = setup();
        for (pty_key, pane) in [("session-a", 1u32), ("session-a:1", 2), ("session-a:2", 3)] {
            put(
                &conn,
                pty_key,
                "session-a",
                &workspace(),
                ResourceHandle::from_u32(pane),
            )
            .unwrap();
        }
        // A resource of a different session must not be included.
        put(
            &conn,
            "session-b",
            "session-b",
            &workspace(),
            ResourceHandle::from_u32(4),
        )
        .unwrap();

        let records = list_for_session(&conn, "session-a").unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(
            records.iter().map(|r| r.pane_id).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }

    #[test]
    fn removing_a_session_forgets_all_its_resources_but_no_others() {
        let conn = setup();
        put(
            &conn,
            "session-a",
            "session-a",
            &workspace(),
            ResourceHandle::from_u32(1),
        )
        .unwrap();
        put(
            &conn,
            "session-a:1",
            "session-a",
            &workspace(),
            ResourceHandle::from_u32(2),
        )
        .unwrap();
        put(
            &conn,
            "session-b",
            "session-b",
            &workspace(),
            ResourceHandle::from_u32(3),
        )
        .unwrap();

        remove_for_session(&conn, "session-a").unwrap();

        assert!(list_for_session(&conn, "session-a").unwrap().is_empty());
        assert_eq!(list_for_session(&conn, "session-b").unwrap().len(), 1);
    }

    #[test]
    fn pruning_drops_dead_panes_and_reports_their_sessions() {
        let conn = setup();
        put(
            &conn,
            "session-a",
            "session-a",
            &workspace(),
            ResourceHandle::from_u32(1),
        )
        .unwrap();
        put(
            &conn,
            "session-b",
            "session-b",
            &workspace(),
            ResourceHandle::from_u32(2),
        )
        .unwrap();

        // Only pane 1 is still on the daemon.
        let live = std::collections::HashSet::from([1u32]);
        let affected = prune_missing(&conn, &live).unwrap();

        assert_eq!(affected, vec!["session-b".to_string()]);
        assert!(get(&conn, "session-a").unwrap().is_some());
        assert!(get(&conn, "session-b").unwrap().is_none());
    }

    #[test]
    fn a_dead_daemon_prunes_everything_once() {
        let conn = setup();
        put(
            &conn,
            "session-a",
            "session-a",
            &workspace(),
            ResourceHandle::from_u32(1),
        )
        .unwrap();
        put(
            &conn,
            "session-a:1",
            "session-a",
            &workspace(),
            ResourceHandle::from_u32(2),
        )
        .unwrap();

        let affected = prune_missing(&conn, &std::collections::HashSet::new()).unwrap();

        // Both resources belong to one session, which must be reported once.
        assert_eq!(affected, vec!["session-a".to_string()]);
        assert!(list_for_session(&conn, "session-a").unwrap().is_empty());
    }

    #[test]
    fn pane_ids_beyond_i32_survive_the_round_trip() {
        let conn = setup();
        put(
            &conn,
            "session-a",
            "session-a",
            &workspace(),
            ResourceHandle::from_u32(u32::MAX),
        )
        .unwrap();

        assert_eq!(get(&conn, "session-a").unwrap().unwrap().pane_id, u32::MAX);
    }

    fn live(pane_id: u32) -> planeai_rmux::LivePane {
        planeai_rmux::LivePane {
            workspace_key: workspace().as_str().to_string(),
            pane_id,
        }
    }

    fn sessions(ids: &[&str]) -> std::collections::HashSet<String> {
        ids.iter().map(|id| (*id).to_string()).collect()
    }

    #[test]
    fn a_recorded_pane_of_a_live_session_is_not_an_orphan() {
        let conn = setup();
        put(
            &conn,
            "session-a",
            "session-a",
            &workspace(),
            ResourceHandle::from_u32(1),
        )
        .unwrap();

        let sweep = orphaned_panes(&conn, &[live(1)], &sessions(&["session-a"])).unwrap();

        assert!(sweep.is_empty(), "{sweep:?}");
    }

    #[test]
    fn a_live_pane_with_no_record_is_an_orphan() {
        let conn = setup();

        // This is what a launch that spawned a pane then failed to persist leaves.
        let sweep = orphaned_panes(&conn, &[live(7)], &sessions(&["session-a"])).unwrap();

        assert_eq!(sweep.panes, vec![live(7)]);
        assert!(sweep.stale_records.is_empty());
    }

    #[test]
    fn a_live_pane_whose_session_row_is_gone_is_an_orphan() {
        let conn = setup();
        put(
            &conn,
            "session-a",
            "session-a",
            &workspace(),
            ResourceHandle::from_u32(1),
        )
        .unwrap();

        // The session row was deleted while its pane kept running.
        let sweep = orphaned_panes(&conn, &[live(1)], &sessions(&[])).unwrap();

        assert_eq!(sweep.panes, vec![live(1)]);
        assert_eq!(sweep.stale_records, vec!["session-a".to_string()]);
    }

    #[test]
    fn a_stale_record_is_reported_even_when_its_pane_is_already_gone() {
        let conn = setup();
        put(
            &conn,
            "session-a",
            "session-a",
            &workspace(),
            ResourceHandle::from_u32(1),
        )
        .unwrap();

        // Nothing live to close, but the mapping still has to be forgotten.
        let sweep = orphaned_panes(&conn, &[], &sessions(&[])).unwrap();

        assert!(sweep.panes.is_empty());
        assert_eq!(sweep.stale_records, vec!["session-a".to_string()]);
    }

    #[test]
    fn shell_tabs_of_a_deleted_session_are_all_swept() {
        let conn = setup();
        for (pty_key, pane) in [("session-a", 1u32), ("session-a:1", 2), ("session-a:2", 3)] {
            put(
                &conn,
                pty_key,
                "session-a",
                &workspace(),
                ResourceHandle::from_u32(pane),
            )
            .unwrap();
        }
        put(
            &conn,
            "session-b",
            "session-b",
            &workspace(),
            ResourceHandle::from_u32(4),
        )
        .unwrap();

        let sweep = orphaned_panes(
            &conn,
            &[live(1), live(2), live(3), live(4)],
            &sessions(&["session-b"]),
        )
        .unwrap();

        // Every resource of the departed session, and none of the survivor's.
        assert_eq!(sweep.panes, vec![live(1), live(2), live(3)]);
        assert_eq!(
            sweep.stale_records,
            vec![
                "session-a".to_string(),
                "session-a:1".to_string(),
                "session-a:2".to_string()
            ]
        );
    }

    #[test]
    fn a_pane_claimed_between_passes_is_spared() {
        // The launch-in-flight case: the first pass saw a pane with no record, the
        // second sees it recorded. Closing it would kill a session the user just
        // started.
        let proposed = OrphanSweep {
            panes: vec![live(7), live(9)],
            stale_records: vec!["gone".to_string()],
        };
        let later = OrphanSweep {
            panes: vec![live(9)],
            stale_records: vec!["gone".to_string()],
        };

        let confirmed = proposed.confirmed_by(&later);

        assert_eq!(confirmed.panes, vec![live(9)]);
        assert_eq!(confirmed.stale_records, vec!["gone".to_string()]);
    }

    #[test]
    fn a_record_that_reappears_between_passes_is_not_forgotten() {
        let proposed = OrphanSweep {
            panes: Vec::new(),
            stale_records: vec!["claimed".to_string(), "gone".to_string()],
        };
        let later = OrphanSweep {
            panes: Vec::new(),
            stale_records: vec!["gone".to_string()],
        };

        let confirmed = proposed.confirmed_by(&later);

        assert_eq!(confirmed.stale_records, vec!["gone".to_string()]);
    }

    #[test]
    fn a_second_pass_finding_nothing_cancels_the_whole_sweep() {
        let proposed = OrphanSweep {
            panes: vec![live(7)],
            stale_records: vec!["gone".to_string()],
        };

        let confirmed = proposed.confirmed_by(&OrphanSweep::default());

        assert!(confirmed.is_empty(), "{confirmed:?}");
    }

    #[test]
    fn a_pane_orphaned_in_both_passes_is_confirmed() {
        // A genuine leak is permanent, so it survives confirmation.
        let sweep = OrphanSweep {
            panes: vec![live(7)],
            stale_records: vec!["gone".to_string()],
        };

        let confirmed = sweep.confirmed_by(&sweep.clone());

        assert_eq!(confirmed, sweep);
    }

    #[test]
    fn removing_a_workspace_forgets_every_session_inside_it() {
        let conn = setup();
        // A task workspace hosts several agent sessions, so its records cannot be
        // found by session id alone (ADR-0012: one workspace per TaskWorkspace).
        let task_workspace = planeai_rmux::WorkspaceKey::Task {
            project_id: "proj-1".to_string(),
            task_key: "PLA-42".to_string(),
        }
        .name();
        for (pty_key, session_id, pane) in [
            ("session-a", "session-a", 1u32),
            ("session-a:1", "session-a", 2),
            ("session-b", "session-b", 3),
        ] {
            put(
                &conn,
                pty_key,
                session_id,
                &task_workspace,
                ResourceHandle::from_u32(pane),
            )
            .unwrap();
        }
        // A different task's workspace must be untouched.
        let other = planeai_rmux::WorkspaceKey::Task {
            project_id: "proj-1".to_string(),
            task_key: "PLA-43".to_string(),
        }
        .name();
        put(
            &conn,
            "session-c",
            "session-c",
            &other,
            ResourceHandle::from_u32(4),
        )
        .unwrap();

        let forgotten = remove_for_workspace(&conn, &task_workspace).unwrap();

        assert_eq!(forgotten, 3);
        assert!(list_for_session(&conn, "session-a").unwrap().is_empty());
        assert!(list_for_session(&conn, "session-b").unwrap().is_empty());
        assert_eq!(list_for_session(&conn, "session-c").unwrap().len(), 1);
    }

    #[test]
    fn removing_an_absent_workspace_is_not_an_error() {
        // Teardown has to be repeatable: a crash between steps leaves the next
        // delete to finish the job (ADR-0012).
        let conn = setup();
        assert_eq!(remove_for_workspace(&conn, &workspace()).unwrap(), 0);
    }

    #[test]
    fn a_daemon_hosting_nothing_yields_no_orphans() {
        let conn = setup();
        put(
            &conn,
            "session-a",
            "session-a",
            &workspace(),
            ResourceHandle::from_u32(1),
        )
        .unwrap();

        let sweep = orphaned_panes(&conn, &[], &sessions(&["session-a"])).unwrap();

        assert!(sweep.is_empty(), "{sweep:?}");
    }

    /// Reproduces the leak that motivated the sweep, from a real database: eleven
    /// session-scoped workspaces whose session rows were gone while every pane was
    /// still running, alongside one task workspace that was legitimately active.
    #[test]
    fn a_real_leak_sweeps_every_departed_pane_and_spares_the_live_one() {
        let conn = setup();

        let departed = [
            ("1c886c9c-58d0-4611-b3d9-b9821677f2ca", 14u32),
            ("27db6b12-b995-4721-a7e8-28f35e6e95ab", 2),
            ("4b911d4c-c64e-4d25-84c1-6ce7e2089c17", 22),
            ("63a977fc-8fe4-437e-8bea-3a88c194af40", 23),
            ("6eb61b96-71be-47dc-92ab-739096244e70", 24),
            ("a1183600-93c7-4ac9-ae7a-8fcfc9dd63ce", 4),
            ("a709d553-14a8-4398-9f5a-65f7f1affee5", 3),
            ("c15d7de7-25e1-484c-8b7c-0b1ce81c50e0", 11),
            ("d0acc818-d15b-4e35-a5b1-4858157975c1", 6),
            ("e425b611-9a19-4b29-9e6d-3b230ddcd38c", 25),
            ("f1c641aa-143c-457a-a015-852a7289deb8", 19),
        ];
        let mut live = Vec::new();
        for (session_id, pane_id) in departed {
            let workspace =
                planeai_rmux::WorkspaceKey::for_session("proj", None, session_id).name();
            put(
                &conn,
                session_id,
                session_id,
                &workspace,
                ResourceHandle::from_u32(pane_id),
            )
            .unwrap();
            live.push(planeai_rmux::LivePane {
                workspace_key: workspace.as_str().to_string(),
                pane_id,
            });
        }

        // The one workspace that is still owned: a task workspace with an active session.
        let owned_workspace = planeai_rmux::WorkspaceKey::Task {
            project_id: "3fe1c738-9683-4f3b-8119-36fee19fbb0b".to_string(),
            task_key: "PLA-323".to_string(),
        }
        .name();
        put(
            &conn,
            "2dc10d52-f2a7-4978-a378-7eaf6b460912",
            "2dc10d52-f2a7-4978-a378-7eaf6b460912",
            &owned_workspace,
            ResourceHandle::from_u32(0),
        )
        .unwrap();
        live.push(planeai_rmux::LivePane {
            workspace_key: owned_workspace.as_str().to_string(),
            pane_id: 0,
        });

        let sweep = orphaned_panes(
            &conn,
            &live,
            &sessions(&["2dc10d52-f2a7-4978-a378-7eaf6b460912"]),
        )
        .unwrap();

        assert_eq!(sweep.panes.len(), 11);
        assert_eq!(sweep.stale_records.len(), 11);
        // Pane 0 is the live agent; closing it would kill a running session.
        assert!(
            !sweep.panes.iter().any(|pane| pane.pane_id == 0),
            "the active task workspace must be spared: {:?}",
            sweep.panes
        );
        // Every workspace the sweep reports must parse, or it cannot be closed.
        for pane in &sweep.panes {
            assert!(
                planeai_rmux::WorkspaceName::from_stored(&pane.workspace_key).is_some(),
                "unparseable workspace: {}",
                pane.workspace_key
            );
        }
    }
}
