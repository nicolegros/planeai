#[tauri::command]
pub async fn list_files(repo_path: String) -> Result<Vec<String>, String> {
    super::blocking(move || {
        let mut cmd = std::process::Command::new("git");
        cmd.args(["ls-files"]).current_dir(&repo_path);
        planeai_core::command::no_window(&mut cmd);
        let output = cmd
            .output()
            .map_err(|e| format!("failed to run git ls-files: {e}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).to_string());
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|l| l.to_string())
            .collect())
    })
    .await
}

/// Validate that `file_path` is confined within `repo_path` after canonicalization.
/// Returns the validated canonical path or an error.
fn validate_file_path(file_path: &str, repo_path: &str) -> Result<std::path::PathBuf, String> {
    let path = std::path::Path::new(file_path);
    // Reject path traversal: check for ".." as a path component
    if path
        .components()
        .any(|c| c == std::path::Component::ParentDir)
    {
        return Err("Access denied: path traversal detected".to_string());
    }
    // Canonicalize the repo root (must exist)
    let canon_root =
        std::fs::canonicalize(repo_path).map_err(|e| format!("Invalid repo path: {e}"))?;
    // For read: canonicalize the target (must exist)
    // For write: canonicalize the parent, then append the filename
    if path.exists() {
        let canon_file =
            std::fs::canonicalize(path).map_err(|e| format!("Cannot resolve path: {e}"))?;
        if !canon_file.starts_with(&canon_root) {
            return Err("Access denied: file is outside the project".to_string());
        }
        Ok(canon_file)
    } else {
        // File doesn't exist yet (write case) - canonicalize parent
        let parent = path
            .parent()
            .ok_or_else(|| "Access denied: invalid path".to_string())?;
        let canon_parent =
            std::fs::canonicalize(parent).map_err(|e| format!("Cannot resolve parent: {e}"))?;
        if !canon_parent.starts_with(&canon_root) {
            return Err("Access denied: file is outside the project".to_string());
        }
        let file_name = path
            .file_name()
            .ok_or_else(|| "Access denied: invalid filename".to_string())?;
        Ok(canon_parent.join(file_name))
    }
}

#[tauri::command]
pub async fn read_file(file_path: String, repo_path: String) -> Result<String, String> {
    super::blocking(move || {
        let validated = validate_file_path(&file_path, &repo_path)?;
        let metadata =
            std::fs::metadata(&validated).map_err(|e| format!("Cannot read file: {e}"))?;
        if metadata.len() > 10 * 1024 * 1024 {
            return Err("File is too large (>10MB)".to_string());
        }
        let bytes = std::fs::read(&validated).map_err(|e| format!("Cannot read file: {e}"))?;
        let check_len = bytes.len().min(8192);
        if bytes[..check_len].contains(&0) {
            return Err("Binary file cannot be opened in the editor".to_string());
        }
        String::from_utf8(bytes).map_err(|_| "File is not valid UTF-8".to_string())
    })
    .await
}

#[tauri::command]
pub async fn write_file(file_path: String, content: String, repo_path: String) -> Result<(), String> {
    super::blocking(move || {
        let validated = validate_file_path(&file_path, &repo_path)?;
        std::fs::write(&validated, &content).map_err(|e| format!("Cannot write file: {e}"))
    })
    .await
}
