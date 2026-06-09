use tauri::Manager;

#[tauri::command]
pub fn check_cli_installed() -> Result<bool, String> {
    let target = std::path::Path::new("/usr/local/bin/planeai-cli");
    Ok(target.exists())
}

#[tauri::command]
pub fn install_cli(app: tauri::AppHandle) -> Result<(), String> {
    let exe_dir = app.path().resource_dir().map_err(|e| e.to_string())?;
    let cli_bin = exe_dir
        .parent()
        .unwrap_or(&exe_dir)
        .join("MacOS")
        .join("planeai-cli");
    if !cli_bin.exists() {
        let current_exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let bin_dir = current_exe.parent().ok_or("cannot determine binary dir")?;
        let alt = bin_dir.join("planeai-cli");
        if !alt.exists() {
            return Err(format!(
                "CLI binary not found at {:?} or {:?}",
                cli_bin, alt
            ));
        }
        return symlink_cli(&alt);
    }
    symlink_cli(&cli_bin)
}

fn symlink_cli(source: &std::path::Path) -> Result<(), String> {
    let target = std::path::Path::new("/usr/local/bin/planeai-cli");
    if target.exists() || target.symlink_metadata().is_ok() {
        std::fs::remove_file(target)
            .map_err(|e| format!("failed to remove existing symlink: {e}"))?;
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(source, target)
            .map_err(|e| format!("failed to create symlink: {e}"))
    }
    #[cfg(windows)]
    {
        Err("symlink_cli not supported on Windows".to_string())
    }
}
