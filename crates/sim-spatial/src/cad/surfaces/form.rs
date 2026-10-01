//! The open operation's parameter form (`CadDocument::ops.form`) on the
//! kit's form (`Kit::form`): RoboCAD's `QInputDialog` prompts and its
//! `ArrayDialog` for `Flow::Form` operations, RoboCAD's numeric bar for
//! the pick-then-form and place tools (`EdgeTool`, `ShellTool`,
//! `PrimitiveTool`).
//!
//! - **Placement.** A `Flow::Form` operation's form is modal, as RoboCAD's
//!   dialogs: centred over a dimmed window that takes every click. A pick
//!   or place operation's form (`ops.active` set) sits at the 3D view's top
//!   right, under the command bar, and leaves the view usable: the user
//!   keeps picking. (The numeric bar spans the 3D view's bottom, so the
//!   form cannot sit beside it there.)
//! - **Rows**: the entry's parameters whose `when` holds, in order, each
//!   showing its draft (`FormState::texts`); `FormState::focus` and the
//!   hits here are parameter indices.
//! - **Clicks** (`FormHit`): a text field takes the keyboard (focus is
//!   display state, set here as the numeric bar sets its own); a choice
//!   option or a checkbox writes `CadFormSet { name, value }`; OK writes
//!   `CadFormSubmit`, Cancel `CadFormCancel`.
//! - **Keys** while a field has the keyboard (`CadInputFocus` is set):
//!   typing edits the draft (`TextDraft`: the first character replaces a
//!   selected text), Tab moves to the next text field, Enter writes
//!   `CadFormSubmit`; Escape is `surfaces::input`'s (`CadFormCancel`). A
//!   press elsewhere ends the typing. With no field focused, Enter submits
//!   (RoboCAD's default button, and Enter applies a tool's values), and Tab
//!   focuses the first text field (RoboCAD's "Numeric entry (Tab)") unless
//!   the numeric bar or the name field has the keyboard.
//! - A modal form opens with its first number field focused (as the
//!   catalogue opens it, RoboCAD's dialog); a tool's form opens unfocused,
//!   as RoboCAD's numeric bar does until Tab.
//! - `form.error` (OK's last refusal) shows under the form in the danger
//!   colour; a tool's hint under that.
use super::POPUP_Z;
use super::other_field_focused;
use crate::app::ModeScope;
use crate::app::actions::Act;
use crate::builder::ui_api::Enabled;
use crate::cad::actions::CadAction;
use crate::cad::document::{CadDocument, CadInputFocus};
use crate::cad::ops::{self, FormState, OpEntry};
use crate::cad::panel::{Control, NameDraft};
use crate::ui_kit::form::{DraftKey, FieldKind, FormHit, FormRow, TextDraft, evaluate};
use crate::ui_kit::{BAR, DANGER, Kit, RIGHT_WIDTH, TOPBAR, UiFonts, size};
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use bevy::ui::prelude::AccessibleLabel;
use serde_json::Value;

/// A clickable part of the form, by parameter index.
#[derive(Component, Clone, Copy, Debug)]
pub(super) struct FormPart(pub FormHit);

/// The form's root.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct FormRoot;

/// The form's width (px).
const WIDTH: f32 = 340.0;

/// The parameters shown now: those whose `when` holds against the drafts
/// (a missing draft reads as the default), in order.
pub(crate) fn shown(entry: &OpEntry, texts: &[String]) -> Vec<usize> {
    let text_of = |name: &str| -> Option<&str> {
        let j = entry.params.iter().position(|p| p.name == name)?;
        Some(texts.get(j).map_or(entry.params[j].default, String::as_str))
    };
    (0..entry.params.len()).filter(|i| entry.params[*i].when.is_none_or(|(on, is)| text_of(on) == Some(is))).collect()
}

/// A field typed into (not a choice or a checkbox).
fn is_text(kind: &FieldKind) -> bool {
    matches!(kind, FieldKind::Number { .. } | FieldKind::Vector { .. } | FieldKind::Json | FieldKind::Text)
}

/// The first shown text field of the open form (a parameter index).
pub(crate) fn first_text_field(form: &FormState) -> Option<usize> {
    let entry = ops::entry(form.op)?;
    shown(entry, &form.texts).into_iter().find(|i| is_text(&entry.params[*i].kind))
}

