//! RoboCAD's "Calculate exact measurements" (ui/widgets.py:456-462,
//! 566-633): the selection's size, volume, area, mass and centroid.
//!
//! RoboCAD serializes each selected body, measures them in a separate
//! process (`measure_worker` over `analysis.selection_properties`) with a
//! 60 s limit, and drops the run (and its result) when the document's
//! revision or the selected nodes change. Here one Dedicated job reads
//! `GET /nodes/{id}` per selected node (its `mass` block is RoboCAD's
//! `mass_properties` of the resolved body, the same values) and combines
//! them as `selection_properties` does ([`combine`]): the union of the
//! bounding boxes, summed volume, area and mass, the centroid weighted by
//! each body's mass (at least 1e-9 g); nodes without a body are skipped.
//! The 60 s deadline is checked between requests and each request's
//! timeout is clamped to what remains. The run is stamped ([`Stamp`]) with
//! the document generation, the shown revision, the edit sequence and the
//! selected nodes; [`cancel_reason`] (the rule [`sync`] applies each frame)
//! drops it, cancelling the job, on any edit (one sent from here, or a
//! newer revision shown) and on any selection change, and the status line
//! says why. A value RoboCAD sent as null is an error naming it, never a
//! number.
use crate::cad::document::CadDocument;
use crate::cad::selection::{CadItems, CadSelection};
use crate::jobs::{Ctx, Job, Pool};
use bevy::prelude::*;
use serde_json::{Value, json};
use sim_runtime::cad_client::{CadClient, MassBlock, SelectionItem};
use std::time::{Duration, Instant};

/// RoboCAD's limit (`timer.start(60000)`).
pub(crate) const LIMIT: Duration = Duration::from_secs(60);
/// The node kinds RoboCAD's button measures (`refresh`: body, sheet, instance).
const MEASURABLE: [&str; 3] = ["body", "sheet", "instance"];

/// What a run answers for: RoboCAD's `(id(doc), doc.revision, ids)` and
/// the edit sequence (an edit sent from here moves it before RoboCAD's
/// revision does).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Stamp {
    pub generation: u64,
    pub revision: u64,
    pub edit_seq: u64,
    /// The selected nodes in the shown tree, in selection order.
    pub nodes: Vec<String>,
}

impl Stamp {
    pub(crate) fn of(doc: &CadDocument, selection: &[SelectionItem]) -> Stamp {
        let nodes = selection.nodes().into_iter().filter(|id| doc.has_node(id)).collect();
        Stamp { generation: doc.generation, revision: doc.shown_revision(), edit_seq: doc.edit_seq, nodes }
    }
}

/// The combined measurement (mm, mm³, mm², g).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Measured {
    pub items: usize,
    pub size: [f64; 3],
    pub volume: f64,
    pub area: f64,
    pub mass_g: f64,
    pub centroid: [f64; 3],
}

impl Measured {
    /// RoboCAD's text (widgets.py:610-612).
    pub(crate) fn text(&self) -> String {
        let (s, c) = (self.size, self.centroid);
        format!(
            "{} measured item(s)\nsize {:.3} × {:.3} × {:.3} mm\nvolume {:.3} cm³\narea {:.2} cm²\nmass {:.2} g\ncentroid ({:.2}, {:.2}, {:.2})",
            self.items,
            s[0],
            s[1],
            s[2],
            self.volume / 1000.0,
            self.area / 100.0,
            self.mass_g,
            c[0],
            c[1],
            c[2]
        )
    }
}

/// A run in flight: what it answers for and its job (dropping it cancels it).
pub(crate) struct ExactRun {
    pub stamp: Stamp,
    job: Job<Measured>,
}

impl ExactRun {
    pub(crate) fn new(stamp: Stamp, job: Job<Measured>) -> Self {
        ExactRun { stamp, job }
    }
}

/// The run, the last result and why the last run ended without one.
#[derive(Default)]
pub(crate) struct ExactState {
    pub(crate) run: Option<ExactRun>,
    /// The last result and what it answers for (dropped when that changes, as RoboCAD's).
    pub(crate) result: Option<(Stamp, Result<Measured, String>)>,
    /// Why the last run was cancelled, for the state it was cancelled in.
    pub(crate) status: Option<(Stamp, String)>,
}

