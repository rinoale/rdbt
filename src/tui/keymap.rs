use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    EnterCommandMode,
    Cancel,
    Help,
    ToggleSafeMode,
    RefreshMetadata,
    ToggleFocus,
    Submit,
    Previous,
    Next,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Esc,
    F(u8),
    Enter,
    Tab,
    BackTab,
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyPattern {
    pub key: Key,
    pub modifiers: KeyModifiers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyBinding {
    pub pattern: KeyPattern,
    pub intent: Intent,
    pub label: &'static str,
    pub description: &'static str,
}

#[derive(Debug, Clone)]
pub struct Keymap {
    bindings: Vec<KeyBinding>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self::rdbt()
    }
}

impl Keymap {
    pub fn rdbt() -> Self {
        let none = KeyModifiers::NONE;
        Self {
            bindings: vec![
                binding(
                    Key::Char(':'),
                    none,
                    Intent::EnterCommandMode,
                    ":",
                    "command mode",
                ),
                binding(Key::Char('?'), none, Intent::Help, "?", "help"),
                binding(Key::Esc, none, Intent::Cancel, "Esc", "cancel"),
                binding(Key::F(2), none, Intent::ToggleSafeMode, "F2", "mode"),
                binding(Key::F(5), none, Intent::RefreshMetadata, "F5", "refresh"),
                binding(Key::Tab, none, Intent::ToggleFocus, "Tab", "focus"),
                binding(
                    Key::BackTab,
                    none,
                    Intent::ToggleFocus,
                    "Shift-Tab",
                    "focus",
                ),
                binding(Key::Enter, none, Intent::Submit, "Enter", "run/open"),
                binding(Key::Up, none, Intent::Previous, "Up", "previous"),
                binding(Key::Down, none, Intent::Next, "Down", "next"),
            ],
        }
    }

    pub fn intent_for(&self, key: KeyEvent) -> Option<Intent> {
        self.bindings
            .iter()
            .find(|binding| binding.pattern.matches(key))
            .map(|binding| binding.intent)
    }

    pub fn bindings(&self) -> &[KeyBinding] {
        &self.bindings
    }
}

pub fn binding(
    key: Key,
    modifiers: KeyModifiers,
    intent: Intent,
    label: &'static str,
    description: &'static str,
) -> KeyBinding {
    KeyBinding {
        pattern: KeyPattern { key, modifiers },
        intent,
        label,
        description,
    }
}

impl KeyPattern {
    fn matches(self, event: KeyEvent) -> bool {
        key_matches(self.key, event.code) && normalized_modifiers(event.modifiers) == self.modifiers
    }
}

pub fn text_input_modifiers(mut modifiers: KeyModifiers) -> bool {
    modifiers.remove(KeyModifiers::SHIFT);
    modifiers.is_empty()
}

fn key_matches(expected: Key, actual: KeyCode) -> bool {
    match (expected, actual) {
        (Key::Char(expected), KeyCode::Char(actual)) => expected == actual,
        (Key::Esc, KeyCode::Esc) => true,
        (Key::F(expected), KeyCode::F(actual)) => expected == actual,
        (Key::Enter, KeyCode::Enter) => true,
        (Key::Tab, KeyCode::Tab) => true,
        (Key::BackTab, KeyCode::BackTab) => true,
        (Key::Up, KeyCode::Up) => true,
        (Key::Down, KeyCode::Down) => true,
        _ => false,
    }
}

fn normalized_modifiers(mut modifiers: KeyModifiers) -> KeyModifiers {
    modifiers.remove(KeyModifiers::SHIFT);
    modifiers
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::{Intent, Keymap};

    #[test]
    fn quit_is_not_bound_to_a_key() {
        let keymap = Keymap::rdbt();
        assert_eq!(
            keymap.intent_for(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
            None
        );
        assert_eq!(
            keymap.intent_for(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            None
        );
    }

    #[test]
    fn colon_enters_command_mode() {
        let keymap = Keymap::rdbt();
        assert_eq!(
            keymap.intent_for(KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE)),
            Some(Intent::EnterCommandMode)
        );
    }

    #[test]
    fn escape_cancels_without_quitting() {
        let keymap = Keymap::rdbt();
        assert_eq!(
            keymap.intent_for(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            Some(Intent::Cancel)
        );
    }

    #[test]
    fn question_mark_opens_help() {
        let keymap = Keymap::rdbt();
        assert_eq!(
            keymap.intent_for(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE)),
            Some(Intent::Help)
        );
    }
}
