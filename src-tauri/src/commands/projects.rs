use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;
use tauri::State;

use crate::cleanup;
use crate::commands::sessions::lifecycle::session_transition;
use crate::db;
use crate::git;
use crate::plugins::PluginRuntimeHandle;
use crate::state::{DbState, ProjectOperationState, PtyState};
use crate::util::expand_tilde;

fn resolve_project_path(path: &str) -> Result<String, String> {
    std::fs::canonicalize(expand_tilde(path))
        .map_err(|error| format!("Cannot resolve project path: {error}"))?
        .into_os_string()
        .into_string()
        .map_err(|_| "Project path is not valid UTF-8.".to_string())
}

const NOT_A_GIT_REPOSITORY: &str = "Not a valid git repository (no .git found).";

fn resolve_repository_path(path: &str) -> Result<String, String> {
    let path = resolve_project_path(path)?;
    if !Path::new(&path).join(".git").exists() {
        return Err(NOT_A_GIT_REPOSITORY.to_string());
    }
    Ok(path)
}

#[tauri::command]
pub async fn create_project(
    state: State<'_, DbState>,
    name: String,
    path: String,
) -> Result<db::Project, String> {
    let conn = state.0.clone();
    crate::commands::blocking(move || {
        let path = resolve_repository_path(&path)?;
        let conn = conn.lock().map_err(|e| e.to_string())?;
        if db::project_name_exists(&conn, &name).map_err(|e| e.to_string())? {
            return Err(format!("A project named '{}' already exists.", name));
        }
        if db::project_path_in_use(&conn, &path, "").map_err(|e| e.to_string())? {
            return Err(format!("A project already uses the path '{}'.", path));
        }
        db::create_project(&conn, &name, &path).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn update_project(
    state: State<'_, DbState>,
    operations: State<'_, ProjectOperationState>,
    id: String,
    name: String,
    path: String,
) -> Result<db::Project, String> {
    let operation_lock = operations.lock_for(&id);
    let _operation_guard = operation_lock.lock_owned().await;
    let conn = state.0.clone();
    crate::commands::blocking(move || save_project(&conn, &id, &name, &path)).await
}

/// Renames a project and/or relinks it to another folder. Relinking reattaches
/// the project's worktree sessions to the new folder before anything is saved.
fn save_project(
    conn: &Mutex<Connection>,
    id: &str,
    name: &str,
    path: &str,
) -> Result<db::Project, String> {
    let (current, sessions) = {
        let conn = conn.lock().map_err(|e| e.to_string())?;
        let current = db::get_project(&conn, id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Project not found.".to_string())?;
        if current.name != name
            && db::project_name_exists(&conn, name).map_err(|e| e.to_string())?
        {
            return Err(format!("A project named '{}' already exists.", name));
        }
        let sessions = db::get_project_sessions(&conn, id).map_err(|e| e.to_string())?;
        (current, sessions)
    };

    // A missing folder cannot be canonicalized, so an unchanged path is kept as-is.
    let path = if expand_tilde(path) == current.path {
        current.path.clone()
    } else {
        resolve_repository_path(path)?
    };

    if current.path != path {
        let conn = conn.lock().map_err(|e| e.to_string())?;
        if db::project_path_in_use(&conn, &path, id).map_err(|e| e.to_string())? {
            return Err(format!("A project already uses the path '{}'.", path));
        }
        drop(conn);
        reattach_worktrees(&path, &sessions)?;
    }

    let conn = conn.lock().map_err(|e| e.to_string())?;
    db::update_project(&conn, id, name, &path).map_err(|e| e.to_string())
}

fn reattach_worktrees(repo_path: &str, sessions: &[db::Session]) -> Result<(), String> {
    let failed: Vec<&str> = sessions
        .iter()
        .filter_map(|session| {
            let worktree = session.worktree_path.as_deref()?;
            if !Path::new(worktree).is_dir() {
                return None;
            }
            match git::worktree_repair(repo_path, worktree) {
                Ok(()) => None,
                Err(error) => {
                    tracing::warn!(session_id = %session.id, %error, "worktree repair failed");
                    Some(session.name.as_str())
                }
            }
        })
        .collect();
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Could not reattach worktree sessions to this folder: {}.",
            failed.join(", ")
        ))
    }
}

#[tauri::command]
pub async fn list_projects(state: State<'_, DbState>) -> Result<Vec<db::Project>, String> {
    let conn = state.0.clone();
    crate::commands::blocking(move || {
        let conn = conn.lock().map_err(|e| e.to_string())?;
        db::list_projects(&conn).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn list_archived_projects(state: State<'_, DbState>) -> Result<Vec<db::Project>, String> {
    let conn = state.0.clone();
    crate::commands::blocking(move || {
        let conn = conn.lock().map_err(|e| e.to_string())?;
        db::list_archived_projects(&conn).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub fn archive_project(
    state: State<DbState>,
    runtime: State<PluginRuntimeHandle>,
    id: String,
) -> Result<(), String> {
    let lifecycle_events = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        let sessions = db::get_project_sessions(&conn, &id).map_err(|e| e.to_string())?;
        db::archive_project(&conn, &id).map_err(|e| e.to_string())?;
        sessions
            .into_iter()
            .filter(|session| session.status != "archived")
            .map(|session| session_transition(&session, &session.status, "archived"))
            .collect::<Vec<_>>()
    };
    for event in lifecycle_events {
        runtime.0.dispatch_session_lifecycle(event);
    }
    Ok(())
}

#[tauri::command]
pub async fn restore_project(state: State<'_, DbState>, id: String) -> Result<(), String> {
    let archived_path = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        db::get_project(&conn, &id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Project not found.".to_string())?
            .path
    };
    let path = crate::commands::blocking(move || resolve_project_path(&archived_path)).await?;
    let conn = state.0.clone();
    crate::commands::blocking(move || {
        let conn = conn.lock().map_err(|e| e.to_string())?;
        let project = db::get_project(&conn, &id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Project not found.".to_string())?;
        if db::project_path_in_use(&conn, &path, &id).map_err(|e| e.to_string())? {
            return Err(format!("A project already uses the path '{}'.", path));
        }
        db::update_project(&conn, &id, &project.name, &path).map_err(|e| e.to_string())?;
        db::restore_project(&conn, &id).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn hide_project(state: State<'_, DbState>, id: String) -> Result<(), String> {
    let conn = state.0.clone();
    crate::commands::blocking(move || {
        let conn = conn.lock().map_err(|e| e.to_string())?;
        db::hide_project(&conn, &id).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn unhide_project(state: State<'_, DbState>, id: String) -> Result<(), String> {
    let conn = state.0.clone();
    crate::commands::blocking(move || {
        let conn = conn.lock().map_err(|e| e.to_string())?;
        db::unhide_project(&conn, &id).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub fn get_project_auto_mode(state: State<DbState>, id: String) -> Result<bool, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT auto_mode FROM projects WHERE id = ?1",
        [&id],
        |row| row.get::<_, i64>(0),
    )
    .map(|v| v != 0)
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_project_auto_mode(
    state: State<DbState>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE projects SET auto_mode = ?1 WHERE id = ?2",
        rusqlite::params![enabled as i64, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn delete_project(
    state: State<'_, DbState>,
    pty_state: State<'_, PtyState>,
    runtime: State<'_, PluginRuntimeHandle>,
    operations: State<'_, ProjectOperationState>,
    id: String,
) -> Result<(), String> {
    // Serialize deletion with launches and PTY attaches for this project. The guard
    // remains held through teardown so a local-backend process cannot appear after
    // the initial PTY detach.
    let operation_lock = operations.lock_for(&id);
    let _operation_guard = operation_lock.lock_owned().await;

    let (sessions, project_path, owned_worktrees) = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        let sessions = db::get_project_sessions(&conn, &id).map_err(|e| e.to_string())?;
        let project_path = db::get_project(&conn, &id)
            .map_err(|e| e.to_string())?
            .map(|project| project.path);
        let owned_worktrees = sessions
            .iter()
            .map(|session| db::session_owns_worktree(&conn, &session.id).map_err(|e| e.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        (sessions, project_path, owned_worktrees)
    };

    // PtyState is main-thread-only; detach before moving process and filesystem
    // teardown to the blocking worker.
    for session in &sessions {
        pty_state.0.detach(&session.id);
    }

    let teardown_sessions = sessions.clone();
    crate::commands::blocking(move || {
        for (session, owns_worktree) in teardown_sessions.iter().zip(owned_worktrees) {
            let kill_errors = cleanup::kill_backend(
                &session.backend,
                session.tmux_name.as_deref(),
                Some(&session.id),
                &cleanup::real_kill_ops(),
            );
            if !kill_errors.is_empty() {
                tracing::warn!(session_id = %session.id, ?kill_errors, "failed to fully stop backend while deleting project");
            }
            if owns_worktree {
                if let (Some(project_path), Some(worktree_path)) =
                    (project_path.as_deref(), session.worktree_path.as_deref())
                {
                    let _ = git::worktree_remove(project_path, worktree_path);
                    let _ = std::fs::remove_dir_all(worktree_path);
                }
            }
        }
        Ok(())
    })
    .await?;

    let lifecycle_events = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        for session in &sessions {
            if session.status != "destroyed" {
                db::destroy_session(&conn, &session.id).map_err(|e| e.to_string())?;
            }
        }
        db::delete_project(&conn, &id).map_err(|e| e.to_string())?;
        sessions
            .iter()
            .filter(|session| session.status != "destroyed")
            .map(|session| session_transition(session, &session.status, "destroyed"))
            .collect::<Vec<_>>()
    };
    for event in lifecycle_events {
        runtime.0.dispatch_session_lifecycle(event);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    fn git(dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn init_repo(path: &Path) {
        std::fs::create_dir_all(path).unwrap();
        git(path, &["init", "-q", "-b", "main"]);
        git(
            path,
            &[
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "init",
            ],
        );
    }

    fn canonical(path: &Path) -> String {
        std::fs::canonicalize(path)
            .unwrap()
            .to_string_lossy()
            .to_string()
    }

    struct Fixture {
        _root: TempDir,
        conn: Mutex<Connection>,
        project_id: String,
        worktree: std::path::PathBuf,
        moved: std::path::PathBuf,
    }

    /// A project with one worktree session whose repo was then moved with `mv`.
    fn moved_project_with_worktree() -> Fixture {
        let root = TempDir::new().unwrap();
        let repo = root.path().join("repo");
        let worktree = root.path().join("worktrees").join("feature");
        init_repo(&repo);
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feature",
                worktree.to_str().unwrap(),
            ],
        );

        let conn = Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        let project = db::create_project(&conn, "repo", &canonical(&repo)).unwrap();
        db::create_session(
            &conn,
            &project.id,
            "feature work",
            "t-1",
            "feature",
            Some(&canonical(&worktree)),
        )
        .unwrap();

        let moved = root.path().join("moved-repo");
        std::fs::rename(&repo, &moved).unwrap();
        Fixture {
            _root: root,
            conn: Mutex::new(conn),
            project_id: project.id,
            worktree,
            moved,
        }
    }

    fn stored_path(fixture: &Fixture) -> String {
        let conn = fixture.conn.lock().unwrap();
        db::get_project(&conn, &fixture.project_id)
            .unwrap()
            .unwrap()
            .path
    }

    #[test]
    fn relinking_a_moved_repo_reattaches_its_worktrees() {
        let fixture = moved_project_with_worktree();
        let broken = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&fixture.worktree)
            .output()
            .unwrap();
        assert_eq!(broken.status.code(), Some(128));

        let saved = save_project(
            &fixture.conn,
            &fixture.project_id,
            "repo",
            fixture.moved.to_str().unwrap(),
        )
        .unwrap();

        assert_eq!(saved.path, canonical(&fixture.moved));
        assert!(!saved.path_missing);
        assert_eq!(
            git(&fixture.worktree, &["rev-parse", "--abbrev-ref", "HEAD"]),
            "feature"
        );
        assert_eq!(
            git(&fixture.worktree, &["rev-parse", "--git-common-dir"]),
            format!("{}/.git", canonical(&fixture.moved))
        );

        let again = save_project(
            &fixture.conn,
            &fixture.project_id,
            "repo",
            fixture.moved.to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(again.path, canonical(&fixture.moved));
    }

    #[test]
    fn relinking_to_another_clone_fails_and_keeps_the_old_path() {
        let fixture = moved_project_with_worktree();
        let old_path = stored_path(&fixture);
        let other = fixture._root.path().join("other-clone");
        init_repo(&other);

        let error = save_project(
            &fixture.conn,
            &fixture.project_id,
            "repo",
            other.to_str().unwrap(),
        )
        .unwrap_err();

        assert_eq!(
            error,
            "Could not reattach worktree sessions to this folder: feature work."
        );
        assert_eq!(stored_path(&fixture), old_path);
        assert_eq!(
            old_path,
            format!("{}/repo", canonical(fixture._root.path()))
        );
    }

    #[test]
    fn relinking_skips_worktrees_that_no_longer_exist() {
        let fixture = moved_project_with_worktree();
        std::fs::remove_dir_all(&fixture.worktree).unwrap();

        let saved = save_project(
            &fixture.conn,
            &fixture.project_id,
            "repo",
            fixture.moved.to_str().unwrap(),
        )
        .unwrap();

        assert_eq!(saved.path, canonical(&fixture.moved));
    }

    #[test]
    fn relinking_to_a_folder_without_git_is_refused() {
        let fixture = moved_project_with_worktree();
        let plain = fixture._root.path().join("plain");
        std::fs::create_dir_all(&plain).unwrap();

        let error = save_project(
            &fixture.conn,
            &fixture.project_id,
            "repo",
            plain.to_str().unwrap(),
        )
        .unwrap_err();

        assert_eq!(error, "Not a valid git repository (no .git found).");
    }

    #[test]
    fn renaming_a_project_whose_folder_is_missing_keeps_its_path() {
        let fixture = moved_project_with_worktree();
        let old_path = stored_path(&fixture);

        let saved = save_project(&fixture.conn, &fixture.project_id, "renamed", &old_path).unwrap();

        assert_eq!((saved.name.as_str(), saved.path_missing), ("renamed", true));
        assert_eq!(
            saved.path,
            format!("{}/repo", canonical(fixture._root.path()))
        );
    }
}
