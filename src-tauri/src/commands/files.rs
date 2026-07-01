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
pub fn read_file(file_path: String) -> Result<String, String> {
    let path = std::path::Path::new(&file_path);
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
}

#[tauri::command]
pub fn write_file(file_path: String, content: String) -> Result<(), String> {
    std::fs::write(&file_path, &content).map_err(|e| format!("Cannot write file: {e}"))
}
