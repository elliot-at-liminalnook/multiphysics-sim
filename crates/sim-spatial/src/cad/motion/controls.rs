//! One discoverable catalogue supplies every rendered motion button. The
//! service remains the authority for program validation and source publication.
use super::*;
pub(crate) type Control = (String, String, CadAction, Result<(), String>);
pub(crate) fn controls(cx: &Cx) -> Vec<Control> {
    controls_of(cx.doc, cx.motion)
}
fn ready(d: &CadDocument, s: &MotionState, op: MotionOp) -> Result<(), String> {
    if matches!(op, MotionOp::Dock | MotionOp::Cancel | MotionOp::Return) {
        return Ok(());
    }
    if s.export.is_some() {
        return Err("Motion inputs are locked while export is running; cancel and wait for its terminal receipt".into());
    }
    if matches!(op, MotionOp::ExportFps | MotionOp::ExportSize) {
        return Ok(());
    }
    if op == MotionOp::Enter {
        return if d.connected() {
            Ok(())
        } else {
            Err("Open a CAD document first".into())
        };
    }
    let id = s.identity.as_ref().ok_or("Enter pose mode first")?;
    guard(d, id)?;
    if matches!(op, MotionOp::Save | MotionOp::Delete) {
        if let Some(e) = d.commit_refusal_for(Some(id.revision), true) {
            return Err(e);
        }
        if op == MotionOp::Delete && s.selected.is_none() {
            return Err("Choose a named program to delete".into());
        }
    }
    if matches!(
        op,
        MotionOp::Position
            | MotionOp::Play
            | MotionOp::Pause
            | MotionOp::Seek
            | MotionOp::Markers
            | MotionOp::Focus
            | MotionOp::Export
    ) && !s.active
    {
        return Err("Enter active reference pose preview first".into());
    }
    if op == MotionOp::Export {
        export::preview_ready(d, s)?;
        if s.program_job.is_some() || s.loading.is_some() {
            return Err("Wait for reference metadata/program validation before exporting".into());
        }
        let path = std::path::Path::new(&s.path);
        if !path.is_absolute() || path.extension().is_none_or(|e| e != "mp4") {
            return Err("Choose an absolute .mp4 destination".into());
        }
    }
    Ok(())
}
pub(crate) fn controls_of(d: &CadDocument, s: &MotionState) -> Vec<Control> {
    let mut out = Vec::new();
    let mut push = |id: String, label: String, args: MotionArgs, extra: Result<(), String>| {
        let readiness = ready(d, s, args.op).and(extra);
        out.push((id, label, args.action(), readiness));
    };
    for (op, label) in [
        (MotionOp::Dock, "Pose / motion"),
        (MotionOp::Enter, "Enter kinematic pose preview"),
        (MotionOp::Return, "Return to CAD pose"),
        (MotionOp::Play, "Play kinematic program"),
        (MotionOp::Pause, "Pause"),
        (MotionOp::Markers, "Toggle joint markers"),
        (MotionOp::Focus, "Focus mechanism"),
        (MotionOp::Sweep, "Make joint sweep"),
        (MotionOp::Validate, "Validate program JSON"),
        (MotionOp::Save, "Save named program (undoable)"),
        (MotionOp::Delete, "Delete selected program (undoable)"),
        (MotionOp::Export, "Export video"),
        (MotionOp::Cancel, "Cancel motion work"),
    ] {
        push(
            format!("cad:motion:{op:?}"),
            label.into(),
            MotionArgs {
                sequence: Some(s.sequence),
                revision: s.identity.as_ref().map(|id| id.revision),
                ..MotionArgs::of(op)
            },
            Ok(()),
        );
    }
    if let Some(m) = &s.metadata {
        for j in &m.joints {
            push(
                format!("cad:motion:joint:{}", j.id),
                format!(
                    "{} [{}]{}",
                    j.name,
                    j.unit,
                    if j.driver {
                        " driver"
                    } else {
                        " follows driver"
                    }
                ),
                MotionArgs {
                    id: Some(j.id.clone()),
                    sequence: Some(s.sequence),
                    ..MotionArgs::of(MotionOp::Joint)
                },
                Ok(()),
            );
            if s.joint.as_deref() == Some(j.id.as_str()) {
                for (label, value) in [
                    ("lower", j.display_lower),
                    ("home", j.home),
                    ("upper", j.display_upper),
                ] {
                    push(
                        format!("cad:motion:position:{}:{label}", j.id),
                        format!("{label}: {value} {}", j.unit),
                        MotionArgs {
                            id: Some(j.id.clone()),
                            position: Some(value),
                            sequence: Some(s.sequence),
                            ..MotionArgs::of(MotionOp::Position)
                        },
                        if j.driver {
                            Ok(())
                        } else {
                            Err("Passive/coupled joints follow the driver".into())
                        },
                    );
                }
            }
        }
    }
    for name in s.programs.keys() {
        push(
            format!("cad:motion:program:{name}"),
            format!("Program {name}"),
            MotionArgs {
                id: Some(name.clone()),
                sequence: Some(s.sequence),
                ..MotionArgs::of(MotionOp::Program)
            },
            Ok(()),
        );
    }
    if let Ok(program) = serde_json::from_str::<Value>(&s.editor) {
        if let Some(duration) = program["duration"]
            .as_f64()
            .filter(|t| t.is_finite() && *t > 0.)
        {
            for i in 0..=20 {
                let time = duration * i as f64 / 20.;
                push(
                    format!("cad:motion:seek:{i}"),
                    format!("Seek {time:.3} s"),
                    MotionArgs {
                        time: Some(time),
                        sequence: Some(s.sequence),
                        ..MotionArgs::of(MotionOp::Seek)
                    },
                    Ok(()),
                );
            }
        }
    }
    for fps in [24, 30, 60] {
        push(
            format!("cad:motion:fps:{fps}"),
            format!("{fps} fps"),
            MotionArgs {
                value: Some(fps.to_string()),
                ..MotionArgs::of(MotionOp::ExportFps)
            },
            Ok(()),
        );
    }
    for size in ["720p", "1080p"] {
        push(
            format!("cad:motion:size:{size}"),
            size.into(),
            MotionArgs {
                value: Some(size.into()),
                ..MotionArgs::of(MotionOp::ExportSize)
            },
            Ok(()),
        );
    }
    out
}
