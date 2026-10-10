//! Ghostty color schemes and the app theme tokens derived from them.

use std::fmt;
use std::path::Path;
use std::sync::OnceLock;

use include_dir::{include_dir, Dir};
use serde::Serialize;

static BUNDLED: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/resources/ghostty-themes");

const BLACK: Rgb = Rgb([0, 0, 0]);
const WHITE: Rgb = Rgb([255, 255, 255]);

const XTERM_PALETTE: [Rgb; 16] = [
    Rgb([0x00, 0x00, 0x00]),
    Rgb([0xcd, 0x00, 0x00]),
    Rgb([0x00, 0xcd, 0x00]),
    Rgb([0xcd, 0xcd, 0x00]),
    Rgb([0x00, 0x00, 0xee]),
    Rgb([0xcd, 0x00, 0xcd]),
    Rgb([0x00, 0xcd, 0xcd]),
    Rgb([0xe5, 0xe5, 0xe5]),
    Rgb([0x7f, 0x7f, 0x7f]),
    Rgb([0xff, 0x00, 0x00]),
    Rgb([0x00, 0xff, 0x00]),
    Rgb([0xff, 0xff, 0x00]),
    Rgb([0x5c, 0x5c, 0xff]),
    Rgb([0xff, 0x00, 0xff]),
    Rgb([0x00, 0xff, 0xff]),
    Rgb([0xff, 0xff, 0xff]),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub [u8; 3]);

impl Rgb {
    pub fn parse_hex(s: &str) -> Option<Rgb> {
        let hex = s.trim().strip_prefix('#').unwrap_or(s.trim());
        if hex.len() != 6 || !hex.is_ascii() {
            return None;
        }
        let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        Some(Rgb([channel(0)?, channel(2)?, channel(4)?]))
    }

    pub fn hex(self) -> String {
        let [r, g, b] = self.0;
        format!("#{r:02x}{g:02x}{b:02x}")
    }

    pub fn rgba(self, alpha: f64) -> String {
        let [r, g, b] = self.0;
        format!("rgba({r}, {g}, {b}, {alpha})")
    }

    /// Linear interpolation: `t = 0` is `self`, `t = 1` is `other`.
    pub fn mix(self, other: Rgb, t: f64) -> Rgb {
        let channel = |i: usize| {
            let a = f64::from(self.0[i]);
            let b = f64::from(other.0[i]);
            (a + (b - a) * t).round().clamp(0.0, 255.0) as u8
        };
        Rgb([channel(0), channel(1), channel(2)])
    }

