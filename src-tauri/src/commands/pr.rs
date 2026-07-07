use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};

use crate::config;
use crate::db;
use crate::pr;
use crate::state::{ConfigState, DbState};

use crate::commands::sessions::helpers::{resolve_task_manager, session_cwd};

/// Common session context needed by PR commands.
pub(super) struct SessionContext {
    pub(super) cwd: String,
    pub(super) branch: String,
    pub(super) pr_url: Option<String>,
    pub(super) task_key: Option<String>,
    pub(super) base_branch: Option<String>,
    pub(super) name: String,
    pub(super) project_id: String,
}

/// Resolve session from DB, returning the fields PR commands need.
/// Locks and releases the DB mutex immediately.
pub(super) fn resolve_session_context(
    db_state: &State<'_, DbState>,
    session_id: &str,
) -> Result<SessionContext, String> {
    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    let session = db::get_session(&conn, session_id)
        .map_err(|e| e.to_string())?
        .ok_or("session not found")?;
    let cwd = session_cwd(&conn, &session).ok_or("cannot resolve session working directory")?;
    Ok(SessionContext {
        cwd,
        branch: session.branch.clone(),
        pr_url: session.pr_url.clone(),
        task_key: session.task_key.clone(),
        base_branch: session.base_branch.clone(),
        name: session.name.clone(),
        project_id: session.project_id.clone(),
    })
}

/// Check PR status for a single session and handle transitions.
pub(crate) fn poll_pr_for_session(
    conn: &rusqlite::Connection,
    cfg: &config::Config,
    session: &db::Session,
) -> Result<bool, String> {
    let pr_cmd = match &cfg.pr_status {
        Some(cmd) => cmd,
        None => return Ok(false),
    };
    if !pr::is_poll_eligible(&session.status, session.pr_state.as_deref()) {
        return Ok(false);
    }
    let cwd = match session_cwd(conn, session) {
        Some(c) => c,
        None => return Ok(false),
    };
    let status = match pr::check_pr_status(pr_cmd, &session.branch, std::path::Path::new(&cwd))? {
        Some(s) => s,
        None => return Ok(false),
    };
    let transition = pr::detect_transition(session.pr_state.as_deref(), &status);
    let _ = db::update_pr_state(conn, &session.id, &status.url, &status.state);
    if let Some(ref t) = transition {
        if let Some(ref task_key) = session.task_key {
            if let Ok(tm) = resolve_task_manager(cfg) {
                let prefix = db::get_project_prefix(conn, &session.project_id);
                pr::fire_pr_hook(tm, t, task_key, &prefix);
            }
        }
    }
    Ok(transition.is_some())
}

fn new_pr_url(cwd: &str, branch: &str, base_branch: Option<&str>) -> Result<String, String> {
    let mut cmd = std::process::Command::new("git");
    cmd.args(["remote", "get-url", "origin"]).current_dir(cwd);
    planeai_core::command::no_window(&mut cmd);
    let output = cmd
        .output()
        .map_err(|e| format!("failed to get remote URL: {e}"))?;
    if !output.status.success() {
        return Err("no origin remote configured".to_string());
    }
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let repo_path = parse_github_repo(&raw).ok_or("could not parse GitHub repo from remote URL")?;
    let compare = match base_branch {
        Some(base) => format!("{base}...{branch}"),
        None => branch.to_string(),
    };
    Ok(format!(
        "https://github.com/{repo_path}/compare/{compare}?expand=1"
    ))
}

fn parse_github_repo(url: &str) -> Option<String> {
    let path = url
        .strip_prefix("git@github.com:")
        .or_else(|| url.strip_prefix("https://github.com/"))?;
    Some(path.trim_end_matches(".git").to_string())
}

