//! WSL (Windows Subsystem for Linux) utilities.
//!
//! Provides distro detection, path translation, and helpers for spawning
//! sessions inside a WSL distribution from a Windows host.
//!
//! All public functions are no-ops or stubs on non-Windows platforms — callers
//! can use them unconditionally without `#[cfg]` guards at call sites.

use serde::{Deserialize, Serialize};

// ─── Configuration ───────────────────────────────────────────────────────────

/// WSL configuration block for `config.json`.
///
/// ```jsonc
/// {
///   "wsl": { "enabled": true, "distro": "Ubuntu" }
/// }
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct WslConfig {
    /// When true, sessions spawn inside the configured WSL distro.
    #[serde(default)]
    pub enabled: bool,
    /// Target distro name (e.g. "Ubuntu"). If absent, uses the system default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distro: Option<String>,
}

// ─── Detection ───────────────────────────────────────────────────────────────

/// Returns `true` if WSL is available and at least one distro is installed.
///
/// On non-Windows platforms, always returns `false`.
#[cfg(windows)]
pub fn is_available() -> bool {
    list_distros().map(|d| !d.is_empty()).unwrap_or(false)
}

#[cfg(not(windows))]
pub fn is_available() -> bool {
    false
}

/// List installed WSL distributions (quiet mode).
///
/// Runs `wsl -l -q` and parses the UTF-16LE output. Returns distro names in
/// order — the first entry is the system default.
#[cfg(windows)]
pub fn list_distros() -> Result<Vec<String>, WslError> {
    use std::process::Command;

    let output = Command::new("wsl")
        .args(["-l", "-q"])
        .output()
        .map_err(|e| WslError::SpawnFailed(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(WslError::CommandFailed(stderr));
    }

    Ok(parse_wsl_list_output(&output.stdout))
}

#[cfg(not(windows))]
pub fn list_distros() -> Result<Vec<String>, WslError> {
    Ok(Vec::new())
}

/// Returns the default WSL distro name (first entry from `wsl -l -q`).
#[cfg(windows)]
pub fn default_distro() -> Result<String, WslError> {
    let distros = list_distros()?;
    distros
        .into_iter()
        .next()
        .ok_or(WslError::NoDistrosInstalled)
}

#[cfg(not(windows))]
pub fn default_distro() -> Result<String, WslError> {
    Err(WslError::NotAvailable)
}

/// Resolve the effective distro: use the configured value or fall back to default.
pub fn resolve_distro(config: &WslConfig) -> Result<String, WslError> {
    match &config.distro {
        Some(d) if !d.is_empty() => Ok(d.clone()),
        _ => default_distro(),
    }
}

// ─── Path translation ────────────────────────────────────────────────────────

/// Convert a Linux path to a Windows UNC path for the given distro.
///
/// `/home/user/project` → `\\wsl.localhost\Ubuntu\home\user\project`
///
/// Returns `None` if the path is empty or doesn't start with `/`.
pub fn to_windows_path(linux_path: &str, distro: &str) -> Option<String> {
    if !linux_path.starts_with('/') {
        return None;
    }
    // Strip leading slash for the join — the UNC prefix provides the root
    let relative = &linux_path[1..];
    // Use forward slashes replaced with backslashes for the UNC path
    let win_relative = relative.replace('/', "\\");
    Some(format!("\\\\wsl.localhost\\{distro}\\{win_relative}"))
}

/// Convert a Windows path (e.g. `C:\Users\foo\project`) to a WSL mount path.
///
/// `C:\Users\foo\project` → `/mnt/c/Users/foo/project`
///
/// Returns `None` if the path is not a drive-letter path.
pub fn to_linux_path(windows_path: &str) -> Option<String> {
    // Handle UNC WSL paths: \\wsl.localhost\Distro\path or \\wsl$\Distro\path
    if let Some(inner) = windows_path
        .strip_prefix("\\\\wsl.localhost\\")
        .or_else(|| windows_path.strip_prefix("\\\\wsl$\\"))
    {
        // inner = "Distro\home\user\project"
        // Skip the distro name, return the rest as a Linux absolute path
        if let Some(pos) = inner.find('\\') {
            let linux_part = &inner[pos..];
            return Some(linux_part.replace('\\', "/"));
        } else {
            // Just distro name, root path
            return Some("/".to_string());
        }
    }

    // Handle drive-letter paths: C:\... or c:\...
    let bytes = windows_path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        let drive = (bytes[0] as char).to_ascii_lowercase();
        let rest = &windows_path[2..];
        let linux_rest = rest.replace('\\', "/");
        Some(format!("/mnt/{drive}{linux_rest}"))
    } else {
        None
    }
}

