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
use crate::cad::types::{
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
    /// A copy of a retained draft re-stamped at the shown document and
    /// revision: the explicit consent a stale draft needs before it can be
    /// sent again (the original stays as it was).
    CopyDraft,
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

pub(crate) fn handle(a: &ComponentsArgs, call: &mut Call, cx: &mut Cx) -> Outcome {
    if call.cancelled && matches!(a.op, ComponentsOp::Submit | ComponentsOp::ImportSelected) {
        return Outcome::Done(Err("components: cancelled before submission; draft retained".into()));
    }
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
        // The draft being applied keeps the values that were sent: an
        // applied draft closes and is hidden, so later edits would be lost.
        ComponentsOp::FormSet if jobs::applying(st, a.draft_index.or(st.current)) => {
            Err(APPLYING.into())
        }
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
            } else if a.revision.is_none() {
                // As a direct submit: the row's revision is its stamp.
                Err("components.revision: required for import_selected (the revision the library row was offered at); nothing was sent".into())
            } else if let Some(revision) = a.revision.filter(|r| *r != doc.shown_revision()) {
                // The row was offered at another revision: nothing is sent.
                Err(format!(
                    "components.revision: the library row was read at revision {revision}, the document is now at revision {}; nothing was sent: choose the file again",
                    doc.shown_revision()
                ))
            } else {
                match form::open_import(st, doc, &path) {
                    Err(e) => Err(e),
                    Ok(()) => {
                        // The import form just opened is the draft refused.
                        let sent = submit(st, doc, None, a.revision);
                        if let (Err(e), Some(d)) = (&sent, st.draft_mut()) {
                            d.error = Some(e.clone());
                        }
                        sent
                    }
                }
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
                Ok(i) if st.drafts.get(i).is_some_and(|d| d.applied) => Err(applied_refusal(i)),
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
        ComponentsOp::CopyDraft => copy_draft(st, doc, a.id.as_deref().unwrap_or("")),
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
        // Only a form's own edit or submission is named on that draft (the
        // one addressed); a refused Select, Cancel or folder read is not.
        // The lock on a draft being applied is the job's state, not the
        // draft's: it is not stored on the draft (it would outlive the job).
        let draft_op = (a.op == ComponentsOp::FormSet && e != APPLYING)
            || (a.op == ComponentsOp::Submit && a.operation.is_none());
        if draft_op
            && let Some(d) = a.draft_index.or(st.current).and_then(|i| st.drafts.get_mut(i))
        {
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
    // The retained draft this request is built from; a reported refusal or
    // failure names it even when another form is open by then.
    let draft = if direct.is_none() { st.current } else { None };
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
    let client = crate::cad::component_service::service(doc)?;
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
            // The jobs RoboCAD already has: if the POST's answer is lost,
            // recovery adopts only a job that is not among them. A failed
            // listing sends nothing, and says so in its own variant.
            let known = match sending.component_jobs() {
                Ok(list) => list.into_iter().map(|s| s.id).collect::<Vec<_>>(),
                Err(e) => return Ok(jobs::Started::NotSent(e.to_string())),
            };
            Ok(jobs::Started::Sent(
                known,
                sending
                    .start_component(&sent, &stamp)
                    .map_err(|e| e.to_string()),
            ))
        },
    );
    // Named in every refusal it causes: "a component rebuild is in
    // progress: place component; wait or cancel it in Components".
    doc.component_busy = Some(operation.op_name().replace('_', " "));
    st.active = Some(jobs::Active::new(identity, began, client, operation, job, draft));
    st.error = None;
    st.touch();
    Ok(json!({"starting":true,"revision":began}))
}

/// Never rebases silently: the copy keeps every field, selection and target
/// of the original, which RoboCAD and `validate_operation` check again
/// against the current model when it is applied.
fn copy_draft(st: &mut ComponentsState, doc: &CadDocument, id: &str) -> Result<Value, String> {
    let index = id
        .parse::<usize>()
        .map_err(|_| "components.draft: invalid index".to_string())?;
    let mut copied = st
        .drafts
        .get(index)
        .ok_or("components.draft: no such retained draft")?
        .clone();
    if copied.applied {
        return Err(applied_refusal(index));
    }
    copied.identity = jobs::Identity::of(doc)?;
    copied.began = doc.shown_revision();
    copied.error = None;
    copied.applied = false;
    st.drafts.push(copied);
    st.current = Some(st.drafts.len() - 1);
    st.focus = None;
    st.open = true;
    st.touch();
    Ok(json!({"current":st.current,"copied_from":index,"revision":doc.shown_revision()}))
}

/// FormSet's refusal on the draft a running job was built from.
const APPLYING: &str =
    "components.draft: this draft is being applied; wait for the rebuild or cancel it";

/// RoboCAD applied this draft: resuming or copying it would apply the same
/// change again, so it is not offered (as in the window).
fn applied_refusal(index: usize) -> String {
    format!(
        "components.draft.{index}: RoboCAD already applied this draft; open a new form to change the model again"
    )
}

fn folder(st: &mut ComponentsState, doc: &CadDocument, path: &str) -> Result<Value, String> {
    if !path.is_empty() && !std::path::Path::new(path).is_absolute() {
        return Err(
            "components.library.path: choose an absolute folder".into(),
        );
    }
    let client = crate::cad::component_service::service(doc)?;
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
            Err("Open a CAD document first".into())
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
        if Some(i) != st.current && !d.applied {
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
    // A draft taken at another document or revision is refused when sent;
    // copying it to the shown revision is the explicit way to reuse it.
    let here = jobs::Identity::of(doc).ok();
    for (i, d) in st.drafts.iter().enumerate() {
        if !d.applied && (here.as_ref() != Some(&d.identity) || d.began != doc.shown_revision()) {
            out.push((
                format!("cad:components:copy-{i}"),
                format!(
                    "Copy {} draft (revision {}) to revision {}",
                    d.kind.label(),
                    d.began,
                    doc.shown_revision()
                ),
                ComponentsArgs {
                    id: Some(i.to_string()),
                    ..ComponentsArgs::of(ComponentsOp::CopyDraft)
                }
                .action(),
                if here.is_some() {
                    Ok(())
                } else {
                    Err("components.document_id: the authoritative document has not been read".into())
                },
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
        "Reusable assemblies. op: state, dock(open), find(value), select(id definition), open(kind make/create/parametric/place/defaults/overrides/reset/detach/transform/import/export/family/link_family; id optional: definition for place/defaults/export, occurrence for overrides/reset/detach/link_family; link_family uses the selected library family), form_set(name,value text; refused on the draft being applied), submit (retained form or typed operation with required revision), close_form (retains draft), resume(id draft index; not an applied draft), copy_draft(id draft index, not an applied draft: a copy re-stamped at the shown document and revision, the consent a stale draft needs), folder(path absolute on service host; absent uses RoboCAD default), import_selected(path, revision required: the row's revision), cancel (durable request). Every window and system_ui control uses this handler; source edits are guarded by document ID and revision and prepared by RoboCAD. Rebuilds block other edits and document/mode changes until terminal. Unsaved/rejected form drafts survive close and mode exit.",
    )]
}
