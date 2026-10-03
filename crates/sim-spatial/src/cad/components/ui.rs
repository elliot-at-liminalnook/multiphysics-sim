//! All native fields use the UI kit's one text entry. This input system
//! translates FieldMsg to the same ComponentsArgs as REST; it does not send
//! requests. The parent CAD panel owns transient nodes and their teardown.
use super::{
    ComponentsArgs, ComponentsFormKind, ComponentsOp, ComponentsState, Control, controls_of,
};
use crate::app::actions::Act;
use crate::app::{ViewerMode};
use crate::cad::panel::CadButton;
use crate::cad::{CadAction, CadDocument, CadKeySet};
use crate::ui_kit::text::{FieldEvent, FieldId, FieldMsg, TextField, TextFieldApp, TextFocus};
use crate::ui_kit::{DANGER, Kit, Look, size};
use bevy::prelude::*;

const FIELD: FieldId = FieldId("cad.components.field");
#[derive(Component, Clone, Debug)]
pub(crate) struct ComponentField(pub String);

pub(crate) fn build(app: &mut App) {
    app.add_text_field(FIELD, TextField::new("Component field").select_on_focus())
        .add_systems(
            Update,
            input
                .in_set(CadKeySet::Focus)
                .run_if(in_state(ViewerMode::Cad)),
        );
}
fn input(
    mut state: ResMut<ComponentsState>,
    fields: Query<&ComponentField, With<crate::ui_kit::activation::Activated>>,
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
    if state.focus.is_none() {
        focus.blur(FIELD);
    }
    for m in events {
        match m.event {
            FieldEvent::Changed(draft) => {
                if let Some(name) = state.focus.clone() {
                    let args = match name.as_str() {
                        "find" => ComponentsArgs {
                            value: Some(draft.text),
                            ..ComponentsArgs::of(ComponentsOp::Find)
                        },
                        "folder" => {
                            state.folder = draft.text;
                            state.touch();
                            continue;
                        }
                        _ => ComponentsArgs {
                            name: Some(name),
                            value: Some(draft.text),
                            draft_index: state.current,
                            ..ComponentsArgs::of(ComponentsOp::FormSet)
                        },
                    };
                    out.write(Act::ui(args.action()));
                }
            }
            FieldEvent::Submit(value) => {
                if state.focus.as_deref() == Some("folder") {
                    out.write(Act::ui(
                        ComponentsArgs {
                            path: Some(value),
                            ..ComponentsArgs::of(ComponentsOp::Folder)
                        }
                        .action(),
                    ));
                }
            }
            FieldEvent::Cancel | FieldEvent::Blur => {
                state.focus = None;
                state.touch();
            }
            FieldEvent::Tab { back } => {
                let keys = state
                    .draft()
                    .map(|d| {
                        visible_fields(d.kind, &d.fields)
                            .into_iter()
                            .map(|(k, _)| k.clone())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                if !keys.is_empty() {
                    let at = state
                        .focus
                        .as_ref()
                        .and_then(|s| keys.iter().position(|k| k == s))
                        .unwrap_or(0);
                    let next = if back {
                        (at + keys.len() - 1) % keys.len()
                    } else {
                        (at + 1) % keys.len()
                    };
                    let key = keys[next].clone();
                    let value = state
                        .draft()
                        .and_then(|d| d.fields.get(&key))
                        .cloned()
                        .unwrap_or_default();
                    state.focus = Some(key);
                    focus.focus(FIELD, value);
                    state.touch();
                }
            }
            _ => {}
        }
    }
    // The draft a running job was built from keeps the values that were
    // sent (`super::jobs::applying`): its fields neither take the keyboard
    // nor keep it, so the kit field never shows text the draft refused.
    let locked = super::jobs::applying(&state, state.current);
    for field in &fields {
        {
            if locked && !["find", "folder"].contains(&field.0.as_str()) {
                state.error = Some(super::APPLYING.into());
                state.touch();
                continue;
            }
            let value = match field.0.as_str() {
                "find" => state.find.clone(),
                "folder" => state.folder.clone(),
                key => state
                    .draft()
                    .and_then(|d| d.fields.get(key))
                    .cloned()
                    .unwrap_or_default(),
            };
            state.focus = Some(field.0.clone());
            focus.focus(FIELD, value);
            state.touch();
        }
    }
    if locked
        && state
            .focus
            .as_deref()
            .is_some_and(|f| f != "find" && f != "folder")
    {
        state.focus = None;
        focus.blur(FIELD);
        state.touch();
    }
}
fn button(p: &mut ChildSpawnerCommands, k: &Kit, controls: &[Control], id: &str) {
    if let Some((_, label, action, ready)) = controls.iter().find(|c| c.0 == id) {
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
}
pub(crate) fn draw(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument, st: &ComponentsState) {
    if !st.open {
        return;
    }
    let controls = controls_of(doc, st);
    p.spawn(k.title("Components"));
    p.spawn(k.caption("Build once. Place linked assemblies. Override only what differs."));
    p.spawn(k.input(
        &st.find,
        "Find a component…",
        ComponentField("find".into()),
        st.focus.as_deref() == Some("find"),
    ));
    if let Some((_, catalogue)) = &st.catalogue {
        for def in &catalogue.definitions {
            if !def.name.to_lowercase().contains(&st.find.to_lowercase()) {
                continue;
            }
            button(
                p,
                k,
                &controls,
                &format!("cad:components:definition-{}", def.id),
            );
        }
    } else {
        p.spawn(k.note("Reading authoritative component catalogue…"));
    }
    for kind in ComponentsFormKind::ALL {
        button(p, k, &controls, &format!("cad:components:{}", kind.name()));
    }
    p.spawn(k.caption("Saved library · folder on the RoboCAD service host"));
    p.spawn(k.input(
        &st.folder,
        "Component library folder",
        ComponentField("folder".into()),
        st.focus.as_deref() == Some("folder"),
    ));
    button(p, k, &controls, "cad:components:folder");
    for (i, _) in st.files.iter().enumerate() {
        button(p, k, &controls, &format!("cad:components:library-{i}"));
    }
    // Closing a form preserves its draft. Reopening against the same source
    // offers retained forms explicitly, including rejected configurations.
    for (index, draft) in st.drafts.iter().enumerate() {
        if Some(index) != st.current && !draft.applied {
            p.spawn(
                k.button(
                    &format!(
                        "Resume {} draft · revision {}",
                        draft.kind.label(),
                        draft.began
                    ),
                    CadButton(
                        ComponentsArgs {
                            id: Some(index.to_string()),
                            ..ComponentsArgs::of(ComponentsOp::Resume)
                        }
                        .action(),
                    ),
                    Look::Ghost,
                    true,
                ),
            );
        }
    }
    for index in 0..st.drafts.len() {
        button(p, k, &controls, &format!("cad:components:copy-{index}"));
    }
    if let Some(d) = st.draft() {
        let locked = super::jobs::applying(st, st.current);
        p.spawn(k.title(d.kind.label()));
        if locked {
            p.spawn(k.note(
                "This draft is being applied: its fields are locked until the rebuild ends or is cancelled.",
            ));
        }
        p.spawn(k.note("Parameter values below are unevaluated expressions, not measured current values. Shared defaults apply unless an occurrence overrides them. Dimensions use explicit units; expressions may refer to parameters. Nested local overrides take precedence over mappings."));
        if d.nested_member {
            p.spawn(k.note("Nested occurrence: branch-local overrides only; move through the parent definition."));
        }
        if let Some(id) = &d.definition
            && let Ok(def) = super::form::definition(st, id)
        {
            p.spawn(k.caption(format!(
                    "Targets: {}",
                    def.targets
                        .iter()
                        .map(|t| format!("{}: {}", t.name, t.id))
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            if let Some((_, catalogue)) = &st.catalogue {
                p.spawn(k.caption(format!("Geometry recipes: {}", catalogue.features)));
            }
        }
        for (name, value) in visible_fields(d.kind, &d.fields) {
            p.spawn(k.caption(name));
            p.spawn(k.input(
                value,
                "",
                ComponentField(name.clone()),
                st.focus.as_deref() == Some(name.as_str()),
            ));
            let choices: Vec<String> = if name == "shape" {
                vec!["box".into(), "cylinder".into()]
            } else if name.ends_with(".enabled") {
                vec!["true".into(), "false".into()]
            } else if name.ends_with(".unit") {
                st.recipes
                    .as_ref()
                    .map(|r| r.units.clone())
                    .unwrap_or_default()
            } else if name.ends_with(".provenance") {
                vec!["measured".into(), "derived".into(), "estimated".into()]
            } else if name == "variant" {
                d.definition
                    .as_deref()
                    .and_then(|id| super::form::definition(st, id).ok())
                    .map(|d| d.variants.keys().cloned().collect())
                    .unwrap_or_else(|| {
                        serde_json::from_str::<
                            std::collections::BTreeMap<
                                String,
                                sim_runtime::cad_client::ComponentVariant,
                            >,
                        >(
                            d.fields.get("variants").map(String::as_str).unwrap_or("{}")
                        )
                        .map(|v| v.into_keys().collect())
                        .unwrap_or_default()
                    })
            } else {
                Vec::new()
            };
            for choice in choices {
                p.spawn(
                    k.button(
                        &choice,
                        CadButton(
                            ComponentsArgs {
                                name: Some(name.clone()),
                                value: Some(choice.clone()),
                                draft_index: st.current,
                                ..ComponentsArgs::of(ComponentsOp::FormSet)
                            }
                            .action(),
                        ),
                        Look::Ghost,
                        !locked,
                    ),
                );
            }
            if let Some(key) = name.strip_prefix("binding.")
                && let Some(id) = &d.definition
                && let Ok(def) = super::form::definition(st, id)
                && let Some(port) = def.ports.get(key)
            {
                p.spawn(k.note(format!("{} · type {}", port.label, port.kind)));
                if let Some(nodes) = doc.doc.as_ref().map(|d| &d.nodes) {
                    for n in nodes.iter().filter(|n| n.kind == port.kind) {
                        p.spawn(
                            k.button(
                                &n.name,
                                CadButton(
                                    ComponentsArgs {
                                        name: Some(name.clone()),
                                        value: Some(n.id.clone()),
                                        draft_index: st.current,
                                        ..ComponentsArgs::of(ComponentsOp::FormSet)
                                    }
                                    .action(),
                                ),
                                Look::Ghost,
                                !locked,
                            ),
                        );
                    }
                }
            }
        }
        if let Some(e) = &d.error {
            p.spawn(k.text(e, size::SMALL, DANGER, 0));
        }
        button(p, k, &controls, "cad:components:submit");
        button(p, k, &controls, "cad:components:close_form");
    }
    if let Some(active) = &st.active {
        // RoboCAD's progress line (ui/components.py ComponentsPanel.poll and
        // watch): "stage · done/total", the stage alone without a total.
        let line = match &active.status {
            Some(job) if job.total > 0 => format!("{} · {}/{}", job.stage, job.done, job.total),
            Some(job) if !job.stage.is_empty() => job.stage.clone(),
            // The start's answer was lost: say so, not "Preparing".
            _ if active.uncertain.is_some() => {
                "Outcome uncertain; reading RoboCAD's jobs (no POST is retried)".into()
            }
            _ => "Preparing component… You can keep viewing the model.".into(),
        };
        p.spawn(k.caption(line));
        if active.cancel_requested {
            p.spawn(k.note(
                "Cancel requested · waiting for RoboCAD to confirm; a change it already applied is reported as applied",
            ));
        }
        button(p, k, &controls, "cad:components:cancel");
    }
    if let Some(e) = &st.error {
        p.spawn(k.text(e, size::SMALL, DANGER, 0));
    }
    p.spawn(
        k.button(
            "Close Components",
            CadButton(
                ComponentsArgs {
                    open: Some(false),
                    ..ComponentsArgs::of(ComponentsOp::Dock)
                }
                .action(),
            ),
            Look::Ghost,
            true,
        ),
    );
}
fn visible_fields(
    kind: ComponentsFormKind,
    fields: &std::collections::BTreeMap<String, String>,
) -> Vec<(&String, &String)> {
    use ComponentsFormKind as K;
    fields
        .iter()
        .filter(|(key, _)| match kind {
            K::Make | K::Create => ["name", "origin"].contains(&key.as_str()),
            K::Parametric => ["name", "shape"].contains(&key.as_str()),
            K::Place => {
                ["name", "origin", "angle_deg", "variant"].contains(&key.as_str())
                    || key.starts_with("binding.")
                    || key.starts_with("override.")
            }
            K::Defaults => {
                ["parameters", "features", "nested", "variants"].contains(&key.as_str())
                    || key.starts_with("parameter.")
                    || key.starts_with("nested.")
                    || key.starts_with("variant.")
            }
            K::Overrides => {
                ["origin", "overrides"].contains(&key.as_str()) || key.starts_with("override.")
            }
            K::Reset | K::Detach => false,
            K::Transform => ["translation", "axis", "angle_deg", "scale"].contains(&key.as_str()),
            K::Import | K::Export => key.as_str() == "path",
            K::Family => ["name", "parameters", "variants", "variant"].contains(&key.as_str()),
            K::LinkFamily => ["variant", "overrides"].contains(&key.as_str()),
        })
        .collect()
}