// ─── Command building ────────────────────────────────────────────────────────

/// Build command arguments for spawning a process inside WSL.
///
/// Returns `("wsl.exe", ["-d", distro, "--cd", cwd, "--", ...cmd_args])`.
pub fn build_wsl_command(
    distro: &str,
    cwd: Option<&str>,
    program: &str,
    args: &[&str],
) -> (String, Vec<String>) {
    let mut wsl_args = vec!["-d".to_string(), distro.to_string()];

    if let Some(dir) = cwd {
        wsl_args.push("--cd".to_string());
        wsl_args.push(dir.to_string());
    }

    wsl_args.push("--".to_string());
    wsl_args.push(program.to_string());
    for arg in args {
        wsl_args.push((*arg).to_string());
    }

    ("wsl.exe".to_string(), wsl_args)
}

/// Build command arguments for running a shell command string inside WSL.
///
/// Wraps the command in `sh -c "<cmd>"` inside WSL.
pub fn build_wsl_shell_command(
    distro: &str,
    cwd: Option<&str>,
    cmd_str: &str,
) -> (String, Vec<String>) {
    build_wsl_command(distro, cwd, "sh", &["-c", cmd_str])
}

/// Build command for spawning an interactive login shell inside WSL.
pub fn build_wsl_login_shell(distro: &str, cwd: Option<&str>) -> (String, Vec<String>) {
    build_wsl_command(distro, cwd, "bash", &["-l"])
}

// ─── Process lifecycle ────────────────────────────────────────────────────────

/// Pre-warm a WSL distro by running a no-op command.
///
/// First WSL invocation after boot has cold-start latency (1–4 seconds for WSL2).
/// Running this at app startup ensures subsequent session spawns are fast (~100ms).
///
/// This is async-friendly — callers should spawn it in the background.
#[cfg(windows)]
pub fn warm_distro(distro: &str) -> Result<(), WslError> {
    use std::process::Command;

    let mut cmd = Command::new("wsl");
    cmd.args(["-d", distro, "--", "true"]);
    crate::command::no_window(&mut cmd);

    let output = cmd
        .output()
        .map_err(|e| WslError::SpawnFailed(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(WslError::CommandFailed(stderr));
    }

    Ok(())
}

#[cfg(not(windows))]
pub fn warm_distro(_distro: &str) -> Result<(), WslError> {
    Ok(())
}

/// The ETX byte (Ctrl+C) — writing this to a PTY stdin sends SIGINT to the
/// foreground process group inside WSL.
pub const ETX: u8 = 0x03;

/// Graceful shutdown sequence for WSL sessions:
/// 1. Write ETX (Ctrl+C) to the PTY to signal the Linux foreground process
/// 2. Wait briefly for the process to handle the signal
/// 3. If still alive, the caller should kill the `wsl.exe` process
///
/// This prevents orphaned Linux processes inside WSL when sessions are destroyed.
///
/// Returns the ETX byte that should be written to the PTY writer before killing.
pub fn graceful_shutdown_bytes() -> &'static [u8] {
    &[ETX]
}

/// Duration to wait after sending ETX before force-killing the process.
/// This gives the Linux process time to handle SIGINT and exit cleanly.
pub const GRACEFUL_SHUTDOWN_DELAY_MS: u64 = 100;

// ─── Errors ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum WslError {
    /// WSL binary not found or failed to execute.
    SpawnFailed(String),
    /// `wsl` command returned non-zero exit status.
    CommandFailed(String),
    /// No WSL distributions are installed.
    NoDistrosInstalled,
    /// WSL is not available on this platform.
    NotAvailable,
}

impl std::fmt::Display for WslError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SpawnFailed(e) => write!(f, "failed to run wsl.exe: {e}"),
            Self::CommandFailed(e) => write!(f, "wsl command failed: {e}"),
            Self::NoDistrosInstalled => write!(f, "no WSL distributions installed"),
            Self::NotAvailable => write!(f, "WSL is not available on this platform"),
        }
    }
}

// ─── Internal helpers ────────────────────────────────────────────────────────