impl ExactState {
    pub(crate) fn running(&self) -> bool {
        self.run.is_some()
    }
    /// What the facts line shows for `now`: the result's text, the error, the
    /// run, or why the last run stopped (None: RoboCAD's preview).
    pub(crate) fn facts(&self, now: &Stamp) -> Option<String> {
        if self.run.as_ref().is_some_and(|r| r.stamp == *now) {
            return Some("Calculating exact measurements… You can keep working.".into());
        }
        match &self.result {
            Some((s, Ok(m))) if s == now => return Some(m.text()),
            Some((s, Err(e))) if s == now => return Some(format!("Measurements unavailable: {e}")),
            _ => {}
        }
        self.status.as_ref().filter(|(s, _)| s == now).map(|(_, why)| why.clone())
    }
}

/// Why a run stamped `then` no longer answers for `now` (None: it does).
pub(crate) fn cancel_reason(then: &Stamp, now: &Stamp) -> Option<&'static str> {
    if then.generation != now.generation {
        Some("the CAD document was replaced or reconnected")
    } else if then.edit_seq != now.edit_seq || then.revision != now.revision {
        Some("the document was edited")
    } else if then.nodes != now.nodes {
        Some("the selection changed")
    } else {
        None
    }
}

/// One frame of the rule (the pure step [`sync`] runs): a run that no
/// longer answers for `now` is dropped (its job cancelled) with the reason
/// as the status; a result for another state is dropped; a finished run's
/// result lands. Returns whether anything shown changed.
pub(crate) fn settle(state: &mut ExactState, now: &Stamp) -> bool {
    let mut changed = false;
    if let Some(why) = state.run.as_ref().and_then(|r| cancel_reason(&r.stamp, now)) {
        state.run = None;
        state.status = Some((now.clone(), format!("Exact measurements cancelled: {why}.")));
        changed = true;
    }
    if state.result.as_ref().is_some_and(|(s, _)| s != now) {
        state.result = None;
        changed = true;
    }
    if state.status.as_ref().is_some_and(|(s, _)| s != now) {
        state.status = None;
        changed = true;
    }
    let landed = state.run.as_ref().and_then(|r| r.job.poll());
    if let Some(result) = landed
        && let Some(run) = state.run.take()
    {
        state.result = Some((run.stamp, result));
        changed = true;
    }
    changed
}

/// Whether the button is enabled now (RoboCAD's: no run, and some selected
/// node is a body, sheet or instance; here also connected).
pub(crate) fn ready(doc: &CadDocument, selection: &[SelectionItem]) -> Result<(), String> {
    let nodes = Stamp::of(doc, selection).nodes;
    if nodes.is_empty() {
        return Err("Nothing selected.".into());
    }
    if doc.physical_edit.exact.running() {
        return Err("exact measurements are being calculated".into());
    }
    if !nodes.iter().any(|id| super::node(doc, id).is_some_and(|n| MEASURABLE.contains(&n.kind.as_str()))) {
        return Err("nothing measurable is selected (bodies, sheets and instances are)".into());
    }
    if !doc.connected() {
        return Err(format!("not connected to RoboCAD: {}", doc.connection_line().0));
    }
    Ok(())
}

/// Start a run over the selected nodes (a run in flight is replaced, as
/// RoboCAD's `_measure_selection` cancels the previous one).
pub(crate) fn start(doc: &mut CadDocument, selection: &[SelectionItem]) -> Result<Value, String> {
    let stamp = Stamp::of(doc, selection);
    if doc.physical_edit.exact.running() {
        doc.physical_edit.exact.run = None;
    }
    ready(doc, selection)?;
    let client = doc.client.clone().ok_or("not connected to RoboCAD")?;
    let ids = stamp.nodes.clone();
    let job = Job::spawn(Pool::Dedicated, doc.generation, "cad-exact-measurement", move |ctx| measure(&client, &ids, ctx));
    let nodes = stamp.nodes.clone();
    doc.physical_edit.exact = ExactState { run: Some(ExactRun::new(stamp, job)), result: None, status: None };
    doc.touch();
    Ok(json!({"message": "Calculating exact measurements… You can keep working.", "nodes": nodes, "limit_s": LIMIT.as_secs()}))
}

