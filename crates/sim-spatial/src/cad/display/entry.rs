//! The section offset entry in the display toolbar (RoboCAD's Section
//! tool's Tab field: `NumericField("offset", 0.0)`, whose commit moves the
//! plane that far along its normal), on the kit's one text field
//! ([`SECTION`], `ui_kit::text`): a press on the field gives it the
//! keyboard with "0" selected, Enter writes `CadSection {offset}` (a
//! refusal stays under the field), Escape or a press elsewhere ends it, and
//! so does hiding the section (the field is drawn only while it is on).
//! `SectionEntry::typing` is its draft (mirrored from the kit's, for the
//! toolbar) and the last refusal, set while the field has the keyboard.
//!
//! RoboCAD's other two Section tool gestures are not bound here: R (rotate
//! 90° about Z) is the Rotate tool's key and Tab the numeric bar's
//! (`transform::input`, `numeric::entry`), and natively the section is a
//! display state, not a tool that could own them, so the toolbar's Rotate
//! chip and this field stand in. Dragging the plane along its normal is
//! not bound either: a left drag in the 3D view is the Select tool's box
//! selection (`pick`) and the camera's, and the plane quad covers the
//! model, so a drag on it would take the drag from both.
use super::{CadDisplay, SectionArgs};
use crate::app::actions::Act;
use crate::builder::ui_api::Enabled;
use crate::cad::actions::CadAction;
use crate::ui_kit::form::{FieldKind, FieldValue, Unit, evaluate};
use crate::ui_kit::text::{FieldEvent, FieldId, FieldMsg, TextDraft, TextField, TextFocus};
use bevy::prelude::*;

/// The section offset field.
pub(in crate::cad) const SECTION: FieldId = FieldId("cad.section.offset");

/// The offset field as the kit spawns it (`display::build`): it opens at
/// "0", selected.
pub(super) fn field() -> TextField {
    TextField::new("Section offset").placeholder("offset, e.g. 5 or 2 cm").select_on_focus()
}

/// A length in mm as the numeric bar reads it (unit expressions: "5",
/// "2 cm", "1/4 in").
const OFFSET: FieldKind = FieldKind::Number { unit: Unit::Length, min: None, max: None, decimals: 3 };

/// The offset field's draft and why its last Enter sent nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct OffsetTyping {
    pub draft: TextDraft,
    pub error: Option<String>,
}

/// The offset field (display only).
#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub struct SectionEntry {
    pub typing: Option<OffsetTyping>,
}

/// Marks the offset field: a press gives it the keyboard.
#[derive(Component, Clone, Copy, Debug)]
pub(super) struct OffsetInput;

/// The field's Enter: `CadSection {offset}` (mm along the plane's normal),
/// None for a zero offset (nothing moves), or why nothing is sent.
pub fn offset_action(text: &str) -> Result<Option<CadAction>, String> {
    match evaluate(&OFFSET, text) {
        Ok(FieldValue::Number(v)) if !v.is_finite() => Err(format!("Offset: {v} is not a finite number of mm")),
        Ok(FieldValue::Number(v)) if v == 0.0 => Ok(None),
        Ok(FieldValue::Number(v)) => Ok(Some(CadAction::CadSection(SectionArgs { offset: Some(v), ..default() }))),
        Ok(_) => Err("type the offset in mm".into()),
        Err(e) => Err(format!("Offset: {e}")),
    }
}

/// Input: the offset field (see the module doc).
pub(super) fn input(
    entry: Option<ResMut<SectionEntry>>,
    presses: Query<Option<&Enabled>, (With<crate::ui_kit::activation::Activated>, With<OffsetInput>)>,
    mut msgs: MessageReader<FieldMsg>,
    mut text: TextFocus,
    display: Option<Res<CadDisplay>>,
    mut out: MessageWriter<Act<CadAction>>,
) {
    let Some(mut entry) = entry else {
        msgs.clear();
        return;
    };
    let before = entry.typing.clone();
    let mut typing = before.clone();
    for m in msgs.read().filter(|m| m.field == SECTION) {
        match &m.event {
            FieldEvent::Changed(d) => {
                if let Some(t) = typing.as_mut() {
                    t.draft = d.clone();
                    t.error = None;
                }
            }
            FieldEvent::Submit(typed) => {
                let Some(t) = typing.as_mut() else {
                    text.blur(SECTION);
                    continue;
                };
                t.draft.text = typed.clone();
                match offset_action(typed) {
                    Ok(action) => {
                        if let Some(a) = action {
                            out.write(Act::ui(a));
                        }
                        typing = None;
                        text.blur(SECTION);
                    }
                    Err(why) => t.error = Some(why),
                }
            }
            FieldEvent::Cancel => typing = None,
            // A press elsewhere or another field's focus (unless this
            // system gave the field the keyboard again since).
            FieldEvent::Blur if !text.focused(SECTION) => typing = None,
            FieldEvent::Blur | FieldEvent::Tab { .. } | FieldEvent::Arrow { .. } => {}
        }
    }
    for enabled in &presses {
        if enabled.is_some_and(|e| !e.0) || typing.is_some() {
            continue;
        }
        // RoboCAD's field opens at 0.
        let draft = TextDraft::new("0", true);
        if text.focus_draft(SECTION, draft.clone()) {
            typing = Some(OffsetTyping { draft, error: None });
        }
    }
    // The field is shown only while the section is on: it cannot keep the keyboard once hidden.
    if typing.is_some() && !display.as_ref().is_some_and(|d| d.section.enabled) {
        typing = None;
    }
    // The kit's focus is the record: a field that lost the keyboard without
    // a message read here ends; a field without a draft gives it back.
    if typing.is_some() && !text.focused(SECTION) {
        typing = None;
    }
    if typing.is_none() {
        text.blur(SECTION);
    }
    if typing != before {
        entry.typing = typing;
    }
}
