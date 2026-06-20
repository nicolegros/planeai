//! PlaneAI Workflow Shell — orchestrates daemon sessions with project context.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use planeai_core::services::{
    self, CreateSessionParams, ProjectService, SessionRecord, SessionService, TaskLaunchRequest,
    TaskService, WorktreeMode, WorktreeService,
};
use planeai_core::tab_switcher::TabSwitcher;
use rusqlite::Connection;
use std::sync::Mutex;

use arboard::Clipboard;
use iced::keyboard;
use iced::widget::{column, container, row, text, text_input, Canvas};
use iced::{event, window, Color, Element, Length, Size, Subscription, Theme};

use crate::adapter::PlaneAiTerminalSession;
use crate::common::*;
use crate::components::{modal_overlay, ComboBoxState, ComboItem};
use crate::daemon_session::{
    attach, daemon_is_connected, detach_daemon_session, ensure_daemon_running_sync,
    kill_daemon_session, list_daemon_sessions, DaemonSession, DaemonSessionInfo,
};
use crate::input;
use crate::project_form::{self, ProjectFormState};
use crate::sidebar::{SidebarAction, SidebarState};
use crate::terminal_view::{TerminalRenderer, TerminalView};
use crate::theme::{self, PlaneAiTheme, ThemeSource};
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
    terminal: TerminalView,
    bytes_processed: u64,
    log_file_exists: bool,
}

// ─── Log replay state ────────────────────────────────────────────────────────

#[allow(dead_code)]
struct LogReplayState {
    terminal: TerminalView,
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
    // Project creation form (Cmd+Shift+N)
    project_form: ProjectFormState,
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
    // Sidebar
    sidebar: Option<SidebarState>,
    sidebar_focused: bool,
    sidebar_dirty: bool,
    // Tab switcher (MRU-based Ctrl+Tab cycling)
    tab_switcher: TabSwitcher,
    tab_switcher_names: Vec<String>,
    mru: Vec<String>,
    // Command palette (Cmd+K)
    command_palette: Option<crate::command_palette::CommandPaletteState>,
    // Theme
    theme_source: ThemeSource,
    theme: PlaneAiTheme,
    // Notify / agent state
    notify_state: planeai_core::notify::SharedNotifyState,
    agent_states: std::collections::HashMap<String, planeai_core::notify::AgentState>,
    // Hook install banner
    show_hook_banner: bool,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
enum Message {
    Poll,
    KeyEvent(keyboard::Event),
    WindowResized(Size),
    SidebarDrag(f32),
    SidebarDragEnd,
    SidebarMouseDown(f32),
    ProjectInputChanged(String),
    ProjectInputSubmit,
    LaunchPromptChanged(String),
    LaunchPromptSubmit,
    ProjectForm(project_form::FormMessage),
    WorktreeBranchChanged(String),
    WorktreeTaskKeyChanged(String),
    WorktreeToggle,
    WorktreeLaunchSubmit,
    TaskPickerSelect(usize),
    TaskLaunchSelected,
    FontLoaded,
    TitleBarDrag,
    TerminalScroll(f32),
    SidebarItemClicked(usize),
    SidebarScrolled(iced::widget::scrollable::Viewport),
    AgentStateChanged {
        session_id: String,
        state: planeai_core::notify::AgentState,
    },
    CheckSilence,
    NotifyIpcMessage(planeai_core::notify::NotifyMessage),
    InstallHooks,
    DismissHookBanner,
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

