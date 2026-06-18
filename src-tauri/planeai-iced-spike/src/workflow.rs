//! PlaneAI Workflow Shell — orchestrates daemon sessions with project context.

use std::path::PathBuf;
use std::time::{Duration, Instant};

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

// ─── Session state ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
enum SessionStatus {
    Running,
    Attached,
    Exited,
    Detached,
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
    // Status/error
    last_error: Option<String>,
    error_time: Option<Instant>,
    // Shortcuts overlay
    show_shortcuts: bool,
    // Session counter for unique ids
    next_id: usize,
}

#[derive(Debug, Clone)]
enum Message {
    Poll,
    KeyEvent(keyboard::Event),
    WindowResized(Size),
    ProjectInputChanged(String),
    ProjectInputSubmit,
}

impl WorkflowApp {
    fn boot() -> (Self, iced::Task<Message>) {
        // Args accessed via std::env since we can't use OnceLock for workflow
        // The run() function passes args via a thread-local or we reconstruct.
        // Actually we use a static — set by run() before boot.
        let args = WORKFLOW_ARGS.get().unwrap();

        // Load config: CLI/env > config file > defaults
        let config = if let Some(ref path) = args.config {
            planeai_core::session_launch::load_launch_config(path).unwrap_or_default()
        } else {
            planeai_core::session_launch::load_default_config()
        };

        let project_cwd = args
            .cwd
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        // Agent command: CLI flag > config default provider command > fallback
        let agent_command = if let Some(ref cmd) = args.agent_command {
            cmd.clone()
        } else if let Some(provider) = config.providers.get(&config.default_provider) {
            provider.command.clone()
        } else {
            "kiro-cli chat".to_string()
        };

        // Extra PATH dirs: CLI augments config
        let mut extra_path_dirs = config.extra_path_dirs.clone();
        extra_path_dirs.extend(args.extra_path_dirs.iter().cloned());

        let cols = args.cols;
        let rows = args.rows;

        // Ensure daemon is running
        let daemon_connected = match ensure_daemon_running_sync() {
            Ok(()) => daemon_is_connected(),
            Err(_) => false,
        };

        // List existing daemon sessions
        let daemon_sessions_listed = if daemon_connected {
            list_daemon_sessions().unwrap_or_default()
        } else {
            Vec::new()
        };

        // Check log dir
        let _log_dir = std::env::var("PLANEAI_SESSION_LOG_DIR").ok();

        (
            Self {
                sessions: Vec::new(),
                active: 0,
                project_cwd,
                agent_command,
                provider_label: config.default_provider.clone(),
                extra_path_dirs,
                cols,
                rows,
                daemon_connected,
                daemon_sessions_listed,
                last_health_check: Some(Instant::now()),
                picking_project: false,
                project_input: String::new(),
                last_error: None,
                error_time: None,
                show_shortcuts: false,
                next_id: 0,
            },
            iced::Task::none(),
        )
    }

