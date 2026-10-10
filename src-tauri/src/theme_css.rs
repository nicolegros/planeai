//! Resolves the configured theme to the CSS applied in each appearance mode.

use std::path::Path;

use serde::Serialize;

use crate::config::{ThemeChoice, ThemeSource};
use crate::terminal_scheme;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ThemeCss {
    pub light: String,
    pub dark: String,
}

/// A side whose theme cannot be loaded gets empty CSS, so the built-in defaults apply to that side only.
pub fn resolve(choice: &ThemeChoice, themes_dir: &Path) -> ThemeCss {
    let side = |source: &ThemeSource| {
        source_css(source, themes_dir).unwrap_or_else(|e| {
            tracing::warn!("theme: {e}");
            String::new()
        })
    };
    match choice {
        ThemeChoice::Css(name) => {
            let css = side(&ThemeSource::Css { css: name.clone() });
            ThemeCss {
                light: css.clone(),
                dark: css,
            }
        }
        ThemeChoice::PerMode { light, dark } => ThemeCss {
            light: side(light),
            dark: side(dark),
        },
    }
}

fn source_css(source: &ThemeSource, themes_dir: &Path) -> Result<String, String> {
    match source {
        ThemeSource::Css { css } => std::fs::read_to_string(themes_dir.join(format!("{css}.css")))
            .map_err(|e| format!("{css}.css: {e}")),
        ThemeSource::Scheme(name) => {
            terminal_scheme::find_scheme(&themes_dir.join("ghostty"), name)
                .map(|scheme| terminal_scheme::scheme_css(&scheme))
                .map_err(|e| e.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE_CSS: &str = ":root { --color-main: #fafafa; }\n.dark { --color-main: #282c34; }\n";

    fn themes_dir() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("one.css"), ONE_CSS).unwrap();
        dir
    }

    fn css(name: &str) -> ThemeSource {
        ThemeSource::Css {
            css: name.to_string(),
        }
    }

    #[test]
    fn a_css_theme_is_used_unchanged_in_both_modes() {
        let dir = themes_dir();
        let resolved = resolve(&ThemeChoice::Css("one".to_string()), dir.path());
        assert_eq!(
            resolved,
            ThemeCss {
                light: ONE_CSS.to_string(),
                dark: ONE_CSS.to_string()
            }
        );
    }

    #[test]
    fn each_mode_resolves_its_own_source() {
        let dir = themes_dir();
        let resolved = resolve(
            &ThemeChoice::PerMode {
                light: css("one"),
                dark: ThemeSource::Scheme("Dracula".to_string()),
            },
            dir.path(),
        );
        assert_eq!(resolved.light, ONE_CSS);
        assert!(resolved
            .dark
            .starts_with("/* Dracula */\n:root,\n.dark {\n"));
        assert!(resolved
            .dark
            .contains("  --terminal-background: #282a36;\n"));
    }

    #[test]
    fn a_missing_source_falls_back_on_its_side_only() {
        let dir = themes_dir();
        let resolved = resolve(
            &ThemeChoice::PerMode {
                light: css("gone"),
                dark: css("one"),
            },
            dir.path(),
        );
        assert_eq!(
            resolved,
            ThemeCss {
                light: String::new(),
                dark: ONE_CSS.to_string()
            }
        );
    }

    #[test]
    fn user_schemes_are_read_from_the_ghostty_subfolder() {
        let dir = themes_dir();
        std::fs::create_dir(dir.path().join("ghostty")).unwrap();
        std::fs::write(
            dir.path().join("ghostty").join("Mine"),
            "background = #010203\nforeground = #fefefe\n",
        )
        .unwrap();
        let resolved = resolve(
            &ThemeChoice::PerMode {
                light: css("one"),
                dark: ThemeSource::Scheme("Mine".to_string()),
            },
            dir.path(),
        );
        assert!(resolved
            .dark
            .contains("  --terminal-background: #010203;\n"));
    }
}