/// Resolve the GitHub "owner/repo" from the origin remote in the given directory.
pub(super) async fn resolve_github_repo(cwd: &str) -> Result<Option<String>, String> {
    if !std::path::Path::new(cwd).exists() {
        tracing::debug!(cwd = %cwd, "cwd does not exist, skipping remote resolution");
        return Ok(None);
    }
    let mut cmd = tokio::process::Command::new(crate::command::resolve("git"));
    cmd.args(["remote", "get-url", "origin"]).current_dir(cwd);
    planeai_core::command::no_window_tokio(&mut cmd);
    let output = match cmd.output().await {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!(cwd = %cwd, error = %e, "failed to spawn git for remote resolution");
            return Ok(None);
        }
    };
    if !output.status.success() {
        tracing::debug!(cwd = %cwd, "git remote get-url origin failed, not a git repo or no origin");
        return Ok(None);
    }
    let remote_url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(parse_github_repo(&remote_url))
}

fn fetch_pr_url_inner(
    conn: &rusqlite::Connection,
    cfg: &config::Config,
    session_id: &str,
) -> Result<Option<String>, String> {
    let pr_cmd = cfg.pr_status.as_ref().ok_or("pr_status not configured")?;
    let session = db::get_session(conn, session_id)
        .map_err(|e| e.to_string())?
        .ok_or("session not found")?;
    let cwd = session_cwd(conn, &session).ok_or("cannot resolve session working directory")?;
    let status = pr::check_pr_status(pr_cmd, &session.branch, std::path::Path::new(&cwd))?;
    match status {
        Some(s) => {
            let _ = db::update_pr_state(conn, session_id, &s.url, &s.state);
            Ok(Some(s.url))
        }
        None => {
            let create_url = new_pr_url(&cwd, &session.branch, session.base_branch.as_deref())?;
            Ok(Some(create_url))
        }
    }
}

#[tauri::command]
pub async fn fetch_pr_url(
    session_id: String,
    db_state: State<'_, DbState>,
    config_state: State<'_, ConfigState>,
) -> Result<Option<String>, String> {
    let db = db_state.0.clone();
    let cfg = config_state.0.lock().map_err(|e| e.to_string())?.clone();
    super::blocking(move || {
        let conn = db.lock().map_err(|e| e.to_string())?;
        fetch_pr_url_inner(&conn, &cfg, &session_id)
    })
    .await
}

#[derive(Debug, Clone, Serialize)]
pub struct PrDefaults {
    pub title: String,
    pub body: String,
    pub base_branch: String,
}

#[tauri::command]
pub async fn generate_pr_defaults(
    session_id: String,
    db_state: State<'_, DbState>,
) -> Result<PrDefaults, String> {
    let ctx = resolve_session_context(&db_state, &session_id)?;

    let title = match &ctx.task_key {
        Some(key) => {
            let prefix = format!("{}: ", key);
            let name = ctx.name.strip_prefix(&prefix).unwrap_or(&ctx.name);
            format!("{} [{}]", name, key)
        }
        None => ctx.name,
    };

    let base_ref = ctx.base_branch.as_deref().unwrap_or("main");

    // Diff stats for body
    let diff_output = {
        let mut cmd = tokio::process::Command::new("git");
        cmd.args(["diff", "--stat", &format!("{}...HEAD", base_ref)])
            .current_dir(&ctx.cwd);
        planeai_core::command::no_window_tokio(&mut cmd);
        cmd.output()
            .await
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    };

    let body = if diff_output.is_empty() {
        String::new()
    } else {
        format!("## Changes\n\n```\n{diff_output}\n```")
    };

    let base_branch = match ctx.base_branch {
        Some(b) => b,
        None => detect_default_branch_async(&ctx.cwd).await,
    };

    Ok(PrDefaults {
        title,
        body,
        base_branch,
    })
}

async fn detect_default_branch_async(cwd: &str) -> String {
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(["symbolic-ref", "refs/remotes/origin/HEAD", "--short"])
        .current_dir(cwd);
    planeai_core::command::no_window_tokio(&mut cmd);
    cmd.output()
        .await
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            s.strip_prefix("origin/").map(|b| b.to_string())
        })
        .unwrap_or_else(|| "main".to_string())
}

