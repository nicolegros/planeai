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
    head_ref: Option<String>,
) -> Result<Vec<git::ChangedFile>, String> {
    git::get_changed_files(&repo_path, &base_branch, head_ref.as_deref())
}

#[tauri::command]
pub fn get_file_diff(
    repo_path: String,
    base_branch: String,
    file_path: String,
    old_path: Option<String>,
    head_ref: Option<String>,
) -> Result<git::FileDiff, String> {
    git::get_file_diff(
        &repo_path,
        &base_branch,
        &file_path,
        old_path.as_deref(),
        head_ref.as_deref(),
    )
}

#[tauri::command]
pub fn get_file_patch(
    repo_path: String,
    base_branch: String,
    file_path: String,
    old_path: Option<String>,
    head_ref: Option<String>,
) -> Result<String, String> {
    git::get_file_patch(
        &repo_path,
        &base_branch,
        &file_path,
        old_path.as_deref(),
        head_ref.as_deref(),
    )
}

#[tauri::command]
pub async fn get_all_file_patches(
    repo_path: String,
    base_branch: String,
    files: Vec<(String, Option<String>)>,
    head_ref: Option<String>,
) -> Result<Vec<String>, String> {
    tokio::task::spawn_blocking(move || {
        git::get_all_file_patches(&repo_path, &base_branch, &files, head_ref.as_deref())
    })
    .await
    .map_err(|e| format!("task failed: {e}"))?
}

#[tauri::command]
pub fn detect_default_branch(repo_path: String) -> Result<String, String> {
    git::detect_default_branch(&repo_path)
}

#[tauri::command]
pub fn list_commits(repo_path: String, limit: u32) -> Result<Vec<git::CommitEntry>, String> {
    git::list_commits(&repo_path, limit)
}
