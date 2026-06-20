//! Self-contained terminal component with scrollback support.
//!
//! Encapsulates an alacritty_terminal Term, VTE processor, grid snapshot,
//! canvas cache, and scroll state into a single reusable struct.

use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Cell;
use alacritty_terminal::vte::ansi::Processor;
use iced::widget::canvas::{self, Cache, Program, Text};
use iced::{mouse, Color, Font, Point, Rectangle, Renderer, Size, Theme};

use crate::common::{
    ansi_color_to_iced, EventProxy, GridCell, GridSnapshot, CELL_HEIGHT_RATIO, CELL_WIDTH_RATIO,
};
use crate::theme::TerminalColors;

// ─── Selection ───────────────────────────────────────────────────────────────

/// A (row, col) position in the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridPos {
    pub row: usize,
    pub col: usize,
}

/// Terminal text selection state.
#[derive(Debug, Clone)]
pub struct Selection {
    pub start: GridPos,
    pub end: GridPos,
}

impl Selection {
    /// Returns (start, end) in reading order.
    pub fn ordered(&self) -> (GridPos, GridPos) {
        if self.start.row < self.end.row
            || (self.start.row == self.end.row && self.start.col <= self.end.col)
        {
            (self.start, self.end)
        } else {
            (self.end, self.start)
        }
    }

    /// Returns true if the given cell is within the selection.
    pub fn contains(&self, row: usize, col: usize) -> bool {
        let (s, e) = self.ordered();
        if row < s.row || row > e.row {
            return false;
        }
        if s.row == e.row {
            return col >= s.col && col <= e.col;
        }
        if row == s.row {
            return col >= s.col;
        }
        if row == e.row {
            return col <= e.col;
        }
        true
    }

    /// Extract the selected text from a grid snapshot.
    pub fn text(&self, snapshot: &GridSnapshot) -> String {
        let (s, e) = self.ordered();
        let mut result = String::new();
        for row in s.row..=e.row.min(snapshot.rows.saturating_sub(1)) {
            let row_cells = &snapshot.cells[row];
            let col_start = if row == s.row { s.col } else { 0 };
            let col_end = if row == e.row {
                e.col.min(snapshot.cols.saturating_sub(1))
            } else {
                snapshot.cols.saturating_sub(1)
            };
            for col in col_start..=col_end {
                let c = row_cells[col].c;
                result.push(if c == '\0' { ' ' } else { c });
            }
            // Trim trailing spaces for non-last lines and add newline
            if row != e.row {
                let trimmed = result.trim_end_matches(' ');
                let trimmed_len = trimmed.len();
                result.truncate(trimmed_len);
                result.push('\n');
            }
        }
        result
    }
}

/// Default number of scrollback lines kept in history.
const DEFAULT_SCROLLBACK: usize = 10_000;

// ─── TermSize with scrollback ────────────────────────────────────────────────

/// Terminal dimensions that reports history capacity to alacritty_terminal.
pub struct ScrollbackTermSize {
    pub cols: usize,
    pub rows: usize,
    pub scrollback: usize,
}

impl Dimensions for ScrollbackTermSize {
    fn columns(&self) -> usize {
        self.cols
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn total_lines(&self) -> usize {
        self.rows + self.scrollback
    }
}

// ─── TerminalView ────────────────────────────────────────────────────────────

/// A self-contained terminal view with scrollback.
pub struct TerminalView {
    pub term: alacritty_terminal::Term<EventProxy>,
    pub processor: Processor,
    pub snapshot: GridSnapshot,
    pub cache: Cache,
    pub selection: Option<Selection>,
}

impl TerminalView {
    /// Create a new terminal view with the given visible dimensions and default scrollback.
    pub fn new(cols: usize, rows: usize) -> Self {
        Self::with_scrollback(cols, rows, DEFAULT_SCROLLBACK)
    }

    /// Create a new terminal view with explicit scrollback size.
    pub fn with_scrollback(cols: usize, rows: usize, scrollback: usize) -> Self {
        let size = ScrollbackTermSize {
            cols,
            rows,
            scrollback,
        };
        let config = alacritty_terminal::term::Config::default();
        let term = alacritty_terminal::Term::new(config, &size, EventProxy);
        let snapshot = snapshot_with_offset(&term, &TerminalColors::default());
        Self {
            term,
            processor: Processor::new(),
            snapshot,
            cache: Cache::new(),
            selection: None,
        }
    }