#[tauri::command]
pub async fn create_pr(
    session_id: String,
    title: String,
    body: String,
    base_branch: String,
    draft: bool,
    db_state: State<'_, DbState>,
    _config_state: State<'_, ConfigState>,
) -> Result<String, String> {
    let ctx = resolve_session_context(&db_state, &session_id)?;

    // Push branch
    let mut push_cmd = tokio::process::Command::new("git");
    push_cmd
        .args(["push", "-u", "origin", &ctx.branch])
        .current_dir(&ctx.cwd);
    planeai_core::command::no_window_tokio(&mut push_cmd);
    let push = push_cmd
        .output()
        .await
        .map_err(|e| format!("failed to run git push: {e}"))?;
    if !push.status.success() {
        let stderr = String::from_utf8_lossy(&push.stderr);
        return Err(format!("git push failed: {stderr}"));
    }

    // Create PR
    let mut args = vec![
        "pr".to_string(),
        "create".to_string(),
        "--title".to_string(),
        title,
        "--body".to_string(),
        body,
        "--base".to_string(),
        base_branch,
    ];
    if draft {
        args.push("--draft".to_string());
    }
    let mut pr_cmd = tokio::process::Command::new(crate::command::resolve("gh"));
    pr_cmd.args(&args).current_dir(&ctx.cwd);
    planeai_core::command::no_window_tokio(&mut pr_cmd);
    let pr_output = pr_cmd
        .output()
        .await
        .map_err(|e| format!("failed to run gh: {e}"))?;
    if !pr_output.status.success() {
        let stderr = String::from_utf8_lossy(&pr_output.stderr).to_string();
        if stderr.contains("not logged") || stderr.contains("auth login") {
            return Err("Run `gh auth login` to authenticate".to_string());
        }
        return Err(format!("gh pr create failed: {stderr}"));
    }

    let url = String::from_utf8_lossy(&pr_output.stdout)
        .trim()
        .to_string();

    // Store in DB
    {
        let conn = db_state.0.lock().map_err(|e| e.to_string())?;
        let _ = db::update_pr_state(&conn, &session_id, &url, "open");
    }

    Ok(url)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiCheck {
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhCheckRun {
    name: String,
    status: String,
    conclusion: Option<String>,
    details_url: Option<String>,
    workflow_name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhPrView {
    status_check_rollup: Vec<GhCheckRun>,
    mergeable: Option<String>,
    merge_state_status: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct PrStatus {
    pub checks: Vec<CiCheck>,
    pub conflicting: bool,
}

#[tauri::command]
pub async fn get_merge_conflict_status(
    session_id: String,
    db_state: State<'_, DbState>,
) -> Result<bool, String> {
    let status = get_pr_status_inner(&db_state, &session_id).await?;
    Ok(status.conflicting)
}

#[tauri::command]
pub async fn get_pr_status(
    session_id: String,
    db_state: State<'_, DbState>,
) -> Result<PrStatus, String> {
    get_pr_status_inner(&db_state, &session_id).await
}

async fn get_pr_status_inner(
    db_state: &State<'_, DbState>,
    session_id: &str,
) -> Result<PrStatus, String> {
    let ctx = resolve_session_context(db_state, session_id)?;
    tracing::debug!(session_id = %session_id, branch = %ctx.branch, cwd = %ctx.cwd, "get_pr_status called");
    if !std::path::Path::new(&ctx.cwd).exists() {
        tracing::warn!(session_id = %session_id, cwd = %ctx.cwd, "cwd does not exist, skipping pr status");
        return Ok(PrStatus {
            checks: vec![],
            conflicting: false,
        });
    }
    if resolve_github_repo(&ctx.cwd).await?.is_none() {
        tracing::debug!("not a GitHub remote, skipping pr status");
        return Ok(PrStatus {
            checks: vec![],
            conflicting: false,
        });
    }

    let mut cmd = tokio::process::Command::new(crate::command::resolve("gh"));
    cmd.args([
        "pr",
        "view",
        &ctx.branch,
        "--json",
        "statusCheckRollup,mergeable,mergeStateStatus",
    ])
    .current_dir(&ctx.cwd);
    planeai_core::command::no_window_tokio(&mut cmd);
    let output = cmd
        .output()
        .await
        .map_err(|e| format!("failed to run gh: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::warn!(stderr = %stderr, "gh pr view failed");
        return Ok(PrStatus {
            checks: vec![],
            conflicting: false,
        });
    }

    let pr_view: GhPrView = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("failed to parse pr view: {e}"))?;

    let conflicting = pr_view.mergeable.as_deref() == Some("CONFLICTING")
        || pr_view.merge_state_status.as_deref() == Some("DIRTY");

    let checks: Vec<CiCheck> = pr_view
        .status_check_rollup
        .into_iter()
        .map(|c| CiCheck {
            name: c.workflow_name.unwrap_or(c.name),
            status: c.status.to_lowercase(),
            conclusion: c.conclusion.map(|s| s.to_lowercase()),
            url: c.details_url,
        })
        .collect();

    tracing::debug!(session_id = %session_id, check_count = checks.len(), conflicting = %conflicting, "pr status fetched");
    Ok(PrStatus {
        checks,
        conflicting,
    })
}

#[tauri::command]
pub async fn get_ci_checks(
    session_id: String,
    db_state: State<'_, DbState>,
) -> Result<Vec<CiCheck>, String> {
    let status = get_pr_status_inner(&db_state, &session_id).await?;
    Ok(status.checks)
}

#[tauri::command]
pub async fn get_allowed_merge_strategies(
    session_id: String,
    db_state: State<'_, DbState>,
) -> Result<Vec<String>, String> {
    let ctx = resolve_session_context(&db_state, &session_id)?;

    let repo = resolve_github_repo(&ctx.cwd)
        .await?
        .ok_or("not a GitHub repo")?;

    let mut cmd = tokio::process::Command::new(crate::command::resolve("gh"));
    cmd.args([
        "api",
        &format!("repos/{repo}"),
        "--jq",
        "{squash: .allow_squash_merge, merge: .allow_merge_commit, rebase: .allow_rebase_merge}",
    ])
    .current_dir(&ctx.cwd);
    planeai_core::command::no_window_tokio(&mut cmd);
    let output = cmd
        .output()
        .await
        .map_err(|e| format!("failed to run gh: {e}"))?;

    if !output.status.success() {
        return Err("failed to fetch repo merge settings".to_string());
    }

    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    parse_merge_settings(&raw)
}

#[derive(Deserialize)]
struct RepoMergeSettings {
    squash: bool,
    merge: bool,
    rebase: bool,
}

fn parse_merge_settings(raw: &str) -> Result<Vec<String>, String> {
    let settings: RepoMergeSettings =
        serde_json::from_str(raw).map_err(|e| format!("failed to parse merge settings: {e}"))?;
    let mut strategies = Vec::new();
    if settings.squash {
        strategies.push("squash".to_string());
    }
    if settings.merge {
        strategies.push("merge".to_string());
    }
    if settings.rebase {
        strategies.push("rebase".to_string());
    }
    Ok(strategies)
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MergeStrategy {
    Squash,
    Merge,
    Rebase,
}

impl MergeStrategy {
    fn as_gh_flag(&self) -> &'static str {
        match self {
            Self::Squash => "--squash",
            Self::Merge => "--merge",
            Self::Rebase => "--rebase",
        }
    }
}

#[tauri::command]
pub async fn merge_pr(
    session_id: String,
    strategy: MergeStrategy,
    app: tauri::AppHandle,
    db_state: State<'_, DbState>,
    config_state: State<'_, ConfigState>,
) -> Result<(), String> {
    let ctx = resolve_session_context(&db_state, &session_id)?;

    let mut cmd = tokio::process::Command::new(crate::command::resolve("gh"));
    cmd.args([
        "pr",
        "merge",
        &ctx.branch,
        strategy.as_gh_flag(),
        "--delete-branch",
    ])
    .current_dir(&ctx.cwd);
    planeai_core::command::no_window_tokio(&mut cmd);
    let output = cmd
        .output()
        .await
        .map_err(|e| format!("failed to run gh: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(format!("merge failed: {stderr}"));
    }

    // Update DB state
    {
        let conn = db_state.0.lock().map_err(|e| e.to_string())?;
        let pr_url = ctx.pr_url.as_deref().unwrap_or_default();
        let _ = db::update_pr_state(&conn, &session_id, pr_url, "merged");
    }

    // Fire task hook
    {
        let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
        if let Some(ref key) = ctx.task_key {
            if let Ok(tm) = resolve_task_manager(&cfg) {
                let prefix = {
                    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
                    db::get_project_prefix(&conn, &ctx.project_id)
                };
                pr::fire_pr_hook(tm, &pr::PrTransition::Merged, key, &prefix);
            }
        }
    }

    // Emit pr-merged event
    let _ = app.emit("pr-merged", serde_json::json!({ "session_id": session_id }));
    // Also refresh sessions so UI picks up new pr_state
    let _ = app.emit("sessions-changed", ());

    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeStateInfo {
    pub blocked: bool,
    pub reasons: Vec<String>,
    pub settings_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct GhMergeStateView {
    merge_state_status: Option<String>,
    review_decision: Option<String>,
    status_check_rollup: Option<Vec<GhCheckRun>>,
}

fn parse_merge_block_reasons(view: &GhMergeStateView) -> Vec<String> {
    let mut reasons = Vec::new();
    if let Some(ref decision) = view.review_decision {
        match decision.as_str() {
            "CHANGES_REQUESTED" => reasons.push("Changes requested by reviewer".to_string()),
            "REVIEW_REQUIRED" => reasons.push("Required reviews not met".to_string()),
            _ => {}
        }
    }
    if let Some(ref checks) = view.status_check_rollup {
        let failing = checks
            .iter()
            .filter(|c| {
                c.conclusion.as_deref() == Some("FAILURE")
                    || c.conclusion.as_deref() == Some("failure")
            })
            .count();
        if failing > 0 {
            reasons.push(format!("Required checks failing ({failing})"));
        }
    }
    if let Some(ref state) = view.merge_state_status {
        if state == "BLOCKED" && reasons.is_empty() {
            reasons.push("Linear history required or other branch protection rule".to_string());
        }
    }
    reasons
}

#[tauri::command]
pub async fn get_merge_state(
    session_id: String,
    db_state: State<'_, DbState>,
) -> Result<MergeStateInfo, String> {
    let ctx = resolve_session_context(&db_state, &session_id)?;

    let repo = resolve_github_repo(&ctx.cwd)
        .await?
        .ok_or("not a GitHub repo")?;

    let mut cmd = tokio::process::Command::new(crate::command::resolve("gh"));
    cmd.args([
        "pr",
        "view",
        &ctx.branch,
        "--json",
        "mergeStateStatus,reviewDecision,statusCheckRollup",
    ])
    .current_dir(&ctx.cwd);
    planeai_core::command::no_window_tokio(&mut cmd);
    let output = cmd
        .output()
        .await
        .map_err(|e| format!("failed to run gh: {e}"))?;

    if !output.status.success() {
        return Ok(MergeStateInfo {
            blocked: false,
            reasons: vec![],
            settings_url: None,
        });
    }

    let view: GhMergeStateView = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("failed to parse merge state: {e}"))?;

    let blocked = view.merge_state_status.as_deref() == Some("BLOCKED");
    let reasons = if blocked {
        parse_merge_block_reasons(&view)
    } else {
        vec![]
    };
    let settings_url = if blocked {
        Some(format!("https://github.com/{repo}/settings/rules"))
    } else {
        None
    };

    Ok(MergeStateInfo {
        blocked,
        reasons,
        settings_url,
    })
}

#[tauri::command]
pub async fn mark_pr_ready(
    session_id: String,
    app: tauri::AppHandle,
    db_state: State<'_, DbState>,
) -> Result<(), String> {
    let ctx = resolve_session_context(&db_state, &session_id)?;

    let mut cmd = tokio::process::Command::new(crate::command::resolve("gh"));
    cmd.args(["pr", "ready", &ctx.branch]).current_dir(&ctx.cwd);
    planeai_core::command::no_window_tokio(&mut cmd);
    let output = cmd
        .output()
        .await
        .map_err(|e| format!("failed to run gh: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(format!("failed to mark PR ready: {stderr}"));
    }

    // Update DB state
    {
        let conn = db_state.0.lock().map_err(|e| e.to_string())?;
        let pr_url = ctx.pr_url.as_deref().unwrap_or_default();
        let _ = db::update_pr_state(&conn, &session_id, pr_url, "open");
    }

    let _ = app.emit("sessions-changed", ());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_github_repo_ssh() {
        assert_eq!(
            parse_github_repo("git@github.com:org/repo.git"),
            Some("org/repo".to_string())
        );
    }

    #[test]
    fn parse_github_repo_https() {
        assert_eq!(
            parse_github_repo("https://github.com/org/repo.git"),
            Some("org/repo".to_string())
        );
    }

    #[test]
    fn parse_github_repo_non_github() {
        assert_eq!(parse_github_repo("git@gitlab.com:org/repo.git"), None);
    }

    #[test]
    #[cfg(unix)]
    fn fetch_pr_url_returns_url_when_pr_exists() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("pr.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\necho '{\"url\":\"https://github.com/org/repo/pull/1\",\"state\":\"open\"}'",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let conn = rusqlite::Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        let project = db::create_project(&conn, "test", dir.path().to_str().unwrap()).unwrap();
        db::create_session_with_id(
            &conn,
            "s1",
            &project.id,
            "sess",
            None,
            "feat/x",
            None,
            None,
            "daemon",
            true,
            None,
            None,
            None,
        )
        .unwrap();

        let cfg = config::Config {
            pr_status: Some(format!("{} {{branch}}", script.display())),
            ..Default::default()
        };

        let result = fetch_pr_url_inner(&conn, &cfg, "s1");
        assert_eq!(
            result.unwrap(),
            Some("https://github.com/org/repo/pull/1".to_string())
        );
    }

    #[test]
    #[cfg(unix)]
    fn fetch_pr_url_returns_create_url_when_no_pr() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("pr.sh");
        std::fs::write(&script, "#!/bin/sh\nexit 1").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        std::process::Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["remote", "add", "origin", "git@github.com:org/repo.git"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let conn = rusqlite::Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        let project = db::create_project(&conn, "test", dir.path().to_str().unwrap()).unwrap();
        db::create_session_with_id(
            &conn,
            "s1",
            &project.id,
            "sess",
            None,
            "feat/x",
            None,
            None,
            "daemon",
            true,
            None,
            Some("main"),
            None,
        )
        .unwrap();

        let cfg = config::Config {
            pr_status: Some(format!("{}", script.display())),
            ..Default::default()
        };

        let result = fetch_pr_url_inner(&conn, &cfg, "s1");
        assert_eq!(
            result.unwrap(),
            Some("https://github.com/org/repo/compare/main...feat/x?expand=1".to_string())
        );
    }

    #[test]
    fn fetch_pr_url_errors_when_not_configured() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        let project = db::create_project(&conn, "test", "/tmp").unwrap();
        db::create_session_with_id(
            &conn,
            "s1",
            &project.id,
            "sess",
            None,
            "feat/x",
            None,
            None,
            "daemon",
            true,
            None,
            None,
            None,
        )
        .unwrap();

        let cfg = config::Config::default();
        let result = fetch_pr_url_inner(&conn, &cfg, "s1");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not configured"));
    }

    #[tokio::test]
    async fn detect_default_branch_fallback() {
        let dir = tempfile::tempdir().unwrap();
        // No git repo — should fallback to "main"
        assert_eq!(
            detect_default_branch_async(dir.path().to_str().unwrap()).await,
            "main"
        );
    }

    #[test]
    #[cfg(unix)]
    fn generate_pr_defaults_uses_session_name_and_base_branch() {
        let dir = tempfile::tempdir().unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let conn = rusqlite::Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        let project = db::create_project(&conn, "test", dir.path().to_str().unwrap()).unwrap();
        db::create_session_with_id(
            &conn,
            "s1",
            &project.id,
            "add login page",
            None,
            "feat/login",
            None,
            None,
            "daemon",
            true,
            None,
            Some("develop"),
            None,
        )
        .unwrap();

        let session = db::get_session(&conn, "s1").unwrap().unwrap();
        let cwd = session_cwd(&conn, &session).expect("should resolve to project path");

        assert_eq!(session.name, "add login page");
        assert_eq!(session.base_branch, Some("develop".to_string()));
        assert_eq!(cwd, dir.path().to_str().unwrap());
    }

    #[test]
    fn pr_defaults_struct_serializes() {
        let defaults = PrDefaults {
            title: "feat: add login".into(),
            body: "## Changes\n\nstuff".into(),
            base_branch: "main".into(),
        };
        let json = serde_json::to_string(&defaults).unwrap();
        assert!(json.contains("feat: add login"));
        assert!(json.contains("base_branch"));
    }

    #[test]
    fn parse_merge_strategies_all_enabled() {
        let result = parse_merge_settings(r#"{"squash":true,"merge":true,"rebase":true}"#).unwrap();
        assert_eq!(result, vec!["squash", "merge", "rebase"]);
    }

    #[test]
    fn parse_merge_strategies_only_squash() {
        let result =
            parse_merge_settings(r#"{"squash":true,"merge":false,"rebase":false}"#).unwrap();
        assert_eq!(result, vec!["squash"]);
    }

    #[test]
    fn parse_merge_strategies_partial() {
        let result =
            parse_merge_settings(r#"{"squash":true,"merge":false,"rebase":true}"#).unwrap();
        assert_eq!(result, vec!["squash", "rebase"]);
    }

    #[test]
    fn parse_merge_strategies_invalid_json() {
        assert!(parse_merge_settings("not json").is_err());
    }

    #[test]
    fn merge_strategy_deserializes() {
        let s: MergeStrategy = serde_json::from_str("\"squash\"").unwrap();
        assert_eq!(s.as_gh_flag(), "--squash");
        let m: MergeStrategy = serde_json::from_str("\"merge\"").unwrap();
        assert_eq!(m.as_gh_flag(), "--merge");
        let r: MergeStrategy = serde_json::from_str("\"rebase\"").unwrap();
        assert_eq!(r.as_gh_flag(), "--rebase");
        assert!(serde_json::from_str::<MergeStrategy>("\"fast-forward\"").is_err());
    }

    #[test]
    fn parse_merge_block_reasons_review_required() {
        let view = GhMergeStateView {
            merge_state_status: Some("BLOCKED".to_string()),
            review_decision: Some("REVIEW_REQUIRED".to_string()),
            status_check_rollup: None,
        };
        let reasons = parse_merge_block_reasons(&view);
        assert!(reasons.iter().any(|r| r.contains("Required reviews")));
    }

    #[test]
    fn parse_merge_block_reasons_changes_requested() {
        let view = GhMergeStateView {
            merge_state_status: Some("BLOCKED".to_string()),
            review_decision: Some("CHANGES_REQUESTED".to_string()),
            status_check_rollup: None,
        };
        let reasons = parse_merge_block_reasons(&view);
        assert!(reasons.iter().any(|r| r.contains("Changes requested")));
    }

    #[test]
    fn parse_merge_block_reasons_failing_checks() {
        let view = GhMergeStateView {
            merge_state_status: Some("BLOCKED".to_string()),
            review_decision: None,
            status_check_rollup: Some(vec![GhCheckRun {
                name: "build".to_string(),
                status: "completed".to_string(),
                conclusion: Some("FAILURE".to_string()),
                details_url: None,
                workflow_name: None,
            }]),
        };
        let reasons = parse_merge_block_reasons(&view);
        assert!(reasons.iter().any(|r| r.contains("checks failing")));
    }

    #[test]
    fn parse_merge_block_reasons_unknown_block() {
        let view = GhMergeStateView {
            merge_state_status: Some("BLOCKED".to_string()),
            review_decision: None,
            status_check_rollup: None,
        };
        let reasons = parse_merge_block_reasons(&view);
        assert!(reasons.iter().any(|r| r.contains("Linear history")));
    }
}
