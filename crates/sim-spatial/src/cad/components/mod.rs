//! Native reusable assemblies. Ownership: RoboCAD owns source definitions,
//! occurrences, geometry, undo and commit revisions. `ComponentsState` owns
//! drafts and durable pending work across mode changes; only this handler
//! starts mutations. `jobs::tick` observes server work in JobResults, after
//! public CadSet::Results; field input writes the same typed action in Input.
//! The CAD panel owns transient draw entities (DespawnOnExit); kit text field
//! entities persist. Blocking IO and library scans run on jobs, never frames.
mod form;
mod jobs;
#[cfg(test)]
mod tests;
mod ui;
mod validate;

use crate::app::actions::Call;
use crate::cad::actions::{CadAction, Cx};
use crate::cad::document::CadDocument;
use crate::cad::selection::CadItems;
use crate::jobs::{Job, Pool};
use bevy::prelude::*;
pub(crate) use form::{ComponentsFormKind, Draft};
pub(crate) use jobs::key;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sim_api::Outcome;
use sim_runtime::cad_client::{
    ComponentCatalogue, ComponentJobStatus, ComponentLibrary, ComponentOperation, ComponentRecipes,
    ComponentStamp,
};
pub(crate) use ui::{build, draw};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ComponentsOp {
    #[default]
    State,
    Dock,
    Find,
    Select,
    Open,
    FormSet,
    Submit,
    CloseForm,
    Folder,
    ImportSelected,
    Resume,
    Cancel,
}

/// REST, system_ui and the window share this validated intent.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ComponentsArgs {
    #[serde(default)]
    pub op: ComponentsOp,
    #[serde(default)]
    pub open: Option<bool>,
    #[serde(default)]
    /// For open(link_family), occurrence-only; family comes from library selection.
    pub id: Option<String>,
    #[serde(default)]
    pub kind: Option<ComponentsFormKind>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub revision: Option<u64>,
    /// Typed direct operation, still checked against catalogue and source stamp.
    #[serde(default)]
    pub operation: Option<ComponentOperation>,
    #[serde(default)]
    pub draft_index: Option<usize>,
}
impl ComponentsArgs {
    pub(crate) fn of(op: ComponentsOp) -> Self {
        Self {
            op,
            ..Default::default()
        }
    }
    pub(crate) fn action(self) -> CadAction {
        CadAction::CadComponents(self)
    }
}

#[derive(Resource, Default)]
pub(crate) struct ComponentsState {
    pub open: bool,
    pub find: String,
    pub selected: Option<String>,
    /// Drafts are never dropped on refusal or mode exit.
    pub drafts: Vec<Draft>,
    pub current: Option<usize>,
    pub focus: Option<String>,
    pub folder: String,
    pub files: Vec<String>,
    pub error: Option<String>,
    pub catalogue: Option<(jobs::Identity, ComponentCatalogue)>,
    pub(crate) reads: Option<(
        jobs::Identity,
        u64,
        Job<(ComponentCatalogue, ComponentRecipes)>,
    )>,
    pub(crate) listing: Option<(jobs::Identity, Job<ComponentLibrary>)>,
    pub(crate) active: Option<jobs::Active>,
    pub history: Vec<ComponentJobStatus>,
    pub revision: u64,
    pub catalogue_revision: u64,
    pub recipes: Option<ComponentRecipes>,
    pub library_identity: Option<jobs::Identity>,
    pub read_at: Option<std::time::Instant>,
    pub selection_after: Option<(jobs::Identity, String)>,
}
impl ComponentsState {
    pub(crate) fn busy(&self) -> bool {
        self.active.is_some()
    }
    pub(crate) fn edit_refusal(&self) -> Option<String> {
        self.busy().then(|| {
            "A component rebuild is already in progress; wait or cancel it in Components".into()
        })
    }
    pub(crate) fn mode_blockers(&self) -> Vec<String> {
        self.edit_refusal().into_iter().collect()
    }
    fn touch(&mut self) {
        self.revision += 1;
    }
    pub(crate) fn draft(&self) -> Option<&Draft> {
        self.current.and_then(|i| self.drafts.get(i))
    }
    fn draft_mut(&mut self) -> Option<&mut Draft> {
        self.current.and_then(|i| self.drafts.get_mut(i))
    }
}

