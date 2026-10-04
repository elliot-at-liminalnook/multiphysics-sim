//! The print studies (ui/app.py:1166-1263): "Split selected for
//! printing…", "Check strength", "Plan print settings and plates", "Whole
//! or split for strength?", "Assembly guide…" and "Test coupons…". Each
//! start is one RoboCAD call through `actions::edit_at` (`POST
//! /print/split` with `background: true`, as RoboCAD's menu uses
//! `split_job`, or `POST /print/{kind}`), refused by name with nothing sent
//! when RoboCAD's document moved since the selection or form was read
//! (`expected_revision` makes RoboCAD refuse it too). The answer is the
//! started job, which [`super::jobs_tracker`] adopts and polls.
//!
//! - **The printing registry** (`GET /print/registry`: printers with
//!   their usable size, filaments, in the registry's order) is read on one
//!   `Pool::Dedicated` job once per connection generation; a failed read is
//!   kept with its error and read again after a reconnect or Refresh
//!   (`CadDocument::mesh_retry` moves) or in a new generation. The forms'
//!   "Printer:" and "Filament:" lists come from it ([`picks`]); the split
//!   and coupon forms (and `ops.print_split`'s) are refused by name until
//!   it is read ([`precheck`]).
//! - **The print study** (`GET /print/study`: `robot_settings
//!   ["print_study"]` and the split groups) is read on one Dedicated job
//!   per (generation, shown revision), the `robot::data` pattern: a job for
//!   an older key is dropped (cancelled) when a newer one starts.
//! - **Selection rules and texts** are RoboCAD's handlers': a strength
//!   check or plan without a study answers RoboCAD's explanation
//!   ([`NO_STUDY`], verbatim); whole-or-split takes the first study part
//!   whose node is selected; the assembly guide and the coupons take a
//!   selected split group, else the split group a selected piece is in.
use super::PrintCall;
use crate::app::actions::Call;
use crate::app::{ViewerMode, ViewerSet};
use crate::cad::actions::edit_at;
use crate::cad::document::{CadDocument, EditDone};
use crate::cad::ops::{Env, OpEntry, Resolved};
use crate::cad::sync::value;
use crate::jobs::{Job, Pool};
use bevy::prelude::*;
use serde_json::{Map, Value, json};
use sim_api::Outcome;
use sim_runtime::cad_client::{PrintRegistry, PrintStudy, SelectionItem, SplitRequest};

/// RoboCAD's "Check strength" explanation when the document has no print
/// study (ui/app.py:1190-1193), verbatim; Plan shows it too.
pub(super) const NO_STUDY: &str = "This document has no print study yet. Set robot_settings['print_study'] to a /print/analyze body (parts with fixtures and loads; loads may read a simulation), e.g. from a script, then run this again.\n\nExample region forms: {'contact': other_node}, {'bottom': true}, {'faces': [3, 4]}, {'sphere': {...}}.";
/// The registry is not read yet (the split and coupon forms wait for it).
pub(super) const REGISTRY_READING: &str = "the printing registry is still being read; try again in a moment";
/// The study keys RoboCAD's whole-or-split passes on (ui/app.py:1222).
const STRENGTH_SPLIT_KEYS: [&str; 5] = ["printer", "material", "simulation", "safety_target", "space"];

/// (document generation, `CadDocument::mesh_retry`) a read was made at: a
/// failed read is tried again when either moves (a reconnect or Refresh).
type Retry = (u64, u64);
/// (document generation, shown revision) the study was read at.
type Key = (u64, u64);

/// A print start this window sent: its edit and what it starts, so
/// `jobs_tracker::edit_answered` adopts the job RoboCAD answers.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Started {
    pub seq: u64,
    pub generation: u64,
    /// split | analyze | plan | strength_split | assembly | coupons.
    pub kind: &'static str,
}

/// The studies' state on the document.
#[derive(Default)]
pub(crate) struct StudiesState {
    /// The printing registry as last read, with when it was read.
    pub(super) registry: Option<(Retry, Result<PrintRegistry, String>)>,
    registry_job: Option<(Retry, Job<PrintRegistry>)>,
    /// The print study as last read: its key, the retry stamp, the answer.
    pub(super) study: Option<(Key, u64, Result<PrintStudy, String>)>,
    study_job: Option<(Key, u64, Job<PrintStudy>)>,
    /// The latest print start sent (until its edit answered).
    pub(super) started: Option<Started>,
}

