//! "Wall thickness check…" (`print.wall_check`, Ctrl+W) and "Validate for
//! printing" (`print.validate`, Ctrl+Shift+V): RoboCAD's reads on a job,
//! RoboCAD's status text (ui/app.py:1113-1135). Nothing is written: the
//! thin points are drawn display only by `thin_overlay`.
//!
//! - **Wall check**: the selected nodes in selection order, else every
//!   visible body ([`visible_bodies`], RoboCAD's `doc.bodies(True)`); one
//!   `Pool::Dedicated` job reads `GET /nodes/{id}/thin?threshold=` per node
//!   (a 404, a node without geometry, is skipped as RoboCAD's `if b is
//!   None: continue`), checking for cancellation between nodes. Answers are
//!   cached per (document generation, node, shown revision, threshold), so
//!   checking an unchanged node again sends nothing; a check whose every
//!   node is cached lands at once without a job. A second check while one
//!   runs replaces it (dropping the job cancels it).
//! - **Validate**: every visible body regardless of selection (RoboCAD's
//!   `validate`), `GET /nodes/{id}/validate` per body on one job; the text
//!   is `validate_for_export`'s (printing.py:138-154). Its tessellation
//!   open-edge count is not served by RoboCAD's REST route (api.py:700-703
//!   answers only the kernel's report), so it is not part of this check:
//!   deliberately different from RoboCAD's desktop, and said so in the state.
//! - **Remembered threshold**: the last threshold a check ran with presets
//!   the next Wall thickness form (RoboCAD's dialog opens at 1.2 each time;
//!   ours does too until a check has run).
//! - **Landing** ([`receive`], JobResults after `sync::receive`): a result
//!   of an older document generation is dropped; errors name the node.
use super::{PrintArgs, PrintCall, PrintOp};
use crate::app::actions::Call;
use crate::app::{ViewerMode, ViewerSet};
use crate::cad::actions::CadAction;
use crate::cad::document::CadDocument;
use crate::cad::ops::{Env, OpEntry, Resolved};
use crate::jobs::{Job, Pool};
use bevy::prelude::*;
use serde_json::{Map, Value, json};
use sim_api::Outcome;
use sim_runtime::cad_client::{ThinRegion, Validation, ValidationIssue};
use std::collections::HashMap;

/// RoboCAD's dialog default (`getDouble(…, 1.2, 0.1, 20, 2)`), mm.
pub(super) const DEFAULT_THRESHOLD: f64 = 1.2;
/// RoboCAD's node kinds `Document.bodies` lists (document.py:430-431).
const BODY_KINDS: [&str; 2] = ["body", "sheet"];
/// Why the validation's text lacks RoboCAD's open-edge lines.
pub(super) const OPEN_EDGE_NOTE: &str = "open-edge check: RoboCAD's desktop only (its tessellation open-edge count is not served by GET /nodes/{id}/validate)";

/// One node's thin read: its regions, or None when it has no geometry (404).
pub(super) type NodeThin = Option<Vec<ThinRegion>>;
/// A cached thin read: (document generation, node, shown revision, threshold bits).
pub(super) type CacheKey = (u64, String, u64, u64);

/// The cache key of node `node`'s thin read.
pub(super) fn cache_key(generation: u64, node: &str, revision: u64, threshold: f64) -> CacheKey {
    (generation, node.to_string(), revision, threshold.to_bits())
}

/// The checks' state on the document.
#[derive(Default)]
pub(crate) struct ChecksState {
    /// The threshold the last wall check ran with (None: none yet).
    pub(super) threshold: Option<f64>,
    /// Thin reads by [`CacheKey`] (kept for one generation and revision).
    pub(super) cache: HashMap<CacheKey, NodeThin>,
    /// The last wall check, with its points.
    pub(super) wall: Option<WallResult>,
    /// The last validation.
    pub(super) validation: Option<ValidationResult>,
    /// The wall check running (its job stamped with the document generation).
    pub(super) wall_job: Option<(WallMeta, Job<Vec<(String, NodeThin)>>)>,
    /// The validation running: the (name, report) of each body with geometry.
    pub(super) validate_job: Option<(ValidateMeta, Job<Vec<(String, Validation)>>)>,
}

/// A check run's inputs (built from the entry, the selection and the values).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum CheckPlan {
    /// The wall check: these nodes, in order, at this threshold (mm).
    Wall { threshold: f64, nodes: Vec<String> },
    /// Validation: these visible bodies (id, name), in the tree's order.
    Validate { bodies: Vec<(String, String)> },
}