        // Open shared PlaneAI DB (don't create a project — user must add one via Cmd+Shift+N)
        let db = match services::open_db() {
            Ok(conn) => Some(Arc::new(Mutex::new(conn))),
            Err(e) => {
                boot_warnings.push(format!("DB open failed: {e} — sessions won't persist"));
                None
            }
        };
        let project: Option<services::Project> = None;

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
        // Clamp initial cols/rows to fit the default window (1200x800) minus sidebar
        let (cw, ch) = planeai_iced_spike::font::cell_dimensions(
            planeai_iced_spike::font::terminal_font_size(),
        );
        let max_cols = ((1200.0 - 224.0) / cw).floor().max(2.0) as usize;
        let max_rows = ((800.0 - 40.0) / ch).floor().max(2.0) as usize;
        let cols = (resolved.request.cols as usize).min(max_cols);
        let rows = (resolved.request.rows as usize).min(max_rows);

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
                project_form: ProjectFormState::default(),
                kill_armed: false,
                db,
                project,
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
                sidebar: None,
                sidebar_focused: false,
                sidebar_dirty: true, // trigger initial load
                tab_switcher: TabSwitcher::new(),
                tab_switcher_names: Vec::new(),
                mru: persisted_sessions.iter().map(|s| s.id.clone()).collect(),
                command_palette: None,
                persisted_sessions,
                theme_source: ThemeSource::load(),
                theme: theme::default_dark_theme(),
                notify_state: Arc::new(Mutex::new(planeai_core::notify::NotifyState::new())),
                agent_states: std::collections::HashMap::new(),
                show_hook_banner: {
                    // Check if any hooks need installation
                    let home = std::env::var("HOME").unwrap_or_default();
                    let kiro_ok = planeai_core::notify::is_kiro_hook_installed_at(
                        &std::path::PathBuf::from(&home).join(".kiro/agents/default.json"),
                    );
                    let claude_ok = planeai_core::notify::is_claude_hook_installed_at(
                        &std::path::PathBuf::from(&home).join(".claude/settings.json"),
                    );
                    // Show banner if either is missing (and the tool is likely installed)
                    let kiro_exists = std::path::Path::new(&format!("{home}/.kiro")).exists();
                    let claude_exists = std::path::Path::new(&format!("{home}/.claude")).exists();
                    (kiro_exists && !kiro_ok) || (claude_exists && !claude_ok)
                },
            },
            planeai_iced_spike::font::font_load_task().map(|_| Message::FontLoaded),
        );
        // Resolve theme from source
        result.0.theme = result
            .0
            .theme_source
            .resolve(result.0.theme_source.current_mode());
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
        let project_id = match &self.project {
            Some(p) => p.id.clone(),
            None => {
                self.set_error("No project available.".into());
                return;
            }
        };
        let params = CreateSessionParams {
            id: session_id.clone(),
            project_id,
            name: String::new(),
            backend: "daemon".to_string(),
            auto_approve: true,
            ..Default::default()
        };
        let persist_err = self.persist_new_session(params);
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
                let terminal = TerminalView::new(self.cols, self.rows);
                let log_file_exists = self.check_log_exists(&session_id);
                self.sessions.push(Session {
                    id,
                    session_id: session_id.clone(),
                    command: self.agent_command.clone(),
                    cwd: self.project_cwd.clone(),
                    status: SessionStatus::Running,
                    backend: Box::new(backend),
                    terminal,
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
        let project_id = match &self.project {
            Some(p) => p.id.clone(),
            None => {
                self.set_error("No project available.".into());
                return;
            }
        };
        let params = CreateSessionParams {
            id: session_id.clone(),
            project_id,
            name: String::new(),
            backend: "daemon".to_string(),
            auto_approve: true,
            ..Default::default()
        };
        let persist_err = self.persist_new_session(params);
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
                let terminal = TerminalView::new(self.cols, self.rows);
                let log_file_exists = self.check_log_exists(&session_id);
                self.sessions.push(Session {
                    id,
                    session_id: session_id.clone(),
                    command: command.to_string(),
                    cwd: self.project_cwd.clone(),
                    status: SessionStatus::Running,
                    backend: Box::new(backend),
                    terminal,
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
                let terminal = TerminalView::new(self.cols, self.rows);
                let log_file_exists = self.check_log_exists(&session_id);
                self.sessions.push(Session {
                    id,
                    session_id: session_id.clone(),
                    command: "attached".to_string(),
                    cwd: self.project_cwd.clone(),
                    status: SessionStatus::Attached,
                    backend: Box::new(backend),
                    terminal,
                    bytes_processed: 0,
                    log_file_exists,
                });
                self.active = self.sessions.len() - 1;
                // Touch MRU for newly attached session
                self.mru.retain(|id| id != &session_id);
                self.mru.insert(0, session_id);
                self.clear_error();
                self.sidebar_dirty = true;
                self.refresh_persisted_sessions();
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
        self.sidebar_dirty = true;
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
        self.sidebar_dirty = true;
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
                // Merge DB order with existing MRU (keep entries not in DB)
                let db_ids: Vec<String> = self
                    .persisted_sessions
                    .iter()
                    .map(|s| s.id.clone())
                    .collect();
                // Add any DB entries not already in MRU
                for id in &db_ids {
                    if !self.mru.contains(id) {
                        self.mru.push(id.clone());
                    }
                }
            }
        }
    }

    /// Persist a new session record to the shared DB. Returns Some(error_msg) on failure.
    fn persist_new_session(&self, params: CreateSessionParams) -> Option<String> {
        if let Some(ref db) = self.db {
            match db.lock() {
                Ok(conn) => {
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
        let project_id = match &self.project {
            Some(proj) => proj.id.clone(),
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
        let params = CreateSessionParams {
            id: session_id.clone(),
            project_id,
            name: String::new(),
            backend: "daemon".to_string(),
            auto_approve: true,
            branch: resolved.branch_name.clone(),
            worktree_path: resolved.worktree_path.clone(),
            task_key: task_key.clone(),
            base_branch: resolved.base_branch.clone(),
            ..Default::default()
        };
        if let Some(msg) = self.persist_new_session(params) {
            if let Some(ref wt) = resolved.worktree_path {
                planeai_core::cleanup::cleanup_worktree(
                    &self.project_cwd.to_string_lossy(),
                    wt,
                    Some(&resolved.branch_name),
                );
            }
            self.set_error(msg);
            return;
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
                let terminal = TerminalView::new(self.cols, self.rows);
                let log_file_exists = self.check_log_exists(&session_id);
                self.sessions.push(Session {
                    id,
                    session_id: session_id.clone(),
                    command: self.agent_command.clone(),
                    cwd: resolved.cwd,
                    status: SessionStatus::Running,
                    backend: Box::new(backend),
                    terminal,
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
        let session_name = self.session_form_name.clone();
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
        if let Some(msg) = self.persist_new_session(params) {
            self.session_form_error = Some(msg);
            return;
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
                let terminal = TerminalView::new(self.cols, self.rows);
                let log_file_exists = self.check_log_exists(&session_id);
                self.sessions.push(Session {
                    id,
                    session_id,
                    command: cmd,
                    cwd: working_dir,
                    status: SessionStatus::Running,
                    backend: Box::new(backend),
                    terminal,
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
        let (_db, project) = match (&self.db, &self.project) {
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
        if let Some(msg) = self.persist_new_session(params) {
            if let Some(ref wt) = wt_resolved.worktree_path {
                planeai_core::cleanup::cleanup_worktree(
                    &self.project_cwd.to_string_lossy(),
                    wt,
                    Some(&wt_resolved.branch_name),
                );
            }
            self.set_error(msg);
            return;
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

                let terminal = TerminalView::new(self.cols, self.rows);
                let log_file_exists = self.check_log_exists(&session_id);
                self.sessions.push(Session {
                    id,
                    session_id: session_id.clone(),
                    command: resolved.command_label.clone(),
                    cwd: wt_resolved.cwd,
                    status: SessionStatus::Running,
                    backend: Box::new(backend),
                    terminal,
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

    fn fire_notification(&self, session_id: &str) {
        let ns = self.notify_state.lock().unwrap();
        let (title, body) = match ns.get_meta(session_id) {
            Some(meta) => (meta.project_name.clone(), format!("{} is ready", meta.name)),
            None => ("planeai".to_string(), "Agent is ready".to_string()),
        };
        drop(ns);
        let _ = notify_rust::Notification::new()
            .summary(&title)
            .body(&body)
            .show();
    }

    fn close_all_overlays(&mut self) {
        self.picking_project = false;
        self.launch_prompt = false;
        self.worktree_prompt = false;
        self.task_picker = false;
        self.new_menu = false;
        self.session_form = false;
        self.show_shortcuts = false;
    }

    fn build_palette_items(&self) -> crate::command_palette::CommandPaletteState {
        use crate::command_palette::{CommandPaletteState, PaletteItem};

        let mut items = Vec::new();

        // Sessions group — query DB for all active sessions (like sidebar)
        let attached_ids: Vec<&str> = self
            .sessions
            .iter()
            .map(|s| s.session_id.as_str())
            .collect();
        let active_sid = self
            .sessions
            .get(self.active)
            .map(|s| s.session_id.as_str());
        let all_sessions: Vec<services::SessionRecord> = self
            .db
            .as_ref()
            .and_then(|db| db.lock().ok())
            .map(|conn| services::SessionService::list_active(&conn).unwrap_or_default())
            .unwrap_or_default();

        if all_sessions.is_empty() {
            // No DB — fall back to attached sessions
            for (i, session) in self.sessions.iter().enumerate() {
                let label = format!("Session {}", i + 1);
                items.push(PaletteItem {
                    id: format!("session_id:{}", session.session_id),
                    label,
                    group: "Sessions".into(),
                    is_active: i == self.active,
                });
            }
        } else {
            for record in &all_sessions {
                let label = if !record.name.is_empty() {
                    record.name.clone()
                } else if !record.branch.is_empty() {
                    record.branch.clone()
                } else {
                    record.id[..record.id.len().min(8)].to_string()
                };
                let is_attached = attached_ids.contains(&record.id.as_str());
                let suffix = if !is_attached { " (detached)" } else { "" };
                items.push(PaletteItem {
                    id: format!("session_id:{}", record.id),
                    label: format!("{label}{suffix}"),
                    group: "Sessions".into(),
                    is_active: active_sid == Some(record.id.as_str()),
                });
            }
        }

        // Actions group
        items.push(PaletteItem {
            id: "action:new".into(),
            label: "New session".into(),
            group: "Actions".into(),
            is_active: false,
        });
        if !self.sessions.is_empty() {
            items.push(PaletteItem {
                id: "action:kill".into(),
                label: "Kill session".into(),
                group: "Actions".into(),
                is_active: false,
            });
            items.push(PaletteItem {
                id: "action:detach".into(),
                label: "Detach session".into(),
                group: "Actions".into(),
                is_active: false,
            });
        }
        items.push(PaletteItem {
            id: "action:shortcuts".into(),
            label: "Toggle shortcuts".into(),
            group: "Actions".into(),
            is_active: false,
        });

        // Tasks group (only if already loaded)
        for (i, task) in self.task_list.iter().enumerate() {
            items.push(PaletteItem {
                id: format!("task:{i}"),
                label: format!("{}: {}", task.key, task.title),
                group: "Tasks".into(),
                is_active: false,
            });
        }

        CommandPaletteState::new(items)
    }

    fn dispatch_palette_action(&mut self, id: &str) {
        if let Some(sid) = id.strip_prefix("session_id:") {
            // If already attached, switch to it
            if let Some(idx) = self.sessions.iter().position(|s| s.session_id == sid) {
                self.switch_to(idx);
            } else {
                // Detached — attach it
                self.attach_session(sid.to_string());
            }
        } else if id == "action:new" {
            self.open_session_form();
        } else if id == "action:kill" {
            self.kill_active();
        } else if id == "action:detach" {
            self.detach_active();
        } else if id == "action:shortcuts" {
            self.show_shortcuts = !self.show_shortcuts;
        } else if let Some(rest) = id.strip_prefix("task:") {
            if let Ok(idx) = rest.parse::<usize>() {
                if idx < self.task_list.len() {
                    self.selected_task = Some(self.task_list[idx].clone());
                }
            }
        }
    }

    fn switch_to(&mut self, idx: usize) {
        if idx < self.sessions.len() && idx != self.active {
            self.active = idx;
            let session = &mut self.sessions[idx];
            session.terminal.update_snapshot(&self.theme.terminal);
            if let Some(ref mut sidebar) = self.sidebar {
                sidebar.set_active_session(Some(self.sessions[idx].session_id.clone()));
            }
            // Touch MRU
            let sid = self.sessions[idx].session_id.clone();
            self.mru.retain(|id| id != &sid);
            self.mru.insert(0, sid);
            // Persist MRU to DB
            if let Some(ref db) = self.db {
                if let Ok(conn) = db.lock() {
                    let refs: Vec<&str> = self.mru.iter().map(|s| s.as_str()).collect();
                    let _ = SessionService::save_mru_order(&conn, &refs);
                }
            }
        }
    }

    fn handle_sidebar_key(&mut self, key: &keyboard::Key) {
        let key_str = match key {
            keyboard::Key::Named(keyboard::key::Named::ArrowDown) => "ArrowDown",
            keyboard::Key::Named(keyboard::key::Named::ArrowUp) => "ArrowUp",
            keyboard::Key::Named(keyboard::key::Named::ArrowLeft) => "ArrowLeft",
            keyboard::Key::Named(keyboard::key::Named::ArrowRight) => "ArrowRight",
            keyboard::Key::Named(keyboard::key::Named::Enter) => "Enter",
            keyboard::Key::Named(keyboard::key::Named::Escape) => "Escape",
            keyboard::Key::Character(c) => c.as_str(),
            _ => return,
        };
        if let Some(ref mut sidebar) = self.sidebar {
            if let Some(action) = sidebar.handle_key(key_str) {
                match action {
                    SidebarAction::FocusTerminal => {
                        self.sidebar_focused = false;
                    }
                    SidebarAction::SwitchSession(sid) => {
                        if let Some(idx) = self.sessions.iter().position(|s| s.session_id == sid) {
                            self.switch_to(idx);
                        } else {
                            self.attach_session(sid);
                        }
                        self.sidebar_focused = false;
                    }
                }
            }
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
        self.sidebar_dirty = true;
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
        let mut tv = TerminalView::new(self.cols, self.rows);
        tv.processor.advance(&mut tv.term, &data);
        tv.update_snapshot(&self.theme.terminal);
        self.log_replay = Some(LogReplayState {
            terminal: tv,
            session_id,
        });
    }
}

impl WorkflowApp {
    fn update(&mut self, message: Message) -> iced::Task<Message> {
        match message {
            Message::TitleBarDrag => {
                return window::oldest().and_then(window::drag);
            }
            Message::SidebarItemClicked(index) => {
                if let Some(ref mut sidebar) = self.sidebar {
                    if let Some(action) = sidebar.handle_click(index) {
                        match action {
                            SidebarAction::FocusTerminal => {
                                self.sidebar_focused = false;
                            }
                            SidebarAction::SwitchSession(sid) => {
                                if let Some(idx) =
                                    self.sessions.iter().position(|s| s.session_id == sid)
                                {
                                    self.switch_to(idx);
                                } else {
                                    self.attach_session(sid);
                                }
                                self.sidebar_focused = false;
                            }
                        }
                    }
                }
            }
            Message::SidebarMouseDown(_) => {
                if let Some(ref mut sidebar) = self.sidebar {
                    sidebar.handle_mouse_down();
                }
            }
            Message::SidebarScrolled(viewport) => {
                if let Some(ref mut sidebar) = self.sidebar {
                    sidebar.on_scrolled(viewport);
                }
            }
            Message::SidebarDrag(x) => {
                // Only track cursor position if sidebar exists; resize check is O(1)
                if let Some(ref mut sidebar) = self.sidebar {
                    sidebar.handle_mouse_move(x);
                }
            }
            Message::SidebarDragEnd => {
                if let Some(ref mut sidebar) = self.sidebar {
                    if let Some(ref db) = self.db {
                        let conn = db.lock().unwrap();
                        sidebar.handle_mouse_up(&conn);
                    }
                }
            }
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
            Message::ProjectForm(msg) => {
                if let Some(result) = self.project_form.update(msg, &self.db) {
                    match result {
                        project_form::SubmitResult::Created(proj, path) => {
                            self.project = Some(proj);
                            self.project_cwd = path.clone();
                            self.recent_projects = add_recent_project(&path.to_string_lossy());
                            self.refresh_persisted_sessions();
                            self.sidebar_dirty = true;
                            self.clear_error();
                        }
                        project_form::SubmitResult::Error(_) => {}
                    }
                }
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
            Message::FontLoaded => {}
            Message::AgentStateChanged { session_id, state } => {
                self.agent_states.insert(session_id, state);
            }
            Message::InstallHooks => {
                let home = std::env::var("HOME").unwrap_or_default();
                // Install all hooks via the shared planeai-core functions
                let _ = planeai_core::notify::install_claude_hook_at(
                    &std::path::PathBuf::from(&home).join(".claude"),
                    &format!("{home}/.claude/hooks/planeai-stop-notify-claude.sh"),
                );
                let copilot_dir = std::env::var("COPILOT_HOME")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|_| std::path::PathBuf::from(&home).join(".copilot"));
                let _ = planeai_core::notify::install_copilot_hook_at(
                    &copilot_dir,
                    &format!("{}/hooks/planeai-stop-notify-copilot.sh", copilot_dir.display()),
                    &format!("{}/hooks/planeai-stop-notify-copilot.ps1", copilot_dir.display()),
                );
                self.show_hook_banner = false;
            }
            Message::DismissHookBanner => {
                self.show_hook_banner = false;
            }
            Message::CheckSilence => {
                let mut to_notify: Vec<(String, planeai_core::notify::AgentState)> = Vec::new();
                {
                    let mut ns = self.notify_state.lock().unwrap();
                    let busy = ns.busy_sessions();
                    for id in busy {
                        if ns.check_silence(&id) {
                            to_notify.push((id, planeai_core::notify::AgentState::Idle));
                        }
                    }
                    let debounced = ns.debounced_sessions();
                    for id in debounced {
                        if ns.check_debounce(&id) {
                            to_notify.push((id, planeai_core::notify::AgentState::Idle));
                        }
                    }
                }
                for (session_id, state) in to_notify {
                    tracing::info!(session_id = %session_id, "notify: idle (silence/debounce timeout)");
                    self.fire_notification(&session_id);
                    self.agent_states.insert(session_id, state);
                }
            }
            Message::NotifyIpcMessage(msg) => {
                use planeai_core::notify::{AgentState as AS, NotifyEvent as NE};
                match msg.event {
                    NE::Busy => {
                        self.notify_state.lock().unwrap().notify_output(&msg.session_id);
                        self.agent_states.insert(msg.session_id.clone(), AS::Busy);
                        tracing::debug!(session_id = %msg.session_id, "notify: busy (hook)");
                    }
                    NE::Notification => {
                        let fired = self
                            .notify_state
                            .lock()
                            .unwrap()
                            .notify_stop_immediate(&msg.session_id);
                        if fired {
                            tracing::info!(session_id = %msg.session_id, "notify: idle (immediate)");
                            self.fire_notification(&msg.session_id);
                            self.agent_states.insert(msg.session_id, AS::Idle);
                        }
                    }
                    NE::Stop => {
                        let mut ns = self.notify_state.lock().unwrap();
                        let hook_enabled =
                            ns.get_meta(&msg.session_id).is_some_and(|m| m.hook_enabled);
                        if hook_enabled {
                            tracing::debug!(session_id = %msg.session_id, "notify: stop (debouncing)");
                            ns.notify_stop_debounced(&msg.session_id);
                        } else {
                            let fired = ns.notify_stop(&msg.session_id);
                            drop(ns);
                            if fired {
                                tracing::info!(session_id = %msg.session_id, "notify: idle (stop, no hook)");
                                self.fire_notification(&msg.session_id);
                                self.agent_states
                                    .insert(msg.session_id, AS::Idle);
                            }
                        }
                    }
                    NE::SessionCreated | NE::SessionChanged => {
                        tracing::debug!(session_id = %msg.session_id, event = ?msg.event, "notify: session event");
                        self.sidebar_dirty = true;
                    }
                }
            }
            Message::TerminalScroll(delta) => {
                if !self.sessions.is_empty() {
                    let session = &mut self.sessions[self.active];
                    let lines = (delta / 3.0).round() as i32;
                    if lines != 0 {
                        session.terminal.scroll(lines);
                        session.terminal.update_snapshot(&self.theme.terminal);
                    }
                }
            }
            Message::WindowResized(size) => {
                let font_size = self.theme.font_size;
                let (cw, ch) = planeai_iced_spike::font::cell_dimensions(font_size);
                let sidebar_w = self.sidebar.as_ref().map_or(0.0, |s| s.width);
                let new_cols = ((size.width - sidebar_w) / cw).floor().max(2.0) as u16;
                let new_rows = ((size.height - 40.0) / ch).floor().max(2.0) as u16;
                if new_cols as usize == self.cols && new_rows as usize == self.rows {
                    return iced::Task::none();
                }
                self.cols = new_cols as usize;
                self.rows = new_rows as usize;
                if self.sessions.is_empty() {
                    return iced::Task::none();
                }
                let session = &mut self.sessions[self.active];
                let term_size = crate::terminal_view::ScrollbackTermSize {
                    cols: self.cols,
                    rows: self.rows,
                    scrollback: 10_000,
                };
                session.terminal.term.resize(term_size);
                let _ = session.backend.resize(new_cols, new_rows);
                session.terminal.update_snapshot(&self.theme.terminal);
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
                    return iced::Task::none();
                }

                // Command palette mode
                if self.command_palette.is_some() {
                    let key_str = match &key {
                        keyboard::Key::Named(keyboard::key::Named::ArrowDown) => Some("ArrowDown"),
                        keyboard::Key::Named(keyboard::key::Named::ArrowUp) => Some("ArrowUp"),
                        keyboard::Key::Named(keyboard::key::Named::Enter) => Some("Enter"),
                        keyboard::Key::Named(keyboard::key::Named::Escape) => Some("Escape"),
                        keyboard::Key::Named(keyboard::key::Named::Backspace) => Some("Backspace"),
                        keyboard::Key::Character(c) => Some(c.as_str()),
                        _ => None,
                    };
                    if let Some(k) = key_str {
                        let event = self.command_palette.as_mut().unwrap().handle_key(k);
                        match event {
                            crate::command_palette::PaletteEvent::Select(id) => {
                                self.command_palette = None;
                                self.dispatch_palette_action(&id);
                            }
                            crate::command_palette::PaletteEvent::Close => {
                                self.command_palette = None;
                            }
                            crate::command_palette::PaletteEvent::None => {}
                        }
                    }
                    return iced::Task::none();
                }

                // Launch prompt mode
                if self.launch_prompt {
                    if matches!(key, keyboard::Key::Named(keyboard::key::Named::Escape)) {
                        self.launch_prompt = false;
                        self.launch_prompt_input.clear();
                    }
                    return iced::Task::none();
                }

                // Project form mode
                if self.project_form.visible {
                    match &key {
                        keyboard::Key::Named(keyboard::key::Named::Escape) => {
                            self.project_form.close();
                        }
                        keyboard::Key::Named(keyboard::key::Named::Tab) => {
                            return if modifiers.shift() {
                                iced::widget::operation::focus_previous()
                            } else {
                                iced::widget::operation::focus_next()
                            };
                        }
                        keyboard::Key::Named(keyboard::key::Named::Enter)
                            if modifiers.command() =>
                        {
                            if let Some(result) = self
                                .project_form
                                .update(project_form::FormMessage::Submit, &self.db)
                            {
                                match result {
                                    project_form::SubmitResult::Created(proj, path) => {
                                        self.project = Some(proj);
                                        self.project_cwd = path.clone();
                                        self.recent_projects =
                                            add_recent_project(&path.to_string_lossy());
                                        self.refresh_persisted_sessions();
                                        self.sidebar_dirty = true;
                                        self.clear_error();
                                    }
                                    project_form::SubmitResult::Error(_) => {}
                                }
                            }
                        }
                        // Let character input pass through to text_input widgets
                        _ => return iced::Task::none(),
                    }
                    return iced::Task::none();
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
                    return iced::Task::none();
                }

                // Tab switcher: Ctrl+Tab / Ctrl+Shift+Tab
                if modifiers.control()
                    && matches!(key, keyboard::Key::Named(keyboard::key::Named::Tab))
                {
                    if !self.tab_switcher.is_cycling() {
                        let valid_ids: std::collections::HashSet<String> =
                            self.sessions.iter().map(|s| s.session_id.clone()).collect();
                        let current = self.sessions.get(self.active).map(|s| s.session_id.clone());
                        let current_ref = current.as_deref();
                        if !self
                            .tab_switcher
                            .start_cycle(&self.mru, current_ref, Some(&valid_ids))
                        {
                            return iced::Task::none();
                        }
                        // Cache names for the overlay
                        self.tab_switcher_names = self
                            .tab_switcher
                            .cycle_list()
                            .iter()
                            .map(|sid| {
                                self.persisted_sessions
                                    .iter()
                                    .find(|s| s.id == *sid)
                                    .map(|r| {
                                        if r.name.is_empty() {
                                            r.branch.clone()
                                        } else {
                                            r.name.clone()
                                        }
                                    })
                                    .or_else(|| {
                                        self.db.as_ref().and_then(|db| {
                                            db.lock().ok().and_then(|conn| {
                                                SessionService::get(&conn, sid).ok().flatten().map(
                                                    |r| {
                                                        if r.name.is_empty() {
                                                            r.branch
                                                        } else {
                                                            r.name
                                                        }
                                                    },
                                                )
                                            })
                                        })
                                    })
                                    .unwrap_or_else(|| sid.clone())
                            })
                            .collect();
                    } else {
                        let direction = if modifiers.shift() { -1 } else { 1 };
                        self.tab_switcher.advance(direction);
                    }
                    return iced::Task::none();
                }

                // Tab switcher: Escape cancels cycling
                if self.tab_switcher.is_cycling()
                    && matches!(key, keyboard::Key::Named(keyboard::key::Named::Escape))
                {
                    if let Some(origin_id) = self.tab_switcher.cancel() {
                        if let Some(idx) =
                            self.sessions.iter().position(|s| s.session_id == origin_id)
                        {
                            self.active = idx;
                        }
                    }
                    return iced::Task::none();
                }

                // Session creation form
                if self.session_form {
                    // When Project field is focused — custom combobox
                    if self.session_form_focus == SessionFormField::Project {
                        // Form-level shortcuts first
                        match &key {
                            keyboard::Key::Named(keyboard::key::Named::Escape) => {
                                self.session_form = false;
                                return iced::Task::none();
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
                                return iced::Task::none();
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
                            return iced::Task::none();
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
                        return iced::Task::none();
                    }
                    // When Task field is focused — custom combobox
                    if self.session_form_focus == SessionFormField::Task {
                        match &key {
                            keyboard::Key::Named(keyboard::key::Named::Escape) => {
                                self.session_form = false;
                                return iced::Task::none();
                            }
                            keyboard::Key::Named(keyboard::key::Named::Tab) => {
                                if modifiers.shift() {
                                    self.session_form_focus = SessionFormField::Project;
                                } else {
                                    self.session_form_focus = SessionFormField::Name;
                                }
                                return iced::Task::none();
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
                            return iced::Task::none();
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
                        return iced::Task::none();
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
                    return iced::Task::none();
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
                    return iced::Task::none();
                }

                // Worktree prompt mode
                if self.worktree_prompt {
                    if matches!(key, keyboard::Key::Named(keyboard::key::Named::Escape)) {
                        self.worktree_prompt = false;
                        self.worktree_error = None;
                    }
                    return iced::Task::none();
                }

                // Project picker mode
                if self.picking_project {
                    if matches!(key, keyboard::Key::Named(keyboard::key::Named::Escape)) {
                        self.picking_project = false;
                        self.project_input.clear();
                        return iced::Task::none();
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
                                        return iced::Task::none();
                                    }
                                }
                            }
                        }
                    }
                    return iced::Task::none();
                }

                // Shortcuts overlay: Escape dismisses
                if self.show_shortcuts {
                    if matches!(key, keyboard::Key::Named(keyboard::key::Named::Escape)) {
                        self.show_shortcuts = false;
                    }
                    return iced::Task::none();
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
                    return iced::Task::none();
                }

                // Cmd+K — command palette
                if cmd
                    && !modifiers.shift()
                    && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "k")
                {
                    if self.command_palette.is_some() {
                        self.command_palette = None;
                    } else {
                        self.close_all_overlays();
                        self.command_palette = Some(self.build_palette_items());
                    }
                    return iced::Task::none();
                }

                // Cmd+N — open "New..." menu
                if cmd
                    && !modifiers.shift()
                    && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "n")
                {
                    self.new_menu = true;
                    self.new_menu_index = 0;
                    self.kill_armed = false;
                    return iced::Task::none();
                }

                // Cmd+Shift+S — toggle sidebar focus
                if cmd
                    && modifiers.shift()
                    && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "s" || c.as_str() == "S")
                {
                    self.sidebar_focused = !self.sidebar_focused;
                    return iced::Task::none();
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
                    return iced::Task::none();
                }

                // Cmd+T — task picker
                if cmd
                    && !modifiers.shift()
                    && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "t")
                {
                    self.open_task_picker();
                    self.kill_armed = false;
                    return iced::Task::none();
                }

                // Cmd+Enter — launch selected task
                if cmd && matches!(key, keyboard::Key::Named(keyboard::key::Named::Enter)) {
                    if self.selected_task.is_some() {
                        self.launch_from_task();
                    }
                    return iced::Task::none();
                }

                // Cmd+Shift+T — clear selected task
                if cmd
                    && modifiers.shift()
                    && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "t" || c.as_str() == "T")
                {
                    self.selected_task = None;
                    return iced::Task::none();
                }

                // Cmd+Shift+N — open project creation form
                if cmd
                    && modifiers.shift()
                    && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "n" || c.as_str() == "N")
                {
                    return self.project_form.open().map(Message::ProjectForm);
                }

                // Cmd+L (macOS) / Ctrl+Shift+L (other) — log replay
                if cmd_safe
                    && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "l" || c.as_str() == "L")
                {
                    self.open_log_replay();
                    return iced::Task::none();
                }

                // Cmd+O — open project picker
                if cmd
                    && !modifiers.shift()
                    && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "o")
                {
                    self.picking_project = !self.picking_project;
                    self.project_input = self.project_cwd.to_string_lossy().to_string();
                    self.recent_projects = load_recent_projects();
                    return iced::Task::none();
                }
                // Cmd+R (macOS) / Ctrl+Shift+R (other) — refresh
                if cmd_safe
                    && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "r" || c.as_str() == "R")
                {
                    self.refresh_daemon_list();
                    return iced::Task::none();
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
                    return iced::Task::none();
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
                    return iced::Task::none();
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
                    return iced::Task::none();
                }
                // Cmd+1..9 — switch sessions
                if cmd && !modifiers.shift() {
                    if let keyboard::Key::Character(c) = &key {
                        if let Ok(digit) = c.as_str().parse::<usize>() {
                            if (1..=9).contains(&digit) {
                                self.switch_to(digit - 1);
                                return iced::Task::none();
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
                                    let sid = &self.sessions[self.active].session_id;
                                    self.notify_state.lock().unwrap().acknowledge(sid);
                                    self.agent_states.remove(sid);
                                }
                            }
                        }
                    }
                    return iced::Task::none();
                }
                // Sidebar: Escape from terminal focuses sidebar
                if !self.sidebar_focused
                    && matches!(key, keyboard::Key::Named(keyboard::key::Named::Escape))
                {
                    self.sidebar_focused = true;
                    return iced::Task::none();
                }
                // When sidebar is focused, route keys there
                if self.sidebar_focused {
                    self.handle_sidebar_key(&key);
                    if let Some(ref sidebar) = self.sidebar {
                        return sidebar.scroll_to_selected();
                    }
                    return iced::Task::none();
                }
                // Forward input to active session
                if !self.sessions.is_empty() {
                    self.kill_armed = false;
                    let bytes = input::encode_key_event(&key, &modifiers, &txt);
                    if let Some(ref b) = bytes {
                        if !b.is_empty() {
                            let _ = self.sessions[self.active].backend.write(b);
                            // Clear agent state on user input (acknowledge)
                            let sid = &self.sessions[self.active].session_id;
                            self.notify_state.lock().unwrap().acknowledge(sid);
                            self.agent_states.remove(sid);
                        }
                    }
                }
            }
            Message::KeyEvent(keyboard::Event::KeyReleased {
                key: keyboard::Key::Named(keyboard::key::Named::Control),
                ..
            }) => {
                if self.tab_switcher.is_cycling() {
                    if let Some(target_id) = self.tab_switcher.commit() {
                        if let Some(idx) =
                            self.sessions.iter().position(|s| s.session_id == target_id)
                        {
                            self.switch_to(idx);
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

                // Poll system dark/light mode
                if let Some(new_theme) = self.theme_source.poll_mode() {
                    self.theme = new_theme;
                    for s in &mut self.sessions {
                        s.terminal.update_snapshot(&self.theme.terminal);
                    }
                }

                self.check_daemon_health();

                // Sidebar: refresh only when data changed (dirty flag)
                if self.sidebar_dirty {
                    self.sidebar_dirty = false;
                    if let Some(ref db) = self.db {
                        if let Ok(conn) = db.lock() {
                            let db_path = planeai_core::app_data_dir().join("planeai.db");
                            if let Some(ref mut sidebar) = self.sidebar {
                                sidebar.refresh(&conn);
                            } else {
                                self.sidebar = Some(SidebarState::new(&conn, &db_path));
                            }
                        }
                    }
                    if let Some(ref mut sidebar) = self.sidebar {
                        sidebar.set_active_session(
                            self.sessions.get(self.active).map(|s| s.session_id.clone()),
                        );
                    }
                }

                // Drain output from all sessions
                for i in 0..self.sessions.len() {
                    let mut got_data = false;
                    loop {
                        let output = self.sessions[i].backend.try_read_batch().unwrap_or(None);
                        match output {
                            Some(data) => {
                                got_data = true;
                                self.sessions[i].bytes_processed += data.len() as u64;
                                let session = &mut self.sessions[i];
                                session
                                    .terminal
                                    .processor
                                    .advance(&mut session.terminal.term, &data);
                            }
                            None => break,
                        }
                    }
                    if got_data {
                        // Notify state machine of output activity
                        let sid = &self.sessions[i].session_id;
                        let was_idle = {
                            let mut ns = self.notify_state.lock().unwrap();
                            let was = ns.get_state(sid) != Some(planeai_core::notify::AgentState::Busy);
                            ns.notify_output(sid);
                            was
                        };
                        if was_idle {
                            self.agent_states.insert(sid.clone(), planeai_core::notify::AgentState::Busy);
                        }
                    }
                    if i == self.active && got_data {
                        let session = &mut self.sessions[i];
                        if !session.terminal.is_scrolled() {
                            session.terminal.scroll_to_bottom();
                        }
                        session.terminal.update_snapshot(&self.theme.terminal);
                    }
                }

                // Update session statuses
                for s in &mut self.sessions {
                    if (s.status == SessionStatus::Running || s.status == SessionStatus::Attached)
                        && s.backend.has_exited()
                    {
                        s.status = SessionStatus::Exited;
                        self.sidebar_dirty = true;
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
        iced::Task::none()
    }
}

impl WorkflowApp {
    fn view(&self) -> Element<'_, Message> {
        let mut left_panel_content = column![].spacing(2).width(Length::Fixed(180.0));

        // Daemon status
        let (indicator, color) = if self.daemon_connected {
            ("⚡ daemon connected", self.theme.accent())
        } else {
            ("⚠ daemon disconnected", self.theme.warning())
        };
        left_panel_content = left_panel_content.push(text(indicator).color(color));
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
                (_, true) => self.theme.accent(),
                (SessionStatus::Exited, _) | (SessionStatus::Killed, _) => self.theme.text_dimmed(),
                (SessionStatus::Detached, _) | (SessionStatus::Unreachable, _) => {
                    self.theme.warning()
                }
                _ => self.theme.text_primary(),
            };
            left_panel_content = left_panel_content.push(text(label).color(color));
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
                left_panel_content =
                    left_panel_content.push(text("── detached ──").color(self.theme.text_dimmed()));
                for info in unattached.iter().take(5) {
                    let label = format!(
                        "  {} {}",
                        if info.alive { "◌" } else { "✕" },
                        &info.session_id[..info.session_id.len().min(14)]
                    );
                    let color = if info.alive {
                        self.theme.warning()
                    } else {
                        self.theme.text_dimmed()
                    };
                    left_panel_content = left_panel_content.push(text(label).color(color));
                }
            }
        }

        let left_panel: Element<'_, Message> = if let Some(ref sidebar) = self.sidebar {
            sidebar.view(
                self.sidebar_focused,
                &self.theme,
                &self.agent_states,
                Message::SidebarItemClicked,
                Message::SidebarScrolled,
            )
        } else {
            container(left_panel_content)
                .padding(8)
                .style(|_: &Theme| container::Style {
                    background: Some(self.theme.panel_bg().into()),
                    ..Default::default()
                })
                .into()
        };

        // Terminal area (or log replay)
        let terminal_area: Element<'_, Message> = if let Some(ref replay) = self.log_replay {
            // Log replay view
            let banner = container(
                text("READ-ONLY LOG REPLAY — Escape to exit").color(self.theme.warning_text()),
            )
            .width(Length::Fill)
            .padding(2)
            .style(|_: &Theme| container::Style {
                background: Some(self.theme.warning_bg().into()),
                ..Default::default()
            });
            let canvas_view = Canvas::new(TerminalRenderer {
                snapshot: &replay.terminal.snapshot,
                cache: &replay.terminal.cache,
                background: self.theme.terminal.background,
                cursor_color: self.theme.terminal.cursor,
                font: self.theme.font,
                font_size: self.theme.font_size,
            })
            .width(Length::Fill)
            .height(Length::Fill);
            column![banner, canvas_view].into()
        } else if self.sessions.is_empty() {
            container(
                text("No sessions. Cmd+N to launch, Cmd+A to attach.")
                    .color(self.theme.text_dimmed()),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
        } else {
            let session = &self.sessions[self.active];
            Canvas::new(TerminalRenderer {
                snapshot: &session.terminal.snapshot,
                cache: &session.terminal.cache,
                background: self.theme.terminal.background,
                cursor_color: self.theme.terminal.cursor,
                font: self.theme.font,
                font_size: self.theme.font_size,
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
        let status_bar = container(text(status_text).color(self.theme.text_primary()))
            .width(Length::Fill)
            .padding(2)
            .style(|_: &Theme| container::Style {
                background: Some(self.theme.panel_bg().into()),
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
                .width(Length::Fill),
            );
            // Show recent projects
            if !self.recent_projects.is_empty() {
                picker_col = picker_col
                    .push(text("Recent (Cmd+1..9 to select):").color(self.theme.text_muted()));
                for (i, p) in self.recent_projects.iter().take(9).enumerate() {
                    let exists = PathBuf::from(p).is_dir();
                    let marker = if !exists { " (missing)" } else { "" };
                    let label = format!(" {}. {}{}", i + 1, p, marker);
                    let color = if exists {
                        self.theme.text_primary()
                    } else {
                        self.theme.text_dimmed()
                    };
                    picker_col = picker_col.push(text(label).color(color));
                }
            }
            let picker = container(picker_col).style(|_: &Theme| container::Style {
                background: Some(self.theme.panel_bg().into()),
                ..Default::default()
            });
            column![picker, main_content].into()
        } else if self.launch_prompt {
            let prompt = container(
                text_input("Command to launch...", &self.launch_prompt_input)
                    .on_input(Message::LaunchPromptChanged)
                    .on_submit(Message::LaunchPromptSubmit)
                    .width(Length::Fill),
            )
            .width(Length::Fill)
            .padding(4)
            .style(|_: &Theme| container::Style {
                background: Some(self.theme.panel_bg().into()),
                ..Default::default()
            });
            column![prompt, main_content].into()
        } else if self.project_form.visible {
            let form_col = self.project_form.view(
                &self.theme,
                |s| Message::ProjectForm(project_form::FormMessage::PathChanged(s)),
                |s| Message::ProjectForm(project_form::FormMessage::NameChanged(s)),
                Message::ProjectForm(project_form::FormMessage::Submit),
                Message::ProjectForm(project_form::FormMessage::Cancel),
            );
            modal_overlay(form_col, main_content.into(), &self.theme)
        } else if self.worktree_prompt {
            let mut wt_col = column![].spacing(4).width(Length::Fill).padding(6);
            let project_name = self
                .project_cwd
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "project".to_string());
            wt_col = wt_col.push(
                text(format!("Worktree Launch — project: {}", project_name))
                    .color(self.theme.accent()),
            );
            let mode_label = if self.worktree_use_worktree {
                "● Worktree mode (Cmd+B to toggle)"
            } else {
                "○ Direct cwd mode (Cmd+B to toggle)"
            };
            wt_col = wt_col.push(text(mode_label).color(self.theme.text_secondary()));
            if self.worktree_use_worktree {
                wt_col = wt_col.push(
                    text_input(
                        "Branch name (e.g. feat/my-feature)...",
                        &self.worktree_branch_input,
                    )
                    .on_input(Message::WorktreeBranchChanged)
                    .on_submit(Message::WorktreeLaunchSubmit)
                    .width(Length::Fill),
                );
                wt_col = wt_col.push(
                    text_input(
                        "Task key (optional, e.g. PLA-42)...",
                        &self.worktree_task_key_input,
                    )
                    .on_input(Message::WorktreeTaskKeyChanged)
                    .on_submit(Message::WorktreeLaunchSubmit)
                    .width(Length::Fill),
                );
                if let Some(ref path) = self.worktree_computed_path {
                    wt_col =
                        wt_col.push(text(format!("→ {}", path)).color(self.theme.text_muted()));
                }
                if let Some(ref err) = self.worktree_error {
                    wt_col = wt_col.push(text(format!("⚠ {}", err)).color(self.theme.error()));
                }
            }
            wt_col = wt_col
                .push(text("Enter to launch | Escape to cancel").color(self.theme.text_dimmed()));
            let wt_panel = container(wt_col).style(|_: &Theme| container::Style {
                background: Some(self.theme.panel_bg().into()),
                ..Default::default()
            });
            column![wt_panel, main_content].into()
        } else if self.new_menu {
            let items = ["Session", "Task"];
            let mut nm_col = column![].spacing(2).width(Length::Fill).padding(6);
            nm_col = nm_col.push(
                text("New... (↑↓ navigate, Enter select, Escape cancel)")
                    .color(self.theme.text_secondary()),
            );
            for (i, item) in items.iter().enumerate() {
                let marker = if i == self.new_menu_index { "▶" } else { " " };
                let color = if i == self.new_menu_index {
                    self.theme.accent()
                } else {
                    self.theme.text_muted()
                };
                nm_col = nm_col.push(text(format!("{} {}", marker, item)).color(color));
            }
            modal_overlay(nm_col, main_content.into(), &self.theme)
        } else if self.session_form {
            let mut sf_col = column![].spacing(3).width(Length::Fill).padding(8);
            sf_col = sf_col.push(text("New Session").color(Color::from_rgb8(255, 255, 255)));

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
                .color(if mode_highlight {
                    Color::from_rgb8(200, 220, 255)
                } else {
                    Color::from_rgb8(160, 160, 160)
                }),
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
                .color(if name_highlight {
                    Color::from_rgb8(100, 220, 255)
                } else {
                    Color::from_rgb8(160, 160, 160)
                }),
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
                .color(if toggles_highlight {
                    Color::from_rgb8(100, 220, 255)
                } else {
                    Color::from_rgb8(160, 160, 160)
                }),
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
                .color(if branch_highlight {
                    Color::from_rgb8(100, 220, 255)
                } else {
                    Color::from_rgb8(160, 160, 160)
                }),
            );

            // Error
            if let Some(ref err) = self.session_form_error {
                sf_col = sf_col
                    .push(text(format!("  ⚠ {}", err)).color(Color::from_rgb8(255, 100, 100)));
            }

            // Footer
            sf_col = sf_col.push(
                text("  Tab=next field | Cmd+Enter=create | Escape=cancel")
                    .color(Color::from_rgb8(80, 80, 80)),
            );

            modal_overlay(sf_col, main_content.into(), &self.theme)
        } else if self.task_picker {
            let mut tp_col = column![].spacing(2).width(Length::Fill).padding(6);
            tp_col = tp_col.push(
                text("Task Picker (↑↓ navigate, Enter select, Escape cancel)")
                    .color(self.theme.accent()),
            );
            if self.task_list.is_empty() {
                tp_col = tp_col.push(
                    text("  No tasks found for this project.").color(self.theme.text_muted()),
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
                        self.theme.accent()
                    } else {
                        self.theme.text_primary()
                    };
                    tp_col = tp_col.push(text(label).color(color));
                }
            }
            let tp_panel = container(tp_col).style(|_: &Theme| container::Style {
                background: Some(self.theme.chrome_bg().into()),
                ..Default::default()
            });
            column![tp_panel, main_content].into()
        } else {
            main_content.into()
        };

        // Title bar (full width, above everything)
        // Same logic as Tauri: project name from DB via session.project_id, session name/branch
        let active_record = if !self.sessions.is_empty() {
            let sid = &self.sessions[self.active].session_id;
            self.persisted_sessions
                .iter()
                .find(|r| r.id == *sid)
                .cloned()
                .or_else(|| {
                    self.db.as_ref().and_then(|db| {
                        let conn = db.lock().unwrap();
                        services::SessionService::get(&conn, sid).ok().flatten()
                    })
                })
        } else {
            None
        };
        let project_name_owned = active_record.as_ref().and_then(|r| {
            self.db.as_ref().and_then(|db| {
                let conn = db.lock().unwrap();
                services::ProjectService::get_by_id(&conn, &r.project_id)
                    .ok()
                    .flatten()
                    .map(|p| p.name)
            })
        });
        let project_name = project_name_owned.as_deref();
        let session_name_owned = active_record.as_ref().map(|r| {
            if r.name.is_empty() {
                r.branch.clone()
            } else {
                r.name.clone()
            }
        });
        let session_name = session_name_owned.as_deref();
        let title_bar = crate::titlebar::view(
            project_name,
            session_name,
            &self.theme,
            Message::TitleBarDrag,
        );
        let base: Element<'_, Message> = column![title_bar, base].into();

        // Hook install banner
        let base: Element<'_, Message> = if self.show_hook_banner {
            let banner = container(
                row![
                    text("⚠ Notification hooks not installed — agents won't signal when ready.")
                        .size(12)
                        .color(self.theme.warning_text()),
                    iced::widget::button(text("Install").size(12))
                        .on_press(Message::InstallHooks)
                        .padding([2, 8]),
                    iced::widget::button(text("✕").size(12))
                        .on_press(Message::DismissHookBanner)
                        .padding([2, 4]),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            )
            .width(Length::Fill)
            .padding([4, 12])
            .style(move |_: &Theme| container::Style {
                background: Some(self.theme.warning_bg().into()),
                ..Default::default()
            });
            column![banner, base].into()
        } else {
            base
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
        } else if self.command_palette.is_some() {
            use iced::widget::stack;
            let palette = self.command_palette.as_ref().unwrap();
            let mut items_col = column![].spacing(1);

            // Search input display
            let search_display = if palette.search().is_empty() {
                "Type to search...".to_string()
            } else {
                format!("{}▏", palette.search())
            };
            let search_color = if palette.search().is_empty() {
                self.theme.text_dimmed()
            } else {
                self.theme.text_primary()
            };
            items_col = items_col.push(
                container(text(search_display).color(search_color))
                    .width(Length::Fill)
                    .padding([6, 8]),
            );

            // Items grouped by section
            let visible = palette.visible_items();
            let cursor_pos = palette.cursor();
            let mut current_group = "";
            for (vi, item) in visible.iter().enumerate() {
                if item.group != current_group {
                    current_group = &item.group;
                    items_col = items_col.push(
                        container(text(current_group).size(11).color(self.theme.text_dimmed()))
                            .padding([4, 8]),
                    );
                }
                // Determine absolute index for cursor highlight
                let is_cursor = vi == cursor_pos % visible.len().max(1);
                let label_color = if item.is_active {
                    self.theme.accent()
                } else if is_cursor {
                    self.theme.text_primary()
                } else {
                    self.theme.text_secondary()
                };
                let item_bg = if is_cursor {
                    Some(Color {
                        a: 0.12,
                        ..self.theme.accent()
                    })
                } else {
                    None
                };
                let active_mark = if item.is_active { " •" } else { "" };
                let txt = text(format!("{}{}", item.label, active_mark)).color(label_color);
                items_col =
                    items_col.push(container(txt).width(Length::Fill).padding([3, 8]).style(
                        move |_: &Theme| container::Style {
                            background: item_bg.map(|c| c.into()),
                            ..Default::default()
                        },
                    ));
            }

            let panel_bg = self.theme.panel_bg();
            let border_color = self.theme.border();
            let panel = container(items_col)
                .padding(4)
                .width(Length::Fixed(420.0))
                .max_height(400.0)
                .style(move |_: &Theme| container::Style {
                    background: Some(panel_bg.into()),
                    border: iced::Border {
                        color: border_color,
                        width: 1.0,
                        radius: 8.0.into(),
                    },
                    ..Default::default()
                });
            let overlay = container(panel)
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .padding(iced::Padding {
                    top: 80.0,
                    right: 0.0,
                    bottom: 0.0,
                    left: 0.0,
                });
            stack![base, overlay].into()
        } else if self.tab_switcher.is_visible() {
            use iced::widget::stack;
            let mut items = column![].spacing(2);
            let selected = self.tab_switcher.index();
            for (i, name) in self.tab_switcher_names.iter().enumerate() {
                let is_selected = i == selected;
                let label = format!("  {}", name);
                let color = if is_selected {
                    self.theme.accent()
                } else {
                    self.theme.text_primary()
                };
                let item_bg = if is_selected {
                    Some(Color {
                        a: 0.15,
                        ..self.theme.accent()
                    })
                } else {
                    None
                };
                let txt = text(label).color(color);
                items = items.push(container(txt).width(Length::Fill).padding([2, 4]).style(
                    move |_: &Theme| container::Style {
                        background: item_bg.map(|c| c.into()),
                        ..Default::default()
                    },
                ));
            }
            let panel_bg = self.theme.panel_bg();
            let border_color = self.theme.border();
            let panel = container(items)
                .padding(8)
                .width(Length::Fixed(700.0))
                .style(move |_: &Theme| container::Style {
                    background: Some(panel_bg.into()),
                    border: iced::Border {
                        color: border_color,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                });
            let overlay = container(panel)
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill);
            stack![base, overlay].into()
        } else {
            base
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch(vec![
            keyboard::listen().map(Message::KeyEvent),
            iced::time::every(Duration::from_millis(16)).map(|_| Message::Poll),
            // Silence/debounce checker — tick every 1s
            iced::time::every(Duration::from_secs(1)).map(|_| Message::CheckSilence),
            // IPC notify listener
            Subscription::run(notify_ipc_stream).map(Message::NotifyIpcMessage),
            event::listen_with(|ev, _status, _id| match ev {
                iced::Event::Window(window::Event::Resized(size)) => {
                    Some(Message::WindowResized(size))
                }
                iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
                    iced::mouse::Button::Left,
                )) => Some(Message::SidebarMouseDown(0.0)),
                iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                    iced::mouse::Button::Left,
                )) => Some(Message::SidebarDragEnd),
                iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                    Some(Message::SidebarDrag(position.x))
                }
                iced::Event::Mouse(iced::mouse::Event::WheelScrolled { delta }) => {
                    let y = match delta {
                        iced::mouse::ScrollDelta::Lines { y, .. } => y,
                        iced::mouse::ScrollDelta::Pixels { y, .. } => y / 20.0,
                    };
                    Some(Message::TerminalScroll(y))
                }
                _ => None,
            }),
        ])
    }
}

// ─── Canvas renderer (moved to terminal_view.rs) ─────────────────────────────

// ─── Static args ─────────────────────────────────────────────────────────────

use std::sync::OnceLock;

/// Returns an async stream that yields NotifyMessages from the IPC socket.
/// Spawns a background thread on first call; subsequent calls share the same receiver.
fn notify_ipc_stream() -> impl iced::futures::Stream<Item = planeai_core::notify::NotifyMessage> {
    use tokio::sync::mpsc;
    static TX: OnceLock<mpsc::UnboundedSender<planeai_core::notify::NotifyMessage>> =
        OnceLock::new();
    static RX: OnceLock<Mutex<Option<mpsc::UnboundedReceiver<planeai_core::notify::NotifyMessage>>>> =
        OnceLock::new();

    // Initialize once: spawn thread + create channel
    let _ = TX.get_or_init(|| {
        let (tx, rx) = mpsc::unbounded_channel();
        RX.get_or_init(|| Mutex::new(Some(rx)));
        let tx2 = tx.clone();
        std::thread::spawn(move || {
            use std::io::{BufRead, BufReader};
            let app_dir = planeai_core::app_data_dir();
            let Ok(listener) =
                planeai_ipc::IpcListener::bind(planeai_ipc::Channel::Notify, &app_dir)
            else {
                tracing::warn!("notify: failed to bind IPC listener");
                return;
            };
            tracing::info!("notify: IPC listener started");
            loop {
                let stream = match listener.accept() {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let tx = tx2.clone();
                std::thread::spawn(move || {
                    let reader = BufReader::new(stream);
                    for line in reader.lines().map_while(Result::ok) {
                        let line = line.trim().to_string();
                        if line.is_empty() {
                            continue;
                        }
                        let msg = planeai_core::notify::parse_notify_message(&line);
                        if msg.session_id.is_empty() {
                            continue;
                        }
                        let _ = tx.send(msg);
                        }
                });
            }
        });
        tx
    });

    // Take the receiver (only first subscription gets it)
    let rx = RX
        .get()
        .and_then(|m| m.lock().unwrap().take())
        .expect("notify IPC receiver already taken");

    tokio_stream::wrappers::UnboundedReceiverStream::new(rx)
}
static WORKFLOW_ARGS: OnceLock<Args> = OnceLock::new();

/// Wraps content in a centered modal overlay panel with standard styling.
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

    // Load terminal font (theme config provides family/size, font module loads bytes)
    let theme_source = theme::ThemeSource::load();
    let font_family = args
        .font_family
        .as_deref()
        .unwrap_or(&theme_source.font_family);
    planeai_iced_spike::font::load(font_family, theme_source.font_size);

    WORKFLOW_ARGS.set(args).unwrap();
    let window_settings = window::Settings {
        size: Size::new(1200.0, 800.0),
        min_size: Some(Size::new(640.0, 480.0)),
        decorations: true,
        transparent: true,
        platform_specific: window::settings::PlatformSpecific {
            title_hidden: true,
            titlebar_transparent: true,
            fullsize_content_view: true,
        },
        ..Default::default()
    };
    let app = iced::application(WorkflowApp::boot, WorkflowApp::update, WorkflowApp::view)
        .title(title)
        .theme(|state: &WorkflowApp| state.theme.to_iced_theme())
        .subscription(WorkflowApp::subscription)
        .settings(iced::Settings {
            default_text_size: iced::Pixels(14.0),
            ..iced::Settings::default()
        })
        .window(window_settings);
    app.run()
}
