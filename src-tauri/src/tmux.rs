use crate::command::tmux_command as command;

/// Generate a tmux session name: planeai-<project>-<8hex>
pub fn session_name(project_name: &str) -> String {
    let hex: String = uuid::Uuid::new_v4().to_string().replace('-', "")[..8].to_string();
    let sanitized = project_name.replace(' ', "-").replace(['.', ':'], "");
    format!("planeai-{}-{}", sanitized, hex)
}

/// Build the tmux command args to create a new session running kiro-cli.
#[allow(dead_code)]
pub fn build_new_session_args(
    tmux_name: &str,
    working_dir: &str,
    auto_approve: bool,
) -> Vec<String> {
    let cmd = if auto_approve {
        "kiro-cli chat --trust-all-tools".to_string()
    } else {
        "kiro-cli chat".to_string()
    };
    vec![
        "new-session".to_string(),
        "-d".to_string(),
        "-s".to_string(),
        tmux_name.to_string(),
        "-c".to_string(),
        working_dir.to_string(),
        cmd,
    ]
}

/// Check if a tmux session exists.
pub fn has_session(tmux_name: &str) -> bool {
    // Use '=' prefix for exact match to avoid tmux interpreting dots as separators
    let target = format!("={}", tmux_name);
    command(&["has-session", "-t", &target])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Kill a tmux session.
pub fn kill_session(tmux_name: &str) -> Result<(), String> {
    let target = format!("={}", tmux_name);
    let output = command(&["kill-session", "-t", &target])
        .output()
        .map_err(|e| format!("failed to run tmux: {e}"))?;

    if !output.status.success() {
        // Ignore "no such session" / "can't find session" errors (already dead)
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.contains("no such session") && !stderr.contains("can't find session") {
            return Err(stderr.to_string());
        }
    }
    Ok(())
}

/// Create a tmux session with remain-on-exit and launch kiro-cli.
#[allow(dead_code)]
pub fn create_session(
    tmux_name: &str,
    working_dir: &str,
    auto_approve: bool,
    session_id: &str,
) -> Result<(), String> {
    let cmd = if auto_approve {
        "kiro-cli chat --trust-all-tools".to_string()
    } else {
        "kiro-cli chat".to_string()
    };
    create_session_with_cmd_and_path(tmux_name, working_dir, &cmd, session_id, &[], None)
}

/// Create a tmux session with a custom command and extra PATH directories.
pub fn create_session_with_cmd_and_path(
    tmux_name: &str,
    working_dir: &str,
    cmd: &str,
    session_id: &str,
    extra_path_dirs: &[String],
    mcp: Option<&planeai_core::mcp::McpEndpoint>,
) -> Result<(), String> {
    let (args, token_file) = new_session_args(
        tmux_name,
        working_dir,
        cmd,
        session_id,
        extra_path_dirs,
        mcp,
        &planeai_paths::app_data_dir().join("mcp-tokens"),
    )?;
    let result = command(&args.iter().map(String::as_str).collect::<Vec<_>>())
        .output()
        .map_err(|e| format!("failed to run tmux: {e}"))
        .and_then(|output| {
            if output.status.success() {
                Ok(())
            } else {
                Err(String::from_utf8_lossy(&output.stderr).to_string())
            }
        });
    if let (Err(_), Some(file)) = (&result, token_file) {
        let _ = std::fs::remove_file(file);
    }
    result
}

/// The agent's shell reads the token from the file named by `$0`, then deletes it.
const READ_MCP_TOKEN: &str =
    r#"PLANEAI_MCP_TOKEN=$(cat "$0"); rm -f "$0"; export PLANEAI_MCP_TOKEN; "#;

/// `tmux new-session` arguments, with the token file they hand over. `-e` values show in
/// `ps` and stay in the tmux session's environment, so the token goes through a file that
/// the agent's shell consumes instead.
fn new_session_args(
    tmux_name: &str,
    working_dir: &str,
    cmd: &str,
    session_id: &str,
    extra_path_dirs: &[String],
    mcp: Option<&planeai_core::mcp::McpEndpoint>,
    token_dir: &std::path::Path,
) -> Result<(Vec<String>, Option<std::path::PathBuf>), String> {
    let mut env = vec![
        format!("PLANEAI_SESSION_ID={session_id}"),
        format!(
            "PATH={}",
            planeai_core::command::augmented_path(extra_path_dirs)
        ),
    ];
    if let Some(endpoint) = mcp {
        env.push(format!("{}={}", planeai_core::mcp::URL_ENV, endpoint.url));
    }
    let mut args: Vec<String> = ["new-session", "-d", "-s", tmux_name, "-c", working_dir]
        .map(String::from)
        .to_vec();
    for var in env {
        args.extend(["-e".to_string(), var]);
    }
    let token_file = match mcp {
        None => {
            args.push(cmd.to_string());
            None
        }
        Some(endpoint) => {
            let file = endpoint
                .write_token_file(token_dir)
                .map_err(|e| format!("failed to hand the MCP token to tmux: {e}"))?;
            args.push(format!(
                "exec /bin/sh -c {} {}",
                quote_for_any_shell(&format!("{READ_MCP_TOKEN}{cmd}")),
                quote_for_any_shell(&file.display().to_string()),
            ));
            Some(file)
        }
    };
    Ok((args, token_file))
}

/// Quote a word for any default shell: `'` and `\` stay out of the quotes, where fish reads escapes.
fn quote_for_any_shell(word: &str) -> String {
    let mut quoted = String::from("'");
    for c in word.chars() {
        match c {
            '\'' | '\\' => {
                quoted.push_str("'\\");
                quoted.push(c);
                quoted.push('\'');
            }
            c => quoted.push(c),
        }
    }
    quoted.push('\'');
    quoted
}

/// Send literal text to a tmux session, followed by Enter.
pub fn send_keys(tmux_name: &str, text: &str) -> Result<(), String> {
    // Send literal text (no key-name interpretation)
    let output = command(&["send-keys", "-t", tmux_name, "-l", text])
        .output()
        .map_err(|e| format!("failed to run tmux: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    // Send Enter separately
    let output = command(&["send-keys", "-t", tmux_name, "Enter"])
        .output()
        .map_err(|e| format!("failed to run tmux: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_name_format() {
        let name = session_name("myapp");
        assert!(name.starts_with("planeai-myapp-"));
        assert_eq!(name.len(), "planeai-myapp-".len() + 8);
    }

    #[test]
    fn the_mcp_token_reaches_the_agent_through_one_command_any_default_shell_runs() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        planeai_core::services::migrate(&conn).unwrap();
        let endpoint = planeai_core::mcp::issue(
            &conn,
            "session-1",
            &planeai_core::mcp::McpLaunch::new("http://127.0.0.1:47437/mcp"),
        )
        .unwrap();
        let [_, (_, token)] = endpoint.env();
        let shells = [
            "/bin/sh",
            "/bin/zsh",
            "/opt/homebrew/bin/fish",
            "/usr/bin/fish",
        ]
        .into_iter()
        .filter(|shell| std::path::Path::new(shell).exists());

        for shell in shells {
            let root = tempfile::tempdir().unwrap();
            let token_dir = root.path().join("Application Support");
            let (args, _) = new_session_args(
                "planeai-demo-1",
                "/tmp",
                r#"printf '%s %s\n' "$PLANEAI_MCP_TOKEN" "$PLANEAI_MCP_URL""#,
                "session-1",
                &[],
                Some(&endpoint),
                &token_dir,
            )
            .unwrap();

            assert!(args.iter().all(|arg| !arg.contains(token)), "{args:?}");
            assert!(args.contains(&"PLANEAI_MCP_URL=http://127.0.0.1:47437/mcp".to_string()));
            let command = args.last().unwrap();
            assert_eq!(
                args[args.len() - 2],
                "PLANEAI_MCP_URL=http://127.0.0.1:47437/mcp"
            );
            let output = std::process::Command::new(shell)
                .args(["-c", command])
                .env("PLANEAI_MCP_URL", "http://127.0.0.1:47437/mcp")
                .output()
                .unwrap();
            assert_eq!(
                String::from_utf8(output.stdout).unwrap(),
                format!("{token} http://127.0.0.1:47437/mcp\n"),
                "{shell}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                std::fs::read_dir(&token_dir).unwrap().count(),
                0,
                "the agent deletes the token file once read"
            );
        }
    }

    #[test]
    fn without_an_mcp_endpoint_tmux_runs_the_command_as_before() {
        let token_dir = tempfile::tempdir().unwrap();
        let (args, _) = new_session_args(
            "planeai-demo-1",
            "/tmp",
            "claude",
            "session-1",
            &[],
            None,
            token_dir.path(),
        )
        .unwrap();
        assert_eq!(args.last().map(String::as_str), Some("claude"));
        assert!(!args.iter().any(|arg| arg.starts_with("PLANEAI_MCP_")));
    }

    #[test]
    fn test_build_new_session_args() {
        let args = build_new_session_args("planeai-myapp-abc12345", "/tmp/myapp", true);
        assert_eq!(args[0], "new-session");
        assert_eq!(args[1], "-d");
        assert_eq!(args[2], "-s");
        assert_eq!(args[3], "planeai-myapp-abc12345");
        assert_eq!(args[4], "-c");
        assert_eq!(args[5], "/tmp/myapp");
        assert_eq!(args[6], "kiro-cli chat --trust-all-tools");

        let args_no = build_new_session_args("planeai-myapp-abc12345", "/tmp/myapp", false);
        assert_eq!(args_no[6], "kiro-cli chat");
    }
}