/// What a wall check asked, kept with its job.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct WallMeta {
    pub threshold: f64,
    pub revision: u64,
    pub generation: u64,
    pub nodes: Vec<String>,
    /// The nodes answered from the cache when it started.
    pub known: Vec<(String, NodeThin)>,
}

/// What a validation asked.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ValidateMeta {
    pub revision: u64,
    pub generation: u64,
    /// The visible bodies asked about.
    pub bodies: usize,
}

/// A wall check's outcome (RoboCAD's `temp_shapes` of red points).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct WallResult {
    pub threshold: f64,
    pub revision: u64,
    pub generation: u64,
    /// Per node, in order: its thin region count, or None without geometry.
    pub counts: Vec<(String, Option<usize>)>,
    /// The points, model mm.
    pub points: Vec<Vec3>,
}

/// A validation's outcome.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ValidationResult {
    pub revision: u64,
    pub generation: u64,
    pub ok: bool,
    /// The bodies validated (those with geometry).
    pub bodies: usize,
    /// `validate_for_export`'s messages (empty when ok).
    pub lines: Vec<String>,
}

/// Python's `str(float)` (`repr`): the shortest text that reads back, with
/// ".0" on whole numbers, and exponent form below 1e-4 or from 1e16.
pub(super) fn py_float(v: f64) -> String {
    if v.is_nan() {
        return "nan".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "inf".into() } else { "-inf".into() };
    }
    let a = v.abs();
    if a != 0.0 && !(1e-4..1e16).contains(&a) {
        // Rust's `{:e}` is the shortest mantissa ("1.5e-7"); Python pads the
        // exponent to two digits with a sign ("1.5e-07", "1e+16").
        let s = format!("{v:e}");
        let (mantissa, exponent) = s.split_once('e').unwrap_or((&s, "0"));
        let (sign, digits) = exponent.strip_prefix('-').map_or(("+", exponent), |d| ("-", d));
        return format!("{mantissa}e{sign}{digits:0>2}");
    }
    let s = format!("{v}");
    if s.contains('.') { s } else { format!("{s}.0") }
}

/// RoboCAD's wall check status (ui/app.py:1126).
pub(super) fn wall_status(marks: usize, threshold: f64) -> String {
    let t = py_float(threshold);
    if marks > 0 { format!("{marks} thin region(s) under {t} mm") } else { format!("No walls thinner than {t} mm") }
}

/// One issue as `validate_for_export` writes it (printing.py:147-148).
pub(super) fn issue_line(name: &str, issue: &ValidationIssue) -> String {
    let mut line = format!("{name}: {}", issue.message);
    if let Some([x, y, z]) = issue.location {
        line.push_str(&format!(" near ({x:.1}, {y:.1}, {z:.1})"));
    }
    if let Some(fix) = issue.fix.as_deref().filter(|f| !f.is_empty()) {
        line.push_str(&format!(" — {fix}"));
    }
    line
}

/// `validate_for_export` over the answered bodies (name, report): whether
/// all are valid and watertight, and the messages of the failing ones. A
/// failing body RoboCAD gave no issue for gets one line saying so (RoboCAD
/// would show an empty warning there).
pub(super) fn validation_lines(reports: &[(String, Validation)]) -> (bool, Vec<String>) {
    let mut ok = true;
    let mut lines = Vec::new();
    for (name, rep) in reports {
        if rep.valid && rep.watertight {
            continue;
        }
        ok = false;
        if rep.issues.is_empty() {
            let what = if rep.valid { "not watertight" } else { "not a valid solid" };
            lines.push(format!("{name}: {what}"));
        }
        lines.extend(rep.issues.iter().map(|i| issue_line(name, i)));
    }
    (ok, lines)
}

/// RoboCAD's validation text: the information box when ok, else the
/// messages (one status line, joined by "; ").
pub(super) fn validation_status(ok: bool, bodies: usize, lines: &[String]) -> Result<String, String> {
    if ok { Ok(format!("{bodies} body(ies): valid and watertight.")) } else { Err(lines.join("; ")) }
}

/// RoboCAD's `doc.bodies(True)`: the bodies and sheets visible through
/// every ancestor, in the tree's walk order.
pub(super) fn visible_bodies(doc: &CadDocument) -> Vec<(String, String)> {
    let Some(d) = &doc.doc else { return Vec::new() };
    d.nodes.iter().filter(|n| BODY_KINDS.contains(&n.kind.as_str()) && n.effective_visible).map(|n| (n.id.clone(), n.name.clone())).collect()
}

