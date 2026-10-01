//! The parameter form (cad-modify; RoboCAD's `QInputDialog.getDouble`/
//! `getInt` prompts and its `ArrayDialog`, ui/widgets.py:1024-1063): a
//! titled panel of labelled fields and OK / Cancel. Numeric fields are
//! evaluated on every keystroke with RoboCAD's unit expressions
//! (`sim_runtime::units`), and an error names the bad token. Choice,
//! pick (a list filled at run time: bodies, joints, motors) and checkbox
//! fields are clickable.
//!
//! No intent logic: the caller owns the drafts and passes one action
//! component per clickable part (`FormHit`); [`evaluate`] and
//! [`TextDraft::key`] are pure functions the caller calls.
//!
//! A number keeps its typed value: RoboCAD's spin boxes round what they
//! show to their decimals, but the form does not round (`decimals` is the
//! caller's to use when it formats a value); a count must be whole.
//! Range errors have no label ("0 is outside 0.01…100"): the caller
//! prefixes the field's label.
use super::Kit;
use super::theme::*;
use bevy::input::keyboard::Key;
use bevy::prelude::*;
use bevy::ui::prelude::AccessibleLabel;
use serde_json::Value;

/// How a number is read: a length (bare numbers mm), an angle (degrees),
/// a whole count (RoboCAD's `NumericField(angle=)` and `QSpinBox`), or a
/// plain number with no unit, fractions allowed (RoboCAD's
/// `NumericField(…, unit="")`: the sketch spiral's turns).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unit {
    Length,
    Angle,
    Count,
    Plain,
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
    /// Plain text as typed (RoboCAD's `QInputDialog.getText`: "Text to sketch:").
    Text,
    /// One of a list the caller fills at run time (RoboCAD's `QComboBox`
    /// of bodies, joints or motors): the text is the chosen entry's key;
    /// `source` names the list (the caller resolves it, `FormRow::picks`).
    /// An empty key is a choice only where the list offers it ("(world)",
    /// "(none)"); the caller leaves an empty optional field out.
    Pick { source: &'static str },
}

/// A field's evaluated value.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FieldValue {
    Number(f64),
    Vector([f64; 3]),
    Choice(usize),
    Check(bool),
    Json(Value),
    Text(String),
}

/// Evaluate a field's text as its kind reads it. Errors name the token
/// (the evaluator's message) or the range RoboCAD's dialog enforces.
pub(crate) fn evaluate(kind: &FieldKind, text: &str) -> Result<FieldValue, String> {
    match *kind {
        FieldKind::Number { unit, min, max, .. } => {
            let v = number(unit, text)?;
            match (min, max) {
                (Some(lo), Some(hi)) if !(lo..=hi).contains(&v) => Err(format!("{v} is outside {lo}\u{2026}{hi}")),
                (Some(lo), None) if v < lo => Err(format!("{v} is below the minimum {lo}")),
                (None, Some(hi)) if v > hi => Err(format!("{v} is above the maximum {hi}")),
                _ => Ok(FieldValue::Number(v)),
            }
        }
        FieldKind::Vector { unit } => {
            let parts: Vec<&str> = text.split(',').collect();
            if parts.len() != 3 {
                return Err(format!("expected three values \"x, y, z\" (got {})", parts.len()));
            }
            let mut v = [0.0; 3];
            for ((slot, part), axis) in v.iter_mut().zip(&parts).zip(["x", "y", "z"]) {
                *slot = number(unit, part).map_err(|e| format!("{axis}: {e}"))?;
            }
            Ok(FieldValue::Vector(v))
        }
        FieldKind::Choice { options } => options
            .iter()
            .position(|o| *o == text)
            .or_else(|| options.iter().position(|o| o.to_lowercase() == text.to_lowercase()))
            .map(FieldValue::Choice)
            .ok_or_else(|| format!("{text:?} is not one of: {}", options.join(", "))),
        FieldKind::Check => match text.trim().to_lowercase().as_str() {
            "true" | "1" | "on" => Ok(FieldValue::Check(true)),
            "false" | "0" | "off" => Ok(FieldValue::Check(false)),
            _ => Err(format!("expected true or false (got {text:?})")),
        },
        FieldKind::Json => serde_json::from_str::<Value>(text).map(FieldValue::Json).map_err(|e| e.to_string()),
        FieldKind::Text => Ok(FieldValue::Text(text.to_string())),
        FieldKind::Pick { .. } => match text.trim() {
            "" => Err("choose one".to_string()),
            key => Ok(FieldValue::Text(key.to_string())),
        },
    }
}

