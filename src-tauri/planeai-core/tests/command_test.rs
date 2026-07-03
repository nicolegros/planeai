use planeai_core::command::{run_command, CommandError};
use tempfile::tempdir;

#[test]
fn successful_command_returns_stdout() {
    let dir = tempdir().unwrap();
    let result = run_command("echo hello", dir.path()).unwrap();
    assert_eq!(result.trim(), "hello");
}

#[test]
fn handles_spaces_in_arguments() {
    let dir = tempdir().unwrap();
    let result = run_command("echo 'hello world'", dir.path()).unwrap();
    assert_eq!(result.trim(), "hello world");
}

#[test]
fn non_zero_exit_returns_error() {
    let dir = tempdir().unwrap();
    let err = run_command("sh -c 'echo oops >&2; exit 42'", dir.path()).unwrap_err();
    match err {
        CommandError::NonZeroExit { status, stderr } => {
            assert_eq!(status, 42);
            assert!(stderr.contains("oops"));
        }
        _ => panic!("expected NonZeroExit, got {err:?}"),
    }
}

#[test]
fn spawn_failure_on_invalid_binary() {
    let dir = tempdir().unwrap();
    let err = run_command("nonexistent_binary_xyz_123", dir.path()).unwrap_err();
    match err {
        CommandError::NonZeroExit { stderr, .. } => {
            assert!(stderr.contains("not found") || stderr.contains("No such file"));
        }
        CommandError::SpawnFailed { .. } => {
            // Also acceptable — depends on OS behavior
        }
    }
}

#[test]
fn empty_command_returns_spawn_failed() {
    let dir = tempdir().unwrap();
    let err = run_command("", dir.path()).unwrap_err();
    match err {
        CommandError::SpawnFailed { source, .. } => {
            assert!(source.contains("empty"));
        }
        _ => panic!("expected SpawnFailed, got {err:?}"),
    }
}

#[test]
fn respects_cwd() {
    let dir = tempdir().unwrap();
    let result = run_command("pwd", dir.path()).unwrap();
    // On macOS, /tmp is a symlink to /private/tmp
    let expected = std::fs::canonicalize(dir.path()).unwrap();
    let actual = std::fs::canonicalize(result.trim()).unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn build_daemon_env_includes_usr_local_bin() {
    use planeai_core::command::build_daemon_env;

    let extra_path_dirs: Vec<String> = vec![];
    let session_id = "shell-tab-test";
    let mut path_buf = String::new();
    let env = build_daemon_env(&extra_path_dirs, session_id, &mut path_buf);

    let path = env.get("PATH").expect("daemon env must include PATH");
    assert!(
        path.contains("/usr/local/bin"),
        "daemon env PATH must include /usr/local/bin for commands like tgf: {path}"
    );
}

#[test]
fn build_daemon_env_includes_session_id() {
    use planeai_core::command::build_daemon_env;

    let extra_path_dirs: Vec<String> = vec![];
    let session_id = "test-tab:1";
    let mut path_buf = String::new();
    let env = build_daemon_env(&extra_path_dirs, session_id, &mut path_buf);

    assert_eq!(
        env.get("PLANEAI_SESSION_ID"),
        Some(&"test-tab:1"),
        "daemon env must include PLANEAI_SESSION_ID"
    );
}

#[test]
fn build_daemon_env_includes_extra_path_dirs() {
    use planeai_core::command::build_daemon_env;

    let extra_path_dirs = vec!["/custom/shims".to_string()];
    let session_id = "shell-tab:2";
    let mut path_buf = String::new();
    let env = build_daemon_env(&extra_path_dirs, session_id, &mut path_buf);

    let path = env.get("PATH").expect("daemon env must include PATH");
    assert!(
        path.contains("/custom/shims"),
        "daemon env PATH must include extra_path_dirs: {path}"
    );
}