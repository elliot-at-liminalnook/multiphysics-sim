//! The print studies (ui/app.py:1166-1263): "Split selected for
//! printing…", "Check strength", "Plan print settings and plates", "Whole
//! or split for strength?", "Assembly guide…" and "Test coupons…". Each
//! start runs in process on the shown snapshot ([`super::jobs_tracker`]),
//! refused by name with nothing started when the document moved since the
//! selection or form was read.
//!
//! - **The printing registry** (`library/printing/registry.json`, read by
//!   `sim_print::registry`: printers with their usable size, filaments, in
//!   the file's order) is read on one `Pool::Io` job once per document
//!   generation; a failed read is kept with its error and read again after
//!   Refresh (`CadDocument::mesh_retry` moves) or in a new generation. The
//!   forms' "Printer:" and "Filament:" lists come from it ([`picks`]); the
//!   split and coupon forms (and `ops.print_split`'s) are refused by name
//!   until it is read ([`precheck`]).
//! - **The print study** (`robot_settings["print_study"]` and the split
//!   groups, nodes with `robot.print_split`, in tree order) is read from
//!   the shown archive when asked ([`study`]): nothing to wait for.
//! - **Selection rules and texts** are RoboCAD's handlers': a strength
//!   check or plan without a study answers RoboCAD's explanation
//!   ([`NO_STUDY`], verbatim); whole-or-split takes the first study part
//!   whose node is selected; the assembly guide and the coupons take a
//!   selected split group, else the split group a selected piece is in.
use super::PrintCall;
use crate::app::actions::Call;
use crate::app::{ViewerMode, ViewerSet};
use crate::cad::document::CadDocument;
use crate::cad::ops::{Env, OpEntry, Resolved};
use crate::cad::sync::value;
use crate::jobs::{Job, Pool};
use bevy::prelude::*;
use serde_json::{Map, Value, json};
use sim_api::Outcome;
use crate::cad::types::{FilamentInfo, Ordered, PrintRegistry, PrintStudy, PrinterInfo, SelectionItem, SplitRequest};

/// RoboCAD's "Check strength" explanation when the document has no print
/// study (ui/app.py:1190-1193), verbatim; Plan shows it too.
pub(super) const NO_STUDY: &str = "This document has no print study yet. Set robot_settings['print_study'] to a /print/analyze body (parts with fixtures and loads; loads may read a simulation), e.g. from a script, then run this again.\n\nExample region forms: {'contact': other_node}, {'bottom': true}, {'faces': [3, 4]}, {'sphere': {...}}.";
/// The registry is not read yet (the split and coupon forms wait for it).
pub(super) const REGISTRY_READING: &str = "the printing registry is still being read; try again in a moment";
/// The study keys RoboCAD's whole-or-split passes on (ui/app.py:1222).
const STRENGTH_SPLIT_KEYS: [&str; 5] = ["printer", "material", "simulation", "safety_target", "space"];

/// (document generation, `CadDocument::mesh_retry`) a read was made at: a
/// failed read is tried again when either moves (a Refresh).
type Retry = (u64, u64);

/// The studies' state on the document.
#[derive(Default)]
pub(crate) struct StudiesState {
    /// The printing registry as last read, with when it was read.
    pub(super) registry: Option<(Retry, Result<PrintRegistry, String>)>,
    registry_job: Option<(Retry, Job<PrintRegistry>)>,
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
        Some((_, Err(e))) => Err(format!("the printing registry could not be read: {e}")),
        None => Err(REGISTRY_READING.to_string()),
    }
}

/// The printing registry as the forms list it: printers (with their usable
/// box) and filaments in the file's order, its path, sha256 and revision.
pub(crate) fn read_registry() -> Result<PrintRegistry, String> {
    let path = sim_print::registry::default_path();
    let loaded = sim_print::registry::load(&path)?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    #[derive(serde::Deserialize)]
    struct Order {
        #[serde(default)]
        printers: Ordered<Value>,
        #[serde(default)]
        materials: Ordered<Value>,
    }
    let order: Order = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let r = &loaded.registry;
    let printers = order.printers.keys().filter_map(|k| r.printers.get(k).map(|p| (k.to_string(), PrinterInfo { name: p.name.clone(), usable_mm: p.usable_mm().to_vec() }))).collect();
    let materials = order.materials.keys().filter_map(|k| r.materials.get(k).map(|m| (k.to_string(), FilamentInfo { name: m.name.clone(), cad_material: Some(m.cad_material.clone()).filter(|c| !c.is_empty()) }))).collect();
    Ok(PrintRegistry { path: Some(path.display().to_string()), sha256: Some(loaded.sha256), revision: json!(r.revision), printers: Ordered(printers), materials: Ordered(materials) })
}

/// The print study of the shown archive: `robot_settings["print_study"]`
/// and the split groups in tree order.
pub(super) fn study(doc: &CadDocument) -> Result<PrintStudy, String> {
    let local = doc.local.as_ref().ok_or("no CAD document is open")?;
    let study = local.archive.manifest["robot_settings"]["print_study"].clone();
    let splits = local.tree.nodes.iter().filter(|n| local.archive.node(&n.id).is_some_and(|m| m["robot"]["print_split"].is_object())).map(|n| n.id.clone()).collect();
    Ok(PrintStudy { revision: doc.shown_revision(), study, splits })
}

/// The study as read at `revision` (the run's), or why not.
fn study_at(doc: &CadDocument, revision: u64) -> Result<PrintStudy, String> {
    let s = study(doc)?;
    if s.revision != revision {
        return Err(format!("the document is at revision {}, the selection was read at {revision}; try again", s.revision));
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

/// `print::send` for a study: the job started in process at the plan's
/// revision; its answer is the started job (`PrintJob`).
pub(super) fn send(doc: &mut CadDocument, _call: &mut Call, plan: StudyPlan) -> Outcome {
    let StudyPlan { kind, revision, request, label: _, message } = plan;
    let body = match request {
        Request::Split(req) => value(&req),
        Request::Start(body) => body,
    };
    Outcome::Done(super::jobs_tracker::start(doc, kind, body, revision, message).map(|job| value(&job)))
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
    let study = match study(doc) {
        Ok(st) => json!({"revision": st.revision, "has_study": st.has_study(), "parts": st.part_nodes(), "splits": st.splits}),
        Err(e) => json!({"error": e}),
    };
    json!({"registry": registry, "study": study})
}

/// The registry read: started once per generation (again after a failed
/// read and a Refresh), landed when done.
pub(super) fn tick(doc: &mut CadDocument) {
    let (generation, retry_count) = (doc.generation, doc.mesh_retry);
    let retry = (generation, retry_count);
    let mut touched = false;
    let s = &mut doc.print.studies;
    if s.registry_job.as_ref().is_some_and(|(k, _)| k.0 != generation) {
        s.registry_job = None;
    }
    let landed = s.registry_job.as_ref().and_then(|(k, job)| job.poll().map(|r| (*k, r)));
    if let Some((k, result)) = landed {
        s.registry_job = None;
        s.registry = Some((k, result));
        touched = true;
    }
    let want = s.registry_job.is_none()
        && match &s.registry {
            None => true,
            Some((k, Ok(_))) => k.0 != generation,
            Some((k, Err(_))) => *k != retry,
        };
    if want {
        let job = Job::spawn(Pool::Io, generation, "cad print registry", |_| read_registry());
        s.registry_job = Some((retry, job));
        touched = true;
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
