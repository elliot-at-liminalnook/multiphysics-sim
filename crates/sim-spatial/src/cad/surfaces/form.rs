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
//!   option, a pick's choice (its key: a body, joint or motor id from
//!   `ops::picks`, "" for "(world)" or "(none)") or a checkbox writes
//!   `CadFormSet { name, value }`; OK writes `CadFormSubmit`, Cancel
//!   `CadFormCancel`.
//! - **Keys** while a field has the keyboard (the form's kit field,
//!   [`FORM`], editing the row `FormState::focus` names): typing edits the
//!   row's draft (the first character replaces a selected text), Tab moves
//!   to the next text field, Enter writes `CadFormSubmit`, Escape writes
//!   `CadFormCancel`. A press elsewhere ends the typing; so does another
//!   field taking the keyboard. With no field focused, Enter submits
//!   (RoboCAD's default button, and Enter applies a tool's values), and Tab
//!   focuses the first text field (RoboCAD's "Numeric entry (Tab)") unless
//!   the numeric bar or the name field has the keyboard. A sketch tool's
//!   form is the exception: Enter outside its fields does nothing here
//!   (RoboCAD's sketch tools take Enter only to finish a spline,
//!   `sketch::interact`), and the field its start focused (the text tool's
//!   "Text to sketch:") keeps the keyboard.
//! - A modal form opens with its first number field focused (as the
//!   catalogue opens it, RoboCAD's dialog); a tool's form opens unfocused,
//!   as RoboCAD's numeric bar does until Tab.
//! - `form.error` (OK's last refusal) shows under the form in the danger
//!   colour; a tool's hint under that; the Add motor form's notes line
//!   (`ops::robot_form::note`) under the fields. A pick's choices are read
//!   from the document each time the form is drawn, and the form is
//!   rebuilt when they change (the motor library or the tree arriving).
use super::POPUP_Z;
use crate::app::ModeScope;
use crate::app::actions::Act;
use crate::builder::ui_api::Enabled;
use crate::cad::actions::CadAction;
use crate::cad::document::CadDocument;
use crate::cad::ops::{self, FormState, OpEntry};
use crate::cad::panel::Control;
use crate::ui_kit::form::{FieldKind, FormHit, FormRow, evaluate};
use crate::ui_kit::text::{FieldEvent, FieldId, FieldMsg, TextDraft, TextFocus};
use crate::ui_kit::{BAR, DANGER, Kit, RIGHT_WIDTH, TOPBAR, UiFonts, size};
use bevy::ecs::system::ParamSet;
use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use bevy::ui::prelude::AccessibleLabel;
use serde_json::Value;

/// The form's text field (`ui_kit::text`): the row it edits is
/// `FormState::focus`. Sticky: a press on the form's own options and
/// buttons keeps the typing; `input` ends it on a press elsewhere.
pub(in crate::cad) const FORM: FieldId = FieldId("cad.form");

/// A clickable part of the form, by parameter index.
#[derive(Component, Clone, Copy, Debug)]
pub(super) struct FormPart(pub FormHit);

