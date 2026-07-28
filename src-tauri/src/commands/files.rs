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

#[tauri::command]
pub async fn read_file(file_path: String) -> Result<String, String> {
    super::blocking(move || {
        let path = std::path::Path::new(&file_path);
        // Reject path traversal: check for ".." as a path component
        if path
            .components()
            .any(|c| c == std::path::Component::ParentDir)
        {
            return Err("Access denied: path traversal detected".to_string());
        }
        let metadata = std::fs::metadata(path).map_err(|e| format!("Cannot read file: {e}"))?;
        if metadata.len() > 10 * 1024 * 1024 {
            return Err("File is too large (>10MB)".to_string());
        }
        let bytes = std::fs::read(path).map_err(|e| format!("Cannot read file: {e}"))?;
        let check_len = bytes.len().min(8192);
        if bytes[..check_len].contains(&0) {
            return Err("Binary file cannot be opened in the editor".to_string());
        }
        String::from_utf8(bytes).map_err(|_| "File is not valid UTF-8".to_string())
    })
    .await
}

#[tauri::command]
pub async fn write_file(file_path: String, content: String) -> Result<(), String> {
    super::blocking(move || {
        let path = std::path::Path::new(&file_path);
        // Reject path traversal: check for ".." as a path component
        if path
            .components()
            .any(|c| c == std::path::Component::ParentDir)
        {
            return Err("Access denied: path traversal detected".to_string());
        }
        std::fs::write(path, &content).map_err(|e| format!("Cannot write file: {e}"))
    })
    .await
}
