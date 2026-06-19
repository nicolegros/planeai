//! PlaneAI Workflow Shell — orchestrates daemon sessions with project context.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use planeai_core::services::{
    self, CreateSessionParams, ProjectService, SessionRecord, SessionService, WorktreeMode,
    WorktreeService,
};
use rusqlite::Connection;
use std::sync::Mutex;

use alacritty_terminal::vte::ansi::Processor;
use arboard::Clipboard;
use iced::keyboard;
use iced::widget::canvas::{self, Cache, Program};
use iced::widget::{column, container, row, text, text_input, Canvas};
use iced::{
    event, window, Color, Element, Font, Length, Point, Rectangle, Renderer, Size, Subscription,
    Theme,
};

use crate::adapter::PlaneAiTerminalSession;
use crate::common::*;
use crate::daemon_session::{
    attach, daemon_is_connected, detach_daemon_session, ensure_daemon_running_sync,
    kill_daemon_session, list_daemon_sessions, DaemonSession, DaemonSessionInfo,
};
use crate::input;
use crate::Args;

// ─── Recent projects ─────────────────────────────────────────────────────────

const MAX_RECENT_PROJECTS: usize = 20;

fn recent_projects_path() -> PathBuf {
    planeai_core::session_launch::config_dir().join("recent_projects.json")
}

