use std::path::Path;
use std::process::Command;

use crate::config;

/// Build a PATH string with user-local bin directories prepended.
/// Delegates to planeai_core::command::augmented_path with config dirs.
#[cfg(test)]
pub fn augmented_path(config_dirs: &[String]) -> String {
    planeai_core::command::augmented_path(config_dirs)
}

/// Resolve a command name to its full path, checking user-local bin directories
/// that may not be in PATH when launched from a GUI app.
#[cfg(not(windows))]
pub fn resolve(cmd: &str) -> String {
    if cmd.starts_with('/') {
        return cmd.to_string();
    }
    let home = config::home_dir();
    let extra_dirs = [
        format!("{home}/.local/bin"),
        format!("{home}/.cargo/bin"),
        format!("{home}/go/bin"),
        "/opt/homebrew/bin".to_string(),
        "/usr/local/bin".to_string(),
    ];
    for dir in &extra_dirs {
        let full = format!("{dir}/{cmd}");
        if Path::new(&full).exists() {
            return full;
        }
    }
    if let Ok(output) = Command::new("/bin/bash")
        .args(["-lc", &format!("which {cmd}")])
        .output()
    {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return path;
            }
        }
    }
    cmd.to_string()
}

/// Resolve a command name on Windows, probing .cmd/.exe extensions and
/// common install directories that may not be in PATH for GUI apps.
#[cfg(windows)]
pub fn resolve(cmd: &str) -> String {
    // Already a full path with extension — return as-is
    if Path::new(cmd).is_absolute() && Path::new(cmd).exists() {
        return cmd.to_string();
    }

    let extensions = [".cmd", ".exe", ".bat"];
    let home = config::home_dir();
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| format!("{home}\\AppData\\Roaming"));
    let localappdata =
        std::env::var("LOCALAPPDATA").unwrap_or_else(|_| format!("{home}\\AppData\\Local"));

    let extra_dirs = [
        format!("{appdata}\\npm"),
        format!("{home}\\.cargo\\bin"),
        format!("{localappdata}\\Programs"),
    ];

    for dir in &extra_dirs {
        // Check with extensions first (copilot.cmd, claude.exe, etc.)
        for ext in &extensions {
            let full = format!("{dir}\\{cmd}{ext}");
            if Path::new(&full).exists() {
                return full;
            }
        }
        // Check without extension (already has one, or is a native .exe)
        let full = format!("{dir}\\{cmd}");
        if Path::new(&full).exists() {
            return full;
        }
    }

    // Fallback: use where.exe to search PATH
    if let Ok(output) = {
        let mut where_cmd = Command::new("where.exe");
        where_cmd.arg(cmd);
        planeai_core::command::no_window(&mut where_cmd);
        where_cmd.output()
    } {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            if !path.is_empty() {
                return path;
            }
        }
    }

    cmd.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn augmented_path_includes_conventional_dirs() {
        let path = augmented_path(&[]);
        let home = config::home_dir();
        if cfg!(windows) {
            assert!(
                path.contains(&format!("{home}\\.cargo\\bin")),
                "PATH should include .cargo\\bin, got: {path}"
            );
        } else {
            assert!(
                path.contains(&format!("{home}/.local/bin")),
                "PATH should include .local/bin, got: {path}"
            );
        }
    }

    #[test]
    fn augmented_path_uses_platform_separator() {
        let path = augmented_path(&[]);
        let sep = if cfg!(windows) { ";" } else { ":" };
        assert!(path.contains(sep), "PATH should use platform separator");
    }

    /// Regression test: gh/git subprocess spawns must use augmented_path to work
    /// when launched from macOS Spotlight (minimal inherited PATH).
    #[test]
    fn augmented_path_includes_homebrew_bin() {
        let path = augmented_path(&[]);
        if cfg!(not(windows)) {
            assert!(
                path.contains("/opt/homebrew/bin"),
                "PATH should include /opt/homebrew/bin for Homebrew-installed tools (gh, git), got: {path}"
            );
            assert!(
                path.contains("/usr/local/bin"),
                "PATH should include /usr/local/bin, got: {path}"
            );
        }
    }

    /// Regression test: resolve() returns full path for known binaries and
    /// gracefully falls back to the bare name when binary is not found.
    #[test]
    #[cfg(unix)]
    fn resolve_finds_known_binary_and_falls_back_for_unknown() {
        // resolve() should find well-known tools and return an absolute path
        let git_path = resolve("git");
        assert!(
            git_path.starts_with('/'),
            "resolve(\"git\") should return an absolute path, got: {git_path}"
        );

        // resolve() should gracefully fall back to the bare name when not found
        let result = resolve("nonexistent_tool_xyz_12345");
        assert_eq!(result, "nonexistent_tool_xyz_12345");
    }

    /// Regression test: augmented_path result, when set as PATH env var on a
    /// subprocess, allows that subprocess to find tools in conventional dirs.
    #[test]
    #[cfg(unix)]
    fn subprocess_with_augmented_path_finds_tools() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let bin_path = dir.path().join("my-test-tool");
        std::fs::write(&bin_path, "#!/bin/sh\necho found").unwrap();
        std::fs::set_permissions(&bin_path, std::fs::Permissions::from_mode(0o755)).unwrap();

        // Build a PATH that includes our temp dir (simulating augmented_path including a dir)
        let aug_path = format!("{}:{}", dir.path().display(), augmented_path(&[]));

        let output = Command::new("/bin/sh")
            .args(["-c", "my-test-tool"])
            .env("PATH", &aug_path)
            .output()
            .unwrap();

        assert!(
            output.status.success(),
            "subprocess with augmented PATH should find tools: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "found");
    }

    /// Regression test: without augmented PATH, subprocess cannot find tools
    /// outside /usr/bin:/bin (simulates Spotlight launch scenario).
    #[test]
    #[cfg(unix)]
    fn subprocess_with_minimal_path_cannot_find_tools() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let bin_path = dir.path().join("my-hidden-tool");
        std::fs::write(&bin_path, "#!/bin/sh\necho found").unwrap();
        std::fs::set_permissions(&bin_path, std::fs::Permissions::from_mode(0o755)).unwrap();

        // Simulate Spotlight's minimal PATH
        let minimal_path = "/usr/bin:/bin:/usr/sbin:/sbin";

        let output = Command::new("/bin/sh")
            .args(["-c", "my-hidden-tool"])
            .env("PATH", minimal_path)
            .output()
            .unwrap();

        assert!(
            !output.status.success(),
            "subprocess with minimal PATH should NOT find tools outside standard dirs"
        );
    }
}