/// What a study start sends.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Request {
    /// `POST /print/split` (background).
    Split(SplitRequest),
    /// `POST /print/{kind}` with RoboCAD's body.
    Start(Value),
}

/// A study run's start.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StudyPlan {
    /// The job's kind as RoboCAD names it.
    pub kind: &'static str,
    /// The revision the selection, the values and the study were read at.
    pub revision: u64,
    pub request: Request,
    /// The edit's label (refusals name it).
    pub label: String,
    /// The status line once RoboCAD answered the start.
    pub message: String,
}

/// The registry for this generation, or why it cannot be used now.
pub(super) fn registry(doc: &CadDocument) -> Result<&PrintRegistry, String> {
    match doc.print.studies.registry.as_ref().filter(|(k, _)| k.0 == doc.generation) {
        Some((_, Ok(r))) => Ok(r),
        Some((_, Err(e))) => Err(format!("RoboCAD's printing registry could not be read: {e}")),
        None if doc.connected() && doc.client.is_some() => Err(REGISTRY_READING.to_string()),
        None => Err(super::PRINT_UNPORTED.to_string()),
    }
}

/// The print study read at the shown revision, or why not.
pub(super) fn study(doc: &CadDocument) -> Result<&PrintStudy, String> {
    let now = (doc.generation, doc.shown_revision());
    match doc.print.studies.study.as_ref().filter(|(k, _, _)| *k == now) {
        Some((_, _, Ok(s))) => Ok(s),
        Some((_, _, Err(e))) => Err(format!("RoboCAD's print study could not be read: {e}")),
        None if doc.connected() && doc.client.is_some() => Err(format!("the print study is still being read (revision {}); try again in a moment", now.1)),
        None => Err(super::PRINT_UNPORTED.to_string()),
    }
}

/// The study as read at `revision` (the run's), or why not.
fn study_at(doc: &CadDocument, revision: u64) -> Result<&PrintStudy, String> {
    let s = study(doc)?;
    if s.revision != revision {
        return Err(format!("the print study was read at revision {}, the selection at {revision}; try again in a moment", s.revision));
    }
    Ok(s)
}

/// A non-empty text value.
fn text(values: &Map<String, Value>, name: &str) -> Option<String> {
    values.get(name).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string)
}

/// `value` must be one of the registry's `what` (when the registry is read;
/// otherwise RoboCAD names an unknown one itself).
fn known(doc: &CadDocument, value: &str, what: &str) -> Result<(), String> {
    let Ok(reg) = registry(doc) else { return Ok(()) };
    let list = ids_of(reg, what);
    if list.contains(&value) { Ok(()) } else { Err(format!("{value} is not a {what} of the printing registry (its {what}s: {})", list.join(", "))) }
}

/// The registry's ids of `what` ("printer" or "filament"), in its order.
fn ids_of<'r>(reg: &'r PrintRegistry, what: &str) -> Vec<&'r str> {
    if what == "printer" { reg.printers.keys().collect() } else { reg.materials.keys().collect() }
}

/// A required pick value: the given id, else why there is none.
fn chosen(doc: &CadDocument, values: &Map<String, Value>, name: &str, what: &str, label: &str) -> Result<String, String> {
    let Some(v) = text(values, name) else {
        return Err(match registry(doc) {
            Err(e) => e,
            Ok(_) => format!("{label}: choose a {what}"),
        });
    };
    known(doc, &v, what)?;
    Ok(v)
}

/// Node `id`'s parent in the shown tree.
fn parent_of(doc: &CadDocument, id: &str) -> Option<String> {
    doc.doc.as_ref()?.nodes.iter().find(|n| n.id == id)?.parent.clone()
}

