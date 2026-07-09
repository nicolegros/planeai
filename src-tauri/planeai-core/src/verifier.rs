//! Verifier gate execution — structured primitive for running verification
//! commands and persisting results to a loop.
//!
//! This module provides the reusable `run_verifier_gate()` operation that the
//! AXI CLI layer and the future recipe runtime both consume.

use crate::loop_service::{AddVerifierRunParams, LoopService};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ─── Constants ───────────────────────────────────────────────────────────────

/// Default timeout for verifier commands (10 minutes).
pub const DEFAULT_TIMEOUT_MS: u64 = 600_000;

/// Default maximum output bytes to capture in memory (10 MB).
pub const DEFAULT_MAX_OUTPUT_BYTES: usize = 10 * 1024 * 1024;

// ─── Types ───────────────────────────────────────────────────────────────────

/// Status of a completed verifier gate execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifierStatus {
    Pass,
    Fail,
    Error,
}

impl VerifierStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Error => "error",
        }
    }
}

/// Resource limits for verifier gate execution.
#[derive(Debug, Clone)]
pub struct VerifierLimits {
    /// Timeout in milliseconds. 0 means no timeout.
    pub timeout_ms: u64,
    /// Maximum output bytes to capture in memory.
    pub max_output_bytes: usize,
}

impl Default for VerifierLimits {
    fn default() -> Self {
        Self {
            timeout_ms: DEFAULT_TIMEOUT_MS,
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
        }
    }
}

/// Input request for running a verifier gate.
#[derive(Debug, Clone)]
pub struct VerifyGateRequest {
    pub loop_id: String,
    pub session_id: String,
    pub name: String,
    pub command: String,
    /// Project path (used for artifact root and CWD fallback).
    pub project_path: String,
    /// Session worktree path (preferred execution CWD).
    pub session_worktree_path: Option<String>,
    /// Resource limits (timeout, output cap).
    pub limits: VerifierLimits,
}

/// Successful result of a verifier gate execution.
#[derive(Debug, Clone)]
pub struct VerifyGateResult {
    pub verifier_run_id: String,
    pub loop_id: String,
    pub session_id: String,
    pub name: String,
    pub command: String,
    pub status: VerifierStatus,
    pub exit_code: Option<i32>,
    pub cwd: String,
    pub output_path: Option<String>,
    pub truncated: bool,
}

/// Errors from verifier gate execution.
#[derive(Debug)]
pub enum VerifyGateError {
    /// The resolved working directory does not exist.
    CwdUnavailable {
        reason: String,
        session_id: String,
        loop_id: String,
    },
    /// Database operation failed.
    Db(String),
    /// Failed to spawn or run the command.
    Execution(String),
}

impl std::fmt::Display for VerifyGateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CwdUnavailable { reason, .. } => {
                write!(f, "verifier working directory unavailable: {reason}")
            }
            Self::Db(e) => write!(f, "database error: {e}"),
            Self::Execution(e) => write!(f, "execution error: {e}"),
        }
    }
}

impl std::error::Error for VerifyGateError {}

// ─── CWD Resolution ──────────────────────────────────────────────────────────

/// Resolve the execution working directory for a verifier gate.
///
/// Rules (strict, no fallback to caller CWD):
/// 1. If session has worktree_path set → require it to exist → use it.
/// 2. Else → require project_path to exist → use it.
/// 3. Else → error.
fn resolve_cwd(request: &VerifyGateRequest) -> Result<String, VerifyGateError> {
    if let Some(ref wt) = request.session_worktree_path {
        if !wt.is_empty() {
            let path = Path::new(wt);
            if path.is_dir() {
                return Ok(wt.clone());
            }
            return Err(VerifyGateError::CwdUnavailable {
                reason: format!("session worktree_path does not exist: {wt}"),
                session_id: request.session_id.clone(),
                loop_id: request.loop_id.clone(),
            });
        }
    }

    let project = &request.project_path;
    if !project.is_empty() && Path::new(project).is_dir() {
        return Ok(project.clone());
    }

    Err(VerifyGateError::CwdUnavailable {
        reason: format!("project path does not exist: {project}"),
        session_id: request.session_id.clone(),
        loop_id: request.loop_id.clone(),
    })
}

// ─── Log Path ────────────────────────────────────────────────────────────────

/// Compute the artifact log path under the project root (survives worktree cleanup).
fn artifact_log_path(project_path: &str, loop_id: &str, run_id: &str) -> PathBuf {
    Path::new(project_path)
        .join(".planeai")
        .join("loops")
        .join(loop_id)
        .join("verifiers")
        .join(format!("{run_id}.log"))
}