    fn launch_session(&mut self) {
        let id = self.next_id;
        self.next_id += 1;
        let result = DaemonSession::spawn_with_cwd(
            id,
            self.cols as u16,
            self.rows as u16,
            Some(&self.agent_command),
            &self.project_cwd,
            &self.extra_path_dirs,
        );
        match result {
            Ok(backend) => {
                let session_id = backend.session_id().to_string();
                let term = new_term(self.cols, self.rows);
                let processor = new_processor();
                let snapshot = snapshot_grid(&term);
                let log_file_exists = self.check_log_exists(&session_id);
                self.sessions.push(Session {
                    id,
                    session_id,
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
                self.clear_error();
            }
            Err(e) => {
                self.set_error(format!("Launch failed: {}", e));
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
        self.sessions.remove(self.active);
        if !self.sessions.is_empty() && self.active >= self.sessions.len() {
            self.active = self.sessions.len() - 1;
        }
    }

    fn kill_active(&mut self) {
        if self.sessions.is_empty() {
            return;
        }
        let session = &self.sessions[self.active];
        let _ = kill_daemon_session(&session.session_id);
        self.sessions.remove(self.active);
        if !self.sessions.is_empty() && self.active >= self.sessions.len() {
            self.active = self.sessions.len() - 1;
        }
    }

    fn refresh_daemon_list(&mut self) {
        self.daemon_connected = daemon_is_connected();
        if self.daemon_connected {
            self.daemon_sessions_listed = list_daemon_sessions().unwrap_or_default();
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
        self.daemon_connected = daemon_is_connected();
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

    fn update(&mut self, message: Message) {
        match message {
            Message::ProjectInputChanged(val) => {
                self.project_input = val;
            }
            Message::ProjectInputSubmit => {
                let path = PathBuf::from(&self.project_input);
                if path.is_dir() {
                    self.project_cwd = path;
                    self.picking_project = false;
                    self.project_input.clear();
                    self.clear_error();
                } else {
                    self.set_error(format!("Not a directory: {}", self.project_input));
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
                // If picking project, Enter submits, Escape cancels
                if self.picking_project {
                    if matches!(key, keyboard::Key::Named(keyboard::key::Named::Escape)) {
                        self.picking_project = false;
                        self.project_input.clear();
                    }
                    // Text input handles Enter via on_submit
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

                // Cmd+/ — toggle shortcuts overlay
                if cmd && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "/") {
                    self.show_shortcuts = !self.show_shortcuts;
                    return;
                }

                // Cmd+N — launch new session
                if cmd
                    && !modifiers.shift()
                    && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "n")
                {
                    self.launch_session();
                    return;
                }
                // Cmd+O — open project picker
                if cmd
                    && !modifiers.shift()
                    && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "o")
                {
                    self.picking_project = !self.picking_project;
                    self.project_input = self.project_cwd.to_string_lossy().to_string();
                    return;
                }
                // Cmd+R — refresh daemon sessions
                if cmd
                    && !modifiers.shift()
                    && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "r")
                {
                    self.refresh_daemon_list();
                    return;
                }
                // Cmd+W — detach active session
                if cmd
                    && !modifiers.shift()
                    && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "w")
                {
                    self.detach_active();
                    return;
                }
                // Cmd+Shift+W — kill active session
                if cmd
                    && modifiers.shift()
                    && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "w" || c.as_str() == "W")
                {
                    self.kill_active();
                    return;
                }
                // Cmd+A — attach first unattached
                if cmd
                    && !modifiers.shift()
                    && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "a")
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
                    // Refresh log availability
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

        // Session cards
        for (i, s) in self.sessions.iter().enumerate() {
            let status_icon = match s.status {
                SessionStatus::Running => "●",
                SessionStatus::Attached => "◉",
                SessionStatus::Exited => "○",
                SessionStatus::Detached => "◌",
            };
            let cwd_name = s
                .cwd
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| s.cwd.to_string_lossy().to_string());
            let log_indicator = if s.log_file_exists { "📄" } else { "" };
            let dropped = s.backend.bytes_dropped();
            let drop_indicator = if dropped > 0 {
                format!(" ⚠{dropped}d")
            } else {
                String::new()
            };
            let cmd_short = s.command.split_whitespace().next().unwrap_or("?");
            let label = format!(
                "{}{} {} {} {}B{}{}",
                if i == self.active { "▶" } else { " " },
                status_icon,
                cmd_short,
                cwd_name,
                s.bytes_processed,
                drop_indicator,
                log_indicator,
            );
            let color = match (&s.status, i == self.active) {
                (_, true) => Color::from_rgb8(100, 200, 255),
                (SessionStatus::Exited, _) => Color::from_rgb8(120, 120, 120),
                (SessionStatus::Detached, _) => Color::from_rgb8(200, 150, 50),
                _ => Color::from_rgb8(180, 180, 180),
            };
            left_panel_content =
                left_panel_content.push(text(label).size(11).color(color).font(Font::MONOSPACE));
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

        // Terminal area
        let terminal_area: Element<'_, Message> = if self.sessions.is_empty() {
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

        // Status bar
        let project_display = self.project_cwd.to_string_lossy();
        let active_info = if !self.sessions.is_empty() {
            let s = &self.sessions[self.active];
            format!(" | {} | {}B", s.command, s.bytes_processed)
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
            let picker = container(
                text_input("Enter project path...", &self.project_input)
                    .on_input(Message::ProjectInputChanged)
                    .on_submit(Message::ProjectInputSubmit)
                    .size(14)
                    .width(Length::Fill),
            )
            .width(Length::Fill)
            .padding(4)
            .style(|_: &Theme| container::Style {
                background: Some(Color::from_rgb8(50, 50, 70).into()),
                ..Default::default()
            });
            column![picker, main_content].into()
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