fn load_recent_projects() -> Vec<String> {
    let path = recent_projects_path();
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

fn save_recent_projects(projects: &[String]) {
    let path = recent_projects_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let json = serde_json::to_string_pretty(projects).unwrap_or_default();
    let _ = std::fs::write(&path, json);
}

fn add_recent_project(path_str: &str) -> Vec<String> {
    let mut projects = load_recent_projects();
    // Deduplicate
    projects.retain(|p| p != path_str);
    // Insert at front
    projects.insert(0, path_str.to_string());
    // Cap at max
    projects.truncate(MAX_RECENT_PROJECTS);
    save_recent_projects(&projects);
    projects
}

// ─── Session state ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
enum SessionStatus {
    Running,
    Attached,
    Exited,
    Detached,
    Unreachable,
    Killed,
}

#[allow(dead_code)]
struct Session {
    id: usize,
    session_id: String,
    command: String,
    cwd: PathBuf,
    status: SessionStatus,
    backend: Box<dyn PlaneAiTerminalSession>,
    term: alacritty_terminal::Term<EventProxy>,
    processor: Processor,
    snapshot: GridSnapshot,
    cache: Cache,
    bytes_processed: u64,
    log_file_exists: bool,
}

// ─── Log replay state ────────────────────────────────────────────────────────

#[allow(dead_code)]
struct LogReplayState {
    term: alacritty_terminal::Term<EventProxy>,
    snapshot: GridSnapshot,
    cache: Cache,
    session_id: String,
}

// ─── App state ───────────────────────────────────────────────────────────────

struct WorkflowApp {
    sessions: Vec<Session>,
    active: usize,
    project_cwd: PathBuf,
    agent_command: String,
    provider_label: String,
    extra_path_dirs: Vec<String>,
    cols: usize,
    rows: usize,
    daemon_connected: bool,
    daemon_sessions_listed: Vec<DaemonSessionInfo>,
    last_health_check: Option<Instant>,
    // Project picker
    picking_project: bool,
    project_input: String,
    recent_projects: Vec<String>,
    // Status/error
    last_error: Option<String>,
    error_time: Option<Instant>,
    // Shortcuts overlay
    show_shortcuts: bool,
    // Session counter for unique ids
    next_id: usize,
    // Log replay
    log_replay: Option<LogReplayState>,
    // Launch prompt (Cmd+Shift+N)
    launch_prompt: bool,
    launch_prompt_input: String,
    // Kill confirmation (two-press)
    kill_armed: bool,
    // DB persistence
    db: Option<Arc<Mutex<Connection>>>,
    project: Option<services::Project>,
    persisted_sessions: Vec<SessionRecord>,
    // Worktree launch mode
    worktree_prompt: bool,
    worktree_branch_input: String,
    worktree_task_key_input: String,
    worktree_use_worktree: bool,
    worktree_computed_path: Option<String>,
    worktree_error: Option<String>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
enum Message {
    Poll,
    KeyEvent(keyboard::Event),
    WindowResized(Size),
    ProjectInputChanged(String),
    ProjectInputSubmit,
    LaunchPromptChanged(String),
    LaunchPromptSubmit,
    WorktreeBranchChanged(String),
    WorktreeTaskKeyChanged(String),
    WorktreeToggle,
    WorktreeLaunchSubmit,
}

impl WorkflowApp {
    fn boot() -> (Self, iced::Task<Message>) {
        let args = WORKFLOW_ARGS.get().unwrap();

        let mut boot_warnings: Vec<String> = Vec::new();

        let config = if let Some(ref path) = args.config {
            match planeai_core::session_launch::load_launch_config(path) {
                Ok(c) => c,
                Err(e) => {
                    boot_warnings.push(format!("Config load failed: {e} — using defaults"));
                    planeai_core::session_launch::LaunchConfig::default()
                }
            }
        } else {
            planeai_core::session_launch::load_default_config()
        };

        let overrides = planeai_core::session_launch::SessionLaunchOverrides {
            cwd: args.cwd.clone(),
            agent_command: args.agent_command.clone(),
            extra_path_dirs: args.extra_path_dirs.clone(),
            cols: Some(args.cols as u16),
            rows: Some(args.rows as u16),
            ..Default::default()
        };
        let resolved = match planeai_core::session_launch::resolve_from_config(&config, &overrides)
        {
            Ok(r) => r,
            Err(e) => {
                boot_warnings.push(format!("Config resolve error: {e} — using defaults"));
                let fallback_config = planeai_core::session_launch::LaunchConfig::default();
                planeai_core::session_launch::resolve_from_config(&fallback_config, &overrides)
                    .unwrap()
            }
        };

        let project_cwd = resolved.request.project_cwd.clone();

        // Open shared PlaneAI DB and ensure project record exists
        let (db, project) = match services::open_db() {
            Ok(conn) => {
                let proj =
                    ProjectService::ensure_project(&conn, &project_cwd.to_string_lossy()).ok();
                (Some(Arc::new(Mutex::new(conn))), proj)
            }
            Err(e) => {
                boot_warnings.push(format!("DB open failed: {e} — sessions won't persist"));
                (None, None)
            }
        };

        let persisted_sessions = if let (Some(ref db), Some(ref proj)) = (&db, &project) {
            let conn = db.lock().unwrap();
            SessionService::list_for_project(&conn, &proj.id).unwrap_or_default()
        } else {
            Vec::new()
        };

        // Stale worktree cleanup (fire-and-forget background thread)
        if let Some(ref db) = db {
            let db_clone = db.clone();
            std::thread::spawn(move || {
                let conn = db_clone.lock().unwrap();
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
                    eprintln!("[stale worktree cleanup] {e}");
                }
            });
        }

        let agent_command = resolved.command_label.clone();
        let extra_path_dirs = resolved.request.extra_path_dirs.clone();
        let provider_label = resolved.provider_label.clone().unwrap_or_default();
        let cols = resolved.request.cols as usize;
        let rows = resolved.request.rows as usize;

        if let Some(ref dir) = resolved.session_log_dir {
            if std::env::var("PLANEAI_SESSION_LOG_DIR").is_err() {
                std::env::set_var("PLANEAI_SESSION_LOG_DIR", dir);
            }
        }

        let daemon_connected = match ensure_daemon_running_sync() {
            Ok(()) => daemon_is_connected(),
            Err(_) => false,
        };

        let daemon_sessions_listed = if daemon_connected {
            list_daemon_sessions().unwrap_or_default()
        } else {
            Vec::new()
        };

        // Add cwd to recent projects
        let recent_projects = add_recent_project(&project_cwd.to_string_lossy());

        let mut result = (
            Self {
                sessions: Vec::new(),
                active: 0,
                project_cwd,
                agent_command,
                provider_label,
                extra_path_dirs,
                cols,
                rows,
                daemon_connected,
                daemon_sessions_listed,
                last_health_check: Some(Instant::now()),
                picking_project: false,
                project_input: String::new(),
                recent_projects,
                last_error: None,
                error_time: None,
                show_shortcuts: false,
                next_id: 0,
                log_replay: None,
                launch_prompt: false,
                launch_prompt_input: String::new(),
                kill_armed: false,
                db,
                project,
                persisted_sessions,
                worktree_prompt: false,
                worktree_branch_input: String::new(),
                worktree_task_key_input: String::new(),
                worktree_use_worktree: false,
                worktree_computed_path: None,
                worktree_error: None,
            },
            iced::Task::none(),
        );
        // Surface boot warnings visibly
        if !boot_warnings.is_empty() {
            result.0.set_error(boot_warnings.join(" | "));
        }
        result
    }

    fn launch_session(&mut self) {
        if self.agent_command.is_empty() {
            self.set_error("No provider command configured. Use --agent-command or config.".into());
            return;
        }
        if !self.daemon_connected {
            self.set_error("Daemon unavailable. Cannot launch session.".into());
            return;
        }
        // Show command in status before launch
        self.clear_error();
        let id = self.next_id;
        self.next_id += 1;

        // Step 1: Reserve session ID and persist DB record BEFORE spawning
        let session_id = uuid::Uuid::new_v4().to_string();
        let persist_err = self.persist_new_session(&session_id, &self.provider_label.clone());
        if let Some(msg) = persist_err {
            self.set_error(msg);
            return;
        }

        // Step 2: Spawn daemon session using the preallocated session ID
        let result = DaemonSession::spawn_with_session_id(
            id,
            &session_id,
            self.cols as u16,
            self.rows as u16,
            Some(&self.agent_command),
            &self.project_cwd,
            &self.extra_path_dirs,
        );
        match result {
            Ok(backend) => {
                let term = new_term(self.cols, self.rows);
                let processor = new_processor();
                let snapshot = snapshot_grid(&term);
                let log_file_exists = self.check_log_exists(&session_id);
                self.sessions.push(Session {
                    id,
                    session_id: session_id.clone(),
                    command: self.agent_command.clone(),
                    cwd: self.project_cwd.clone(),
                    status: SessionStatus::Running,
                    backend: Box::new(backend),
                    term,
                    processor,
                    snapshot,
                    cache: Cache::new(),
                    bytes_processed: 0,
                    log_file_exists,
                });
                self.active = self.sessions.len() - 1;
                self.refresh_persisted_sessions();
            }
            Err(e) => {
                // Step 3: Spawn failed — mark DB record as destroyed to prevent orphan
                if let Some(ref db) = self.db {
                    if let Ok(conn) = db.lock() {
                        let _ = SessionService::set_status(&conn, &session_id, "destroyed");
                    }
                }
                self.set_error(format!("Launch failed: {}", e));
                self.refresh_persisted_sessions();
            }
        }
    }

    fn launch_session_with_command(&mut self, command: &str) {
        if command.is_empty() {
            self.set_error("Command cannot be empty.".into());
            return;
        }
        if !self.daemon_connected {
            self.set_error("Daemon unavailable. Cannot launch session.".into());
            return;
        }
        let id = self.next_id;
        self.next_id += 1;

        // Step 1: Reserve session ID and persist DB record BEFORE spawning
        let session_id = uuid::Uuid::new_v4().to_string();
        let persist_err = self.persist_new_session(&session_id, command);
        if let Some(msg) = persist_err {
            self.set_error(msg);
            return;
        }

        // Step 2: Spawn daemon session using the preallocated session ID
        let result = DaemonSession::spawn_with_session_id(
            id,
            &session_id,
            self.cols as u16,
            self.rows as u16,
            Some(command),
            &self.project_cwd,
            &self.extra_path_dirs,
        );
        match result {
            Ok(backend) => {
                let term = new_term(self.cols, self.rows);
                let processor = new_processor();
                let snapshot = snapshot_grid(&term);
                let log_file_exists = self.check_log_exists(&session_id);
                self.sessions.push(Session {
                    id,
                    session_id: session_id.clone(),
                    command: command.to_string(),
                    cwd: self.project_cwd.clone(),
                    status: SessionStatus::Running,
                    backend: Box::new(backend),
                    term,
                    processor,
                    snapshot,
                    cache: Cache::new(),
                    bytes_processed: 0,
                    log_file_exists,
                });
                self.active = self.sessions.len() - 1;
                self.clear_error();
                self.refresh_persisted_sessions();
            }
            Err(e) => {
                // Step 3: Spawn failed — mark DB record as destroyed to prevent orphan
                if let Some(ref db) = self.db {
                    if let Ok(conn) = db.lock() {
                        let _ = SessionService::set_status(&conn, &session_id, "destroyed");
                    }
                }
                self.set_error(format!("Launch failed: {}", e));
                self.refresh_persisted_sessions();
            }
        }
    }

    fn attach_session(&mut self, session_id: String) {
        let id = self.next_id;
        self.next_id += 1;
        let result = attach(id, &session_id, self.cols as u16, self.rows as u16);
        match result {
            Ok(backend) => {
                let term = new_term(self.cols, self.rows);
                let processor = new_processor();
                let snapshot = snapshot_grid(&term);
                let log_file_exists = self.check_log_exists(&session_id);
                self.sessions.push(Session {
                    id,
                    session_id,
                    command: "attached".to_string(),
                    cwd: self.project_cwd.clone(),
                    status: SessionStatus::Attached,
                    backend: Box::new(backend),
                    term,
                    processor,
                    snapshot,
                    cache: Cache::new(),
                    bytes_processed: 0,
                    log_file_exists,
                });
                self.active = self.sessions.len() - 1;
                self.clear_error();
            }
            Err(e) => {
                self.set_error(format!("Attach failed: {}", e));
            }
        }
    }

    fn detach_active(&mut self) {
        if self.sessions.is_empty() {
            return;
        }
        let session = &self.sessions[self.active];
        let _ = detach_daemon_session(&session.session_id);
        // Update DB status on detach (session still alive in daemon)
        if let Some(ref db) = self.db {
            if let Ok(conn) = db.lock() {
                let _ = SessionService::set_status(&conn, &session.session_id, "active");
            }
        }
        self.sessions.remove(self.active);
        if !self.sessions.is_empty() && self.active >= self.sessions.len() {
            self.active = self.sessions.len() - 1;
        }
        self.refresh_daemon_list();
    }

    fn kill_active(&mut self) {
        if self.sessions.is_empty() {
            return;
        }
        if !self.kill_armed {
            self.kill_armed = true;
            self.set_error("Kill armed. Press Cmd+Shift+W again to confirm.".into());
            return;
        }
        self.kill_armed = false;
        let session = &mut self.sessions[self.active];
        let _ = kill_daemon_session(&session.session_id);
        session.status = SessionStatus::Killed;

        // Clean up worktree if this session used one — fetch record directly by session_id
        let session_id_for_cleanup = session.session_id.clone();
        if let Some(ref db) = self.db {
            if let Ok(conn) = db.lock() {
                if let Ok(Some(rec)) = SessionService::get(&conn, &session_id_for_cleanup) {
                    if let Some(ref wt_path) = rec.worktree_path {
                        if let Ok(Some(proj)) = ProjectService::get_by_id(&conn, &rec.project_id) {
                            let branch = if rec.branch.is_empty() {
                                None
                            } else {
                                Some(rec.branch.as_str())
                            };
                            let errors = planeai_core::cleanup::cleanup_worktree(
                                &proj.path, wt_path, branch,
                            );
                            if !errors.is_empty() {
                                tracing::warn!(errors = ?errors, "worktree cleanup errors");
                            }
                        } else {
                            tracing::error!(
                                session_id = %session_id_for_cleanup,
                                project_id = %rec.project_id,
                                "cannot resolve project for worktree cleanup — skipping"
                            );
                        }
                    }
                }
            }
        }

        // Update DB status
        if let Some(ref db) = self.db {
            if let Ok(conn) = db.lock() {
                let _ = SessionService::set_status(&conn, &session.session_id, "destroyed");
            }
        }
        self.sessions.remove(self.active);
        if !self.sessions.is_empty() && self.active >= self.sessions.len() {
            self.active = self.sessions.len() - 1;
        }
        self.refresh_daemon_list();
    }

    fn refresh_daemon_list(&mut self) {
        self.daemon_connected = daemon_is_connected();
        if self.daemon_connected {
            self.daemon_sessions_listed = list_daemon_sessions().unwrap_or_default();
        }
    }

    fn refresh_persisted_sessions(&mut self) {
        if let (Some(ref db), Some(ref project)) = (&self.db, &self.project) {
            if let Ok(conn) = db.lock() {
                self.persisted_sessions =
                    SessionService::list_for_project(&conn, &project.id).unwrap_or_default();
            }
        }
    }

    /// Persist a new session record to the shared DB. Returns Some(error_msg) on failure.
    fn persist_new_session(&self, session_id: &str, name: &str) -> Option<String> {
        if let (Some(ref db), Some(ref project)) = (&self.db, &self.project) {
            match db.lock() {
                Ok(conn) => {
                    let params = CreateSessionParams {
                        id: session_id.to_string(),
                        project_id: project.id.clone(),
                        name: name.to_string(),
                        backend: "daemon".to_string(),
                        auto_approve: true,
                        ..Default::default()
                    };
                    if let Err(e) = SessionService::create(&conn, &params) {
                        return Some(format!("DB persist failed: {e}"));
                    }
                }
                Err(e) => return Some(format!("DB lock failed: {e}")),
            }
        }
        None
    }

    /// Launch a session in worktree mode: create worktree, persist record, spawn daemon.
    fn launch_session_in_worktree(&mut self) {
        if self.agent_command.is_empty() {
            self.set_error("No provider command configured.".into());
            return;
        }
        if !self.daemon_connected {
            self.set_error("Daemon unavailable.".into());
            return;
        }
        // Require DB + project record before creating worktree
        let (db, project_id) = match (&self.db, &self.project) {
            (Some(db), Some(proj)) => (db.clone(), proj.id.clone()),
            _ => {
                self.set_error("DB/project unavailable — cannot persist worktree session.".into());
                return;
            }
        };
        let branch = self.worktree_branch_input.trim().to_string();
        if let Err(e) = WorktreeService::validate_branch_name(&branch) {
            self.worktree_error = Some(e);
            return;
        }

        let project_name = self
            .project_cwd
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "project".to_string());

        let task_key = if self.worktree_task_key_input.trim().is_empty() {
            None
        } else {
            Some(self.worktree_task_key_input.trim().to_string())
        };

        let session_id = uuid::Uuid::new_v4().to_string();
        let mode = WorktreeMode::Create {
            base_project_path: self.project_cwd.clone(),
            branch_name: branch.clone(),
            task_key: task_key.clone(),
        };

        // Detect base branch
        let base_branch =
            match planeai_core::git::detect_default_branch(&self.project_cwd.to_string_lossy()) {
                Ok(b) => b,
                Err(_) => "main".to_string(),
            };

        // Resolve worktree (creates it on disk)
        let resolved = match WorktreeService::resolve_worktree(
            &mode,
            &project_name,
            &self.project_cwd,
            &session_id,
            &base_branch,
        ) {
            Ok(r) => r,
            Err(e) => {
                self.worktree_error = Some(format!("Worktree creation failed: {e}"));
                return;
            }
        };

        // Persist session record BEFORE spawning (db/project_id validated at top)
        match db.lock() {
            Ok(conn) => {
                let params = CreateSessionParams {
                    id: session_id.clone(),
                    project_id,
                    name: self.provider_label.clone(),
                    backend: "daemon".to_string(),
                    auto_approve: true,
                    branch: resolved.branch_name.clone(),
                    worktree_path: resolved.worktree_path.clone(),
                    task_key: task_key.clone(),
                    base_branch: resolved.base_branch.clone(),
                    ..Default::default()
                };
                if let Err(e) = SessionService::create(&conn, &params) {
                    // Rollback: clean up the worktree we just created
                    if let Some(ref wt) = resolved.worktree_path {
                        planeai_core::cleanup::cleanup_worktree(
                            &self.project_cwd.to_string_lossy(),
                            wt,
                            Some(&resolved.branch_name),
                        );
                    }
                    self.set_error(format!("DB persist failed: {e}"));
                    return;
                }
            }
            Err(e) => {
                // Rollback: clean up the worktree we just created
                if let Some(ref wt) = resolved.worktree_path {
                    planeai_core::cleanup::cleanup_worktree(
                        &self.project_cwd.to_string_lossy(),
                        wt,
                        Some(&resolved.branch_name),
                    );
                }
                self.set_error(format!("DB lock failed: {e}"));
                return;
            }
        }

        // Spawn daemon in the worktree cwd
        let id = self.next_id;
        self.next_id += 1;
        let result = DaemonSession::spawn_with_session_id(
            id,
            &session_id,
            self.cols as u16,
            self.rows as u16,
            Some(&self.agent_command),
            &resolved.cwd,
            &self.extra_path_dirs,
        );
        match result {
            Ok(backend) => {
                let term = new_term(self.cols, self.rows);
                let processor = new_processor();
                let snapshot = snapshot_grid(&term);
                let log_file_exists = self.check_log_exists(&session_id);
                self.sessions.push(Session {
                    id,
                    session_id: session_id.clone(),
                    command: self.agent_command.clone(),
                    cwd: resolved.cwd,
                    status: SessionStatus::Running,
                    backend: Box::new(backend),
                    term,
                    processor,
                    snapshot,
                    cache: Cache::new(),
                    bytes_processed: 0,
                    log_file_exists,
                });
                self.active = self.sessions.len() - 1;
                self.worktree_prompt = false;
                self.worktree_branch_input.clear();
                self.worktree_task_key_input.clear();
                self.worktree_error = None;
                self.clear_error();
                self.refresh_persisted_sessions();
            }
            Err(e) => {
                // Spawn failed — clean up worktree and mark DB record as destroyed
                if let Some(ref wt) = resolved.worktree_path {
                    planeai_core::cleanup::cleanup_worktree(
                        &self.project_cwd.to_string_lossy(),
                        wt,
                        Some(&resolved.branch_name),
                    );
                }
                if let Some(ref db) = self.db {
                    if let Ok(conn) = db.lock() {
                        let _ = SessionService::set_status(&conn, &session_id, "destroyed");
                    }
                }
                self.set_error(format!("Launch failed: {e}"));
                self.refresh_persisted_sessions();
            }
        }
    }

    /// Recompute the worktree preview path for the current inputs.
    fn update_worktree_preview(&mut self) {
        let project_name = self
            .project_cwd
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "project".to_string());
        // Use a dummy session_id for preview
        let preview_id = "00000000-0000-0000-0000-000000000000";
        let short = WorktreeService::short_id(preview_id);
        let path = WorktreeService::worktree_path(&project_name, &short);
        self.worktree_computed_path = Some(path.to_string_lossy().to_string());
    }

    fn check_daemon_health(&mut self) {
        let now = Instant::now();
        let should_check = self
            .last_health_check
            .map(|t| now.duration_since(t) >= Duration::from_secs(5))
            .unwrap_or(true);
        if !should_check {
            return;
        }
        self.last_health_check = Some(now);
        let was_connected = self.daemon_connected;
        self.daemon_connected = daemon_is_connected();
        if was_connected && !self.daemon_connected {
            // Mark running sessions as unreachable
            for s in &mut self.sessions {
                if s.status == SessionStatus::Running || s.status == SessionStatus::Attached {
                    s.status = SessionStatus::Unreachable;
                }
            }
        }
    }

    fn check_log_exists(&self, session_id: &str) -> bool {
        if let Ok(dir) = std::env::var("PLANEAI_SESSION_LOG_DIR") {
            PathBuf::from(dir)
                .join("sessions")
                .join(session_id)
                .join("meta.json")
                .exists()
        } else {
            false
        }
    }

    fn set_error(&mut self, msg: String) {
        self.last_error = Some(msg);
        self.error_time = Some(Instant::now());
    }

    fn clear_error(&mut self) {
        self.last_error = None;
        self.error_time = None;
    }

    fn switch_to(&mut self, idx: usize) {
        if idx < self.sessions.len() && idx != self.active {
            self.active = idx;
            let session = &mut self.sessions[idx];
            session.snapshot = snapshot_grid(&session.term);
            session.cache.clear();
        }
    }

    fn select_project(&mut self, path_str: &str) {
        let expanded = planeai_core::session_launch::expand_tilde(path_str);
        let path = PathBuf::from(&expanded);
        if !path.is_dir() {
            self.set_error(format!("Not a directory: {}", expanded));
            return;
        }
        // Ensure DB project record first — abort if DB unavailable
        let db_clone = self.db.clone();
        if let Some(db) = db_clone {
            match db.lock() {
                Ok(conn) => match ProjectService::ensure_project(&conn, &expanded) {
                    Ok(proj) => self.project = Some(proj),
                    Err(e) => {
                        self.set_error(format!("Project persist failed: {e}"));
                        return;
                    }
                },
                Err(e) => {
                    self.set_error(format!("DB lock failed: {e}"));
                    return;
                }
            }
        }
        // DB succeeded — now mutate UI state
        self.project_cwd = path;
        self.picking_project = false;
        self.project_input.clear();
        self.recent_projects = add_recent_project(&expanded);
        self.refresh_persisted_sessions();
        self.clear_error();
    }

    fn open_log_replay(&mut self) {
        if self.sessions.is_empty() {
            self.set_error("No active session for log replay.".into());
            return;
        }
        let session = &self.sessions[self.active];
        let session_id = session.session_id.clone();
        let log_dir = match std::env::var("PLANEAI_SESSION_LOG_DIR") {
            Ok(d) => d,
            Err(_) => {
                self.set_error("Log replay failed: PLANEAI_SESSION_LOG_DIR not set.".into());
                return;
            }
        };
        let session_log_dir = PathBuf::from(&log_dir).join("sessions").join(&session_id);
        // Find the .ansi log file (named <timestamp>_output.ansi)
        let ansi_path = std::fs::read_dir(&session_log_dir)
            .ok()
            .and_then(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.extension().map(|e| e == "ansi").unwrap_or(false))
                    .max() // latest file
            });
        let ansi_path = match ansi_path {
            Some(p) => p,
            None => {
                self.set_error(format!(
                    "No log file for session {}",
                    &session_id[..session_id.len().min(14)]
                ));
                return;
            }
        };
        // Canonicalize and verify path stays under log dir
        let canonical = match ansi_path.canonicalize() {
            Ok(p) => p,
            Err(e) => {
                self.set_error(format!("Log replay failed: {}", e));
                return;
            }
        };
        let log_dir_canonical = PathBuf::from(&log_dir)
            .canonicalize()
            .unwrap_or_else(|_| PathBuf::from(&log_dir));
        if !canonical.starts_with(&log_dir_canonical) {
            self.set_error("Log replay failed: path escapes session log directory.".into());
            return;
        }
        let data = match std::fs::read(&canonical) {
            Ok(d) => d,
            Err(e) => {
                self.set_error(format!("Log replay failed: {}", e));
                return;
            }
        };
        let mut term = new_term(self.cols, self.rows);
        let mut processor = new_processor();
        processor.advance(&mut term, &data);
        let snapshot = snapshot_grid(&term);
        self.log_replay = Some(LogReplayState {
            term,
            snapshot,
            cache: Cache::new(),
            session_id,
        });
    }
}