    /// WCAG 2 relative luminance.
    pub fn luminance(self) -> f64 {
        let linear = |c: u8| {
            let c = f64::from(c) / 255.0;
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let [r, g, b] = self.0;
        0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
    }

    /// WCAG 2 contrast ratio, from 1 to 21.
    pub fn contrast(self, other: Rgb) -> f64 {
        let (a, b) = (self.luminance(), other.luminance());
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
}

fn prefers_light_text(surface: Rgb) -> bool {
    WHITE.contrast(surface) >= BLACK.contrast(surface)
}

/// Moves `color` toward white on dark surfaces (black on light ones) until it reaches `min` contrast against every surface.
pub fn ensure_contrast_all(color: Rgb, surfaces: &[Rgb], min: f64) -> Rgb {
    let Some(&first) = surfaces.first() else {
        return color;
    };
    let target = if prefers_light_text(first) {
        WHITE
    } else {
        BLACK
    };
    let readable = |c: Rgb| surfaces.iter().all(|&s| c.contrast(s) >= min);
    (0..=20)
        .map(|step| color.mix(target, f64::from(step) * 0.05))
        .find(|&c| readable(c))
        .unwrap_or(target)
}

pub fn ensure_contrast(color: Rgb, against: Rgb, min: f64) -> Rgb {
    ensure_contrast_all(color, &[against], min)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalScheme {
    pub name: String,
    pub background: Rgb,
    pub foreground: Rgb,
    pub cursor: Option<Rgb>,
    pub cursor_text: Option<Rgb>,
    pub selection_background: Option<Rgb>,
    pub selection_foreground: Option<Rgb>,
    pub palette: [Rgb; 16],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemeError {
    InvalidColor { key: String, value: String },
    InvalidPaletteIndex(String),
    Missing(&'static str),
    NotFound(String),
}

impl fmt::Display for SchemeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SchemeError::InvalidColor { key, value } => {
                write!(f, "invalid color for {key}: {value}")
            }
            SchemeError::InvalidPaletteIndex(entry) => write!(f, "invalid palette entry: {entry}"),
            SchemeError::Missing(key) => write!(f, "missing {key}"),
            SchemeError::NotFound(name) => write!(f, "color scheme not found: {name}"),
        }
    }
}

impl std::error::Error for SchemeError {}

impl TerminalScheme {
    /// Parses the Ghostty theme format; keys other than colors used here are ignored.
    pub fn parse(name: &str, source: &str) -> Result<TerminalScheme, SchemeError> {
        let color = |key: &str, value: &str| {
            Rgb::parse_hex(value).ok_or_else(|| SchemeError::InvalidColor {
                key: key.to_string(),
                value: value.to_string(),
            })
        };
        let mut palette = XTERM_PALETTE;
        let (mut background, mut foreground) = (None, None);
        let (mut cursor, mut cursor_text, mut selection, mut selection_fg) =
            (None, None, None, None);
        for line in source.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            match key {
                "palette" => {
                    let (index, hex) = value
                        .split_once('=')
                        .ok_or_else(|| SchemeError::InvalidPaletteIndex(value.to_string()))?;
                    let index: usize = index
                        .trim()
                        .parse()
                        .ok()
                        .filter(|i| *i < 16)
                        .ok_or_else(|| SchemeError::InvalidPaletteIndex(value.to_string()))?;
                    palette[index] = color(key, hex)?;
                }
                "background" => background = Some(color(key, value)?),
                "foreground" => foreground = Some(color(key, value)?),
                "cursor-color" => cursor = Some(color(key, value)?),
                "cursor-text" => cursor_text = Some(color(key, value)?),
                "selection-background" => selection = Some(color(key, value)?),
                "selection-foreground" => selection_fg = Some(color(key, value)?),
                _ => {}
            }
        }
        Ok(TerminalScheme {
            name: name.to_string(),
            background: background.ok_or(SchemeError::Missing("background"))?,
            foreground: foreground.ok_or(SchemeError::Missing("foreground"))?,
            cursor,
            cursor_text,
            selection_background: selection,
            selection_foreground: selection_fg,
            palette,
        })
    }

    /// Polarity follows the surface, not fg vs bg, so text can always reach contrast on it.
    pub fn is_dark(&self) -> bool {
        prefers_light_text(self.background)
    }
}

/// Precomputed colors that the token rules read from.
struct Derived<'a> {
    s: &'a TerminalScheme,
    p: &'a [Rgb; 16],
    dark: bool,
    frame: Rgb,
    t1: Rgb,
    t2: Rgb,
    t3: Rgb,
    selection: Rgb,
}

impl<'a> Derived<'a> {
    fn new(s: &'a TerminalScheme) -> Self {
        let (bg, fg) = (s.background, s.foreground);
        let dark = s.is_dark();
        let frame = bg.mix(BLACK, if dark { 0.175 } else { 0.04 });
        // Text sits on both the frame and the main surface, so it must read on each.
        let surfaces = [frame, bg];
        Derived {
            s,
            p: &s.palette,
            dark,
            frame,
            t1: ensure_contrast_all(fg, &surfaces, 4.5),
            t2: ensure_contrast_all(fg.mix(bg, 0.35), &surfaces, 4.5),
            t3: ensure_contrast_all(fg.mix(bg, 0.55), &surfaces, 3.0),
            selection: s.selection_background.unwrap_or(bg.mix(fg, 0.25)),
        }
    }

    fn bg(&self) -> Rgb {
        self.s.background
    }

    fn fg(&self) -> Rgb {
        self.s.foreground
    }

    fn on_accent(&self) -> Rgb {
        let accent = self.p[4];
        if self.bg().contrast(accent) >= self.fg().contrast(accent) {
            self.bg()
        } else {
            self.fg()
        }
    }
}

