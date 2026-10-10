use super::*;
use std::collections::HashMap;

fn rgb(hex: &str) -> Rgb {
    Rgb::parse_hex(hex).unwrap()
}

fn bundled(name: &str) -> TerminalScheme {
    bundled_schemes()
        .iter()
        .find(|s| s.name == name)
        .cloned()
        .unwrap()
}

fn tokens(scheme: &TerminalScheme) -> HashMap<&'static str, String> {
    derive_tokens(scheme).into_iter().collect()
}

#[test]
fn parses_a_ghostty_theme() {
    let source = "\
palette = 0=#000000
palette = 1=#110000
palette = 2=#002200
palette = 3=#333300
palette = 4=#000044
palette = 5=#550055
palette = 6=#006666
palette = 7=#777777
palette = 8=#888888
palette = 9=#990000
palette = 10=#00aa00
palette = 11=#bbbb00
palette = 12=#0000cc
palette = 13=#dd00dd
palette = 14=#00eeee
palette = 15=#ffffff
background = #101010
foreground = #f0f0f0
cursor-color = #abcdef
cursor-text = #123456
selection-background = #202020
selection-foreground = #fefefe
";
    let scheme = TerminalScheme::parse("Sample", source).unwrap();
    assert_eq!(
        scheme,
        TerminalScheme {
            name: "Sample".to_string(),
            background: Rgb([0x10, 0x10, 0x10]),
            foreground: Rgb([0xf0, 0xf0, 0xf0]),
            cursor: Some(Rgb([0xab, 0xcd, 0xef])),
            cursor_text: Some(Rgb([0x12, 0x34, 0x56])),
            selection_background: Some(Rgb([0x20, 0x20, 0x20])),
            selection_foreground: Some(Rgb([0xfe, 0xfe, 0xfe])),
            palette: [
                Rgb([0x00, 0x00, 0x00]),
                Rgb([0x11, 0x00, 0x00]),
                Rgb([0x00, 0x22, 0x00]),
                Rgb([0x33, 0x33, 0x00]),
                Rgb([0x00, 0x00, 0x44]),
                Rgb([0x55, 0x00, 0x55]),
                Rgb([0x00, 0x66, 0x66]),
                Rgb([0x77, 0x77, 0x77]),
                Rgb([0x88, 0x88, 0x88]),
                Rgb([0x99, 0x00, 0x00]),
                Rgb([0x00, 0xaa, 0x00]),
                Rgb([0xbb, 0xbb, 0x00]),
                Rgb([0x00, 0x00, 0xcc]),
                Rgb([0xdd, 0x00, 0xdd]),
                Rgb([0x00, 0xee, 0xee]),
                Rgb([0xff, 0xff, 0xff]),
            ],
        }
    );
    assert!(scheme.is_dark());
}

#[test]
fn missing_palette_entries_fall_back_to_xterm_colors() {
    let source = "palette = 1=#ff1111\nbackground = #ffffff\nforeground = #000000\n";
    let scheme = TerminalScheme::parse("Partial", source).unwrap();
    assert_eq!(scheme.palette[1], rgb("#ff1111"));
    assert_eq!(scheme.palette[4], rgb("#0000ee"));
    assert_eq!(scheme.palette[12], rgb("#5c5cff"));
    assert_eq!(scheme.cursor, None);
    assert_eq!(scheme.selection_background, None);
    assert!(!scheme.is_dark());
}

#[test]
fn parse_reports_invalid_colors_and_missing_keys() {
    assert_eq!(
        TerminalScheme::parse("Bad", "background = #zzzzzz\nforeground = #000000"),
        Err(SchemeError::InvalidColor {
            key: "background".to_string(),
            value: "#zzzzzz".to_string()
        })
    );
    assert_eq!(
        TerminalScheme::parse("Bad", "palette = 16=#000000"),
        Err(SchemeError::InvalidPaletteIndex("16=#000000".to_string()))
    );
    assert_eq!(
        TerminalScheme::parse("Bad", "foreground = #000000"),
        Err(SchemeError::Missing("background"))
    );
}

#[test]
fn rgb_helpers() {
    assert_eq!(rgb("#282C34").hex(), "#282c34");
    assert_eq!(Rgb::parse_hex("#12345"), None);
    assert_eq!(BLACK.mix(WHITE, 0.5), Rgb([128, 128, 128]));
    assert_eq!(rgb("#dcdfe4").rgba(0.08), "rgba(220, 223, 228, 0.08)");
    assert!((BLACK.contrast(WHITE) - 21.0).abs() < 1e-9);
    assert!((WHITE.contrast(WHITE) - 1.0).abs() < 1e-9);
}

#[test]
fn ensure_contrast_moves_toward_the_readable_extreme() {
    let dark_bg = rgb("#202020");
    let lifted = ensure_contrast(rgb("#303030"), dark_bg, 4.5);
    assert!(lifted.contrast(dark_bg) >= 4.5);
    assert!(lifted.luminance() > rgb("#303030").luminance());

    let light_bg = rgb("#f0f0f0");
    let darkened = ensure_contrast(rgb("#d0d0d0"), light_bg, 4.5);
    assert!(darkened.contrast(light_bg) >= 4.5);
    assert!(darkened.luminance() < rgb("#d0d0d0").luminance());

    let already = rgb("#ffffff");
    assert_eq!(ensure_contrast(already, dark_bg, 4.5), already);
}

