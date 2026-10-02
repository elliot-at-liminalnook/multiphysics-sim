//! The inspector's name field (split from `panel.rs`) on the kit's one
//! text field ([`NAME`], `ui_kit::text`): a press on the field gives it the
//! keyboard with the node's name (no select-all); Enter writes `CadPatch
//! {"name"}` (nothing when unchanged; an empty name, or an Enter while
//! edits cannot be sent, keeps the field focused with the reason under it);
//! Escape cancels. The field is `sticky`: a press elsewhere does not end
//! it, but selecting another node or pressing a CAD button does.
//!
//! The kit's focus is the only record of whether it is open; [`NameDraft`]
//! keeps what the field edits (the node), its text mirrored from the kit's
//! draft (the panel's rebuild key) and the last refusal.
use super::{CadButton, edit_blocked, patch};
use crate::app::actions::Act;
use crate::builder::ui_api::Enabled;
use crate::cad::actions::CadAction;
use crate::cad::document::CadDocument;
use crate::cad::selection::{CadItems, CadSelection};
use crate::ui_kit::text::{FieldEvent, FieldId, FieldMsg, TextField, TextFocus};
use bevy::prelude::*;
use serde_json::Value;

/// The inspector's name field.
pub(in crate::cad) const NAME: FieldId = FieldId("cad.name");

/// The name field as the kit spawns it (`panel::build`): sticky, no
/// select-all on focus (the caret goes after the name).
pub(super) fn field() -> TextField {
    TextField::new("Name").placeholder("Name").sticky()
}

/// The inspector's name field: a press starts the draft with `name`.
#[derive(Component, Clone, Debug)]
pub(in crate::cad) struct NameField {
    pub id: String,
    pub name: String,
}

/// The rename in progress. Not a document edit until Enter.
#[derive(Resource, Default, Debug)]
pub(in crate::cad) struct NameDraft {
    /// The node the field renames while it has the keyboard.
    pub node: Option<String>,
    /// The field's text while it has the keyboard (the kit's draft,
    /// mirrored so the panel rebuilds as it is typed).
    pub text: String,
    /// Why the last Enter sent nothing (empty name, edits blocked); cleared
    /// by typing or when the draft ends.
    pub refusal: Option<String>,
}
impl NameDraft {
    /// The node being renamed and the text typed, while the field is open.
    pub(in crate::cad) fn editing(&self) -> Option<(&str, &str)> {
        self.node.as_deref().map(|n| (n, self.text.as_str()))
    }
}

/// End the draft (read before writing: a `DerefMut` every frame would mark
/// it changed and rebuild the panels each frame).
pub(super) fn end(draft: &mut ResMut<NameDraft>) {
    if draft.node.is_some() || draft.refusal.is_some() || !draft.text.is_empty() {
        let d = &mut **draft;
        d.node = None;
        d.text.clear();
        d.refusal = None;
    }
}

/// Input: a pressed, enabled CAD button writes its action (and ends a name
/// draft: the field is sticky, so the kit does not end it on that press).
#[allow(clippy::type_complexity)]
pub(super) fn buttons(doc: Option<Res<CadDocument>>, files: Option<Res<crate::cad::files::CadFiles>>, clicks: Query<(&CadButton, Option<&Enabled>), (With<crate::ui_kit::activation::Activated>, With<Button>)>, mut draft: ResMut<NameDraft>, mut text: TextFocus, mut out: MessageWriter<Act<CadAction>>) {
    let Some(doc) = doc else { return };
    for (button, enabled) in &clicks {
        if enabled.is_some_and(|e| !e.0) {
            continue;
        }
        text.blur(NAME);
        end(&mut draft);
        let action = if matches!(&button.0, CadAction::CadFile(a) if a.op == crate::cad::files::FileOp::Close) {
            let Some(files) = files.as_deref() else { continue };
            crate::cad::activation::guard_files(&doc, files, button.0.clone())
        } else { crate::cad::activation::guard(&doc, button.0.clone()) };
        out.write(Act::ui(action));
    }
}

/// Why an Enter of `text` renaming `id` sends nothing, if it does; None
/// with `unchanged` true when the name is already `text`.
fn refusal(doc: Option<&CadDocument>, id: &str, text: &str) -> (bool, Option<String>) {
    let unchanged = doc.and_then(|d| d.doc.as_ref()).and_then(|d| d.nodes.iter().find(|n| n.id == id)).is_some_and(|n| n.name == text);
    let why = if unchanged {
        None
    } else if text.trim().is_empty() {
        Some("A name cannot be empty: type one, or Escape to keep the current name.".to_string())
    } else {
        doc.and_then(edit_blocked).map(|why| format!("Not renamed: {why}. Enter again once it clears, or Escape."))
    };
    (unchanged, why)
}

/// Input: the name field's messages and presses (see the module doc).
pub(in crate::cad) fn name_entry(
    mut draft: ResMut<NameDraft>,
    fields: Query<&NameField, With<crate::ui_kit::activation::Activated>>,
    doc: Option<Res<CadDocument>>,
    mut msgs: MessageReader<FieldMsg>,
    mut text: TextFocus,
    mut out: MessageWriter<Act<CadAction>>,
    selection: CadSelection,
) {
    for m in msgs.read().filter(|m| m.field == NAME) {
        match &m.event {
            FieldEvent::Changed(d) => {
                if draft.node.is_some() && (draft.text != d.text || draft.refusal.is_some()) {
                    draft.text = d.text.clone();
                    draft.refusal = None;
                }
            }
            FieldEvent::Submit(typed) => {
                let Some(id) = draft.node.clone() else {
                    text.blur(NAME);
                    continue;
                };
                // Refusals keep the field (and its text) open, with the reason under it.
                match refusal(doc.as_deref(), &id, typed) {
                    (_, Some(why)) => {
                        if draft.refusal.as_ref() != Some(&why) {
                            draft.refusal = Some(why);
                        }
                    }
                    (unchanged, None) => {
                        text.blur(NAME);
                        end(&mut draft);
                        if !unchanged {
                            out.write(Act::ui(patch(&id, "name", Value::String(typed.clone()))));
                        }
                    }
                }
            }
            FieldEvent::Cancel => end(&mut draft),
            // A field focused again since its blur keeps its draft.
            FieldEvent::Blur if !text.focused(NAME) => end(&mut draft),
            FieldEvent::Blur | FieldEvent::Tab { .. } | FieldEvent::Arrow { .. } => {}
        }
    }
    // Selecting another node drops the draft.
    let selected = doc.as_ref().and_then(|_| selection.items().first_node().map(str::to_string));
    if draft.node.as_deref().is_some_and(|id| selected.as_deref() != Some(id)) {
        text.blur(NAME);
        end(&mut draft);
    }
    for field in &fields {
        if draft.node.as_deref() != Some(field.id.as_str()) && text.focus(NAME, field.name.clone()) {
            draft.node = Some(field.id.clone());
            draft.text = field.name.clone();
            draft.refusal = None;
        }
    }
    // The kit's focus is the record: a draft whose field lost the keyboard
    // without a message this system read (another system's blur) ends.
    if draft.node.is_some() && !text.focused(NAME) {
        end(&mut draft);
    }
}
