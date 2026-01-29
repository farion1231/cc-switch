use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone)]
pub struct TextInput {
    value: String,
    cursor: usize, // byte index
    masked: bool,
}

impl TextInput {
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        Self {
            cursor: value.len(),
            value,
            masked: false,
        }
    }

    pub fn masked(value: impl Into<String>) -> Self {
        let mut input = Self::new(value);
        input.masked = true;
        input
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn set_value(&mut self, value: impl Into<String>) {
        self.value = value.into();
        self.cursor = self.value.len();
    }

    pub fn display_value(&self) -> String {
        if !self.masked {
            return self.value.clone();
        }
        let len = self.value.chars().count();
        "*".repeat(len)
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char(c)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                if self.cursor > self.value.len() {
                    self.cursor = self.value.len();
                }
                self.value.insert(self.cursor, c);
                self.cursor += c.len_utf8();
                true
            }
            KeyCode::Backspace => {
                if self.value.is_empty() || self.cursor == 0 {
                    return false;
                }
                let prev = prev_char_boundary(&self.value, self.cursor);
                if prev == self.cursor {
                    return false;
                }
                self.value.drain(prev..self.cursor);
                self.cursor = prev;
                true
            }
            KeyCode::Delete => {
                if self.value.is_empty() || self.cursor >= self.value.len() {
                    return false;
                }
                let next = next_char_boundary(&self.value, self.cursor);
                if next == self.cursor {
                    return false;
                }
                self.value.drain(self.cursor..next);
                true
            }
            KeyCode::Left => {
                self.cursor = prev_char_boundary(&self.value, self.cursor);
                false
            }
            KeyCode::Right => {
                self.cursor = next_char_boundary(&self.value, self.cursor);
                false
            }
            KeyCode::Home => {
                self.cursor = 0;
                false
            }
            KeyCode::End => {
                self.cursor = self.value.len();
                false
            }
            _ => false,
        }
    }
}

fn prev_char_boundary(text: &str, idx: usize) -> usize {
    let idx = idx.min(text.len());
    if idx == 0 {
        return 0;
    }
    text.char_indices()
        .take_while(|(i, _)| *i < idx)
        .map(|(i, _)| i)
        .last()
        .unwrap_or(0)
}

fn next_char_boundary(text: &str, idx: usize) -> usize {
    let idx = idx.min(text.len());
    if idx >= text.len() {
        return text.len();
    }
    text.char_indices()
        .map(|(i, _)| i)
        .filter(|i| *i > idx)
        .next()
        .unwrap_or(text.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::empty(),
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        }
    }

    #[test]
    fn text_input_inserts_and_deletes() {
        let mut input = TextInput::new("");
        assert!(input.handle_key(key(KeyCode::Char('a'))));
        assert!(input.handle_key(key(KeyCode::Char('b'))));
        assert_eq!(input.value(), "ab");

        assert!(input.handle_key(key(KeyCode::Backspace)));
        assert_eq!(input.value(), "a");

        assert!(!input.handle_key(key(KeyCode::Delete)));
        assert_eq!(input.value(), "a");

        input.handle_key(key(KeyCode::Home));
        assert!(input.handle_key(key(KeyCode::Delete)));
        assert_eq!(input.value(), "");
    }

    #[test]
    fn text_input_moves_cursor() {
        let mut input = TextInput::new("ab");
        input.handle_key(key(KeyCode::Left));
        assert!(input.handle_key(key(KeyCode::Char('x'))));
        assert_eq!(input.value(), "axb");
    }

    #[test]
    fn masked_input_hides_value() {
        let mut input = TextInput::masked("secret");
        assert_eq!(input.display_value(), "******");
        assert!(input.handle_key(key(KeyCode::Char('x'))));
        assert_eq!(input.value(), "secretx");
        assert_eq!(input.display_value(), "*******");
    }
}