/// RoboCAD's split groups of a selection (ui/app.py:1231-1234): the
/// selected nodes that are split groups, else the split groups the
/// selected nodes are in (their parents), each once, in selection order.
pub(super) fn split_groups(doc: &CadDocument, nodes: &[String], splits: &[String]) -> Vec<String> {
    let is_split = |id: &str| splits.iter().any(|s| s == id);
    let selected: Vec<String> = nodes.iter().filter(|n| is_split(n)).cloned().collect();
    if !selected.is_empty() {
        return selected;
    }
    let mut parents: Vec<String> = Vec::new();
    for n in nodes {
        if let Some(p) = parent_of(doc, n)
            && is_split(&p)
            && !parents.contains(&p)
        {
            parents.push(p);
        }
    }
    parents
}

/// `print::build_plan` for the study entries: RoboCAD's selection rules,
/// texts and bodies, with `expected_revision` = the run's revision.
pub(super) fn build(call: PrintCall, entry: &OpEntry, r: &Resolved, values: &Map<String, Value>, doc: &CadDocument, _env: &Env) -> Result<StudyPlan, String> {
    let revision = r.revision;
    let plan = |kind: &'static str, request: Request, label: String, message: String| StudyPlan { kind, revision, request, label, message };
    match call {
        PrintCall::Split => {
            let node = r.nodes.first().cloned().ok_or_else(|| entry.refusal.to_string())?;
            let printer = chosen(doc, values, "printer", "printer", entry.label)?;
            let joint = text(values, "joint").unwrap_or_else(|| "auto".to_string());
            let name = doc.node_name(&node);
            let message = format!("Started split of {name} for the {printer}");
            let request = SplitRequest { node, printer: Some(printer), joint: Some(joint), expected_revision: Some(revision), background: true, ..SplitRequest::default() };
            Ok(plan("split", Request::Split(request), format!("Split {name} for printing"), message))
        }
        PrintCall::Strength | PrintCall::Plan => {
            let s = study_at(doc, revision)?;
            if !s.has_study() {
                return Err(NO_STUDY.to_string());
            }
            let Value::Object(mut body) = s.study.clone() else {
                return Err("the document's print study (robot_settings print_study) is not an object; RoboCAD's /print/analyze body is one".to_string());
            };
            body.insert("expected_revision".into(), json!(revision));
            let n = s.part_nodes().len();
            Ok(if call == PrintCall::Strength {
                plan("analyze", Request::Start(Value::Object(body)), "Check strength".into(), format!("Started analyze: the strength of the print study's {n} part(s)"))
            } else {
                plan("plan", Request::Start(Value::Object(body)), "Plan print settings and plates".into(), format!("Started plan: settings and plates for the print study's {n} part(s)"))
            })
        }
        PrintCall::StrengthSplit => {
            // RoboCAD's handler checks the selection first (nothing selected
            // is its refusal, whatever the study read's state).
            if r.nodes.is_empty() {
                return Err(entry.refusal.to_string());
            }
            let s = study_at(doc, revision)?;
            let parts = s.study["parts"].as_array().map_or(&[][..], Vec::as_slice);
            let part = parts.iter().find(|p| p["node"].as_str().is_some_and(|n| r.nodes.iter().any(|x| x == n))).ok_or_else(|| entry.refusal.to_string())?;
            let node = part["node"].as_str().unwrap_or_default().to_string();
            let mut body = Map::new();
            if let Some(m) = s.study.as_object() {
                for k in STRENGTH_SPLIT_KEYS {
                    if let Some(v) = m.get(k) {
                        body.insert(k.into(), v.clone());
                    }
                }
            }
            body.insert("node".into(), json!(node));
            body.insert("part".into(), part.clone());
            body.insert("expected_revision".into(), json!(revision));
            let name = doc.node_name(&node);
            Ok(plan("strength_split", Request::Start(Value::Object(body)), "Whole or split for strength".into(), format!("Started strength_split for {name}")))
        }
        PrintCall::Assembly => {
            if r.nodes.is_empty() {
                return Err(entry.refusal.to_string());
            }
            let s = study_at(doc, revision)?;
            let group = split_groups(doc, &r.nodes, &s.splits).into_iter().next().ok_or_else(|| entry.refusal.to_string())?;
            let name = doc.node_name(&group);
            Ok(plan("assembly", Request::Start(json!({"group": group, "expected_revision": revision})), format!("Assembly guide for {name}"), format!("Started assembly for {name}")))
        }
        PrintCall::Coupons => {
            // With a selection the study read is required: split membership
            // comes only from GET /print/study, and sending group None for a
            // selected split would silently make material-only coupons.
            let group = if r.nodes.is_empty() { None } else { split_groups(doc, &r.nodes, &study_at(doc, revision)?.splits).into_iter().next() };
            let printer = chosen(doc, values, "printer", "printer", entry.label)?;
            let material = chosen(doc, values, "material", "filament", entry.label)?;
            let what = group.as_deref().map_or_else(|| format!("the {material}"), |g| doc.node_name(g));
            let body = json!({"group": group, "printer": printer, "material": material, "expected_revision": revision});
            Ok(plan("coupons", Request::Start(body), "Test coupons".into(), format!("Started coupons for {what} on the {printer}")))
        }
        _ => Err(format!("{} is not a print study", entry.id)),
    }
}

