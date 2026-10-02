//! The physical rows' typing (Input), the editors' pattern
//! (`editors::entry`), on the kit's one text field ([`PHYSICAL`],
//! `ui_kit::text`): a press on a row's field gives it the keyboard with a
//! draft of its value (selected, so typing replaces it); Enter checks it
//! as the row reads it (`physical_edit::parse_color`, `joint_override`)
//! and writes one `CadInspector` stamped with the revision the draft
//! opened at (an unchanged value sends nothing); an error, or an edit that
//! cannot be sent now, keeps the field focused with the reason. Escape, a
//! press elsewhere, another node selected, an open materials dialog, or
//! another field taking the keyboard ends it. `PhysicalEdit::draft` is
//! the row the field edits, its text mirrored from the kit's draft (the
//! panel and `cad_state` show it) and the last refusal.
use super::physical_edit::{InspectorArgs, InspectorOp, RowDraft, RowField, joint_override, parse_color};
use super::rows::{PhysicalField, current_text};
use crate::app::actions::Act;
use crate::cad::actions::CadAction;
use crate::cad::document::CadDocument;
use crate::cad::selection::{CadItems, CadSelection};
use crate::ui_kit::text::{FieldEvent, FieldId, FieldMsg, TextDraft, TextField, TextFocus};
use bevy::prelude::*;

/// The physical rows' field: one kit field for every row (the row is
/// `PhysicalEdit::draft`).
pub(in crate::cad) const PHYSICAL: FieldId = FieldId("cad.inspector.physical");

/// The physical rows' field as the kit spawns it (`build_physical`).
pub(in crate::cad) fn field() -> TextField {
    TextField::new("Physical property").select_on_focus()
}

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
pub(in crate::cad) fn entry(
    doc: Option<ResMut<CadDocument>>,
    presses: Query<&PhysicalField, With<crate::ui_kit::activation::Activated>>,
    mut msgs: MessageReader<FieldMsg>,
    mut text: TextFocus,
    mut out: MessageWriter<Act<CadAction>>,
    selection: CadSelection,
) {
    let Some(mut doc) = doc else {
        msgs.clear();
        return;
    };
    let before = doc.physical_edit.draft.clone();
    let mut draft = before.clone();
    for m in msgs.read().filter(|m| m.field == PHYSICAL) {
        match &m.event {
            FieldEvent::Changed(t) => {
                if let Some(d) = draft.as_mut() {
                    d.draft = t.clone();
                    d.error = None;
                }
            }
            FieldEvent::Submit(typed) => {
                let Some(d) = draft.as_mut() else {
                    text.blur(PHYSICAL);
                    continue;
                };
                d.draft.text = typed.clone();
                match submit(&doc, d) {
                    Ok(action) => {
                        if let Some(action) = action {
                            out.write(Act::ui(action));
                        }
                        draft = None;
                        text.blur(PHYSICAL);
                    }
                    Err(why) => d.error = Some(why),
                }
            }
            FieldEvent::Cancel => draft = None,
            // A press elsewhere or another field's focus (unless this
            // system gave the field the keyboard again since).
            FieldEvent::Blur if !text.focused(PHYSICAL) => draft = None,
            FieldEvent::Blur | FieldEvent::Tab { .. } | FieldEvent::Arrow { .. } => {}
        }
    }
    let mut started = false;
    for f in &presses {
        if draft.as_ref().is_some_and(|d| d.node == f.node && d.field == f.field) {
            continue;
        }
        let opened = TextDraft::new(current_text(&doc, &f.node, f.field), true);
        if text.focus_draft(PHYSICAL, opened.clone()) {
            draft = Some(RowDraft { node: f.node.clone(), field: f.field, draft: opened, error: None, began: doc.shown_revision() });
            started = true;
        }
    }
    if !started && let Some(node) = draft.as_ref().map(|d| d.node.clone()) {
        let selected = selection.items().first_node().map(str::to_string);
        // A materials dialog is modal: it has the keyboard while open.
        let dialog = doc.materials.form.is_some();
        if dialog || selected.as_deref() != Some(node.as_str()) {
            draft = None;
            text.blur(PHYSICAL);
        }
    }
    // The kit's focus is the record: a draft whose field lost the keyboard
    // without a message read here ends, and a field without a draft blurs.
    if draft.is_some() && !text.focused(PHYSICAL) {
        draft = None;
    }
    if draft.is_none() {
        text.blur(PHYSICAL);
    }
    if draft != before {
        doc.physical_edit.draft = draft;
        // The panels refresh on the document's revision.
        doc.touch();
    }
}
