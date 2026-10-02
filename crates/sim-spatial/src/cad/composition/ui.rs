use super::*;
use crate::{
    app::{InputSet, ViewerMode, ViewerSet, actions::Act},
    cad::panel::CadButton,
    ui_kit::text::{EnterKey, FieldEvent, FieldId, FieldMsg, TextField, TextFieldApp, TextFocus},
    ui_kit::{Kit, Look, SUBTLE, TEXT, WARN, size},
};
const FIELD: FieldId = FieldId("cad.composition");
#[derive(Component, Clone)]
struct Input(String);
pub(crate) fn build(app: &mut App) {
    app.add_text_field(
        FIELD,
        TextField::new("Composition field").enter(EnterKey::ShiftNewline),
    )
    .add_systems(
        Update,
        input
            .in_set(InputSet::Window)
            .in_set(crate::cad::CadKeySet::Focus)
            .in_set(ViewerSet::Input)
            .run_if(in_state(ViewerMode::Cad)),
    );
}
fn input(
    mut st: ResMut<CadCompositionState>,
    fields: Query<(&Interaction, &Input), Changed<Interaction>>,
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
    for msg in &messages {
        let Some(name) = st.typing.clone() else {
            continue;
        };
        match &msg.event {
            FieldEvent::Changed(draft) => {
                let mut a = CadCompositionArgs::of(CompositionOp::SetField);
                a.draft_index = st.current;
                a.name = Some(name);
                a.draft_index = st.current;
                a.value = Some(draft.text.clone());
                out.write(Act::ui(a.action()));
            }
            FieldEvent::Submit(_) | FieldEvent::Cancel | FieldEvent::Blur => {
                focus.blur(FIELD);
                st.typing = None;
                st.revision += 1;
            }
            _ => {}
        }
    }
    for (interaction, field) in &fields {
        if *interaction == Interaction::Pressed {
            let value = if field.0 == "check_id" {
                st.check_draft.clone()
            } else {
                st.current
                    .and_then(|i| st.drafts.get(i))
                    .and_then(|d| d.fields.get(&field.0))
                    .cloned()
                    .unwrap_or_default()
            };
            st.typing = Some(field.0.clone());
            focus.focus(FIELD, value);
            st.revision += 1;
        }
    }
}
pub(super) fn controls(
    doc: &CadDocument,
    st: &CadCompositionState,
) -> Vec<(String, String, CadAction, Result<(), String>)> {
    let mut result = Vec::new();
    let mut add =
        |id: String, label: String, mut a: CadCompositionArgs, ready: Result<(), String>| {
            if matches!(
                a.op,
                CompositionOp::Submit | CompositionOp::AddParameter | CompositionOp::SetField
            ) {
                a.draft_index = st.current;
            }
            result.push((format!("cad:composition:{id}"), label, a.action(), ready))
        };
    add(
        "dock".into(),
        "System composition".into(),
        CadCompositionArgs::of(CompositionOp::Dock),
        Ok(()),
    );
    add(
        "overview".into(),
        "Overview".into(),
        CadCompositionArgs::of(CompositionOp::Overview),
        Ok(()),
    );
    for (id, value, label) in [("zoom_in", "1.25", "Zoom +"), ("zoom_out", "0.8", "Zoom −")] {
        let mut a = CadCompositionArgs::of(CompositionOp::Zoom);
        a.value = Some(value.into());
        add(id.into(), label.into(), a, Ok(()));
    }
    add(
        "replace_form".into(),
        "Edit complete source graph…".into(),
        CadCompositionArgs::of(CompositionOp::ReplaceForm),
        st.snapshot
            .as_ref()
            .map(|_| ())
            .ok_or("Wait for source graph".into()),
    );
    add(
        "arrange".into(),
        "Arrange display".into(),
        CadCompositionArgs::of(CompositionOp::Arrange),
        Ok(()),
    );
    for (id, axis, delta, label) in [
        ("pan_left", "x", "-40", "Pan left"),
        ("pan_right", "x", "40", "Pan right"),
        ("pan_up", "y", "-40", "Pan up"),
        ("pan_down", "y", "40", "Pan down"),
    ] {
        let mut a = CadCompositionArgs::of(CompositionOp::Pan);
        a.name = Some(axis.into());
        a.value = Some(delta.into());
        add(id.into(), label.into(), a, Ok(()));
    }
    for (index, draft) in st.drafts.iter().enumerate() {
        let mut a = CadCompositionArgs::of(CompositionOp::ResumeDraft);
        a.id = Some(index.to_string());
        add(
            format!("draft:{index}"),
            format!("Resume draft {index} · revision {}", draft.revision),
            a,
            Ok(()),
        );
        let mut a = CadCompositionArgs::of(CompositionOp::CopyDraft);
        a.id = Some(index.to_string());
        add(
            format!("copy_draft:{index}"),
            format!("Copy draft {index} to current revision"),
            a,
            Ok(()),
        );
    }
    add(
        "import_check".into(),
        "Read completed check bindings".into(),
        CadCompositionArgs::of(CompositionOp::ImportCheck),
        (!st.check_draft.is_empty())
            .then_some(())
            .ok_or("Enter a completed check ID".into()),
    );
    add(
        "clear_check".into(),
        "Clear imported check choice".into(),
        CadCompositionArgs::of(CompositionOp::ClearCheck),
        Ok(()),
    );
    if let Some(snapshot) = &st.snapshot {
        for t in &snapshot.types {
            let mut a = CadCompositionArgs::of(CompositionOp::New);
            a.id = Some(t.component_type.clone());
            add(
                format!("type:{}", t.component_type),
                format!("Add {}", t.name),
                a,
                t.parameters_complete
                    .then_some(())
                    .ok_or("Parameter metadata unavailable".into()),
            );
        }
        for (id, c) in &snapshot.graph.graph.components {
            let mut a = CadCompositionArgs::of(CompositionOp::Select);
            a.id = Some(id.clone());
            add(format!("component:{id}"), c.name.clone(), a, Ok(()));
            let mut a = CadCompositionArgs::of(CompositionOp::Remove);
            a.id = Some(id.clone());
            a.revision = Some(snapshot.graph.revision);
            add(
                format!("remove:{id}"),
                format!("Remove {}", c.name),
                a,
                doc.commit_refusal(Some(snapshot.graph.revision))
                    .map_or(Ok(()), Err),
            );
            let mut a = CadCompositionArgs::of(CompositionOp::Focus);
            a.id = Some(id.clone());
            add(
                format!("focus:{id}"),
                format!("Focus {}", c.name),
                a,
                Ok(()),
            );
        }
        for (id, p) in snapshot
            .presentation
            .iter()
            .flat_map(|presented| presented.projection.view.ports.iter())
        {
            let mut a = CadCompositionArgs::of(CompositionOp::Port);
            a.port = Some(CadEndpoint {
                component_id: p.component.clone(),
                port: p.name.clone(),
            });
            a.revision = Some(snapshot.graph.revision);
            add(
                format!("port:{id}"),
                format!(
                    "Connect {}.{}",
                    snapshot
                        .graph
                        .graph
                        .components
                        .get(&p.component)
                        .map(|c| c.name.as_str())
                        .unwrap_or(&p.component),
                    p.name
                ),
                a,
                doc.commit_refusal(Some(snapshot.graph.revision))
                    .map_or(Ok(()), Err),
            );
        }
        for id in snapshot.graph.graph.connections.keys() {
            let mut a = CadCompositionArgs::of(CompositionOp::Open);
            a.id = Some(id.clone());
            a.revision = Some(snapshot.graph.revision);
            add(
                format!("open:{id}"),
                format!("Remove connection {id}"),
                a,
                doc.commit_refusal(Some(snapshot.graph.revision))
                    .map_or(Ok(()), Err),
            );
        }
    }
    if let Some(pending) = &st.pending_port {
        let mut a = CadCompositionArgs::of(CompositionOp::LeaveOpen);
        a.revision = Some(pending.revision);
        add(
            "leave_open".into(),
            "Leave port open".into(),
            a,
            ports::validate_pending(doc, st, Some(pending.revision)).map(|_| ()),
        );
        add(
            "cancel_port".into(),
            "Cancel connection".into(),
            CadCompositionArgs::of(CompositionOp::CancelPort),
            Ok(()),
        );
    }
    if let Some(d) = st.current.and_then(|i| st.drafts.get(i)) {
        add(
            "add_parameter".into(),
            "Add named parameter".into(),
            CadCompositionArgs::of(CompositionOp::AddParameter),
            Ok(()),
        );
        add(
            "submit".into(),
            "Apply component".into(),
            CadCompositionArgs::of(CompositionOp::Submit),
            doc.commit_refusal(Some(d.revision)).map_or(Ok(()), Err),
        );
    }
    result
}
fn field(
    p: &mut ChildSpawnerCommands,
    k: &Kit,
    st: &CadCompositionState,
    name: &str,
    value: &str,
    label: &str,
) {
    p.spawn(k.text(label, size::DETAIL, SUBTLE, 0));
    p.spawn(k.input(
        value,
        label,
        Input(name.into()),
        st.typing.as_deref() == Some(name),
    ));
}
pub(crate) fn draw(
    p: &mut ChildSpawnerCommands,
    k: &Kit,
    doc: &CadDocument,
    st: &CadCompositionState,
) {
    let all = controls(doc, st);
    let button = |p: &mut ChildSpawnerCommands, id: &str| {
        if let Some((_, label, a, ready)) =
            all.iter().find(|c| c.0 == format!("cad:composition:{id}"))
        {
            p.spawn(k.button(label, CadButton(a.clone()), Look::Ghost, ready.is_ok()));
        }
    };
    button(p, "dock");
    // Pending intent outlives both the displayed snapshot and the open dock.
    // Use the same stamped readiness and typed actions as system_ui/REST.
    if st.pending_port.is_some() {
        button(p, "leave_open");
        button(p, "cancel_port");
    }
    if !st.open {
        return;
    }
    if let Some(error) = &st.error {
        p.spawn(k.text(error, size::DETAIL, WARN, 2));
    }
    button(p, "overview");
    button(p, "zoom_in");
    button(p, "zoom_out");
    for id in [
        "pan_left",
        "pan_right",
        "pan_up",
        "pan_down",
        "arrange",
        "replace_form",
    ] {
        button(p, id);
    }
    for (index, _) in st.drafts.iter().enumerate() {
        button(p, &format!("draft:{index}"));
        button(p, &format!("copy_draft:{index}"));
    }
    field(
        p,
        k,
        st,
        "check_id",
        &st.check_draft,
        "Completed check ID (read only)",
    );
    button(p, "import_check");
    button(p, "clear_check");
    if let Some(imported) = &st.imports {
        p.spawn(k.text(
            format!(
                "Check {} · {}{}{}",
                imported.run_id,
                imported.state,
                if imported.stale {
                    " · measured results stale"
                } else {
                    ""
                },
                if imported.metadata_stale {
                    " · binding metadata stale"
                } else {
                    ""
                }
            ),
            size::DETAIL,
            SUBTLE,
            2,
        ));
    }
    if let Some(snapshot) = &st.snapshot {
        // Actual shared layout coordinates drive the graph; routes are hyperedge
        // branches, never fabricated physical pair connections.
        if let Some(presented) = &snapshot.presentation {
            let layout = &presented.layout;
            let scale = if st.zoom == 0. { 0.35 } else { st.zoom * 0.35 };
            let [lo, hi] = layout.bounds;
            p.spawn(Node {
                position_type: PositionType::Relative,
                width: Val::Px(((hi.x - lo.x) * scale + 24.).max(260.)),
                height: Val::Px(((hi.y - lo.y) * scale + 24.).max(120.)),
                overflow: Overflow::clip(),
                ..default()
            })
            .with_children(|canvas| {
                for route in layout.nets.values() {
                    crate::graph_presentation::route(
                        canvas,
                        route,
                        |p| {
                            Vec2::new(
                                (p.x - lo.x) * scale + 12. + st.pan[0],
                                (p.y - lo.y) * scale + 12. + st.pan[1],
                            )
                        },
                        SUBTLE,
                        1.5,
                    );
                }
                for (id, node) in &layout.nodes {
                    let label = snapshot
                        .graph
                        .graph
                        .components
                        .get(id)
                        .map(|c| c.name.as_str())
                        .unwrap_or(id);
                    let mut a = CadCompositionArgs::of(CompositionOp::Select);
                    a.id = Some(id.clone());
                    canvas
                        .spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px((node.position.x - lo.x) * scale + 12. + st.pan[0]),
                                top: Val::Px((node.position.y - lo.y) * scale + 12. + st.pan[1]),
                                width: Val::Px(sim_diagram::layout::WIDTH * scale),
                                height: Val::Px(node.height * scale),
                                ..default()
                            },
                            BackgroundColor(crate::ui_kit::SURFACE),
                        ))
                        .with_children(|card| {
                            card.spawn(k.button(label, CadButton(a.action()), Look::Ghost, true));
                        });
                }
            });
        }
        for t in &snapshot.types {
            button(p, &format!("type:{}", t.component_type));
        }
        if let Some(d) = st.current.and_then(|i| st.drafts.get(i)) {
            if let Some(source) = d.fields.get("graph") {
                field(
                    p,
                    k,
                    st,
                    "graph",
                    source,
                    "Complete version-1 source graph JSON (Shift+Enter newline)",
                );
                button(p, "submit");
            } else {
                for (name, label) in [
                    ("name", "Name"),
                    ("binding", "Imported binding"),
                    ("body_id", "CAD body ID"),
                ] {
                    field(
                        p,
                        k,
                        st,
                        name,
                        d.fields.get(name).map(String::as_str).unwrap_or(""),
                        label,
                    );
                }
                if let Some(nodes) = doc.doc.as_ref().map(|d| &d.nodes) {
                    for n in nodes
                        .iter()
                        .filter(|n| matches!(n.kind.as_str(), "body" | "instance"))
                    {
                        let mut a = CadCompositionArgs::of(CompositionOp::SetField);
                        a.draft_index = st.current;
                        a.name = Some("body_id".into());
                        a.value = Some(n.id.clone());
                        p.spawn(k.button(
                            &format!("Attach {}", n.name),
                            CadButton(a.action()),
                            Look::Ghost,
                            true,
                        ));
                    }
                }
                if let Some(imported) = &st.imports {
                    for i in &imported.imported {
                        if d.fields.get("type") == Some(&i.component_type) {
                            let mut a = CadCompositionArgs::of(CompositionOp::SetField);
                            a.draft_index = st.current;
                            a.name = Some("binding".into());
                            a.value = Some(i.binding.clone());
                            p.spawn(k.button(
                                &format!("Bind {}", i.name),
                                CadButton(a.action()),
                                Look::Ghost,
                                !imported.metadata_stale,
                            ));
                        }
                    }
                }
                if let Some(t) = snapshot
                    .types
                    .iter()
                    .find(|t| d.fields.get("type") == Some(&t.component_type))
                {
                    p.spawn(k.text(format!("Type: {}", t.component_type), size::DETAIL, TEXT, 0));
                    for param in t.parameters.as_deref().unwrap_or(&[]) {
                        if param.name.contains('*') {
                            p.spawn(k.text(
                                format!(
                                    "Named parameters matching {} [{}]",
                                    param.name, param.unit
                                ),
                                size::DETAIL,
                                SUBTLE,
                                0,
                            ));
                            for (key, value) in d.fields.iter().filter(|(key, _)| {
                                key.strip_prefix("parameter.")
                                    .is_some_and(|name| adapter::matches(&param.name, name))
                            }) {
                                field(p, k, st, key, value, key);
                            }
                            field(
                                p,
                                k,
                                st,
                                "parameter_name",
                                d.fields
                                    .get("parameter_name")
                                    .map(String::as_str)
                                    .unwrap_or(""),
                                "Concrete parameter name",
                            );
                            field(
                                p,
                                k,
                                st,
                                "parameter_value",
                                d.fields
                                    .get("parameter_value")
                                    .map(String::as_str)
                                    .unwrap_or(""),
                                "Parameter value",
                            );
                            button(p, "add_parameter");
                            continue;
                        }
                        let name = format!("parameter.{}", param.name);
                        field(
                            p,
                            k,
                            st,
                            &name,
                            d.fields.get(&name).map(String::as_str).unwrap_or(""),
                            &format!(
                                "{} [{}] · blank = imported/default {:?}",
                                param.name, param.unit, param.default
                            ),
                        );
                    }
                    for (kind, recipe) in snapshot
                        .recipes
                        .iter()
                        .filter(|(_, r)| r.component_type == t.component_type)
                    {
                        let mut a = CadCompositionArgs::of(CompositionOp::SetField);
                        a.draft_index = st.current;
                        a.name = Some("recipe.kind".into());
                        a.value = Some(kind.clone());
                        p.spawn(k.button(
                            &format!(
                                "Geometry rule {kind} → {}",
                                recipe
                                    .outputs
                                    .keys()
                                    .cloned()
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                            CadButton(a.action()),
                            Look::Ghost,
                            true,
                        ));
                    }
                    if let Some(kind) = d.fields.get("recipe.kind") {
                        p.spawn(k.text(
                            format!("Derived by {kind}; geometry/provenance owned by RoboCAD"),
                            size::DETAIL,
                            SUBTLE,
                            2,
                        ));
                        let mut a = CadCompositionArgs::of(CompositionOp::SetField);
                        a.draft_index = st.current;
                        a.name = Some("recipe.kind".into());
                        a.value = Some(String::new());
                        p.spawn(k.button(
                            "Clear geometry rule",
                            CadButton(a.action()),
                            Look::Ghost,
                            true,
                        ));
                    }
                    if let Some(recipe) = d
                        .fields
                        .get("recipe.kind")
                        .and_then(|kind| snapshot.recipes.get(kind))
                    {
                        for (name, input) in &recipe.inputs {
                            let key = format!("recipe.{name}");
                            field(
                                p,
                                k,
                                st,
                                &key,
                                d.fields.get(&key).map(String::as_str).unwrap_or(""),
                                &format!(
                                    "{} [{}] · default {:?}",
                                    input.label, input.unit, input.default
                                ),
                            );
                        }
                    }
                }
                button(p, "submit");
            }
        }
        for (id, c) in &snapshot.graph.graph.components {
            button(p, &format!("component:{id}"));
            button(p, &format!("focus:{id}"));
            button(p, &format!("remove:{id}"));
            if let Some(recipe) = &c.derivation {
                p.spawn(k.text(
                    format!("{} · derived {} · body {:?}", c.name, recipe, c.body_id),
                    size::DETAIL,
                    SUBTLE,
                    2,
                ));
            }
        }
        for id in snapshot
            .presentation
            .iter()
            .flat_map(|presented| presented.projection.view.ports.keys())
        {
            button(p, &format!("port:{id}"));
        }
        for id in snapshot.graph.graph.connections.keys() {
            button(p, &format!("open:{id}"));
        }
    }
}