pub(super) fn build(call: PrintCall, entry: &OpEntry, r: &Resolved, values: &Map<String, Value>, doc: &CadDocument, _env: &Env) -> Result<CheckPlan, String> {
    match call {
        PrintCall::WallCheck => {
            let threshold = values.get("threshold").and_then(Value::as_f64).filter(|t| t.is_finite() && *t > 0.0).ok_or_else(|| format!("{}: the threshold must be a positive number of mm", entry.label))?;
            // No selection and no visible bodies: RoboCAD's loop runs over an
            // empty list and says "No walls thinner than {t} mm"
            // (ui/app.py:1117-1126); the empty plan lands at once with that.
            let nodes = if r.nodes.is_empty() { visible_bodies(doc).into_iter().map(|(id, _)| id).collect() } else { r.nodes.clone() };
            Ok(CheckPlan::Wall { threshold, nodes })
        }
        PrintCall::Validate => Ok(CheckPlan::Validate { bodies: visible_bodies(doc) }),
        other => Err(format!("{}: {other:?} is not a check", entry.id)),
    }
}

pub(super) fn send(doc: &mut CadDocument, _call: &mut Call, plan: CheckPlan) -> Outcome {
    Outcome::Done(match plan {
        CheckPlan::Wall { threshold, nodes } => start_wall(doc, threshold, nodes),
        CheckPlan::Validate { bodies } => start_validate(doc, bodies),
    })
}

/// The cached answers for `nodes` at (generation, revision, threshold), and the nodes to read.
pub(super) fn split_cached(state: &ChecksState, generation: u64, revision: u64, threshold: f64, nodes: &[String]) -> (Vec<(String, NodeThin)>, Vec<String>) {
    let mut known = Vec::new();
    let mut missing = Vec::new();
    for n in nodes {
        match state.cache.get(&cache_key(generation, n, revision, threshold)) {
            Some(thin) => known.push((n.clone(), thin.clone())),
            None if !missing.contains(n) => missing.push(n.clone()),
            None => {}
        }
    }
    (known, missing)
}

/// Start a wall check (or land it at once from the cache).
pub(super) fn start_wall(doc: &mut CadDocument, threshold: f64, nodes: Vec<String>) -> Result<Value, String> {
    const LABEL: &str = "Wall thickness check";
    let client = doc.client.clone().filter(|_| doc.connected()).ok_or_else(|| format!("{LABEL}: not connected to RoboCAD: {}", doc.connection_line().0))?;
    let (generation, revision) = (doc.generation, doc.shown_revision());
    let (known, missing) = split_cached(&doc.print.checks, generation, revision, threshold, &nodes);
    let answer = json!({"reading": "thin", "nodes": nodes, "threshold": threshold, "revision": revision, "cached": known.len(), "sent": missing.len()});
    let meta = WallMeta { threshold, revision, generation, nodes, known };
    let checks = &mut doc.print.checks;
    checks.threshold = Some(threshold);
    // A check running is replaced (dropping its job cancels it).
    checks.wall_job = None;
    if missing.is_empty() {
        land_wall(doc, meta, Ok(Vec::new()));
        return Ok(answer);
    }
    let job = Job::spawn(Pool::Dedicated, generation, "cad-print-thin", move |ctx| {
        let mut out = Vec::with_capacity(missing.len());
        for id in missing {
            if ctx.cancelled() {
                return Err("cancelled".into());
            }
            match client.thin_walls(&id, threshold) {
                Ok(regions) => out.push((id, Some(regions))),
                // RoboCAD's `resolved_body(i) is None: continue`.
                Err(e) if e.not_found() => out.push((id, None)),
                Err(e) => return Err(format!("{id}: {e}")),
            }
        }
        Ok(out)
    });
    checks.wall_job = Some((meta, job));
    doc.touch();
    Ok(answer)
}

