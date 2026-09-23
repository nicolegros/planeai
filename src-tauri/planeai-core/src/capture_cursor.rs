//! Capture-based incremental reads shared by the tmux and rmux backends.
//!
//! Neither backend exposes a monotonic byte offset the way the daemon's ring
//! buffer does, so an incremental read works from a captured snapshot of the
//! pane instead. The cursor is `<label>:<line_count>:<hash>`: the line count says
//! how much was already delivered, and the hash of the first and last lines
//! detects that the region the cursor referred to has scrolled off or been reset.
//!
//! For rmux this is the only option available: `PaneRecoveryOptions` has no
//! "resume at sequence N" input, so a fresh process cannot ask the daemon for the
//! bytes since a stored position (ADR-0012, probe 3).

/// Result of an incremental read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorRead {
    /// Output produced since the cursor.
    pub text: String,
    /// Cursor to pass to the next read.
    pub cursor: String,
    /// Whether content was lost between the cursor and the earliest available line.
    pub truncated: bool,
}

/// Build a cursor describing `lines` as already delivered.
pub fn build_cursor(label: &str, lines: &[&str]) -> String {
    format!("{label}:{}:{}", lines.len(), hash_lines(lines))
}

/// Parse a cursor, rejecting one issued for a different backend.
///
/// Surrounding double quotes are accepted: TOON quotes any value containing a
/// colon, so a cursor copied straight out of `axi session read` output arrives
/// quoted. Rejecting it would make the documented polling loop unusable.
pub fn parse_cursor(label: &str, cursor: &str) -> Result<(usize, u64), String> {
    let cursor = cursor.trim().trim_matches('"');
    let parts: Vec<&str> = cursor.splitn(3, ':').collect();
    if parts.len() != 3 || parts[0] != label {
        return Err(format!("invalid {label} cursor: {cursor}"));
    }
    let line_count: usize = parts[1]
        .parse()
        .map_err(|_| format!("invalid cursor line count: {}", parts[1]))?;
    let hash: u64 = parts[2]
        .parse()
        .map_err(|_| format!("invalid cursor hash: {}", parts[2]))?;
    Ok((line_count, hash))
}

/// Return the output in `captured` that follows `cursor`.
///
/// `max_bytes` of 0 means unlimited. When a cap applies, the cursor advances only
/// over the lines actually returned, so the remainder arrives on the next poll.
pub fn read_after(
    label: &str,
    captured: &str,
    cursor: &str,
    max_bytes: usize,
) -> Result<CursorRead, String> {
    let all_lines: Vec<&str> = captured.lines().collect();
    let (previous_count, previous_hash) = parse_cursor(label, cursor)?;

    // A cursor is trustworthy only if the region it covered still hashes the same.
    let truncated = if previous_count == 0 {
        false
    } else if previous_count <= all_lines.len() {
        hash_lines(&all_lines[..previous_count]) != previous_hash
    } else {
        // History is now shorter than the cursor position.
        true
    };

    let new_content = if truncated {
        captured.to_string()
    } else if previous_count >= all_lines.len() {
        String::new()
    } else {
        all_lines[previous_count..].join("\n")
    };

    let (text, was_capped) = cap_to_whole_lines(&new_content, max_bytes);

    let cursor = if was_capped && !truncated {
        // Advance only over the lines actually delivered. Counting newlines would
        // under-count by one, because joining lines leaves no trailing newline —
        // so a cap that delivers exactly one line would never advance the cursor
        // and that line would be redelivered forever.
        let delivered = if text.is_empty() {
            0
        } else {
            text.lines().count()
        };
        let end = (previous_count + delivered).min(all_lines.len());
        build_cursor(label, &all_lines[..end])
    } else {
        build_cursor(label, &all_lines)
    };

    Ok(CursorRead {
        text,
        cursor,
        truncated,
    })
}

/// Return the last `lines` lines of a capture.
pub fn tail(captured: &str, lines: usize) -> String {
    let all_lines: Vec<&str> = captured.lines().collect();
    let start = all_lines.len().saturating_sub(lines);
    all_lines[start..].join("\n")
}

/// Truncate to whole lines within a byte budget, so a line-based cursor can
/// advance precisely. A partial trailing line is dropped and redelivered later.
fn cap_to_whole_lines(content: &str, max_bytes: usize) -> (String, bool) {
    if max_bytes == 0 || content.len() <= max_bytes {
        return (content.to_string(), false);
    }
    let safe_end = content
        .char_indices()
        .take_while(|(index, _)| *index < max_bytes)
        .last()
        .map_or(0, |(index, character)| index + character.len_utf8());
    let capped = &content[..safe_end];
    match capped.rfind('\n') {
        Some(last_newline) => (content[..last_newline].to_string(), true),
        None => (capped.to_string(), true),
    }
}

