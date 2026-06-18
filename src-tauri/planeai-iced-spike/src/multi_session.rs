use std::fs;
use std::io::Write as IoWrite;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use alacritty_terminal::vte::ansi::Processor;
use arboard::Clipboard;
use iced::keyboard;
use iced::widget::canvas::{self, Cache, Program};
use iced::widget::{column, container, row, text, Canvas};
use iced::{event, window, Color, Element, Font, Length, Point, Rectangle, Renderer, Size, Subscription, Theme};
use iced::mouse;
use serde_json::json;

use crate::adapter::PlaneAiTerminalSession;
use crate::common::*;
use crate::input;
use crate::shell::{QueuePolicy, Shell};
use crate::Args;

static MULTI_ARGS: OnceLock<Args> = OnceLock::new();

struct Session {
    id: usize,
    name: String,
    backend: Box<dyn PlaneAiTerminalSession>,
    term: alacritty_terminal::Term<EventProxy>,
    processor: Processor,
    snapshot: GridSnapshot,
    cache: Cache,
    bytes_processed: u64,
    parse_times: Vec<f64>,
    render_works: Vec<f64>,
    dirty: bool,
}

struct MultiApp {
    sessions: Vec<Session>,
    active: usize,
    boot_instant: Instant,
    args: Args,
    metrics_lines: Vec<serde_json::Value>,
    switch_latencies: Vec<f64>,
    active_parse_times: Vec<f64>,
    active_render_works: Vec<f64>,
    inactive_parse_times: Vec<f64>,
    frame_deltas: Vec<f64>,
    last_frame_instant: Option<Instant>,
    frames: u64,
    done: bool,
    last_frame_ms: f64,
    cols: usize,
    rows: usize,
    // UI drain metrics
    ui_poll_count: u64,
    ui_batches_drained_total: u64,
    ui_bytes_drained_total: u64,
}

#[derive(Debug, Clone)]
enum Message {
    Poll,
    KeyEvent(keyboard::Event),
    WindowResized(Size),
}

impl MultiApp {
    fn boot() -> (Self, iced::Task<Message>) {
        let args = MULTI_ARGS.get().unwrap().clone();
        let cols = args.cols;
        let rows = args.rows;
        let mut sessions = Vec::with_capacity(args.sessions);
        let mut metrics_lines = Vec::new();
        let boot = Instant::now();

        for i in 0..args.sessions {
            let backend: Box<dyn PlaneAiTerminalSession> = match args.session_source.as_str() {
                "spike-local" => {
                    let policy = QueuePolicy::from_str(&args.output_queue_policy);
                    Box::new(Shell::spawn_with_policy(
                        i, cols as u16, rows as u16,
                        args.session_command.as_deref(),
                        policy,
                    ))
                }
                "planeai-local" => {
                    Box::new(crate::planeai_local::PlaneAiLocalSession::spawn(
                        i, cols as u16, rows as u16,
                        args.session_command.as_deref(),
                    ).unwrap_or_else(|e| panic!("Failed to spawn planeai-local session {}: {}", i, e)))
                }
                "planeai-daemon" => {
                    Box::new(crate::daemon_session::DaemonSession::spawn(
                        i, cols as u16, rows as u16,
                        args.session_command.as_deref(),
                    ).unwrap_or_else(|e| panic!("Failed to spawn planeai-daemon session {}: {}", i, e)))
                }
                other => {
                    eprintln!("Error: unsupported --session-source '{}'. Supported: spike-local, planeai-local, planeai-daemon", other);
                    std::process::exit(1);
                }
            };
            let term = new_term(cols, rows);
            let processor = new_processor();
            let snapshot = snapshot_grid(&term);
            let name = format!("Session {}", i + 1);

            metrics_lines.push(json!({
                "schema_version": 1,
                "event_type": "session_created",
                "backend": args.backend,
                "session_source": args.session_source,
                "timestamp_ms": boot.elapsed().as_secs_f64() * 1000.0,
                "session_id": i,
                "session_name": &name,
                "command": args.session_command.as_deref().unwrap_or("<shell>"),
            }));

            sessions.push(Session {
                id: i, name, backend, term, processor, snapshot,
                cache: Cache::new(), bytes_processed: 0, parse_times: Vec::new(),
                render_works: Vec::new(), dirty: false,
            });
        }

        (Self {
            sessions, active: 0, boot_instant: boot, args,
            metrics_lines, switch_latencies: Vec::new(),
            active_parse_times: Vec::new(), active_render_works: Vec::new(),
            inactive_parse_times: Vec::new(), frame_deltas: Vec::new(),
            last_frame_instant: None, frames: 0, done: false,
            last_frame_ms: 0.0, cols, rows,
            ui_poll_count: 0, ui_batches_drained_total: 0, ui_bytes_drained_total: 0,
        }, iced::Task::none())
    }

