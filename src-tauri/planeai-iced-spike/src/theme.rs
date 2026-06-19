//! Theme struct and loading logic for the iced app.
//!
//! Loads theme CSS from `~/.config/planeai/themes/{name}.css` and config
//! from `~/.config/planeai/config.json`. Resolves light/dark mode from config.

use iced::{Color, Font};
use std::fs;

use crate::theme_parser::{parse_theme_css, ColorMap, ParsedThemeCss};

// ─── Public types ────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct ColorScale {
    pub s50: Color,
    pub s100: Color,
    pub s200: Color,
    pub s300: Color,
    pub s400: Color,
    pub s500: Color,
    pub s600: Color,
    pub s700: Color,
    pub s800: Color,
    pub s900: Color,
    pub s950: Color,
}

#[derive(Clone, Debug)]
pub struct TerminalColors {
    pub background: Color,
    pub foreground: Color,
    pub cursor: Color,
    pub selection: Color,
    pub black: Color,
    pub red: Color,
    pub green: Color,
    pub yellow: Color,
    pub blue: Color,
    pub magenta: Color,
    pub cyan: Color,
    pub white: Color,
    pub bright_black: Color,
    pub bright_red: Color,
    pub bright_green: Color,
    pub bright_yellow: Color,
    pub bright_blue: Color,
    pub bright_magenta: Color,
    pub bright_cyan: Color,
    pub bright_white: Color,
}

impl Default for TerminalColors {
    fn default() -> Self {
        terminal_from_map(&crate::theme_parser::ColorMap::new())
    }
}

#[derive(Clone, Debug)]
pub struct PlaneAiTheme {
    pub surface: ColorScale,
    pub primary: ColorScale,
    pub error: ColorScale,
    pub warning: ColorScale,
    pub terminal: TerminalColors,
    pub font: Font,
    pub font_size: f32,
    pub mode: Mode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Light,
    Dark,
}

impl PlaneAiTheme {
    /// Background for panels/sidebars — light in light mode, dark in dark mode.
    pub fn panel_bg(&self) -> Color {
        match self.mode {
            Mode::Dark => self.surface.s800,
            Mode::Light => self.surface.s100,
        }
    }

    /// Background for the main content area / status bar.
    pub fn chrome_bg(&self) -> Color {
        match self.mode {
            Mode::Dark => self.surface.s900,
            Mode::Light => self.surface.s50,
        }
    }

    /// Primary body text — dark on light, light on dark.
    pub fn text_primary(&self) -> Color {
        match self.mode {
            Mode::Dark => self.surface.s100,
            Mode::Light => self.surface.s900,
        }
    }

    /// Secondary/muted text.
    pub fn text_muted(&self) -> Color {
        match self.mode {
            Mode::Dark => self.surface.s400,
            Mode::Light => self.surface.s500,
        }
    }

    /// Dimmed/disabled text.
    pub fn text_dimmed(&self) -> Color {
        match self.mode {
            Mode::Dark => self.surface.s500,
            Mode::Light => self.surface.s400,
        }
    }
}

/// Holds both parsed blocks so we can swap mode at runtime without re-reading the file.
pub struct ThemeSource {
    parsed: ParsedThemeCss,
    pub font_family: String,
    pub font_size: f32,
}

impl ThemeSource {
    pub fn load() -> Self {
        let config_dir = planeai_core::session_launch::config_dir();
        let (theme_name, font_family, font_size, _) = read_config(&config_dir);
        let css_path = config_dir.join("themes").join(format!("{theme_name}.css"));
        let css = fs::read_to_string(&css_path).unwrap_or_default();
        let parsed = parse_theme_css(&css);
        Self {
            parsed,
            font_family,
            font_size,
        }
    }

    pub fn resolve(&self, mode: Mode) -> PlaneAiTheme {
        let map = match mode {
            Mode::Light => &self.parsed.light,
            Mode::Dark => {
                if self.parsed.dark.is_empty() {
                    &self.parsed.light
                } else {
                    &self.parsed.dark
                }
            }
        };
        let mut theme = theme_from_map(map);
        theme.font = make_font(&self.font_family);
        theme.font_size = self.font_size;
        theme.mode = mode;
        theme
    }
}