/// Hash enough of the capture to detect both history rolloff and tail changes.
fn hash_lines(lines: &[&str]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    lines.len().hash(&mut hasher);
    // First lines anchor the capture: they change when history is trimmed.
    let first_end = lines.len().min(5);
    for line in &lines[..first_end] {
        line.hash(&mut hasher);
    }
    // Last lines detect a reset or rewrite at the tail.
    let start = lines.len().saturating_sub(10);
    for line in &lines[start..] {
        line.hash(&mut hasher);
    }
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RMUX: &str = "rmux";

    fn zero(label: &str) -> String {
        format!("{label}:0:0")
    }

    #[test]
    fn a_zero_cursor_returns_everything() {
        let read = read_after(RMUX, "one\ntwo\nthree", &zero(RMUX), 0).unwrap();

        assert_eq!(read.text, "one\ntwo\nthree");
        assert!(!read.truncated);
    }

    #[test]
    fn a_returned_cursor_yields_only_new_output() {
        let first = read_after(RMUX, "one\ntwo", &zero(RMUX), 0).unwrap();
        let second = read_after(RMUX, "one\ntwo\nthree", &first.cursor, 0).unwrap();

        assert_eq!(second.text, "three");
        assert!(!second.truncated);
    }

    #[test]
    fn an_unchanged_pane_yields_nothing() {
        let first = read_after(RMUX, "one\ntwo", &zero(RMUX), 0).unwrap();
        let second = read_after(RMUX, "one\ntwo", &first.cursor, 0).unwrap();

        assert_eq!(second.text, "");
        assert!(!second.truncated);
        assert_eq!(second.cursor, first.cursor);
    }

    #[test]
    fn rewritten_history_reports_truncation_and_resets() {
        let first = read_after(RMUX, "one\ntwo\nthree", &zero(RMUX), 0).unwrap();
        // The anchor lines changed, so the cursor can no longer be trusted.
        let second = read_after(RMUX, "different\nlines\nentirely", &first.cursor, 0).unwrap();

        assert!(second.truncated);
        assert_eq!(second.text, "different\nlines\nentirely");
    }

    #[test]
    fn a_capture_shorter_than_the_cursor_reports_truncation() {
        let first = read_after(RMUX, "one\ntwo\nthree\nfour", &zero(RMUX), 0).unwrap();
        let second = read_after(RMUX, "four", &first.cursor, 0).unwrap();

        assert!(second.truncated);
    }

    #[test]
    fn max_bytes_caps_at_a_line_boundary_and_the_rest_follows() {
        let captured = "aaaa\nbbbb\ncccc";
        let first = read_after(RMUX, captured, &zero(RMUX), 6).unwrap();

        // Only whole lines are delivered, so the cursor stays line-aligned.
        assert_eq!(first.text, "aaaa");
        assert!(!first.truncated);

        let second = read_after(RMUX, captured, &first.cursor, 0).unwrap();
        assert_eq!(second.text, "bbbb\ncccc");
    }

    #[test]
    fn repeated_capped_reads_eventually_deliver_everything() {
        let captured = "l1\nl2\nl3\nl4\nl5";
        let mut cursor = zero(RMUX);
        let mut delivered: Vec<String> = Vec::new();

        for _ in 0..10 {
            let read = read_after(RMUX, captured, &cursor, 4).unwrap();
            cursor = read.cursor;
            if read.text.is_empty() {
                break;
            }
            delivered.push(read.text);
        }

        assert_eq!(delivered.join("\n"), captured);
    }

    #[test]
    fn a_cursor_from_another_backend_is_rejected() {
        // A daemon byte-offset cursor must not be silently reinterpreted as lines.
        let error = read_after(RMUX, "one", "daemon:1234", 0).unwrap_err();
        assert!(error.contains("invalid rmux cursor"));

        let error = read_after(RMUX, "one", "tmux:1:2", 0).unwrap_err();
        assert!(error.contains("invalid rmux cursor"));
    }

    #[test]
    fn a_cursor_quoted_by_toon_output_is_accepted() {
        // `axi session read` prints `cursor: "rmux:2:123"`, and an agent that
        // copies that field verbatim must be able to pass it straight back.
        let first = read_after(RMUX, "one\ntwo", &zero(RMUX), 0).unwrap();
        let quoted = format!("\"{}\"", first.cursor);

        let second = read_after(RMUX, "one\ntwo\nthree", &quoted, 0).unwrap();
        assert_eq!(second.text, "three");
        assert!(!second.truncated);
    }

    #[test]
    fn a_quoted_cursor_from_another_backend_is_still_rejected() {
        let error = read_after(RMUX, "one", "\"tmux:1:2\"", 0).unwrap_err();
        assert!(error.contains("invalid rmux cursor"));
    }

    #[test]
    fn malformed_cursors_are_rejected() {
        assert!(read_after(RMUX, "one", "rmux:abc:1", 0).is_err());
        assert!(read_after(RMUX, "one", "rmux:1:xyz", 0).is_err());
        assert!(read_after(RMUX, "one", "rmux:1", 0).is_err());
    }

    #[test]
    fn the_label_appears_in_the_cursor_so_backends_cannot_be_confused() {
        let read = read_after("tmux", "one", "tmux:0:0", 0).unwrap();
        assert!(read.cursor.starts_with("tmux:"));

        let read = read_after(RMUX, "one", &zero(RMUX), 0).unwrap();
        assert!(read.cursor.starts_with("rmux:"));
    }

    #[test]
    fn tail_returns_the_last_lines() {
        assert_eq!(tail("one\ntwo\nthree", 2), "two\nthree");
        assert_eq!(tail("one", 5), "one");
        assert_eq!(tail("", 5), "");
    }
}