impl WorkflowApp {
    fn update(&mut self, message: Message) {
        match message {
            Message::ProjectInputChanged(val) => {
                self.project_input = val;
            }
            Message::ProjectInputSubmit => {
                let input = self.project_input.clone();
                self.select_project(&input);
            }
            Message::LaunchPromptChanged(val) => {
                self.launch_prompt_input = val;
            }
            Message::LaunchPromptSubmit => {
                let cmd = self.launch_prompt_input.clone();
                self.launch_prompt = false;
                self.launch_prompt_input.clear();
                self.launch_session_with_command(&cmd);
            }
            Message::WorktreeBranchChanged(val) => {
                self.worktree_branch_input = val;
                self.worktree_error = None;
                self.update_worktree_preview();
            }
            Message::WorktreeTaskKeyChanged(val) => {
                self.worktree_task_key_input = val;
            }
            Message::WorktreeToggle => {
                self.worktree_use_worktree = !self.worktree_use_worktree;
                if self.worktree_use_worktree {
                    self.update_worktree_preview();
                }
            }
            Message::WorktreeLaunchSubmit => {
                if self.worktree_use_worktree {
                    self.launch_session_in_worktree();
                } else {
                    self.worktree_prompt = false;
                    self.launch_session();
                }
            }
            Message::WindowResized(size) => {
                let cw = 9.0f32;
                let ch = 18.0f32;
                let new_cols = ((size.width - 180.0) / cw).floor().max(2.0) as u16;
                let new_rows = ((size.height - 40.0) / ch).floor().max(2.0) as u16;
                if new_cols as usize == self.cols && new_rows as usize == self.rows {
                    return;
                }
                self.cols = new_cols as usize;
                self.rows = new_rows as usize;
                if self.sessions.is_empty() {
                    return;
                }
                let session = &mut self.sessions[self.active];
                let term_size = TermSize {
                    cols: self.cols,
                    rows: self.rows,
                };
                session.term.resize(term_size);
                let _ = session.backend.resize(new_cols, new_rows);
                session.snapshot = snapshot_grid(&session.term);
                session.cache.clear();
            }
            Message::KeyEvent(keyboard::Event::KeyPressed {
                key,
                modifiers,
                text: txt,
                ..
            }) => {
                // Log replay mode: Escape exits
                if self.log_replay.is_some() {
                    if matches!(key, keyboard::Key::Named(keyboard::key::Named::Escape)) {
                        self.log_replay = None;
                    }
                    return;
                }

                // Launch prompt mode
                if self.launch_prompt {
                    if matches!(key, keyboard::Key::Named(keyboard::key::Named::Escape)) {
                        self.launch_prompt = false;
                        self.launch_prompt_input.clear();
                    }
                    return;
                }

                // Worktree prompt mode
                if self.worktree_prompt {
                    if matches!(key, keyboard::Key::Named(keyboard::key::Named::Escape)) {
                        self.worktree_prompt = false;
                        self.worktree_error = None;
                    }
                    return;
                }

                // Project picker mode
                if self.picking_project {
                    if matches!(key, keyboard::Key::Named(keyboard::key::Named::Escape)) {
                        self.picking_project = false;
                        self.project_input.clear();
                        return;
                    }
                    // Cmd+1..9 selects from recent projects
                    let cmd = if cfg!(target_os = "macos") {
                        modifiers.command()
                    } else {
                        modifiers.control()
                    };
                    if cmd {
                        if let keyboard::Key::Character(c) = &key {
                            if let Ok(digit) = c.as_str().parse::<usize>() {
                                if (1..=9).contains(&digit) {
                                    if let Some(path) = self.recent_projects.get(digit - 1).cloned()
                                    {
                                        self.select_project(&path);
                                        return;
                                    }
                                }
                            }
                        }
                    }
                    return;
                }

                // Shortcuts overlay: Escape dismisses
                if self.show_shortcuts {
                    if matches!(key, keyboard::Key::Named(keyboard::key::Named::Escape)) {
                        self.show_shortcuts = false;
                    }
                    return;
                }

                let cmd = if cfg!(target_os = "macos") {
                    modifiers.command()
                } else {
                    modifiers.control()
                };

                // On non-macOS, require Shift for keys that conflict with terminal
                // (Ctrl+A, Ctrl+L, Ctrl+R, Ctrl+W are terminal sequences)
                let cmd_safe = if cfg!(target_os = "macos") {
                    modifiers.command()
                } else {
                    modifiers.control() && modifiers.shift()
                };

                // Cmd+/ — toggle shortcuts overlay
                if cmd && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "/") {
                    self.show_shortcuts = !self.show_shortcuts;
                    self.kill_armed = false;
                    return;
                }

                // Cmd+N — launch new session
                if cmd
                    && !modifiers.shift()
                    && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "n")
                {
                    self.launch_session();
                    self.kill_armed = false;
                    return;
                }

                // Cmd+B — worktree launch prompt
                if cmd
                    && !modifiers.shift()
                    && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "b")
                {
                    self.worktree_prompt = !self.worktree_prompt;
                    if self.worktree_prompt {
                        self.worktree_use_worktree = true;
                        self.update_worktree_preview();
                    }
                    self.kill_armed = false;
                    return;
                }