/// `print::send` for a study: ONE RoboCAD call through `actions::edit_at`
/// at the plan's revision; the edit's result is the started job
/// (`PrintJob`), adopted by `jobs_tracker::edit_answered`.
pub(super) fn send(doc: &mut CadDocument, call: &mut Call, plan: StudyPlan) -> Outcome {
    let StudyPlan { kind, revision, request, label, message } = plan;
    let before = doc.edit_seq;
    let outcome = match request {
        Request::Split(req) => edit_at(doc, call, Some(revision), label, move |c| c.print_split_job(&req).map(|job| EditDone { message, result: value(&job) })),
        Request::Start(body) => edit_at(doc, call, Some(revision), label, move |c| c.print_start(kind, &body).map(|job| EditDone { message, result: value(&job) })),
    };
    if doc.edit.is_some() && doc.edit_seq != before {
        doc.print.studies.started = Some(Started { seq: doc.edit_seq, generation: doc.generation, kind });
        doc.touch();
    }
    outcome
}

/// A pick list of the registry (key, label), in the registry's order:
/// "printers" ("id (x × y × z mm)", RoboCAD's split dialog), "printer_ids"
/// and "filaments" (ids, RoboCAD's coupon dialog). Empty while unread.
pub(super) fn picks(source: &str, doc: &CadDocument) -> Vec<(String, String)> {
    let Ok(reg) = registry(doc) else { return Vec::new() };
    match source {
        "printers" => reg.printers.0.iter().map(|(id, p)| (id.clone(), p.label(id, crate::cad::ops::g))).collect(),
        "printer_ids" => reg.printers.keys().map(|id| (id.to_string(), id.to_string())).collect(),
        "filaments" => reg.materials.keys().map(|id| (id.to_string(), id.to_string())).collect(),
        _ => Vec::new(),
    }
}

/// RoboCAD presets nothing beyond each list's first entry (its
/// `getItem(…, 0, False)`), which `robot_form::seed` normalises to.
pub(super) fn seed(_entry: &OpEntry, _doc: &CadDocument, _env: &Env, _texts: &mut [String]) {}

/// The split and coupon dialogs (and `ops.print_split`'s form) list the
/// registry: refused by name while it is being read or could not be read
/// (RoboCAD's `load()` would raise). `ops.print_split`'s printer is a pick
/// over "printer_ids": unread, that list is empty and the seed
/// normalisation would blank its default "bambu-h2c".
pub(super) fn precheck(entry: &OpEntry, doc: &CadDocument, _selection: &[SelectionItem]) -> Option<String> {
    if !matches!(entry.id, "print.split" | "print.coupons" | "ops.print_split") {
        return None;
    }
    match registry(doc) {
        Err(e) => Some(e),
        Ok(r) if r.printers.is_empty() => Some("the printing registry lists no printers".to_string()),
        Ok(r) if entry.id == "print.coupons" && r.materials.is_empty() => Some("the printing registry lists no filaments".to_string()),
        Ok(_) => None,
    }
}