/// Start a validation of `bodies` (id, name).
fn start_validate(doc: &mut CadDocument, bodies: Vec<(String, String)>) -> Result<Value, String> {
    const LABEL: &str = "Validate for printing";
    let client = doc.client.clone().filter(|_| doc.connected()).ok_or_else(|| format!("{LABEL}: not connected to RoboCAD: {}", doc.connection_line().0))?;
    let (generation, revision) = (doc.generation, doc.shown_revision());
    let answer = json!({"reading": "validate", "bodies": bodies.iter().map(|(id, _)| id).collect::<Vec<_>>(), "revision": revision});
    let meta = ValidateMeta { revision, generation, bodies: bodies.len() };
    let job = Job::spawn(Pool::Dedicated, generation, "cad-print-validate", move |ctx| {
        let mut out = Vec::with_capacity(bodies.len());
        for (id, name) in bodies {
            if ctx.cancelled() {
                return Err("cancelled".into());
            }
            match client.validate_node(&id) {
                Ok(rep) => out.push((name, rep)),
                // Not one of RoboCAD's `bodies` (no geometry): not validated.
                Err(e) if e.not_found() => {}
                Err(e) => return Err(format!("{name}: {e}")),
            }
        }
        Ok(out)
    });
    // A validation running is replaced (dropping its job cancels it).
    doc.print.checks.validate_job = Some((meta, job));
    doc.touch();
    Ok(answer)
}

/// A wall check's answers landed (`fetched`: the nodes read now, or the
/// job's error naming its node): cached, the points kept, RoboCAD's status.
pub(super) fn land_wall(doc: &mut CadDocument, meta: WallMeta, fetched: Result<Vec<(String, NodeThin)>, String>) {
    let fetched = match fetched {
        Ok(f) => f,
        Err(e) => {
            let e = name_node(doc, &e);
            return doc.show(Err(format!("Wall thickness check: {e}")));
        }
    };
    let current = meta.generation == doc.generation && meta.revision == doc.shown_revision();
    let shown_revision = doc.shown_revision();
    let checks = &mut doc.print.checks;
    // One generation and revision is kept: an older one cannot be asked
    // again. Only a result for the shown revision prunes: a late result for
    // an older one must not delete newer reads (its own entries are dropped
    // by the next current landing).
    if current {
        checks.cache.retain(|k, _| k.0 == meta.generation && k.2 == meta.revision);
    }
    for (id, thin) in &fetched {
        checks.cache.insert(cache_key(meta.generation, id, meta.revision, meta.threshold), thin.clone());
    }
    let answers: HashMap<&str, &NodeThin> = meta.known.iter().chain(fetched.iter()).map(|(id, t)| (id.as_str(), t)).collect();
    let mut counts = Vec::with_capacity(meta.nodes.len());
    let mut points = Vec::new();
    for id in &meta.nodes {
        let thin = answers.get(id.as_str()).copied().and_then(Option::as_ref);
        counts.push((id.clone(), thin.map(Vec::len)));
        points.extend(thin.into_iter().flatten().map(|r| Vec3::new(r.point[0] as f32, r.point[1] as f32, r.point[2] as f32)));
    }
    let mut status = wall_status(points.len(), meta.threshold);
    if !current && !points.is_empty() {
        // Not drawn (`drawn` needs the shown revision): not reported as if it were.
        status = format!("{status} (read at revision {}; the document is now at revision {shown_revision}: they are not drawn)", meta.revision);
    }
    checks.wall = Some(WallResult { threshold: meta.threshold, revision: meta.revision, generation: meta.generation, counts, points });
    doc.show(Ok(status));
}

/// An error "{id}: …" from the job with the node's name for its id.
fn name_node(doc: &CadDocument, e: &str) -> String {
    match e.split_once(": ") {
        Some((id, rest)) if doc.doc.as_ref().is_some_and(|d| d.nodes.iter().any(|n| n.id == id)) => format!("{}: {rest}", doc.node_name(id)),
        _ => e.to_string(),
    }
}

/// A validation's answers landed.
fn land_validation(doc: &mut CadDocument, meta: ValidateMeta, result: Result<Vec<(String, Validation)>, String>) {
    let reports = match result {
        Ok(r) => r,
        Err(e) => return doc.show(Err(format!("Validate for printing: {e}"))),
    };
    let (ok, lines) = validation_lines(&reports);
    let status = validation_status(ok, reports.len(), &lines);
    doc.print.checks.validation = Some(ValidationResult { revision: meta.revision, generation: meta.generation, ok, bodies: reports.len(), lines });
    doc.show(status);
}

/// The wall check's points to draw: only at the shown revision of this document.
pub(super) fn drawn(doc: &CadDocument) -> Option<&WallResult> {
    doc.print.checks.wall.as_ref().filter(|w| w.generation == doc.generation && w.revision == doc.shown_revision() && !w.points.is_empty())
}

/// A newly opened Wall thickness form: the last threshold a check ran with.
pub(super) fn seed(entry: &OpEntry, doc: &CadDocument, texts: &mut [String]) {
    let Some(t) = doc.print.checks.threshold else { return };
    if let Some(slot) = entry.params.iter().position(|p| p.name == "threshold").and_then(|i| texts.get_mut(i)) {
        *slot = py_float(t);
    }
}

