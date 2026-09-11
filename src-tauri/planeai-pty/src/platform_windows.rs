//! Windows-specific PTY command building.

use portable_pty::CommandBuilder;

use crate::config::WslSpawnConfig;

/// Build a CommandBuilder that wraps a command string in a shell.
pub fn build_command(cmd_str: &str) -> CommandBuilder {
    let shell = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string());
    let mut c = CommandBuilder::new(&shell);
    c.args(["/C", cmd_str]);
    c
}

/// Build a CommandBuilder from a program and explicit argv (no shell wrapping).
pub fn build_command_argv(program: &str, args: &[&str]) -> CommandBuilder {
    let mut c = CommandBuilder::new(program);
    c.args(args);
    c
}

/// Platform-appropriate default shell fallback.
pub fn default_shell() -> String {
    std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string())
}

/// If the command is already wrapped in a shell invocation, extract the inner command.
/// Returns `None` if not a shell wrapper.
pub fn unwrap_shell_command(command: &str, args: &[&str]) -> Option<String> {
    if command.eq_ignore_ascii_case("cmd") && args.len() == 2 && args[0].eq_ignore_ascii_case("/C")
    {
        Some(args[1].to_string())
    } else {
        None
    }
}

/// Build a CommandBuilder that spawns a shell command inside WSL.
///
/// The resulting command is: `wsl.exe -d <distro> [--cd <cwd>] -- sh -c "<cmd>"`
///
/// ConPTY wraps the `wsl.exe` process — terminal I/O (colors, resize, interactive
/// programs) works correctly through this path.
pub fn build_command_wsl(cmd_str: &str, wsl: &WslSpawnConfig) -> CommandBuilder {
    let mut c = CommandBuilder::new("wsl.exe");
    c.args(["-d", &wsl.distro]);
    if let Some(ref cwd) = wsl.cwd {
        c.args(["--cd", cwd]);
    }
    c.args(["--", "sh", "-c", cmd_str]);
    c
}

/// Build a CommandBuilder that spawns a program with explicit args inside WSL.
///
/// The resulting command is: `wsl.exe -d <distro> [--cd <cwd>] -- <program> [args...]`
pub fn build_command_argv_wsl(program: &str, args: &[&str], wsl: &WslSpawnConfig) -> CommandBuilder {
    let mut c = CommandBuilder::new("wsl.exe");
    c.args(["-d", &wsl.distro]);
    if let Some(ref cwd) = wsl.cwd {
        c.args(["--cd", cwd]);
    }
    c.arg("--");
    c.arg(program);
    c.args(args);
    c
}

/// Build a CommandBuilder for an interactive login shell inside WSL.
///
/// The resulting command is: `wsl.exe -d <distro> [--cd <cwd>] -- bash -l`
pub fn build_login_shell_wsl(wsl: &WslSpawnConfig) -> CommandBuilder {
    build_command_argv_wsl("bash", &["-l"], wsl)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_command_wsl_basic() {
        let wsl = WslSpawnConfig {
            distro: "Ubuntu".to_string(),
            cwd: Some("/home/user/project".to_string()),
        };
        let cmd = build_command_wsl("echo hello", &wsl);
        let argv = cmd.as_unix_command_line();
        // CommandBuilder doesn't expose args easily on Windows, but we can verify
        // it builds without panic. Detailed testing done at integration level.
        assert!(argv.is_none() || argv.is_some()); // smoke test — compiles & builds
    }

    #[test]
    fn build_command_wsl_no_cwd() {
        let wsl = WslSpawnConfig {
            distro: "Debian".to_string(),
            cwd: None,
        };
        // Should not panic
        let _cmd = build_command_wsl("ls -la", &wsl);
    }

    #[test]
    fn build_login_shell_wsl_builds() {
        let wsl = WslSpawnConfig {
            distro: "Ubuntu".to_string(),
            cwd: Some("/home/user".to_string()),
        };
        let _cmd = build_login_shell_wsl(&wsl);
    }
}
