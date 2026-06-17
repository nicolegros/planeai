use iced::keyboard::{key::Named, Key, Modifiers};

pub fn encode_key_event(key: &Key, modifiers: &Modifiers, text: &Option<smol_str::SmolStr>) -> Option<Vec<u8>> {
    if modifiers.control() {
        if let Key::Character(c) = key {
            let b = c.as_str().bytes().next()?;
            let ctrl = match b {
                b'a'..=b'z' => b - b'a' + 1,
                b'A'..=b'Z' => b - b'A' + 1,
                b'[' => 27,
                b'\\' => 28,
                b']' => 29,
                b'^' => 30,
                b'_' => 31,
                _ => return Some(c.as_str().as_bytes().to_vec()),
            };
            return Some(vec![ctrl]);
        }
    }

    match key {
        Key::Named(named) => {
            let bytes: &[u8] = match named {
                Named::Enter => b"\r",
                Named::Backspace => b"\x7f",
                Named::Tab => b"\t",
                Named::Escape => b"\x1b",
                Named::ArrowUp => b"\x1b[A",
                Named::ArrowDown => b"\x1b[B",
                Named::ArrowRight => b"\x1b[C",
                Named::ArrowLeft => b"\x1b[D",
                Named::Home => b"\x1b[H",
                Named::End => b"\x1b[F",
                Named::PageUp => b"\x1b[5~",
                Named::PageDown => b"\x1b[6~",
                Named::Delete => b"\x1b[3~",
                Named::Insert => b"\x1b[2~",
                _ => return None,
            };
            Some(bytes.to_vec())
        }
        Key::Character(_) => {
            if let Some(t) = text {
                let s = t.as_str();
                if !s.is_empty() {
                    return Some(s.as_bytes().to_vec());
                }
            }
            if let Key::Character(c) = key {
                Some(c.as_str().as_bytes().to_vec())
            } else {
                None
            }
        }
        _ => None,
    }
}
