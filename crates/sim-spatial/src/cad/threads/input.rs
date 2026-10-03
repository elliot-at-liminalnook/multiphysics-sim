//! The Comments dock's typing and Escape, on the kit's one text field
//! (`ui_kit::text`): three fields, each mirrored into `ThreadsState` so the
//! dock redraws (as the materials panel mirrors its search).
//!
//! - **The composer** ([`COMPOSE`]; RoboCAD's `QPlainTextEdit`, "Write a
//!   reply…"): Enter posts (Shift+Enter types a newline), Escape cancels the
//!   draft (`cancel_draft`). A press on its area gives it the keyboard with
//!   the draft; Reply / Post annotation / Save edit write the action the
//!   draft stands for (`threads::submit_action`), Cancel writes `discard`.
//!   The draft stays until RoboCAD takes it (`threads::edit_answered` asks
//!   for the keyboard back, `ThreadsState::release`); a handler that fills
//!   it (Edit message, Insert part link, a placed pin) asks for the
//!   keyboard (`ThreadsState::claim`).
//! - **The author** ([`AUTHOR`], "You", "Your display name") and **the part
//!   label** ([`LABEL`], the inline "Part label for this discussion"
//!   dialog: Enter is OK, Escape Cancel).
//! - **Escape** ([`escape`]): with Annotate or Reattach active it ends the
//!   tool; with linked parts shown alone it is Return to assembly. It runs
//!   in the public `CadKeySet::Escape`, after the two-step key gate and
//!   after `CadKeySet::EscapeTool` (the reference calibrate tool's Escape,
//!   which consumes the key while that tool is on), before the Select tool's
//!   Escape (`CadKeySet::ToolKeys`, `transform::keys`), and consumes the
//!   key then, and stands aside while a command surface or form is open (its
//!   own Escape) or the keyboard is held (`keys::free`).
//!
//! Every system here runs in CAD mode only, so no field of the dock can
//! take the keyboard in another mode.
use super::dock::ThreadsInput;
use super::{Field, ThreadsArgs, ThreadsOp, submit_action};
use crate::app::actions::Act;
use crate::app::{ViewerMode};
use crate::cad::actions::CadAction;
use crate::cad::document::CadDocument;
use crate::ui_kit::text::{EnterKey, FieldEvent, FieldId, FieldMsg, TextDraft, TextField, TextFocus};
use bevy::prelude::*;

/// The composer.
pub(in crate::cad) const COMPOSE: FieldId = FieldId("cad.threads.compose");
/// The author.
pub(in crate::cad) const AUTHOR: FieldId = FieldId("cad.threads.author");
/// The part label dialog's field.
pub(in crate::cad) const LABEL: FieldId = FieldId("cad.threads.label");

fn field_of(id: FieldId) -> Option<Field> {
    match id {
        COMPOSE => Some(Field::Compose),
        AUTHOR => Some(Field::Author),
        LABEL => Some(Field::Label),
        _ => None,
    }
}

fn id_of(f: Field) -> FieldId {
    match f {
        Field::Compose => COMPOSE,
        Field::Author => AUTHOR,
        Field::Label => LABEL,
    }
}

/// The state's text of field `f`.
fn text_of(doc: &CadDocument, f: Field) -> String {
    let st = &doc.threads;
    match f {
        Field::Compose => st.compose.clone(),
        Field::Author => st.author.clone(),
        Field::Label => st.label.as_ref().map(|l| l.text.clone()).unwrap_or_default(),
    }
}

/// Write `action` as a click.
fn write(out: &mut MessageWriter<Act<CadAction>>, action: CadAction) {
    out.write(Act::ui(action));
}

/// The composer's post: the action its draft stands for, unless empty.
fn post(doc: &CadDocument, out: &mut MessageWriter<Act<CadAction>>) {
    if doc.threads.compose.trim().is_empty() {
        return;
    }
    if let Some((action, _)) = submit_action(doc) {
        write(out, action);
    }
}