type Rule = fn(&Derived) -> String;

static TOKENS: &[(&str, Rule)] = &[
    ("--color-canvas", |d| d.frame.hex()),
    ("--color-chrome", |d| d.frame.hex()),
    ("--color-sidebar", |d| d.frame.hex()),
    ("--color-main", |d| d.bg().hex()),
    ("--color-panel", |d| d.bg().hex()),
    ("--color-panel-hi", |d| d.bg().mix(d.fg(), 0.08).hex()),
    ("--color-t1", |d| d.t1.hex()),
    ("--color-t2", |d| d.t2.hex()),
    ("--color-t3", |d| d.t3.hex()),
    ("--color-border", |d| d.fg().rgba(0.08)),
    ("--color-border-s", |d| d.fg().rgba(0.14)),
    ("--color-accent", |d| d.p[4].hex()),
    ("--color-on-accent", |d| d.on_accent().hex()),
    ("--color-accent-bg", |d| {
        d.p[4].rgba(if d.dark { 0.12 } else { 0.08 })
    }),
    ("--color-scrim", |d| {
        if d.dark {
            BLACK.rgba(0.55)
        } else {
            d.fg().rgba(0.3)
        }
    }),
    ("--color-status-running", |d| d.p[2].hex()),
    ("--color-status-review", |d| d.p[3].hex()),
    ("--color-status-exited", |d| d.p[1].hex()),
    ("--color-status-idle", |d| d.t3.hex()),
    ("--terminal-background", |d| d.bg().hex()),
    ("--terminal-foreground", |d| d.fg().hex()),
    ("--terminal-cursor", |d| d.s.cursor.unwrap_or(d.fg()).hex()),
    ("--terminal-cursor-text", |d| {
        d.s.cursor_text.unwrap_or(d.bg()).hex()
    }),
    ("--terminal-selection", |d| d.selection.hex()),
    ("--terminal-selection-foreground", |d| {
        d.s.selection_foreground.unwrap_or(d.fg()).hex()
    }),
    ("--terminal-black", |d| d.p[0].hex()),
    ("--terminal-red", |d| d.p[1].hex()),
    ("--terminal-green", |d| d.p[2].hex()),
    ("--terminal-yellow", |d| d.p[3].hex()),
    ("--terminal-blue", |d| d.p[4].hex()),
    ("--terminal-magenta", |d| d.p[5].hex()),
    ("--terminal-cyan", |d| d.p[6].hex()),
    ("--terminal-white", |d| d.p[7].hex()),
    ("--terminal-bright-black", |d| d.p[8].hex()),
    ("--terminal-bright-red", |d| d.p[9].hex()),
    ("--terminal-bright-green", |d| d.p[10].hex()),
    ("--terminal-bright-yellow", |d| d.p[11].hex()),
    ("--terminal-bright-blue", |d| d.p[12].hex()),
    ("--terminal-bright-magenta", |d| d.p[13].hex()),
    ("--terminal-bright-cyan", |d| d.p[14].hex()),
    ("--terminal-bright-white", |d| d.p[15].hex()),
    ("--diff-add-bg", |d| d.bg().mix(d.p[2], 0.15).hex()),
    ("--diff-del-bg", |d| d.bg().mix(d.p[1], 0.15).hex()),
    ("--diff-add-color", |d| d.p[2].hex()),
    ("--diff-del-color", |d| d.p[1].hex()),
    ("--editor-background", |d| d.bg().hex()),
    ("--editor-foreground", |d| d.fg().hex()),
    ("--editor-selection", |d| d.selection.hex()),
    ("--editor-line-number", |d| d.t3.hex()),
    ("--editor-added", |d| d.p[2].hex()),
    ("--editor-deleted", |d| d.p[1].hex()),
    ("--editor-added-bg", |d| d.bg().mix(d.p[2], 0.15).hex()),
    ("--editor-deleted-bg", |d| d.bg().mix(d.p[1], 0.15).hex()),
    ("--editor-keyword", |d| d.p[5].hex()),
    ("--editor-string", |d| d.p[2].hex()),
    ("--editor-comment", |d| {
        ensure_contrast(d.p[8], d.bg(), 2.5).hex()
    }),
    ("--editor-number", |d| d.p[3].hex()),
    ("--editor-variable", |d| d.fg().hex()),
    ("--editor-type", |d| d.p[3].hex()),
    ("--editor-function", |d| d.p[4].hex()),
    ("--editor-property", |d| d.p[4].hex()),
    ("--editor-operator", |d| d.p[6].hex()),
    ("--editor-punctuation", |d| d.fg().hex()),
    ("--editor-meta", |d| d.p[5].hex()),
];