/// Write verifier output to a log file. Creates parent directories as needed.
fn write_log(path: &Path, content: &[u8]) -> Result<String, String> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create dirs: {e}"))?;
    }
    let mut file = std::fs::File::create(path).map_err(|e| format!("create file: {e}"))?;
    file.write_all(content).map_err(|e| format!("write: {e}"))?;
    Ok(path.to_string_lossy().to_string())
}

// ─── Execution ───────────────────────────────────────────────────────────────

/// Run a verifier gate command and persist results.
///
/// This is the structured primitive that both AXI and the recipe runtime use.
///
/// # Blocking
///
/// This function blocks the calling thread for the duration of command execution
/// (up to `limits.timeout_ms`). It spawns child threads to drain pipes and uses
/// a polling loop with `thread::sleep`.
///
/// **Do not call from a Tauri IPC handler or any async context on the main thread.**
/// From the Tauri app, wrap in `commands::blocking(|| { ... }).await` or
/// `tokio::task::spawn_blocking`. From the CLI binary (no async runtime), direct
/// calls are safe.
pub fn run_verifier_gate(
    conn: &Connection,
    request: VerifyGateRequest,
) -> Result<VerifyGateResult, VerifyGateError> {
    // 1. Resolve CWD (strict — no fallback)
    let cwd = resolve_cwd(&request)?;

    // 2. Insert pending verifier_run
    let verifier_run = LoopService::add_verifier_run(
        conn,
        AddVerifierRunParams {
            loop_id: request.loop_id.clone(),
            session_id: Some(request.session_id.clone()),
            verifier_type: "command".to_string(),
            name: request.name.clone(),
            command: request.command.clone(),
        },
    )
    .map_err(|e| VerifyGateError::Db(format!("failed to create verifier run: {e}")))?;

    // 3. Transition to 'running' with started_at before execution
    LoopService::start_verifier_run(conn, &verifier_run.id)
        .map_err(|e| VerifyGateError::Db(format!("failed to start verifier run: {e}")))?;

    // 4. Run the command with timeout and output cap
    let (status, exit_code, output_bytes, truncated) = execute_command(
        &request.command,
        &cwd,
        request.limits.timeout_ms,
        request.limits.max_output_bytes,
    );

    // 5. Write log to project-level artifact root
    let log_path = artifact_log_path(&request.project_path, &request.loop_id, &verifier_run.id);
    let output_path = match write_log(&log_path, &output_bytes) {
        Ok(p) => Some(p),
        Err(e) => {
            tracing::warn!(error = %e, "failed to write verifier log");
            None
        }
    };

    // 6. Atomically complete: update verifier_run + append event
    let event_payload = serde_json::json!({
        "verifier_run_id": verifier_run.id,
        "name": request.name,
        "command": request.command,
        "status": status.as_str(),
        "exit_code": exit_code,
        "cwd": &cwd,
        "output_path": &output_path,
    });

    LoopService::complete_verifier_run(
        conn,
        &verifier_run.id,
        status.as_str(),
        exit_code,
        output_path.as_deref(),
        &event_payload,
    )
    .map_err(|e| VerifyGateError::Db(format!("failed to complete verifier run: {e}")))?;

    Ok(VerifyGateResult {
        verifier_run_id: verifier_run.id,
        loop_id: request.loop_id,
        session_id: request.session_id,
        name: request.name,
        command: request.command,
        status,
        exit_code,
        cwd,
        output_path,
        truncated,
    })
}

