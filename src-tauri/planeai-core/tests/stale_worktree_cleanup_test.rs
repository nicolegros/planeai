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
