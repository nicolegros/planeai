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
}
