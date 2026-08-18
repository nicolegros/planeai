#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bench;
mod cleanup;
mod command;
mod commands;
mod config;
mod daemon;
mod daemon_client;
mod db;
mod file_explorer;
mod git;
mod logging;
mod notify;
mod output_observer;
mod paths;
mod plugin_packages;
mod plugins;
mod pr;
mod pty;
mod pty_planeai_core_adapter;
mod session_backend;
mod session_logs;
mod session_ops;
mod session_restart;
mod startup;
mod state;
mod symphony;
mod template;
#[cfg(not(windows))]
mod tmux;
mod updater;
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
    // Raise the file descriptor soft limit on macOS/Linux.
    // The default macOS soft limit is 256, which is easily exhausted by
    // WebView, multiple daemon data connections, and session log files.
    #[cfg(unix)]
    planeai_paths::raise_fd_limit();

    let app_dir = planeai_paths::app_data_dir();
    std::fs::create_dir_all(&app_dir).expect("failed to create app data dir");
    let log_dir = app_dir.join("logs");
    std::fs::create_dir_all(&log_dir).expect("failed to create log dir");
    let _log_guard = logging::init(&log_dir);

    tracing::info!("planeai starting");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
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
            planeai_tasks::sqlite::migrate(&conn).expect("failed to run task migrations");
            planeai_jira::db::migrate(&conn).expect("failed to run jira migrations");
            plugins::migrate(&conn).expect("failed to run plugin runtime migrations");
            let bundled_plugins =
                plugins::bundled_manifests().expect("invalid bundled plugin manifest");
            plugins::sync_inventory(&conn, &bundled_plugins)
                .expect("failed to persist bundled plugin inventory");
            let interrupted_plugins = plugins::reconcile_interrupted_runs(&conn)
                .expect("failed to reconcile plugin runtime state");
            if interrupted_plugins > 0 {
                tracing::warn!(
                    count = interrupted_plugins,
                    "reconciled interrupted plugin runtimes"
                );
            }
            tracing::info!("database initialized");

            // Config: migrate from DB if needed, then load
            let config_dir = config::config_dir(&app.package_info().name);
            if let Ok(settings) = db::get_settings(&conn) {
                let _ = config::migrate_from_db(&config_dir, &settings);
            }
            let (mut cfg, _warnings) = config::load(&config_dir);
            if let Err(error) = config::migrate_legacy_jira_plugin_settings(&config_dir, &app_dir, &mut cfg) {
                tracing::warn!(%error, "failed to migrate legacy Jira configuration into plugin settings");
            }
            if std::env::var("PLANEAI_SESSION_LOG_DIR").is_err() {
                if let Some(ref dir) = cfg.session_log_dir {
                    std::env::set_var("PLANEAI_SESSION_LOG_DIR", dir);
                }
            }
            tracing::info!("config loaded");

            // Revive sessions
            #[cfg(not(windows))]
            let _ = startup::revive_sessions(
                &conn,
                &cfg,
                tmux::has_session,
                tmux::create_session_with_cmd_and_path,
            );
            #[cfg(windows)]
            let _ = startup::revive_sessions(
                &conn,
                &cfg,
                |_| false,
                |_, _, _, _, _| Err("tmux not available".to_string()),
            );

            // Reconcile daemon sessions (mark dead ones as exited)
            startup::reconcile_daemon_sessions(&conn, &cfg);

            // Reconcile local sessions (cannot survive app restart)
            startup::reconcile_local_sessions(&conn);

            // Stale worktree cleanup (fire-and-forget background thread)
            let cleanup_db_path = planeai_paths::db_path();
            std::thread::spawn(move || {
                let conn = match rusqlite::Connection::open(&cleanup_db_path) {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::warn!("stale worktree cleanup: failed to open db: {e}");
                        return;
                    }
                };
                let errors = planeai_core::cleanup::cleanup_stale_worktrees(
                    &conn,
                    |project_path, wt_path| {
                        if !std::path::Path::new(wt_path).exists() {
                            return Ok(());
                        }
                        let _ = planeai_core::git::worktree_remove(project_path, wt_path);
                        if std::path::Path::new(wt_path).exists() {
                            std::fs::remove_dir_all(wt_path).map_err(|e| e.to_string())?;
                        }
                        Ok(())
                    },
                );
                for e in &errors {
                    tracing::warn!("stale worktree cleanup: {e}");
                }
                if errors.is_empty() {
                    tracing::info!("stale worktree cleanup: complete");
                }
            });
            // Legacy Jira integration is intentionally inert while connection
            // ownership lives in the bundled Jira plugin.
            app.manage(ConfigState(Mutex::new(cfg)));

            // Daemon state (lazily connects to daemon)
            app.manage(DaemonState(tokio::sync::Mutex::new(None)));

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
                let _ = std::fs::write(&path, content);
            }

            let db_arc = Arc::new(Mutex::new(conn));
            app.manage(DbState(db_arc.clone()));
            app.manage(plugins::PluginRuntimeHandle::new(
                db_arc.clone(),
                app.handle().clone(),
            ));
            let plugin_runtime = app.state::<plugins::PluginRuntimeHandle>().0.clone();
            tauri::async_runtime::spawn(async move {
                plugin_runtime.start_enabled().await;
            });

            // Notification system
            let notify_state: notify::SharedNotifyState =
                Arc::new(Mutex::new(notify::NotifyState::new()));
            notify::start_socket_listener(&app_dir, notify_state.clone(), app.handle().clone());
            notify::start_silence_checker(notify_state.clone(), app.handle().clone());
            app.manage(NotifyHandle(notify_state.clone()));

            // Register active sessions in NotifyState (must happen after notify_state exists)
            {
                let conn = db_arc.lock().unwrap();
                let cfg_state = app.state::<ConfigState>();
                let cfg = cfg_state.0.lock().unwrap();
                startup::register_active_sessions(&conn, &cfg, &notify_state);
            }

            // Refresh hook scripts to latest bundled version (idempotent, only updates
            // scripts for hooks that are already installed on the user's system).
            planeai_core::notify::refresh_hook_scripts(&config::home_dir());

            // PTY manager with notify wired in
            let pty_mgr = pty::PtyManager::new();
            pty_mgr.set_observer(Arc::new(notify::NotifyObserver::new(
                notify_state.clone(),
                app.handle().clone(),
            )));
            app.manage(PtyState(pty_mgr));
            app.manage(FileExplorerState(Mutex::new(
                file_explorer::WatcherManager::new(),
            )));

            // Warm font cache in background
            startup::warm_font_cache();

            // PR status background poll
            startup::start_pr_poller(app.handle());

            // Daemon exit event listener
            startup::start_daemon_event_listener(app.handle());

            // Symphony orchestrator
            let symphony_state = startup::init_symphony(app, &app_dir, &db_arc);
            app.manage(SymphonyHandle(Mutex::new(symphony_state)));

            // Auto-update check (fire-and-forget on startup)
            updater::check_for_updates(app.handle());

            tracing::info!("app setup complete");

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            create_project,
            list_projects,
            list_archived_projects,
            archive_project,
            restore_project,
            hide_project,
            unhide_project,
            get_project_auto_mode,
            set_project_auto_mode,
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
            get_file_patch,
            get_all_file_patches,
            get_combined_patch,
            detect_default_branch,
            clone_repository,
            list_commits,
            list_files,
            read_file,
            write_file,
            list_monospace_fonts,
            get_config,
            get_log_dir,
            update_config,
            refresh_config,
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
            increment_tab_count,
            check_tmux_available,
            save_session_layout,
            get_session_layout,
            restart_session,
            archive_session,
            destroy_session,
            get_task_details,
            list_task_items,
            list_all_task_items,
            create_task_item,
            edit_task_item,
            move_task_item,
            fire_task_notify_hook,
            fe_list_directory,
            fe_list_all_paths,
            fe_create_file,
            fe_create_directory,
            fe_rename_entry,
            fe_delete_to_trash,
            fe_watch_directory,
            fe_unwatch_directory,
            fetch_pr_url,
            create_pr,
            generate_pr_defaults,
            get_ci_checks,
            get_ci_failure_logs,
            get_pr_comments,
            get_allowed_merge_strategies,
            get_merge_conflict_status,
            get_pr_status,
            merge_pr,
            mark_pr_ready,
            get_merge_state,
            link_pr_url,
            check_cli_installed,
            install_cli,
            list_stale_worktrees,
            run_stale_worktree_cleanup,
            get_symphony_status,
            bench::bench_replay_file,
            bench::bench_fixture_info,
            bench::bench_write_metrics,
            bench::bench_write_snapshot,
            bench::bench_get_config,
            session_logs::get_session_log_dir,
            session_logs::list_session_logs,
            session_logs::get_session_log_metadata,
            session_logs::read_session_log_chunk,
            session_logs::open_session_log_folder,
            session_logs::delete_session_log,
            session_logs::is_dogfood_log_viewer_enabled,
            list_plugins,
            install_local_plugin,
            remove_local_plugin,
            plugin_call,
            plugin_settings,
            update_plugin_settings,
            local_plugin_ui_source,
            plugin_data_changed,
            enable_plugin,
            disable_plugin,
            reload_plugin,
            list_loop_runs,
            get_loop_run_detail,
            list_loop_recipes,
            create_loop_run,
            start_loop,
            tick_loop,
            stop_loop,
            delete_loop,
            updater::install_update,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { code, api, .. } = event {
                if code == Some(tauri::RESTART_EXIT_CODE) {
                    tracing::warn!(
                        "restart requested; plugin runtimes cannot be gracefully stopped"
                    );
                    return;
                }
                let runtime = app.state::<plugins::PluginRuntimeHandle>().0.clone();
                if runtime.exit_is_permitted() {
                    return;
                }
                api.prevent_exit();
                if runtime.begin_shutdown() {
                    let app_handle = app.clone();
                    tauri::async_runtime::spawn(async move {
                        runtime.shutdown_all().await;
                        runtime.permit_exit();
                        app_handle.exit(0);
                    });
                }
            }
        });
}
