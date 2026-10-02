//! Kit text fields and actual window controls share the typed CAD action path.
use super::*;
use crate::app::actions::Act;
use crate::app::{InputSet, ViewerMode, ViewerSet};
use crate::cad::{CadKeySet, panel::CadButton};
use crate::ui_kit::text::{FieldEvent, FieldId, FieldMsg, TextField, TextFieldApp, TextFocus};
use crate::ui_kit::{Kit, Look};
const FIELD: FieldId = FieldId("cad.experiments.field");
#[derive(Component, Clone)]
pub(crate) struct ExperimentField {
    pub name: String,
    pub index: usize,
}
pub(crate) fn build(app: &mut App) {
    app.add_text_field(FIELD, TextField::new("Experiment input").select_on_focus())
        .add_systems(
            Update,
            input
                .in_set(ViewerSet::Input)
                .in_set(InputSet::Window)
                .in_set(CadKeySet::Focus)
                .run_if(in_state(ViewerMode::Cad)),
        );
}
fn input(
    mut st: ResMut<ExperimentsState>,
    fields: Query<(&Interaction, &ExperimentField), Changed<Interaction>>,
    mut text: ParamSet<(MessageReader<FieldMsg>, TextFocus)>,
    mut out: MessageWriter<Act<CadAction>>,
) {
    let events = text
        .p0()
        .read()
        .filter(|m| m.field == FIELD)
        .cloned()
        .collect::<Vec<_>>();
    let mut focus = text.p1();
    if !st.open || st.current.is_none() || st.focus.is_none() || st.focus_index != st.current {
        st.focus = None;
        st.focus_index = None;
        focus.blur(FIELD);
    }
    for event in events {
        match event.event {
            FieldEvent::Changed(draft) => {
                if let Some(name) = st.focus.clone() {
                    out.write(Act::ui(
                        ExperimentsArgs {
                            name: Some(name),
                            value: Some(draft.text),
                            draft_index: st.focus_index,
                            ..ExperimentsArgs::of(ExperimentsOp::Set)
                        }
                        .action(),
                    ));
                }
            }
            FieldEvent::Cancel | FieldEvent::Blur => {
                st.focus = None;
                st.touch();
            }
            _ => {}
        }
    }
    for (interaction, field) in &fields {
        if *interaction == Interaction::Pressed && st.current == Some(field.index) {
            if let Some(d) = st.draft() {
                let value = d.get(&field.name).to_string();
                st.focus = Some(field.name.clone());
                st.focus_index = Some(field.index);
                focus.focus(FIELD, value);
                st.touch();
            }
        }
    }
}
pub(crate) fn draw(
    p: &mut ChildSpawnerCommands,
    k: &Kit,
    doc: &CadDocument,
    st: &ExperimentsState,
) {
    let controls = controls_of(doc, st);
    draw_control(p, k, &controls, "dock");
    // Durable cancellation remains rendered even with closed dock/no snapshot.
    for control in controls
        .iter()
        .filter(|c| ["cad:experiments:cancel", "cad:experiments:discover"].contains(&c.0.as_str()))
    {
        button(p, k, control);
    }
    if !st.open {
        return;
    }
    p.spawn(k.title("Experiments · captured sources"));
    p.spawn(k.note("Simulation runs in the captured shared Rust runner. Quick check disables contact/noise; validation enables contact/flex/noise. Results are provisional until physical qualification. Python/OCCT and a built registry executable remain required."));
    for id in ["new", "rebase", "refresh"] {
        draw_control(p, k, &controls, id);
    }
    for control in controls.iter().filter(|c| {
        c.0.starts_with("cad:experiments:resume-")
            || c.0.starts_with("cad:experiments:run-")
            || c.0.starts_with("cad:experiments:candidate-")
    }) {
        button(p, k, control);
    }
    if let Some(d) = st.draft() {
        p.spawn(k.note("Empty unlinked Rhai editors use the captured authoritative reference defaults. Linked editors are read-only; unlink before editing. Imported modules are retained in the complete source bundle."));
        p.spawn(k.caption(format!(
            "Draft {} · document {} · generation {} · r{} · edit {}",
            d.stamp.draft_index,
            d.stamp.document_id,
            d.stamp.generation,
            d.stamp.revision,
            d.stamp.sequence
        )));
        for (name, value) in &d.fields {
            p.spawn(k.caption(match name.as_str() {
                "system" => "System · Rhai",
                "controller" => "Controller · Rhai / captured process JSON",
                "parameters" => "Parameters · system/controller/settings/seed JSON",
                "operations" => "Candidate / atomic batch · authoritative operations JSON",
                "script_path" => "Repository model script path",
                "script_params" => "Model script parameters JSON",
                other => other,
            }));
            p.spawn(k.input(
                value,
                "",
                ExperimentField {
                    name: name.clone(),
                    index: d.stamp.draft_index,
                },
                st.focus.as_deref() == Some(name.as_str()),
            ));
            let choices: &[&str] = match name.as_str() {
                "profile" => &["quick_check", "validation"],
                "language" => &["rhai", "process"],
                "interface" => &["position_target", "driver_duty"],
                "controller_enabled" => &["true", "false"],
                _ => &[],
            };
            for choice in choices {
                p.spawn(
                    k.button(
                        *choice,
                        CadButton(
                            ExperimentsArgs {
                                name: Some(name.clone()),
                                value: Some((*choice).into()),
                                draft_index: st.current,
                                draft_sequence: Some(d.stamp.sequence),
                                ..ExperimentsArgs::of(ExperimentsOp::Set)
                            }
                            .action(),
                        ),
                        Look::Ghost,
                        true,
                    ),
                );
            }
            if let Some(key) = name
                .strip_suffix("_path")
                .filter(|key| ["system", "controller"].contains(key))
            {
                p.spawn(
                    k.button(
                        &format!("Link / unlink {key} file"),
                        CadButton(
                            ExperimentsArgs {
                                name: Some(key.into()),
                                value: Some(value.clone()),
                                draft_index: st.current,
                                draft_sequence: Some(d.stamp.sequence),
                                ..ExperimentsArgs::of(ExperimentsOp::Link)
                            }
                            .action(),
                        ),
                        Look::Secondary,
                        true,
                    ),
                );
            }
        }
        p.spawn(k.caption(format!(
            "Linked service-host source bundles: {:?} · auto rerun {} · 750 ms debounce",
            d.linked, d.auto
        )));
        if let Some(error) = &d.error {
            p.spawn(k.note(error));
        }
    }
    for id in [
        "preflight",
        "run",
        "auto",
        "restore",
        "restore_graph",
        "baseline",
        "compare",
        "candidate_create",
        "candidate_read",
        "candidate_accept",
        "candidate_discard",
        "candidate_run",
        "script",
        "batch",
        "close_draft",
        "import_composition",
        "review",
        "candidate_review",
    ] {
        draw_control(p, k, &controls, id);
    }
    if let Some(catalogue) = &st.catalogue {
        p.spawn(k.caption("Shared registry · types, ports, units, parameters"));
        p.spawn(k.note(serde_json::to_string_pretty(catalogue).unwrap_or_default()));
    }
    if let Some(v) = &st.diagnostics {
        p.spawn(k.caption("Diagnostics / captured inputs / candidate change summary"));
        p.spawn(k.note(serde_json::to_string_pretty(v).unwrap_or_default()));
    }
    if let Some(v) = &st.comparison {
        p.spawn(k.caption("Baseline comparison · captured identities"));
        p.spawn(k.note(serde_json::to_string_pretty(v).unwrap_or_default()));
    }
    if let Some(a) = &st.active {
        p.spawn(k.note(serde_json::to_string_pretty(&a.json()).unwrap_or_default()));
    }
    if let Some(e) = &st.error {
        p.spawn(k.note(e));
    }
    p.spawn(
        k.button(
            "Close Experiments · retain drafts / request cancel",
            CadButton(
                ExperimentsArgs {
                    open: Some(false),
                    ..ExperimentsArgs::of(ExperimentsOp::Dock)
                }
                .action(),
            ),
            Look::Ghost,
            true,
        ),
    );
}
fn button(p: &mut ChildSpawnerCommands, k: &Kit, c: &Control) {
    p.spawn(k.button(&c.1, CadButton(c.2.clone()), Look::Secondary, c.3.is_ok()));
    if let Err(e) = &c.3 {
        p.spawn(k.note(e));
    }
}
fn draw_control(p: &mut ChildSpawnerCommands, k: &Kit, controls: &[Control], id: &str) {
    if let Some(c) = controls
        .iter()
        .find(|c| c.0 == format!("cad:experiments:{id}"))
    {
        button(p, k, c);
    }
}