                // Cmd+Shift+N — launch with different command
                if cmd
                    && modifiers.shift()
                    && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "n" || c.as_str() == "N")
                {
                    self.launch_prompt = true;
                    self.launch_prompt_input = self.agent_command.clone();
                    return;
                }

                // Cmd+L (macOS) / Ctrl+Shift+L (other) — log replay
                if cmd_safe
                    && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "l" || c.as_str() == "L")
                {
                    self.open_log_replay();
                    return;
                }

                // Cmd+O — open project picker
                if cmd
                    && !modifiers.shift()
                    && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "o")
                {
                    self.picking_project = !self.picking_project;
                    self.project_input = self.project_cwd.to_string_lossy().to_string();
                    self.recent_projects = load_recent_projects();
                    return;
                }
                // Cmd+R (macOS) / Ctrl+Shift+R (other) — refresh
                if cmd_safe
                    && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "r" || c.as_str() == "R")
                {
                    self.refresh_daemon_list();
                    return;
                }
                // Cmd+W (macOS, no shift) / Ctrl+Shift+W (other, no alt) — detach
                let is_detach = if cfg!(target_os = "macos") {
                    modifiers.command()
                        && !modifiers.shift()
                        && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "w")
                } else {
                    modifiers.control()
                        && modifiers.shift()
                        && !modifiers.alt()
                        && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "w" || c.as_str() == "W")
                };
                if is_detach {
                    self.detach_active();
                    return;
                }
                // Cmd+Shift+W (macOS) / Ctrl+Shift+Alt+W (other) — kill
                let is_kill = if cfg!(target_os = "macos") {
                    modifiers.command()
                        && modifiers.shift()
                        && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "w" || c.as_str() == "W")
                } else {
                    modifiers.control()
                        && modifiers.shift()
                        && modifiers.alt()
                        && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "w" || c.as_str() == "W")
                };
                if is_kill {
                    self.kill_active();
                    return;
                }
                // Cmd+A (macOS) / Ctrl+Shift+A (other) — attach first unattached
                if cmd_safe
                    && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "a" || c.as_str() == "A")
                {
                    let attached_ids: Vec<&str> = self
                        .sessions
                        .iter()
                        .map(|s| s.session_id.as_str())
                        .collect();
                    if let Some(info) = self
                        .daemon_sessions_listed
                        .iter()
                        .find(|i| i.alive && !attached_ids.contains(&i.session_id.as_str()))
                    {
                        let sid = info.session_id.clone();
                        self.attach_session(sid);
                    }
                    return;
                }
                // Cmd+1..9 — switch sessions
                if cmd && !modifiers.shift() {
                    if let keyboard::Key::Character(c) = &key {
                        if let Ok(digit) = c.as_str().parse::<usize>() {
                            if (1..=9).contains(&digit) {
                                self.switch_to(digit - 1);
                                return;
                            }
                        }
                    }
                }
                // Paste
                let is_paste = if cfg!(target_os = "macos") {
                    modifiers.command()
                        && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "v")
                } else {
                    modifiers.control()
                        && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "v")
                };
                if is_paste {
                    if !self.sessions.is_empty() {
                        if let Ok(mut clipboard) = Clipboard::new() {
                            if let Ok(t) = clipboard.get_text() {
                                if !t.is_empty() {
                                    let _ = self.sessions[self.active].backend.write(t.as_bytes());
                                }
                            }
                        }
                    }
                    return;
                }
                // Forward input to active session
                if !self.sessions.is_empty() {
                    self.kill_armed = false;
                    let bytes = input::encode_key_event(&key, &modifiers, &txt);
                    if let Some(ref b) = bytes {
                        if !b.is_empty() {
                            let _ = self.sessions[self.active].backend.write(b);
                        }
                    }
                }
            }
            Message::KeyEvent(_) => {}
            Message::Poll => {
                // Fade errors after 5s
                if let Some(t) = self.error_time {
                    if t.elapsed() >= Duration::from_secs(5) {
                        self.clear_error();
                    }
                }

                self.check_daemon_health();

                // Drain output from all sessions
                for i in 0..self.sessions.len() {
                    loop {
                        let output = self.sessions[i].backend.try_read_batch().unwrap_or(None);
                        match output {
                            Some(data) => {
                                self.sessions[i].bytes_processed += data.len() as u64;
                                let session = &mut self.sessions[i];
                                session.processor.advance(&mut session.term, &data);
                            }
                            None => break,
                        }
                    }
                    if i == self.active {
                        let session = &mut self.sessions[i];
                        session.snapshot = snapshot_grid(&session.term);
                        session.cache.clear();
                    }
                }

                // Update session statuses
                for s in &mut self.sessions {
                    if (s.status == SessionStatus::Running || s.status == SessionStatus::Attached)
                        && s.backend.has_exited()
                    {
                        s.status = SessionStatus::Exited;
                    }
                    if !s.log_file_exists {
                        if let Ok(dir) = std::env::var("PLANEAI_SESSION_LOG_DIR") {
                            s.log_file_exists = PathBuf::from(&dir)
                                .join("sessions")
                                .join(&s.session_id)
                                .join("meta.json")
                                .exists();
                        }
                    }
                }
            }
        }
    }
}

