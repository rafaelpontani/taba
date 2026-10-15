//! The keys: decodes the bytes the raw-mode terminal delivers (UTF-8 text and ANSI sequences
//! for arrows and the like).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    Delete,
    Backspace,
    Enter,
    Tab,
    BackTab,
    Esc,
    Char(char),
    /// Ctrl with a lowercase letter (`Ctrl('s')`).
    Ctrl(char),
}

/// Decodes everything that arrived in one read. An unknown sequence is skipped whole.
pub fn decode(bytes: &[u8]) -> Vec<Key> {
    let mut keys = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            0x1b => {
                if i + 1 >= bytes.len() {
                    keys.push(Key::Esc);
                    i += 1;
                    continue;
                }
                match bytes[i + 1] {
                    b'[' | b'O' => {
                        // CSI / SS3: parameters and then the final byte (0x40..=0x7e).
                        let mut j = i + 2;
                        while j < bytes.len() && !(0x40..=0x7e).contains(&bytes[j]) {
                            j += 1;
                        }
                        let params = &bytes[i + 2..j.min(bytes.len())];
                        if let Some(&final_byte) = bytes.get(j) {
                            if let Some(key) = csi_key(params, final_byte) {
                                keys.push(key);
                            }
                        }
                        i = j + 1;
                    }
                    // Alt+key and others: ignores the ESC and processes the key.
                    _ => i += 1,
                }
            }
            0x0d | 0x0a => {
                keys.push(Key::Enter);
                i += 1;
            }
            0x09 => {
                keys.push(Key::Tab);
                i += 1;
            }
            0x7f | 0x08 => {
                keys.push(Key::Backspace);
                i += 1;
            }
            0x01..=0x1a => {
                keys.push(Key::Ctrl((b - 1 + b'a') as char));
                i += 1;
            }
            0x00 | 0x1c..=0x1f => i += 1,
            _ => {
                // A UTF-8 character (1 to 4 bytes).
                let len = match b {
                    0x00..=0x7f => 1,
                    0xc0..=0xdf => 2,
                    0xe0..=0xef => 3,
                    0xf0..=0xf7 => 4,
                    _ => 1,
                };
                let end = (i + len).min(bytes.len());
                if let Some(c) = std::str::from_utf8(&bytes[i..end])
                    .ok()
                    .and_then(|s| s.chars().next())
                {
                    keys.push(Key::Char(c));
                }
                i = end;
            }
        }
    }
    keys
}

fn csi_key(params: &[u8], final_byte: u8) -> Option<Key> {
    match final_byte {
        b'A' => Some(Key::Up),
        b'B' => Some(Key::Down),
        b'C' => Some(Key::Right),
        b'D' => Some(Key::Left),
        b'H' => Some(Key::Home),
        b'F' => Some(Key::End),
        b'Z' => Some(Key::BackTab),
        b'~' => match params {
            b"1" | b"7" => Some(Key::Home),
            b"4" | b"8" => Some(Key::End),
            b"3" => Some(Key::Delete),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests;
