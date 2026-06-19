//! PlaneAI Workflow Shell — orchestrates daemon sessions with project context.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use planeai_core::services::{
    self, CreateSessionParams, ProjectService, SessionRecord, SessionService, TaskLaunchRequest,
    TaskService, WorktreeMode, WorktreeService,
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
use crate::combobox::{ComboBoxState, ComboItem};
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

// ─── Session form types ──────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum SessionFormMode {
    Manual,
    FromTask,
}

#[derive(Debug, Clone, PartialEq)]
enum SessionFormField {
    Mode,
    Project,
    Task,
    Name,
    Branch,
    Toggles,
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
    // Task picker (Cmd+T)
    task_picker: bool,
    task_list: Vec<planeai_tasks::model::Task>,
    task_picker_index: usize,
    selected_task: Option<planeai_tasks::model::Task>,
    // New... menu (Cmd+N)
    new_menu: bool,
    new_menu_index: usize,
    // Session creation form
    session_form: bool,
    session_form_mode: SessionFormMode,
    session_form_name: String,
    session_form_branch: String,
    session_form_use_worktree: bool,
    session_form_auto_approve: bool,
    session_form_provider_idx: usize,
    session_form_task_combo: ComboBoxState,
    session_form_task_list: Vec<planeai_tasks::model::Task>,
    session_form_focus: SessionFormField,
    session_form_error: Option<String>,
    session_form_project_combo: ComboBoxState,
    provider_keys: Vec<String>,
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
    TaskPickerSelect(usize),
    TaskLaunchSelected,
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
                task_picker: false,
                task_list: Vec::new(),
                task_picker_index: 0,
                selected_task: None,
                new_menu: false,
                new_menu_index: 0,
                session_form: false,
                session_form_mode: SessionFormMode::Manual,
                session_form_name: String::new(),
                session_form_branch: String::new(),
                session_form_use_worktree: false,
                session_form_auto_approve: true,
                session_form_provider_idx: 0,
                session_form_task_combo: ComboBoxState::new(Vec::new()),
                session_form_task_list: Vec::new(),
                session_form_focus: SessionFormField::Mode,
                session_form_error: None,
                session_form_project_combo: ComboBoxState::new(Vec::new()),
                provider_keys: Vec::new(),
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
                    // Fire lifecycle hook for task-linked sessions on kill
                    if let Some(ref tk) = rec.task_key {
                        if let Ok(Some(proj)) = ProjectService::get_by_id(&conn, &rec.project_id) {
                            let db_path = planeai_core::app_data_dir().join("planeai.db");
                            if let Err(e) =
                                TaskService::fire_lifecycle_hook(&db_path, &proj.name, tk, "todo")
                            {
                                tracing::warn!(task_key = %tk, error = %e, "lifecycle hook (todo) failed");
                            }
                        }
                    }
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

    /// Open task picker — loads tasks for the current project.
    fn open_task_picker(&mut self) {
        let project_name = match &self.project {
            Some(p) => p.name.clone(),
            None => {
                self.set_error("No project selected.".into());
                return;
            }
        };
        let db_path = planeai_core::app_data_dir().join("planeai.db");
        match TaskService::list_for_project(&db_path, &project_name) {
            Ok(tasks) => {
                self.task_list = tasks;
                self.task_picker_index = 0;
                self.task_picker = true;
            }
            Err(e) => {
                self.set_error(format!("Task list: {e}"));
            }
        }
    }

    /// Open the session creation form.
    fn open_session_form(&mut self) {
        // Load provider keys from config
        let config = if let Some(path) = WORKFLOW_ARGS.get().and_then(|a| a.config.as_ref()) {
            planeai_core::session_launch::load_launch_config(path).unwrap_or_default()
        } else {
            planeai_core::session_launch::load_default_config()
        };
        self.provider_keys = config.providers.keys().cloned().collect();
        self.provider_keys.sort();
        self.session_form_provider_idx = self
            .provider_keys
            .iter()
            .position(|k| k == &config.default_provider)
            .unwrap_or(0);

        // Load projects from DB
        let mut project_items = Vec::new();
        if let Some(ref db) = self.db {
            if let Ok(conn) = db.lock() {
                if let Ok(projects) = ProjectService::list_active(&conn) {
                    project_items = projects
                        .into_iter()
                        .map(|p| ComboItem {
                            id: p.id,
                            label: p.name,
                        })
                        .collect();
                }
            }
        }
        self.session_form_project_combo = ComboBoxState::new(project_items);
        // Pre-select current project
        if let Some(ref p) = self.project {
            self.session_form_project_combo.select_by_id(&p.id);
        }

        self.session_form = true;
        self.session_form_mode = SessionFormMode::Manual;
        self.session_form_name.clear();
        self.session_form_branch.clear();
        self.session_form_use_worktree = false;
        self.session_form_auto_approve = true;
        self.session_form_task_combo = ComboBoxState::new(Vec::new());
        self.session_form_task_list.clear();
        self.session_form_focus = SessionFormField::Mode;
        self.session_form_error = None;
    }

