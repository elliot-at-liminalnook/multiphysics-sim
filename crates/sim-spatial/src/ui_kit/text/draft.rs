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

/// A text field's draft: the text, the caret (a byte index on a char
/// boundary) and the selection. The selection is either the whole text
/// (`select_all`, RoboCAD's `selectAll` on focus: the next character
/// replaces it) or the range between `anchor` and the caret (Shift+arrows
/// extend it). Typing replaces whichever selection is live.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct TextDraft {
    pub text: String,
    pub select_all: bool,
    pub caret: usize,
    pub anchor: Option<usize>,
}
impl TextDraft {
    /// A draft of `text`, selected when `select_all`; the caret is at the end.
    pub(crate) fn new(text: impl Into<String>, select_all: bool) -> Self {
        let text = text.into();
        let caret = text.len();
        Self { text, select_all, caret, anchor: None }
    }

    /// The selected byte range, if any (the whole text when `select_all`,
    /// else anchor↔caret when they differ).
    pub(crate) fn selection(&self) -> Option<(usize, usize)> {
        if self.select_all {
            return Some((0, self.text.len()));
        }
        let anchor = self.anchor?;
        (anchor != self.caret).then_some((anchor.min(self.caret), anchor.max(self.caret)))
    }

    /// The caret clamped to a char boundary inside the text (drafts are
    /// also replaced wholesale, which can strand a caret past the end).
    fn caret(&self) -> usize {
        let mut caret = self.caret.min(self.text.len());
        while !self.text.is_char_boundary(caret) {
            caret -= 1;
        }
        caret
    }

    fn clear_selection(&mut self) {
        self.select_all = false;
        self.anchor = None;
    }

    /// Delete the selected range if there is one; the caret lands at its start.
    fn delete_selection(&mut self) -> bool {
        let Some((start, end)) = self.selection() else { return false };
        self.text.replace_range(start..end, "");
        self.caret = start;
        self.clear_selection();
        true
    }

    /// The byte index one char before/after `at` (clamped).
    fn prev(&self, at: usize) -> usize {
        self.text[..at].char_indices().last().map_or(0, |(i, _)| i)
    }
    fn next(&self, at: usize) -> usize {
        self.text[at..].chars().next().map_or(at, |c| at + c.len_utf8())
    }

    /// Move the caret to `to`; with `extend`, the selection anchor stays
    /// (set to the old caret when there was none: Shift+arrows). Without,
    /// any selection collapses.
    fn move_to(&mut self, to: usize, extend: bool) {
        if extend {
            if self.select_all {
                self.anchor = Some(0);
                self.select_all = false;
            } else if self.anchor.is_none() {
                self.anchor = Some(self.caret());
            }
        } else {
            self.clear_selection();
        }
        self.caret = to;
    }

    /// Apply one pressed key (`chord`: Control/Command held, so characters
    /// are not typed; `shift`: selection extension for the caret keys).
    /// `Edited` when the text, the caret or the selection changed (typing
    /// the selected text's own character over it still clears the selection).
    /// A typed text with any character `filter` refuses is not typed
    /// (`Ignored`), as Bevy's `EditableTextFilter` refuses an insert.
    pub(crate) fn key(&mut self, key: &Key, chord: bool, shift: bool, filter: Option<fn(char) -> bool>) -> DraftKey {
        let before = (self.text.clone(), self.select_all, self.caret, self.anchor);
        let allowed = |s: &str| filter.is_none_or(|f| s.chars().all(f));
        match key {
            Key::Enter => return DraftKey::Enter,
            Key::Escape => return DraftKey::Escape,
            Key::Tab => return DraftKey::Tab,
            Key::ArrowLeft if !chord => {
                let caret = self.caret();
                let to = match self.selection() {
                    Some((start, _)) if !shift => start,
                    _ => self.prev(caret),
                };
                self.move_to(to, shift);
            }
            Key::ArrowRight if !chord => {
                let caret = self.caret();
                let to = match self.selection() {
                    Some((_, end)) if !shift => end,
                    _ => self.next(caret),
                };
                self.move_to(to, shift);
            }
            Key::Home if !chord => {
                let caret = self.caret();
                let to = self.text[..caret].rfind('\n').map_or(0, |i| i + 1);
                self.move_to(to, shift);
            }
            Key::End if !chord => {
                let caret = self.caret();
                let to = self.text[caret..].find('\n').map_or(self.text.len(), |i| caret + i);
                self.move_to(to, shift);
            }
            Key::Backspace => {
                if !self.delete_selection() {
                    let caret = self.caret();
                    if caret > 0 {
                        let start = self.prev(caret);
                        self.text.replace_range(start..caret, "");
                        self.caret = start;
                    }
                }
            }
            Key::Delete => {
                if !self.delete_selection() {
                    let caret = self.caret();
                    if caret < self.text.len() {
                        let end = self.next(caret);
                        self.text.replace_range(caret..end, "");
                    }
                }
            }
            Key::Space if !chord && allowed(" ") => self.type_text(" "),
            Key::Character(c) if !chord && !c.chars().any(char::is_control) && allowed(c.as_str()) => self.type_text(c.as_str()),
            _ => {}
        }
        if (self.text.clone(), self.select_all, self.caret, self.anchor) != before { DraftKey::Edited } else { DraftKey::Ignored }
    }

    /// Type `text` at the caret (replacing any selection), as
    /// `cad::numeric`'s `type_text`.
    pub(crate) fn type_text(&mut self, text: &str) {
        if !self.delete_selection() {
            self.caret = self.caret();
            self.clear_selection();
        }
        self.text.insert_str(self.caret, text);
        self.caret += text.len();
    }
}
