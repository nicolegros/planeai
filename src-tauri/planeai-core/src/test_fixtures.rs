//! Shared test fixtures for verifier and loop integration tests.
//!
//! Available under `#[cfg(test)]` or the `test-support` feature flag.

use crate::loop_run::LoopStrategy;
use crate::loop_service::{AddLoopSessionParams, CreateLoopParams, LoopService};
use crate::services::{CreateSessionParams, ProjectService, SessionService};
use rusqlite::Connection;

/// Create a project, a running loop, and a session enrolled in that loop.
/// Returns (loop_id, session_id).
pub fn setup_loop_with_session(
    conn: &Connection,
    project_path: &str,
    worktree_path: Option<&str>,
) -> (String, String) {
    use crate::loop_run::LoopStatus;

    let project = ProjectService::create(conn, "testapp", project_path).unwrap();

    let loop_run = LoopService::create_loop(
        conn,
        CreateLoopParams {
            project_id: project.id.clone(),
            task_key: None,
            created_by_session_id: None,
            strategy: LoopStrategy::new("maker-verifier"),
            goal: "Test verify".into(),
            max_rounds: 3,
            policy_json: None,
            budget_json: None,
        },
    )
    .unwrap();

    LoopService::update_loop_status(conn, &loop_run.id, LoopStatus::Running).unwrap();

    let session_id = uuid::Uuid::new_v4().to_string();

    SessionService::create(
        conn,
        &CreateSessionParams {
            id: session_id.clone(),
            project_id: project.id.clone(),
            name: "Maker".to_string(),
            branch: "main".to_string(),
            worktree_path: worktree_path.map(|s| s.to_string()),
            provider: Some("claude".to_string()),
            backend: "daemon".to_string(),
            auto_approve: true,
            ..Default::default()
        },
    )
    .unwrap();

    LoopService::add_loop_session(
        conn,
        AddLoopSessionParams {
            loop_id: loop_run.id.clone(),
            session_id: session_id.clone(),
            role: "maker".to_string(),
            round: 1,
            provider: Some("claude".to_string()),
            status: "running".to_string(),
        },
    )
    .unwrap();

    (loop_run.id, session_id)
}
