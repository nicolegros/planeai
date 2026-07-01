use crate::git;
use crate::util::expand_tilde;

use super::blocking;

#[tauri::command]
pub fn validate_git_repo(path: String) -> Result<bool, String> {
    let path = expand_tilde(&path);
    let git_dir = std::path::Path::new(&path).join(".git");
    Ok(git_dir.exists())
}

#[tauri::command]
pub async fn list_branches(repo_path: String) -> Result<Vec<String>, String> {
    blocking(move || git::list_branches(&repo_path)).await
}

#[tauri::command]
pub async fn get_changed_files(
    repo_path: String,
    base_branch: String,
    head_ref: Option<String>,
) -> Result<Vec<git::ChangedFile>, String> {
    blocking(move || git::get_changed_files(&repo_path, &base_branch, head_ref.as_deref())).await
}

#[tauri::command]
pub async fn get_file_diff(
    repo_path: String,
    base_branch: String,
    file_path: String,
    old_path: Option<String>,
    head_ref: Option<String>,
) -> Result<git::FileDiff, String> {
    blocking(move || {
        git::get_file_diff(
            &repo_path,
            &base_branch,
            &file_path,
            old_path.as_deref(),
            head_ref.as_deref(),
        )
    })
    .await
}

#[tauri::command]
pub async fn get_file_patch(
    repo_path: String,
    base_branch: String,
    file_path: String,
    old_path: Option<String>,
    head_ref: Option<String>,
) -> Result<String, String> {
    blocking(move || {
        git::get_file_patch(
            &repo_path,
            &base_branch,
            &file_path,
            old_path.as_deref(),
            head_ref.as_deref(),
        )
    })
    .await
}

#[tauri::command]
pub async fn get_all_file_patches(
    repo_path: String,
    base_branch: String,
    files: Vec<(String, Option<String>)>,
    head_ref: Option<String>,
) -> Result<Vec<String>, String> {
    blocking(move || {
        git::get_all_file_patches(&repo_path, &base_branch, &files, head_ref.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn detect_default_branch(repo_path: String) -> Result<String, String> {
    blocking(move || git::detect_default_branch(&repo_path)).await
}

#[tauri::command]
pub async fn list_commits(repo_path: String, limit: u32) -> Result<Vec<git::CommitEntry>, String> {
    blocking(move || git::list_commits(&repo_path, limit)).await
}
