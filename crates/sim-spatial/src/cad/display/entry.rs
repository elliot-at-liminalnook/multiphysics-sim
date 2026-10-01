//! The section offset entry in the display toolbar (RoboCAD's Section
//! tool's Tab field: `NumericField("offset", 0.0)`, whose commit moves the
//! plane that far along its normal). Typing follows the saved views
//! panel's pattern (`views::panel::input`): a press on the field gives it
//! the keyboard (ending the name field's, the numeric bar's and the saved
//! views panel's), Enter writes `CadSection {offset}` (a refusal stays
//! under the field), Escape or a press elsewhere ends it; `CadInputFocus`
//! is set while the field has the keyboard and in the frame it ends.
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
use crate::cad::document::{CadDocument, CadInputFocus};
use crate::cad::panel::NameDraft;
use crate::cad::views::CadViews;
use crate::ui_kit::form::{DraftKey, FieldKind, FieldValue, TextDraft, Unit, evaluate};
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;

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
#[allow(clippy::too_many_arguments)]
pub(super) fn input(
    entry: Option<ResMut<SectionEntry>>,
    doc: Option<ResMut<CadDocument>>,
    presses: Query<(&Interaction, Option<&Enabled>), (Changed<Interaction>, With<OffsetInput>)>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    mut events: MessageReader<KeyboardInput>,
    (focus, name, views, display): (Option<ResMut<CadInputFocus>>, Option<ResMut<NameDraft>>, Option<ResMut<CadViews>>, Option<Res<CadDisplay>>),
    mut out: MessageWriter<Act<CadAction>>,
) {
    let (Some(mut entry), Some(mut doc)) = (entry, doc) else {
        events.clear();
        return;
    };
    let before = entry.typing.clone();
    let mut typing = before.clone();
    let (mut started, mut ended, mut on_field) = (false, false, false);
    for (interaction, enabled) in &presses {
        if *interaction != Interaction::Pressed || enabled.is_some_and(|e| !e.0) {
            continue;
        }
        on_field = true;
        if typing.is_none() {
            // RoboCAD's field opens at 0.
            typing = Some(OffsetTyping { draft: TextDraft { text: "0".into(), select_all: true }, error: None });
            started = true;
        }
    }
    if started {
        // One field holds the keyboard.
        if let Some(mut name) = name
            && name.editing.is_some()
        {
            name.editing = None;
            name.refusal = None;
        }
        if doc.tool_state.numeric.focus.is_some() {
            doc.tool_state.numeric.focus = None;
            doc.tool_state.numeric.began = None;
        }
        if let Some(mut views) = views
            && views.typing.is_some()
        {
            views.typing = None;
        }
    } else if typing.is_some() && !on_field && mouse.as_ref().is_some_and(|m| m.just_pressed(MouseButton::Left)) {
        typing = None;
        ended = true;
    }
    // The field is shown only while the section is on: it cannot keep the keyboard once hidden.
    if typing.is_some() && !display.as_ref().is_some_and(|d| d.section.enabled) {
        typing = None;
        ended = true;
        started = false;
    }
    if started || typing.is_none() || doc.ops.surface.is_some() {
        // Keys pressed before the field took the keyboard are not its text;
        // an open command surface has the keyboard meanwhile.
        events.clear();
    } else {
        let chord = keys.as_ref().is_some_and(|k| k.any_pressed([KeyCode::SuperLeft, KeyCode::SuperRight, KeyCode::ControlLeft, KeyCode::ControlRight]));
        let typed: Vec<KeyboardInput> = events.read().filter(|e| e.state == ButtonState::Pressed).cloned().collect();
        for e in typed {
            let Some(t) = typing.as_mut() else { break };
            match t.draft.key(&e.logical_key, chord) {
                DraftKey::Enter => {
                    match offset_action(&t.draft.text) {
                        Ok(action) => {
                            if let Some(a) = action {
                                out.write(Act::ui(a));
                            }
                            typing = None;
                            ended = true;
                        }
                        Err(why) => t.error = Some(why),
                    }
                    break;
                }
                DraftKey::Escape => {
                    typing = None;
                    ended = true;
                    break;
                }
                DraftKey::Edited => t.error = None,
                DraftKey::Tab | DraftKey::Ignored => {}
            }
        }
    }
    if typing != before {
        entry.typing = typing;
    }
    if let Some(mut focus) = focus
        && (entry.typing.is_some() || ended)
        && !focus.0
    {
        focus.0 = true;
    }
}
