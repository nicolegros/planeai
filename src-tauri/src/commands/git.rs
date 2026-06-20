use crate::git;
use crate::util::expand_tilde;

#[tauri::command]
pub fn validate_git_repo(path: String) -> Result<bool, String> {
    let path = expand_tilde(&path);
    let git_dir = std::path::Path::new(&path).join(".git");
    Ok(git_dir.exists())
}

#[tauri::command]
pub fn list_branches(repo_path: String) -> Result<Vec<String>, String> {
    git::list_branches(&repo_path)
}

#[tauri::command]
pub fn get_changed_files(
    repo_path: String,
    base_branch: String,
) -> Result<Vec<git::ChangedFile>, String> {
    git::get_changed_files(&repo_path, &base_branch)
}

#[tauri::command]
pub fn get_file_diff(
    repo_path: String,
    base_branch: String,
    file_path: String,
    old_path: Option<String>,
) -> Result<git::FileDiff, String> {
    git::get_file_diff(&repo_path, &base_branch, &file_path, old_path.as_deref())
}

#[tauri::command]
pub fn detect_default_branch(repo_path: String) -> Result<String, String> {
    git::detect_default_branch(&repo_path)
}

#[tauri::command]
pub fn stage_lines(
    repo_path: String,
    file_path: String,
    base_branch: String,
    start_line: u32,
    end_line: u32,
) -> Result<(), String> {
    git::stage_lines(&repo_path, &file_path, &base_branch, start_line, end_line)
}

#[tauri::command]
pub fn revert_lines(
    repo_path: String,
    file_path: String,
    base_branch: String,
    start_line: u32,
    end_line: u32,
) -> Result<(), String> {
    git::revert_lines(&repo_path, &file_path, &base_branch, start_line, end_line)
}
