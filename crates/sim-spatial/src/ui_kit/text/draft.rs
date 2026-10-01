//! A text field's draft and what one key does to it (moved here from
//! `form.rs`; the numeric bar's editing rules, RoboCAD's `selectAll`).
use bevy::input::keyboard::Key;

/// What a key did to a draft ([`TextDraft::key`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DraftKey {
    Edited,
    Enter,
    Escape,
    Tab,
    Ignored,
}

/// A text field's draft: the text and whether it is selected (the next
/// character replaces it, RoboCAD's `selectAll`).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct TextDraft {
    pub text: String,
    pub select_all: bool,
}
impl TextDraft {
    /// A draft of `text`, selected when `select_all`.
    pub(crate) fn new(text: impl Into<String>, select_all: bool) -> Self {
        Self { text: text.into(), select_all }
    }

    /// Apply one pressed key (`chord`: Control/Command held, so characters
    /// are not typed). `Edited` when the text or the selection changed (typing
    /// the selected text's own character over it still clears the selection).
    pub(crate) fn key(&mut self, key: &Key, chord: bool) -> DraftKey {
        self.key_filtered(key, chord, None)
    }

    /// [`TextDraft::key`] with a character filter: a typed text with any
    /// character the filter refuses is not typed (`Ignored`), as Bevy's
    /// `EditableTextFilter` refuses an insert.
    pub(crate) fn key_filtered(&mut self, key: &Key, chord: bool, filter: Option<fn(char) -> bool>) -> DraftKey {
        let (text, select_all) = (self.text.clone(), self.select_all);
        let allowed = |s: &str| filter.is_none_or(|f| s.chars().all(f));
        match key {
            Key::Enter => return DraftKey::Enter,
            Key::Escape => return DraftKey::Escape,
            Key::Tab => return DraftKey::Tab,
            Key::Backspace => {
                if self.select_all {
                    self.text.clear();
                    self.select_all = false;
                } else {
                    self.text.pop();
                }
            }
            Key::Space if !chord && allowed(" ") => self.type_text(" "),
            Key::Character(c) if !chord && !c.chars().any(char::is_control) && allowed(c.as_str()) => self.type_text(c.as_str()),
            _ => {}
        }
        if self.text != text || self.select_all != select_all { DraftKey::Edited } else { DraftKey::Ignored }
    }

    /// Type `text` (replacing a selected text), as `cad::numeric`'s `type_text`.
    pub(crate) fn type_text(&mut self, text: &str) {
        if self.select_all {
            self.text.clear();
            self.select_all = false;
        }
        self.text.push_str(text);
    }
}
