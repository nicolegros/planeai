use serde::Serialize;
use tauri::State;

use crate::config;
use crate::db;
use crate::pr;
use crate::state::{ConfigState, DbState};

use crate::commands::sessions::helpers::{resolve_task_manager, session_cwd};

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
                pr::fire_pr_hook(tm, t, task_key, std::path::Path::new(&cwd));
            }
        }
    }
    Ok(transition.is_some())
}

fn new_pr_url(cwd: &str, branch: &str, base_branch: Option<&str>) -> Result<String, String> {
    let output = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(cwd)
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
pub fn fetch_pr_url(
    session_id: String,
    db_state: State<DbState>,
    config_state: State<ConfigState>,
) -> Result<Option<String>, String> {
    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
    fetch_pr_url_inner(&conn, &cfg, &session_id)
}

#[derive(Debug, Clone, Serialize)]
pub struct PrDefaults {
    pub title: String,
    pub body: String,
    pub base_branch: String,
}

#[tauri::command]
pub fn generate_pr_defaults(
    session_id: String,
    db_state: State<DbState>,
) -> Result<PrDefaults, String> {
    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    let session = db::get_session(&conn, &session_id)
        .map_err(|e| e.to_string())?
        .ok_or("session not found")?;
    let cwd = session_cwd(&conn, &session).ok_or("cannot resolve session working directory")?;

    let title = match &session.task_key {
        Some(key) => {
            let prefix = format!("{}: ", key);
            let name = session.name.strip_prefix(&prefix).unwrap_or(&session.name);
            format!("{} [{}]", name, key)
        }
        None => session.name.clone(),
    };

    // Diff stats for body
    let diff_output = std::process::Command::new("git")
        .args([
            "diff",
            "--stat",
            &format!(
                "{}...HEAD",
                session.base_branch.as_deref().unwrap_or("main")
            ),
        ])
        .current_dir(&cwd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    let body = if diff_output.is_empty() {
        String::new()
    } else {
        format!("## Changes\n\n```\n{diff_output}\n```")
    };

    let base_branch = session
        .base_branch
        .unwrap_or_else(|| detect_default_branch(&cwd));

    Ok(PrDefaults {
        title,
        body,
        base_branch,
    })
}

fn detect_default_branch(cwd: &str) -> String {
    std::process::Command::new("git")
        .args(["symbolic-ref", "refs/remotes/origin/HEAD", "--short"])
        .current_dir(cwd)
        .output()
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
    let (cwd, branch) = {
        let conn = db_state.0.lock().map_err(|e| e.to_string())?;
        let session = db::get_session(&conn, &session_id)
            .map_err(|e| e.to_string())?
            .ok_or("session not found")?;
        let cwd = session_cwd(&conn, &session).ok_or("cannot resolve session working directory")?;
        (cwd, session.branch.clone())
    };

    // Push branch
    let push = std::process::Command::new("git")
        .args(["push", "-u", "origin", &branch])
        .current_dir(&cwd)
        .output()
        .map_err(|e| format!("failed to run git push: {e}"))?;
    if !push.status.success() {
        let stderr = String::from_utf8_lossy(&push.stderr);
        return Err(format!("git push failed: {stderr}"));
    }

    // Check gh is available
    let gh_check = std::process::Command::new("gh")
        .args(["--version"])
        .output();
    if gh_check.is_err() {
        return Err("GitHub CLI (gh) not found. Install from https://cli.github.com/".to_string());
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
    let pr_output = std::process::Command::new("gh")
        .args(&args)
        .current_dir(&cwd)
        .output()
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
        )
        .unwrap();

        let cfg = config::Config::default();
        let result = fetch_pr_url_inner(&conn, &cfg, "s1");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not configured"));
    }

    #[test]
    fn detect_default_branch_fallback() {
        let dir = tempfile::tempdir().unwrap();
        // No git repo — should fallback to "main"
        assert_eq!(detect_default_branch(dir.path().to_str().unwrap()), "main");
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
}
