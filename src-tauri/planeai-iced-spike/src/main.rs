#![recursion_limit = "256"]

pub mod adapter;
pub mod common;
pub mod components;
pub mod daemon_session;
pub mod input;
mod multi_session;
pub mod planeai_local;
pub mod project_form;
pub mod shell;
pub mod sidebar;
pub mod theme;
pub mod theme_parser;
mod workflow;

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use alacritty_terminal::event::Event;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Cell;
use alacritty_terminal::vte::ansi::Processor;
use arboard::Clipboard;
use clap::Parser as ClapParser;
use iced::event;
use iced::keyboard;
use iced::mouse;
use iced::widget::canvas::{self, Cache, Program, Text};
use iced::widget::Canvas;
use iced::window;
use iced::{Color, Element, Font, Length, Point, Rectangle, Renderer, Size, Subscription, Theme};
use serde_json::json;

// --- CLI ---

#[derive(ClapParser, Debug, Clone)]
pub struct Args {
    #[arg(long)]
    pub replay: Option<PathBuf>,
    #[arg(long)]
    pub shell: bool,
    #[arg(long)]
    pub command: Option<String>,
    #[arg(long, default_value_t = 120)]
    pub cols: usize,
    #[arg(long, default_value_t = 40)]
    pub rows: usize,
    #[arg(long, default_value_t = 16384)]
    pub chunk_size: usize,
    #[arg(long, default_value_t = 4)]
    pub chunk_interval_ms: u64,
    #[arg(long)]
    pub metrics: Option<PathBuf>,
    #[arg(long, default_value = "iced-alacritty")]
    pub backend: String,
    #[arg(long)]
    pub exit_when_done: bool,
    #[arg(long)]
    pub snapshot: Option<PathBuf>,
    #[arg(long)]
    pub font_size: Option<f32>,
    #[arg(long)]
    pub font_family: Option<String>,
    #[arg(long)]
    pub scrollback_lines: Option<usize>,
    #[arg(long)]
    pub max_runtime_ms: Option<u64>,
    #[arg(long)]
    pub warmup_ms: Option<u64>,
    #[arg(long)]
    pub input_benchmark: bool,
    #[arg(long, default_value_t = 50)]
    pub input_interval_ms: u64,
    #[arg(long, default_value_t = 100)]
    pub input_events: u64,
    #[arg(long)]
    pub flood_command: Option<String>,
    #[arg(long, default_value = "block")]
    pub output_queue_policy: String,
    #[arg(long)]
    pub multi_session: bool,
    #[arg(long, default_value_t = 3)]
    pub sessions: usize,
    #[arg(long)]
    pub session_command: Option<String>,
    #[arg(long, default_value = "spike-local")]
    pub session_source: String,
    #[arg(long, default_value_t = true)]
    pub detach_on_close: bool,
    #[arg(long)]
    pub kill_on_close: bool,
    #[arg(long)]
    pub kill_sessions_on_exit: bool,
    #[arg(long)]
    pub planeai_workflow: bool,
    #[arg(long)]
    pub cwd: Option<PathBuf>,
    #[arg(long)]
    pub agent_command: Option<String>,
    #[arg(long)]
    pub extra_path_dirs: Vec<String>,
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// Enable auto-approve/yolo mode for task launches
    #[arg(long)]
    pub yolo: bool,
}

static ARGS: OnceLock<Args> = OnceLock::new();

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((p / 100.0) * (sorted.len() as f64 - 1.0)).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn get_rss_mb() -> f64 {
    #[cfg(target_os = "macos")]
    unsafe {
        let mut info = std::mem::zeroed::<libc::rusage>();
        if libc::getrusage(libc::RUSAGE_SELF, &mut info) == 0 {
            return info.ru_maxrss as f64 / (1024.0 * 1024.0);
        }
    }
    0.0
}

