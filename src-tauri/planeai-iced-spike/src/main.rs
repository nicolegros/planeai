#![recursion_limit = "256"]

mod shell;

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
use clap::Parser as ClapParser;
use iced::keyboard;
use iced::mouse;
use iced::widget::canvas::{self, Cache, Program, Text};
use iced::widget::Canvas;
use iced::{Color, Element, Font, Length, Point, Rectangle, Renderer, Size, Subscription, Theme};
use serde_json::json;

// --- CLI (contract-required flags) ---

#[derive(ClapParser, Debug, Clone)]
struct Args {
    /// Path to raw ANSI fixture file
    #[arg(long, required_unless_present = "shell")]
    replay: Option<PathBuf>,
    /// Launch a live local shell instead of replaying a fixture
    #[arg(long)]
    shell: bool,
    /// Terminal columns
    #[arg(long, default_value_t = 120)]
    cols: usize,
    /// Terminal rows
    #[arg(long, default_value_t = 40)]
    rows: usize,
    /// Bytes fed per tick
    #[arg(long, default_value_t = 16384)]
    chunk_size: usize,
    /// Milliseconds between chunks (0 = maxspeed)
    #[arg(long, default_value_t = 4)]
    chunk_interval_ms: u64,
    /// JSONL output path
    #[arg(long)]
    metrics: Option<PathBuf>,
    /// Backend identifier
    #[arg(long, default_value = "iced-alacritty")]
    backend: String,
    /// Exit after replay completes
    #[arg(long)]
    exit_when_done: bool,
    /// Write visible text snapshot after replay
    #[arg(long)]
    snapshot: Option<PathBuf>,
    // --- optional flags ---
    #[arg(long)]
    font_size: Option<f32>,
    #[arg(long)]
    scrollback_lines: Option<usize>,
    #[arg(long)]
    max_runtime_ms: Option<u64>,
    #[arg(long)]
    warmup_ms: Option<u64>,
}

static ARGS: OnceLock<Args> = OnceLock::new();

// --- Helpers ---

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
    fn columns(&self) -> usize { self.cols }
    fn screen_lines(&self) -> usize { self.rows }
    fn total_lines(&self) -> usize { self.rows }
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
                    (0,0,0),(205,49,49),(13,188,121),(229,229,16),
                    (36,114,200),(188,63,188),(17,168,205),(229,229,229),
                    (102,102,102),(241,76,76),(35,209,139),(245,245,67),
                    (59,142,234),(214,112,214),(41,184,219),(255,255,255),
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
struct GridCell { c: char, fg: Color, bg: Color }

#[derive(Clone)]
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
        // trim trailing spaces per line
        let trimmed = out.trim_end_matches(' ');
        out.truncate(trimmed.len());
        out.push('\n');
    }
    out
}

// --- Keyboard input translation ---

