use crossterm::event::{KeyEvent, KeyModifiers};
use rustui::keymap::{KeyBinding as FrameworkKeyBinding, Keymap as FrameworkKeymap, binding};

pub use rustui::keymap::{Key, text_input_modifiers};

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

pub type KeyBinding = FrameworkKeyBinding<Intent>;

#[derive(Debug, Clone)]
pub struct Keymap {
    inner: FrameworkKeymap<Intent>,
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
            inner: FrameworkKeymap::new(vec![
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
            ]),
        }
    }

    pub fn intent_for(&self, key: KeyEvent) -> Option<Intent> {
        self.inner.intent_for(key)
    }

    pub fn bindings(&self) -> &[KeyBinding] {
        self.inner.bindings()
    }
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
