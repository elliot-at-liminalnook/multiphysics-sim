//! The physical rows' typing (Input), the editors' pattern
//! (`editors::entry`): a press on a row's field opens a draft of its value
//! (selected, so typing replaces it); Enter checks it as the row reads it
//! (`physical_edit::parse_color`, `joint_override`) and writes one
//! `CadInspector` stamped with the revision the draft opened at (an
//! unchanged value sends nothing); an error, or an edit that cannot be
//! sent now, keeps the draft open with the reason. Escape, a press
//! elsewhere, another node selected, or another field taking the keyboard
//! (the name field, the editors, the materials panel, which ends this one
//! when it starts) ends it. One field holds the keyboard: opening a draft
//! ends the name field's, the editors', the numeric bar's and the materials
//! panel's typing. `CadInputFocus` is set while a draft is open and in the
//! frame it ends.
use super::physical_edit::{InspectorArgs, InspectorOp, RowDraft, RowField, joint_override, parse_color};
use super::rows::{PhysicalField, current_text};
use crate::app::actions::Act;
use crate::cad::actions::CadAction;
use crate::cad::document::{CadDocument, CadInputFocus};
use crate::cad::panel::NameDraft;
use crate::cad::selection::{CadItems, CadSelection};
use crate::ui_kit::form::{DraftKey, TextDraft};
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;

/// The action a draft's Enter writes: None when the value is unchanged,
/// else the checked edit; Err why nothing is sent (shown under the field).
pub(crate) fn submit(doc: &CadDocument, d: &RowDraft) -> Result<Option<CadAction>, String> {
    if super::node(doc, &d.node).is_none() {
        return Err("This node is no longer in RoboCAD's document; Escape.".into());
    }
    let text = d.draft.text.trim();
    if text == current_text(doc, &d.node, d.field).trim() {
        return Ok(None);
    }
    let args = match d.field {
        RowField::Color => InspectorArgs { op: InspectorOp::Color, id: Some(d.node.clone()), color: Some(parse_color(text)?), revision: Some(d.began), ..InspectorArgs::default() },
        RowField::Joint(f) => {
            // Checked here so the error stays under the field.
            joint_override(f, text)?;
            InspectorArgs { op: InspectorOp::JointPhysics, id: Some(d.node.clone()), field: Some(f), value: Some(text.to_string()), revision: Some(d.began), ..InspectorArgs::default() }
        }
    };
    if let Some(why) = doc.commit_refusal(Some(d.began)) {
        return Err(format!("Not sent: {why}. Enter again once it clears, or Escape and reopen the field for the current values."));
    }
    Ok(Some(CadAction::CadInspector(args)))
}

/// Input: the physical rows' drafts (see the module doc).
#[allow(clippy::too_many_arguments)]
pub(in crate::cad) fn entry(
    doc: Option<ResMut<CadDocument>>,
    presses: Query<(&Interaction, &PhysicalField), Changed<Interaction>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    mut events: MessageReader<KeyboardInput>,
    focus: Option<ResMut<CadInputFocus>>,
    name: Option<ResMut<NameDraft>>,
    mut out: MessageWriter<Act<CadAction>>,
    selection: CadSelection,
) {
    let Some(mut doc) = doc else {
        events.clear();
        return;
    };
    let before = doc.physical_edit.draft.clone();
    let mut draft = before.clone();
    let (mut started, mut ended, mut on_field) = (false, false, false);
    for (interaction, f) in &presses {
        if *interaction != Interaction::Pressed {
            continue;
        }
        on_field = true;
        if draft.as_ref().is_some_and(|d| d.node == f.node && d.field == f.field) {
            continue;
        }
        let text = current_text(&doc, &f.node, f.field);
        draft = Some(RowDraft { node: f.node.clone(), field: f.field, draft: TextDraft { text, select_all: true }, error: None, began: doc.shown_revision() });
        started = true;
    }
    let naming = name.as_ref().is_some_and(|d| d.editing.is_some());
    if started {
        // One field holds the keyboard.
        if doc.tool_state.numeric.focus.is_some() {
            doc.tool_state.numeric.focus = None;
            doc.tool_state.numeric.began = None;
        }
        if doc.tool_state.inspector_edit.is_some() {
            doc.tool_state.inspector_edit = None;
        }
        if doc.materials.typing() {
            doc.materials.end_typing();
        }
        if naming && let Some(mut name) = name {
            name.editing = None;
            name.refusal = None;
        }
    } else if let Some(node) = draft.as_ref().map(|d| d.node.clone()) {
        let selected = selection.items().first_node().map(str::to_string);
        let elsewhere = !on_field && buttons.as_ref().is_some_and(|b| b.just_pressed(MouseButton::Left));
        // The editors opened a draft since (they run after this system).
        let editors = doc.tool_state.inspector_edit.is_some();
        if naming || elsewhere || editors || selected.as_deref() != Some(node.as_str()) {
            draft = None;
            ended = true;
        }
    }
    if started || draft.is_none() || doc.ops.surface.is_some() {
        // Keys pressed before the field took the keyboard are not its text;
        // an open command surface has the keyboard meanwhile.
        events.clear();
    } else {
        let chord = keys.as_ref().is_some_and(|k| k.any_pressed([KeyCode::SuperLeft, KeyCode::SuperRight, KeyCode::ControlLeft, KeyCode::ControlRight]));
        let typed: Vec<KeyboardInput> = events.read().filter(|e| e.state == ButtonState::Pressed).cloned().collect();
        for e in typed {
            let Some(d) = draft.as_mut() else { break };
            match d.draft.key(&e.logical_key, chord) {
                DraftKey::Enter => {
                    match submit(&doc, d) {
                        Ok(action) => {
                            if let Some(action) = action {
                                out.write(Act::ui(action));
                            }
                            draft = None;
                            ended = true;
                        }
                        Err(why) => d.error = Some(why),
                    }
                    break;
                }
                DraftKey::Escape => {
                    draft = None;
                    ended = true;
                    break;
                }
                DraftKey::Edited => d.error = None,
                DraftKey::Tab | DraftKey::Ignored => {}
            }
        }
    }
    if draft != before {
        doc.physical_edit.draft = draft;
        // The panels refresh on the document's revision.
        doc.touch();
    }
    if let Some(mut focus) = focus
        && (doc.physical_edit.draft.is_some() || ended)
        && !focus.0
    {
        focus.0 = true;
    }
}
