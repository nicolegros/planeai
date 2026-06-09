use tauri::{Emitter, State};

use crate::file_explorer;
use crate::state::FileExplorerState;

#[tauri::command]
pub fn fe_list_directory(path: String) -> Result<Vec<file_explorer::DirEntry>, String> {
    file_explorer::list_directory(&path)
}

#[tauri::command]
pub fn fe_create_file(path: String) -> Result<(), String> {
    file_explorer::create_file(&path)
}

#[tauri::command]
pub fn fe_create_directory(path: String) -> Result<(), String> {
    file_explorer::create_directory(&path)
}

#[tauri::command]
pub fn fe_rename_entry(old_path: String, new_path: String) -> Result<(), String> {
    file_explorer::rename_entry(&old_path, &new_path)
}

#[tauri::command]
pub fn fe_delete_to_trash(path: String) -> Result<(), String> {
    file_explorer::delete_to_trash(&path)
}

#[tauri::command]
pub fn fe_watch_directory(
    session_id: String,
    path: String,
    state: State<'_, FileExplorerState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let (tx, rx) = std::sync::mpsc::channel();
    state.0.lock().unwrap().watch(&session_id, &path, tx)?;
    let handle = app.clone();
    std::thread::spawn(move || {
        while let Ok(event) = rx.recv() {
            let _ = handle.emit("fs-change", &event);
        }
    });
    Ok(())
}

#[tauri::command]
pub fn fe_unwatch_directory(
    session_id: String,
    state: State<'_, FileExplorerState>,
) -> Result<(), String> {
    state.0.lock().unwrap().unwatch(&session_id);
    Ok(())
}
