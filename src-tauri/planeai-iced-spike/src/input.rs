use iced::keyboard::{key::Named, Key, Modifiers};

/// Terminal input encoder.
///
/// Expected byte mappings:
///
/// | Key           | Bytes              | Notes                    |
/// |---------------|--------------------|--------------------------|
/// | Enter         | `\r` (0x0d)        | Ctrl-M overlaps          |
/// | Backspace     | `\x7f` (0x7f)      | DEL                      |
/// | Tab           | `\t` (0x09)        | Ctrl-I overlaps          |
/// | Escape        | `\x1b` (0x1b)      | Ctrl-[ overlaps          |
/// | Ctrl-C        | `0x03`             | ETX                      |
/// | Ctrl-D        | `0x04`             | EOT                      |
/// | Ctrl-L        | `0x0c`             | FF (clear)               |
/// | Ctrl-A        | `0x01`             |                          |
/// | Ctrl-Z        | `0x1a`             |                          |
/// | Arrow Up      | `\x1b[A`           | CSI A                    |
/// | Arrow Down    | `\x1b[B`           | CSI B                    |
/// | Arrow Right   | `\x1b[C`           | CSI C                    |
/// | Arrow Left    | `\x1b[D`           | CSI D                    |
/// | Home          | `\x1b[H`           | CSI H                    |
/// | End           | `\x1b[F`           | CSI F                    |
/// | PageUp        | `\x1b[5~`          | CSI 5~                   |
/// | PageDown      | `\x1b[6~`          | CSI 6~                   |
/// | Delete        | `\x1b[3~`          | CSI 3~                   |
/// | Insert        | `\x1b[2~`          | CSI 2~                   |

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
                Named::Space => b" ",
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

#[cfg(test)]
mod tests {
    use super::*;
    use smol_str::SmolStr;

    fn no_mods() -> Modifiers { Modifiers::empty() }
    fn ctrl() -> Modifiers { Modifiers::CTRL }

    fn named(n: Named) -> (Key, Modifiers, Option<SmolStr>) {
        (Key::Named(n), no_mods(), None)
    }

    fn ctrl_char(c: &str) -> (Key, Modifiers, Option<SmolStr>) {
        (Key::Character(SmolStr::new(c)), ctrl(), None)
    }

    #[test]
    fn test_enter() {
        let (k, m, t) = named(Named::Enter);
        assert_eq!(encode_key_event(&k, &m, &t), Some(b"\r".to_vec()));
    }

    #[test]
    fn test_backspace() {
        let (k, m, t) = named(Named::Backspace);
        assert_eq!(encode_key_event(&k, &m, &t), Some(b"\x7f".to_vec()));
    }

    #[test]
    fn test_tab() {
        let (k, m, t) = named(Named::Tab);
        assert_eq!(encode_key_event(&k, &m, &t), Some(b"\t".to_vec()));
    }

    #[test]
    fn test_escape() {
        let (k, m, t) = named(Named::Escape);
        assert_eq!(encode_key_event(&k, &m, &t), Some(b"\x1b".to_vec()));
    }

    #[test]
    fn test_ctrl_c() {
        let (k, m, t) = ctrl_char("c");
        assert_eq!(encode_key_event(&k, &m, &t), Some(vec![0x03]));
    }

    #[test]
    fn test_ctrl_d() {
        let (k, m, t) = ctrl_char("d");
        assert_eq!(encode_key_event(&k, &m, &t), Some(vec![0x04]));
    }

    #[test]
    fn test_ctrl_l() {
        let (k, m, t) = ctrl_char("l");
        assert_eq!(encode_key_event(&k, &m, &t), Some(vec![0x0c]));
    }

    #[test]
    fn test_ctrl_a() {
        let (k, m, t) = ctrl_char("a");
        assert_eq!(encode_key_event(&k, &m, &t), Some(vec![0x01]));
    }

    #[test]
    fn test_ctrl_z() {
        let (k, m, t) = ctrl_char("z");
        assert_eq!(encode_key_event(&k, &m, &t), Some(vec![0x1a]));
    }

    #[test]
    fn test_arrows() {
        let cases = [
            (Named::ArrowUp, b"\x1b[A"),
            (Named::ArrowDown, b"\x1b[B"),
            (Named::ArrowRight, b"\x1b[C"),
            (Named::ArrowLeft, b"\x1b[D"),
        ];
        for (n, expected) in cases {
            let (k, m, t) = named(n);
            assert_eq!(encode_key_event(&k, &m, &t), Some(expected.to_vec()));
        }
    }

    #[test]
    fn test_home_end() {
        let (k, m, t) = named(Named::Home);
        assert_eq!(encode_key_event(&k, &m, &t), Some(b"\x1b[H".to_vec()));
        let (k, m, t) = named(Named::End);
        assert_eq!(encode_key_event(&k, &m, &t), Some(b"\x1b[F".to_vec()));
    }

    #[test]
    fn test_page_up_down() {
        let (k, m, t) = named(Named::PageUp);
        assert_eq!(encode_key_event(&k, &m, &t), Some(b"\x1b[5~".to_vec()));
        let (k, m, t) = named(Named::PageDown);
        assert_eq!(encode_key_event(&k, &m, &t), Some(b"\x1b[6~".to_vec()));
    }

    #[test]
    fn test_delete_insert() {
        let (k, m, t) = named(Named::Delete);
        assert_eq!(encode_key_event(&k, &m, &t), Some(b"\x1b[3~".to_vec()));
        let (k, m, t) = named(Named::Insert);
        assert_eq!(encode_key_event(&k, &m, &t), Some(b"\x1b[2~".to_vec()));
    }

    #[test]
    fn test_printable_char() {
        let k = Key::Character(SmolStr::new("a"));
        let t = Some(SmolStr::new("a"));
        assert_eq!(encode_key_event(&k, &no_mods(), &t), Some(b"a".to_vec()));
    }
}