/// Input: the dock's field messages and presses (see the module doc).
#[allow(clippy::type_complexity)]
fn input(doc: Option<ResMut<CadDocument>>, presses: Query<&ThreadsInput, With<crate::ui_kit::activation::Activated>>, mut msgs: MessageReader<FieldMsg>, mut text: TextFocus, mut out: MessageWriter<Act<CadAction>>) {
    let Some(mut doc) = doc else {
        msgs.clear();
        return;
    };
    let messages: Vec<FieldMsg> = msgs.read().filter(|m| field_of(m.field).is_some()).cloned().collect();
    if [COMPOSE, AUTHOR, LABEL].into_iter().any(|id| text.suspended(id)) {
        return;
    }
    let pressed: Vec<ThreadsInput> = presses.iter().map(|a| *a).collect();
    let kit_focus = [Field::Compose, Field::Author, Field::Label].into_iter().find(|f| text.focused(id_of(*f)));
    // Read first: a `ResMut` deref would mark the document changed every frame.
    {
        let st = &doc.threads;
        if messages.is_empty() && pressed.is_empty() && st.claim.is_none() && !st.release && st.focus == kit_focus {
            return;
        }
    }
    let mut edited = false;
    for m in messages {
        match (m.field, m.event) {
            (COMPOSE, FieldEvent::Changed(d)) => {
                if doc.threads.compose != d.text {
                    doc.threads.compose = d.text;
                    doc.threads.error = None;
                    edited = true;
                }
            }
            (COMPOSE, FieldEvent::Submit(typed)) => {
                if doc.threads.compose != typed {
                    doc.threads.compose = typed;
                    edited = true;
                }
                post(&doc, &mut out);
                text.blur(COMPOSE);
            }
            // The kit has taken the keyboard already.
            (COMPOSE, FieldEvent::Cancel) => write(&mut out, ThreadsArgs::of(ThreadsOp::Discard).action()),
            (AUTHOR, FieldEvent::Changed(d)) => {
                if doc.threads.author != d.text {
                    doc.threads.author = d.text;
                    edited = true;
                }
            }
            (AUTHOR, FieldEvent::Submit(_)) => text.blur(AUTHOR),
            (LABEL, FieldEvent::Changed(d)) => {
                if let Some(l) = doc.threads.label.as_mut()
                    && l.text != d.text
                {
                    l.text = d.text;
                    edited = true;
                }
            }
            (LABEL, FieldEvent::Submit(typed)) => {
                if let Some(l) = doc.threads.label.clone() {
                    write(&mut out, ThreadsArgs { op: ThreadsOp::Label, thread: Some(l.thread), node: Some(l.node), label: Some(typed), ..ThreadsArgs::default() }.action());
                }
                text.blur(LABEL);
            }
            (LABEL, FieldEvent::Cancel) => write(&mut out, ThreadsArgs::of(ThreadsOp::LabelCancel).action()),
            _ => {}
        }
    }
    for p in pressed {
        match p {
            ThreadsInput::Compose => {
                text.focus_draft(COMPOSE, TextDraft::new(text_of(&doc, Field::Compose), false));
            }
            ThreadsInput::Author => {
                text.focus_draft(AUTHOR, TextDraft::new(text_of(&doc, Field::Author), false));
            }
            ThreadsInput::Label => {
                text.focus_draft(LABEL, TextDraft::new(text_of(&doc, Field::Label), false));
            }
            ThreadsInput::Submit => {
                post(&doc, &mut out);
                text.blur(COMPOSE);
            }
            ThreadsInput::Cancel => {
                write(&mut out, ThreadsArgs::of(ThreadsOp::Discard).action());
                text.blur(COMPOSE);
            }
        }
    }
    // The draft was posted or cancelled: the composer lets the keyboard go.
    if doc.threads.release {
        doc.threads.release = false;
        text.blur(COMPOSE);
        text.set(COMPOSE, TextDraft::default());
    }
    // A handler filled a field (Edit message, Insert part link, a placed
    // pin, the label dialog): it takes the keyboard with that text.
    if let Some(f) = doc.threads.claim.take() {
        let open = match f {
            Field::Label => doc.threads.label.is_some(),
            Field::Compose | Field::Author => true,
        };
        if open {
            text.focus_draft(id_of(f), TextDraft::new(text_of(&doc, f), f == Field::Label));
        }
    }
    // The label dialog closed: its field lets the keyboard go.
    if doc.threads.label.is_none() {
        text.blur(LABEL);
    }
    // The state is the draft's record: a focused field shows it (a REST
    // `draft` while typing, an insert).
    if let Some(f) = [Field::Compose, Field::Author, Field::Label].into_iter().find(|f| text.focused(id_of(*f))) {
        let want = text_of(&doc, f);
        if text.draft(id_of(f)).is_some_and(|d| d.text != want) {
            text.set(id_of(f), TextDraft::new(want, false));
        }
    }
    let focus = [Field::Compose, Field::Author, Field::Label].into_iter().find(|f| text.focused(id_of(*f)));
    if doc.threads.focus != focus {
        doc.threads.focus = focus;
        edited = true;
    }
    if edited {
        doc.touch();
    }
}

/// Input: Escape ends Annotate or returns to the assembly (see the module doc).
/// An open file form (`files::form`) takes Escape first: it closes itself.
fn escape(keys: Option<ResMut<ButtonInput<KeyCode>>>, doc: Option<Res<CadDocument>>, files: Option<Res<crate::cad::files::CadFiles>>, mut out: MessageWriter<Act<CadAction>>) {
    let (Some(mut keys), Some(doc)) = (keys, doc) else { return };
    let file_form = files.as_ref().is_some_and(|f| f.form.is_some());
    if !keys.just_pressed(KeyCode::Escape) || doc.ops.surface.is_some() || doc.ops.form.is_some() || file_form {
        return;
    }
    let op = if doc.threads.tool.is_some() {
        ThreadsOp::Cancel
    } else if doc.threads.isolation.is_some() {
        ThreadsOp::Return
    } else {
        return;
    };
    keys.clear_just_pressed(KeyCode::Escape);
    write(&mut out, ThreadsArgs::of(op).action());
}

/// CadPlugin: the three fields and the dock's input (Input).
pub(super) fn build(app: &mut App) {
    use crate::ui_kit::text::TextFieldApp;
    app.add_text_field(COMPOSE, TextField::new("Annotation message").placeholder("Write a reply…").enter(EnterKey::ShiftNewline))
        .add_text_field(AUTHOR, TextField::new("Comment author").placeholder("Your display name"))
        .add_text_field(LABEL, TextField::new("Plain-language label").select_on_focus())
        .add_systems(
            Update,
            (
                // Before CAD's keys: a field given the keyboard this frame holds the frame's keys.
                input.in_set(crate::cad::CadKeySet::Focus),
                // The public Escape step: after `EscapeTool` (the calibrate tool ends first) and
                // before the Select tool's (`ToolKeys`); it consumes the key when it acts.
                escape.run_if(crate::cad::keys::free).in_set(crate::cad::CadKeySet::Escape),
            )
                .run_if(in_state(ViewerMode::Cad)),
        );
}