    /// Scroll the terminal display by a delta (positive = scroll up into history).
    pub fn scroll(&mut self, delta: i32) {
        self.term.grid_mut().scroll_display(Scroll::Delta(delta));
    }

    /// Reset scroll position to the bottom (latest output).
    pub fn scroll_to_bottom(&mut self) {
        self.term.grid_mut().scroll_display(Scroll::Bottom);
    }

    /// Returns true if the terminal is scrolled up (not at the bottom).
    pub fn is_scrolled(&self) -> bool {
        self.term.grid().display_offset() != 0
    }

    /// Current display offset (number of lines scrolled up).
    pub fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    /// Take a fresh snapshot of the visible grid (accounting for display_offset).
    pub fn update_snapshot(&mut self, tc: &TerminalColors) {
        self.snapshot = snapshot_with_offset(&self.term, tc);
        self.cache.clear();
    }
}

// ─── Grid snapshot with display_offset support ───────────────────────────────

/// Snapshot the visible portion of the terminal grid, respecting display_offset.
fn snapshot_with_offset(
    term: &alacritty_terminal::Term<EventProxy>,
    tc: &TerminalColors,
) -> GridSnapshot {
    let grid = term.grid();
    let rows = grid.screen_lines();
    let cols = grid.columns();
    let display_offset = grid.display_offset() as i32;
    let cursor = grid.cursor.point;

    let mut cells = Vec::with_capacity(rows);
    for i in 0..rows {
        let line_idx = Line(i as i32 - display_offset);
        let mut row = Vec::with_capacity(cols);
        for j in 0..cols {
            let cell: &Cell = &grid[line_idx][Column(j)];
            row.push(GridCell {
                c: cell.c,
                fg: ansi_color_to_iced(&cell.fg, tc),
                bg: ansi_color_to_iced(&cell.bg, tc),
            });
        }
        cells.push(row);
    }

    // Only show cursor when not scrolled up
    let (cursor_line, cursor_col) = if display_offset == 0 {
        (cursor.line.0 as usize, cursor.column.0)
    } else {
        (usize::MAX, usize::MAX)
    };

    GridSnapshot {
        cells,
        cursor_line,
        cursor_col,
        cols,
        rows,
    }
}

// ─── Canvas renderer ─────────────────────────────────────────────────────────

/// Canvas Program that renders a TerminalView's snapshot.
pub struct TerminalRenderer<'a> {
    pub snapshot: &'a GridSnapshot,
    pub cache: &'a Cache,
    pub background: Color,
    pub cursor_color: Color,
    pub selection_color: Color,
    pub selection: &'a Option<Selection>,
    pub font: Font,
    pub font_size: f32,
}

impl<'a, M> Program<M> for TerminalRenderer<'a> {
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
            let font_size = self.font_size;
            let cw = font_size * CELL_WIDTH_RATIO;
            let ch = font_size * CELL_HEIGHT_RATIO;
            frame.fill_rectangle(Point::ORIGIN, bounds.size(), self.background);

            for (ri, row) in self.snapshot.cells.iter().enumerate() {
                for (ci, cell) in row.iter().enumerate() {
                    let x = ci as f32 * cw;
                    let y = ri as f32 * ch;
                    if cell.bg != self.background {
                        frame.fill_rectangle(Point::new(x, y), Size::new(cw, ch), cell.bg);
                    }
                    // Selection highlight
                    if let Some(sel) = self.selection {
                        if sel.contains(ri, ci) {
                            frame.fill_rectangle(
                                Point::new(x, y),
                                Size::new(cw, ch),
                                self.selection_color,
                            );
                        }
                    }
                    if ri == self.snapshot.cursor_line && ci == self.snapshot.cursor_col {
                        let cursor_with_alpha = Color {
                            a: 0.4,
                            ..self.cursor_color
                        };
                        frame.fill_rectangle(
                            Point::new(x, y),
                            Size::new(cw, ch),
                            cursor_with_alpha,
                        );
                    }
                    if cell.c != ' ' && cell.c != '\0' {
                        frame.fill_text(Text {
                            content: cell.c.to_string(),
                            position: Point::new(x, y + 1.0),
                            color: cell.fg,
                            size: font_size.into(),
                            font: self.font,
                            ..Default::default()
                        });
                    }
                }
            }
        });
        vec![geom]
    }
}