pub fn derive_tokens(scheme: &TerminalScheme) -> Vec<(&'static str, String)> {
    let derived = Derived::new(scheme);
    TOKENS
        .iter()
        .map(|(name, rule)| (*name, rule(&derived)))
        .collect()
}

/// Applies in both modes, so it overrides the built-in `.dark` defaults when used for dark mode.
pub fn scheme_css(scheme: &TerminalScheme) -> String {
    let body: String = derive_tokens(scheme)
        .into_iter()
        .map(|(name, value)| format!("  {name}: {value};\n"))
        .collect();
    format!("/* {} */\n:root,\n.dark {{\n{body}}}\n", scheme.name)
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SchemeSummary {
    pub name: String,
    pub dark: bool,
    pub background: String,
    pub foreground: String,
    pub accent: String,
}

impl From<&TerminalScheme> for SchemeSummary {
    fn from(scheme: &TerminalScheme) -> Self {
        SchemeSummary {
            name: scheme.name.clone(),
            dark: scheme.is_dark(),
            background: scheme.background.hex(),
            foreground: scheme.foreground.hex(),
            accent: scheme.palette[4].hex(),
        }
    }
}

/// Provenance files shipped beside the bundled schemes.
const METADATA_FILES: [&str; 3] = ["CREDITS.md", "LICENSE", "UPSTREAM"];

/// Bundled scheme files as `(name, source)`.
fn bundled_sources() -> impl Iterator<Item = (&'static str, &'static str)> {
    BUNDLED.files().filter_map(|file| {
        let name = file.path().file_name()?.to_str()?;
        if METADATA_FILES.contains(&name) {
            return None;
        }
        Some((name, file.contents_utf8()?))
    })
}

fn bundled_schemes() -> &'static [TerminalScheme] {
    static PARSED: OnceLock<Vec<TerminalScheme>> = OnceLock::new();
    PARSED.get_or_init(|| {
        bundled_sources()
            .filter_map(|(name, source)| TerminalScheme::parse(name, source).ok())
            .collect()
    })
}

/// Rejects names that would escape the user schemes folder.
fn is_plain_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && !name.contains(['/', '\\'])
}

fn user_scheme(user_dir: &Path, name: &str) -> Option<Result<TerminalScheme, SchemeError>> {
    if !is_plain_name(name) {
        return None;
    }
    let source = std::fs::read_to_string(user_dir.join(name)).ok()?;
    Some(TerminalScheme::parse(name, &source))
}

/// Looks up a scheme by name; a file in `user_dir` overrides the bundled one.
pub fn find_scheme(user_dir: &Path, name: &str) -> Result<TerminalScheme, SchemeError> {
    if let Some(result) = user_scheme(user_dir, name) {
        return result;
    }
    bundled_schemes()
        .iter()
        .find(|s| s.name == name)
        .cloned()
        .ok_or_else(|| SchemeError::NotFound(name.to_string()))
}

/// Bundled plus user schemes sorted by name; unparseable user files are skipped.
pub fn list_schemes(user_dir: &Path) -> Vec<SchemeSummary> {
    let mut by_name: std::collections::BTreeMap<String, SchemeSummary> = bundled_schemes()
        .iter()
        .map(|s| (s.name.clone(), SchemeSummary::from(s)))
        .collect();
    if let Ok(entries) = std::fs::read_dir(user_dir) {
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if let Some(Ok(scheme)) = user_scheme(user_dir, &name) {
                by_name.insert(name, SchemeSummary::from(&scheme));
            }
        }
    }
    by_name.into_values().collect()
}

#[cfg(test)]
#[path = "terminal_scheme_tests.rs"]
mod tests;
