use serde::Deserialize;
use tauri::State;

use crate::db;
use crate::state::DbState;

use crate::commands::sessions::helpers::session_cwd;

/// Extract run ID from a GitHub Actions details URL.
/// e.g. "https://github.com/org/repo/actions/runs/12345/job/67890" → Some("12345")
fn extract_run_id(url: &str) -> Option<&str> {
    let marker = "/actions/runs/";
    let start = url.find(marker)? + marker.len();
    let rest = &url[start..];
    let end = rest.find('/').unwrap_or(rest.len());
    Some(&rest[..end])
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhCheckRun {
    conclusion: Option<String>,
    details_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhPrView {
    status_check_rollup: Vec<GhCheckRun>,
}

/// Fetch actual failure logs from the failed workflow run.
#[tauri::command]
pub async fn get_ci_failure_logs(
    session_id: String,
    db_state: State<'_, DbState>,
) -> Result<String, String> {
    let (cwd, branch) = {
        let conn = db_state.0.lock().map_err(|e| e.to_string())?;
        let session = db::get_session(&conn, &session_id)
            .map_err(|e| e.to_string())?
            .ok_or("session not found")?;
        let cwd = session_cwd(&conn, &session).ok_or("cannot resolve session working directory")?;
        (cwd, session.branch.clone())
    };

    tracing::info!(branch = %branch, "fetching CI failure logs");

    let mut cmd = tokio::process::Command::new("gh");
    cmd.args(["pr", "view", &branch, "--json", "statusCheckRollup"])
        .current_dir(&cwd);
    planeai_core::command::no_window_tokio(&mut cmd);
    let output = cmd
        .output()
        .await
        .map_err(|e| format!("failed to run gh: {e}"))?;

    if !output.status.success() {
        return Err("gh pr view failed".to_string());
    }

    let pr_view: GhPrView =
        serde_json::from_slice(&output.stdout).map_err(|e| format!("failed to parse: {e}"))?;

    let run_id = pr_view
        .status_check_rollup
        .iter()
        .filter(|c| {
            c.conclusion
                .as_deref()
                .is_some_and(|s| s.eq_ignore_ascii_case("failure"))
        })
        .find_map(|c| c.details_url.as_deref().and_then(extract_run_id));

    let Some(run_id) = run_id else {
        tracing::info!(branch = %branch, "no failed check with run ID found");
        return Err("no failed run found".to_string());
    };

    tracing::info!(run_id = %run_id, "found failed run, fetching logs");
    let mut log_cmd = tokio::process::Command::new("gh");
    log_cmd
        .args(["run", "view", run_id, "--log-failed"])
        .current_dir(&cwd);
    planeai_core::command::no_window_tokio(&mut log_cmd);
    let log_output = log_cmd
        .output()
        .await
        .map_err(|e| format!("gh run view failed: {e}"))?;

    if !log_output.status.success() {
        let stderr = String::from_utf8_lossy(&log_output.stderr);
        tracing::warn!(run_id = %run_id, stderr = %stderr, "gh run view --log-failed failed");
        return Err(format!("failed to fetch logs: {stderr}"));
    }

    let full = String::from_utf8_lossy(&log_output.stdout).to_string();
    tracing::info!(
        bytes = full.len(),
        lines = full.lines().count(),
        "got failure logs"
    );

    let truncated: String = full
        .lines()
        .rev()
        .take(200)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");

    Ok(format!(
        "CI failed on branch `{branch}`. Here are the failure logs:\n\n{truncated}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_run_id_from_actions_url() {
        assert_eq!(
            extract_run_id("https://github.com/org/repo/actions/runs/12345/job/67890"),
            Some("12345")
        );
    }

    #[test]
    fn extract_run_id_no_job() {
        assert_eq!(
            extract_run_id("https://github.com/org/repo/actions/runs/99999"),
            Some("99999")
        );
    }

    #[test]
    fn extract_run_id_not_actions_url() {
        assert_eq!(extract_run_id("https://github.com/org/repo/pull/1"), None);
    }
}
