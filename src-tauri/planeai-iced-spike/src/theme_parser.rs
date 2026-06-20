//! Minimal CSS custom-property parser for planeai theme files.
//!
//! Extracts `--variable: value;` declarations from `:root {}` and `.dark {}` blocks,
//! converting `hsl()` and `#hex` color values to (r, g, b) tuples.

use std::collections::HashMap;

/// Parsed output: variable name → (r, g, b)
pub type ColorMap = HashMap<String, (u8, u8, u8)>;

/// Result of parsing a theme CSS file.
#[derive(Debug, Default)]
pub struct ParsedThemeCss {
    pub light: ColorMap,
    pub dark: ColorMap,
}

/// Parse a theme CSS string into light (`:root`) and dark (`.dark`) color maps.
pub fn parse_theme_css(css: &str) -> ParsedThemeCss {
    let mut result = ParsedThemeCss::default();

    #[derive(Clone, Copy)]
    enum Block {
        Light,
        Dark,
    }

    let mut active: Option<Block> = None;
    let mut depth = 0u32;

    for line in css.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("/*") || trimmed.starts_with("//") {
            continue;
        }

        // Detect block entry
        let entering = if trimmed.starts_with(":root") {
            Some(Block::Light)
        } else if trimmed.starts_with(".dark") {
            Some(Block::Dark)
        } else {
            None
        };

        if let Some(block) = entering {
            active = Some(block);
            if trimmed.contains('{') {
                depth += 1;
            }
            if let Some(after) = trimmed.split('{').nth(1) {
                let after = after.trim().trim_end_matches('}');
                let map = match block {
                    Block::Light => &mut result.light,
                    Block::Dark => &mut result.dark,
                };
                if let Some((name, rgb)) = parse_variable_line(after) {
                    map.insert(name, rgb);
                }
                if trimmed.contains('}') {
                    depth = 0;
                    active = None;
                }
            }
            continue;
        }

        if trimmed.contains('{') && active.is_some() {
            depth += 1;
            continue;
        }
        if trimmed.contains('}') {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                active = None;
            }
            continue;
        }

        if let Some(block) = active {
            let map = match block {
                Block::Light => &mut result.light,
                Block::Dark => &mut result.dark,
            };
            if let Some((name, rgb)) = parse_variable_line(trimmed) {
                map.insert(name, rgb);
            }
        }
    }

    result
}

fn parse_variable_line(line: &str) -> Option<(String, (u8, u8, u8))> {
    let line = line.trim();
    if !line.starts_with("--") {
        return None;
    }
    let colon = line.find(':')?;
    let name = line[2..colon].trim().to_string();
    let value = line[colon + 1..].trim().trim_end_matches(';').trim();

    let rgb = if value.starts_with('#') {
        parse_hex(value)?
    } else if value.starts_with("hsl(") {
        parse_hsl(value)?
    } else {
        return None;
    };

    Some((name, rgb))
}

/// Parse a hex color string like `#ff7b72` into (r, g, b).
pub fn parse_hex(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.strip_prefix('#')?;
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some((r, g, b))
}

/// Parse an HSL value like `hsl(240 5% 96%)` into (r, g, b).
pub fn parse_hsl(s: &str) -> Option<(u8, u8, u8)> {
    let inner = s.strip_prefix("hsl(")?.strip_suffix(')')?;
    let parts: Vec<&str> = inner.split_whitespace().collect();
    if parts.len() != 3 {
        return None;
    }
    let h: f64 = parts[0].parse().ok()?;
    let s_pct: f64 = parts[1].strip_suffix('%')?.parse().ok()?;
    let l_pct: f64 = parts[2].strip_suffix('%')?.parse().ok()?;

    let (r, g, b) = hsl_to_rgb(h, s_pct / 100.0, l_pct / 100.0);
    Some((
        (r * 255.0).round() as u8,
        (g * 255.0).round() as u8,
        (b * 255.0).round() as u8,
    ))
}

fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (f64, f64, f64) {
    if s == 0.0 {
        return (l, l, l);
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let h_norm = h / 360.0;
    (
        hue_to_rgb(p, q, h_norm + 1.0 / 3.0),
        hue_to_rgb(p, q, h_norm),
        hue_to_rgb(p, q, h_norm - 1.0 / 3.0),
    )
}

fn hue_to_rgb(p: f64, q: f64, mut t: f64) -> f64 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        return p + (q - p) * 6.0 * t;
    }
    if t < 1.0 / 2.0 {
        return q;
    }
    if t < 2.0 / 3.0 {
        return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── parse_hex ───────────────────────────────────────────────────────────

    #[test]
    fn hex_parses_6_digit() {
        assert_eq!(parse_hex("#ff7b72"), Some((255, 123, 114)));
    }

    #[test]
    fn hex_parses_uppercase() {
        assert_eq!(parse_hex("#FF7B72"), Some((255, 123, 114)));
    }

    #[test]
    fn hex_rejects_invalid() {
        assert_eq!(parse_hex("not-a-color"), None);
        assert_eq!(parse_hex("#gg0000"), None);
        assert_eq!(parse_hex("#fff"), None); // 3-digit not supported
    }

    // ─── parse_hsl ───────────────────────────────────────────────────────────

    #[test]
    fn hsl_parses_achromatic_white() {
        // hsl(0 0% 100%) → white
        assert_eq!(parse_hsl("hsl(0 0% 100%)"), Some((255, 255, 255)));
    }

    #[test]
    fn hsl_parses_achromatic_black() {
        assert_eq!(parse_hsl("hsl(0 0% 0%)"), Some((0, 0, 0)));
    }

    #[test]
    fn hsl_parses_chromatic() {
        // hsl(240 5% 96%) → should be near-white with slight blue
        let (r, g, b) = parse_hsl("hsl(240 5% 96%)").unwrap();
        // Expected: ~244, 244, 245 (very light grayish blue)
        assert!((r as i16 - 244).abs() <= 1);
        assert!((g as i16 - 244).abs() <= 1);
        assert!((b as i16 - 245).abs() <= 1);
    }

    #[test]
    fn hsl_parses_saturated_red() {
        // hsl(347 77% 50%) → vivid red-pink
        let (r, g, b) = parse_hsl("hsl(347 77% 50%)").unwrap();
        assert!(r > 200); // definitely red-heavy
        assert!(g < 50);
        assert!(b < 100);
    }

    #[test]
    fn hsl_rejects_invalid() {
        assert_eq!(parse_hsl("rgb(255, 0, 0)"), None);
        assert_eq!(parse_hsl("not-hsl"), None);
    }

    // ─── parse_theme_css ─────────────────────────────────────────────────────

    #[test]
    fn parses_root_hex_variables() {
        let css = r#"
:root {
  --terminal-background: #ffffff;
  --terminal-foreground: #171717;
}
"#;
        let parsed = parse_theme_css(css);
        assert_eq!(
            parsed.light.get("terminal-background"),
            Some(&(255, 255, 255))
        );
        assert_eq!(parsed.light.get("terminal-foreground"), Some(&(23, 23, 23)));
        assert!(parsed.dark.is_empty());
    }

    #[test]
    fn parses_dark_block() {
        let css = r#"
:root {
  --terminal-background: #ffffff;
}
.dark {
  --terminal-background: #0d0d0d;
}
"#;
        let parsed = parse_theme_css(css);
        assert_eq!(
            parsed.light.get("terminal-background"),
            Some(&(255, 255, 255))
        );
        assert_eq!(parsed.dark.get("terminal-background"), Some(&(13, 13, 13)));
    }

    #[test]
    fn parses_hsl_variables() {
        let css = r#"
:root {
  --color-surface-50: hsl(0 0% 100%);
  --color-surface-950: hsl(0 0% 5%);
}
"#;
        let parsed = parse_theme_css(css);
        assert_eq!(parsed.light.get("color-surface-50"), Some(&(255, 255, 255)));
        assert_eq!(parsed.light.get("color-surface-950"), Some(&(13, 13, 13)));
    }

    #[test]
    fn ignores_comments_and_non_variable_lines() {
        let css = r#"
/* this is a comment */
:root {
  /* Surface colors */
  --terminal-red: #cf222e;
  font-family: sans-serif;
}
"#;
        let parsed = parse_theme_css(css);
        assert_eq!(parsed.light.get("terminal-red"), Some(&(207, 34, 46)));
        assert_eq!(parsed.light.len(), 1); // only the variable, not font-family
    }

    #[test]
    fn parses_full_theme_file() {
        let css = include_str!("../../resources/themes/default.css");
        let parsed = parse_theme_css(css);

        // Spot-check light terminal colors
        assert_eq!(
            parsed.light.get("terminal-background"),
            Some(&(255, 255, 255))
        );
        assert_eq!(parsed.light.get("terminal-foreground"), Some(&(23, 23, 23)));
        assert_eq!(parsed.light.get("terminal-red"), Some(&(207, 34, 46)));

        // Spot-check dark terminal colors
        assert_eq!(parsed.dark.get("terminal-background"), Some(&(13, 13, 13)));
        assert_eq!(
            parsed.dark.get("terminal-foreground"),
            Some(&(242, 242, 242))
        );
        assert_eq!(parsed.dark.get("terminal-red"), Some(&(255, 123, 114)));

        // Verify both blocks have all terminal colors
        assert!(parsed.light.contains_key("terminal-cursor"));
        assert!(parsed.dark.contains_key("terminal-cursor"));

        // HSL surface colors present
        assert!(parsed.light.contains_key("color-surface-500"));
        assert!(parsed.dark.contains_key("color-surface-500"));
    }

    #[test]
    fn strips_variable_prefix() {
        // Variable names should have the leading `--` stripped
        let css = ":root { --terminal-red: #cf222e; }";
        let parsed = parse_theme_css(css);
        assert!(parsed.light.contains_key("terminal-red"));
        assert!(!parsed.light.contains_key("--terminal-red"));
    }
}