/// One number as `unit` reads it (the numeric bar's mapping,
/// `cad::transform::FieldKind::evaluate`); a count must be whole; a count
/// and a plain number take no unit (the evaluator would scale "2cm" to 20).
fn number(unit: Unit, text: &str) -> Result<f64, String> {
    if matches!(unit, Unit::Count | Unit::Plain)
        && let Some((token, at)) = unit_token(text)
    {
        let what = if unit == Unit::Count { "a count" } else { "this number" };
        return Err(format!("'{token}' at {at}: {what} takes no unit"));
    }
    let v = match unit {
        Unit::Length => sim_runtime::units::evaluate(text, false, Some("mm")),
        Unit::Angle => sim_runtime::units::evaluate(text, true, None),
        Unit::Count | Unit::Plain => sim_runtime::units::evaluate(text, false, None),
    }
    .map_err(|e| e.to_string())?;
    if unit == Unit::Count && v.fract() != 0.0 {
        return Err(format!("a count must be a whole number (got {v})"));
    }
    Ok(v)
}

/// The first length or angle unit named in `text`, with its character
/// position in the trimmed text (as the evaluator counts positions). Names
/// are scanned as `sim_runtime::units` tokenizes them (ASCII letters, `_`,
/// `µ`, `°`, `"`, `'`); an exponent's `e` ("1e3") reads as the constant `e`,
/// which is no unit.
fn unit_token(text: &str) -> Option<(String, usize)> {
    use sim_runtime::units::{ANGLE_UNITS, LENGTH_UNITS};
    let is_name = |c: char| c.is_ascii_alphabetic() || matches!(c, '_' | '\u{b5}' | '\u{b0}' | '"' | '\'');
    let chars: Vec<char> = text.trim().chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if !is_name(chars[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && is_name(chars[i]) {
            i += 1;
        }
        let name: String = chars[start..i].iter().collect();
        if LENGTH_UNITS.iter().chain(ANGLE_UNITS).any(|(u, _)| *u == name) {
            return Some((name, start));
        }
    }
    None
}

/// A number as the form shows it under its field: "1.5 mm", "45°", "3".
fn show(unit: Unit, v: f64) -> String {
    match unit {
        Unit::Length => sim_runtime::units::format_length(v, "mm", 3),
        Unit::Angle => sim_runtime::units::format_angle(v, 2),
        Unit::Count | Unit::Plain => format!("{v}"),
    }
}

/// One row the form shows.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FormRow<'a> {
    pub label: &'a str,
    pub kind: FieldKind,
    /// The draft as typed (a choice's option, "true"/"false" for a checkbox).
    pub text: &'a str,
    pub focused: bool,
    /// The field may be left empty (its default is empty and the caller
    /// leaves it out): empty, it is neither evaluated nor shown as an error.
    pub optional: bool,
    /// The focused field's text is selected (the next key replaces it).
    pub selected: bool,
    /// A `FieldKind::Pick` field's choices as (key, label), in order;
    /// empty for every other kind. `FormHit::Option(i, k)` names `picks[k]`.
    pub picks: &'a [(String, String)],
}

/// What a click on a form part means; the caller turns each into its action component.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FormHit {
    /// Field `i` takes the keyboard.
    Field(usize),
    /// Field `i`'s choice option `option` (a `Pick` field's `picks[option]`).
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
    /// Apply one pressed key (`chord`: Control/Command held, so characters
    /// are not typed). `Edited` when the text or the selection changed (typing
    /// the selected text's own character over it still clears the selection).
    pub(crate) fn key(&mut self, key: &Key, chord: bool) -> DraftKey {
        let (text, select_all) = (self.text.clone(), self.select_all);
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
            Key::Space if !chord => self.type_text(" "),
            Key::Character(c) if !chord && !c.chars().any(char::is_control) => self.type_text(c.as_str()),
            _ => {}
        }
        if self.text != text || self.select_all != select_all { DraftKey::Edited } else { DraftKey::Ignored }
    }

    /// Type `text` (replacing a selected text), as `cad::numeric`'s `type_text`.
    fn type_text(&mut self, text: &str) {
        if self.select_all {
            self.text.clear();
            self.select_all = false;
        }
        self.text.push_str(text);
    }
}

