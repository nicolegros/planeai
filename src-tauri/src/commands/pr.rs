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
}
