use tauri::Manager;

#[tauri::command]
pub fn check_cli_installed() -> Result<bool, String> {
    let cli_target = std::path::Path::new("/usr/local/bin/planeai-cli");
    let daemon_target = std::path::Path::new("/usr/local/bin/planeai-daemon");
    Ok(is_executable(cli_target) && is_executable(daemon_target))
}

#[tauri::command]
pub fn install_cli(app: tauri::AppHandle) -> Result<(), String> {
    let exe_dir = app.path().resource_dir().map_err(|e| e.to_string())?;
    let macos_dir = exe_dir.parent().unwrap_or(&exe_dir).join("MacOS");

    // Resolve CLI binary
    let cli_bin = resolve_binary(&macos_dir, "planeai-cli")?;
    symlink_binary(&cli_bin, "/usr/local/bin/planeai-cli")?;

    // Resolve daemon binary
    let daemon_bin = resolve_binary(&macos_dir, "planeai-daemon")?;
    symlink_binary(&daemon_bin, "/usr/local/bin/planeai-daemon")?;

    Ok(())
}

fn resolve_binary(macos_dir: &std::path::Path, name: &str) -> Result<std::path::PathBuf, String> {
    let primary = macos_dir.join(name);
    if primary.exists() {
        return Ok(primary);
    }
    let current_exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let bin_dir = current_exe.parent().ok_or("cannot determine binary dir")?;
    let alt = bin_dir.join(name);
    if alt.exists() {
        return Ok(alt);
    }
    Err(format!(
        "binary not found: {name} (checked {:?} and {:?})",
        primary, alt
    ))
}

fn symlink_binary(source: &std::path::Path, target_path: &str) -> Result<(), String> {
    let target = std::path::Path::new(target_path);
    if target.exists() || target.symlink_metadata().is_ok() {
        std::fs::remove_file(target)
            .map_err(|e| format!("failed to remove existing symlink at {target_path}: {e}"))?;
    }
    #[cfg(unix)]
    {
        ensure_executable(source)?;
        std::os::unix::fs::symlink(source, target)
            .map_err(|e| format!("failed to create symlink {target_path}: {e}"))
    }
    #[cfg(windows)]
    {
        let _ = source;
        Err(format!(
            "symlink not supported on Windows for {target_path}"
        ))
    }
}

/// Ensure the file at `path` has executable permission bits set.
#[cfg(unix)]
fn ensure_executable(path: &std::path::Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = std::fs::metadata(path)
        .map_err(|e| format!("failed to read metadata for {}: {e}", path.display()))?;
    let mut perms = metadata.permissions();
    let mode = perms.mode();
    if mode & 0o111 == 0 {
        perms.set_mode(mode | 0o111);
        std::fs::set_permissions(path, perms).map_err(|e| {
            format!(
                "failed to set executable permissions on {}: {e}",
                path.display()
            )
        })?;
    }
    Ok(())
}

/// Check whether the given path points to an executable file (follows symlinks).
#[cfg(unix)]
fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(path) {
        Ok(meta) => meta.is_file() && (meta.permissions().mode() & 0o111 != 0),
        Err(_) => false,
    }
}

#[cfg(windows)]
fn is_executable(path: &std::path::Path) -> bool {
    path.is_file()
}