impl Kit<'_> {
    /// The form: `title`, one row per field (label, field or options,
    /// the evaluation or error under a numeric field), then OK (enabled
    /// when `ok_enabled`) and Cancel, then whatever `footer` spawns (a
    /// refusal, a hint) inside the same panel. `hit` gives each clickable
    /// part's action component. The panel is the form's frame, `width` wide
    /// (at least 320 px when `None`). Accessible labels name each field and
    /// button.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn form<A: Component>(&self, parent: &mut ChildSpawnerCommands, title: &str, rows: &[FormRow], ok_enabled: bool, width: Option<f32>, hit: impl Fn(FormHit) -> A, footer: impl FnOnce(&mut ChildSpawnerCommands)) {
        // The numeric bar's panel (cad/numeric.rs `spawn`).
        parent
            .spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(8.0),
                    padding: UiRect::all(Val::Px(12.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(6.0)),
                    width: width.map_or(Val::Auto, Val::Px),
                    min_width: if width.is_some() { Val::Auto } else { Val::Px(320.0) },
                    ..default()
                },
                BackgroundColor(SURFACE),
                BorderColor::all(BORDER),
                AccessibleLabel::new(title),
            ))
            .with_children(|panel| {
                panel.spawn(self.title(title));
                for (i, row) in rows.iter().enumerate() {
                    panel.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(3.0), ..default() }).with_children(|cell| self.form_row(cell, i, row, &hit));
                }
                panel.spawn(Node { justify_content: JustifyContent::FlexEnd, column_gap: Val::Px(6.0), margin: UiRect::top(Val::Px(4.0)), ..default() }).with_children(|buttons| {
                    buttons.spawn(self.button("OK", hit(FormHit::Ok), Look::Primary, ok_enabled));
                    buttons.spawn(self.button("Cancel", hit(FormHit::Cancel), Look::Secondary, true));
                });
                footer(panel);
            });
    }

    /// One form row: its label, then its field (with the evaluation or the
    /// error under it, as the numeric bar shows them), options or checkbox.
    fn form_row<A: Component>(&self, cell: &mut ChildSpawnerCommands, i: usize, row: &FormRow, hit: &impl Fn(FormHit) -> A) {
        match row.kind {
            FieldKind::Choice { options } => {
                cell.spawn(self.text(row.label, size::CAPTION, SUBTLE, 1));
                let current = evaluate(&row.kind, row.text).ok();
                cell.spawn(self.segments()).with_children(|strip| {
                    for (k, option) in options.iter().enumerate() {
                        strip.spawn(self.segment(option, hit(FormHit::Option(i, k)), current == Some(FieldValue::Choice(k)), true));
                    }
                });
            }
            FieldKind::Pick { .. } => {
                cell.spawn(self.text(row.label, size::CAPTION, SUBTLE, 1));
                // RoboCAD's combo box as a wrapped row of chips, the chosen one on.
                if row.picks.is_empty() {
                    cell.spawn(self.text("(nothing to choose from yet)", size::CAPTION, FAINT, 0));
                    return;
                }
                let current = row.text.trim();
                cell.spawn(super::wrap()).with_children(|strip| {
                    for (k, (key, label)) in row.picks.iter().enumerate() {
                        strip.spawn(self.chip(label, hit(FormHit::Option(i, k)), key == current, true));
                    }
                });
                if !current.is_empty() && !row.picks.iter().any(|(key, _)| key == current) {
                    cell.spawn(self.text(format!("{current} is not one of these"), size::CAPTION, DANGER, 0));
                }
            }
            FieldKind::Check => {
                // RoboCAD's checkbox carries its own label ("As live instances").
                cell.spawn(Node { flex_shrink: 0.0, ..default() }).with_children(|line| {
                    line.spawn(self.chip(row.label, hit(FormHit::Check(i)), evaluate(&row.kind, row.text) == Ok(FieldValue::Check(true)), true));
                });
            }
            FieldKind::Number { .. } | FieldKind::Vector { .. } | FieldKind::Json | FieldKind::Text => {
                cell.spawn(self.text(row.label, size::CAPTION, SUBTLE, 1));
                // An empty optional field is left out, not an error.
                if row.optional && row.text.is_empty() {
                    let mut input = cell.spawn(self.input_selectable(row.text, "(optional)", hit(FormHit::Field(i)), row.focused, row.selected));
                    input.insert(AccessibleLabel::new(row.label));
                    return;
                }
                let result = evaluate(&row.kind, row.text);
                {
                    // The kit input labels itself with its text; the field is named by its label.
                    let mut input = cell.spawn(self.input_selectable(row.text, row.label, hit(FormHit::Field(i)), row.focused, row.selected));
                    input.insert(AccessibleLabel::new(row.label));
                    if result.is_err() {
                        // RoboCAD's red border (cad/numeric.rs `body`).
                        input.insert(BorderColor::all(DANGER));
                    }
                }
                let shown = match (&result, row.kind) {
                    (Ok(FieldValue::Number(v)), FieldKind::Number { unit, .. }) => Some(format!("= {}", show(unit, *v))),
                    (Ok(FieldValue::Vector(v)), FieldKind::Vector { unit }) => Some(format!("= {}", v.map(|x| show(unit, x)).join(", "))),
                    _ => None,
                };
                match (result, shown) {
                    (Err(e), _) => {
                        cell.spawn(self.text(e, size::CAPTION, DANGER, 0));
                    }
                    (Ok(_), Some(shown)) => {
                        cell.spawn(self.text(shown, size::CAPTION, SUBTLE, 0));
                    }
                    (Ok(_), None) => {}
                }
            }
        }
    }
}
