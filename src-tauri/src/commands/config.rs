use tauri::State;

use crate::config;
use crate::state::ConfigState;

#[tauri::command]
pub fn get_config(state: State<ConfigState>) -> Result<config::Config, String> {
    let cfg = state.0.lock().map_err(|e| e.to_string())?;
    Ok(cfg.clone())
}

#[tauri::command]
pub fn update_config(
    state: State<ConfigState>,
    mut new_config: config::Config,
    app: tauri::AppHandle,
) -> Result<(), String> {
    if let Some(ref raw) = new_config.projects_base_path {
        let normalized = config::normalize_base_path(raw);
        new_config.projects_base_path = if normalized.is_empty() {
            None
        } else {
            Some(normalized)
        };
    }
    let config_dir = config::config_dir(&app.package_info().name);
    config::save(&config_dir, &new_config)?;
    let mut cfg = state.0.lock().map_err(|e| e.to_string())?;
    *cfg = new_config;
    Ok(())
}

#[tauri::command]
pub fn get_theme_css(state: State<ConfigState>, app: tauri::AppHandle) -> Result<String, String> {
    let cfg = state.0.lock().map_err(|e| e.to_string())?;
    let theme_name = &cfg.appearance.theme;
    let config_dir = config::config_dir(&app.package_info().name);
    let theme_path = config_dir
        .join("themes")
        .join(format!("{}.css", theme_name));
    std::fs::read_to_string(&theme_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_themes(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    let config_dir = config::config_dir(&app.package_info().name);
    let themes_dir = config_dir.join("themes");
    let mut names = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&themes_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "css") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    names.push(stem.to_string());
                }
            }
        }
    }
    names.sort();
    Ok(names)
}

#[tauri::command]
pub async fn list_monospace_fonts() -> Result<Vec<String>, String> {
    use font_kit::family_name::FamilyName;
    use font_kit::properties::Properties;
    use font_kit::source::SystemSource;
    use std::sync::OnceLock;

    static CACHE: OnceLock<Vec<String>> = OnceLock::new();

    Ok(CACHE
        .get_or_init(|| {
            let source = SystemSource::new();
            let all_families = source.all_families().unwrap_or_default();

            let mut fonts: Vec<String> = all_families
                .into_iter()
                .filter(|name| !name.starts_with('.'))
                .filter(|name| {
                    source
                        .select_best_match(&[FamilyName::Title(name.clone())], &Properties::new())
                        .ok()
                        .and_then(|handle| handle.load().ok())
                        .map(|font| font.is_monospace())
                        .unwrap_or(false)
                })
                .collect();
            fonts.sort();
            fonts.dedup();
            fonts
        })
        .clone())
}