/// Whether OK can be pressed: every shown text field evaluates (an empty
/// optional one is left out, as `ops::values` leaves it out).
fn ok_ready(entry: &OpEntry, form: &FormState) -> Result<(), String> {
    for i in shown(entry, &form.texts) {
        let p = &entry.params[i];
        let text = form.texts.get(i).map_or(p.default, String::as_str);
        if !is_text(&p.kind) || (text.is_empty() && p.default.is_empty()) {
            continue;
        }
        evaluate(&p.kind, text).map_err(|e| format!("{} ({}): {e}", p.label, p.name))?;
    }
    Ok(())
}

/// The open form's `system_ui` controls: `cad:form:ok` (enabled when every
/// field evaluates), `cad:form:cancel`, and `cad:form:set:<name>:<value>`
/// for each shown choice's options and checkbox's states (a name or option
/// with ":" cannot be a control id and is left to REST `cad_form_set`).
pub(crate) fn controls(doc: &CadDocument) -> Vec<Control> {
    let Some(form) = &doc.ops.form else { return Vec::new() };
    let Some(entry) = ops::entry(form.op) else { return Vec::new() };
    let mut out = vec![
        Control { id: "cad:form:ok".into(), label: format!("OK: {}", entry.label), action: CadAction::CadFormSubmit, ready: ok_ready(entry, form) },
        Control { id: "cad:form:cancel".into(), label: format!("Cancel: {}", entry.label), action: CadAction::CadFormCancel, ready: Ok(()) },
    ];
    let fits = |s: &str| !s.is_empty() && !s.contains(':');
    for i in shown(entry, &form.texts) {
        let p = &entry.params[i];
        if !fits(p.name) {
            continue;
        }
        let set = |value: Value| CadAction::CadFormSet { name: p.name.to_string(), value };
        match p.kind {
            FieldKind::Choice { options } => {
                for o in options.iter().filter(|o| fits(o)) {
                    out.push(Control { id: format!("cad:form:set:{}:{o}", p.name), label: format!("{}: {o}", p.label), action: set(Value::String(o.to_string())), ready: Ok(()) });
                }
            }
            FieldKind::Check => {
                for v in [true, false] {
                    out.push(Control { id: format!("cad:form:set:{}:{v}", p.name), label: format!("{}: {}", p.label, if v { "on" } else { "off" }), action: set(Value::Bool(v)), ready: Ok(()) });
                }
            }
            _ => {}
        }
    }
    out
}

/// The next shown text field after `i` (cycling), or `i`.
fn next_text(entry: &OpEntry, texts: &[String], i: usize) -> usize {
    let fields: Vec<usize> = shown(entry, texts).into_iter().filter(|j| is_text(&entry.params[*j].kind)).collect();
    match fields.iter().position(|j| *j == i) {
        Some(at) => fields[(at + 1) % fields.len()],
        None => fields.first().copied().unwrap_or(i),
    }
}

