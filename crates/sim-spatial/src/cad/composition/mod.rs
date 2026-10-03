//! Composition ownership: RoboCAD owns source graph, geometry and undo. This
//! resource owns retained drafts and disposable presentation. Actions are
//! occurrences through CadAction; reads/layout are jobs consumed in public
//! JobResults after CadSet::Results. Panel children inherit its linked lifetime.
//! No layout, zoom or focus intent sends a source mutation.
mod adapter;
mod jobs;
mod ports;
use jobs::imports_current;
#[cfg(test)]
mod tests;
mod ui;
use crate::{
    app::actions::{Call, Spec, spec},
    cad::{
        actions::{CAD, CadAction, Cx, edit_at},
        document::{CadDocument, EditDone},
    },
    jobs::{Job, Pool},
};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sim_api::Outcome;
use sim_runtime::cad_client::composition::*;
use std::collections::{BTreeMap, BTreeSet};
pub(crate) use ui::{build, draw};

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CompositionOp {
    #[default]
    State,
    Dock,
    New,
    Select,
    SetField,
    ReplaceForm,
    ResumeDraft,
    CopyDraft,
    Pan,
    Arrange,
    AddParameter,
    Submit,
    Remove,
    Port,
    Open,
    LeaveOpen,
    CancelPort,
    ImportCheck,
    ClearCheck,
    Overview,
    Focus,
    Zoom,
}
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CadCompositionArgs {
    #[serde(default)]
    pub op: CompositionOp,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub port: Option<CadEndpoint>,
    #[serde(default)]
    pub command: Option<GraphCommand>,
    #[serde(default)]
    pub revision: Option<u64>,
    #[serde(default)]
    pub draft_index: Option<usize>,
}
impl CadCompositionArgs {
    pub(crate) fn of(op: CompositionOp) -> Self {
        Self {
            op,
            ..Default::default()
        }
    }
    pub(crate) fn action(self) -> CadAction {
        CadAction::CadComposition(self)
    }
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Draft {
    pub document_id: Option<String>,
    pub generation: u64,
    pub revision: u64,
    pub original: Option<String>,
    pub fields: BTreeMap<String, String>,
    pub error: Option<String>,
}
pub(crate) struct Snapshot {
    pub graph: GraphSnapshot,
    pub types: Vec<SystemType>,
    pub recipes: BTreeMap<String, Recipe>,
    pub presentation: Option<sim_diagram::composition::Presentation>,
    pub presentation_error: Option<String>,
    check_id: String,
    imports: Option<ImportedSnapshot>,
}
#[derive(Resource, Default)]
pub(crate) struct CadCompositionState {
    pub open: bool,
    pub drafts: Vec<Draft>,
    pub current: Option<usize>,
    pub selected: Option<String>,
    pending_port: Option<ports::PendingPort>,
    submitted_port: Option<ports::SubmittedPort>,
    pub imports: Option<ImportedSnapshot>,
    pub check_id: String,
    pub check_draft: String,
    pub error: Option<String>,
    pub zoom: f32,
    pub pan: [f32; 2],
    pub focus: Option<String>,
    pub revision: u64,
    pub snapshot: Option<Snapshot>,
    snapshot_key: Option<(u64, u64)>,
    read: Option<((u64, u64), Job<Snapshot>)>,
    imported_job: Option<(u64, Job<ImportedSnapshot>)>,
    failed: Option<(u64, u64)>,
    pub typing: Option<String>,
}
pub(crate) struct CoreParts;
impl Plugin for CoreParts {
    fn build(&self, app: &mut App) {
        app.init_resource::<CadCompositionState>().add_systems(
            Update,
            jobs::tick
                .in_set(crate::app::ViewerSet::JobResults)
                .after(crate::cad::CadSet::Results),
        );
    }
}
fn set_check(st: &mut CadCompositionState, check: String) {
    st.read = None;
    st.snapshot = None;
    st.snapshot_key = None;
    st.imports = None;
    st.imported_job = None;
    st.failed = None;
    if st.pending_port.take().is_some() {
        st.error = Some(
            "composition.port: pending connection cancelled because imported check changed".into(),
        );
    }
    st.check_id = check;
}
pub(crate) fn handle(a: &CadCompositionArgs, call: &mut Call, cx: &mut Cx) -> Outcome {
    let st = &mut *cx.composition;
    let doc = &mut *cx.doc;
    let result: Result<Value, String> = (|| match a.op {
        CompositionOp::State => Ok(state_json(doc, st)),
        CompositionOp::Dock => {
            st.open = !st.open;
            Ok(json!({"open":st.open}))
        }
        CompositionOp::New | CompositionOp::Select => {
            let mut fields = BTreeMap::new();
            let mut original = None;
            if matches!(a.op, CompositionOp::Select) {
                let id = a.id.as_deref().ok_or("composition.id: required")?;
                let c = st
                    .snapshot
                    .as_ref()
                    .and_then(|s| s.graph.graph.components.get(id))
                    .ok_or("composition.id: unavailable component")?;
                if let Some(body) = &c.body_id {
                    cx.shared.set([sim_runtime::cad_client::SelectionItem(
                        body.clone(),
                        "body".into(),
                        0,
                    )])?;
                }
                original = Some(id.into());
                st.selected = Some(id.into());
                fields.insert("name".into(), c.name.clone());
                fields.insert("type".into(), c.component_type.clone());
                fields.insert("body_id".into(), c.body_id.clone().unwrap_or_default());
                fields.insert("binding".into(), c.binding.clone().unwrap_or_default());
                if let Some(recipe) = &c.derivation {
                    for (k, v) in recipe
                        .as_object()
                        .ok_or("composition.derivation: object required")?
                    {
                        fields.insert(
                            format!("recipe.{k}"),
                            v.as_str()
                                .map(str::to_string)
                                .unwrap_or_else(|| v.to_string()),
                        );
                    }
                }
                for (n, v) in &c.parameters {
                    fields.insert(format!("parameter.{n}"), v.to_string());
                }
            } else {
                fields.insert("name".into(), "Component".into());
                fields.insert("type".into(), a.id.clone().unwrap_or_default());
            }
            st.drafts.push(Draft {
                document_id: doc.doc.as_ref().and_then(|d| d.document_id.clone()),
                generation: doc.generation,
                revision: doc.shown_revision(),
                original,
                fields,
                error: None,
            });
            st.current = Some(st.drafts.len() - 1);
            st.typing = None;
            Ok(json!({"draft":st.current}))
        }
        CompositionOp::ReplaceForm => {
            let graph = st
                .snapshot
                .as_ref()
                .ok_or("composition.graph: wait for source snapshot")?;
            let source =
                serde_json::to_string_pretty(&graph.graph.graph).map_err(|e| e.to_string())?;
            st.drafts.push(Draft {
                document_id: doc.doc.as_ref().and_then(|d| d.document_id.clone()),
                generation: doc.generation,
                revision: doc.shown_revision(),
                original: None,
                fields: BTreeMap::from([("graph".into(), source)]),
                error: None,
            });
            st.current = Some(st.drafts.len() - 1);
            st.typing = None;
            Ok(json!({"draft":st.current}))
        }
        CompositionOp::ResumeDraft | CompositionOp::CopyDraft => {
            let index =
                a.id.as_deref()
                    .ok_or("composition.draft.id: required")?
                    .parse::<usize>()
                    .map_err(|_| "composition.draft.id: index required")?;
            let draft = st
                .drafts
                .get(index)
                .ok_or("composition.draft.id: unknown retained draft")?
                .clone();
            if matches!(a.op, CompositionOp::CopyDraft) {
                let mut copied = draft;
                copied.document_id = doc.doc.as_ref().and_then(|d| d.document_id.clone());
                copied.generation = doc.generation;
                copied.revision = doc.shown_revision();
                copied.error = None;
                st.drafts.push(copied);
                st.current = Some(st.drafts.len() - 1);
            } else {
                st.current = Some(index);
                st.typing = None;
            }
            Ok(json!({"draft":st.current}))
        }
        CompositionOp::Pan => {
            let axis = match a.name.as_deref() {
                Some("x") => 0,
                Some("y") => 1,
                _ => return Err("composition.pan.name: x or y required".into()),
            };
            let delta = a
                .value
                .as_deref()
                .unwrap_or("0")
                .parse::<f32>()
                .map_err(|_| "composition.pan.value: numeric display offset required")?;
            if !delta.is_finite() {
                return Err("composition.pan.value: finite required".into());
            }
            st.pan[axis] = (st.pan[axis] + delta).clamp(-10000., 10000.);
            Ok(json!({"pan":st.pan}))
        }
        CompositionOp::Arrange => {
            st.pan = [0., 0.];
            st.zoom = 1.;
            st.snapshot = None;
            st.snapshot_key = None;
            st.read = None;
            st.failed = None;
            Ok(json!({"arranging":true}))
        }
        CompositionOp::SetField => {
            let name = a.name.as_deref().ok_or("composition.field: required")?;
            if name == "check_id" {
                st.check_draft = a.value.clone().unwrap_or_default();
                return Ok(json!({"check_id":st.check_draft}));
            }
            if name == "recipe.kind" {
                let mut copied = a
                    .draft_index
                    .or(st.current)
                    .and_then(|i| st.drafts.get(i))
                    .ok_or("Open a composition form first")?
                    .clone();
                copied.fields.retain(|key, _| !key.starts_with("recipe."));
                copied
                    .fields
                    .insert(name.into(), a.value.clone().unwrap_or_default());
                st.drafts.push(copied);
                st.current = Some(st.drafts.len() - 1);
                return Ok(json!({"draft":st.current}));
            }
            let draft = a
                .draft_index
                .or(st.current)
                .and_then(|i| st.drafts.get_mut(i))
                .ok_or("Open a composition form first")?;
            draft
                .fields
                .insert(name.into(), a.value.clone().unwrap_or_default());
            Ok(json!({"draft":draft}))
        }
        CompositionOp::AddParameter => {
            let draft = a
                .draft_index
                .or(st.current)
                .and_then(|i| st.drafts.get_mut(i))
                .ok_or("Open a composition form first")?;
            let name = draft
                .fields
                .get("parameter_name")
                .cloned()
                .unwrap_or_default();
            if name.is_empty() || name.contains('*') {
                return Err("composition.parameter_name: concrete parameter name required".into());
            }
            let value = draft
                .fields
                .get("parameter_value")
                .cloned()
                .unwrap_or_default();
            draft.fields.insert(format!("parameter.{name}"), value);
            Ok(json!({"draft":draft}))
        }
        CompositionOp::ImportCheck => {
            let check = a.id.clone().unwrap_or_else(|| st.check_draft.clone());
            if check.trim().is_empty() {
                return Err("composition.check_id: completed check ID required".into());
            }
            let client = doc.client.clone().ok_or("Not connected to RoboCAD")?;
            set_check(st, check.clone());
            st.imported_job = Some((
                doc.generation,
                Job::spawn(
                    Pool::Dedicated,
                    doc.generation,
                    "read imported component bindings",
                    move |_| client.system_imports(&check).map_err(|e| e.to_string()),
                ),
            ));
            Ok(json!({"reading":true}))
        }
        CompositionOp::ClearCheck => {
            set_check(st, String::new());
            st.check_draft.clear();
            Ok(json!({"check_id":null}))
        }
        CompositionOp::Overview | CompositionOp::Focus => {
            st.focus = if matches!(a.op, CompositionOp::Focus) {
                a.id.clone().or_else(|| st.selected.clone())
            } else {
                None
            };
            st.snapshot = None;
            st.failed = None;
            st.read = None;
            Ok(json!({"focus":st.focus}))
        }
        CompositionOp::Zoom => {
            let factor = a
                .value
                .as_deref()
                .unwrap_or("1")
                .parse()
                .map_err(|_| "composition.zoom: numeric factor required")?;
            st.zoom =
                sim_diagram::composition::zoom(if st.zoom == 0. { 1. } else { st.zoom }, factor);
            Ok(json!({"zoom":st.zoom}))
        }
        CompositionOp::Port => ports::pick(doc, st, call, a),
        CompositionOp::LeaveOpen => ports::leave_open(doc, st, call, a.revision),
        CompositionOp::CancelPort => Ok(ports::cancel(st)),
        CompositionOp::Remove => mutation(
            doc,
            st,
            call,
            GraphCommand::Remove {
                id: a.id.clone().ok_or("composition.id: required")?,
            },
            a.revision.unwrap_or(doc.shown_revision()),
        ),
        CompositionOp::Open => mutation(
            doc,
            st,
            call,
            GraphCommand::Open {
                id: a.id.clone().ok_or("composition.id: required")?,
            },
            a.revision.unwrap_or(doc.shown_revision()),
        ),
        CompositionOp::Submit => {
            let (command, revision) = if let Some(c) = &a.command {
                (
                    c.clone(),
                    a.revision.ok_or("composition.revision: required")?,
                )
            } else {
                draft_command_at(doc, st, a.draft_index.or(st.current))?
            };
            mutation(doc, st, call, command, revision)
        }
    })();
    if let Err(error) = &result {
        st.error = Some(error.clone());
        // Only a draft's own edit or submission is named on that draft (the
        // one addressed, not whichever form is open); a refused port pick,
        // pan or check read is not the form's error.
        let draft_op = matches!(a.op, CompositionOp::SetField | CompositionOp::AddParameter)
            || (matches!(a.op, CompositionOp::Submit) && a.command.is_none());
        if draft_op
            && let Some(d) = a.draft_index.or(st.current).and_then(|i| st.drafts.get_mut(i))
        {
            d.error = Some(error.clone());
        }
    }
    st.revision += 1;
    doc.touch();
    if result
        .as_ref()
        .is_ok_and(|v| v.get("pending") == Some(&Value::Bool(true)))
        && call.rest()
    {
        Outcome::Pending
    } else {
        Outcome::Done(result)
    }
}
#[cfg(test)]
fn draft_command(
    doc: &CadDocument,
    st: &CadCompositionState,
) -> Result<(GraphCommand, u64), String> {
    draft_command_at(doc, st, st.current)
}
fn draft_command_at(
    doc: &CadDocument,
    st: &CadCompositionState,
    index: Option<usize>,
) -> Result<(GraphCommand, u64), String> {
    let draft = index
        .and_then(|i| st.drafts.get(i))
        .ok_or("Open a composition form first")?;
    if draft.generation != doc.generation {
        return Err("composition.draft: document changed; old draft retained".into());
    }
    if draft.document_id.is_none()
        || draft.document_id.as_deref() != doc.doc.as_ref().and_then(|d| d.document_id.as_deref())
    {
        return Err("composition.draft.document_id: document changed; old draft retained".into());
    }
    if let Some(why) = doc.commit_refusal(Some(draft.revision)) {
        return Err(why);
    }
    if let Some(source) = draft.fields.get("graph") {
        let graph = serde_json::from_str::<CadGraph>(source)
            .map_err(|e| format!("composition.graph: {e}"))?;
        graph.validate_storage()?;
        return Ok((GraphCommand::Replace { graph }, draft.revision));
    }
    let get = |n: &str| draft.fields.get(n).cloned().unwrap_or_default();
    let mut parameters = BTreeMap::new();
    let mut recipe = serde_json::Map::new();
    for (key, value) in &draft.fields {
        if value.trim().is_empty() {
            continue;
        }
        if let Some(n) = key.strip_prefix("parameter.") {
            parameters.insert(
                n.into(),
                value
                    .parse::<f64>()
                    .map_err(|_| format!("composition.parameters.{n}: finite number required"))?,
            );
        }
        if let Some(n) = key.strip_prefix("recipe.") {
            recipe.insert(
                n.into(),
                if n == "kind" {
                    json!(value)
                } else {
                    json!(
                        value
                            .parse::<f64>()
                            .map_err(|_| format!("composition.derivation.{n}: number required"))?
                    )
                },
            );
        }
    }
    let optional = |n: &str| {
        let s = get(n);
        (!s.is_empty()).then_some(s)
    };
    let c = CadComponent {
        id: draft.original.clone().unwrap_or_else(|| "draft".into()),
        name: get("name"),
        component_type: get("type"),
        body_id: optional("body_id"),
        binding: optional("binding"),
        parameters,
        derivation: recipe
            .get("kind")
            .filter(|v| v.as_str().is_some_and(|s| !s.is_empty()))
            .map(|_| Value::Object(recipe.clone())),
    };
    Ok((
        match &draft.original {
            Some(id) => GraphCommand::Update {
                id: id.clone(),
                component: c,
            },
            None => GraphCommand::Add { component: c },
        },
        draft.revision,
    ))
}
fn mutation(
    doc: &mut CadDocument,
    st: &mut CadCompositionState,
    call: &mut Call,
    command: GraphCommand,
    revision: u64,
) -> Result<Value, String> {
    if let Some(why) = doc.commit_refusal(Some(revision)) {
        return Err(why);
    }
    if st.snapshot_key != Some((doc.generation, revision)) {
        return Err(
            "composition.graph: wait for the current document revision; source draft retained"
                .into(),
        );
    }
    let snapshot = st
        .snapshot
        .as_ref()
        .ok_or("composition.catalogue: wait for current graph")?;
    let document_id = doc
        .doc
        .as_ref()
        .and_then(|d| d.document_id.clone())
        .ok_or("composition.document_id: service did not identify document")?;
    if snapshot.graph.document_id.as_deref() != Some(document_id.as_str()) {
        return Err(
            "composition.document_id: source snapshot belongs to a different document".into(),
        );
    }
    let mut graph = snapshot.graph.graph.clone();
    match &command {
        GraphCommand::Add { component } => {
            if graph.components.contains_key(&component.id) {
                return Err("composition.component.id: already exists".into());
            }
            graph
                .components
                .insert(component.id.clone(), component.clone());
        }
        GraphCommand::Update { id, component } => {
            if component.id != *id || !graph.components.contains_key(id) {
                return Err(
                    "composition.component.id: missing component or identity mismatch".into(),
                );
            }
            graph.components.insert(id.clone(), component.clone());
        }
        GraphCommand::Connect { ports } => {
            let mut ports = ports.clone();
            let joined = graph
                .connections
                .iter()
                .filter(|(_, n)| n.ports.iter().any(|p| ports.contains(p)))
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            for id in joined {
                if let Some(net) = graph.connections.remove(&id) {
                    for port in net.ports {
                        if !ports.contains(&port) {
                            ports.push(port);
                        }
                    }
                }
            }
            graph.connections.insert(
                "pending".into(),
                CadConnection {
                    id: "pending".into(),
                    ports,
                },
            );
        }
        GraphCommand::Replace { graph: replacement } => {
            graph = replacement.clone();
        }
        GraphCommand::Remove { id } => {
            graph.components.remove(id);
        }
        GraphCommand::Open { .. } => {}
    }
    if !matches!(
        command,
        GraphCommand::Remove { .. } | GraphCommand::Open { .. }
    ) {
        adapter::adapt(
            &graph,
            revision,
            &snapshot.types,
            st.imports
                .as_ref()
                .filter(|i| !i.metadata_stale)
                .map(|i| i.imported.as_slice())
                .unwrap_or(&[]),
            &snapshot.recipes,
        )?;
    }
    let client = doc.client.clone().ok_or("Not connected to RoboCAD")?;
    // The shared CAD edit path starts the job and guards document/generation.
    let sent = command.clone();
    let bound = graph.components.values().any(|c| c.binding.is_some());
    if bound
        && st
            .imports
            .as_ref()
            .is_some_and(|i| !imports_current(doc, i))
    {
        return Err("composition.imports: metadata belongs to an older revision; refresh or remove bindings".into());
    }
    let check = (bound && !st.check_id.is_empty()).then(|| st.check_id.clone());
    match edit_at(
        doc,
        call,
        Some(revision),
        "Edit system composition".into(),
        move |_| {
            let result = client.composition_edit_guarded(
                revision,
                &sent,
                check.as_deref(),
                Some(&document_id),
            )?;
            Ok(EditDone {
                message: "System composition updated".into(),
                result: serde_json::to_value(result).unwrap_or(Value::Null),
            })
        },
    ) {
        Outcome::Done(result) => result,
        Outcome::Pending => Ok(json!({"pending":true})),
        Outcome::Image(_) => Err("system composition edits don't produce images".into()),
    }
}
pub(crate) fn state_json(_doc: &CadDocument, st: &CadCompositionState) -> Value {
    json!({"open":st.open,"drafts":st.drafts,"current":st.current,"selected":st.selected,"pending_port":st.pending_port,"submitted_port":st.submitted_port,"error":st.error,"graph":st.snapshot.as_ref().map(|s|&s.graph),"types":st.snapshot.as_ref().map(|s|&s.types),"recipes":st.snapshot.as_ref().map(|s|&s.recipes),"imports":st.imports,"zoom":st.zoom,"pan":st.pan,"focus":st.focus,"reading":st.read.is_some(),"revision":st.revision})
}
pub(crate) fn key(st: &CadCompositionState) -> String {
    format!("{}:{}:{:?}", st.open, st.revision, st.typing)
}
pub(crate) fn specs() -> Vec<Spec> {
    vec![spec(
        "cad_composition",
        CAD,
        json!({"op":"state"}),
        "CAD composition: metadata-backed types, parameters, geometry recipes, imported bindings and typed connections. port(endpoint,revision) captures the displayed document/generation/revision; leave_open(revision) creates an unused singleton at that stamp; cancel_port clears pending intent without a source edit; open(id,revision) removes an existing whole connection. Mutations require source revision; overview/focus/zoom/layout are presentation only. Drafts survive refusals.",
    )]
}
pub(crate) fn controls_of(
    doc: &CadDocument,
    st: &CadCompositionState,
) -> Vec<(String, String, CadAction, Result<(), String>)> {
    ui::controls(doc, st)
}

pub(crate) fn port_edit_answered(
    st: &mut CadCompositionState,
    generation: u64,
    seq: u64,
    result: Result<(), String>,
) {
    ports::answered(st, generation, seq, result);
}
