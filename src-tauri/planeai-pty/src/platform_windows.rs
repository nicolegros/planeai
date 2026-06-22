//! Windows-specific PTY command building.

use portable_pty::CommandBuilder;

/// Build a CommandBuilder that wraps a command string in a shell.
pub fn build_command(cmd_str: &str) -> CommandBuilder {
    let shell = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string());
    let mut c = CommandBuilder::new(&shell);
    c.args(["/C", cmd_str]);
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