impl WorkflowApp {
    fn view(&self) -> Element<'_, Message> {
        let mut left_panel_content = column![].spacing(2).width(Length::Fixed(180.0));

        // Daemon status
        let (indicator, color) = if self.daemon_connected {
            ("⚡ daemon connected", Color::from_rgb8(100, 200, 100))
        } else {
            ("⚠ daemon disconnected", Color::from_rgb8(255, 150, 50))
        };
        left_panel_content =
            left_panel_content.push(text(indicator).size(11).color(color).font(Font::MONOSPACE));
        left_panel_content = left_panel_content.push(text("").size(4));

        // Session cards (Part 8)
        for (i, s) in self.sessions.iter().enumerate() {
            let status_icon = match s.status {
                SessionStatus::Running => "●",
                SessionStatus::Attached => "◉",
                SessionStatus::Exited => "○",
                SessionStatus::Detached => "◌",
                SessionStatus::Unreachable => "✕",
                SessionStatus::Killed => "☠",
            };
            let status_label = match s.status {
                SessionStatus::Running => "Run",
                SessionStatus::Attached => "Att",
                SessionStatus::Exited => "Exit",
                SessionStatus::Detached => "Det",
                SessionStatus::Unreachable => "Unr",
                SessionStatus::Killed => "Kill",
            };
            let cwd_name = s
                .cwd
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| s.cwd.to_string_lossy().to_string());
            let log_indicator = if s.log_file_exists { " 📄" } else { "" };
            let dropped = s.backend.bytes_dropped();
            let drop_indicator = if dropped > 0 {
                format!(" ⚠{dropped}")
            } else {
                String::new()
            };
            let cmd_short = s.command.split_whitespace().next().unwrap_or("?");
            let sid_short = &s.session_id[..s.session_id.len().min(14)];
            let active_marker = if i == self.active { "▶" } else { " " };
            let provider_short = if !self.provider_label.is_empty() {
                &self.provider_label
            } else {
                cmd_short
            };
            // Enrich from persisted record (branch, task_key, worktree)
            let persisted = self
                .persisted_sessions
                .iter()
                .find(|r| r.id == s.session_id);
            let branch_display = persisted
                .and_then(|r| {
                    let b = &r.branch;
                    if b.is_empty() {
                        None
                    } else {
                        Some(b.as_str())
                    }
                })
                .unwrap_or("");
            let task_display = persisted.and_then(|r| r.task_key.as_deref()).unwrap_or("");
            let wt_indicator = if persisted.and_then(|r| r.worktree_path.as_ref()).is_some() {
                "🌿"
            } else {
                ""
            };
            let extra_meta = match (branch_display.is_empty(), task_display.is_empty()) {
                (false, false) => format!(" {wt_indicator}[{task_display}:{branch_display}]"),
                (false, true) => format!(" {wt_indicator}[{branch_display}]"),
                (true, false) => format!(" {wt_indicator}[{task_display}]"),
                (true, true) => {
                    if !wt_indicator.is_empty() {
                        format!(" {wt_indicator}")
                    } else {
                        String::new()
                    }
                }
            };
            let label = format!(
                "{}{} {} {} {} {} daemon{}{}{}",
                active_marker,
                status_icon,
                provider_short,
                cwd_name,
                sid_short,
                status_label,
                extra_meta,
                drop_indicator,
                log_indicator,
            );
            let color = match (&s.status, i == self.active) {
                (_, true) => Color::from_rgb8(100, 200, 255),
                (SessionStatus::Exited, _) | (SessionStatus::Killed, _) => {
                    Color::from_rgb8(120, 120, 120)
                }
                (SessionStatus::Detached, _) | (SessionStatus::Unreachable, _) => {
                    Color::from_rgb8(200, 150, 50)
                }
                _ => Color::from_rgb8(180, 180, 180),
            };
            left_panel_content =
                left_panel_content.push(text(label).size(10).color(color).font(Font::MONOSPACE));
        }

        // Unattached daemon sessions
        if !self.daemon_sessions_listed.is_empty() {
            let attached_ids: Vec<&str> = self
                .sessions
                .iter()
                .map(|s| s.session_id.as_str())
                .collect();
            let unattached: Vec<&DaemonSessionInfo> = self
                .daemon_sessions_listed
                .iter()
                .filter(|i| !attached_ids.contains(&i.session_id.as_str()))
                .collect();
            if !unattached.is_empty() {
                left_panel_content = left_panel_content.push(text("").size(4));
                left_panel_content = left_panel_content.push(
                    text("── detached ──")
                        .size(10)
                        .color(Color::from_rgb8(120, 120, 120))
                        .font(Font::MONOSPACE),
                );
                for info in unattached.iter().take(5) {
                    let label = format!(
                        "  {} {}",
                        if info.alive { "◌" } else { "✕" },
                        &info.session_id[..info.session_id.len().min(14)]
                    );
                    let color = if info.alive {
                        Color::from_rgb8(200, 150, 50)
                    } else {
                        Color::from_rgb8(100, 100, 100)
                    };
                    left_panel_content = left_panel_content
                        .push(text(label).size(11).color(color).font(Font::MONOSPACE));
                }
            }
        }

        let left_panel =
            container(left_panel_content)
                .padding(8)
                .style(|_: &Theme| container::Style {
                    background: Some(Color::from_rgb8(20, 20, 20).into()),
                    ..Default::default()
                });

        // Terminal area (or log replay)
        let terminal_area: Element<'_, Message> = if let Some(ref replay) = self.log_replay {
            // Log replay view
            let banner = container(
                text("READ-ONLY LOG REPLAY — Escape to exit")
                    .size(12)
                    .color(Color::from_rgb8(255, 200, 50))
                    .font(Font::MONOSPACE),
            )
            .width(Length::Fill)
            .padding(2)
            .style(|_: &Theme| container::Style {
                background: Some(Color::from_rgb8(60, 50, 20).into()),
                ..Default::default()
            });
            let canvas_view = Canvas::new(WorkflowTermRenderer {
                snapshot: &replay.snapshot,
                cache: &replay.cache,
            })
            .width(Length::Fill)
            .height(Length::Fill);
            column![banner, canvas_view].into()
        } else if self.sessions.is_empty() {
            container(
                text("No sessions. Cmd+N to launch, Cmd+A to attach.")
                    .size(14)
                    .color(Color::from_rgb8(120, 120, 120))
                    .font(Font::MONOSPACE),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
        } else {
            let session = &self.sessions[self.active];
            Canvas::new(WorkflowTermRenderer {
                snapshot: &session.snapshot,
                cache: &session.cache,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        };

        // Status bar (Part 9)
        let project_display = self.project_cwd.to_string_lossy();
        let active_info = if !self.sessions.is_empty() {
            let s = &self.sessions[self.active];
            format!(
                " | {} | {} | {}",
                s.command,
                &s.session_id[..s.session_id.len().min(14)],
                match s.status {
                    SessionStatus::Running => "Running",
                    SessionStatus::Attached => "Attached",
                    SessionStatus::Exited => "Exited",
                    SessionStatus::Detached => "Detached",
                    SessionStatus::Unreachable => "Unreachable",
                    SessionStatus::Killed => "Killed",
                }
            )
        } else {
            format!(" | {} | {}", self.provider_label, self.agent_command)
        };
        let error_display = self.last_error.as_deref().unwrap_or("");
        let status_text = format!(
            " {} {}{}{}",
            if self.daemon_connected { "⚡" } else { "⚠" },
            project_display,
            active_info,
            if error_display.is_empty() {
                String::new()
            } else {
                format!(" | ⚠ {}", error_display)
            },
        );
        let status_bar = container(
            text(status_text)
                .size(12)
                .color(Color::from_rgb8(180, 180, 180))
                .font(Font::MONOSPACE),
        )
        .width(Length::Fill)
        .padding(2)
        .style(|_: &Theme| container::Style {
            background: Some(Color::from_rgb8(40, 40, 40).into()),
            ..Default::default()
        });

        // Build layout
        let main_content = row![left_panel, column![terminal_area, status_bar]];

        let base: Element<'_, Message> = if self.picking_project {
            // Project picker with recent list
            let mut picker_col = column![].spacing(2).width(Length::Fill).padding(4);
            picker_col = picker_col.push(
                text_input(
                    "Enter project path (~/... expanded)...",
                    &self.project_input,
                )
                .on_input(Message::ProjectInputChanged)
                .on_submit(Message::ProjectInputSubmit)
                .size(14)
                .width(Length::Fill),
            );
            // Show recent projects
            if !self.recent_projects.is_empty() {
                picker_col = picker_col.push(
                    text("Recent (Cmd+1..9 to select):")
                        .size(11)
                        .color(Color::from_rgb8(150, 150, 150))
                        .font(Font::MONOSPACE),
                );
                for (i, p) in self.recent_projects.iter().take(9).enumerate() {
                    let exists = PathBuf::from(p).is_dir();
                    let marker = if !exists { " (missing)" } else { "" };
                    let label = format!(" {}. {}{}", i + 1, p, marker);
                    let color = if exists {
                        Color::from_rgb8(180, 180, 180)
                    } else {
                        Color::from_rgb8(100, 100, 100)
                    };
                    picker_col =
                        picker_col.push(text(label).size(11).color(color).font(Font::MONOSPACE));
                }
            }
            let picker = container(picker_col).style(|_: &Theme| container::Style {
                background: Some(Color::from_rgb8(50, 50, 70).into()),
                ..Default::default()
            });
            column![picker, main_content].into()
        } else if self.launch_prompt {
            let prompt = container(
                text_input("Command to launch...", &self.launch_prompt_input)
                    .on_input(Message::LaunchPromptChanged)
                    .on_submit(Message::LaunchPromptSubmit)
                    .size(14)
                    .width(Length::Fill),
            )
            .width(Length::Fill)
            .padding(4)
            .style(|_: &Theme| container::Style {
                background: Some(Color::from_rgb8(50, 70, 50).into()),
                ..Default::default()
            });
            column![prompt, main_content].into()
        } else if self.worktree_prompt {
            let mut wt_col = column![].spacing(4).width(Length::Fill).padding(6);
            let project_name = self
                .project_cwd
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "project".to_string());
            wt_col = wt_col.push(
                text(format!("Worktree Launch — project: {}", project_name))
                    .size(12)
                    .color(Color::from_rgb8(180, 220, 255))
                    .font(Font::MONOSPACE),
            );
            let mode_label = if self.worktree_use_worktree {
                "● Worktree mode (Cmd+B to toggle)"
            } else {
                "○ Direct cwd mode (Cmd+B to toggle)"
            };
            wt_col = wt_col.push(
                text(mode_label)
                    .size(11)
                    .color(Color::from_rgb8(150, 200, 150))
                    .font(Font::MONOSPACE),
            );
            if self.worktree_use_worktree {
                wt_col = wt_col.push(
                    text_input(
                        "Branch name (e.g. feat/my-feature)...",
                        &self.worktree_branch_input,
                    )
                    .on_input(Message::WorktreeBranchChanged)
                    .on_submit(Message::WorktreeLaunchSubmit)
                    .size(13)
                    .width(Length::Fill),
                );
                wt_col = wt_col.push(
                    text_input(
                        "Task key (optional, e.g. PLA-42)...",
                        &self.worktree_task_key_input,
                    )
                    .on_input(Message::WorktreeTaskKeyChanged)
                    .on_submit(Message::WorktreeLaunchSubmit)
                    .size(13)
                    .width(Length::Fill),
                );
                if let Some(ref path) = self.worktree_computed_path {
                    wt_col = wt_col.push(
                        text(format!("→ {}", path))
                            .size(10)
                            .color(Color::from_rgb8(120, 150, 180))
                            .font(Font::MONOSPACE),
                    );
                }
                if let Some(ref err) = self.worktree_error {
                    wt_col = wt_col.push(
                        text(format!("⚠ {}", err))
                            .size(11)
                            .color(Color::from_rgb8(255, 100, 100))
                            .font(Font::MONOSPACE),
                    );
                }
            }
            wt_col = wt_col.push(
                text("Enter to launch | Escape to cancel")
                    .size(10)
                    .color(Color::from_rgb8(100, 100, 100))
                    .font(Font::MONOSPACE),
            );
            let wt_panel = container(wt_col).style(|_: &Theme| container::Style {
                background: Some(Color::from_rgb8(40, 50, 60).into()),
                ..Default::default()
            });
            column![wt_panel, main_content].into()
        } else {
            main_content.into()
        };

        if self.show_shortcuts {
            use iced::widget::stack;
            let overlay = container(shortcuts_overlay())
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .style(|_: &Theme| container::Style {
                    background: Some(Color::from_rgba8(0, 0, 0, 0.7).into()),
                    ..Default::default()
                });
            stack![base, overlay].into()
        } else {
            base
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch(vec![
            keyboard::listen().map(Message::KeyEvent),
            iced::time::every(Duration::from_millis(16)).map(|_| Message::Poll),
            event::listen_with(|ev, _status, _id| {
                if let iced::Event::Window(window::Event::Resized(size)) = ev {
                    Some(Message::WindowResized(size))
                } else {
                    None
                }
            }),
        ])
    }
}