/// Execute a shell command with timeout and output cap.
///
/// Stdout and stderr are drained concurrently with the wait loop to prevent
/// pipe deadlocks when the child produces more output than the OS pipe buffer
/// (~64 KB). Reader threads consume up to `max_output_bytes` total.
///
/// Returns (status, exit_code, combined_output, was_truncated).
fn execute_command(
    command: &str,
    cwd: &str,
    timeout_ms: u64,
    max_output_bytes: usize,
) -> (VerifierStatus, Option<i32>, Vec<u8>, bool) {
    use std::process::{Command, Stdio};

    let shell = if cfg!(windows) { "cmd" } else { "sh" };
    let shell_flag = if cfg!(windows) { "/C" } else { "-c" };

    let mut cmd = Command::new(shell);
    cmd.arg(shell_flag)
        .arg(command)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::command::no_window(&mut cmd);

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let msg = format!("failed to execute command: {e}");
            return (VerifierStatus::Error, None, msg.into_bytes(), false);
        }
    };

    // Take pipes immediately and drain them in background threads to avoid
    // deadlock when output exceeds the OS pipe buffer.
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    let stdout_cap = max_output_bytes;
    let stderr_cap = max_output_bytes; // each stream gets up to max; we combine + trim later

    let stdout_handle = std::thread::spawn(move || drain_pipe(stdout_pipe, stdout_cap));
    let stderr_handle = std::thread::spawn(move || drain_pipe(stderr_pipe, stderr_cap));

    // Wait with timeout (pipes are being drained concurrently)
    let wait_result = if timeout_ms == 0 {
        child.wait().map(Some)
    } else {
        wait_with_timeout(&mut child, timeout_ms)
    };

    // Collect drained output
    let stdout_bytes = stdout_handle.join().unwrap_or_default();
    let stderr_bytes = stderr_handle.join().unwrap_or_default();

    match wait_result {
        Ok(Some(exit_status)) => {
            let code = exit_status.code().unwrap_or(-1);

            // Combine and enforce total cap
            let mut combined = stdout_bytes;
            if !stderr_bytes.is_empty() {
                if !combined.is_empty() {
                    combined.push(b'\n');
                }
                combined.extend_from_slice(&stderr_bytes);
            }

            let truncated = combined.len() >= max_output_bytes;
            if truncated {
                combined.truncate(max_output_bytes);
                combined.extend_from_slice(b"\n--- OUTPUT TRUNCATED ---\n");
            }

            let status = if code == 0 {
                VerifierStatus::Pass
            } else {
                VerifierStatus::Fail
            };
            (status, Some(code), combined, truncated)
        }
        Ok(None) => {
            // Timeout — kill the process
            let _ = child.kill();
            let _ = child.wait();
            let msg = format!("command timed out after {timeout_ms}ms");
            (VerifierStatus::Error, None, msg.into_bytes(), false)
        }
        Err(e) => {
            let msg = format!("failed to wait for command: {e}");
            (VerifierStatus::Error, None, msg.into_bytes(), false)
        }
    }
}

/// Drain a pipe into a Vec, reading up to `max_bytes`. Consumes the full stream
/// but discards bytes beyond the cap so the child doesn't block.
fn drain_pipe(pipe: Option<impl std::io::Read>, max_bytes: usize) -> Vec<u8> {
    let Some(mut reader) = pipe else {
        return Vec::new();
    };

    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                let remaining = max_bytes.saturating_sub(buf.len());
                if remaining > 0 {
                    let take = n.min(remaining);
                    buf.extend_from_slice(&chunk[..take]);
                }
                // Keep reading even past cap to prevent child from blocking
            }
            Err(_) => break,
        }
    }
    buf
}

