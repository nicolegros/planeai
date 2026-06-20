//! System font loading via font-kit for Iced.
//!
//! This module handles the one thing Iced can't do on its own: resolving a font
//! family name to actual font bytes so the renderer can use it. Config reading
//! lives in `theme.rs`; this module just loads and caches the bytes.

use font_kit::family_name::FamilyName;
use font_kit::handle::Handle;
use font_kit::properties::Properties;
use font_kit::source::SystemSource;
use iced::Font;
use std::sync::OnceLock;

/// The resolved terminal `Font` (family name reference for Iced).
static TERMINAL_FONT: OnceLock<Font> = OnceLock::new();

/// The raw font bytes (kept alive for Iced's font loader).
static FONT_BYTES: OnceLock<&'static [u8]> = OnceLock::new();

/// The configured terminal font size.
static FONT_SIZE: OnceLock<f32> = OnceLock::new();

/// Load a system font by family name and return its bytes.
/// Returns `None` if the font cannot be found or read.
pub fn load_system_font_bytes(family: &str) -> Option<Vec<u8>> {
    let source = SystemSource::new();
    let handle = source
        .select_best_match(&[FamilyName::Title(family.to_string())], &Properties::new())
        .ok()?;
    match handle {
        Handle::Path {
            path,
            font_index: _,
        } => std::fs::read(path).ok(),
        Handle::Memory { bytes, .. } => Some(bytes.to_vec()),
    }
}

/// Load and register a font by family name and size. Call once at startup.
/// Returns the Iced `Font` to use for rendering.
pub fn load(family: &str, size: f32) -> Font {
    let _ = FONT_SIZE.set(size);

    if let Some(bytes) = load_system_font_bytes(family) {
        let static_bytes: &'static [u8] = Box::leak(bytes.into_boxed_slice());
        let _ = FONT_BYTES.set(static_bytes);

        let font = Font {
            family: iced::font::Family::Name(Box::leak(family.to_string().into_boxed_str())),
            ..Font::MONOSPACE
        };
        let _ = TERMINAL_FONT.set(font);
        font
    } else {
        Font::MONOSPACE
    }
}

/// Get the resolved terminal font. Falls back to `Font::MONOSPACE`.
pub fn terminal_font() -> Font {
    TERMINAL_FONT.get().copied().unwrap_or(Font::MONOSPACE)
}

/// Get the configured terminal font size.
pub fn terminal_font_size() -> f32 {
    FONT_SIZE.get().copied().unwrap_or(14.0)
}

/// Monospace cell dimensions derived from font size.
/// Returns `(cell_width, cell_height)`.
pub fn cell_dimensions(font_size: f32) -> (f32, f32) {
    (
        font_size * crate::common::CELL_WIDTH_RATIO,
        font_size * crate::common::CELL_HEIGHT_RATIO,
    )
}

/// Return an Iced Task that loads the custom font bytes into the renderer.
/// If no custom font was loaded, returns `Task::none()`.
pub fn font_load_task() -> iced::Task<Result<(), iced::font::Error>> {
    match FONT_BYTES.get() {
        Some(bytes) => iced::font::load(*bytes),
        None => iced::Task::none(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_font_defaults_to_monospace() {
        assert_eq!(terminal_font(), Font::MONOSPACE);
    }

    #[test]
    fn load_system_font_bytes_returns_some_for_known_font() {
        if cfg!(target_os = "macos") {
            let bytes = load_system_font_bytes("Menlo");
            assert!(bytes.is_some());
            assert!(!bytes.unwrap().is_empty());
        }
    }

    #[test]
    fn load_system_font_bytes_returns_none_for_unknown_font() {
        let bytes = load_system_font_bytes("NonExistentFont12345XYZ");
        assert!(bytes.is_none());
    }
}