    fn switch_to(&mut self, idx: usize) {
        if idx >= self.sessions.len() || idx == self.active { return; }
        let switch_start = Instant::now();
        let from = self.active;
        self.active = idx;
        let session = &mut self.sessions[idx];
        session.snapshot = snapshot_grid(&session.term);
        session.cache.clear();
        session.dirty = false;
        let switch_latency = switch_start.elapsed().as_secs_f64() * 1000.0;
        self.switch_latencies.push(switch_latency);
        self.metrics_lines.push(json!({
            "schema_version": 1,
            "event_type": "session_switched",
            "backend": self.args.backend,
            "timestamp_ms": ts_ms(&self.boot_instant),
            "from_session_id": from,
            "to_session_id": idx,
            "switch_latency_ms": switch_latency,
            "active_session_dirty_rows": self.rows,
        }));
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::WindowResized(size) => {
                let cw = 9.0f32;
                let ch = 18.0f32;
                let new_cols = ((size.width - 140.0) / cw).floor().max(2.0) as u16;
                let new_rows = ((size.height - 20.0) / ch).floor().max(2.0) as u16;
                if new_cols == self.cols as u16 && new_rows == self.rows as u16 { return; }
                self.cols = new_cols as usize;
                self.rows = new_rows as usize;
                let session = &mut self.sessions[self.active];
                let term_size = TermSize { cols: self.cols, rows: self.rows };
                session.term.resize(term_size);
                let _ = session.backend.resize(new_cols, new_rows);
                session.snapshot = snapshot_grid(&session.term);
                session.cache.clear();
            }
            Message::KeyEvent(keyboard::Event::KeyPressed { key, modifiers, text: txt, .. }) => {
                if self.sessions.is_empty() { return; }
                let cmd = if cfg!(target_os = "macos") { modifiers.command() } else { modifiers.control() };

                // Cmd+1..9
                if cmd && !modifiers.shift() {
                    if let keyboard::Key::Character(c) = &key {
                        if let Ok(digit) = c.as_str().parse::<usize>() {
                            if digit >= 1 && digit <= 9 {
                                self.switch_to(digit - 1);
                                return;
                            }
                        }
                    }
                }
                // Cmd+Tab / Cmd+Shift+Tab
                if cmd && matches!(key, keyboard::Key::Named(keyboard::key::Named::Tab)) {
                    let len = self.sessions.len();
                    if modifiers.shift() {
                        self.switch_to((self.active + len - 1) % len);
                    } else {
                        self.switch_to((self.active + 1) % len);
                    }
                    return;
                }
                // Cmd+N
                if cmd && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "n") {
                    let id = self.sessions.len();
                    let backend: Box<dyn PlaneAiTerminalSession> = match self.args.session_source.as_str() {
                        "spike-local" => {
                            let policy = QueuePolicy::from_str(&self.args.output_queue_policy);
                            Box::new(Shell::spawn_with_policy(
                                id, self.cols as u16, self.rows as u16,
                                self.args.session_command.as_deref(), policy,
                            ))
                        }
                        "planeai-local" => {
                            Box::new(crate::planeai_local::PlaneAiLocalSession::spawn(
                                id, self.cols as u16, self.rows as u16,
                                self.args.session_command.as_deref(),
                            ).unwrap_or_else(|e| panic!("Failed to spawn planeai-local session {}: {}", id, e)))
                        }
                        "planeai-daemon" => {
                            Box::new(crate::daemon_session::DaemonSession::spawn(
                                id, self.cols as u16, self.rows as u16,
                                self.args.session_command.as_deref(),
                            ).unwrap_or_else(|e| panic!("Failed to spawn planeai-daemon session {}: {}", id, e)))
                        }
                        other => {
                            eprintln!("Error: unsupported --session-source '{}'. Supported: spike-local, planeai-local, planeai-daemon", other);
                            return;
                        }
                    };
                    let term = new_term(self.cols, self.rows);
                    let processor = new_processor();
                    let snapshot = snapshot_grid(&term);
                    let name = format!("Session {}", id + 1);
                    self.metrics_lines.push(json!({
                        "schema_version": 1, "event_type": "session_created",
                        "backend": self.args.backend,
                        "timestamp_ms": ts_ms(&self.boot_instant),
                        "session_id": id, "session_name": &name,
                    }));
                    self.sessions.push(Session {
                        id, name, backend, term, processor, snapshot,
                        cache: Cache::new(), bytes_processed: 0, parse_times: Vec::new(),
                        render_works: Vec::new(), dirty: false,
                    });
                    self.switch_to(id);
                    return;
                }
                // Cmd+W
                if cmd && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "w") {
                    if self.sessions.len() <= 1 { return; }
                    let closed_id = self.sessions[self.active].id;
                    self.metrics_lines.push(json!({
                        "schema_version": 1, "event_type": "session_closed",
                        "backend": self.args.backend,
                        "timestamp_ms": ts_ms(&self.boot_instant),
                        "session_id": closed_id,
                    }));
                    self.sessions.remove(self.active);
                    if self.active >= self.sessions.len() { self.active = self.sessions.len() - 1; }
                    let idx = self.active;
                    self.sessions[idx].snapshot = snapshot_grid(&self.sessions[idx].term);
                    self.sessions[idx].cache.clear();
                    return;
                }
                // Paste
                let is_paste = if cfg!(target_os = "macos") {
                    modifiers.command() && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "v")
                } else {
                    modifiers.control() && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "v")
                };
                if is_paste {
                    if let Ok(mut clipboard) = Clipboard::new() {
                        if let Ok(t) = clipboard.get_text() {
                            if !t.is_empty() {
                                let _ = self.sessions[self.active].backend.write(t.as_bytes());
                            }
                        }
                    }
                    return;
                }
                // Input to active session
                let bytes = input::encode_key_event(&key, &modifiers, &txt);
                if let Some(ref b) = bytes {
                    if !b.is_empty() {
                        let _ = self.sessions[self.active].backend.write(b);
                    }
                }
            }
            Message::KeyEvent(_) => {}
            Message::Poll => {
                if self.done || self.sessions.is_empty() { return; }
                let now = Instant::now();
                if let Some(prev) = self.last_frame_instant {
                    let delta = now.duration_since(prev).as_secs_f64() * 1000.0;
                    self.frame_deltas.push(delta);
                    self.last_frame_ms = delta;
                }
                self.last_frame_instant = Some(now);

                self.ui_poll_count += 1;
                for i in 0..self.sessions.len() {
                    const MAX_BYTES_PER_SESSION_PER_POLL: usize = 2 * 1024 * 1024; // 2 MB
                    let mut total_drained = 0usize;
                    loop {
                        if total_drained >= MAX_BYTES_PER_SESSION_PER_POLL { break; }
                        let output = self.sessions[i].backend.try_read_batch().unwrap_or(None);
                        match output {
                            Some(data) => {
                                let batch_len = data.len();
                                total_drained += batch_len;
                                self.ui_batches_drained_total += 1;
                                self.ui_bytes_drained_total += batch_len as u64;
                                self.sessions[i].bytes_processed += batch_len as u64;
                                let parse_start = Instant::now();
                                let session = &mut self.sessions[i];
                                session.processor.advance(&mut session.term, &data);
                                let parse_ms = parse_start.elapsed().as_secs_f64() * 1000.0;
                                session.parse_times.push(parse_ms);

                                if i == self.active {
                                    self.active_parse_times.push(parse_ms);
                                } else {
                                    self.inactive_parse_times.push(parse_ms);
                                    self.sessions[i].dirty = true;
                                }
                            }
                            None => break,
                        }
                    }
                    // Only snapshot the active session once after all batches are drained
                    if i == self.active && total_drained > 0 {
                        let render_start = Instant::now();
                        let session = &mut self.sessions[i];
                        session.snapshot = snapshot_grid(&session.term);
                        session.cache.clear();
                        let render_ms = render_start.elapsed().as_secs_f64() * 1000.0;
                        self.sessions[i].render_works.push(render_ms);
                        self.active_render_works.push(render_ms);
                        self.sessions[i].dirty = false;
                    }
                }
                self.frames += 1;

                if let Some(max_ms) = self.args.max_runtime_ms {
                    if ts_ms(&self.boot_instant) > max_ms as f64 {
                        self.done = true;
                        self.finish();
                    }
                }
                if self.args.exit_when_done && self.args.session_command.is_some() {
                    let all_exited = self.sessions.iter().all(|s| s.backend.has_exited() && s.backend.pending_bytes() == 0);
                    if all_exited {
                        self.done = true;
                        self.finish();
                    }
                }
            }
        }
    }

    fn finish(&mut self) {
        let wall_ms = ts_ms(&self.boot_instant);
        let total_bytes: u64 = self.sessions.iter().map(|s| s.bytes_processed).sum();
        let active_bytes = self.sessions.get(self.active).map(|s| s.bytes_processed).unwrap_or(0);
        let inactive_bytes = total_bytes - active_bytes;

        let mut sw = self.switch_latencies.clone(); sw.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut ap = self.active_parse_times.clone(); ap.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut ar = self.active_render_works.clone(); ar.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut ip = self.inactive_parse_times.clone(); ip.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut fd = self.frame_deltas.clone(); fd.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let max_pending: Vec<u64> = self.sessions.iter().map(|s| s.backend.max_pending_bytes() as u64).collect();
        let dropped: Vec<u64> = self.sessions.iter().map(|s| s.backend.bytes_dropped()).collect();
        let total_dropped: u64 = dropped.iter().sum();
        let pipeline_diags: Vec<_> = self.sessions.iter().map(|s| s.backend.pipeline_diag()).collect();

        let summary = json!({
            "schema_version": 1, "event_type": "summary",
            "backend": self.args.backend, "session_source": self.args.session_source,
            "daemon_pty_core": std::env::var("PLANEAI_DAEMON_PTY_CORE").unwrap_or_else(|_| "legacy".to_string()),
            "mode": "multi-session", "session_count": self.sessions.len(),
            "cols": self.cols, "rows": self.rows,
            "wall_time_ms": wall_ms, "total_bytes": total_bytes,
            "total_bytes_active_sessions": active_bytes,
            "total_bytes_inactive_sessions": inactive_bytes,
            "total_daemon_output_bytes": if self.args.session_source == "planeai-daemon" { total_bytes } else { 0 },
            "average_mb_per_sec": if wall_ms > 0.0 { (total_bytes as f64 / 1_048_576.0) / (wall_ms / 1000.0) } else { 0.0 },
            "p50_session_switch_latency_ms": pct_or_null(&sw, 50.0),
            "p95_session_switch_latency_ms": pct_or_null(&sw, 95.0),
            "p99_session_switch_latency_ms": pct_or_null(&sw, 99.0),
            "p50_active_render_work_ms": pct_or_null(&ar, 50.0),
            "p95_active_render_work_ms": pct_or_null(&ar, 95.0),
            "p99_active_render_work_ms": pct_or_null(&ar, 99.0),
            "p50_active_parse_time_ms": pct_or_null(&ap, 50.0),
            "p95_active_parse_time_ms": pct_or_null(&ap, 95.0),
            "p99_active_parse_time_ms": pct_or_null(&ap, 99.0),
            "p50_inactive_parse_time_ms": pct_or_null(&ip, 50.0),
            "p95_inactive_parse_time_ms": pct_or_null(&ip, 95.0),
            "p99_inactive_parse_time_ms": pct_or_null(&ip, 99.0),
            "p50_frame_delta_ms": pct_or_null(&fd, 50.0),
            "p95_frame_delta_ms": pct_or_null(&fd, 95.0),
            "p99_frame_delta_ms": pct_or_null(&fd, 99.0),
            "frames_over_16_7ms": fd.iter().filter(|&&t| t > 16.7).count(),
            "frames_over_33_3ms": fd.iter().filter(|&&t| t > 33.3).count(),
            "frames_over_50ms": fd.iter().filter(|&&t| t > 50.0).count(),
            "max_pending_pty_output_bytes_total": max_pending.iter().max().copied().unwrap_or(0),
            "max_pending_pty_output_bytes_per_session": max_pending,
            "output_bytes_dropped_total": total_dropped,
            "output_bytes_dropped_per_session": dropped,
            "ui_poll_count": self.ui_poll_count,
            "ui_batches_drained_total": self.ui_batches_drained_total,
            "ui_bytes_drained_total": self.ui_bytes_drained_total,
            "ui_avg_batches_per_poll": if self.ui_poll_count > 0 { self.ui_batches_drained_total as f64 / self.ui_poll_count as f64 } else { 0.0 },
            "ui_avg_bytes_per_poll": if self.ui_poll_count > 0 { self.ui_bytes_drained_total as f64 / self.ui_poll_count as f64 } else { 0.0 },
            "pipeline_diag_per_session": pipeline_diags,
            "final_rss_mb": get_rss_mb(),
        });
        self.metrics_lines.push(summary);
        self.write_metrics();
        if self.args.exit_when_done { std::process::exit(0); }
    }

    fn write_metrics(&self) {
        if let Some(ref path) = self.args.metrics {
            if let Some(parent) = path.parent() { let _ = fs::create_dir_all(parent); }
            let mut file = fs::File::create(path).expect("Failed to create metrics file");
            for line in &self.metrics_lines {
                writeln!(file, "{}", serde_json::to_string(line).unwrap()).unwrap();
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        if self.sessions.is_empty() { return text("No sessions").into(); }

        let mut session_list = column![].spacing(2).width(Length::Fixed(140.0));
        for (i, s) in self.sessions.iter().enumerate() {
            let label = if i == self.active { format!("▶ {}", s.name) } else { format!("  {}", s.name) };
            let color = if i == self.active { Color::from_rgb8(100, 200, 255) } else { Color::from_rgb8(180, 180, 180) };
            session_list = session_list.push(text(label).size(13).color(color).font(Font::MONOSPACE));
        }
        let left_panel = container(session_list).padding(8)
            .style(|_: &Theme| container::Style { background: Some(Color::from_rgb8(20, 20, 20).into()), ..Default::default() });

        let active_session = &self.sessions[self.active];
        let terminal_canvas = Canvas::new(MultiTermRenderer { snapshot: &active_session.snapshot, cache: &active_session.cache })
            .width(Length::Fill).height(Length::Fill);

        let pending = self.sessions[self.active].backend.pending_bytes();
        let bytes = self.sessions[self.active].bytes_processed;
        let status_text = format!(
            " {} | {} bytes | queue: {} | frame: {:.1}ms | sessions: {} | source: {}",
            active_session.name, bytes, pending, self.last_frame_ms, self.sessions.len(), self.args.session_source,
        );
        let status_bar = container(text(status_text).size(12).color(Color::from_rgb8(180, 180, 180)).font(Font::MONOSPACE))
            .width(Length::Fill).padding(2)
            .style(|_: &Theme| container::Style { background: Some(Color::from_rgb8(40, 40, 40).into()), ..Default::default() });

        row![left_panel, column![terminal_canvas, status_bar]].into()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch(vec![
            keyboard::listen().map(Message::KeyEvent),
            iced::time::every(Duration::from_millis(16)).map(|_| Message::Poll),
            event::listen_with(|ev, _status, _id| {
                if let iced::Event::Window(window::Event::Resized(size)) = ev { Some(Message::WindowResized(size)) } else { None }
            }),
        ])
    }
}

fn pct_or_null(sorted: &[f64], p: f64) -> serde_json::Value {
    if sorted.is_empty() { json!(null) } else { json!(percentile(sorted, p)) }
}

struct MultiTermRenderer<'a> { snapshot: &'a GridSnapshot, cache: &'a Cache }

impl<'a> Program<Message> for MultiTermRenderer<'a> {
    type State = ();
    fn draw(&self, _state: &Self::State, renderer: &Renderer, _theme: &Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<canvas::Geometry> {
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
                        frame.fill_rectangle(Point::new(x, y), Size::new(cw, ch), Color::from_rgba8(200, 200, 200, 0.4));
                    }
                    if cell.c != ' ' && cell.c != '\0' {
                        frame.fill_text(canvas::Text {
                            content: cell.c.to_string(), position: Point::new(x, y + 1.0),
                            color: cell.fg, size: font_size.into(), font: Font::MONOSPACE, ..Default::default()
                        });
                    }
                }
            }
        });
        vec![geom]
    }
}

fn title(_state: &MultiApp) -> String { "PlaneAI Multi-Session Spike".into() }

pub fn run(args: Args) -> iced::Result {
    let cols = args.cols;
    let rows = args.rows;
    MULTI_ARGS.set(args).unwrap();
    iced::application(MultiApp::boot, MultiApp::update, MultiApp::view)
        .title(title)
        .subscription(MultiApp::subscription)
        .window_size(Size::new(cols as f32 * 9.0 + 140.0, rows as f32 * 18.0 + 20.0))
        .run()
}