/// Read the configured mode preference from config.json.
pub fn read_mode_preference() -> Option<String> {
    let config_dir = planeai_core::session_launch::config_dir();
    read_config(&config_dir).3
}

// ─── Private helpers ─────────────────────────────────────────────────────────

/// Returns (theme_name, font_family, font_size, mode_preference).
fn read_config(config_dir: &std::path::Path) -> (String, String, f32, Option<String>) {
    let path = config_dir.join("config.json");
    let json = fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok());

    let theme_name = json
        .as_ref()
        .and_then(|j| {
            j.get("appearance")?
                .get("theme")?
                .as_str()
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "default".to_string());

    let font_family = json
        .as_ref()
        .and_then(|j| {
            j.get("terminal")?
                .get("font_family")?
                .as_str()
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| default_font_family().to_string());

    let font_size = json
        .as_ref()
        .and_then(|j| {
            j.get("terminal")?
                .get("font_size")?
                .as_u64()
                .map(|n| n as f32)
        })
        .unwrap_or(14.0);

    let mode_pref = json.as_ref().and_then(|j| {
        j.get("appearance")?
            .get("mode")?
            .as_str()
            .map(|s| s.to_string())
    });

    (theme_name, font_family, font_size, mode_pref)
}

fn default_font_family() -> &'static str {
    if cfg!(windows) {
        "Cascadia Mono"
    } else {
        "Menlo"
    }
}

fn rgb_to_color(rgb: (u8, u8, u8)) -> Color {
    Color::from_rgb8(rgb.0, rgb.1, rgb.2)
}

fn get_color(map: &ColorMap, key: &str, fallback: (u8, u8, u8)) -> Color {
    rgb_to_color(*map.get(key).unwrap_or(&fallback))
}

fn scale_from_map(map: &ColorMap, prefix: &str, fallback: (u8, u8, u8)) -> ColorScale {
    ColorScale {
        s50: get_color(map, &format!("{prefix}-50"), fallback),
        s100: get_color(map, &format!("{prefix}-100"), fallback),
        s200: get_color(map, &format!("{prefix}-200"), fallback),
        s300: get_color(map, &format!("{prefix}-300"), fallback),
        s400: get_color(map, &format!("{prefix}-400"), fallback),
        s500: get_color(map, &format!("{prefix}-500"), fallback),
        s600: get_color(map, &format!("{prefix}-600"), fallback),
        s700: get_color(map, &format!("{prefix}-700"), fallback),
        s800: get_color(map, &format!("{prefix}-800"), fallback),
        s900: get_color(map, &format!("{prefix}-900"), fallback),
        s950: get_color(map, &format!("{prefix}-950"), fallback),
    }
}

pub(crate) fn terminal_from_map(map: &ColorMap) -> TerminalColors {
    TerminalColors {
        background: get_color(map, "terminal-background", (13, 13, 13)),
        foreground: get_color(map, "terminal-foreground", (242, 242, 242)),
        cursor: get_color(map, "terminal-cursor", (88, 166, 255)),
        selection: get_color(map, "terminal-selection", (38, 79, 120)),
        black: get_color(map, "terminal-black", (72, 79, 88)),
        red: get_color(map, "terminal-red", (255, 123, 114)),
        green: get_color(map, "terminal-green", (63, 185, 80)),
        yellow: get_color(map, "terminal-yellow", (210, 153, 34)),
        blue: get_color(map, "terminal-blue", (88, 166, 255)),
        magenta: get_color(map, "terminal-magenta", (188, 140, 255)),
        cyan: get_color(map, "terminal-cyan", (57, 197, 207)),
        white: get_color(map, "terminal-white", (177, 186, 196)),
        bright_black: get_color(map, "terminal-bright-black", (110, 118, 129)),
        bright_red: get_color(map, "terminal-bright-red", (255, 161, 152)),
        bright_green: get_color(map, "terminal-bright-green", (86, 211, 100)),
        bright_yellow: get_color(map, "terminal-bright-yellow", (227, 179, 65)),
        bright_blue: get_color(map, "terminal-bright-blue", (121, 192, 255)),
        bright_magenta: get_color(map, "terminal-bright-magenta", (210, 168, 255)),
        bright_cyan: get_color(map, "terminal-bright-cyan", (86, 212, 221)),
        bright_white: get_color(map, "terminal-bright-white", (240, 246, 252)),
    }
}

