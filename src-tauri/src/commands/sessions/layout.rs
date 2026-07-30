use rusqlite::params;
use tauri::State;

use crate::state::DbState;

#[tauri::command]
pub async fn save_session_layout(
    session_id: String,
    layout_json: String,
    db_state: State<'_, DbState>,
) -> Result<(), String> {
    if layout_json.len() > 1_048_576 {
        return Err("layout too large".to_string());
    }
    let conn = db_state.0.clone();
    crate::commands::blocking(move || {
        let conn = conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO session_layouts (session_id, layout_json, updated_at)
             VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(session_id) DO UPDATE SET
               layout_json = excluded.layout_json,
               updated_at = excluded.updated_at",
            params![session_id, layout_json],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn get_session_layout(
    session_id: String,
    db_state: State<'_, DbState>,
) -> Result<Option<String>, String> {
    let conn = db_state.0.clone();
    crate::commands::blocking(move || {
        let conn = conn.lock().map_err(|e| e.to_string())?;
        let result = conn.query_row(
            "SELECT layout_json FROM session_layouts WHERE session_id = ?1",
            params![session_id],
            |row| row.get::<_, String>(0),
        );
        match result {
            Ok(json) => Ok(Some(json)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    })
    .await
}