fn ts_ms(start: &Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

// --- Alacritty terminal setup ---

struct EventProxy;
impl alacritty_terminal::event::EventListener for EventProxy {
    fn send_event(&self, _event: Event) {}
}

struct TermSize {
    cols: usize,
    rows: usize,
}
impl Dimensions for TermSize {
    fn columns(&self) -> usize {
        self.cols
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn total_lines(&self) -> usize {
        self.rows
    }
}

// --- Color mapping ---

fn ansi_color_to_iced(color: &alacritty_terminal::vte::ansi::Color) -> Color {
    use alacritty_terminal::vte::ansi::Color as AC;
    use alacritty_terminal::vte::ansi::NamedColor;
    match color {
        AC::Named(n) => match n {
            NamedColor::Black => Color::from_rgb8(0, 0, 0),
            NamedColor::Red => Color::from_rgb8(205, 49, 49),
            NamedColor::Green => Color::from_rgb8(13, 188, 121),
            NamedColor::Yellow => Color::from_rgb8(229, 229, 16),
            NamedColor::Blue => Color::from_rgb8(36, 114, 200),
            NamedColor::Magenta => Color::from_rgb8(188, 63, 188),
            NamedColor::Cyan => Color::from_rgb8(17, 168, 205),
            NamedColor::White => Color::from_rgb8(229, 229, 229),
            NamedColor::BrightBlack => Color::from_rgb8(102, 102, 102),
            NamedColor::BrightRed => Color::from_rgb8(241, 76, 76),
            NamedColor::BrightGreen => Color::from_rgb8(35, 209, 139),
            NamedColor::BrightYellow => Color::from_rgb8(245, 245, 67),
            NamedColor::BrightBlue => Color::from_rgb8(59, 142, 234),
            NamedColor::BrightMagenta => Color::from_rgb8(214, 112, 214),
            NamedColor::BrightCyan => Color::from_rgb8(41, 184, 219),
            NamedColor::BrightWhite | NamedColor::Foreground | NamedColor::BrightForeground => {
                Color::from_rgb8(229, 229, 229)
            }
            NamedColor::Background => Color::from_rgb8(0, 0, 0),
            _ => Color::from_rgb8(229, 229, 229),
        },
        AC::Spec(rgb) => Color::from_rgb8(rgb.r, rgb.g, rgb.b),
        AC::Indexed(idx) => {
            let i = *idx;
            if i < 16 {
                let table: [(u8, u8, u8); 16] = [
                    (0, 0, 0),
                    (205, 49, 49),
                    (13, 188, 121),
                    (229, 229, 16),
                    (36, 114, 200),
                    (188, 63, 188),
                    (17, 168, 205),
                    (229, 229, 229),
                    (102, 102, 102),
                    (241, 76, 76),
                    (35, 209, 139),
                    (245, 245, 67),
                    (59, 142, 234),
                    (214, 112, 214),
                    (41, 184, 219),
                    (255, 255, 255),
                ];
                let (r, g, b) = table[i as usize];
                Color::from_rgb8(r, g, b)
            } else if i < 232 {
                let j = i - 16;
                let r = (j / 36) % 6;
                let g = (j / 6) % 6;
                let b = j % 6;
                let v = |c: u8| if c == 0 { 0u8 } else { 55 + 40 * c };
                Color::from_rgb8(v(r), v(g), v(b))
            } else {
                let v = 8 + 10 * (i - 232);
                Color::from_rgb8(v, v, v)
            }
        }
    }
}

// --- Grid snapshot ---

#[derive(Clone)]
struct GridCell {
    c: char,
    fg: Color,
    bg: Color,
}

#[derive(Clone)]
#[allow(dead_code)]
struct GridSnapshot {
    cells: Vec<Vec<GridCell>>,
    cursor_line: usize,
    cursor_col: usize,
    cols: usize,
    rows: usize,
}

fn snapshot_grid(term: &alacritty_terminal::Term<EventProxy>) -> GridSnapshot {
    let grid = term.grid();
    let rows = grid.screen_lines();
    let cols = grid.columns();
    let cursor = grid.cursor.point;
    let mut cells = Vec::with_capacity(rows);
    for i in 0..rows {
        let mut row = Vec::with_capacity(cols);
        for j in 0..cols {
            let cell: &Cell = &grid[Line(i as i32)][Column(j)];
            row.push(GridCell {
                c: cell.c,
                fg: ansi_color_to_iced(&cell.fg),
                bg: ansi_color_to_iced(&cell.bg),
            });
        }
        cells.push(row);
    }
    GridSnapshot {
        cells,
        cursor_line: cursor.line.0 as usize,
        cursor_col: cursor.column.0,
        cols,
        rows,
    }
}

fn snapshot_text(term: &alacritty_terminal::Term<EventProxy>) -> String {
    let grid = term.grid();
    let rows = grid.screen_lines();
    let cols = grid.columns();
    let mut out = String::new();
    for i in 0..rows {
        for j in 0..cols {
            let c = grid[Line(i as i32)][Column(j)].c;
            out.push(if c == '\0' { ' ' } else { c });
        }
        let trimmed = out.trim_end_matches(' ');
        out.truncate(trimmed.len());
        out.push('\n');
    }
    out
}

// --- App State ---

struct App {
    data: Vec<u8>,
    offset: usize,
    term: alacritty_terminal::Term<EventProxy>,
    processor: Processor,
    snapshot: GridSnapshot,
    cache: Cache,
    done: bool,
    boot_instant: Instant,
    replay_start: Option<Instant>,
    last_frame_instant: Option<Instant>,
    frames: u64,
    frame_deltas: Vec<f64>,
    render_works: Vec<f64>,
    parse_times: Vec<f64>,
    max_pending_unparsed: usize,
    metrics_lines: Vec<serde_json::Value>,
    // shell mode
    pty: Option<shell::Shell>,
    is_shell_mode: bool,
    // input metrics
    input_id_counter: u64,
    input_write_latencies: Vec<f64>,
    input_events_received: u64,
    input_events_written: u64,
    pty_output_batches: u64,
    pty_output_bytes: u64,
    max_pending_pty_output: usize,
    // input benchmark
    input_bench_sent: u64,
    input_bench_last: Option<Instant>,
    // window size tracking
    last_cols: u16,
    last_rows: u16,
    // warmup
    warmup_done: bool,
    frame_deltas_after_warmup: Vec<f64>,
    // paste
    input_events_failed: u64,
}

#[derive(Debug, Clone)]
enum Message {
    Tick,
    KeyEvent(keyboard::Event),
    PtyPoll,
    WindowResized(Size),
}

impl App {
    fn boot() -> (Self, iced::Task<Message>) {
        let args = ARGS.get().unwrap();
        let is_shell_mode = args.shell || args.command.is_some();
        let data = if let Some(ref path) = args.replay {
            fs::read(path).expect("Failed to read replay file")
        } else {
            Vec::new()
        };
        let scrollback = args.scrollback_lines.unwrap_or(0);
        let size = TermSize {
            cols: args.cols,
            rows: args.rows + scrollback,
        };
        let config = alacritty_terminal::term::Config::default();
        let term = alacritty_terminal::Term::new(config, &size, EventProxy);
        let processor = Processor::new();
        let snapshot = snapshot_grid(&term);

        let pty = if is_shell_mode {
            let cmd = args.command.as_deref().or(args.flood_command.as_deref());
            let policy = shell::QueuePolicy::from_str(&args.output_queue_policy);
            Some(shell::Shell::spawn_with_policy(
                0,
                args.cols as u16,
                args.rows as u16,
                cmd,
                policy,
            ))
        } else {
            None
        };

        (
            Self {
                data,
                offset: 0,
                term,
                processor,
                snapshot,
                cache: Cache::new(),
                done: false,
                boot_instant: Instant::now(),
                replay_start: None,
                last_frame_instant: None,
                frames: 0,
                frame_deltas: Vec::new(),
                render_works: Vec::new(),
                parse_times: Vec::new(),
                max_pending_unparsed: 0,
                metrics_lines: Vec::new(),
                pty,
                is_shell_mode,
                input_id_counter: 0,
                input_write_latencies: Vec::new(),
                input_events_received: 0,
                input_events_written: 0,
                pty_output_batches: 0,
                pty_output_bytes: 0,
                max_pending_pty_output: 0,
                input_bench_sent: 0,
                input_bench_last: None,
                last_cols: args.cols as u16,
                last_rows: args.rows as u16,
                warmup_done: false,
                frame_deltas_after_warmup: Vec::new(),
                input_events_failed: 0,
            },
            planeai_iced_spike::font::font_load_task().discard(),
        )
    }

    fn common_fields(&self) -> serde_json::Value {
        let args = ARGS.get().unwrap();
        let fixture = args
            .replay
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| {
                if args.command.is_some() {
                    "<command>".into()
                } else {
                    "<shell>".into()
                }
            });
        json!({
            "schema_version": 1,
            "backend": args.backend,
            "fixture": fixture,
            "cols": args.cols,
            "rows": args.rows,
            "chunk_size": args.chunk_size,
            "chunk_interval_ms": args.chunk_interval_ms,
        })
    }

    fn emit(&mut self, event_type: &str, extra: serde_json::Value) {
        let mut obj = self.common_fields();
        let map = obj.as_object_mut().unwrap();
        map.insert("timestamp_ms".into(), json!(ts_ms(&self.boot_instant)));
        map.insert("event_type".into(), json!(event_type));
        if let Some(extra_map) = extra.as_object() {
            for (k, v) in extra_map {
                map.insert(k.clone(), v.clone());
            }
        }
        self.metrics_lines.push(obj);
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::WindowResized(size) => {
                if self.pty.is_none() {
                    return;
                }
                // Compute new cols/rows from window size
                let font_size = planeai_iced_spike::font::terminal_font_size();
                let (cw, ch) = planeai_iced_spike::font::cell_dimensions(font_size);
                let new_cols = (size.width / cw).floor() as u16;
                let new_rows = (size.height / ch).floor() as u16;
                if new_cols < 2 || new_rows < 2 {
                    return;
                }
                if new_cols == self.last_cols && new_rows == self.last_rows {
                    return;
                }

                let resize_start = Instant::now();
                // Resize alacritty_terminal
                let term_size = TermSize {
                    cols: new_cols as usize,
                    rows: new_rows as usize,
                };
                self.term.resize(term_size);
                // Resize PTY
                self.pty.as_ref().unwrap().resize(new_cols, new_rows);
                let latency = resize_start.elapsed().as_secs_f64() * 1000.0;

                self.emit(
                    "pty_resize",
                    json!({
                        "cols": new_cols,
                        "rows": new_rows,
                        "prev_cols": self.last_cols,
                        "prev_rows": self.last_rows,
                        "resize_latency_ms": latency,
                    }),
                );

                self.last_cols = new_cols;
                self.last_rows = new_rows;
                self.snapshot = snapshot_grid(&self.term);
                self.cache.clear();
            }
            Message::KeyEvent(keyboard::Event::KeyPressed {
                key,
                modifiers,
                text,
                ..
            }) => {
                if self.pty.is_none() {
                    return;
                }

                // Paste: Cmd+V (macOS) or Ctrl+V (Linux/Windows without Ctrl encoding)
                let is_paste = if cfg!(target_os = "macos") {
                    modifiers.command()
                        && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "v")
                } else {
                    modifiers.control()
                        && matches!(&key, keyboard::Key::Character(c) if c.as_str() == "v")
                };

                if is_paste {
                    self.handle_paste();
                    return;
                }

                self.input_events_received += 1;
                self.input_id_counter += 1;
                let input_id = self.input_id_counter;

                self.emit(
                    "input_event_received",
                    json!({
                        "input_id": input_id,
                        "input_kind": "key",
                        "key": format!("{:?}", key),
                    }),
                );

                let bytes = input::encode_key_event(&key, &modifiers, &text);
                if let Some(ref bytes) = bytes {
                    if !bytes.is_empty() {
                        let write_start = Instant::now();
                        self.pty.as_ref().unwrap().write(bytes);
                        let latency = write_start.elapsed().as_secs_f64() * 1000.0;
                        self.input_write_latencies.push(latency);
                        self.input_events_written += 1;

                        self.emit(
                            "input_write_done",
                            json!({
                                "input_id": input_id,
                                "input_bytes": bytes.len(),
                                "input_write_latency_ms": latency,
                            }),
                        );
                    }
                }
            }
            Message::KeyEvent(_) => {}
            Message::PtyPoll => {
                if self.pty.is_none() {
                    return;
                }

                // Drain output
                let pending = self.pty.as_ref().unwrap().pending_len();
                if pending > self.max_pending_pty_output {
                    self.max_pending_pty_output = pending;
                }
                let output = self.pty.as_ref().unwrap().drain();

                if !output.is_empty() {
                    let batch_len = output.len();
                    self.pty_output_batches += 1;
                    self.pty_output_bytes += batch_len as u64;

                    let parse_start = Instant::now();
                    self.processor.advance(&mut self.term, &output);
                    let parse_ms = parse_start.elapsed().as_secs_f64() * 1000.0;
                    self.parse_times.push(parse_ms);

                    let render_start = Instant::now();
                    self.snapshot = snapshot_grid(&self.term);
                    self.cache.clear();
                    let render_ms = render_start.elapsed().as_secs_f64() * 1000.0;
                    self.render_works.push(render_ms);
                    self.frames += 1;

                    let now = Instant::now();
                    let frame_delta = self
                        .last_frame_instant
                        .map(|prev| now.duration_since(prev).as_secs_f64() * 1000.0);
                    self.last_frame_instant = Some(now);
                    if let Some(fd) = frame_delta {
                        self.frame_deltas.push(fd);
                        // Warmup: skip early frames
                        if !self.warmup_done {
                            let warmup_ms = ARGS.get().unwrap().warmup_ms.unwrap_or(500);
                            if ts_ms(&self.boot_instant) > warmup_ms as f64 {
                                self.warmup_done = true;
                            }
                        }
                        if self.warmup_done {
                            self.frame_deltas_after_warmup.push(fd);
                        }
                    }

                    let pending_after = self.pty.as_ref().unwrap().pending_len();
                    self.emit(
                        "pty_output_batch",
                        json!({
                            "batch_bytes": batch_len,
                            "parse_time_ms": parse_ms,
                            "render_work_ms": render_ms,
                            "frame_delta_ms": frame_delta,
                            "pending_pty_output_bytes": pending_after,
                        }),
                    );
                }

                // Input benchmark: inject synthetic keystrokes
                let args = ARGS.get().unwrap();
                if args.input_benchmark {
                    let should_send = if let Some(last) = self.input_bench_last {
                        last.elapsed() >= Duration::from_millis(args.input_interval_ms)
                    } else {
                        true
                    };
                    if should_send && self.input_bench_sent < args.input_events {
                        self.input_bench_sent += 1;
                        self.input_bench_last = Some(Instant::now());
                        self.input_id_counter += 1;
                        let input_id = self.input_id_counter;
                        let ch = b'a' + ((self.input_bench_sent % 26) as u8);

                        self.emit(
                            "input_event_received",
                            json!({
                                "input_id": input_id,
                                "input_kind": "synthetic",
                                "key": (ch as char).to_string(),
                            }),
                        );

                        let write_start = Instant::now();
                        self.pty.as_ref().unwrap().write(&[ch]);
                        let latency = write_start.elapsed().as_secs_f64() * 1000.0;
                        self.input_write_latencies.push(latency);
                        self.input_events_received += 1;
                        self.input_events_written += 1;

                        self.emit(
                            "input_write_done",
                            json!({
                                "input_id": input_id,
                                "input_bytes": 1,
                                "input_write_latency_ms": latency,
                            }),
                        );
                    }
                    if self.input_bench_sent >= args.input_events && args.exit_when_done {
                        self.done = true;
                        self.finish_shell();
                    }
                }

                // Detect PTY child exit
                if !self.done
                    && args.exit_when_done
                    && self.pty.as_ref().unwrap().has_exited()
                    && self.pty.as_ref().unwrap().pending_len() == 0
                {
                    self.done = true;
                    self.finish_shell();
                }
            }
            Message::Tick => {
                if self.done || self.is_shell_mode {
                    return;
                }
                let args = ARGS.get().unwrap();

                if self.replay_start.is_none() {
                    self.replay_start = Some(Instant::now());
                    self.emit(
                        "replay_start",
                        json!({
                            "bytes_total": 0,
                            "queue_depth_bytes": self.data.len(),
                        }),
                    );
                }

                let now = Instant::now();
                let frame_delta_ms = self
                    .last_frame_instant
                    .map(|prev| now.duration_since(prev).as_secs_f64() * 1000.0);
                self.last_frame_instant = Some(now);
                if let Some(fd) = frame_delta_ms {
                    self.frame_deltas.push(fd);
                    if !self.warmup_done {
                        let warmup_ms = ARGS.get().unwrap().warmup_ms.unwrap_or(500);
                        if ts_ms(&self.boot_instant) > warmup_ms as f64 {
                            self.warmup_done = true;
                        }
                    }
                    if self.warmup_done {
                        self.frame_deltas_after_warmup.push(fd);
                    }
                }

                let end = (self.offset + args.chunk_size).min(self.data.len());
                let chunk = self.data[self.offset..end].to_vec();
                let bytes_fed = end - self.offset;

                // max_pending_unparsed = chunk size (synchronous replay)
                if bytes_fed > self.max_pending_unparsed {
                    self.max_pending_unparsed = bytes_fed;
                }

                self.emit(
                    "chunk_sent",
                    json!({
                        "bytes_total": self.offset + bytes_fed,
                        "bytes_since_last_event": bytes_fed,
                        "queue_depth_bytes": self.data.len() - end,
                    }),
                );

                let parse_start = Instant::now();
                self.processor.advance(&mut self.term, &chunk);
                let parse_time_ms = parse_start.elapsed().as_secs_f64() * 1000.0;
                self.parse_times.push(parse_time_ms);
                self.offset = end;

                self.emit(
                    "parse_batch",
                    json!({
                        "bytes_total": self.offset,
                        "bytes_since_last_event": bytes_fed,
                        "parse_time_ms": parse_time_ms,
                        "queue_depth_bytes": self.data.len() - self.offset,
                    }),
                );

                let render_start = Instant::now();
                self.snapshot = snapshot_grid(&self.term);
                self.cache.clear();
                let render_work_ms = render_start.elapsed().as_secs_f64() * 1000.0;
                self.render_works.push(render_work_ms);
                self.frames += 1;

                self.emit(
                    "render_frame",
                    json!({
                        "bytes_total": self.offset,
                        "render_work_ms": render_work_ms,
                        "frame_delta_ms": frame_delta_ms,
                        "queue_depth_bytes": self.data.len() - self.offset,
                        "rss_mb": get_rss_mb(),
                    }),
                );

                if self.frames.is_multiple_of(10) {
                    self.emit(
                        "frame_sample",
                        json!({
                            "bytes_total": self.offset,
                            "queue_depth_bytes": self.data.len() - self.offset,
                            "rss_mb": get_rss_mb(),
                            "frame_delta_ms": frame_delta_ms,
                            "render_work_ms": render_work_ms,
                            "parse_time_ms": parse_time_ms,
                        }),
                    );
                }
                if self.frames.is_multiple_of(20) {
                    self.emit(
                        "backlog_sample",
                        json!({
                            "bytes_total": self.offset,
                            "queue_depth_bytes": self.data.len() - self.offset,
                        }),
                    );
                }

                if let Some(max_ms) = args.max_runtime_ms {
                    if ts_ms(&self.replay_start.unwrap()) > max_ms as f64 {
                        self.done = true;
                    }
                }
                if self.offset >= self.data.len() {
                    self.done = true;
                }
                if self.done {
                    self.finish_replay();
                }
            }
        }
    }

    fn handle_paste(&mut self) {
        let mut clipboard = match Clipboard::new() {
            Ok(c) => c,
            Err(_) => {
                self.input_events_failed += 1;
                return;
            }
        };
        let text = match clipboard.get_text() {
            Ok(t) => t,
            Err(_) => {
                self.input_events_failed += 1;
                return;
            }
        };
        if text.is_empty() {
            return;
        }

        self.input_events_received += 1;
        self.input_id_counter += 1;
        let input_id = self.input_id_counter;
        let bytes = text.as_bytes();

        self.emit(
            "input_event_received",
            json!({
                "input_id": input_id,
                "input_kind": "paste",
                "input_bytes": bytes.len(),
            }),
        );

        // Write paste content to PTY (no bracketed paste mode — documented limitation)
        let write_start = Instant::now();
        self.pty.as_ref().unwrap().write(bytes);
        let latency = write_start.elapsed().as_secs_f64() * 1000.0;
        self.input_write_latencies.push(latency);
        self.input_events_written += 1;

        self.emit(
            "input_write_done",
            json!({
                "input_id": input_id,
                "input_kind": "paste",
                "input_bytes": bytes.len(),
                "input_write_latency_ms": latency,
            }),
        );
    }

    fn finish_replay(&mut self) {
        let args = ARGS.get().unwrap();
        let replay_start = self.replay_start.unwrap();
        let wall_time_ms = replay_start.elapsed().as_secs_f64() * 1000.0;

        self.emit(
            "replay_done",
            json!({
                "bytes_total": self.offset,
                "queue_depth_bytes": self.data.len() - self.offset,
                "wall_time_ms": wall_time_ms,
            }),
        );

        if let Some(snap_path) = &args.snapshot {
            if let Some(parent) = snap_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            fs::write(snap_path, snapshot_text(&self.term)).expect("Failed to write snapshot");
        }

        let total_bytes = self.data.len();
        let replay_mode = if args.chunk_interval_ms == 0 {
            "maxspeed"
        } else {
            "realtime"
        };
        let expected_min =
            ((total_bytes as f64 / args.chunk_size as f64).ceil() as u64) * args.chunk_interval_ms;
        let avg_mb_s = if wall_time_ms > 0.0 {
            (total_bytes as f64 / 1_048_576.0) / (wall_time_ms / 1000.0)
        } else {
            0.0
        };

        let mut fd = self.frame_deltas.clone();
        fd.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut fd_warmup = self.frame_deltas_after_warmup.clone();
        fd_warmup.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut rw = self.render_works.clone();
        rw.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut pt = self.parse_times.clone();
        pt.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let startup_ms = replay_start.duration_since(self.boot_instant).as_secs_f64() * 1000.0;
        let warmup_ms = args.warmup_ms.unwrap_or(500);

        let summary = json!({
            "schema_version": 1, "event_type": "summary", "backend": args.backend,
            "fixture": args.replay.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
            "cols": args.cols, "rows": args.rows,
            "chunk_size": args.chunk_size, "chunk_interval_ms": args.chunk_interval_ms,
            "replay_mode": replay_mode,
            "total_bytes": total_bytes, "total_chunks": self.frames,
            "actual_chunk_count": self.frames,
            "average_chunk_size": if self.frames > 0 { total_bytes as f64 / self.frames as f64 } else { 0.0 },
            "expected_min_replay_time_ms": expected_min,
            "wall_time_ms": wall_time_ms, "producer_time_ms": wall_time_ms,
            "drain_time_ms": 0.0, "total_replay_time_ms": wall_time_ms,
            "average_mb_per_sec": avg_mb_s,
            "p50_frame_delta_ms": percentile(&fd, 50.0),
            "p95_frame_delta_ms": percentile(&fd_warmup, 95.0),
            "p99_frame_delta_ms": percentile(&fd_warmup, 99.0),
            "warmup_ms": warmup_ms,
            "frame_samples_total": self.frame_deltas.len(),
            "frame_samples_after_warmup": self.frame_deltas_after_warmup.len(),
            "p50_render_work_ms": percentile(&rw, 50.0),
            "p95_render_work_ms": percentile(&rw, 95.0),
            "p99_render_work_ms": percentile(&rw, 99.0),
            "p50_parse_time_ms": percentile(&pt, 50.0),
            "p95_parse_time_ms": percentile(&pt, 95.0),
            "p99_parse_time_ms": percentile(&pt, 99.0),
            "p50_write_latency_ms": null,
            "p95_write_latency_ms": null,
            "p99_write_latency_ms": null,
            "frames_over_16_7ms": fd_warmup.iter().filter(|&&t| t > 16.7).count(),
            "frames_over_33_3ms": fd_warmup.iter().filter(|&&t| t > 33.3).count(),
            "frames_over_50ms": fd_warmup.iter().filter(|&&t| t > 50.0).count(),
            "max_queue_depth_bytes": self.max_pending_unparsed,
            "queue_depth_at_end_bytes": self.data.len().saturating_sub(self.offset),
            "fixture_bytes_loaded": self.data.len(),
            "max_pending_unparsed_bytes": self.max_pending_unparsed,
            "max_pending_unrendered_bytes": 0,
            "max_pending_input_bytes": 0,
            "max_pending_pty_output_bytes": 0,
            "output_queue_capacity_bytes": null,
            "output_queue_policy": "synchronous",
            "output_bytes_dropped": 0,
            "startup_time_ms": startup_ms,
            "final_rss_mb": get_rss_mb(),
            "final_js_heap_mb": null,
            "snapshot_path": args.snapshot.as_ref().map(|p| p.to_string_lossy().to_string()),
            // input metrics (null for replay)
            "p50_input_write_latency_ms": null,
            "p95_input_write_latency_ms": null,
            "p99_input_write_latency_ms": null,
            "p50_input_to_echo_latency_ms": null,
            "p95_input_to_echo_latency_ms": null,
            "p99_input_to_echo_latency_ms": null,
            "input_events_received": 0,
            "input_events_written": 0,
            "input_events_echoed": null,
            "input_events_failed": 0,
            "max_input_queue_depth": 0,
            "pty_output_batches": 0,
            "pty_output_bytes": 0,
        });
        self.metrics_lines.push(summary);
        self.write_metrics();

        if args.exit_when_done {
            std::process::exit(0);
        }
    }

    fn finish_shell(&mut self) {
        let args = ARGS.get().unwrap();
        let wall_time_ms = ts_ms(&self.boot_instant);

        self.emit("shell_exit", json!({ "wall_time_ms": wall_time_ms }));

        let mut fd = self.frame_deltas.clone();
        fd.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut fd_warmup = self.frame_deltas_after_warmup.clone();
        fd_warmup.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut rw = self.render_works.clone();
        rw.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut pt = self.parse_times.clone();
        pt.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut iwl = self.input_write_latencies.clone();
        iwl.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let max_pty = self
            .pty
            .as_ref()
            .map(|p| *p.max_pending_bytes.lock().unwrap())
            .unwrap_or(0);
        let bytes_dropped = self.pty.as_ref().map(|p| p.bytes_dropped()).unwrap_or(0);
        let warmup_ms = args.warmup_ms.unwrap_or(500);

        let summary = json!({
            "schema_version": 1, "event_type": "summary", "backend": args.backend,
            "fixture": if args.command.is_some() { "<command>" } else { "<shell>" },
            "cols": self.last_cols, "rows": self.last_rows,
            "chunk_size": args.chunk_size, "chunk_interval_ms": args.chunk_interval_ms,
            "replay_mode": "live",
            "total_bytes": self.pty_output_bytes,
            "total_chunks": self.pty_output_batches,
            "actual_chunk_count": self.pty_output_batches,
            "average_chunk_size": if self.pty_output_batches > 0 { self.pty_output_bytes as f64 / self.pty_output_batches as f64 } else { 0.0 },
            "expected_min_replay_time_ms": 0,
            "wall_time_ms": wall_time_ms, "producer_time_ms": wall_time_ms,
            "drain_time_ms": 0.0, "total_replay_time_ms": wall_time_ms,
            "average_mb_per_sec": if wall_time_ms > 0.0 { (self.pty_output_bytes as f64 / 1_048_576.0) / (wall_time_ms / 1000.0) } else { 0.0 },
            // All frame deltas (raw)
            "p50_frame_delta_ms": percentile(&fd, 50.0),
            "p95_frame_delta_ms": percentile(&fd_warmup, 95.0),
            "p99_frame_delta_ms": percentile(&fd_warmup, 99.0),
            // Warmup-corrected
            "warmup_ms": warmup_ms,
            "frame_samples_total": self.frame_deltas.len(),
            "frame_samples_after_warmup": self.frame_deltas_after_warmup.len(),
            "p50_render_work_ms": percentile(&rw, 50.0),
            "p95_render_work_ms": percentile(&rw, 95.0),
            "p99_render_work_ms": percentile(&rw, 99.0),
            "p50_parse_time_ms": percentile(&pt, 50.0),
            "p95_parse_time_ms": percentile(&pt, 95.0),
            "p99_parse_time_ms": percentile(&pt, 99.0),
            "p50_write_latency_ms": null,
            "p95_write_latency_ms": null,
            "p99_write_latency_ms": null,
            "frames_over_16_7ms": fd_warmup.iter().filter(|&&t| t > 16.7).count(),
            "frames_over_33_3ms": fd_warmup.iter().filter(|&&t| t > 33.3).count(),
            "frames_over_50ms": fd_warmup.iter().filter(|&&t| t > 50.0).count(),
            // Queue/backpressure
            "output_queue_capacity_bytes": shell::MAX_BUFFER,
            "output_queue_policy": self.pty.as_ref().map(|p| p.policy.as_str()).unwrap_or("block"),
            "output_bytes_dropped": bytes_dropped,
            "producer_block_count": self.pty.as_ref().map(|p| p.producer_block_count()).unwrap_or(0),
            "producer_block_duration_ms": self.pty.as_ref().map(|p| p.producer_block_duration_ms()).unwrap_or(0.0),
            "max_pending_pty_output_bytes": max_pty,
            "queue_depth_at_end_bytes": self.pty.as_ref().map(|p| p.pending_len()).unwrap_or(0),
            "max_queue_depth_bytes": max_pty,
            "fixture_bytes_loaded": 0,
            "max_pending_unparsed_bytes": 0,
            "max_pending_unrendered_bytes": 0,
            "max_pending_input_bytes": 0,
            "startup_time_ms": 0.0,
            "final_rss_mb": get_rss_mb(),
            "final_js_heap_mb": null,
            "snapshot_path": null,
            // input metrics
            "p50_input_write_latency_ms": if iwl.is_empty() { json!(null) } else { json!(percentile(&iwl, 50.0)) },
            "p95_input_write_latency_ms": if iwl.is_empty() { json!(null) } else { json!(percentile(&iwl, 95.0)) },
            "p99_input_write_latency_ms": if iwl.is_empty() { json!(null) } else { json!(percentile(&iwl, 99.0)) },
            "max_input_write_latency_ms": if iwl.is_empty() { json!(null) } else { json!(iwl.last().unwrap()) },
            "p50_input_to_echo_latency_ms": null,
            "p95_input_to_echo_latency_ms": null,
            "p99_input_to_echo_latency_ms": null,
            "input_events_received": self.input_events_received,
            "input_events_written": self.input_events_written,
            "input_events_echoed": null,
            "input_events_failed": self.input_events_failed,
            "max_input_queue_depth": 0,
            "pty_output_batches": self.pty_output_batches,
            "pty_output_bytes": self.pty_output_bytes,
        });
        self.metrics_lines.push(summary);
        self.write_metrics();

        if args.exit_when_done {
            std::process::exit(0);
        }
    }

    fn write_metrics(&self) {
        let args = ARGS.get().unwrap();
        if let Some(path) = &args.metrics {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let mut file = fs::File::create(path).expect("Failed to create metrics file");
            for line in &self.metrics_lines {
                writeln!(file, "{}", serde_json::to_string(line).unwrap()).unwrap();
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        Canvas::new(TermRenderer {
            snapshot: &self.snapshot,
            cache: &self.cache,
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut subs = Vec::new();

        if self.pty.is_some() {
            subs.push(keyboard::listen().map(Message::KeyEvent));
            subs.push(iced::time::every(Duration::from_millis(16)).map(|_| Message::PtyPoll));
            subs.push(event::listen_with(|ev, _status, _id| {
                if let iced::Event::Window(window::Event::Resized(size)) = ev {
                    Some(Message::WindowResized(size))
                } else {
                    None
                }
            }));
        }

        if !self.done && !self.data.is_empty() && !self.is_shell_mode {
            let interval = ARGS.get().unwrap().chunk_interval_ms;
            let ms = if interval == 0 { 1 } else { interval };
            subs.push(iced::time::every(Duration::from_millis(ms)).map(|_| Message::Tick));
        }

        Subscription::batch(subs)
    }
}

// --- Canvas Program ---

struct TermRenderer<'a> {
    snapshot: &'a GridSnapshot,
    cache: &'a Cache,
}

impl<'a> Program<Message> for TermRenderer<'a> {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let geom = self.cache.draw(renderer, bounds.size(), |frame| {
            let font_size = planeai_iced_spike::font::terminal_font_size();
            let (cw, ch) = planeai_iced_spike::font::cell_dimensions(font_size);

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
                        frame.fill_text(Text {
                            content: cell.c.to_string(),
                            position: Point::new(x, y + 1.0),
                            color: cell.fg,
                            size: font_size.into(),
                            font: planeai_iced_spike::font::terminal_font(),
                            ..Default::default()
                        });
                    }
                }
            }
        });
        vec![geom]
    }
}

