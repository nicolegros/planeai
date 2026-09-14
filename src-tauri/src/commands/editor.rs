use std::path::{Component, Path, PathBuf};

use tauri::State;

use crate::config::{self, Config};
use crate::db;
use crate::state::{ConfigState, DbState};

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedEditorLaunch {
    command: String,
    args: Vec<String>,
    project_path: String,
    file_path: String,
}

fn resolve_file_path(project_path: &str, file_path: &str) -> Result<String, String> {
    let file = Path::new(file_path);
    if file.as_os_str().is_empty() {
        return Err("File path is required".to_string());
    }
    if file
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return Err("File path must not contain '..'".to_string());
    }

    let target = if file.is_absolute() {
        file.to_path_buf()
    } else {
        PathBuf::from(project_path).join(file)
    };
    let canonical_project = std::fs::canonicalize(project_path)
        .map_err(|error| format!("Cannot resolve project path: {error}"))?;
    let canonical_target = std::fs::canonicalize(target)
        .map_err(|error| format!("Cannot resolve file path: {error}"))?;
    if !canonical_target.starts_with(&canonical_project) {
        return Err("File path is outside the project".to_string());
    }
    Ok(canonical_target.to_string_lossy().into_owned())
}

fn resolve_editor_launch(
    config: &Config,
    project_path: String,
    requested_file_path: &str,
    required_mode: &str,
) -> Result<ResolvedEditorLaunch, String> {
    let editor = config
        .editor
        .as_ref()
        .ok_or_else(|| "Embedded editor is selected".to_string())?;
    config::validate_editor_config(editor)?;
    if editor.mode != required_mode {
        return Err(format!("{} editor is not selected", required_mode));
    }

    let file_path = resolve_file_path(&project_path, requested_file_path)?;
    let args = editor
        .args
        .iter()
        .map(|arg| {
            arg.replace("{file}", &file_path)
                .replace("{project}", &project_path)
        })
        .collect();

    Ok(ResolvedEditorLaunch {
        command: editor.command.clone(),
        args,
        project_path,
        file_path,
    })
}

