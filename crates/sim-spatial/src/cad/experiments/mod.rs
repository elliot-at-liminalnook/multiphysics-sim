//! Durable experiment authoring. CAD sources/undo remain service-owned; this
//! resource owns retained draft versions, captured request stamps and pending
//! cancellation across dock/mode teardown. Panel entities are parent-owned.
//! Field occurrences write CadExperiments in Input; the sole CAD apply handler
//! validates them in Actions. Jobs poll in public JobResults after Cad Results,
//! without a mode gate. Blocking IO/catalogue/linked reads are jobs, never frames.
mod automatic;
mod form;
mod lifecycle;
mod reads;
mod source;
#[cfg(test)]
mod tests;
mod ui;
use crate::app::actions::Call;
use crate::cad::CadDocument;
use crate::cad::actions::{CadAction, Cx};
use bevy::prelude::*;
pub(crate) use form::Draft;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sim_api::Outcome;
use std::collections::BTreeMap;
pub(crate) use ui::{build, draw};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExperimentsOp {
    #[default]
    State,
    Dock,
    New,
    Rebase,
    Resume,
    Set,
    CloseDraft,
    Refresh,
    Select,
    Preflight,
    Run,
    Cancel,
    Discover,
    Restore,
    RestoreGraph,
    Baseline,
    Compare,
    Link,
    Auto,
    CandidateSelect,
    CandidateCreate,
    CandidateRead,
    CandidateAccept,
    CandidateDiscard,
    CandidateRun,
    Script,
    Batch,
    ImportComposition,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExperimentsArgs {
    #[serde(default)]
    pub op: ExperimentsOp,
    #[serde(default)]
    pub open: Option<bool>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub draft_index: Option<usize>,
    #[serde(default)]
    pub draft_sequence: Option<u64>,
    #[serde(default)]
    pub revision: Option<u64>,
    #[serde(default)]
    pub automatic_epoch: Option<u64>,
}
impl ExperimentsArgs {
    pub(crate) fn of(op: ExperimentsOp) -> Self {
        Self {
            op,
            ..Default::default()
        }
    }
    pub(crate) fn action(self) -> CadAction {
        CadAction::CadExperiments(self)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Stamp {
    pub generation: u64,
    pub document_id: String,
    pub revision: u64,
    pub draft_index: usize,
    pub sequence: u64,
}
impl Stamp {
    pub(crate) fn of(doc: &CadDocument, index: usize, sequence: u64) -> Result<Self, String> {
        let id = doc
            .doc_key
            .as_ref()
            .and_then(|(id, _)| id.clone())
            .ok_or("experiments.document_id: waiting for authoritative document")?;
        Ok(Self {
            generation: doc.generation,
            document_id: id,
            revision: doc.shown_revision(),
            draft_index: index,
            sequence,
        })
    }
    pub(crate) fn document_matches(&self, doc: &CadDocument) -> bool {
        doc.generation == self.generation
            && doc.doc_key.as_ref().and_then(|(id, _)| id.as_deref())
                == Some(self.document_id.as_str())
    }
    pub(crate) fn draft_matches(&self, st: &ExperimentsState) -> bool {
        st.current == Some(self.draft_index)
            && st
                .drafts
                .get(self.draft_index)
                .is_some_and(|d| d.stamp == *self)
    }
}
#[derive(Resource, Default)]
pub(crate) struct ExperimentsState {
    pub open: bool,
    pub automatic_epoch: u64,
    pub(crate) queued_automatic: Option<automatic::Queued>,
    pub drafts: Vec<Draft>,
    pub current: Option<usize>,
    pub focus: Option<String>,
    pub focus_index: Option<usize>,
    pub selected: Option<String>,
    pub candidate: Option<String>,
    pub baseline: Option<String>,
    pub history: Vec<Value>,
    pub candidates: Vec<Value>,
    pub catalogue: Option<Value>,
    pub diagnostics: Option<Value>,
    pub comparison: Option<Value>,
    pub inputs: Option<Value>,
    pub error: Option<String>,
    pub revision: u64,
    pub completed_check: Option<(u64, String, u64, String)>,
    pub(crate) active: Option<lifecycle::Active>,
    pub(crate) read: Option<lifecycle::Read>,
    pub(crate) linked: Option<lifecycle::Linked>,
    pub(crate) last_read: Option<std::time::Instant>,
    pub(crate) read_identity: Option<(u64, String)>,
}
impl ExperimentsState {
    pub(crate) fn touch(&mut self) {
        self.revision += 1;
    }
    pub(crate) fn draft(&self) -> Option<&Draft> {
        self.current.and_then(|i| self.drafts.get(i))
    }
    pub(crate) fn busy(&self) -> bool {
        self.active.is_some()
    }
    pub(crate) fn request_cancel(&mut self) {
        if let Some(a) = self.active.as_mut() {
            a.cancel_requested = true;
            self.touch();
        }
    }
    pub(crate) fn mode_blockers(&self) -> Vec<String> {
        self.active
            .as_ref()
            .map(|_| "A source operation is awaiting acknowledgement in Experiments".to_string())
            .into_iter()
            .collect()
    }
    pub(crate) fn edit_refusal(&self) -> Option<String> {
        self.mode_blockers().into_iter().next()
    }
}
pub(crate) struct CoreParts;
impl Plugin for CoreParts {
    fn build(&self, app: &mut App) {
        app.init_resource::<ExperimentsState>().add_systems(
            Update,
            lifecycle::tick
                .in_set(crate::app::ViewerSet::JobResults)
                .after(crate::cad::CadSet::Results),
        );
    }
}

pub(crate) fn key(st: &ExperimentsState) -> String {
    st.revision.to_string()
}
pub(crate) fn handle(a: &ExperimentsArgs, call: &mut Call, cx: &mut Cx) -> Outcome {
    if matches!(
        a.op,
        ExperimentsOp::Script
            | ExperimentsOp::Batch
            | ExperimentsOp::CandidateAccept
            | ExperimentsOp::RestoreGraph
    ) {
        return source::source_edit(a, call, cx);
    }
    if a.op == ExperimentsOp::ImportComposition {
        let Some((generation, id, revision, check)) = cx.experiments.completed_check.clone() else {
            return Outcome::Done(Err("experiments.check: complete a preflight first".into()));
        };
        if cx.doc.generation != generation
            || cx.doc.doc_key.as_ref().and_then(|(id, _)| id.as_deref()) != Some(id.as_str())
            || cx.doc.shown_revision() != revision
        {
            return Outcome::Done(Err(
                "experiments.check: document changed since preflight".into()
            ));
        }
        return crate::cad::composition::handle(
            &crate::cad::composition::CadCompositionArgs {
                op: crate::cad::composition::CompositionOp::ImportCheck,
                id: Some(check),
                ..Default::default()
            },
            call,
            cx,
        );
    }
    let doc = &mut *cx.doc;
    let st = &mut *cx.experiments;
    let result = (|| -> Result<Value, String> {
        use ExperimentsOp as O;
        match a.op {
            O::State => return Ok(state_json(doc, st)),
            O::Dock => {
                automatic::dock(st, a.open.unwrap_or(!st.open));
            }
            O::New => {
                let index = st.drafts.len();
                let stamp = Stamp::of(doc, index, 0)?;
                st.drafts.push(Draft::new(stamp));
                st.current = Some(index);
                st.focus = None;
                st.focus_index = None;
                automatic::dock(st, true);
            }
            O::Rebase => {
                if let Some(error) = doc.commit_refusal(None) {
                    return Err(error);
                }
                let mut d = st
                    .draft()
                    .cloned()
                    .ok_or("Open or resume a retained draft first")?;
                if !d.stamp.document_matches(doc) {
                    return Err("experiments.rebase: refuse copying a draft into another document; original retained".into());
                }
                d.stamp = Stamp::of(doc, st.drafts.len(), d.stamp.sequence + 1)?;
                d.edited = std::time::Instant::now();
                d.submitted_sequence = None;
                st.current = Some(st.drafts.len());
                st.drafts.push(d);
                st.focus = None;
            }
            O::Resume => {
                let index =
                    a.id.as_deref()
                        .unwrap_or("")
                        .parse::<usize>()
                        .map_err(|_| "experiments.draft: invalid index")?;
                if index >= st.drafts.len() {
                    return Err("experiments.draft: no such retained draft".into());
                }
                st.current = Some(index);
                st.focus = None;
                automatic::dock(st, true);
            }
            O::Set => form::set(st, a)?,
            O::CloseDraft => {
                st.current = None;
                st.focus = None;
            }
            O::Refresh => lifecycle::refresh(st, doc)?,
            O::Select => {
                if st.read.is_some() {
                    return Err(
                        "experiments.read: prior request pending; selection preserved".into(),
                    );
                }
                let id = a.id.clone().ok_or("experiments.id: required")?;
                if !st
                    .history
                    .iter()
                    .any(|r| r["id"].as_str() == Some(id.as_str()))
                {
                    return Err("experiments.id: choose a listed captured run".into());
                }
                st.selected = Some(id);
                lifecycle::read_selected(st, doc, false)?;
            }
            O::Preflight | O::Run | O::CandidateRun => lifecycle::start(st, doc, a)?,
            O::Cancel => {
                let active = st
                    .active
                    .as_mut()
                    .ok_or("No experiment operation is pending")?;
                active.cancel_requested = true;
            }
            O::Discover => lifecycle::discover(st)?,
            O::Restore => lifecycle::read_selected(st, doc, true)?,
            O::RestoreGraph | O::ImportComposition => {
                unreachable!("handled by shared source/import path")
            }
            O::Baseline => {
                st.baseline = Some(st.selected.clone().ok_or("Select a run first")?);
            }
            O::Compare => lifecycle::compare(st, doc)?,
            O::Link => form::link(st, a)?,
            O::Auto => {
                automatic::configure(st, a.open)?;
            }
            O::CandidateSelect | O::CandidateRead => {
                if st.read.is_some() {
                    return Err(
                        "experiments.read: prior request pending; selection preserved".into(),
                    );
                }
                let id =
                    a.id.clone()
                        .or_else(|| st.candidate.clone())
                        .ok_or("Select a candidate first")?;
                if !st
                    .candidates
                    .iter()
                    .any(|r| r["id"].as_str() == Some(id.as_str()))
                {
                    return Err("candidate.id: choose a listed candidate".into());
                }
                st.candidate = Some(id);
                lifecycle::read_candidate(st, doc)?;
            }
            O::CandidateCreate
            | O::CandidateAccept
            | O::CandidateDiscard
            | O::Script
            | O::Batch => lifecycle::mutate(st, doc, a)?,
        }
        st.touch();
        doc.touch();
        Ok(json!({"accepted":true,"draft_retained":true}))
    })();
    if let Err(error) = &result {
        st.error = Some(error.clone());
        if let Some(i) = st.current {
            st.drafts[i].error = Some(error.clone());
        }
        st.touch();
        doc.touch();
    }
    Outcome::Done(result)
}
pub(crate) fn state_json(_doc: &CadDocument, st: &ExperimentsState) -> Value {
    json!({"open":st.open,"automatic_epoch":st.automatic_epoch,"drafts":st.drafts,"current":st.current,"selected":st.selected,"candidate":st.candidate,"baseline":st.baseline,"history":st.history,"candidates":st.candidates,"catalogue":st.catalogue,"diagnostics":st.diagnostics,"comparison":st.comparison,"inputs":st.inputs,"error":st.error,"completed_check":st.completed_check,"active":st.active.as_ref().map(lifecycle::Active::json),"revision":st.revision})
}
pub(crate) type Control = (String, String, CadAction, Result<(), String>);
pub(crate) fn controls(cx: &Cx) -> Vec<Control> {
    controls_of(cx.doc, cx.experiments)
}
pub(crate) fn controls_of(doc: &CadDocument, st: &ExperimentsState) -> Vec<Control> {
    use ExperimentsOp as O;
    let mut out = Vec::new();
    let available = if st.busy() {
        Err("An operation is pending; cancel or discover its outcome".into())
    } else {
        Ok(())
    };
    for (op, id, label) in [
        (O::Dock, "dock", "Experiments"),
        (O::New, "new", "New retained experiment draft"),
        (
            O::Rebase,
            "rebase",
            "Copy retained inputs to current CAD revision",
        ),
        (
            O::Refresh,
            "refresh",
            "Refresh runs / candidates / registry",
        ),
        (O::Preflight, "preflight", "Check system"),
        (O::Run, "run", "Run captured experiment"),
        (O::Restore, "restore", "Restore run inputs to draft"),
        (
            O::RestoreGraph,
            "restore_graph",
            "Restore captured graph · undoable",
        ),
        (O::Baseline, "baseline", "Set baseline"),
        (O::Compare, "compare", "Inspect / compare"),
        (O::Auto, "auto", "Toggle rerun after edits · 750 ms"),
        (
            O::CandidateCreate,
            "candidate_create",
            "Create isolated candidate",
        ),
        (O::CandidateRead, "candidate_read", "Read candidate changes"),
        (
            O::CandidateAccept,
            "candidate_accept",
            "Accept candidate · undoable",
        ),
        (
            O::CandidateDiscard,
            "candidate_discard",
            "Refuse candidate · retain record",
        ),
        (O::CandidateRun, "candidate_run", "Experiment on candidate"),
        (
            O::Script,
            "script",
            "Run repository model script · undoable",
        ),
        (O::Batch, "batch", "Apply atomic edit batch · undoable"),
        (
            O::ImportComposition,
            "import_composition",
            "Import completed preflight into composition",
        ),
        (O::CloseDraft, "close_draft", "Close draft · keep inputs"),
    ] {
        let mut ready = Ok(());
        if matches!(
            op,
            O::Preflight
                | O::Run
                | O::CandidateCreate
                | O::CandidateRun
                | O::Script
                | O::Batch
                | O::RestoreGraph
                | O::CandidateAccept
                | O::CandidateDiscard
        ) {
            ready = available.clone().and_then(|_| {
                form::guard_for(
                    doc,
                    st,
                    matches!(op, O::Run | O::Preflight | O::CandidateRun),
                )
                .map(|_| ())
            });
        }
        if ready.is_ok() && matches!(op, O::Run | O::Preflight | O::CandidateRun) {
            ready = form::request(doc, st, op == O::Preflight).map(|_| ());
            if let Some(d) = st.draft() {
                if !d.linked.is_empty()
                    && (st.linked.is_some()
                        || d.linked.keys().any(|key| !d.restored.contains_key(key)))
                {
                    ready = Err("Waiting for stamped linked source capture".into());
                }
            }
        }
        if ready.is_ok() && matches!(op, O::CandidateCreate | O::Batch) {
            ready = form::candidate_request(doc, st).map(|_| ());
        }
        if matches!(
            op,
            O::Refresh | O::Select | O::Restore | O::Compare | O::CandidateRead
        ) && (!doc.connected() || doc.stale.is_some() || st.read.is_some())
        {
            ready = Err("Waiting for a current connected document and completed read".into());
        }
        if matches!(op, O::Restore | O::Baseline | O::Compare | O::RestoreGraph)
            && st.selected.is_none()
        {
            ready = Err("Select a captured run".into());
        }
        if matches!(
            op,
            O::CandidateRead | O::CandidateAccept | O::CandidateDiscard | O::CandidateRun
        ) && st.candidate.is_none()
        {
            ready = Err("Select a candidate".into());
        }
        if op == O::Compare && st.baseline.is_none() {
            ready = Err("Set a baseline first".into());
        }
        if op == O::ImportComposition {
            ready = match &st.completed_check {
                Some((generation, id, revision, _))
                    if doc.generation == *generation
                        && doc.doc_key.as_ref().and_then(|(id, _)| id.as_deref())
                            == Some(id.as_str())
                        && doc.shown_revision() == *revision =>
                {
                    Ok(())
                }
                _ => Err("Complete a preflight against the current document first".into()),
            };
        }
        out.push((
            format!("cad:experiments:{id}"),
            label.into(),
            ExperimentsArgs {
                draft_index: st.current,
                draft_sequence: st.draft().map(|d| d.stamp.sequence),
                revision: Some(doc.shown_revision()),
                ..ExperimentsArgs::of(op)
            }
            .action(),
            ready,
        ));
    }
    for (id, label, run, candidate) in [
        ("review", "Review captured run", st.selected.clone(), None),
        (
            "candidate_review",
            "Review isolated candidate",
            None,
            st.candidate.clone(),
        ),
    ] {
        let ready = if run.is_some() || candidate.is_some() {
            Ok(())
        } else {
            Err("Select a captured source first".into())
        };
        out.push((
            format!("cad:experiments:{id}"),
            label.into(),
            crate::cad::experiment_review::ReviewArgs {
                op: crate::cad::experiment_review::ReviewOp::Open,
                id: run,
                candidate,
                baseline: st.baseline.clone(),
                ..Default::default()
            }
            .action(),
            ready,
        ));
    }
    if st.busy() {
        out.push((
            "cad:experiments:cancel".into(),
            "Cancel pending experiment operation".into(),
            ExperimentsArgs::of(O::Cancel).action(),
            Ok(()),
        ));
        out.push((
            "cad:experiments:discover".into(),
            "Discover ambiguous outcome · read only".into(),
            ExperimentsArgs::of(O::Discover).action(),
            Ok(()),
        ));
    }
    for (i, _) in st.drafts.iter().enumerate() {
        out.push((
            format!("cad:experiments:resume-{i}"),
            format!("Resume retained draft {i}"),
            ExperimentsArgs {
                id: Some(i.to_string()),
                ..ExperimentsArgs::of(O::Resume)
            }
            .action(),
            Ok(()),
        ));
    }
    for (kind, list, op) in [
        ("run", &st.history, O::Select),
        ("candidate", &st.candidates, O::CandidateSelect),
    ] {
        for record in list {
            if let Some(id) = record["id"].as_str() {
                out.push((
                    format!("cad:experiments:{kind}-{id}"),
                    format!(
                        "{} · {} · {} · r{}",
                        record["label"].as_str().unwrap_or(id),
                        record["state"].as_str().unwrap_or("unknown"),
                        id,
                        record["revision"]
                    ),
                    ExperimentsArgs {
                        id: Some(id.into()),
                        ..ExperimentsArgs::of(op)
                    }
                    .action(),
                    Ok(()),
                ));
            }
        }
    }
    out
}
pub(crate) fn command_action(id: &str) -> Option<CadAction> {
    match id {
        "view.experiments" => Some(
            ExperimentsArgs {
                open: Some(true),
                ..ExperimentsArgs::of(ExperimentsOp::Dock)
            }
            .action(),
        ),
        "simulation.experiment" => Some(ExperimentsArgs::of(ExperimentsOp::Run).action()),
        _ => None,
    }
}
pub(crate) fn specs() -> Vec<crate::app::actions::Spec> {
    vec![crate::app::actions::spec(
        "cad_experiments",
        crate::cad::actions::CAD,
        json!({"op":"dock","open":true}),
        "Captured experiments and candidates: state, dock, new, resume(id), set(name,value,draft_index,draft_sequence), close_draft, refresh, select(id), preflight, run, cancel, discover (read-only unknown-outcome recovery), restore (new retained draft), restore_graph (guarded undo), baseline, compare, link(name system/controller,value absolute path), auto(open), candidate_select/read/create/accept/discard/run, script and batch. All rendered controls share this handler. Drafts and rejected sources survive teardown; stamps guard document generation/identity/revision and draft sequence; never retry unknown POSTs.",
    )]
}