/// Input: the form's clicks and keys (see the module doc).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn input(
    doc: Option<ResMut<CadDocument>>,
    parts: Query<(&Interaction, &FormPart, Option<&Enabled>), Changed<Interaction>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    mut events: MessageReader<KeyboardInput>,
    focus: Option<ResMut<CadInputFocus>>,
    draft: Option<ResMut<NameDraft>>,
    mut out: MessageWriter<Act<CadAction>>,
    mut seen: Local<Option<(&'static str, u64)>>,
) {
    let Some(mut doc) = doc else {
        events.clear();
        return;
    };
    let Some(form) = doc.ops.form.clone() else {
        events.clear();
        *seen = None;
        return;
    };
    let Some(entry) = ops::entry(form.op) else {
        events.clear();
        return;
    };
    // An open command surface has the keyboard.
    if doc.ops.surface.is_some() {
        events.clear();
        return;
    }
    let modal = doc.ops.active.is_none();
    let fields: Vec<usize> = shown(entry, &form.texts).into_iter().filter(|i| is_text(&entry.params[*i].kind)).collect();
    let mut at = form.focus.filter(|i| fields.contains(i));
    let mut select_all = form.select_all;
    let mut texts = form.texts.clone();
    let (mut started, mut ended) = (false, false);
    if *seen != Some((form.op, form.began)) {
        *seen = Some((form.op, form.began));
        // A tool's values are not focused until Tab (RoboCAD's numeric bar).
        if !modal {
            at = None;
        }
        // Keys pressed before the form opened (its own shortcut) are not its text.
        events.clear();
        started = at.is_some();
    }
    let mut pressed_part = false;
    for (interaction, part, enabled) in &parts {
        if *interaction != Interaction::Pressed {
            continue;
        }
        pressed_part = true;
        let param = |i: usize| entry.params.get(i);
        match part.0 {
            FormHit::Field(i) if fields.contains(&i) => {
                at = Some(i);
                select_all = true;
                started = true;
            }
            FormHit::Option(i, k) => {
                if let Some(p) = param(i)
                    && let FieldKind::Choice { options } = p.kind
                    && let Some(o) = options.get(k)
                {
                    out.write(Act::ui(CadAction::CadFormSet { name: p.name.to_string(), value: Value::String(o.to_string()) }));
                }
            }
            FormHit::Check(i) => {
                if let Some(p) = param(i) {
                    let on = texts.get(i).map_or(p.default, String::as_str) == "true";
                    out.write(Act::ui(CadAction::CadFormSet { name: p.name.to_string(), value: Value::Bool(!on) }));
                }
            }
            FormHit::Ok if enabled.is_none_or(|e| e.0) => {
                out.write(Act::ui(CadAction::CadFormSubmit));
            }
            FormHit::Cancel => {
                out.write(Act::ui(CadAction::CadFormCancel));
            }
            _ => {}
        }
    }
    if at.is_some() && !pressed_part && !started && buttons.as_ref().is_some_and(|b| b.just_pressed(MouseButton::Left)) {
        // A press elsewhere ends the typing.
        at = None;
        ended = true;
    }
    let others = other_field_focused(&doc, draft.as_deref()) || focus.as_ref().is_some_and(|f| f.0);
    let tab = keys.as_ref().is_some_and(|k| k.just_pressed(KeyCode::Tab));
    if at.is_none() && !started && tab && !others
        && let Some(first) = fields.first()
    {
        // RoboCAD's "Numeric entry (Tab)" with a form open.
        at = Some(*first);
        select_all = true;
        started = true;
        events.clear();
    }
    if started {
        // Typing here: the numeric bar's entry and the name draft end.
        if doc.tool_state.numeric.focus.is_some() {
            doc.tool_state.numeric.focus = None;
            doc.tool_state.numeric.began = None;
        }
        if let Some(mut draft) = draft
            && draft.editing.is_some()
        {
            draft.editing = None;
            draft.refusal = None;
        }
        events.clear();
    } else if let Some(mut i) = at {
        let chord = keys.as_ref().is_some_and(|k| k.any_pressed([KeyCode::SuperLeft, KeyCode::SuperRight, KeyCode::ControlLeft, KeyCode::ControlRight]));
        let typed: Vec<KeyboardInput> = events.read().filter(|e| e.state == ButtonState::Pressed).cloned().collect();
        for e in typed {
            let mut d = TextDraft { text: texts.get(i).cloned().unwrap_or_default(), select_all };
            match d.key(&e.logical_key, chord) {
                DraftKey::Edited => {
                    if let Some(t) = texts.get_mut(i) {
                        *t = d.text;
                    }
                    select_all = d.select_all;
                }
                DraftKey::Enter => {
                    out.write(Act::ui(CadAction::CadFormSubmit));
                    break;
                }
                DraftKey::Tab => {
                    i = next_text(entry, &texts, i);
                    select_all = true;
                }
                // Escape: `surfaces::input` cancels the form.
                DraftKey::Escape => {
                    ended = true;
                    break;
                }
                DraftKey::Ignored => {}
            }
        }
        at = Some(i);
    } else {
        events.clear();
        if keys.as_ref().is_some_and(|k| k.just_pressed(KeyCode::Enter)) && !others {
            out.write(Act::ui(CadAction::CadFormSubmit));
        }
    }
    if (at != form.focus || select_all != form.select_all || texts != form.texts)
        && let Some(f) = doc.ops.form.as_mut()
        && f.op == form.op
    {
        f.focus = at;
        f.select_all = select_all;
        f.texts = texts;
    }
    if (at.is_some() || started || ended)
        && let Some(mut focus) = focus
        && !focus.0
    {
        focus.0 = true;
    }
}

