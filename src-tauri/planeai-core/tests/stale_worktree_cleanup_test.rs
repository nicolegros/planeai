//! Tests for stale worktree cleanup on app launch.

use planeai_core::cleanup::cleanup_stale_worktrees;
use planeai_core::services::*;
use std::cell::RefCell;

fn test_db() -> rusqlite::Connection {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let conn = open_db_at(&path).unwrap();
    std::mem::forget(dir);
    conn
}

fn insert_session(
    conn: &rusqlite::Connection,
    id: &str,
    project_id: &str,
    status: &str,
    worktree_path: Option<&str>,
    updated_at: &str,
) {
    conn.execute(
        "INSERT INTO sessions (id, project_id, name, branch, status, created_at, worktree_path, updated_at)
         VALUES (?1, ?2, '', 'main', ?3, ?4, ?5, ?6)",
        rusqlite::params![id, project_id, status, updated_at, worktree_path, updated_at],
    ).unwrap();
}

#[test]
fn exited_session_older_than_48h_gets_worktree_removed() {
    let conn = test_db();
    let project = ProjectService::ensure_project(&conn, "/tmp/myapp").unwrap();

    // Session exited 3 days ago
    let three_days_ago = (chrono::Utc::now() - chrono::Duration::hours(72)).to_rfc3339();
    insert_session(
        &conn,
        "sess-1",
        &project.id,
        "exited",
        Some("/tmp/wt/abc"),
        &three_days_ago,
    );

    thread_local! {
        static REMOVED: RefCell<Vec<(String, String)>> = const { RefCell::new(vec![]) };
    }

    let errors = cleanup_stale_worktrees(&conn, |project_path, wt_path| {
        REMOVED.with(|r| {
            r.borrow_mut()
                .push((project_path.to_string(), wt_path.to_string()))
        });
        Ok(())
    });

    assert!(errors.is_empty());
    REMOVED.with(|r| {
        assert_eq!(
            r.borrow().as_slice(),
            &[("/tmp/myapp".to_string(), "/tmp/wt/abc".to_string())]
        );
    });
}

#[test]
fn exited_session_less_than_48h_is_skipped() {
    let conn = test_db();
    let project = ProjectService::ensure_project(&conn, "/tmp/myapp").unwrap();

    // Session exited 12 hours ago — too recent
    let twelve_hours_ago = (chrono::Utc::now() - chrono::Duration::hours(12)).to_rfc3339();
    insert_session(
        &conn,
        "sess-1",
        &project.id,
        "exited",
        Some("/tmp/wt/abc"),
        &twelve_hours_ago,
    );

    let errors = cleanup_stale_worktrees(&conn, |_, _| {
        panic!("should not be called for recent sessions");
    });

    assert!(errors.is_empty());
}

#[test]
fn active_sessions_are_skipped() {
    let conn = test_db();
    let project = ProjectService::ensure_project(&conn, "/tmp/myapp").unwrap();

    let old = (chrono::Utc::now() - chrono::Duration::hours(72)).to_rfc3339();
    insert_session(
        &conn,
        "sess-active",
        &project.id,
        "active",
        Some("/tmp/wt/a"),
        &old,
    );

    let errors = cleanup_stale_worktrees(&conn, |_, _| {
        panic!("should not be called for active sessions");
    });

    assert!(errors.is_empty());
}

#[test]
fn archived_session_older_than_48h_gets_worktree_removed() {
    let conn = test_db();
    let project = ProjectService::ensure_project(&conn, "/tmp/myapp").unwrap();

    let old = (chrono::Utc::now() - chrono::Duration::hours(72)).to_rfc3339();
    insert_session(
        &conn,
        "sess-archived",
        &project.id,
        "archived",
        Some("/tmp/wt/b"),
        &old,
    );

    thread_local! {
        static REMOVED: RefCell<Vec<(String, String)>> = const { RefCell::new(vec![]) };
    }

    let errors = cleanup_stale_worktrees(&conn, |project_path, wt_path| {
        REMOVED.with(|r| {
            r.borrow_mut()
                .push((project_path.to_string(), wt_path.to_string()))
        });
        Ok(())
    });

    assert!(errors.is_empty());
    REMOVED.with(|r| {
        assert_eq!(
            r.borrow().as_slice(),
            &[("/tmp/myapp".to_string(), "/tmp/wt/b".to_string())]
        );
    });
}