fn theme_from_map(map: &ColorMap) -> PlaneAiTheme {
    PlaneAiTheme {
        surface: surface_fallback(map),
        primary: primary_fallback(map),
        error: scale_from_map(map, "color-error", (255, 123, 114)),
        warning: scale_from_map(map, "color-warning", (210, 153, 34)),
        terminal: terminal_from_map(map),
        font: Font::MONOSPACE,
        font_size: 14.0,
        mode: Mode::Dark,
    }
}

/// Create an iced Font from a family name. The leaked &'static str is intentional —
/// iced requires 'static lifetime for font family names, and we only call this once
/// per theme load (not per frame).
fn make_font(family: &str) -> Font {
    Font {
        family: iced::font::Family::Name(Box::leak(family.to_string().into_boxed_str())),
        ..Font::MONOSPACE
    }
}

fn surface_fallback(map: &ColorMap) -> ColorScale {
    ColorScale {
        s50: get_color(map, "color-surface-50", (242, 242, 242)),
        s100: get_color(map, "color-surface-100", (224, 224, 224)),
        s200: get_color(map, "color-surface-200", (191, 191, 191)),
        s300: get_color(map, "color-surface-300", (158, 158, 158)),
        s400: get_color(map, "color-surface-400", (124, 124, 128)),
        s500: get_color(map, "color-surface-500", (93, 93, 96)),
        s600: get_color(map, "color-surface-600", (65, 65, 68)),
        s700: get_color(map, "color-surface-700", (44, 44, 48)),
        s800: get_color(map, "color-surface-800", (32, 32, 35)),
        s900: get_color(map, "color-surface-900", (23, 23, 23)),
        s950: get_color(map, "color-surface-950", (10, 10, 10)),
    }
}

fn primary_fallback(map: &ColorMap) -> ColorScale {
    ColorScale {
        s50: get_color(map, "color-primary-50", (10, 10, 10)),
        s100: get_color(map, "color-primary-100", (26, 26, 26)),
        s200: get_color(map, "color-primary-200", (51, 51, 51)),
        s300: get_color(map, "color-primary-300", (89, 89, 89)),
        s400: get_color(map, "color-primary-400", (128, 128, 128)),
        s500: get_color(map, "color-primary-500", (245, 245, 245)),
        s600: get_color(map, "color-primary-600", (235, 235, 235)),
        s700: get_color(map, "color-primary-700", (224, 224, 224)),
        s800: get_color(map, "color-primary-800", (242, 242, 242)),
        s900: get_color(map, "color-primary-900", (250, 250, 250)),
        s950: get_color(map, "color-primary-950", (255, 255, 255)),
    }
}

/// Default dark theme — used when no config/theme file is found.
pub fn default_dark_theme() -> PlaneAiTheme {
    let source = ThemeSource {
        parsed: ParsedThemeCss::default(),
        font_family: default_font_family().to_string(),
        font_size: 14.0,
    };
    source.resolve(Mode::Dark)
}

/// Detect the current system mode using `dark-light` crate.
pub fn detect_system_mode() -> Mode {
    match dark_light::detect() {
        Ok(dark_light::Mode::Light) => Mode::Light,
        _ => Mode::Dark,
    }
}

/// Resolve the effective mode from the user's config preference.
/// "system" → detect from OS, "light"/"dark" → explicit.
pub fn resolve_mode() -> Mode {
    match read_mode_preference().as_deref() {
        Some("light") => Mode::Light,
        Some("dark") => Mode::Dark,
        _ => detect_system_mode(), // "system" or missing → detect
    }
}