pub(crate) struct CoreParts;
impl Plugin for CoreParts {
    fn build(&self, app: &mut App) {
        app.init_resource::<ComponentsState>().add_systems(
            Update,
            jobs::tick
                .in_set(crate::app::ViewerSet::JobResults)
                .after(crate::cad::CadSet::Results),
        );
    }
}

pub(crate) fn handle(a: &ComponentsArgs, _call: &mut Call, cx: &mut Cx) -> Outcome {
    let items = cx.shared.items();
    let doc = &mut *cx.doc;
    let st = &mut *cx.components;
    let result = match a.op {
        ComponentsOp::State => Ok(state_json(doc, st)),
        ComponentsOp::Dock => {
            st.open = a.open.unwrap_or(!st.open);
            if !st.open {
                st.focus = None;
            }
            st.touch();
            Ok(json!({"open":st.open}))
        }
        ComponentsOp::Find => {
            st.find = a.value.clone().unwrap_or_default();
            st.touch();
            Ok(json!({"find":st.find}))
        }
        ComponentsOp::Select => {
            let id = a.id.clone().unwrap_or_default();
            if st
                .catalogue
                .as_ref()
                .is_some_and(|(_, c)| c.definitions.iter().any(|d| d.id == id))
            {
                st.selected = Some(id.clone());
                st.touch();
                Ok(json!({"selected":id}))
            } else {
                Err(format!(
                    "components.definition.{id}: no such definition in the current catalogue"
                ))
            }
        }
        ComponentsOp::Open => form::open(
            st,
            doc,
            a.kind.unwrap_or(ComponentsFormKind::Make),
            a.id.as_deref(),
            &items.nodes(),
        ),
        ComponentsOp::FormSet => form::set_at(
            st,
            a.draft_index,
            a.name.as_deref().unwrap_or(""),
            a.value.as_deref().unwrap_or(""),
        ),
        ComponentsOp::CloseForm => {
            st.current = None;
            st.focus = None;
            st.touch();
            Ok(json!({"draft_retained":true}))
        }
        ComponentsOp::Folder => folder(st, doc, a.path.as_deref().unwrap_or("")),
        ComponentsOp::ImportSelected => {
            let path = a.path.clone().unwrap_or_default();
            if !st.files.contains(&path) {
                Err(format!(
                    "components.library.{path}: select a listed component file"
                ))
            } else {
                form::open_import(st, doc, &path).and_then(|()| submit(st, doc, None, a.revision))
            }
        }
        ComponentsOp::Submit => {
            if a.operation.is_none() && a.draft_index.is_some() && a.draft_index != st.current {
                Err("components.draft: selected form changed; original draft retained".into())
            } else {
                submit(st, doc, a.operation.clone(), a.revision)
            }
        }
        ComponentsOp::Resume => {
            let i =
                a.id.as_deref()
                    .unwrap_or("")
                    .parse::<usize>()
                    .map_err(|_| "components.draft: invalid index".to_string());
            match i {
                Ok(i) if i < st.drafts.len() => {
                    st.current = Some(i);
                    st.focus = None;
                    st.open = true;
                    st.touch();
                    Ok(json!({"current":i}))
                }
                _ => Err("components.draft: no such retained draft".into()),
            }
        }
        ComponentsOp::Cancel => {
            if let Some(active) = st.active.as_mut() {
                active.cancel_requested = true;
                st.touch();
                Ok(json!({"cancelling":true}))
            } else {
                Err("No component rebuild is running".into())
            }
        }
    };
    if let Err(e) = &result {
        st.error = Some(e.clone());
        if let Some(d) = st.draft_mut() {
            d.error = Some(e.clone());
        }
        st.touch();
    }
    doc.touch();
    Outcome::Done(result)
}

