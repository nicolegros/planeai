use tauri::State;

use crate::config;
use crate::state::ConfigState;
use crate::terminal_scheme;

#[tauri::command]
pub fn refresh_config(
    state: State<ConfigState>,
    app: tauri::AppHandle,
) -> Result<config::Config, String> {
    let config_dir = config::config_dir(&app.package_info().name);
    tracing::info!("refreshing config from disk");
    let new_config = config::refresh(&config_dir).map_err(|e| {
        tracing::warn!("config refresh failed: {e}");
        e
    })?;
    let mut cfg = state.0.lock().map_err(|e| e.to_string())?;
    *cfg = new_config.clone();
    tracing::info!("config refreshed successfully");
    Ok(new_config)
}

#[tauri::command]
pub fn get_config(state: State<ConfigState>) -> Result<config::Config, String> {
    let cfg = state.0.lock().map_err(|e| e.to_string())?;
    Ok(cfg.clone())
}

/// Built-in defaults, so the UI can offer "reset to default" without duplicating them.
#[tauri::command]
pub async fn get_config_defaults() -> config::Config {
    config::Config::default()
}

/// Resolved binary per agent (configured providers plus presets), `None` when not found.
#[tauri::command]
pub async fn detect_providers(
    state: State<'_, ConfigState>,
) -> Result<std::collections::HashMap<String, Option<String>>, String> {
    let cfg = state.0.lock().map_err(|e| e.to_string())?.clone();
    crate::commands::blocking(move || Ok(config::detect_provider_binaries(&cfg))).await
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
    config::validate(&new_config)?;
    let config_dir = config::config_dir(&app.package_info().name);
    config::save(&config_dir, &new_config)?;
    let mut cfg = state.0.lock().map_err(|e| e.to_string())?;
    *cfg = new_config;
    Ok(())
}

#[tauri::command]
pub async fn get_theme_css(
    state: State<'_, ConfigState>,
    app: tauri::AppHandle,
) -> Result<String, String> {
    let theme = state
        .0
        .lock()
        .map_err(|e| e.to_string())?
        .appearance
        .theme
        .clone();
    let themes_dir = config::config_dir(&app.package_info().name).join("themes");
    crate::commands::blocking(move || match theme {
        config::ThemeChoice::Css(name) => {
            std::fs::read_to_string(themes_dir.join(format!("{name}.css")))
                .map_err(|e| e.to_string())
        }
        config::ThemeChoice::Schemes { light, dark } => {
            let user_dir = themes_dir.join("ghostty");
            let find = |name: &str| {
                terminal_scheme::find_scheme(&user_dir, name).map_err(|e| e.to_string())
            };
            Ok(terminal_scheme::schemes_css(&find(&light)?, &find(&dark)?))
        }
    })
    .await
}

#[tauri::command]
pub async fn list_themes(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    let themes_dir = config::config_dir(&app.package_info().name).join("themes");
    crate::commands::blocking(move || {
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
    })
    .await
}

/// Bundled Ghostty color schemes plus the user's own from `<config_dir>/themes/ghostty/`.
#[tauri::command]
pub async fn list_terminal_schemes(
    app: tauri::AppHandle,
) -> Result<Vec<terminal_scheme::SchemeSummary>, String> {
    let user_dir = config::config_dir(&app.package_info().name)
        .join("themes")
        .join("ghostty");
    crate::commands::blocking(move || Ok(terminal_scheme::list_schemes(&user_dir))).await
}

#[tauri::command]
pub fn get_log_dir() -> String {
    planeai_paths::app_data_dir()
        .join("logs")
        .to_string_lossy()
        .into_owned()
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