/// Wait for a child process with a timeout. Returns Ok(None) on timeout.
fn wait_with_timeout(
    child: &mut std::process::Child,
    timeout_ms: u64,
) -> std::io::Result<Option<std::process::ExitStatus>> {
    use std::time::{Duration, Instant};

    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let poll_interval = Duration::from_millis(50);

    loop {
        match child.try_wait()? {
            Some(status) => return Ok(Some(status)),
            None => {
                if Instant::now() >= deadline {
                    return Ok(None);
                }
                std::thread::sleep(poll_interval);
            }
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loop_service::LoopService;
    use crate::services::open_db_at;

    fn test_db() -> Connection {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let conn = open_db_at(&path).unwrap();
        // Keep tempdir alive for the test by leaking — tests are short-lived
        std::mem::forget(dir);
        conn
    }

    fn setup_loop_and_session(
        conn: &Connection,
        project_path: &str,
        worktree_path: Option<&str>,
    ) -> (String, String) {
        crate::test_fixtures::setup_loop_with_session(conn, project_path, worktree_path)
    }

    #[test]
    fn successful_command_records_pass() {
        let conn = test_db();
        let dir = tempfile::tempdir().unwrap();
        let project_path = dir.path().to_string_lossy().to_string();
        let (loop_id, session_id) = setup_loop_and_session(&conn, &project_path, None);

        let result = run_verifier_gate(
            &conn,
            VerifyGateRequest {
                loop_id: loop_id.clone(),
                session_id,
                name: "echo-test".to_string(),
                command: "echo hello".to_string(),
                project_path: project_path.clone(),
                session_worktree_path: None,
                limits: VerifierLimits::default(),
            },
        )
        .unwrap();

        assert_eq!(result.status, VerifierStatus::Pass);
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(result.name, "echo-test");
        assert!(result.output_path.is_some());
        assert!(!result.truncated);
    }

    #[test]
    fn failing_command_records_fail_with_exit_code() {
        let conn = test_db();
        let dir = tempfile::tempdir().unwrap();
        let project_path = dir.path().to_string_lossy().to_string();
        let (loop_id, session_id) = setup_loop_and_session(&conn, &project_path, None);

        let result = run_verifier_gate(
            &conn,
            VerifyGateRequest {
                loop_id,
                session_id,
                name: "failing-test".to_string(),
                command: "exit 42".to_string(),
                project_path,
                session_worktree_path: None,
                limits: VerifierLimits::default(),
            },
        )
        .unwrap();

        assert_eq!(result.status, VerifierStatus::Fail);
        assert_eq!(result.exit_code, Some(42));
    }

    #[test]
    fn missing_worktree_path_returns_error() {
        let conn = test_db();
        let dir = tempfile::tempdir().unwrap();
        let project_path = dir.path().to_string_lossy().to_string();
        let (loop_id, session_id) = setup_loop_and_session(&conn, &project_path, None);

        let err = run_verifier_gate(
            &conn,
            VerifyGateRequest {
                loop_id,
                session_id,
                name: "test".to_string(),
                command: "echo hi".to_string(),
                project_path,
                session_worktree_path: Some("/nonexistent/worktree/path".to_string()),
                limits: VerifierLimits::default(),
            },
        )
        .unwrap_err();

        match err {
            VerifyGateError::CwdUnavailable { reason, .. } => {
                assert!(reason.contains("worktree_path does not exist"));
            }
            _ => panic!("expected CwdUnavailable, got: {err:?}"),
        }
    }

    #[test]
    fn missing_project_path_returns_error_when_no_worktree() {
        let conn = test_db();
        let dir = tempfile::tempdir().unwrap();
        let project_path = dir.path().to_string_lossy().to_string();
        let (loop_id, session_id) = setup_loop_and_session(&conn, &project_path, None);

        let err = run_verifier_gate(
            &conn,
            VerifyGateRequest {
                loop_id,
                session_id,
                name: "test".to_string(),
                command: "echo hi".to_string(),
                project_path: "/nonexistent/project/path".to_string(),
                session_worktree_path: None,
                limits: VerifierLimits::default(),
            },
        )
        .unwrap_err();

        match err {
            VerifyGateError::CwdUnavailable { reason, .. } => {
                assert!(reason.contains("project path does not exist"));
            }
            _ => panic!("expected CwdUnavailable, got: {err:?}"),
        }
    }

    #[test]
    fn no_fallback_to_caller_cwd() {
        // Even with both paths missing, we should get an error, not success
        let conn = test_db();
        let dir = tempfile::tempdir().unwrap();
        let project_path = dir.path().to_string_lossy().to_string();
        let (loop_id, session_id) = setup_loop_and_session(&conn, &project_path, None);

        let err = run_verifier_gate(
            &conn,
            VerifyGateRequest {
                loop_id,
                session_id,
                name: "test".to_string(),
                command: "echo hi".to_string(),
                project_path: "/does/not/exist".to_string(),
                session_worktree_path: Some("/also/does/not/exist".to_string()),
                limits: VerifierLimits::default(),
            },
        )
        .unwrap_err();

        assert!(matches!(err, VerifyGateError::CwdUnavailable { .. }));
    }

    #[test]
    fn output_log_written_to_project_artifact_root() {
        let conn = test_db();
        let dir = tempfile::tempdir().unwrap();
        let project_path = dir.path().to_string_lossy().to_string();
        let (loop_id, session_id) = setup_loop_and_session(&conn, &project_path, None);

        let result = run_verifier_gate(
            &conn,
            VerifyGateRequest {
                loop_id: loop_id.clone(),
                session_id,
                name: "log-test".to_string(),
                command: "echo 'test output'".to_string(),
                project_path: project_path.clone(),
                session_worktree_path: None,
                limits: VerifierLimits::default(),
            },
        )
        .unwrap();

        let log_path = result.output_path.unwrap();
        // Log should be under project path, not worktree
        assert!(log_path.starts_with(&project_path));
        assert!(log_path.contains(".planeai/loops/"));
        assert!(log_path.contains("/verifiers/"));

        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(content.contains("test output"));
    }

    #[test]
    fn event_appended_on_completion() {
        let conn = test_db();
        let dir = tempfile::tempdir().unwrap();
        let project_path = dir.path().to_string_lossy().to_string();
        let (loop_id, session_id) = setup_loop_and_session(&conn, &project_path, None);

        run_verifier_gate(
            &conn,
            VerifyGateRequest {
                loop_id: loop_id.clone(),
                session_id,
                name: "event-test".to_string(),
                command: "echo done".to_string(),
                project_path,
                session_worktree_path: None,
                limits: VerifierLimits::default(),
            },
        )
        .unwrap();

        let events = LoopService::list_loop_events(&conn, &loop_id).unwrap();
        let verifier_events: Vec<_> = events
            .iter()
            .filter(|e| e.kind == "verifier_completed")
            .collect();
        assert_eq!(verifier_events.len(), 1);
        let payload = &verifier_events[0].payload_json;
        assert_eq!(payload["name"].as_str().unwrap(), "event-test");
        assert_eq!(payload["status"].as_str().unwrap(), "pass");
        assert_eq!(payload["exit_code"].as_i64().unwrap(), 0);
        assert!(payload["cwd"].as_str().is_some());
        assert!(payload["output_path"].as_str().is_some());
    }

    #[test]
    fn cwd_prefers_worktree_path() {
        let conn = test_db();
        let project_dir = tempfile::tempdir().unwrap();
        let wt_dir = tempfile::tempdir().unwrap();
        let project_path = project_dir.path().to_string_lossy().to_string();
        let wt_path = wt_dir.path().to_string_lossy().to_string();
        let (loop_id, session_id) = setup_loop_and_session(&conn, &project_path, Some(&wt_path));

        // Write marker in worktree dir
        std::fs::write(wt_dir.path().join("marker.txt"), "worktree").unwrap();

        let cmd = if cfg!(windows) {
            "type marker.txt"
        } else {
            "cat marker.txt"
        };

        let result = run_verifier_gate(
            &conn,
            VerifyGateRequest {
                loop_id,
                session_id,
                name: "cwd-test".to_string(),
                command: cmd.to_string(),
                project_path: project_path.clone(),
                session_worktree_path: Some(wt_path.clone()),
                limits: VerifierLimits::default(),
            },
        )
        .unwrap();

        assert_eq!(result.status, VerifierStatus::Pass);
        assert_eq!(result.cwd, wt_path);

        // Log should still be under project path (artifact root)
        let log_path = result.output_path.unwrap();
        assert!(
            log_path.starts_with(&project_path),
            "log should be under project root: {log_path}"
        );
    }

    #[test]
    fn timeout_returns_error_status() {
        let conn = test_db();
        let dir = tempfile::tempdir().unwrap();
        let project_path = dir.path().to_string_lossy().to_string();
        let (loop_id, session_id) = setup_loop_and_session(&conn, &project_path, None);

        let result = run_verifier_gate(
            &conn,
            VerifyGateRequest {
                loop_id,
                session_id,
                name: "timeout-test".to_string(),
                command: "sleep 60".to_string(),
                project_path,
                session_worktree_path: None,
                limits: VerifierLimits {
                    timeout_ms: 200, // 200ms timeout
                    ..Default::default()
                },
            },
        )
        .unwrap();

        assert_eq!(result.status, VerifierStatus::Error);
        assert_eq!(result.exit_code, None);
        // Log should contain timeout message
        if let Some(ref path) = result.output_path {
            let content = std::fs::read_to_string(path).unwrap();
            assert!(content.contains("timed out"), "log: {content}");
        }
    }

    #[test]
    fn output_cap_truncates_large_output() {
        let conn = test_db();
        let dir = tempfile::tempdir().unwrap();
        let project_path = dir.path().to_string_lossy().to_string();
        let (loop_id, session_id) = setup_loop_and_session(&conn, &project_path, None);

        // Generate output larger than cap
        let cmd = if cfg!(windows) {
            // Windows: generate ~200 bytes
            "echo aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        } else {
            // Unix: generate output > 100 bytes
            "yes | head -c 200"
        };

        let result = run_verifier_gate(
            &conn,
            VerifyGateRequest {
                loop_id,
                session_id,
                name: "cap-test".to_string(),
                command: cmd.to_string(),
                project_path,
                session_worktree_path: None,
                limits: VerifierLimits {
                    max_output_bytes: 100, // 100 byte cap
                    ..Default::default()
                },
            },
        )
        .unwrap();

        assert!(result.truncated);
        if let Some(ref path) = result.output_path {
            let content = std::fs::read_to_string(path).unwrap();
            assert!(content.contains("TRUNCATED"));
        }
    }
}