fn key_to_bytes(key: &keyboard::Key, modifiers: &keyboard::Modifiers) -> Vec<u8> {
    use keyboard::key::Named;
    use keyboard::Key;

    match key {
        Key::Character(c) => {
            let ch = c.as_str();
            if modifiers.control() {
                // Ctrl+letter → control code
                if let Some(b) = ch.bytes().next() {
                    let ctrl = match b {
                        b'a'..=b'z' => b - b'a' + 1,
                        b'A'..=b'Z' => b - b'A' + 1,
                        b'[' => 27,
                        b'\\' => 28,
                        b']' => 29,
                        b'^' => 30,
                        b'_' => 31,
                        _ => return ch.as_bytes().to_vec(),
                    };
                    return vec![ctrl];
                }
            }
            ch.as_bytes().to_vec()
        }
        Key::Named(named) => match named {
            Named::Enter => b"\r".to_vec(),
            Named::Backspace => b"\x7f".to_vec(),
            Named::Tab => b"\t".to_vec(),
            Named::Escape => b"\x1b".to_vec(),
            Named::ArrowUp => b"\x1b[A".to_vec(),
            Named::ArrowDown => b"\x1b[B".to_vec(),
            Named::ArrowRight => b"\x1b[C".to_vec(),
            Named::ArrowLeft => b"\x1b[D".to_vec(),
            Named::Home => b"\x1b[H".to_vec(),
            Named::End => b"\x1b[F".to_vec(),
            Named::PageUp => b"\x1b[5~".to_vec(),
            Named::PageDown => b"\x1b[6~".to_vec(),
            Named::Delete => b"\x1b[3~".to_vec(),
            Named::Insert => b"\x1b[2~".to_vec(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

// --- Iced App State ---

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
    // collected timing vectors
    frame_deltas: Vec<f64>,
    render_works: Vec<f64>,
    parse_times: Vec<f64>,
    max_pending_unparsed: usize,
    // metrics output lines (buffered as JSON values)
    metrics_lines: Vec<serde_json::Value>,
    // shell mode
    pty: Option<shell::Shell>,
}

#[derive(Debug, Clone)]
enum Message {
    Tick,
    KeyEvent(keyboard::Event),
    PtyPoll,
}

impl App {
    fn boot() -> (Self, iced::Task<Message>) {
        let args = ARGS.get().unwrap();
        let data = if let Some(ref path) = args.replay {
            fs::read(path).expect("Failed to read replay file")
        } else {
            Vec::new()
        };
        let scrollback = args.scrollback_lines.unwrap_or(0);
        let size = TermSize { cols: args.cols, rows: args.rows + scrollback };
        let config = alacritty_terminal::term::Config::default();
        let term = alacritty_terminal::Term::new(config, &size, EventProxy);
        let processor = Processor::new();
        let snapshot = snapshot_grid(&term);
        let pty = if args.shell {
            Some(shell::Shell::spawn(args.cols as u16, args.rows as u16))
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
            },
            iced::Task::none(),
        )
    }

    fn common_fields(&self) -> serde_json::Value {
        let args = ARGS.get().unwrap();
        let fixture = args.replay.as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "<shell>".to_string());
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
        map.insert("timestamp_ms".into(), json!(ts_ms(&self.replay_start.unwrap_or(self.boot_instant))));
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
            Message::KeyEvent(keyboard::Event::KeyPressed { key, modifiers, text, .. }) => {
                // Use text field for normal typing, key_to_bytes for control sequences
                let bytes = if modifiers.control() {
                    key_to_bytes(&key, &modifiers)
                } else if let Some(ref t) = text {
                    t.as_bytes().to_vec()
                } else {
                    key_to_bytes(&key, &modifiers)
                };
                if !bytes.is_empty() {
                    if let Some(ref pty) = self.pty {
                        pty.write(&bytes);
                    }
                }
            }
            Message::KeyEvent(_) => {}
            Message::PtyPoll => {
                if let Some(ref pty) = self.pty {
                    let output = pty.drain();
                    if !output.is_empty() {
                        self.processor.advance(&mut self.term, &output);
                        self.snapshot = snapshot_grid(&self.term);
                        self.cache.clear();
                    }
                }
            }
            Message::Tick => {
                if self.done {
                    return;
                }
                let args = ARGS.get().unwrap();

                // First tick: emit replay_start
                if self.replay_start.is_none() {
                    self.replay_start = Some(Instant::now());
                    self.emit("replay_start", json!({
                        "bytes_total": 0,
                        "queue_depth_bytes": self.data.len(),
                    }));
                }

                let now = Instant::now();
                let frame_delta_ms = self.last_frame_instant
                    .map(|prev| now.duration_since(prev).as_secs_f64() * 1000.0);
                self.last_frame_instant = Some(now);
                if let Some(fd) = frame_delta_ms {
                    self.frame_deltas.push(fd);
                }

                let end = (self.offset + args.chunk_size).min(self.data.len());
                let chunk = self.data[self.offset..end].to_vec();
                let bytes_fed = end - self.offset;

                // chunk_sent
                self.emit("chunk_sent", json!({
                    "bytes_total": self.offset + bytes_fed,
                    "bytes_since_last_event": bytes_fed,
                    "queue_depth_bytes": self.data.len() - end,
                }));

                // Track max pending unparsed: this is the chunk size about to be parsed
                // (in synchronous replay, only the current chunk is "queued but unparsed")
                if bytes_fed > self.max_pending_unparsed {
                    self.max_pending_unparsed = bytes_fed;
                }

                // parse_batch
                let parse_start = Instant::now();
                self.processor.advance(&mut self.term, &chunk);
                let parse_time_ms = parse_start.elapsed().as_secs_f64() * 1000.0;
                self.parse_times.push(parse_time_ms);
                self.offset = end;

                self.emit("parse_batch", json!({
                    "bytes_total": self.offset,
                    "bytes_since_last_event": bytes_fed,
                    "parse_time_ms": parse_time_ms,
                    "queue_depth_bytes": self.data.len() - self.offset,
                }));

                // render_frame
                let render_start = Instant::now();
                self.snapshot = snapshot_grid(&self.term);
                self.cache.clear();
                let render_work_ms = render_start.elapsed().as_secs_f64() * 1000.0;
                self.render_works.push(render_work_ms);
                self.frames += 1;

                self.emit("render_frame", json!({
                    "bytes_total": self.offset,
                    "render_work_ms": render_work_ms,
                    "frame_delta_ms": frame_delta_ms,
                    "queue_depth_bytes": self.data.len() - self.offset,
                    "rss_mb": get_rss_mb(),
                }));

                // frame_sample (periodic, every 10 frames)
                if self.frames % 10 == 0 {
                    self.emit("frame_sample", json!({
                        "bytes_total": self.offset,
                        "queue_depth_bytes": self.data.len() - self.offset,
                        "rss_mb": get_rss_mb(),
                        "frame_delta_ms": frame_delta_ms,
                        "render_work_ms": render_work_ms,
                        "parse_time_ms": parse_time_ms,
                    }));
                }

                // backlog_sample (periodic, every 20 frames)
                if self.frames % 20 == 0 {
                    self.emit("backlog_sample", json!({
                        "bytes_total": self.offset,
                        "queue_depth_bytes": self.data.len() - self.offset,
                    }));
                }

                // Check max runtime
                if let Some(max_ms) = args.max_runtime_ms {
                    if ts_ms(&self.replay_start.unwrap()) > max_ms as f64 {
                        self.done = true;
                    }
                }

                if self.offset >= self.data.len() {
                    self.done = true;
                }

                if self.done {
                    self.finish();
                }
            }
        }
    }

    fn finish(&mut self) {
        let args = ARGS.get().unwrap();
        let replay_start = self.replay_start.unwrap();
        let wall_time_ms = ts_ms(&replay_start);

        // replay_done event
        self.emit("replay_done", json!({
            "bytes_total": self.offset,
            "queue_depth_bytes": self.data.len() - self.offset,
            "wall_time_ms": wall_time_ms,
        }));

        // Write snapshot if requested
        if let Some(snap_path) = &args.snapshot {
            if let Some(parent) = snap_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let text = snapshot_text(&self.term);
            fs::write(snap_path, text).expect("Failed to write snapshot");
        }

        // Compute summary
        let total_bytes = self.data.len();
        let total_chunks = self.frames;
        let replay_mode = if args.chunk_interval_ms == 0 { "maxspeed" } else { "realtime" };
        let expected_min_replay_time_ms =
            ((total_bytes as f64 / args.chunk_size as f64).ceil() as u64)
                * args.chunk_interval_ms;
        let avg_mb_s = if wall_time_ms > 0.0 {
            (total_bytes as f64 / 1_048_576.0) / (wall_time_ms / 1000.0)
        } else { 0.0 };

        let mut fd = self.frame_deltas.clone();
        fd.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut rw = self.render_works.clone();
        rw.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut pt = self.parse_times.clone();
        pt.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let startup_time_ms = replay_start.duration_since(self.boot_instant).as_secs_f64() * 1000.0;

        let snapshot_path = args.snapshot.as_ref().map(|p| p.to_string_lossy().to_string());

        let summary = json!({
            "schema_version": 1,
            "event_type": "summary",
            "backend": args.backend,
            "fixture": args.replay.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
            "cols": args.cols,
            "rows": args.rows,
            "chunk_size": args.chunk_size,
            "chunk_interval_ms": args.chunk_interval_ms,
            "replay_mode": replay_mode,
            "total_bytes": total_bytes,
            "total_chunks": total_chunks,
            "actual_chunk_count": total_chunks,
            "average_chunk_size": if total_chunks > 0 { total_bytes as f64 / total_chunks as f64 } else { 0.0 },
            "expected_min_replay_time_ms": expected_min_replay_time_ms,
            "wall_time_ms": wall_time_ms,
            "producer_time_ms": wall_time_ms,
            "drain_time_ms": 0.0,
            "total_replay_time_ms": wall_time_ms,
            "average_mb_per_sec": avg_mb_s,
            "p50_frame_delta_ms": percentile(&fd, 50.0),
            "p95_frame_delta_ms": percentile(&fd, 95.0),
            "p99_frame_delta_ms": percentile(&fd, 99.0),
            "p50_render_work_ms": percentile(&rw, 50.0),
            "p95_render_work_ms": percentile(&rw, 95.0),
            "p99_render_work_ms": percentile(&rw, 99.0),
            "p50_parse_time_ms": percentile(&pt, 50.0),
            "p95_parse_time_ms": percentile(&pt, 95.0),
            "p99_parse_time_ms": percentile(&pt, 99.0),
            "p50_write_latency_ms": null,
            "p95_write_latency_ms": null,
            "p99_write_latency_ms": null,
            "frames_over_16_7ms": fd.iter().filter(|&&t| t > 16.7).count(),
            "frames_over_33_3ms": fd.iter().filter(|&&t| t > 33.3).count(),
            "frames_over_50ms": fd.iter().filter(|&&t| t > 50.0).count(),
            "max_queue_depth_bytes": self.max_pending_unparsed,
            "queue_depth_at_end_bytes": self.data.len() - self.offset,
            "max_pending_unparsed_bytes": self.max_pending_unparsed,
            "max_pending_unrendered_bytes": 0,
            "startup_time_ms": startup_time_ms,
            "final_rss_mb": get_rss_mb(),
            "final_js_heap_mb": null,
            "snapshot_path": snapshot_path,
        });
        self.metrics_lines.push(summary);

        // Write metrics file
        if let Some(path) = &args.metrics {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let mut file = fs::File::create(path).expect("Failed to create metrics file");
            for line in &self.metrics_lines {
                writeln!(file, "{}", serde_json::to_string(line).unwrap()).unwrap();
            }
        }

        if args.exit_when_done {
            std::process::exit(0);
        }
    }

    fn view(&self) -> Element<'_, Message> {
        Canvas::new(TermRenderer { snapshot: &self.snapshot, cache: &self.cache })
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut subs = Vec::new();

        // Keyboard input (for shell mode)
        if self.pty.is_some() {
            subs.push(keyboard::listen().map(Message::KeyEvent));
            subs.push(
                iced::time::every(Duration::from_millis(16)).map(|_| Message::PtyPoll)
            );
        }

        // Replay tick
        if !self.done && !self.data.is_empty() {
            let interval = ARGS.get().unwrap().chunk_interval_ms;
            let ms = if interval == 0 { 1 } else { interval };
            subs.push(
                iced::time::every(Duration::from_millis(ms)).map(|_| Message::Tick)
            );
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
            let cw = bounds.width / self.snapshot.cols as f32;
            let ch = bounds.height / self.snapshot.rows as f32;
            let font_size = ARGS.get()
                .and_then(|a| a.font_size)
                .unwrap_or((ch * 0.85).min(16.0));

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
                            Point::new(x, y), Size::new(cw, ch),
                            Color::from_rgba8(200, 200, 200, 0.4),
                        );
                    }
                    if cell.c != ' ' && cell.c != '\0' {
                        frame.fill_text(Text {
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

// --- Main ---

fn title(_state: &App) -> String {
    String::from("PlaneAI Iced Terminal Spike")
}

fn main() -> iced::Result {
    let args = Args::parse();
    let cols = args.cols;
    let rows = args.rows;
    ARGS.set(args).unwrap();

    iced::application(App::boot, App::update, App::view)
        .title(title)
        .subscription(App::subscription)
        .window_size(Size::new(cols as f32 * 9.0, rows as f32 * 18.0))
        .run()
}