/// Parse the raw output of `wsl -l -q`, which is UTF-16LE with BOM on Windows.
///
/// Handles:
/// - UTF-16LE byte order mark (FF FE)
/// - Null bytes between ASCII characters
/// - Trailing null terminators on each line
/// - Empty lines
pub fn parse_wsl_list_output(raw: &[u8]) -> Vec<String> {
    let text = decode_utf16le(raw);
    text.lines()
        .map(|line| line.trim().trim_end_matches('\0').trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Decode a UTF-16LE byte slice (possibly with BOM) into a String.
fn decode_utf16le(raw: &[u8]) -> String {
    let bytes = if raw.starts_with(&[0xFF, 0xFE]) {
        &raw[2..] // Skip BOM
    } else {
        raw
    };

    // Interpret as u16 pairs
    let u16s: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect();

    String::from_utf16_lossy(&u16s)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Config serialization ──

    #[test]
    fn wsl_config_default_is_disabled() {
        let config = WslConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.distro, None);
    }

    #[test]
    fn wsl_config_deserializes_from_json() {
        let json = r#"{"enabled": true, "distro": "Ubuntu-22.04"}"#;
        let config: WslConfig = serde_json::from_str(json).unwrap();
        assert!(config.enabled);
        assert_eq!(config.distro.as_deref(), Some("Ubuntu-22.04"));
    }

    #[test]
    fn wsl_config_deserializes_minimal() {
        // Only `enabled` specified — distro should default to None
        let json = r#"{"enabled": true}"#;
        let config: WslConfig = serde_json::from_str(json).unwrap();
        assert!(config.enabled);
        assert_eq!(config.distro, None);
    }

    #[test]
    fn wsl_config_deserializes_empty_object() {
        let json = r#"{}"#;
        let config: WslConfig = serde_json::from_str(json).unwrap();
        assert!(!config.enabled);
        assert_eq!(config.distro, None);
    }

    #[test]
    fn wsl_config_serializes_skipping_none_distro() {
        let config = WslConfig {
            enabled: true,
            distro: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.contains("distro"));
    }

    // ── UTF-16LE parsing ──

    #[test]
    fn parse_utf16le_with_bom() {
        // "Ubuntu\r\nDebian\r\n" in UTF-16LE with BOM
        let input = b"\xFF\xFE\
            U\x00b\x00u\x00n\x00t\x00u\x00\r\x00\n\x00\
            D\x00e\x00b\x00i\x00a\x00n\x00\r\x00\n\x00";
        let result = parse_wsl_list_output(input);
        assert_eq!(result, vec!["Ubuntu", "Debian"]);
    }

    #[test]
    fn parse_utf16le_without_bom() {
        // "Ubuntu\r\n" in UTF-16LE without BOM
        let input = b"U\x00b\x00u\x00n\x00t\x00u\x00\r\x00\n\x00";
        let result = parse_wsl_list_output(input);
        assert_eq!(result, vec!["Ubuntu"]);
    }

    #[test]
    fn parse_utf16le_with_null_terminators() {
        // Some WSL versions add null chars at end of each name
        let input = b"\xFF\xFEU\x00b\x00u\x00n\x00t\x00u\x00\x00\x00\r\x00\n\x00";
        let result = parse_wsl_list_output(input);
        assert_eq!(result, vec!["Ubuntu"]);
    }

    #[test]
    fn parse_utf16le_empty_input() {
        let result = parse_wsl_list_output(b"");
        assert!(result.is_empty());
    }

    #[test]
    fn parse_utf16le_bom_only() {
        let result = parse_wsl_list_output(b"\xFF\xFE");
        assert!(result.is_empty());
    }

    #[test]
    fn parse_utf16le_multiple_distros_with_empty_lines() {
        // "Ubuntu\r\n\r\nDebian\r\n" — empty line in between
        let input = b"\xFF\xFE\
            U\x00b\x00u\x00n\x00t\x00u\x00\r\x00\n\x00\
            \r\x00\n\x00\
            D\x00e\x00b\x00i\x00a\x00n\x00\r\x00\n\x00";
        let result = parse_wsl_list_output(input);
        assert_eq!(result, vec!["Ubuntu", "Debian"]);
    }

    // ── Path translation ──

    #[test]
    fn to_windows_path_basic() {
        let result = to_windows_path("/home/user/project", "Ubuntu");
        assert_eq!(
            result,
            Some("\\\\wsl.localhost\\Ubuntu\\home\\user\\project".to_string())
        );
    }

    #[test]
    fn to_windows_path_root() {
        let result = to_windows_path("/", "Ubuntu");
        assert_eq!(result, Some("\\\\wsl.localhost\\Ubuntu\\".to_string()));
    }

    #[test]
    fn to_windows_path_rejects_relative() {
        let result = to_windows_path("relative/path", "Ubuntu");
        assert_eq!(result, None);
    }

    #[test]
    fn to_windows_path_rejects_empty() {
        let result = to_windows_path("", "Ubuntu");
        assert_eq!(result, None);
    }

    #[test]
    fn to_linux_path_drive_letter() {
        let result = to_linux_path("C:\\Users\\foo\\project");
        assert_eq!(result, Some("/mnt/c/Users/foo/project".to_string()));
    }

    #[test]
    fn to_linux_path_lowercase_drive() {
        let result = to_linux_path("d:\\code");
        assert_eq!(result, Some("/mnt/d/code".to_string()));
    }

    #[test]
    fn to_linux_path_drive_root() {
        let result = to_linux_path("C:\\");
        assert_eq!(result, Some("/mnt/c/".to_string()));
    }

    #[test]
    fn to_linux_path_unc_wsl_localhost() {
        let result = to_linux_path("\\\\wsl.localhost\\Ubuntu\\home\\user");
        assert_eq!(result, Some("/home/user".to_string()));
    }

    #[test]
    fn to_linux_path_unc_wsl_dollar() {
        let result = to_linux_path("\\\\wsl$\\Debian\\etc\\hosts");
        assert_eq!(result, Some("/etc/hosts".to_string()));
    }

    #[test]
    fn to_linux_path_unc_distro_only() {
        let result = to_linux_path("\\\\wsl.localhost\\Ubuntu");
        assert_eq!(result, Some("/".to_string()));
    }

    #[test]
    fn to_linux_path_rejects_relative() {
        let result = to_linux_path("relative\\path");
        assert_eq!(result, None);
    }

    #[test]
    fn to_linux_path_rejects_empty() {
        let result = to_linux_path("");
        assert_eq!(result, None);
    }

    // ── Command building ──

    #[test]
    fn build_wsl_command_basic() {
        let (program, args) = build_wsl_command("Ubuntu", Some("/home/user"), "bash", &["-l"]);
        assert_eq!(program, "wsl.exe");
        assert_eq!(
            args,
            vec!["-d", "Ubuntu", "--cd", "/home/user", "--", "bash", "-l"]
        );
    }

    #[test]
    fn build_wsl_command_no_cwd() {
        let (program, args) = build_wsl_command("Debian", None, "echo", &["hello"]);
        assert_eq!(program, "wsl.exe");
        assert_eq!(args, vec!["-d", "Debian", "--", "echo", "hello"]);
    }

    #[test]
    fn build_wsl_shell_command_wraps_in_sh() {
        let (program, args) =
            build_wsl_shell_command("Ubuntu", Some("/tmp"), "echo hello && ls");
        assert_eq!(program, "wsl.exe");
        assert_eq!(
            args,
            vec!["-d", "Ubuntu", "--cd", "/tmp", "--", "sh", "-c", "echo hello && ls"]
        );
    }

    #[test]
    fn build_wsl_login_shell_basic() {
        let (program, args) = build_wsl_login_shell("Ubuntu", Some("/home/user/project"));
        assert_eq!(program, "wsl.exe");
        assert_eq!(
            args,
            vec!["-d", "Ubuntu", "--cd", "/home/user/project", "--", "bash", "-l"]
        );
    }

    // ── resolve_distro ──

    #[test]
    fn resolve_distro_uses_config_value() {
        let config = WslConfig {
            enabled: true,
            distro: Some("Fedora".to_string()),
        };
        let result = resolve_distro(&config);
        assert_eq!(result, Ok("Fedora".to_string()));
    }

    #[test]
    fn resolve_distro_empty_string_falls_back() {
        let config = WslConfig {
            enabled: true,
            distro: Some(String::new()),
        };
        // On non-Windows, this returns NotAvailable since default_distro is a stub
        #[cfg(not(windows))]
        assert_eq!(resolve_distro(&config), Err(WslError::NotAvailable));
    }

    // ── Process lifecycle ──

    #[test]
    fn etx_constant_is_ctrl_c() {
        assert_eq!(ETX, 0x03);
    }

    #[test]
    fn graceful_shutdown_bytes_returns_etx() {
        let bytes = graceful_shutdown_bytes();
        assert_eq!(bytes, &[0x03]);
    }

    #[test]
    fn graceful_shutdown_delay_is_reasonable() {
        // Should be long enough for signal propagation but not so long it blocks UX
        let delay = GRACEFUL_SHUTDOWN_DELAY_MS;
        assert!(delay >= 50, "delay too short: {delay}ms");
        assert!(delay <= 500, "delay too long: {delay}ms");
    }

    #[test]
    fn warm_distro_on_non_windows_is_noop() {
        // On non-Windows, warm_distro should succeed without doing anything
        #[cfg(not(windows))]
        assert_eq!(warm_distro("Ubuntu"), Ok(()));
    }
}