/// The form's root.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(in crate::cad) struct FormRoot;

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
/// for each shown choice's options, pick's choices (by key; "-" for the
/// empty key of "(world)" or "(none)") and checkbox's states (a name or
/// option with ":" cannot be a control id and is left to REST `cad_form_set`).
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
            FieldKind::Pick { source } => {
                for (key, label) in ops::picks(source, doc) {
                    let token = if key.is_empty() { "-".to_string() } else { key.clone() };
                    if fits(token.as_str()) {
                        out.push(Control { id: format!("cad:form:set:{}:{token}", p.name), label: format!("{}: {label}", p.label), action: set(Value::String(key)), ready: Ok(()) });
                    }
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

/// Input: the form's clicks, its field's events and the keys it reads
/// with no field typing (see the module doc).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn input(
    doc: Option<ResMut<CadDocument>>,
    parts: Query<(&FormPart, Option<&Enabled>), With<crate::ui_kit::activation::Activated>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    // The field's messages are read first, then `TextFocus` acts (a `ParamSet`: one at a time).
    mut field: ParamSet<(MessageReader<FieldMsg>, TextFocus)>,
    mut out: MessageWriter<Act<CadAction>>,
    mut seen: Local<Option<(&'static str, u64)>>,
) {
    let events: Vec<FieldEvent> = field.p0().read().filter(|m| m.field == FORM).map(|m| m.event.clone()).collect();
    let mut text = field.p1();
    let Some(mut doc) = doc else {
        text.blur(FORM);
        return;
    };
    let Some(form) = doc.ops.form.clone() else {
        *seen = None;
        text.blur(FORM);
        return;
    };
    let Some(entry) = ops::entry(form.op) else {
        text.blur(FORM);
        return;
    };
    // An open command surface has the keyboard: the field gives it up and
    // takes it back (its row kept) once the surface closes.
    if doc.ops.surface.is_some() {
        text.blur(FORM);
        return;
    }
    let modal = doc.ops.active.is_none();
    let sketching = matches!(entry.flow, ops::Flow::Sketch(_));
    let fields: Vec<usize> = shown(entry, &form.texts).into_iter().filter(|i| is_text(&entry.params[*i].kind)).collect();
    let mut at = form.focus.filter(|i| fields.contains(i));
    let mut select_all = form.select_all;
    let mut texts = form.texts.clone();
    let mut started = false;
    // A form new this frame: the field's events are the previous form's
    // draft (still focused when an OK opened this one), not its rows'.
    let fresh = *seen != Some((form.op, form.began));
    if fresh {
        *seen = Some((form.op, form.began));
        // A tool's values are not focused until Tab (RoboCAD's numeric bar),
        // except the field a sketch tool's start focused: the text tool's
        // "Text to sketch:" (RoboCAD's `getText` dialog comes first;
        // `sketch::interact::begin`).
        if !modal && !sketching {
            at = None;
        }
        started = at.is_some();
    }
    // The field's events (the kit typed this frame's keys before this runs).
    for event in events.into_iter().filter(|_| !fresh) {
        match event {
            FieldEvent::Changed(draft) => {
                if let Some(t) = at.and_then(|i| texts.get_mut(i)) {
                    *t = draft.text;
                    select_all = draft.select_all;
                }
            }
            // The text tool's "Text to sketch:" is RoboCAD's `getText`
            // dialog: its OK starts the tool's clicks (the text is placed
            // where the user clicks), it does not place the text.
            FieldEvent::Submit(_) if sketching && at.and_then(|i| entry.params.get(i)).is_some_and(|p| p.kind == FieldKind::Text) => {
                at = None;
                text.blur(FORM);
            }
            // OK; the field keeps the keyboard until the form closes (a
            // refusal shows under it and the typing goes on).
            FieldEvent::Submit(_) => {
                out.write(Act::ui(crate::cad::activation::guard(&doc, CadAction::CadFormSubmit)));
            }
            // Escape cancels the form (the kit has taken the keyboard away).
            FieldEvent::Cancel => {
                at = None;
                out.write(Act::ui(crate::cad::activation::guard(&doc, CadAction::CadFormCancel)));
            }
            FieldEvent::Tab { .. } => {
                if let Some(i) = at {
                    let next = next_text(entry, &texts, i);
                    at = Some(next);
                    select_all = true;
                    text.focus_draft(FORM, TextDraft::new(texts.get(next).cloned().unwrap_or_default(), true));
                }
            }
            // Another field took the keyboard, or a mode switch.
            FieldEvent::Blur => at = None,
            FieldEvent::Arrow { .. } => {}
        }
    }
    let mut pressed_part = false;
    for (part, enabled) in &parts {
        pressed_part = true;
        let param = |i: usize| entry.params.get(i);
        match part.0 {
            FormHit::Field(i) if fields.contains(&i) => {
                at = Some(i);
                select_all = true;
                started = true;
                text.focus_draft(FORM, TextDraft::new(texts.get(i).cloned().unwrap_or_default(), true));
            }
            FormHit::Option(i, k) => match param(i).map(|p| (p, p.kind)) {
                Some((p, FieldKind::Choice { options })) => {
                    if let Some(o) = options.get(k) {
                        out.write(Act::ui(crate::cad::activation::guard(&doc, CadAction::CadFormSet { name: p.name.to_string(), value: Value::String(o.to_string()) })));
                    }
                }
                // The choice's key, as the form showed the list (read again now: the same document).
                Some((p, FieldKind::Pick { source })) => {
                    if let Some((key, _)) = ops::picks(source, &doc).into_iter().nth(k) {
                        out.write(Act::ui(crate::cad::activation::guard(&doc, CadAction::CadFormSet { name: p.name.to_string(), value: Value::String(key) })));
                    }
                }
                _ => {}
            },
            FormHit::Check(i) => {
                if let Some(p) = param(i) {
                    let on = texts.get(i).map_or(p.default, String::as_str) == "true";
                    out.write(Act::ui(crate::cad::activation::guard(&doc, CadAction::CadFormSet { name: p.name.to_string(), value: Value::Bool(!on) })));
                }
            }
            FormHit::Ok if enabled.is_none_or(|e| e.0) => {
                out.write(Act::ui(crate::cad::activation::guard(&doc, CadAction::CadFormSubmit)));
            }
            FormHit::Cancel => {
                out.write(Act::ui(crate::cad::activation::guard(&doc, CadAction::CadFormCancel)));
            }
            _ => {}
        }
    }
    // A press elsewhere ends the typing (the field is sticky, so a press on
    // the form's own options, checkboxes and buttons keeps it, as before).
    if at.is_some() && !pressed_part && !started && buttons.as_ref().is_some_and(|b| b.just_pressed(MouseButton::Left)) {
        at = None;
    }
    // With no field typing, the keys reach `ButtonInput` (a typing field's
    // are consumed by the kit, the numeric bar's and the name field's too).
    let free = !text.typing();
    let tab = keys.as_ref().is_some_and(|k| k.just_pressed(KeyCode::Tab));
    if at.is_none() && !started && tab && free
        && let Some(first) = fields.first()
    {
        // RoboCAD's "Numeric entry (Tab)" with a form open.
        at = Some(*first);
        select_all = true;
        started = true;
    }
    // A sketch tool's Enter outside its fields is not OK: RoboCAD's
    // `SketchTool.key` takes Enter only to finish a spline
    // (`sketch::interact`), and its Tab values commit from the numeric bar.
    if at.is_none() && !started && free && !sketching && keys.as_ref().is_some_and(|k| k.just_pressed(KeyCode::Enter)) {
        out.write(Act::ui(crate::cad::activation::guard(&doc, CadAction::CadFormSubmit)));
    }
    // The kit field follows the row: it takes the keyboard for a row asked
    // for here or by an action (the catalogue's first field, `numeric.entry`,
    // the text tool's start), gives it up without one, and shows the row's
    // text when it changed from outside (`CadFormSet`).
    match at {
        Some(i) => {
            let want = TextDraft::new(texts.get(i).cloned().unwrap_or_default(), select_all);
            if !text.focused(FORM) {
                text.focus_draft(FORM, want);
            } else if text.draft(FORM) != Some(&want) {
                text.set(FORM, want);
            }
        }
        None => text.blur(FORM),
    }
    if (at != form.focus || select_all != form.select_all || texts != form.texts)
        && let Some(f) = doc.ops.form.as_mut()
        && f.op == form.op
    {
        f.focus = at;
        f.select_all = select_all;
        f.texts = texts;
    }
}

/// The shown pick fields' choices (by parameter index; empty for other
/// kinds) and the form's notes line, from the document now.
fn lists(doc: &CadDocument, entry: &OpEntry, form: &FormState) -> (Vec<Vec<(String, String)>>, Option<String>) {
    let picks = entry.params.iter().map(|p| if let FieldKind::Pick { source } = p.kind { ops::picks(source, doc) } else { Vec::new() }).collect();
    (picks, ops::note(entry, &form.texts, doc))
}

/// What the form shows, as a comparable text (with the picks' choices and
/// the notes line, which change as RoboCAD's answers arrive).
fn form_key(doc: &CadDocument) -> Option<String> {
    let form = doc.ops.form.as_ref()?;
    let entry = ops::entry(form.op)?;
    Some(format!("{:?}", (doc.generation, doc.ops.form_sequence, form, doc.ops.active, lists(doc, entry, form))))
}

/// Present: the form, rebuilt when its drafts, focus or error change;
/// despawned when it closes.
pub(super) fn draw(mut commands: Commands, doc: Option<Res<CadDocument>>, fonts: Res<UiFonts>, roots: Query<Entity, With<FormRoot>>, mut last: Local<Option<String>>) {
    let key = doc.as_deref().and_then(|d| form_key(d).map(|key| format!("{key}|source={}", crate::cad::activation::render_key(d))));
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
    let (picks, note) = lists(doc, entry, form);
    let rows: Vec<FormRow> = rows_at
        .iter()
        .map(|i| {
            let p = &entry.params[*i];
            let focused = form.focus == Some(*i);
            // An empty default is optional (`ok_ready` and `ops::values` leave it out empty).
            FormRow { label: p.label, kind: p.kind, text: form.texts.get(*i).map_or(p.default, String::as_str), focused, optional: p.default.is_empty(), selected: focused && form.select_all, picks: picks.get(*i).map_or(&[][..], Vec::as_slice) }
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
        // RoboCAD's dialogs are modal: the dimmed window takes every click,
        // the switcher strip's too (z 42 is above the strip's 40). A switch
        // would lose the dialog's typed values, and the switch's
        // `leaving_blockers` do not check open forms.
        commands
            .spawn((
                Node { position_type: PositionType::Absolute, left: Val::Px(0.0), right: Val::Px(0.0), top: Val::Px(0.0), bottom: Val::Px(0.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
                BackgroundColor(BAR.with_alpha(0.55)),
                FocusPolicy::Block,
                GlobalZIndex(POPUP_Z - 2),
                AccessibleLabel::new(format!("{} parameters", entry.label)),
                crate::ui_kit::activation::ModalFocus,
                bevy::input_focus::tab_navigation::TabGroup::modal(),
                FormRoot,
                DespawnOnExit(ModeScope::Cad),
            ))
            .with_children(|backdrop| body(backdrop, &k, entry, &rows, ok, &hit, (form.error.as_ref(), note.as_deref()), modal));
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
            .with_children(|p| body(p, &k, entry, &rows, ok, &hit, (form.error.as_ref(), note.as_deref()), modal));
    }
}

/// The form's content: the kit form, with OK's last refusal, the notes
/// line and a tool's hint in its panel under the buttons.
#[allow(clippy::too_many_arguments)]
fn body(p: &mut ChildSpawnerCommands, k: &Kit, entry: &OpEntry, rows: &[FormRow], ok: bool, hit: &impl Fn(FormHit) -> FormPart, (error, note): (Option<&String>, Option<&str>), modal: bool) {
    k.form(p, entry.label, rows, ok, Some(WIDTH), hit, |p| {
        if let Some(note) = note {
            p.spawn(k.note(note.to_string()));
        }
        if let Some(error) = error {
            p.spawn(k.text(error.clone(), size::SMALL, DANGER, 0));
        }
        if !modal && !entry.hint.is_empty() {
            p.spawn(k.note(entry.hint));
        }
    });
}
