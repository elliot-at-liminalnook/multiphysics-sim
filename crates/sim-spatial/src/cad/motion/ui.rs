use super::*;
use crate::app::actions::Act;
use crate::cad::panel::CadButton;
use crate::ui_kit::text::{
    EnterKey, FieldEvent, FieldId, FieldMsg, TextField, TextFieldApp, TextFocus,
};
use crate::ui_kit::{Kit, Look};
const FIELD: FieldId = FieldId("cad.motion.editor");
#[derive(Component)]
struct MotionField(&'static str, u64);
pub(crate) fn build(a: &mut App) {
    super::export::build(a);
    a.add_text_field(
        FIELD,
        TextField::new("Motion field").enter(EnterKey::ShiftNewline),
    )
    .add_systems(
        Update,
        input
            .in_set(crate::app::ViewerSet::Input)
            .in_set(crate::app::InputSet::Window)
            .in_set(crate::cad::CadKeySet::Focus)
            .run_if(in_state(crate::app::ViewerMode::Cad)),
    );
}
fn input(
    mut s: ResMut<MotionState>,
    q: Query<(&Interaction, &MotionField), Changed<Interaction>>,
    mut text: ParamSet<(MessageReader<FieldMsg>, TextFocus)>,
    mut out: MessageWriter<Act<CadAction>>,
) {
    let messages = text
        .p0()
        .read()
        .filter(|m| m.field == FIELD)
        .cloned()
        .collect::<Vec<_>>();
    let mut focus = text.p1();
    if s.focus.is_none() {
        focus.blur(FIELD);
    }
    for m in messages {
        match m.event {
            FieldEvent::Changed(draft) => {
                let args = match s.focus.as_deref() {
                    Some("path") => MotionArgs {
                        value: Some(draft.text),
                        ..MotionArgs::of(MotionOp::Path)
                    },
                    Some("position") => {
                        let Ok(p) = draft.text.parse::<f64>() else {
                            continue;
                        };
                        MotionArgs {
                            position: Some(p),
                            id: s.joint.clone(),
                            sequence: Some(s.sequence),
                            ..MotionArgs::of(MotionOp::Position)
                        }
                    }
                    Some("seek") => {
                        let Ok(t) = draft.text.parse::<f64>() else {
                            continue;
                        };
                        MotionArgs {
                            time: Some(t),
                            sequence: Some(s.sequence),
                            ..MotionArgs::of(MotionOp::Seek)
                        }
                    }
                    Some("editor") => MotionArgs {
                        value: Some(draft.text),
                        sequence: Some(s.sequence),
                        ..MotionArgs::of(MotionOp::Editor)
                    },
                    _ => continue,
                };
                out.write(Act::ui(args.action()));
            }
            FieldEvent::Cancel | FieldEvent::Blur => {
                s.focus = None;
                s.touch();
            }
            _ => {}
        }
    }
    for (i, f) in &q {
        if *i == Interaction::Pressed && f.1 == s.sequence {
            s.focus = Some(f.0.into());
            let value = match f.0 {
                "path" => s.path.clone(),
                "position" => s
                    .joint
                    .as_ref()
                    .and_then(|id| s.positions.get(id))
                    .unwrap_or(&0.)
                    .to_string(),
                "seek" => s.cursor.to_string(),
                _ => s.editor.clone(),
            };
            focus.focus(FIELD, value);
            s.touch();
        }
    }
}
pub(crate) fn draw(p: &mut ChildSpawnerCommands, k: &Kit, d: &CadDocument, s: &MotionState) {
    for (_, label, action, ready) in controls_of(d, s) {
        if s.open
            || matches!(
                &action,
                CadAction::CadMotion(MotionArgs {
                    op: MotionOp::Dock,
                    ..
                })
            )
            || (s.busy()
                && matches!(
                    &action,
                    CadAction::CadMotion(MotionArgs {
                        op: MotionOp::Cancel,
                        ..
                    })
                ))
        {
            p.spawn(k.button(label, CadButton(action), Look::Secondary, ready.is_ok()));
            if let Err(e) = ready {
                p.spawn(k.note(e));
            }
        }
    }
    if !s.open {
        return;
    }
    p.spawn(k.title("Reference pose / named programs"));
    p.spawn(k.note("Kinematic display preview · geometry stays in its source CAD pose · no physics or hardware control. Returning restores the saved view and display."));
    if let Some(m) = &s.metadata {
        for assumption in &m.assumptions {
            p.spawn(k.note(assumption));
        }
        for j in &m.joints {
            if s.joint.as_deref() == Some(j.id.as_str()) {
                p.spawn(k.caption(format!(
                    "Actual bounds {:?}…{:?} {}; display range {}…{} {} (not hardware stops)",
                    j.lower, j.upper, j.unit, j.display_lower, j.display_upper, j.unit
                )));
                p.spawn(k.input(
                    s.positions.get(&j.id).unwrap_or(&j.home).to_string(),
                    "Joint value",
                    MotionField("position", s.sequence),
                    s.focus.as_deref() == Some("position"),
                ));
            }
        }
    }
    p.spawn(k.input(
        &s.editor,
        "Program JSON",
        MotionField("editor", s.sequence),
        s.focus.as_deref() == Some("editor"),
    ));
    p.spawn(k.input(
        s.cursor.to_string(),
        "Seek seconds",
        MotionField("seek", s.sequence),
        s.focus.as_deref() == Some("seek"),
    ));
    p.spawn(k.input(
        &s.path,
        "Absolute output video path",
        MotionField("path", s.sequence),
        s.focus.as_deref() == Some("path"),
    ));
    if let Some(sample) = &s.sample {
        p.spawn(k.caption(format!(
            "{} s · loop closure error {} mm",
            sample.time, sample.closure_error_mm
        )));
    }
    if let Some(export) = &s.export {
        p.spawn(k.caption(format!("Export {}", export.state())));
    }
    if let Some(e) = &s.error {
        p.spawn(k.note(e));
    }
}