/// What the form shows, as a comparable text.
fn form_key(doc: &CadDocument) -> Option<String> {
    let form = doc.ops.form.as_ref()?;
    Some(format!("{:?}", (doc.generation, form, doc.ops.active)))
}

/// Present: the form, rebuilt when its drafts, focus or error change;
/// despawned when it closes.
pub(super) fn draw(mut commands: Commands, doc: Option<Res<CadDocument>>, fonts: Res<UiFonts>, roots: Query<Entity, With<FormRoot>>, mut last: Local<Option<String>>) {
    let key = doc.as_deref().and_then(form_key);
    let shown_now = roots.iter().next().is_some();
    if key == *last && shown_now == key.is_some() {
        return;
    }
    *last = key;
    for root in &roots {
        commands.entity(root).despawn();
    }
    let Some(doc) = doc.as_deref() else { return };
    let Some(form) = &doc.ops.form else { return };
    let Some(entry) = ops::entry(form.op) else { return };
    let k = Kit::new(&fonts);
    let rows_at = shown(entry, &form.texts);
    let rows: Vec<FormRow> = rows_at
        .iter()
        .map(|i| {
            let p = &entry.params[*i];
            let focused = form.focus == Some(*i);
            // An empty default is optional (`ok_ready` and `ops::values` leave it out empty).
            FormRow { label: p.label, kind: p.kind, text: form.texts.get(*i).map_or(p.default, String::as_str), focused, optional: p.default.is_empty(), selected: focused && form.select_all }
        })
        .collect();
    let param = |r: usize| rows_at.get(r).copied().unwrap_or(usize::MAX);
    let hit = |h: FormHit| {
        FormPart(match h {
            FormHit::Field(r) => FormHit::Field(param(r)),
            FormHit::Option(r, o) => FormHit::Option(param(r), o),
            FormHit::Check(r) => FormHit::Check(param(r)),
            other => other,
        })
    };
    let ok = ok_ready(entry, form).is_ok();
    let modal = doc.ops.active.is_none();
    // The kit's form panel is the frame; the root here only places it.
    if modal {
        // RoboCAD's dialogs are modal: the dimmed window takes every click.
        commands
            .spawn((
                Node { position_type: PositionType::Absolute, left: Val::Px(0.0), right: Val::Px(0.0), top: Val::Px(0.0), bottom: Val::Px(0.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
                BackgroundColor(BAR.with_alpha(0.55)),
                FocusPolicy::Block,
                GlobalZIndex(POPUP_Z - 2),
                AccessibleLabel::new(format!("{} parameters", entry.label)),
                FormRoot,
                DespawnOnExit(ModeScope::Cad),
            ))
            .with_children(|backdrop| body(backdrop, &k, entry, &rows, ok, &hit, form.error.as_ref(), modal));
    } else {
        commands
            .spawn((
                Node { position_type: PositionType::Absolute, right: Val::Px(RIGHT_WIDTH + 8.0), top: Val::Px(TOPBAR + super::COMMAND_BAR + 8.0), ..default() },
                FocusPolicy::Block,
                GlobalZIndex(POPUP_Z - 3),
                AccessibleLabel::new(format!("{} values", entry.label)),
                FormRoot,
                DespawnOnExit(ModeScope::Cad),
            ))
            .with_children(|p| body(p, &k, entry, &rows, ok, &hit, form.error.as_ref(), modal));
    }
}

/// The form's content: the kit form, with OK's last refusal and a tool's
/// hint in its panel under the buttons.
#[allow(clippy::too_many_arguments)]
fn body(p: &mut ChildSpawnerCommands, k: &Kit, entry: &OpEntry, rows: &[FormRow], ok: bool, hit: &impl Fn(FormHit) -> FormPart, error: Option<&String>, modal: bool) {
    k.form(p, entry.label, rows, ok, Some(WIDTH), hit, |p| {
        if let Some(error) = error {
            p.spawn(k.text(error.clone(), size::SMALL, DANGER, 0));
        }
        if !modal && !entry.hint.is_empty() {
            p.spawn(k.note(entry.hint));
        }
    });
}