// --- Main ---

fn title(_state: &App) -> String {
    String::from("PlaneAI Iced Terminal Spike")
}

fn main() -> iced::Result {
    let args = Args::parse();

    // Workflow mode
    if args.planeai_workflow {
        return workflow::run(args);
    }

    // Multi-session mode
    if args.multi_session {
        return multi_session::run(args);
    }

    // Validate: need one of --replay, --shell, or --command
    if args.replay.is_none() && !args.shell && args.command.is_none() {
        eprintln!("Error: one of --replay, --shell, or --command is required");
        std::process::exit(1);
    }

    let cols = args.cols;
    let rows = args.rows;

    // Load terminal font from config (CLI args override)
    {
        let ts = crate::theme::ThemeSource::load();
        let family = args.font_family.as_deref().unwrap_or(&ts.font_family);
        let size = args.font_size.unwrap_or(ts.font_size);
        planeai_iced_spike::font::load(family, size);
    }

    let font_size = planeai_iced_spike::font::terminal_font_size();
    let (cw, ch) = planeai_iced_spike::font::cell_dimensions(font_size);

    ARGS.set(args).unwrap();

    let mut app = iced::application(App::boot, App::update, App::view)
        .title(title)
        .subscription(App::subscription)
        .window_size(Size::new(cols as f32 * cw, rows as f32 * ch));
    let font = planeai_iced_spike::font::terminal_font();
    if font != Font::MONOSPACE {
        app = app.default_font(font);
    }
    app.run()
}