/// `cad_print {op: clear}`: the wall check's points are cleared (RoboCAD's
/// `temp_shapes = []`); its counts stay in the state.
pub(super) fn clear(doc: &mut CadDocument) -> Value {
    let cleared = doc.print.checks.wall.as_mut().map_or(0, |w| std::mem::take(&mut w.points).len());
    doc.touch();
    json!({"cleared": cleared, "checks": state_json(doc)})
}

/// `cad_state.print.checks`.
pub(super) fn state_json(doc: &CadDocument) -> Value {
    let c = &doc.print.checks;
    let current = |generation: u64, revision: u64| generation == doc.generation && revision == doc.shown_revision();
    let wall = c.wall.as_ref().map(|w| {
        let stale = !current(w.generation, w.revision);
        let note = stale.then(|| format!("read at revision {}; the document is now at revision {}: the points are not drawn", w.revision, doc.shown_revision()));
        let marks: usize = w.counts.iter().filter_map(|(_, n)| *n).sum();
        json!({
            "threshold": w.threshold,
            "revision": w.revision,
            "nodes": w.counts.iter().map(|(id, n)| json!({"id": id, "name": doc.node_name(id), "count": n, "geometry": n.is_some()})).collect::<Vec<_>>(),
            "points": w.points.len(),
            "shown": !stale && !w.points.is_empty(),
            "stale": stale,
            "note": note,
            "status": wall_status(marks, w.threshold),
        })
    });
    let validation = c.validation.as_ref().map(|v| {
        json!({
            "revision": v.revision,
            "current": current(v.generation, v.revision),
            "ok": v.ok,
            "bodies": v.bodies,
            "lines": v.lines,
        })
    });
    json!({
        "threshold": c.threshold.unwrap_or(DEFAULT_THRESHOLD),
        "threshold_remembered": c.threshold.is_some(),
        "wall_check": wall,
        "wall_check_running": c.wall_job.as_ref().map(|(m, _)| json!({"threshold": m.threshold, "revision": m.revision, "nodes": m.nodes, "cached": m.known.len()})),
        "validation": validation,
        "validation_running": c.validate_job.as_ref().map(|(m, _)| json!({"revision": m.revision, "bodies": m.bodies})),
        "open_edge_check": OPEN_EDGE_NOTE,
        "cached_reads": c.cache.len(),
    })
}

/// `cad:print:clear_checks` while the wall check's points are drawn.
pub(super) fn controls(doc: &CadDocument) -> Vec<(String, String, CadAction, Result<(), String>)> {
    match drawn(doc) {
        Some(_) => vec![("cad:print:clear_checks".into(), "Clear wall check marks".into(), PrintArgs::of(PrintOp::Clear), Ok(()))],
        None => Vec::new(),
    }
}

/// CadCorePlugin: the checks' landing (windowless).
pub(super) fn build_core(app: &mut App) {
    app.add_systems(Update, receive.after(crate::cad::sync::receive).in_set(ViewerSet::JobResults).run_if(in_state(ViewerMode::Cad)));
}

/// JobResults: a check that came back. A result of an older document
/// generation is dropped; frames keep coming while a read is out.
pub(super) fn receive(doc: Option<ResMut<CadDocument>>, redraw: Option<MessageWriter<bevy::window::RequestRedraw>>) {
    let Some(mut doc) = doc else { return };
    // Read through `Deref` first: a `ResMut` deref would mark the document changed.
    let c = &doc.print.checks;
    let wall = c.wall_job.as_ref().and_then(|(_, job)| job.poll());
    let valid = c.validate_job.as_ref().and_then(|(_, job)| job.poll());
    let running = c.wall_job.is_some() || c.validate_job.is_some();
    if wall.is_none() && valid.is_none() {
        if running && let Some(mut redraw) = redraw {
            redraw.write(bevy::window::RequestRedraw);
        }
        return;
    }
    let doc = &mut *doc;
    if let Some(result) = wall
        && let Some((meta, job)) = doc.print.checks.wall_job.take()
    {
        doc.touch();
        if job.generation() == doc.generation {
            land_wall(doc, meta, result);
        }
    }
    if let Some(result) = valid
        && let Some((meta, job)) = doc.print.checks.validate_job.take()
    {
        doc.touch();
        if job.generation() == doc.generation {
            land_validation(doc, meta, result);
        }
    }
}
