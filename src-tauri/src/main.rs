#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cleanup;
mod command;
mod commands;
mod config;
mod db;
mod file_explorer;
mod git;
mod notify;
mod pr;
mod pty;
mod session_ops;
mod startup;
mod state;
mod symphony;
mod task_manager;
mod template;
#[cfg(not(windows))]
mod tmux;
mod util;

use rusqlite::Connection;
use std::sync::{Arc, Mutex};
use tauri::{
    menu::{Menu, PredefinedMenuItem, Submenu},
    Manager,
};

use commands::*;
use state::*;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let menu = Menu::with_items(
                app,
                &[
                    &Submenu::with_items(
                        app,
                        "planeai",
                        true,
                        &[
                            &PredefinedMenuItem::about(app, None, None)?,
                            &PredefinedMenuItem::separator(app)?,
                            &PredefinedMenuItem::hide(app, None)?,
                            &PredefinedMenuItem::hide_others(app, None)?,
                            &PredefinedMenuItem::show_all(app, None)?,
                            &PredefinedMenuItem::separator(app)?,
                            &PredefinedMenuItem::quit(app, None)?,
                        ],
                    )?,
                    &Submenu::with_items(
                        app,
                        "Edit",
                        true,
                        &[
                            &PredefinedMenuItem::undo(app, None)?,
                            &PredefinedMenuItem::redo(app, None)?,
                            &PredefinedMenuItem::separator(app)?,
                            &PredefinedMenuItem::cut(app, None)?,
                            &PredefinedMenuItem::copy(app, None)?,
                            &PredefinedMenuItem::paste(app, None)?,
                            &PredefinedMenuItem::select_all(app, None)?,
                        ],
                    )?,
                    &Submenu::with_items(
                        app,
                        "Window",
                        true,
                        &[
                            &PredefinedMenuItem::minimize(app, None)?,
                            &PredefinedMenuItem::maximize(app, None)?,
                            &PredefinedMenuItem::fullscreen(app, None)?,
                        ],
                    )?,
                ],
            )?;
            app.set_menu(menu)?;

            let app_dir = app
                .path()
                .app_data_dir()
                .expect("failed to get app data dir");
            std::fs::create_dir_all(&app_dir).expect("failed to create app data dir");
            let db_path = app_dir.join("planeai.db");
            let conn = Connection::open(db_path).expect("failed to open database");
            db::migrate(&conn).expect("failed to run migrations");

            // Config: migrate from DB if needed, then load
            let config_dir = config::config_dir(&app.package_info().name);
            if let Ok(settings) = db::get_settings(&conn) {
                let _ = config::migrate_from_db(&config_dir, &settings);
            }
            let (cfg, _warnings) = config::load(&config_dir);

            // Revive sessions
            #[cfg(not(windows))]
            let _ = startup::revive_sessions(
                &conn,
                &cfg,
                tmux::has_session,
                tmux::create_session_with_cmd,
            );
            #[cfg(windows)]
            let _ = startup::revive_sessions(
                &conn,
                &cfg,
                |_| false,
                |_, _, _, _| Err("tmux not available".to_string()),
            );

            app.manage(ConfigState(Mutex::new(cfg)));

            // Scaffold themes dir with bundled themes if missing
            let themes_dir = config_dir.join("themes");
            let _ = std::fs::create_dir_all(&themes_dir);
            let bundled_themes: &[(&str, &str)] = &[
                (
                    "default.css",
                    include_str!("../resources/themes/default.css"),
                ),
                ("github.css", include_str!("../resources/themes/github.css")),
                ("one.css", include_str!("../resources/themes/one.css")),
                (
                    "catppuccin.css",
                    include_str!("../resources/themes/catppuccin.css"),
                ),
                (
                    "dracula.css",
                    include_str!("../resources/themes/dracula.css"),
                ),
            ];
            for (name, content) in bundled_themes {
                let path = themes_dir.join(name);
                if !path.exists() {
                    let _ = std::fs::write(&path, content);
                }
            }

            let db_arc = Arc::new(Mutex::new(conn));
            app.manage(DbState(db_arc.clone()));

            // Notification system
            let notify_state: notify::SharedNotifyState =
                Arc::new(Mutex::new(notify::NotifyState::new()));
            notify::start_socket_listener(&app_dir, notify_state.clone(), app.handle().clone());
            notify::start_silence_checker(notify_state.clone(), app.handle().clone());
            app.manage(NotifyHandle(notify_state.clone()));

            // PTY manager with notify wired in
            let pty_mgr = pty::PtyManager::new();
            pty_mgr.set_notify_state(notify_state);
            #[cfg(unix)]
            pty_mgr.set_socket_path(notify::socket_path(&app_dir).to_string_lossy().into_owned());
            #[cfg(windows)]
            pty_mgr.set_socket_path(notify::PIPE_NAME.to_string());
            app.manage(PtyState(pty_mgr));
            app.manage(FileExplorerState(Mutex::new(
                file_explorer::WatcherManager::new(),
            )));

            // Warm font cache in background
            startup::warm_font_cache();

            // PR status background poll
            startup::start_pr_poller(app.handle());

            // Symphony orchestrator
            let symphony_state = startup::init_symphony(app, &app_dir, &db_arc);
            app.manage(SymphonyHandle(Mutex::new(symphony_state)));

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            create_project,
            list_projects,
            list_archived_projects,
            archive_project,
            restore_project,
            get_project_auto_mode,
            set_project_auto_mode,
            set_project_task_manager,
            delete_project,
            create_session,
            list_sessions,
            delete_session,
            rename_session,
            list_archived_sessions,
            restore_session,
            validate_git_repo,
            list_branches,
            get_changed_files,
            get_file_diff,
            detect_default_branch,
            list_files,
            read_file,
            write_file,
            list_monospace_fonts,
            get_config,
            update_config,
            get_theme_css,
            list_themes,
            launch_session,
            attach_session,
            write_to_pty,
            resize_pty,
            pause_pty,
            resume_pty,
            check_session_alive,
            is_notify_hook_installed,
            install_notify_hook,
            acknowledge_session,
            mark_exited,
            save_mru_order,
            spawn_tab,
            close_tab,
            check_tmux_available,
            restart_session,
            archive_session,
            destroy_session,
            get_task_details,
            list_task_items,
            fire_task_notify_hook,
            fe_list_directory,
            fe_create_file,
            fe_create_directory,
            fe_rename_entry,
            fe_delete_to_trash,
            fe_watch_directory,
            fe_unwatch_directory,
            fetch_pr_url,
            check_cli_installed,
            install_cli,
            get_symphony_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