    /// Load tasks into the session form task combobox.
    fn session_form_load_tasks(&mut self) {
        let project_name = match &self.project {
            Some(p) => p.name.clone(),
            None => return,
        };
        let db_path = planeai_core::app_data_dir().join("planeai.db");
        // Try project name first, then try main git repo name (for worktrees/subdirs)
        let tasks = TaskService::list_for_project(&db_path, &project_name)
            .ok()
            .filter(|t| !t.is_empty())
            .or_else(|| {
                // For worktrees, git-common-dir points to main repo's .git
                std::process::Command::new("git")
                    .args(["rev-parse", "--git-common-dir"])
                    .current_dir(&self.project_cwd)
                    .output()
                    .ok()
                    .and_then(|o| {
                        if o.status.success() {
                            String::from_utf8(o.stdout).ok()
                        } else {
                            None
                        }
                    })
                    .and_then(|git_dir| {
                        // git_dir is like /path/to/project/.git — parent is project root
                        std::path::Path::new(git_dir.trim())
                            .parent()
                            .and_then(|p| p.file_name())
                            .map(|n| n.to_string_lossy().to_string())
                    })
                    .and_then(|name| TaskService::list_for_project(&db_path, &name).ok())
            })
            .unwrap_or_default();

        let items: Vec<ComboItem> = tasks
            .iter()
            .map(|t| ComboItem {
                id: t.key.clone(),
                label: format!("{}: {}", t.key, t.title),
            })
            .collect();
        self.session_form_task_combo = ComboBoxState::new(items);
        self.session_form_task_list = tasks;
    }