fn submit(
    st: &mut ComponentsState,
    doc: &mut CadDocument,
    direct: Option<ComponentOperation>,
    revision: Option<u64>,
) -> Result<Value, String> {
    if let Some(e) = st.edit_refusal() {
        return Err(e);
    }
    let identity = jobs::Identity::of(doc)?;
    if st.catalogue.as_ref().is_none_or(|(id, _)| *id != identity)
        || st.catalogue_revision != doc.shown_revision()
    {
        return Err("components.catalogue: waiting for metadata at the shown document revision; drafts are preserved".into());
    }
    let (operation, began) = match direct {
        Some(op) => (
            op,
            revision.ok_or("components.revision: required for a direct operation")?,
        ),
        None => {
            let d = st.draft().ok_or("Open a component form first")?;
            if d.identity != identity {
                return Err("components.draft: document or connection changed; reopen against this document (the old draft is retained)".into());
            }
            (form::operation(d, st, doc)?, d.began)
        }
    };
    if let Some(e) = doc.commit_refusal(Some(began)) {
        return Err(e);
    }
    validate::validate_operation(&operation, st, doc)?;
    let client = doc.client.clone().ok_or("Not connected to RoboCAD")?;
    let stamp = ComponentStamp {
        document_id: identity.document_id.clone(),
        expected_revision: began,
    };
    let sent = operation.clone();
    let sending = client.clone();
    let job = Job::spawn(
        Pool::Dedicated,
        identity.generation,
        "cad component start",
        move |_| {
            sending
                .start_component(&sent, &stamp)
                .map_err(|e| e.to_string())
        },
    );
    doc.component_busy =
        Some("A component rebuild is in progress; wait or cancel in Components".into());
    st.active = Some(jobs::Active::new(identity, began, client, operation, job));
    st.error = None;
    st.touch();
    Ok(json!({"starting":true,"revision":began}))
}

fn folder(st: &mut ComponentsState, doc: &CadDocument, path: &str) -> Result<Value, String> {
    if !path.is_empty() && !std::path::Path::new(path).is_absolute() {
        return Err(
            "components.library.path: choose an absolute folder on the RoboCAD service host".into(),
        );
    }
    let client = doc.client.clone().ok_or("Not connected to RoboCAD")?;
    st.folder = path.into();
    let p = path.to_string();
    st.listing = Some((
        jobs::Identity::of(doc)?,
        Job::spawn(
            Pool::Dedicated,
            doc.generation,
            "component library folder",
            move |_| {
                client
                    .component_library((!p.is_empty()).then_some(p.as_str()))
                    .map_err(|e| e.to_string())
            },
        ),
    ));
    st.touch();
    Ok(json!({"folder":path,"listing":true}))
}

pub(crate) fn state_json(_doc: &CadDocument, st: &ComponentsState) -> Value {
    json!({"open":st.open,"find":st.find,"selected":st.selected,"folder":st.folder,"files":st.files,"error":st.error,"drafts":st.drafts,"current":st.current,"catalogue":st.catalogue.as_ref().map(|(_,c)|c),"job":st.active.as_ref().and_then(|a|a.status.as_ref()),"starting":st.active.as_ref().is_some_and(|a|a.start.is_some()),"cancel_requested":st.active.as_ref().is_some_and(|a|a.cancel_requested),"history":st.history,"revision":st.revision})
}