/// Stop the run in flight.
pub(crate) fn cancel(doc: &mut CadDocument) -> Result<Value, String> {
    let Some(run) = doc.physical_edit.exact.run.take() else { return Err("no exact measurement is running".into()) };
    doc.physical_edit.exact.status = Some((run.stamp, "Exact measurements cancelled.".into()));
    doc.touch();
    Ok(json!({"message": "Exact measurements cancelled."}))
}

/// The job: one `GET /nodes/{id}` per node within RoboCAD's limit.
fn measure(client: &CadClient, ids: &[String], ctx: &Ctx) -> Result<Measured, String> {
    let deadline = Instant::now() + LIMIT;
    let expired = || format!("calculation stopped at RoboCAD's {} s limit; try a smaller selection", LIMIT.as_secs());
    let mut blocks = Vec::new();
    for id in ids {
        if ctx.cancelled() {
            return Err("cancelled".into());
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(expired());
        }
        let detail = client.clone().with_timeout(left).node(id).map_err(|e| if Instant::now() >= deadline { expired() } else { e.to_string() })?;
        blocks.push((detail.summary.name, detail.mass));
    }
    combine(&blocks)
}

/// `analysis.selection_properties` over the nodes' mass blocks (nodes
/// without a body have none and are skipped, as RoboCAD's measure skips
/// nodes without a resolved body).
pub(crate) fn combine(blocks: &[(String, Option<MassBlock>)]) -> Result<Measured, String> {
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    let (mut volume, mut area, mut mass) = (0.0, 0.0, 0.0);
    let mut cx = [0.0; 3];
    let mut items = 0;
    for (name, block) in blocks {
        let Some(m) = block else { continue };
        let one = |v: Option<f64>, what: &str| v.ok_or_else(|| format!("RoboCAD sent null for {name}'s {what}"));
        let three = |v: &[Option<f64>], what: &str| match v {
            [Some(x), Some(y), Some(z)] => Ok([*x, *y, *z]),
            _ => Err(format!("RoboCAD sent no readable {what} for {name}")),
        };
        let (bmin, bmax) = (three(m.bbox_min.as_slice(), "bounding box minimum")?, three(m.bbox_max.as_slice(), "bounding box maximum")?);
        let g = one(m.mass_g, "mass")?;
        let c = three(m.centroid.as_slice(), "centroid")?;
        volume += one(m.volume_mm3, "volume")?;
        area += one(m.area_mm2, "area")?;
        mass += g;
        for j in 0..3 {
            lo[j] = lo[j].min(bmin[j]);
            hi[j] = hi[j].max(bmax[j]);
            cx[j] += c[j] * g.max(1e-9);
        }
        items += 1;
    }
    if items == 0 {
        return Err("No measurable geometry selected".into());
    }
    let total = mass.max(1e-9);
    Ok(Measured { items, size: [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]], volume, area, mass_g: mass, centroid: [cx[0] / total, cx[1] / total, cx[2] / total] })
}

/// `cad_state.inspector_physical.exact`.
pub(crate) fn state_json(state: &ExactState) -> Value {
    json!({
        "running": state.run.as_ref().map(|r| json!({"nodes": r.stamp.nodes, "revision": r.stamp.revision})),
        "result": state.result.as_ref().map(|(s, r)| match r {
            Ok(m) => json!({"nodes": s.nodes, "revision": s.revision, "text": m.text(), "items": m.items, "size_mm": m.size, "volume_mm3": m.volume, "area_mm2": m.area, "mass_g": m.mass_g, "centroid_mm": m.centroid}),
            Err(e) => json!({"nodes": s.nodes, "revision": s.revision, "error": e}),
        }),
        "status": state.status.as_ref().map(|(_, s)| s),
    })
}

/// JobResults: [`settle`] against the document and the shared selection now.
pub(crate) fn sync(doc: Option<ResMut<CadDocument>>, selection: CadSelection) {
    let Some(mut doc) = doc else { return };
    let e = &doc.physical_edit.exact;
    if e.run.is_none() && e.result.is_none() && e.status.is_none() {
        return;
    }
    let now = Stamp::of(&doc, &selection.items());
    if settle(&mut doc.physical_edit.exact, &now) {
        doc.touch();
    }
}
