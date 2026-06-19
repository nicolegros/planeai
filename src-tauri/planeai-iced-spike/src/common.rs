use alacritty_terminal::event::Event;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Cell;
use alacritty_terminal::vte::ansi::Processor;
use iced::widget::canvas::{self, Cache, Program, Text};
use iced::{mouse, Color, Font, Point, Rectangle, Renderer, Size, Theme};
use std::time::Instant;

use crate::theme::TerminalColors;

pub struct EventProxy;
impl alacritty_terminal::event::EventListener for EventProxy {
    fn send_event(&self, _event: Event) {}
}

pub struct TermSize {
    pub cols: usize,
    pub rows: usize,
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

#[derive(Clone)]
pub struct GridCell {
    pub c: char,
    pub fg: Color,
    pub bg: Color,
}

#[derive(Clone)]
pub struct GridSnapshot {
    pub cells: Vec<Vec<GridCell>>,
    pub cursor_line: usize,
    pub cursor_col: usize,
    pub cols: usize,
    pub rows: usize,
}

pub fn ansi_color_to_iced(color: &alacritty_terminal::vte::ansi::Color) -> Color {
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

pub fn snapshot_grid(term: &alacritty_terminal::Term<EventProxy>) -> GridSnapshot {
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

/// Themed variant: resolves ANSI colors using the loaded terminal theme.
pub fn ansi_color_to_iced_themed(
    color: &alacritty_terminal::vte::ansi::Color,
    tc: &TerminalColors,
) -> Color {
    use alacritty_terminal::vte::ansi::Color as AC;
    use alacritty_terminal::vte::ansi::NamedColor;
    match color {
        AC::Named(n) => match n {
            NamedColor::Black => tc.black,
            NamedColor::Red => tc.red,
            NamedColor::Green => tc.green,
            NamedColor::Yellow => tc.yellow,
            NamedColor::Blue => tc.blue,
            NamedColor::Magenta => tc.magenta,
            NamedColor::Cyan => tc.cyan,
            NamedColor::White => tc.white,
            NamedColor::BrightBlack => tc.bright_black,
            NamedColor::BrightRed => tc.bright_red,
            NamedColor::BrightGreen => tc.bright_green,
            NamedColor::BrightYellow => tc.bright_yellow,
            NamedColor::BrightBlue => tc.bright_blue,
            NamedColor::BrightMagenta => tc.bright_magenta,
            NamedColor::BrightCyan => tc.bright_cyan,
            NamedColor::BrightWhite => tc.bright_white,
            NamedColor::Foreground | NamedColor::BrightForeground => tc.foreground,
            NamedColor::Background => tc.background,
            _ => tc.foreground,
        },
        AC::Spec(rgb) => Color::from_rgb8(rgb.r, rgb.g, rgb.b),
        AC::Indexed(idx) => {
            let i = *idx;
            if i < 16 {
                match i {
                    0 => tc.black,
                    1 => tc.red,
                    2 => tc.green,
                    3 => tc.yellow,
                    4 => tc.blue,
                    5 => tc.magenta,
                    6 => tc.cyan,
                    7 => tc.white,
                    8 => tc.bright_black,
                    9 => tc.bright_red,
                    10 => tc.bright_green,
                    11 => tc.bright_yellow,
                    12 => tc.bright_blue,
                    13 => tc.bright_magenta,
                    14 => tc.bright_cyan,
                    15 => tc.bright_white,
                    _ => tc.foreground,
                }
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

/// Themed grid snapshot — uses terminal colors from theme.
pub fn snapshot_grid_themed(
    term: &alacritty_terminal::Term<EventProxy>,
    tc: &TerminalColors,
) -> GridSnapshot {
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
                fg: ansi_color_to_iced_themed(&cell.fg, tc),
                bg: ansi_color_to_iced_themed(&cell.bg, tc),
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

pub fn snapshot_text(term: &alacritty_terminal::Term<EventProxy>) -> String {
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

pub fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((p / 100.0) * (sorted.len() as f64 - 1.0)).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

pub fn get_rss_mb() -> f64 {
    #[cfg(target_os = "macos")]
    unsafe {
        let mut info = std::mem::zeroed::<libc::rusage>();
        if libc::getrusage(libc::RUSAGE_SELF, &mut info) == 0 {
            return info.ru_maxrss as f64 / (1024.0 * 1024.0);
        }
    }
    0.0
}

pub fn ts_ms(start: &Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

pub fn new_term(cols: usize, rows: usize) -> alacritty_terminal::Term<EventProxy> {
    let size = TermSize { cols, rows };
    let config = alacritty_terminal::term::Config::default();
    alacritty_terminal::Term::new(config, &size, EventProxy)
}

pub fn new_processor() -> Processor {
    Processor::new()
}

/// Canvas program for rendering a terminal grid snapshot.
pub struct TermRenderer<'a> {
    pub snapshot: &'a GridSnapshot,
    pub cache: &'a Cache,
    pub font_size: Option<f32>,
}

impl<'a> Program<()> for TermRenderer<'a> {
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
            let font_size = self.font_size.unwrap_or((ch * 0.85).min(16.0));

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

/// Keyboard shortcuts overlay widget.
pub fn shortcuts_overlay<'a, M: 'a>() -> iced::widget::Column<'a, M> {
    use iced::widget::{column, row, text};
    use iced::Length;

    let mod_key = if cfg!(target_os = "macos") {
        "⌘"
    } else {
        "Ctrl+"
    };

    let mut col = column![text("Keyboard Shortcuts")
        .size(16)
        .color(Color::from_rgb8(255, 255, 255))
        .font(Font::MONOSPACE)]
    .spacing(4)
    .padding(20)
    .width(Length::Fixed(320.0));

    // Helper to add a section header
    macro_rules! header {
        ($col:expr, $label:expr) => {
            $col = $col.push(text("").size(8));
            $col = $col.push(
                text($label)
                    .size(12)
                    .color(Color::from_rgb8(100, 200, 255))
                    .font(Font::MONOSPACE),
            );
        };
    }
    macro_rules! shortcut {
        ($col:expr, $key:expr, $desc:expr) => {
            $col = $col.push(row![
                text($key).size(12).color(Color::from_rgb8(200, 200, 200)).font(Font::MONOSPACE).width(Length::Fixed(140.0)),
                text($desc).size(12).color(Color::from_rgb8(160, 160, 160)).font(Font::MONOSPACE),
            ].spacing(8));
        };
    }

    header!(col, "Sessions");
    shortcut!(col, format!("{}N", mod_key), "New...");
    shortcut!(col, format!("{}⇧N", mod_key), "New with custom cmd");
    shortcut!(col, format!("{}B", mod_key), "Worktree launch");
    shortcut!(col, format!("{}A", mod_key), "Attach session");
    shortcut!(col, format!("{}W", mod_key), "Detach session");
    shortcut!(col, format!("{}⇧W", mod_key), "Kill session");
    shortcut!(col, format!("{}1–9", mod_key), "Jump to session");
    shortcut!(col, format!("{}Tab", mod_key), "Next session");
    shortcut!(col, format!("{}⇧Tab", mod_key), "Previous session");
    shortcut!(col, format!("{}R", mod_key), "Refresh daemon list");
    shortcut!(col, format!("{}L", mod_key), "Replay session log");

    header!(col, "Tasks");
    shortcut!(col, format!("{}T", mod_key), "Task picker");
    shortcut!(col, format!("{}⇧T", mod_key), "Clear selected task");
    shortcut!(col, format!("{}↵", mod_key), "Launch selected task");

    header!(col, "Terminal");
    shortcut!(col, format!("{}V", mod_key), "Paste");
    shortcut!(col, "Escape", "Focus sidebar");

    header!(col, "Sidebar");
    shortcut!(col, format!("{}⇧S", mod_key), "Toggle sidebar focus");
    shortcut!(col, "j / ↓", "Move down");
    shortcut!(col, "k / ↑", "Move up");
    shortcut!(col, "h / ←", "Collapse section");
    shortcut!(col, "l / →", "Expand section");
    shortcut!(col, "Enter", "Select / toggle");
    shortcut!(col, "Escape", "Focus terminal");

    header!(col, "View");
    shortcut!(col, format!("{}O", mod_key), "Open project picker");
    shortcut!(col, format!("{}/", mod_key), "Keyboard shortcuts");
    shortcut!(col, "Escape", "Dismiss overlay");

    col
}