#[test]
fn one_half_dark_terminal_tokens_match_the_ghostty_file() {
    let t = tokens(&bundled("One Half Dark"));
    let expected = [
        ("--terminal-background", "#282c34"),
        ("--terminal-foreground", "#dcdfe4"),
        ("--terminal-cursor", "#a3b3cc"),
        ("--terminal-selection", "#474e5d"),
        ("--terminal-black", "#282c34"),
        ("--terminal-red", "#e06c75"),
        ("--terminal-green", "#98c379"),
        ("--terminal-yellow", "#e5c07b"),
        ("--terminal-blue", "#61afef"),
        ("--terminal-magenta", "#c678dd"),
        ("--terminal-cyan", "#56b6c2"),
        ("--terminal-white", "#dcdfe4"),
        ("--terminal-bright-black", "#5d677a"),
        ("--terminal-bright-red", "#e06c75"),
        ("--terminal-bright-green", "#98c379"),
        ("--terminal-bright-yellow", "#e5c07b"),
        ("--terminal-bright-blue", "#61afef"),
        ("--terminal-bright-magenta", "#c678dd"),
        ("--terminal-bright-cyan", "#56b6c2"),
        ("--terminal-bright-white", "#dcdfe4"),
    ];
    for (name, value) in expected {
        assert_eq!(t[name], value, "{name}");
    }
}

#[test]
fn selection_and_cursor_text_colors_come_from_the_scheme() {
    let t = tokens(&bundled("Gruvbox Light"));
    assert_eq!(t["--terminal-selection"], "#3c3836");
    assert_eq!(t["--terminal-selection-foreground"], "#fbf1c7");
    assert_eq!(t["--terminal-cursor-text"], "#fbf1c7");
}

#[test]
fn one_half_dark_derived_tokens() {
    let t = tokens(&bundled("One Half Dark"));
    // #282c34 * (1 - 0.175)
    assert_eq!(t["--color-canvas"], "#21242b");
    // #282c34 + (#dcdfe4 - #282c34) * 0.08
    assert_eq!(t["--color-panel-hi"], "#363a42");
    // #282c34 + (#98c379 - #282c34) * 0.15
    assert_eq!(t["--diff-add-bg"], "#39433e");
    assert_eq!(t["--color-main"], "#282c34");
    assert_eq!(t["--color-accent"], "#61afef");
    assert_eq!(t["--color-on-accent"], "#282c34");
    assert_eq!(t["--color-accent-bg"], "rgba(97, 175, 239, 0.12)");
    assert_eq!(t["--color-scrim"], "rgba(0, 0, 0, 0.55)");
    assert_eq!(t["--color-border"], "rgba(220, 223, 228, 0.08)");
    assert!(!t.keys().any(|name| name.starts_with("--radius")));
}

#[test]
fn one_half_light_uses_a_light_frame_and_scrim() {
    let t = tokens(&bundled("One Half Light"));
    // #fafafa * 0.96
    assert_eq!(t["--color-canvas"], "#f0f0f0");
    assert_eq!(t["--color-scrim"], "rgba(56, 58, 66, 0.3)");
    assert_eq!(t["--color-accent-bg"], "rgba(1, 132, 188, 0.08)");
}

#[test]
fn every_bundled_scheme_parses_and_derives_readable_text() {
    let names: Vec<&str> = BUNDLED
        .files()
        .filter_map(|f| f.path().file_name()?.to_str())
        .filter(|n| *n != "LICENSE")
        .collect();
    assert!(names.len() > 700, "only {} bundled schemes", names.len());
    for name in names {
        let source = BUNDLED.get_file(name).unwrap().contents_utf8().unwrap();
        let scheme = TerminalScheme::parse(name, source)
            .unwrap_or_else(|e| panic!("{name} failed to parse: {e}"));
        let t = tokens(&scheme);
        let canvas = rgb(&t["--color-canvas"]);
        let main = rgb(&t["--color-main"]);
        for (token, min) in [
            ("--color-t1", 4.5),
            ("--color-t2", 4.5),
            ("--color-t3", 3.0),
        ] {
            let color = rgb(&t[token]);
            for surface in [canvas, main] {
                let ratio = color.contrast(surface);
                assert!(ratio >= min, "{name} {token} contrast {ratio:.2} < {min}");
            }
        }
    }
}

#[test]
fn schemes_css_has_a_light_root_block_and_a_dark_block() {
    let css = schemes_css(&bundled("One Half Light"), &bundled("One Half Dark"));
    let dark_at = css.find(".dark {").unwrap();
    let root_at = css.find(":root {").unwrap();
    assert!(root_at < dark_at);
    assert!(css[root_at..dark_at].contains("--terminal-background: #fafafa;"));
    assert!(css[dark_at..].contains("--terminal-background: #282c34;"));
}

#[test]
fn user_schemes_override_bundled_ones_and_are_listed() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("One Half Dark"),
        "background = #000000\nforeground = #ffffff\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("My Scheme"),
        "background = #ffffff\nforeground = #000000\npalette = 4=#123456\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("Broken"), "background = nope\n").unwrap();

    assert_eq!(
        find_scheme(dir.path(), "One Half Dark").unwrap().background,
        BLACK
    );
    assert_eq!(
        find_scheme(dir.path(), "Missing"),
        Err(SchemeError::NotFound("Missing".to_string()))
    );
    assert!(find_scheme(dir.path(), "../One Half Dark").is_err());

    let list = list_schemes(dir.path());
    assert!(list.windows(2).all(|w| w[0].name < w[1].name));
    assert!(!list
        .iter()
        .any(|s| s.name == "Broken" || s.name == "LICENSE"));
    assert_eq!(
        list.iter().find(|s| s.name == "My Scheme"),
        Some(&SchemeSummary {
            name: "My Scheme".to_string(),
            dark: false,
            background: "#ffffff".to_string(),
            foreground: "#000000".to_string(),
            accent: "#123456".to_string(),
        })
    );
    assert!(
        list.iter()
            .find(|s| s.name == "One Half Dark")
            .unwrap()
            .dark
    );
}
