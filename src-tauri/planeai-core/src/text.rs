/// Strip ANSI escape sequences from raw PTY output, returning plain UTF-8 text.
/// Handles CSI sequences (\x1b[...X), OSC sequences (\x1b]...ST), and simple
/// two-byte escapes (\x1bX).
pub fn strip_ansi(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            match chars.peek() {
                Some('[') => {
                    // CSI sequence: consume until final byte (0x40–0x7E)
                    chars.next(); // consume '['
                    while let Some(&c) = chars.peek() {
                        chars.next();
                        if ('@'..='~').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    // OSC sequence: consume until ST (\x1b\\ or \x07)
                    chars.next(); // consume ']'
                    while let Some(c) = chars.next() {
                        if c == '\x07' {
                            break;
                        }
                        if c == '\x1b' {
                            if chars.peek() == Some(&'\\') {
                                chars.next();
                            }
                            break;
                        }
                    }
                }
                Some(_) => {
                    // Two-byte escape: consume one more char
                    chars.next();
                }
                None => {}
            }
        } else if ch == '\r' {
            // Skip carriage returns (terminal line endings are \r\n)
            continue;
        } else {
            out.push(ch);
        }
    }
    out
}

/// Take the last N lines from text.
pub fn tail_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= n {
        text.to_string()
    } else {
        lines[lines.len() - n..].join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_ansi_removes_csi_sequences() {
        let input = b"\x1b[31mhello\x1b[0m world";
        assert_eq!(strip_ansi(input), "hello world");
    }

    #[test]
    fn strip_ansi_removes_osc_sequences() {
        let input = b"\x1b]0;title\x07some text";
        assert_eq!(strip_ansi(input), "some text");
    }

    #[test]
    fn strip_ansi_removes_carriage_returns() {
        let input = b"line1\r\nline2\r\n";
        assert_eq!(strip_ansi(input), "line1\nline2\n");
    }

    #[test]
    fn strip_ansi_plain_text_unchanged() {
        let input = b"plain text";
        assert_eq!(strip_ansi(input), "plain text");
    }

    #[test]
    fn tail_lines_returns_all_when_fewer_than_n() {
        let text = "a\nb\nc";
        assert_eq!(tail_lines(text, 5), "a\nb\nc");
    }

    #[test]
    fn tail_lines_returns_last_n() {
        let text = "a\nb\nc\nd\ne";
        assert_eq!(tail_lines(text, 2), "d\ne");
    }

    #[test]
    fn tail_lines_exact_count() {
        let text = "a\nb\nc";
        assert_eq!(tail_lines(text, 3), "a\nb\nc");
    }
}