/// `cad_state.print.studies`.
pub(super) fn state_json(doc: &CadDocument) -> Value {
    let s = &doc.print.studies;
    let registry = match s.registry.as_ref().filter(|(k, _)| k.0 == doc.generation) {
        Some((_, Ok(r))) => json!({"read": true, "printers": r.printers.keys().collect::<Vec<_>>(), "filaments": r.materials.keys().collect::<Vec<_>>(), "path": r.path, "sha256": r.sha256}),
        Some((_, Err(e))) => json!({"read": false, "error": e, "reading": s.registry_job.is_some()}),
        None => json!({"read": false, "reading": s.registry_job.is_some()}),
    };
    let now = (doc.generation, doc.shown_revision());
    let study = match s.study.as_ref() {
        Some((k, _, Ok(st))) => json!({"read_at": k.1, "current": *k == now, "revision": st.revision, "has_study": st.has_study(), "parts": st.part_nodes(), "splits": st.splits}),
        Some((k, _, Err(e))) => json!({"read_at": k.1, "current": *k == now, "error": e}),
        None => Value::Null,
    };
    json!({
        "registry": registry,
        "study": study,
        "study_reading": s.study_job.is_some(),
        "started": s.started.as_ref().map(|st| json!({"edit": st.seq, "generation": st.generation, "kind": st.kind})),
    })
}

/// The reads at the current keys: started when needed, an older key's job
/// dropped, a result landed (the `robot::data::sync` pattern).
pub(super) fn tick(doc: &mut CadDocument) {
    let (generation, retry_count) = (doc.generation, doc.mesh_retry);
    let retry = (generation, retry_count);
    let now = (generation, doc.shown_revision());
    let mut touched = false;
    let s = &mut doc.print.studies;
    // Registry: one read per generation.
    if s.registry_job.as_ref().is_some_and(|(k, _)| k.0 != generation) {
        s.registry_job = None;
    }
    let landed = s.registry_job.as_ref().and_then(|(k, job)| job.poll().map(|r| (*k, r)));
    if let Some((k, result)) = landed {
        s.registry_job = None;
        s.registry = Some((k, result));
        touched = true;
    }
    // Study: one read per (generation, shown revision).
    if s.study_job.as_ref().is_some_and(|(k, _, _)| *k != now) {
        s.study_job = None;
    }
    let landed = s.study_job.as_ref().and_then(|(k, at, job)| job.poll().map(|r| (*k, *at, r)));
    if let Some((k, at, result)) = landed {
        s.study_job = None;
        s.study = Some((k, at, result));
        touched = true;
    }
    let want_registry = s.registry_job.is_none()
        && match &s.registry {
            None => true,
            Some((k, Ok(_))) => k.0 != generation,
            Some((k, Err(_))) => *k != retry,
        };
    let want_study = s.study_job.is_none()
        && match &s.study {
            None => true,
            Some((k, _, Ok(_))) => *k != now,
            Some((k, at, Err(_))) => *k != now || *at != retry_count,
        };
    let client = doc.client.clone().filter(|_| doc.connected());
    if let Some(client) = client {
        if want_registry {
            let c = client.clone();
            let job = Job::spawn(Pool::Dedicated, generation, "cad print registry", move |_| c.print_registry().map_err(|e| e.to_string()));
            doc.print.studies.registry_job = Some((retry, job));
            touched = true;
        }
        if want_study && doc.doc_key.is_some() {
            let job = Job::spawn(Pool::Dedicated, generation, "cad print study", move |ctx| {
                if ctx.cancelled() {
                    return Err("superseded by a newer revision".to_string());
                }
                client.print_study().map_err(|e| e.to_string())
            });
            doc.print.studies.study_job = Some((now, retry_count, job));
            touched = true;
        }
    }
    if touched {
        doc.touch();
    }
}

/// JobResults: [`tick`].
fn sync(doc: Option<ResMut<CadDocument>>) {
    let Some(mut doc) = doc else { return };
    tick(&mut doc);
}

/// CadCorePlugin: the registry and study reads (JobResults, after `sync::receive`).
pub(super) fn build_core(app: &mut App) {
    app.add_systems(Update, sync.after(crate::cad::CadSet::Results).in_set(ViewerSet::JobResults).run_if(in_state(ViewerMode::Cad)));
}