// ─── Canvas renderer ─────────────────────────────────────────────────────────

struct WorkflowTermRenderer<'a> {
    snapshot: &'a GridSnapshot,
    cache: &'a Cache,
}

impl<'a> Program<Message> for WorkflowTermRenderer<'a> {
    type State = ();
    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let geom = self.cache.draw(renderer, bounds.size(), |frame| {
            let cw = bounds.width / self.snapshot.cols as f32;
            let ch = bounds.height / self.snapshot.rows as f32;
            let font_size = (ch * 0.85).min(16.0);
            frame.fill_rectangle(Point::ORIGIN, bounds.size(), Color::from_rgb8(30, 30, 30));
            for (ri, row) in self.snapshot.cells.iter().enumerate() {
                for (ci, cell) in row.iter().enumerate() {
                    let x = ci as f32 * cw;
                    let y = ri as f32 * ch;
                    if cell.bg != Color::from_rgb8(0, 0, 0) {
                        frame.fill_rectangle(Point::new(x, y), Size::new(cw, ch), cell.bg);
                    }
                    if ri == self.snapshot.cursor_line && ci == self.snapshot.cursor_col {
                        frame.fill_rectangle(
                            Point::new(x, y),
                            Size::new(cw, ch),
                            Color::from_rgba8(200, 200, 200, 0.4),
                        );
                    }
                    if cell.c != ' ' && cell.c != '\0' {
                        frame.fill_text(canvas::Text {
                            content: cell.c.to_string(),
                            position: Point::new(x, y + 1.0),
                            color: cell.fg,
                            size: font_size.into(),
                            font: Font::MONOSPACE,
                            ..Default::default()
                        });
                    }
                }
            }
        });
        vec![geom]
    }
}

// ─── Static args ─────────────────────────────────────────────────────────────

use std::sync::OnceLock;
static WORKFLOW_ARGS: OnceLock<Args> = OnceLock::new();

fn title(_state: &WorkflowApp) -> String {
    "PlaneAI Workflow Shell".into()
}

// ─── Entry point ─────────────────────────────────────────────────────────────

pub fn run(args: Args) -> iced::Result {
    let cols = args.cols;
    let rows = args.rows;
    WORKFLOW_ARGS.set(args).unwrap();
    iced::application(WorkflowApp::boot, WorkflowApp::update, WorkflowApp::view)
        .title(title)
        .subscription(WorkflowApp::subscription)
        .window_size(Size::new(
            cols as f32 * 9.0 + 180.0,
            rows as f32 * 18.0 + 40.0,
        ))
        .run()
}
