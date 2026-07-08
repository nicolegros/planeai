use planeai_core::prompt_lock::{self, LockError};
use rusqlite::Connection;

fn setup() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
        .unwrap();
    planeai_core::services::migrate_project_session_schema(&conn).unwrap();
    prompt_lock::migrate(&conn).unwrap();
    conn
}

#[test]
fn acquire_and_release() {
    let conn = setup();
    let lock = prompt_lock::acquire(&conn, "session-1").unwrap();
    assert_eq!(lock.session_id, "session-1");
    assert!(!lock.owner_id.is_empty());

    prompt_lock::release(&conn, &lock).unwrap();

    // Table should be empty after release
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM prompt_locks", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn duplicate_acquire_returns_busy() {
    let conn = setup();
    let lock = prompt_lock::acquire(&conn, "session-1").unwrap();

    let err = prompt_lock::acquire(&conn, "session-1").unwrap_err();
    match &err {
        LockError::Busy {
            session_id,
            owner_id,
            ..
        } => {
            assert_eq!(session_id, "session-1");
            assert_eq!(owner_id, &lock.owner_id);
        }
        other => panic!("expected Busy, got {:?}", other),
    }
    assert!(err.to_string().contains("already in progress"));
}

#[test]
fn stale_lock_recovery() {
    let conn = setup();

    // Insert a lock with a timestamp far in the past (stale)
    conn.execute(
        "INSERT INTO prompt_locks (session_id, owner_id, acquired_at) VALUES (?1, ?2, ?3)",
        rusqlite::params!["session-1", "old-owner", "2020-01-01T00:00:00+00:00"],
    )
    .unwrap();

    // acquire should clean the stale lock and succeed
    let lock = prompt_lock::acquire(&conn, "session-1").unwrap();
    assert_eq!(lock.session_id, "session-1");
    assert_ne!(lock.owner_id, "old-owner");
}

#[test]
fn release_allows_reacquire() {
    let conn = setup();

    let lock1 = prompt_lock::acquire(&conn, "session-1").unwrap();
    prompt_lock::release(&conn, &lock1).unwrap();

    let lock2 = prompt_lock::acquire(&conn, "session-1").unwrap();
    assert_eq!(lock2.session_id, "session-1");
    // Different owner_id each time
    assert_ne!(lock1.owner_id, lock2.owner_id);
}

#[test]
fn different_sessions_independent() {
    let conn = setup();

    let lock_a = prompt_lock::acquire(&conn, "session-a").unwrap();
    let lock_b = prompt_lock::acquire(&conn, "session-b").unwrap();

    assert_eq!(lock_a.session_id, "session-a");
    assert_eq!(lock_b.session_id, "session-b");

    // Release one does not affect the other
    prompt_lock::release(&conn, &lock_a).unwrap();

    let err = prompt_lock::acquire(&conn, "session-b").unwrap_err();
    assert!(matches!(err, LockError::Busy { .. }));

    // session-a can be reacquired
    let _lock_a2 = prompt_lock::acquire(&conn, "session-a").unwrap();
}

// ─── PromptLockGuard (RAII) ──────────────────────────────────────────────────

#[test]
fn guard_releases_on_drop() {
    let conn = setup();
    {
        let _guard = prompt_lock::acquire_guard(&conn, "session-1").unwrap();
        // Lock is held inside this scope
        let err = prompt_lock::acquire(&conn, "session-1").unwrap_err();
        assert!(matches!(err, LockError::Busy { .. }));
    }
    // Guard dropped — lock should be released
    let _lock = prompt_lock::acquire(&conn, "session-1").unwrap();
}

#[test]
fn guard_explicit_release_returns_ok() {
    let conn = setup();
    let guard = prompt_lock::acquire_guard(&conn, "session-1").unwrap();
    guard.release().unwrap();

    // Lock is released, can reacquire
    let _lock = prompt_lock::acquire(&conn, "session-1").unwrap();
}

#[test]
fn guard_releases_on_early_return() {
    let conn = setup();

    fn inner(conn: &Connection) -> Result<(), String> {
        let _guard = prompt_lock::acquire_guard(conn, "session-1").map_err(|e| e.to_string())?;
        // Simulate early return via ?
        Err("backend error".to_string())?;
        #[allow(unreachable_code)]
        Ok(())
    }

    let result = inner(&conn);
    assert!(result.is_err());

    // Lock should be released despite early return
    let _lock = prompt_lock::acquire(&conn, "session-1").unwrap();
}