    /// Auto-fill form fields from the selected task.
    fn session_form_apply_task(&mut self) {
        let selected_key = match &self.session_form_task_combo.selected {
            Some(item) => item.id.clone(),
            None => return,
        };
        if let Some(task) = self
            .session_form_task_list
            .iter()
            .find(|t| t.key == selected_key)
        {
            self.session_form_name = format!("{}: {}", task.key, task.title);
            let slug = format!(
                "{}/{}",
                task.key.to_lowercase(),
                task.title
                    .to_lowercase()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join("-")
                    .chars()
                    .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '/')
                    .collect::<String>()
            );
            self.session_form_branch = slug;
        }
    }

    /// Submit the session creation form — unified launch path matching Tauri app.
    fn submit_session_form(&mut self) {
        if !self.daemon_connected {
            self.session_form_error = Some("Daemon unavailable.".into());
            return;
        }
        let (db, project) = match (&self.db, &self.project) {
            (Some(db), Some(proj)) => (db.clone(), proj.clone()),
            _ => {
                self.session_form_error = Some("DB/project unavailable.".into());
                return;
            }
        };

        // Resolve task prompt (if From Task mode)
        let (task_key, task_prompt) = if self.session_form_mode == SessionFormMode::FromTask {
            let selected_key = match &self.session_form_task_combo.selected {
                Some(item) => item.id.clone(),
                None => {
                    self.session_form_error = Some("No task selected.".into());
                    return;
                }
            };
            let task = match self
                .session_form_task_list
                .iter()
                .find(|t| t.key == selected_key)
            {
                Some(t) => t.clone(),
                None => {
                    self.session_form_error = Some("Task not found.".into());
                    return;
                }
            };
            let prompt = format!(
                "Implement task {}: {}\n\n{}",
                task.key, task.title, task.description
            );
            (Some(task.key), Some(prompt))
        } else {
            (None, None)
        };

        // Load config (same as Tauri app)
        let config = planeai_core::session_launch::load_default_config();
        let provider_id = self
            .provider_keys
            .get(self.session_form_provider_idx)
            .cloned()
            .unwrap_or(config.default_provider.clone());
        let provider = match config.providers.get(&provider_id) {
            Some(p) => p.clone(),
            None => {
                self.session_form_error = Some(format!("Unknown provider: {provider_id}"));
                return;
            }
        };

        // Build command with prompt injection (same as Tauri app)
        let launch_cmd = planeai_core::session_launch::build_provider_launch_command(
            &provider,
            self.session_form_auto_approve,
            task_prompt.as_deref(),
            false, // manual launches are not autonomous
        );
        let cmd = launch_cmd.command;

        // Resolve branch and worktree
        let branch = if self.session_form_branch.is_empty() {
            self.session_form_name
                .to_lowercase()
                .replace(' ', "-")
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '/')
                .collect::<String>()
        } else {
            self.session_form_branch.clone()
        };

        let (working_dir, worktree_path) = if self.session_form_use_worktree {
            let base_branch =
                planeai_core::git::detect_default_branch(&self.project_cwd.to_string_lossy())
                    .unwrap_or_else(|_| "main".to_string());
            let short_id = &uuid::Uuid::new_v4().to_string().replace('-', "")[..8].to_string();
            let sanitized = project
                .name
                .to_lowercase()
                .replace(|c: char| !c.is_alphanumeric(), "-");
            let home = std::env::var("HOME").unwrap_or_default();
            let wt_path = format!("{home}/.planeai/worktrees/{sanitized}/{short_id}");
            if let Some(parent) = std::path::Path::new(&wt_path).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = planeai_core::git::worktree_add(
                &self.project_cwd.to_string_lossy(),
                &wt_path,
                &branch,
                &base_branch,
            ) {
                self.session_form_error = Some(format!("Worktree: {e}"));
                return;
            }
            (std::path::PathBuf::from(&wt_path), Some(wt_path))
        } else {
            (self.project_cwd.clone(), None)
        };

        // Persist session record
        let session_id = uuid::Uuid::new_v4().to_string();
        let session_name = if self.session_form_name.is_empty() {
            provider_id.clone()
        } else {
            self.session_form_name.clone()
        };
        match db.lock() {
            Ok(conn) => {
                let params = CreateSessionParams {
                    id: session_id.clone(),
                    project_id: project.id.clone(),
                    name: session_name.clone(),
                    backend: "daemon".to_string(),
                    auto_approve: self.session_form_auto_approve,
                    branch: branch.clone(),
                    worktree_path: worktree_path.clone(),
                    task_key: task_key.clone(),
                    base_branch: None,
                    provider: Some(provider_id.clone()),
                    ..Default::default()
                };
                if let Err(e) = SessionService::create(&conn, &params) {
                    self.session_form_error = Some(format!("DB: {e}"));
                    return;
                }
            }
            Err(e) => {
                self.session_form_error = Some(format!("DB lock: {e}"));
                return;
            }
        }

        // Spawn daemon session
        let id = self.next_id;
        self.next_id += 1;
        let result = DaemonSession::spawn_with_session_id(
            id,
            &session_id,
            self.cols as u16,
            self.rows as u16,
            Some(&cmd),
            &working_dir,
            &self.extra_path_dirs,
        );
        match result {
            Ok(backend) => {
                // Fire on_start lifecycle hook for task-linked sessions
                if let Some(ref tk) = task_key {
                    let db_path = planeai_core::app_data_dir().join("planeai.db");
                    let _ = TaskService::fire_lifecycle_hook(
                        &db_path,
                        &project.name,
                        tk,
                        "in_progress",
                    );
                }
                let term = new_term(self.cols, self.rows);
                let processor = new_processor();
                let snapshot = snapshot_grid(&term);
                let log_file_exists = self.check_log_exists(&session_id);
                self.sessions.push(Session {
                    id,
                    session_id,
                    command: cmd,
                    cwd: working_dir,
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
                self.session_form = false;
                self.clear_error();
                self.refresh_persisted_sessions();
            }
            Err(e) => {
                // Cleanup worktree on failure
                if let Some(ref wt) = worktree_path {
                    planeai_core::cleanup::cleanup_worktree(
                        &self.project_cwd.to_string_lossy(),
                        wt,
                        Some(&branch),
                    );
                }
                if let Ok(conn) = db.lock() {
                    let _ = SessionService::set_status(&conn, &session_id, "destroyed");
                }
                self.session_form_error = Some(format!("Launch failed: {e}"));
            }
        }
    }

    /// Launch session from selected task with full shared task/worktree integration.
    fn launch_from_task(&mut self) {
        let task = match &self.selected_task {
            Some(t) => t.clone(),
            None => {
                self.set_error("No task selected.".into());
                return;
            }
        };
        if !self.daemon_connected {
            self.set_error("Daemon unavailable.".into());
            return;
        }
        let (db, project) = match (&self.db, &self.project) {
            (Some(db), Some(proj)) => (db.clone(), proj.clone()),
            _ => {
                self.set_error("DB/project unavailable.".into());
                return;
            }
        };

        let config = if let Some(path) = WORKFLOW_ARGS.get().and_then(|a| a.config.as_ref()) {
            planeai_core::session_launch::load_launch_config(path).unwrap_or_default()
        } else {
            planeai_core::session_launch::load_default_config()
        };

        let auto_approve = self.session_form_auto_approve;
        let request = TaskLaunchRequest {
            project_id: project.id.clone(),
            project_name: project.name.clone(),
            project_path: self.project_cwd.clone(),
            task_key: task.key.clone(),
            task_title: task.title.clone(),
            task_description: task.description.clone(),
            task_base_branch: task.base_branch.clone(),
            provider_id: None,
            auto_approve,
            autonomous: false, // manual task launch
            cols: self.cols as u16,
            rows: self.rows as u16,
        };

        let (resolved, worktree_mode) =
            match TaskService::resolve_task_launch(&request, &config, None) {
                Ok(r) => r,
                Err(e) => {
                    self.set_error(format!("Task resolve: {e}"));
                    return;
                }
            };

        let session_id = resolved.request.session_id.clone();

        // Detect base branch
        let base_branch =
            match planeai_core::git::detect_default_branch(&self.project_cwd.to_string_lossy()) {
                Ok(b) => b,
                Err(_) => task.base_branch.clone(),
            };

        // Resolve worktree (creates it on disk)
        let wt_resolved = match WorktreeService::resolve_worktree(
            &worktree_mode,
            &project.name,
            &self.project_cwd,
            &session_id,
            &base_branch,
        ) {
            Ok(r) => r,
            Err(e) => {
                self.set_error(format!("Worktree: {e}"));
                return;
            }
        };

        // Persist session record BEFORE spawning
        match db.lock() {
            Ok(conn) => {
                let params = CreateSessionParams {
                    id: session_id.clone(),
                    project_id: project.id.clone(),
                    name: format!("{}: {}", task.key, task.title),
                    backend: "daemon".to_string(),
                    auto_approve,
                    branch: wt_resolved.branch_name.clone(),
                    worktree_path: wt_resolved.worktree_path.clone(),
                    task_key: Some(task.key.clone()),
                    base_branch: wt_resolved.base_branch.clone(),
                    provider: Some(self.provider_label.clone()),
                    ..Default::default()
                };
                if let Err(e) = SessionService::create(&conn, &params) {
                    if let Some(ref wt) = wt_resolved.worktree_path {
                        planeai_core::cleanup::cleanup_worktree(
                            &self.project_cwd.to_string_lossy(),
                            wt,
                            Some(&wt_resolved.branch_name),
                        );
                    }
                    self.set_error(format!("DB persist: {e}"));
                    return;
                }
            }
            Err(e) => {
                if let Some(ref wt) = wt_resolved.worktree_path {
                    planeai_core::cleanup::cleanup_worktree(
                        &self.project_cwd.to_string_lossy(),
                        wt,
                        Some(&wt_resolved.branch_name),
                    );
                }
                self.set_error(format!("DB lock: {e}"));
                return;
            }
        }

        // Spawn daemon session in worktree cwd
        let id = self.next_id;
        self.next_id += 1;
        let result = DaemonSession::spawn_with_session_id(
            id,
            &session_id,
            self.cols as u16,
            self.rows as u16,
            Some(&resolved.command_label),
            &wt_resolved.cwd,
            &self.extra_path_dirs,
        );
        match result {
            Ok(backend) => {
                // Fire on_start lifecycle hook only after successful spawn
                let db_path = planeai_core::app_data_dir().join("planeai.db");
                if let Err(e) = TaskService::fire_lifecycle_hook(
                    &db_path,
                    &project.name,
                    &task.key,
                    "in_progress",
                ) {
                    tracing::warn!(task_key = %task.key, error = %e, "lifecycle hook (in_progress) failed");
                }

                let term = new_term(self.cols, self.rows);
                let processor = new_processor();
                let snapshot = snapshot_grid(&term);
                let log_file_exists = self.check_log_exists(&session_id);
                self.sessions.push(Session {
                    id,
                    session_id: session_id.clone(),
                    command: resolved.command_label.clone(),
                    cwd: wt_resolved.cwd,
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
                if let Some(ref wt) = wt_resolved.worktree_path {
                    planeai_core::cleanup::cleanup_worktree(
                        &self.project_cwd.to_string_lossy(),
                        wt,
                        Some(&wt_resolved.branch_name),
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
            Message::TaskPickerSelect(idx) => {
                if idx < self.task_list.len() {
                    self.selected_task = Some(self.task_list[idx].clone());
                    self.task_picker = false;
                }
            }
            Message::TaskLaunchSelected => {
                self.launch_from_task();
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

                // New... menu mode
                if self.new_menu {
                    match &key {
                        keyboard::Key::Named(keyboard::key::Named::Escape) => {
                            self.new_menu = false;
                        }
                        keyboard::Key::Named(keyboard::key::Named::ArrowDown) => {
                            self.new_menu_index = (self.new_menu_index + 1).min(1);
                        }
                        keyboard::Key::Named(keyboard::key::Named::ArrowUp) => {
                            self.new_menu_index = self.new_menu_index.saturating_sub(1);
                        }
                        keyboard::Key::Named(keyboard::key::Named::Enter) => {
                            self.new_menu = false;
                            if self.new_menu_index == 0 {
                                self.open_session_form();
                            } else {
                                // TODO: task creation form
                                self.set_error("Task creation not yet implemented.".into());
                            }
                        }
                        _ => {}
                    }
                    return;
                }

                // Session creation form
                if self.session_form {
                    // When Project field is focused — custom combobox
                    if self.session_form_focus == SessionFormField::Project {
                        // Form-level shortcuts first
                        match &key {
                            keyboard::Key::Named(keyboard::key::Named::Escape) => {
                                self.session_form = false;
                                return;
                            }
                            keyboard::Key::Named(keyboard::key::Named::Tab) => {
                                if modifiers.shift() {
                                    self.session_form_focus = SessionFormField::Mode;
                                } else {
                                    self.session_form_focus = match self.session_form_mode {
                                        SessionFormMode::FromTask => SessionFormField::Task,
                                        SessionFormMode::Manual => SessionFormField::Name,
                                    };
                                }
                                return;
                            }
                            _ => {}
                        }
                        let cmd = if cfg!(target_os = "macos") {
                            modifiers.command()
                        } else {
                            modifiers.control()
                        };
                        if cmd && matches!(&key, keyboard::Key::Named(keyboard::key::Named::Enter))
                        {
                            self.submit_session_form();
                            return;
                        }
                        // Delegate to combobox
                        let key_str = match &key {
                            keyboard::Key::Named(keyboard::key::Named::ArrowDown) => "ArrowDown",
                            keyboard::Key::Named(keyboard::key::Named::ArrowUp) => "ArrowUp",
                            keyboard::Key::Named(keyboard::key::Named::Backspace) => "Backspace",
                            keyboard::Key::Named(keyboard::key::Named::Enter) => "Enter",
                            keyboard::Key::Character(c) => c.as_str(),
                            _ => "",
                        };
                        if !key_str.is_empty() {
                            if let Some(selected) =
                                self.session_form_project_combo.handle_key(key_str)
                            {
                                // Project was selected — look up path from DB projects
                                let path = if let Some(ref db) = self.db {
                                    if let Ok(conn) = db.lock() {
                                        ProjectService::get_by_id(&conn, &selected.id)
                                            .ok()
                                            .flatten()
                                            .map(|p| p.path)
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                };
                                if let Some(path) = path {
                                    self.select_project(&path);
                                }
                            }
                        }
                        return;
                    }
                    // When Task field is focused — custom combobox
                    if self.session_form_focus == SessionFormField::Task {
                        match &key {
                            keyboard::Key::Named(keyboard::key::Named::Escape) => {
                                self.session_form = false;
                                return;
                            }
                            keyboard::Key::Named(keyboard::key::Named::Tab) => {
                                if modifiers.shift() {
                                    self.session_form_focus = SessionFormField::Project;
                                } else {
                                    self.session_form_focus = SessionFormField::Name;
                                }
                                return;
                            }
                            _ => {}
                        }
                        let cmd = if cfg!(target_os = "macos") {
                            modifiers.command()
                        } else {
                            modifiers.control()
                        };
                        if cmd && matches!(&key, keyboard::Key::Named(keyboard::key::Named::Enter))
                        {
                            self.submit_session_form();
                            return;
                        }
                        let key_str = match &key {
                            keyboard::Key::Named(keyboard::key::Named::ArrowDown) => "ArrowDown",
                            keyboard::Key::Named(keyboard::key::Named::ArrowUp) => "ArrowUp",
                            keyboard::Key::Named(keyboard::key::Named::Backspace) => "Backspace",
                            keyboard::Key::Named(keyboard::key::Named::Enter) => "Enter",
                            keyboard::Key::Character(c) => c.as_str(),
                            _ => "",
                        };
                        if !key_str.is_empty()
                            && self.session_form_task_combo.handle_key(key_str).is_some()
                        {
                            self.session_form_apply_task();
                        }
                        return;
                    }
                    match &key {
                        keyboard::Key::Named(keyboard::key::Named::Escape) => {
                            self.session_form = false;
                        }
                        keyboard::Key::Named(keyboard::key::Named::Tab) => {
                            if modifiers.shift() {
                                // Reverse cycle
                                self.session_form_focus =
                                    match (&self.session_form_mode, &self.session_form_focus) {
                                        (_, SessionFormField::Mode) => SessionFormField::Branch,
                                        (_, SessionFormField::Branch) => SessionFormField::Toggles,
                                        (_, SessionFormField::Toggles) => SessionFormField::Name,
                                        (SessionFormMode::FromTask, SessionFormField::Name) => {
                                            SessionFormField::Task
                                        }
                                        (SessionFormMode::FromTask, SessionFormField::Task) => {
                                            SessionFormField::Project
                                        }
                                        (SessionFormMode::Manual, SessionFormField::Name) => {
                                            SessionFormField::Project
                                        }
                                        (_, SessionFormField::Project) => SessionFormField::Mode,
                                        _ => SessionFormField::Mode,
                                    };
                            } else {
                                // Forward cycle
                                self.session_form_focus =
                                    match (&self.session_form_mode, &self.session_form_focus) {
                                        (_, SessionFormField::Mode) => SessionFormField::Project,
                                        (SessionFormMode::FromTask, SessionFormField::Project) => {
                                            SessionFormField::Task
                                        }
                                        (SessionFormMode::Manual, SessionFormField::Project) => {
                                            SessionFormField::Name
                                        }
                                        (SessionFormMode::FromTask, SessionFormField::Task) => {
                                            SessionFormField::Name
                                        }
                                        (_, SessionFormField::Name) => SessionFormField::Toggles,
                                        (_, SessionFormField::Toggles) => SessionFormField::Branch,
                                        (_, SessionFormField::Branch) => SessionFormField::Mode,
                                        _ => SessionFormField::Mode,
                                    };
                            }
                        }
                        keyboard::Key::Named(keyboard::key::Named::Enter) => {
                            let cmd = if cfg!(target_os = "macos") {
                                modifiers.command()
                            } else {
                                modifiers.control()
                            };
                            if cmd {
                                self.submit_session_form();
                            } else if self.session_form_focus == SessionFormField::Mode {
                                // Toggle mode on Enter at mode field
                                self.session_form_mode = match self.session_form_mode {
                                    SessionFormMode::Manual => {
                                        self.session_form_load_tasks();
                                        SessionFormMode::FromTask
                                    }
                                    SessionFormMode::FromTask => SessionFormMode::Manual,
                                };
                            } else if self.session_form_focus == SessionFormField::Task {
                                // Handled by task combobox above
                            }
                        }
                        keyboard::Key::Named(keyboard::key::Named::ArrowDown) => {}
                        keyboard::Key::Named(keyboard::key::Named::ArrowUp) => {}
                        keyboard::Key::Named(keyboard::key::Named::ArrowLeft) => {
                            if self.session_form_focus == SessionFormField::Mode {
                                self.session_form_mode = SessionFormMode::Manual;
                            }
                        }
                        keyboard::Key::Named(keyboard::key::Named::ArrowRight) => {
                            if self.session_form_focus == SessionFormField::Mode {
                                self.session_form_mode = SessionFormMode::FromTask;
                                self.session_form_load_tasks();
                            }
                        }
                        keyboard::Key::Named(keyboard::key::Named::Backspace) => {
                            match self.session_form_focus {
                                SessionFormField::Name => {
                                    self.session_form_name.pop();
                                }
                                SessionFormField::Branch => {
                                    self.session_form_branch.pop();
                                }
                                _ => {}
                            }
                        }
                        keyboard::Key::Character(c) => {
                            let ch = c.as_str();
                            // Toggles: w=worktree, a=auto-approve, p=cycle provider
                            if self.session_form_focus == SessionFormField::Toggles {
                                match ch {
                                    "w" => {
                                        self.session_form_use_worktree =
                                            !self.session_form_use_worktree
                                    }
                                    "a" => {
                                        self.session_form_auto_approve =
                                            !self.session_form_auto_approve
                                    }
                                    "p" if !self.provider_keys.is_empty() => {
                                        self.session_form_provider_idx =
                                            (self.session_form_provider_idx + 1)
                                                % self.provider_keys.len();
                                    }
                                    _ => {}
                                }
                            } else if self.session_form_focus == SessionFormField::Name {
                                self.session_form_name.push_str(ch);
                            } else if self.session_form_focus == SessionFormField::Branch {
                                self.session_form_branch.push_str(ch);
                            } else if self.session_form_focus == SessionFormField::Mode {
                                match ch {
                                    "m" => self.session_form_mode = SessionFormMode::Manual,
                                    "t" => {
                                        self.session_form_mode = SessionFormMode::FromTask;
                                        self.session_form_load_tasks();
                                    }
                                    _ => {}
                                }
                            }
                        }
                        _ => {}
                    }
                    return;
                }

                // Task picker mode
                if self.task_picker {
                    match &key {
                        keyboard::Key::Named(keyboard::key::Named::Escape) => {
                            self.task_picker = false;
                        }
                        keyboard::Key::Named(keyboard::key::Named::ArrowDown) => {
                            if !self.task_list.is_empty() {
                                self.task_picker_index =
                                    (self.task_picker_index + 1).min(self.task_list.len() - 1);
                            }
                        }
                        keyboard::Key::Named(keyboard::key::Named::ArrowUp) => {
                            self.task_picker_index = self.task_picker_index.saturating_sub(1);
                        }
                        keyboard::Key::Named(keyboard::key::Named::Enter) => {
                            if self.task_picker_index < self.task_list.len() {
                                self.selected_task =
                                    Some(self.task_list[self.task_picker_index].clone());
                                self.task_picker = false;
                            }
                        }
                        _ => {}
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

                // Cmd+N — open "New..." menu
                if cmd
                    && !modifiers.shift()
                    && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "n")
                {
                    self.new_menu = true;
                    self.new_menu_index = 0;
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

                // Cmd+T — task picker
                if cmd
                    && !modifiers.shift()
                    && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "t")
                {
                    self.open_task_picker();
                    self.kill_armed = false;
                    return;
                }

                // Cmd+Enter — launch selected task
                if cmd && matches!(key, keyboard::Key::Named(keyboard::key::Named::Enter)) {
                    if self.selected_task.is_some() {
                        self.launch_from_task();
                    }
                    return;
                }

                // Cmd+Shift+T — clear selected task
                if cmd
                    && modifiers.shift()
                    && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "t" || c.as_str() == "T")
                {
                    self.selected_task = None;
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
                        // Mark exited in DB and fire lifecycle hook
                        if let Some(ref db) = self.db {
                            if let Ok(conn) = db.lock() {
                                let _ = SessionService::mark_exited(&conn, &s.session_id);
                                // Fire on_complete for task-linked sessions
                                if let Ok(Some(rec)) = SessionService::get(&conn, &s.session_id) {
                                    if let Some(ref tk) = rec.task_key {
                                        if let Ok(Some(proj)) =
                                            ProjectService::get_by_id(&conn, &rec.project_id)
                                        {
                                            let db_path =
                                                planeai_core::app_data_dir().join("planeai.db");
                                            if let Err(e) = TaskService::fire_lifecycle_hook(
                                                &db_path, &proj.name, tk, "done",
                                            ) {
                                                tracing::warn!(task_key = %tk, error = %e, "lifecycle hook (done) failed");
                                            }
                                        }
                                    }
                                }
                            }
                        }
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
        let task_display = self
            .selected_task
            .as_ref()
            .map(|t| format!(" | 📋 {}: {}", t.key, t.title))
            .unwrap_or_default();
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
            " {} {}{}{}{}",
            if self.daemon_connected { "⚡" } else { "⚠" },
            project_display,
            task_display,
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
        } else if self.new_menu {
            let items = ["Session", "Task"];
            let mut nm_col = column![].spacing(2).width(Length::Fill).padding(6);
            nm_col = nm_col.push(
                text("New... (↑↓ navigate, Enter select, Escape cancel)")
                    .size(12)
                    .color(Color::from_rgb8(180, 220, 255))
                    .font(Font::MONOSPACE),
            );
            for (i, item) in items.iter().enumerate() {
                let marker = if i == self.new_menu_index { "▶" } else { " " };
                let color = if i == self.new_menu_index {
                    Color::from_rgb8(100, 220, 255)
                } else {
                    Color::from_rgb8(180, 180, 180)
                };
                nm_col = nm_col.push(
                    text(format!("{} {}", marker, item))
                        .size(12)
                        .color(color)
                        .font(Font::MONOSPACE),
                );
            }
            let nm_panel = container(nm_col)
                .width(Length::Fixed(300.0))
                .padding(12)
                .style(|_: &Theme| container::Style {
                    background: Some(Color::from_rgb8(30, 40, 55).into()),
                    border: iced::Border {
                        color: Color::from_rgb8(60, 70, 90),
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                });
            let overlay = container(nm_panel)
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .style(|_: &Theme| container::Style {
                    background: Some(Color::from_rgba8(0, 0, 0, 0.6).into()),
                    ..Default::default()
                });
            {
                use iced::widget::stack;
                let base_el: Element<'_, Message> = main_content.into();
                stack![base_el, overlay].into()
            }
        } else if self.session_form {
            let mut sf_col = column![].spacing(3).width(Length::Fill).padding(8);
            sf_col = sf_col.push(
                text("New Session")
                    .size(13)
                    .color(Color::from_rgb8(255, 255, 255))
                    .font(Font::MONOSPACE),
            );

            // Mode toggle
            let mode_highlight = self.session_form_focus == SessionFormField::Mode;
            let mode_prefix = if mode_highlight { "▶ " } else { "  " };
            sf_col = sf_col.push(
                text(format!(
                    "{}[{}Manual{}]  [{}From task{}]",
                    mode_prefix,
                    if self.session_form_mode == SessionFormMode::Manual {
                        "●"
                    } else {
                        " "
                    },
                    " M",
                    if self.session_form_mode == SessionFormMode::FromTask {
                        "●"
                    } else {
                        " "
                    },
                    " T",
                ))
                .size(11)
                .color(if mode_highlight {
                    Color::from_rgb8(200, 220, 255)
                } else {
                    Color::from_rgb8(160, 160, 160)
                })
                .font(Font::MONOSPACE),
            );

            // Project (custom combobox)
            let proj_focused = self.session_form_focus == SessionFormField::Project;
            sf_col = sf_col.push(
                self.session_form_project_combo
                    .view::<Message>("Project", proj_focused),
            );

            // Task picker (From task mode only)
            if self.session_form_mode == SessionFormMode::FromTask {
                let task_focused = self.session_form_focus == SessionFormField::Task;
                sf_col = sf_col.push(
                    self.session_form_task_combo
                        .view::<Message>("Task", task_focused),
                );
            }

            // Name field
            let name_highlight = self.session_form_focus == SessionFormField::Name;
            let name_prefix = if name_highlight { "▶ " } else { "  " };
            let name_display = if self.session_form_name.is_empty() {
                "(auto)".to_string()
            } else {
                self.session_form_name.clone()
            };
            sf_col = sf_col.push(
                text(format!(
                    "{}Name: {}{}",
                    name_prefix,
                    name_display,
                    if name_highlight { "▏" } else { "" }
                ))
                .size(11)
                .color(if name_highlight {
                    Color::from_rgb8(100, 220, 255)
                } else {
                    Color::from_rgb8(160, 160, 160)
                })
                .font(Font::MONOSPACE),
            );

            // Toggles
            let toggles_highlight = self.session_form_focus == SessionFormField::Toggles;
            let toggles_prefix = if toggles_highlight { "▶ " } else { "  " };
            let wt_mark = if self.session_form_use_worktree {
                "●"
            } else {
                "○"
            };
            let aa_mark = if self.session_form_auto_approve {
                "●"
            } else {
                "○"
            };
            let provider_name = self
                .provider_keys
                .get(self.session_form_provider_idx)
                .cloned()
                .unwrap_or_else(|| self.provider_label.clone());
            sf_col = sf_col.push(
                text(format!(
                    "{}[{}] Worktree W  [{}] Auto-approve A  Provider: {} P",
                    toggles_prefix, wt_mark, aa_mark, provider_name
                ))
                .size(11)
                .color(if toggles_highlight {
                    Color::from_rgb8(100, 220, 255)
                } else {
                    Color::from_rgb8(160, 160, 160)
                })
                .font(Font::MONOSPACE),
            );

            // Branch field
            let branch_highlight = self.session_form_focus == SessionFormField::Branch;
            let branch_prefix = if branch_highlight { "▶ " } else { "  " };
            let branch_display = if self.session_form_branch.is_empty() {
                "(auto from name)".to_string()
            } else {
                self.session_form_branch.clone()
            };
            sf_col = sf_col.push(
                text(format!(
                    "{}Branch: {}{}",
                    branch_prefix,
                    branch_display,
                    if branch_highlight { "▏" } else { "" }
                ))
                .size(11)
                .color(if branch_highlight {
                    Color::from_rgb8(100, 220, 255)
                } else {
                    Color::from_rgb8(160, 160, 160)
                })
                .font(Font::MONOSPACE),
            );

            // Error
            if let Some(ref err) = self.session_form_error {
                sf_col = sf_col.push(
                    text(format!("  ⚠ {}", err))
                        .size(11)
                        .color(Color::from_rgb8(255, 100, 100))
                        .font(Font::MONOSPACE),
                );
            }

            // Footer
            sf_col = sf_col.push(
                text("  Tab=next field | Cmd+Enter=create | Escape=cancel")
                    .size(10)
                    .color(Color::from_rgb8(80, 80, 80))
                    .font(Font::MONOSPACE),
            );

            let sf_panel = container(sf_col)
                .width(Length::Fixed(500.0))
                .padding(16)
                .style(|_: &Theme| container::Style {
                    background: Some(Color::from_rgb8(25, 30, 40).into()),
                    border: iced::Border {
                        color: Color::from_rgb8(60, 70, 90),
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                });
            // Render as modal overlay
            let overlay = container(sf_panel)
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .style(|_: &Theme| container::Style {
                    background: Some(Color::from_rgba8(0, 0, 0, 0.6).into()),
                    ..Default::default()
                });
            {
                use iced::widget::stack;
                let base_el: Element<'_, Message> = main_content.into();
                stack![base_el, overlay].into()
            }
        } else if self.task_picker {
            let mut tp_col = column![].spacing(2).width(Length::Fill).padding(6);
            tp_col = tp_col.push(
                text("Task Picker (↑↓ navigate, Enter select, Escape cancel)")
                    .size(12)
                    .color(Color::from_rgb8(180, 220, 255))
                    .font(Font::MONOSPACE),
            );
            if self.task_list.is_empty() {
                tp_col = tp_col.push(
                    text("  No tasks found for this project.")
                        .size(11)
                        .color(Color::from_rgb8(150, 150, 150))
                        .font(Font::MONOSPACE),
                );
            } else {
                for (i, task) in self.task_list.iter().take(15).enumerate() {
                    let marker = if i == self.task_picker_index {
                        "▶"
                    } else {
                        " "
                    };
                    let label = format!(
                        "{} [{}] {} — {}",
                        marker,
                        task.key,
                        task.title,
                        task.status.as_str()
                    );
                    let color = if i == self.task_picker_index {
                        Color::from_rgb8(100, 220, 255)
                    } else {
                        Color::from_rgb8(180, 180, 180)
                    };
                    tp_col = tp_col.push(text(label).size(11).color(color).font(Font::MONOSPACE));
                }
            }
            let tp_panel = container(tp_col).style(|_: &Theme| container::Style {
                background: Some(Color::from_rgb8(30, 40, 55).into()),
                ..Default::default()
            });
            column![tp_panel, main_content].into()
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
    // Initialize logging to same location as Tauri app
    let log_dir = planeai_core::app_data_dir().join("logs");
    let _ = std::fs::create_dir_all(&log_dir);
    let file_appender = tracing_appender::rolling::daily(&log_dir, "planeai.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(non_blocking)
        .with_ansi(false)
        .init();
    tracing::info!("planeai-iced starting");

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
