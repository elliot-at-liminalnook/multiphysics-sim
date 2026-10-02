use super::*;
use crate::app::actions::Act;
use crate::cad::panel::CadButton;
use crate::ui_kit::text::{FieldEvent, FieldId, FieldMsg, TextField, TextFieldApp, TextFocus};
use crate::ui_kit::{Kit, Look};
const FIELD: FieldId = FieldId("cad.review.note");
#[derive(Component, Debug)]
struct ReviewField(String, u64);
pub(crate) fn build(a: &mut App) {
    super::scene::build(a);
    super::plots::build(a);
    a.add_text_field(FIELD, TextField::new("Review text"))
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
    mut s: ResMut<ReviewState>,
    q: Query<&ReviewField, With<crate::ui_kit::activation::Activated>>,
    mut fields: ParamSet<(MessageReader<FieldMsg>, TextFocus)>,
    mut out: MessageWriter<Act<CadAction>>,
) {
    let messages = fields
        .p0()
        .read()
        .filter(|m| m.field == FIELD)
        .cloned()
        .collect::<Vec<_>>();
    let mut focus = fields.p1();
    if s.focus.is_none() {
        focus.blur(FIELD);
    }
    for m in messages {
        match m.event {
            FieldEvent::Changed(d) => {
                let op = match s.focus.as_deref() {
                    Some("note") => ReviewOp::Field,
                    Some("filter") => ReviewOp::Filter,
                    Some("signal") => ReviewOp::Signal,
                    Some("source") => ReviewOp::Source,
                    _ => continue,
                };
                out.write(Act::ui(
                    ReviewArgs {
                        value: Some(d.text),
                        sequence: Some(s.sequence),
                        ..ReviewArgs::of(op)
                    }
                    .action(),
                ));
            }
            FieldEvent::Cancel | FieldEvent::Blur => {
                s.focus = None;
                s.touch();
            }
            _ => {}
        }
    }
    for f in &q {
        if f.1 == s.sequence {
            let value = match f.0.as_str() {
                "note" => s.note.clone(),
                "filter" => s.filter.clone(),
                "signal" => s.signal.clone(),
                _ => s.source.clone().unwrap_or_default(),
            };
            s.focus = Some(f.0.clone());
            focus.focus(FIELD, value);
            s.touch();
        }
    }
}
pub(crate) fn draw(p: &mut ChildSpawnerCommands, k: &Kit, d: &CadDocument, s: &ReviewState) {
    let controls = controls_of(d, s);
    // Durable cancellation remains a rendered control even after dock closure.
    for (_, label, action, ready) in controls.iter().filter(|c| {
        s.open
            || matches!(
                c.2,
                CadAction::CadExperimentReview(ReviewArgs {
                    op: ReviewOp::Dock,
                    ..
                })
            )
            || (s.busy()
                && matches!(
                    c.2,
                    CadAction::CadExperimentReview(ReviewArgs {
                        op: ReviewOp::Cancel,
                        ..
                    })
                ))
    }) {
        p.spawn(k.button(
            label,
            CadButton(action.clone()),
            Look::Secondary,
            ready.is_ok(),
        ));
        if let Err(e) = ready {
            p.spawn(k.note(e));
        }
    }
    if !s.open {
        return;
    }
    p.spawn(k.title("Captured run · read only"));
    p.spawn(k.note("Simulated replay: captured geometry in mm; signals declare SI units. Live selection is an explicit ID mapping. Missing geometry is never replaced with live CAD."));
    if let Some(c) = &s.captured {
        p.spawn(k.caption(format!(
            "Source {:?} · captured revision {:?} · live revision {} · fidelity {}",
            c.geometry.identity,
            c.geometry.identity.revision,
            d.shown_revision(),
            c.result["profile"]
        )));
        if c.geometry.identity.document_id != d.doc_key.as_ref().and_then(|k| k.0.clone())
            || c.geometry.identity.revision != Some(d.shown_revision())
        {
            p.spawn(k.note("Captured/live identity or revision differs; this remains an isolated historical view."));
        }
        if let Some(e) = &c.geometry.missing_reason {
            p.spawn(k.note(e));
        }
        for key in [
            "settings",
            "limitations",
            "warnings",
            "evaluation",
            "objectives",
            "component_derivations",
        ] {
            p.spawn(k.caption(format!("{key}: {}", c.result[key])));
        }
        p.spawn(k.caption(format!("Baseline comparison: {}", c.comparison)));
        p.spawn(k.caption(format!("Sample {} s · {}", s.cursor, s.sample["index"])));
        for (name, value) in [
            ("filter", &s.filter),
            ("signal", &s.signal),
            ("note", &s.note),
        ] {
            p.spawn(k.input(
                value,
                name,
                ReviewField(name.into(), s.sequence),
                s.focus.as_deref() == Some(name),
            ));
        }
        if let Some(image) = &s.chart {
            p.spawn(k.chart_image(
                image.clone(),
                Node {
                    width: Val::Percent(100.),
                    height: Val::Px(140.),
                    ..default()
                },
                true,
            ));
            p.spawn(k.caption(
                "Blue selected run · gold baseline matched by stable identity and declared unit",
            ));
        }
        p.spawn(k.caption(format!("Captured traces: {}", c.result["trace"]["signals"])));
        p.spawn(k.caption(format!(
            "Baseline traces: {}",
            c.baseline["trace"]["signals"]
        )));
        for n in &c.geometry.nodes {
            if !n.source.is_null() {
                p.spawn(k.caption(format!(
                    "Captured {} source (read only): {}",
                    n.name, n.source
                )));
            }
        }
        if let Some(path) = &s.source {
            if let Some((kind, path)) = path.split_once('/') {
                p.spawn(k.caption(format!(
                    "Captured source · read only\n{}",
                    c.sources[kind]["files"][path]
                )));
            }
        }
    }
    if let Some(e) = &s.error {
        p.spawn(k.note(e));
    }
}