#[test]
fn successful_cleanup_nulls_worktree_path() {
    let conn = test_db();
    let project = ProjectService::ensure_project(&conn, "/tmp/myapp").unwrap();

    let old = (chrono::Utc::now() - chrono::Duration::hours(72)).to_rfc3339();
    insert_session(
        &conn,
        "sess-1",
        &project.id,
        "exited",
        Some("/tmp/wt/abc"),
        &old,
    );

    let errors = cleanup_stale_worktrees(&conn, |_, _| Ok(()));
    assert!(errors.is_empty());

    // worktree_path should now be NULL
    let wt: Option<String> = conn
        .query_row(
            "SELECT worktree_path FROM sessions WHERE id = 'sess-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(wt.is_none());
}

#[test]
fn failed_cleanup_preserves_worktree_path() {
    let conn = test_db();
    let project = ProjectService::ensure_project(&conn, "/tmp/myapp").unwrap();

    let old = (chrono::Utc::now() - chrono::Duration::hours(72)).to_rfc3339();
    insert_session(
        &conn,
        "sess-1",
        &project.id,
        "exited",
        Some("/tmp/wt/abc"),
        &old,
    );

    let errors = cleanup_stale_worktrees(&conn, |_, _| Err("disk error".to_string()));
    assert_eq!(errors.len(), 1);

    // worktree_path should still be set
    let wt: Option<String> = conn
        .query_row(
            "SELECT worktree_path FROM sessions WHERE id = 'sess-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(wt.as_deref(), Some("/tmp/wt/abc"));
}

#[test]
fn null_worktree_path_sessions_are_skipped() {
    let conn = test_db();
    let project = ProjectService::ensure_project(&conn, "/tmp/myapp").unwrap();

    let old = (chrono::Utc::now() - chrono::Duration::hours(72)).to_rfc3339();
    insert_session(&conn, "sess-1", &project.id, "exited", None, &old);

    let errors = cleanup_stale_worktrees(&conn, |_, _| {
        panic!("should not be called for sessions without worktree_path");
    });

    assert!(errors.is_empty());
}

#[test]
fn nonexistent_directory_no_error_when_closure_succeeds() {
    let conn = test_db();
    let project = ProjectService::ensure_project(&conn, "/tmp/myapp").unwrap();

    let old = (chrono::Utc::now() - chrono::Duration::hours(72)).to_rfc3339();
    // Path doesn't exist on disk — closure handles it gracefully
    insert_session(
        &conn,
        "sess-1",
        &project.id,
        "exited",
        Some("/nonexistent/wt/xyz"),
        &old,
    );

    let errors = cleanup_stale_worktrees(&conn, |_project_path, _wt_path| {
        // Production would check exists() and skip — returning Ok
        Ok(())
    });

    assert!(errors.is_empty());
}

#[test]
fn failed_removal_collects_error_and_continues() {
    let conn = test_db();
    let project = ProjectService::ensure_project(&conn, "/tmp/myapp").unwrap();

    let old = (chrono::Utc::now() - chrono::Duration::hours(72)).to_rfc3339();
    insert_session(
        &conn,
        "sess-1",
        &project.id,
        "exited",
        Some("/tmp/wt/a"),
        &old,
    );
    insert_session(
        &conn,
        "sess-2",
        &project.id,
        "exited",
        Some("/tmp/wt/b"),
        &old,
    );

    thread_local! {
        static CALLED: RefCell<Vec<String>> = const { RefCell::new(vec![]) };
    }

    let errors = cleanup_stale_worktrees(&conn, |_, wt_path| {
        CALLED.with(|c| c.borrow_mut().push(wt_path.to_string()));
        if wt_path == "/tmp/wt/a" {
            Err("permission denied".to_string())
        } else {
            Ok(())
        }
    });

    // First session failed, second was still processed
    CALLED.with(|c| assert_eq!(c.borrow().len(), 2));
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("permission denied"));
}

#[test]
fn list_stale_worktrees_returns_matching_sessions() {
    use planeai_core::cleanup::list_stale_worktrees;

    let conn = test_db();
    let project = ProjectService::ensure_project(&conn, "/tmp/myapp").unwrap();

    let old = (chrono::Utc::now() - chrono::Duration::hours(72)).to_rfc3339();
    let recent = (chrono::Utc::now() - chrono::Duration::hours(12)).to_rfc3339();

    // Should be listed (old + exited + has worktree)
    insert_session(
        &conn,
        "sess-1",
        &project.id,
        "exited",
        Some("/tmp/wt/a"),
        &old,
    );
    // Should NOT be listed (too recent)
    insert_session(
        &conn,
        "sess-2",
        &project.id,
        "exited",
        Some("/tmp/wt/b"),
        &recent,
    );
    // Should NOT be listed (active)
    insert_session(
        &conn,
        "sess-3",
        &project.id,
        "active",
        Some("/tmp/wt/c"),
        &old,
    );

    let result = list_stale_worktrees(&conn).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].session_name, "");
    assert_eq!(result[0].worktree_path, "/tmp/wt/a");
    assert_eq!(result[0].branch, "main");
}

#[test]
fn run_stale_worktree_cleanup_removes_entries_from_list() {
    use planeai_core::cleanup::{list_stale_worktrees, run_stale_worktree_cleanup};

    let conn = test_db();
    let project = ProjectService::ensure_project(&conn, "/tmp/myapp").unwrap();

    let old = (chrono::Utc::now() - chrono::Duration::hours(72)).to_rfc3339();
    insert_session(
        &conn,
        "sess-1",
        &project.id,
        "exited",
        Some("/tmp/wt/a"),
        &old,
    );

    // Before cleanup: one stale worktree
    assert_eq!(list_stale_worktrees(&conn).unwrap().len(), 1);

    // Run cleanup (noop remover — just marks as cleaned)
    let errors = run_stale_worktree_cleanup(&conn, |_, _| Ok(()));
    assert!(errors.is_empty());

    // After cleanup: list is empty
    assert_eq!(list_stale_worktrees(&conn).unwrap().len(), 0);
}

#[test]
fn stale_check_uses_status_changed_at_not_updated_at() {
    use planeai_core::cleanup::list_stale_worktrees;

    let conn = test_db();
    let project = ProjectService::ensure_project(&conn, "/tmp/myapp").unwrap();

    let old = (chrono::Utc::now() - chrono::Duration::hours(72)).to_rfc3339();
    let recent = (chrono::Utc::now() - chrono::Duration::hours(12)).to_rfc3339();

    // Session with old status_changed_at but recent updated_at (simulates migration bump)
    conn.execute(
        "INSERT INTO sessions (id, project_id, name, branch, status, created_at, worktree_path, updated_at, status_changed_at)
         VALUES (?1, ?2, '', 'main', 'destroyed', ?3, '/tmp/wt/a', ?4, ?5)",
        rusqlite::params!["sess-1", project.id, &old, &recent, &old],
    ).unwrap();

    // Should be listed because status_changed_at is old, even though updated_at is recent
    let result = list_stale_worktrees(&conn).unwrap();
    assert_eq!(result.len(), 1);
}