pub(crate) type Control = (String, String, CadAction, Result<(), String>);
pub(crate) fn controls(cx: &Cx) -> Vec<Control> {
    controls_of(cx.doc, cx.components)
}
pub(crate) fn controls_of(doc: &CadDocument, st: &ComponentsState) -> Vec<Control> {
    let mut out = vec![(
        "cad:components:dock".into(),
        "Components".into(),
        ComponentsArgs::of(ComponentsOp::Dock).action(),
        Ok(()),
    )];
    for kind in ComponentsFormKind::ALL {
        out.push((
            format!("cad:components:{}", kind.name()),
            kind.label().into(),
            ComponentsArgs {
                kind: Some(*kind),
                ..ComponentsArgs::of(ComponentsOp::Open)
            }
            .action(),
            Ok(()),
        ));
    }
    if let Some((_, catalogue)) = &st.catalogue {
        for d in &catalogue.definitions {
            if !d.name.to_lowercase().contains(&st.find.to_lowercase()) {
                continue;
            }
            let count = doc.doc.as_ref().map_or(0, |doc| {
                doc.nodes
                    .iter()
                    .filter(|n| {
                        n.component_instance
                            .as_ref()
                            .and_then(|i| i["definition_id"].as_str())
                            == Some(d.id.as_str())
                    })
                    .count()
            });
            out.push((
                format!("cad:components:definition-{}", d.id),
                format!(
                    "{} · r{} · {} placed{}",
                    d.name,
                    d.revision,
                    count,
                    if st.selected.as_deref() == Some(d.id.as_str()) {
                        " · selected"
                    } else {
                        ""
                    }
                ),
                ComponentsArgs {
                    id: Some(d.id.clone()),
                    ..ComponentsArgs::of(ComponentsOp::Select)
                }
                .action(),
                Ok(()),
            ));
        }
    }
    out.push((
        "cad:components:folder".into(),
        "Choose / refresh folder".into(),
        ComponentsArgs {
            path: Some(st.folder.clone()),
            ..ComponentsArgs::of(ComponentsOp::Folder)
        }
        .action(),
        if doc.connected() {
            Ok(())
        } else {
            Err("Not connected to RoboCAD".into())
        },
    ));
    for (i, file) in st.files.iter().enumerate() {
        out.push((
            format!("cad:components:library-{i}"),
            file.clone(),
            ComponentsArgs {
                path: Some(file.clone()),
                revision: Some(doc.shown_revision()),
                ..ComponentsArgs::of(ComponentsOp::ImportSelected)
            }
            .action(),
            st.edit_refusal().map_or(Ok(()), Err),
        ));
    }
    if st.draft().is_some() {
        out.push((
            "cad:components:submit".into(),
            "Apply component".into(),
            ComponentsArgs {
                draft_index: st.current,
                ..ComponentsArgs::of(ComponentsOp::Submit)
            }
            .action(),
            st.edit_refusal()
                .or_else(|| doc.commit_refusal(st.draft().map(|d| d.began)))
                .map_or(Ok(()), Err),
        ));
        out.push((
            "cad:components:close_form".into(),
            "Close (keep draft)".into(),
            ComponentsArgs::of(ComponentsOp::CloseForm).action(),
            Ok(()),
        ));
    }
    for (i, d) in st.drafts.iter().enumerate() {
        if Some(i) != st.current {
            out.push((
                format!("cad:components:resume-{i}"),
                format!("Resume {} draft", d.kind.label()),
                ComponentsArgs {
                    id: Some(i.to_string()),
                    ..ComponentsArgs::of(ComponentsOp::Resume)
                }
                .action(),
                Ok(()),
            ));
        }
    }
    if st.busy() {
        out.push((
            "cad:components:cancel".into(),
            "Cancel rebuild".into(),
            ComponentsArgs::of(ComponentsOp::Cancel).action(),
            Ok(()),
        ));
    }
    out
}
pub(crate) fn command_action(id: &str) -> Option<CadAction> {
    match id {
        "components.show" => Some(
            ComponentsArgs {
                open: Some(true),
                ..ComponentsArgs::of(ComponentsOp::Dock)
            }
            .action(),
        ),
        "components.make" => Some(
            ComponentsArgs {
                kind: Some(ComponentsFormKind::Make),
                ..ComponentsArgs::of(ComponentsOp::Open)
            }
            .action(),
        ),
        _ => None,
    }
}

pub(crate) fn specs() -> Vec<crate::app::actions::Spec> {
    vec![crate::app::actions::spec(
        "cad_components",
        crate::cad::actions::CAD,
        json!({"op":"open","kind":"place"}),
        "Reusable assemblies. op: state, dock(open), find(value), select(id definition), open(kind make/create/parametric/place/defaults/overrides/reset/detach/transform/import/export/family/link_family; id optional: definition for place/defaults/export, occurrence for overrides/reset/detach/link_family; link_family uses the selected library family), form_set(name,value text), submit (retained form or typed operation with required revision), close_form (retains draft), resume(id draft index), folder(path absolute on service host; absent uses RoboCAD default), import_selected(path,revision), cancel (durable request). Every window and system_ui control uses this handler; source edits are guarded by document ID and revision and prepared by RoboCAD. Rebuilds block other edits and document/mode changes until terminal. Unsaved/rejected form drafts survive close and mode exit.",
    )]
}
