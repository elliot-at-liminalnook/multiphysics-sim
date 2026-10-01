//! The parameter form (cad-modify; RoboCAD's `QInputDialog.getDouble`/
//! `getInt` prompts and its `ArrayDialog`, ui/widgets.py:1024-1063): a
//! titled panel of labelled fields and OK / Cancel. Numeric fields are
//! evaluated on every keystroke with RoboCAD's unit expressions
//! (`sim_runtime::units`), and an error names the bad token. Choice and
//! checkbox fields are clickable.
//!
//! No intent logic: the caller owns the drafts and passes one action
//! component per clickable part (`FormHit`); [`evaluate`] and
//! [`TextDraft::key`] are pure functions the caller calls.
use super::Kit;
use bevy::input::keyboard::Key;
use bevy::prelude::*;
use serde_json::Value;

/// How a number is read: a length (bare numbers mm), an angle (degrees),
/// a whole count, or a plain factor (RoboCAD's `NumericField(angle=)`
/// and `QSpinBox`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unit {
    Length,
    Angle,
    Count,
    Factor,
}

/// A field's kind, with RoboCAD's range where its dialog sets one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum FieldKind {
    /// One number; `min`/`max` inclusive, `decimals` as the dialog rounds (0 for counts).
    Number { unit: Unit, min: Option<f64>, max: Option<f64>, decimals: u8 },
    /// Three numbers "x, y, z", each read as `unit`.
    Vector { unit: Unit },
    /// One of `options` (the text is the option).
    Choice { options: &'static [&'static str] },
    /// On or off (the text is "true" or "false").
    Check,
    /// A JSON value as typed (REST-only shapes such as a components map).
    Json,
}

/// A field's evaluated value.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FieldValue {
    Number(f64),
    Vector([f64; 3]),
    Choice(usize),
    Check(bool),
    Json(Value),
}

/// Evaluate a field's text as its kind reads it. Errors name the token
/// (the evaluator's message) or the range RoboCAD's dialog enforces.
pub(crate) fn evaluate(kind: &FieldKind, text: &str) -> Result<FieldValue, String> {
    let _ = (kind, text);
    todo!("ui_kit::form::evaluate")
}

/// One row the form shows.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FormRow<'a> {
    pub label: &'a str,
    pub kind: FieldKind,
    /// The draft as typed (a choice's option, "true"/"false" for a checkbox).
    pub text: &'a str,
    pub focused: bool,
}

/// What a click on a form part means; the caller turns each into its action component.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FormHit {
    /// Field `i` takes the keyboard.
    Field(usize),
    /// Field `i`'s choice option `option`.
    Option(usize, usize),
    /// Field `i`'s checkbox.
    Check(usize),
    Ok,
    Cancel,
}

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
    /// Apply one pressed key (`chord`: Control/Command held, so characters are not typed).
    pub(crate) fn key(&mut self, key: &Key, chord: bool) -> DraftKey {
        let _ = (key, chord);
        todo!("ui_kit::form::TextDraft::key")
    }
}

impl Kit<'_> {
    /// The form: `title`, one row per field (label, field or options,
    /// the evaluation or error under a numeric field), then OK (enabled
    /// when `ok_enabled`) and Cancel. `hit` gives each clickable part's
    /// action component. Accessible labels name each field and button.
    pub(crate) fn form<A: Component>(&self, parent: &mut ChildSpawnerCommands, title: &str, rows: &[FormRow], ok_enabled: bool, hit: impl Fn(FormHit) -> A) {
        let _ = (parent, title, rows, ok_enabled, hit);
        todo!("Kit::form")
    }
}