async fn session_project_path(
    session_id: String,
    db_state: State<'_, DbState>,
) -> Result<String, String> {
    let connection = db_state.0.clone();
    super::blocking(move || {
        let conn = connection.lock().map_err(|e| e.to_string())?;
        let session = db::get_session(&conn, &session_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Session not found".to_string())?;
        super::sessions::helpers::session_cwd(&conn, &session)
            .ok_or_else(|| "Project not found for session".to_string())
    })
    .await
}

async fn configured_launch(
    config: Config,
    project_path: String,
    file_path: String,
    mode: &'static str,
) -> Result<ResolvedEditorLaunch, String> {
    super::blocking(move || resolve_editor_launch(&config, project_path, &file_path, mode)).await
}

#[cfg(not(windows))]
fn terminal_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(windows)]
fn terminal_quote(value: &str) -> String {
    let escaped = value
        .replace('^', "^^")
        .replace('&', "^&")
        .replace('|', "^|")
        .replace('<', "^<")
        .replace('>', "^>")
        .replace('%', "^%")
        .replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn terminal_command(launch: &ResolvedEditorLaunch) -> String {
    std::iter::once(terminal_quote(&launch.command))
        .chain(launch.args.iter().map(|arg| terminal_quote(arg)))
        .collect::<Vec<_>>()
        .join(" ")
}

#[tauri::command]
pub async fn get_terminal_editor_command(
    session_id: String,
    file_path: String,
    db_state: State<'_, DbState>,
    config_state: State<'_, ConfigState>,
) -> Result<String, String> {
    let config = config_state.0.lock().map_err(|e| e.to_string())?.clone();
    let project_path = session_project_path(session_id, db_state).await?;
    let launch = configured_launch(config, project_path, file_path, "terminal").await?;
    Ok(terminal_command(&launch))
}

#[tauri::command]
pub async fn open_external_editor(
    session_id: String,
    file_path: String,
    db_state: State<'_, DbState>,
    config_state: State<'_, ConfigState>,
) -> Result<(), String> {
    let config = config_state.0.lock().map_err(|e| e.to_string())?.clone();
    let extra_path_dirs = config.resolved_extra_path_dirs();
    let project_path = session_project_path(session_id, db_state).await?;
    let launch = configured_launch(config, project_path, file_path, "external").await?;

    let mut command = tokio::process::Command::new(&launch.command);
    command
        .args(&launch.args)
        .current_dir(&launch.project_path)
        .env(
            "PATH",
            planeai_core::command::augmented_path(&extra_path_dirs),
        );
    planeai_core::command::no_window_tokio(&mut command);
    command
        .spawn()
        .map_err(|error| format!("Failed to launch editor '{}': {error}", launch.command))?;
    tracing::info!(
        command = %launch.command,
        file = %launch.file_path,
        project = %launch.project_path,
        "launched external editor"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EditorConfig;

    fn editor(mode: &str, command: &str, args: &[&str]) -> EditorConfig {
        EditorConfig {
            mode: mode.to_string(),
            command: command.to_string(),
            args: args.iter().map(ToString::to_string).collect(),
        }
    }

    #[test]
    fn validates_non_embedded_editors_require_an_executable_and_file_placeholder() {
        assert_eq!(
            config::validate_editor_config(&editor("external", "", &["{file}"])),
            Err("Editor executable is required".to_string())
        );
        assert_eq!(
            config::validate_editor_config(&editor("terminal", "nvim", &[])),
            Err("Editor arguments must include {file}".to_string())
        );
        assert!(config::validate_editor_config(&editor("embedded", "", &[])).is_ok());
    }

    #[test]
    fn expands_file_and_project_placeholders() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("repo");
        let source = project.join("src");
        std::fs::create_dir_all(&source).unwrap();
        let file = source.join("main.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        let project_path = project.to_string_lossy().into_owned();
        let file_path = std::fs::canonicalize(&file)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let config = Config {
            editor: Some(editor(
                "external",
                "code",
                &["--goto", "{file}", "{project}"],
            )),
            ..Config::default()
        };
        let launch =
            resolve_editor_launch(&config, project_path.clone(), "src/main.rs", "external")
                .unwrap();

        assert_eq!(launch.file_path, file_path);
        assert_eq!(
            launch.args,
            vec!["--goto", file_path.as_str(), project_path.as_str()]
        );
    }

    #[test]
    fn rejects_absolute_files_outside_the_project() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("repo");
        let outside = temp.path().join("secret.rs");
        std::fs::create_dir(&project).unwrap();
        std::fs::write(&outside, "secret").unwrap();

        assert_eq!(
            resolve_file_path(&project.to_string_lossy(), &outside.to_string_lossy()),
            Err("File path is outside the project".to_string())
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinks_that_escape_the_project() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("repo");
        let outside = temp.path().join("secret.rs");
        std::fs::create_dir(&project).unwrap();
        std::fs::write(&outside, "secret").unwrap();
        std::os::unix::fs::symlink(&outside, project.join("linked.rs")).unwrap();

        assert_eq!(
            resolve_file_path(
                &project.to_string_lossy(),
                &project.join("linked.rs").to_string_lossy()
            ),
            Err("File path is outside the project".to_string())
        );
    }

    #[test]
    fn rejects_relative_parent_traversal() {
        assert_eq!(
            resolve_file_path("/work/repo", "../secret"),
            Err("File path must not contain '..'".to_string())
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn terminal_command_quotes_each_argument() {
        let launch = ResolvedEditorLaunch {
            command: "nvim".to_string(),
            args: vec!["/work/my repo/o'hare.rs".to_string()],
            project_path: "/work/my repo".to_string(),
            file_path: "/work/my repo/o'hare.rs".to_string(),
        };
        assert_eq!(
            terminal_command(&launch),
            "'nvim' '/work/my repo/o'\\''hare.rs'"
        );
    }
}
