//! The print study's results in the stress overlay (RoboCAD's "Strength
//! overlay on/off", `print.overlay`, which toggles the same overlay as
//! `view.stress`: `results::ResultsOp::PrintOverlay`).
//!
//! A strength check or plan hangs a "print" block on each part's node
//! (print_jobs.py:228-237 and :283-287: `safety_factor`, `governing`,
//! `passes`, `fidelity`, `registry_sha256`, `cad_revision`, …), served by
//! `GET /results/nodes` (`robot::data`). [`inputs`] turns a block into
//! the overlay's inputs so the one shared rule
//! (`sim_domain_robot::stress_results::link_colours`, through
//! `results::overlay::cad_colours`) colours it: a single cell at the origin
//! whose stress is the governing failure index 1 / safety factor, with
//! yield 1, so the whole body takes one colour, red at failure (safety
//! factor 1) and blue at a failure index of 0.1 %.
//!
//! *Deliberately different:* RoboCAD's own window samples the study's
//! per-voxel failure-index field from `result_dir` at each mesh vertex
//! (ui/viewport.py:831-840, green ≤ 0.25 → red ≥ 1). That field is a file
//! RoboCAD wrote beside its run; porting the sampling is out of scope, so
//! this window shows each part's governing value uniformly, on the shared
//! scale.
//!
//! **Staleness**: RoboCAD records the revision it analysed
//! (`cad_revision`, the snapshot's) and publishes the result as one undo
//! step, which moves the revision by exactly one (`PublishState.apply` →
//! `Document.touch` → `notify('changed')`; `revision` is not one of the
//! restored state fields). So a result reads "current" while the shown
//! revision is at most `cad_revision + 1`, and "stale (computed at
//! revision N, now M)" after any later revision (an undo past it removes
//! the block itself). The revision is the document's, not the part's: a
//! later publish (another plan or strength check, a split, an exploded
//! view) or an edit then its undo moves it without touching that part, so
//! an earlier block can read "stale" while its part is unchanged. The
//! label is conservative, and the rule says so ([`RULE`]).
use crate::cad::document::CadDocument;
use crate::cad::results::Inputs;
use serde_json::{Value, json};
use sim_domain_robot::stress_results::Hotspot;
use sim_runtime::cad_client::NodeResult;

/// The section RoboCAD's print blocks carry.
pub(crate) const SECTION: &str = "print";
/// How this window colours a print result (the panel and state say so).
pub(crate) const RULE: &str = "print strength: each part in one colour, its governing failure index (1 / safety factor) on the stress scale, red at failure; RoboCAD's own window colours each voxel's failure index from its run folder. Stale means the document's revision moved on since the result was published, not that the part changed: another plan, strength check, split or exploded view, or an edit then undo, moves it too, so an earlier result can read stale while its part is unchanged";

/// The overlay's inputs for a node's print block: a single cell at the
/// origin with the failure index 1 / safety factor as its stress, yield 1,
/// centre of mass at the origin. None for another section, or a safety
/// factor that is missing, not finite or not positive.
pub(crate) fn inputs(node: &NodeResult) -> Option<Inputs> {
    let block = &node.results;
    if block["section"].as_str() != Some(SECTION) {
        return None;
    }
    let sf = block["safety_factor"].as_f64().filter(|v| v.is_finite() && *v > 0.0)?;
    Some(Inputs { hotspot: Hotspot { cells: vec![[0.0; 3]], stress_pa: vec![1.0 / sf] }, yield_pa: Some(1.0), com_m: Some([0.0; 3]) })
}

/// A print block's staleness against the shown revision (see the module doc).
pub(crate) fn staleness(block: &Value, shown: u64) -> String {
    match block["cad_revision"].as_u64() {
        None => "unknown (RoboCAD recorded no cad_revision)".to_string(),
        Some(at) if shown <= at + 1 => "current".to_string(),
        Some(at) => format!("stale (computed at revision {at}, now {shown})"),
    }
}

/// The nodes carrying a print block in the robot reads, in id order.
fn blocks(doc: &CadDocument) -> Vec<(&str, &NodeResult)> {
    doc.robot.data.results().map_or_else(Vec::new, |r| r.nodes.iter().filter(|(_, n)| n.results["section"].as_str() == Some(SECTION)).map(|(id, n)| (id.as_str(), n)).collect())
}

/// The results panel's print line, when any print block is present:
/// "Print strength: N part(s); least safety factor F on NAME (current)",
/// or "…; stale (computed at revision R, now M)" once [`staleness`] says
/// so (no nested parentheses: [`tag`]).
pub(crate) fn panel_line(doc: &CadDocument) -> Option<String> {
    let found = blocks(doc);
    if found.is_empty() {
        return None;
    }
    let shown = doc.shown_revision();
    let worst = found.iter().filter_map(|(id, n)| n.results["safety_factor"].as_f64().map(|sf| (sf, *id, n))).fold(None, |best: Option<(f64, &str, &&NodeResult)>, x| match best {
        Some(b) if b.0 <= x.0 => Some(b),
        _ => Some(x),
    });
    let n = found.len();
    Some(match worst {
        Some((sf, id, node)) => format!("Print strength: {n} part(s); least safety factor {sf:.2} on {}{}", doc.node_name(id), tag(&node.results, shown)),
        None => format!("Print strength: {n} part(s) without a safety factor{}", tag(&found[0].1.results, shown)),
    })
}

/// The panel line's end for a block's [`staleness`]: " (current)", else
/// "; stale (computed at revision R, now M)" or "; staleness unknown (…)".
pub(crate) fn tag(block: &Value, shown: u64) -> String {
    let s = staleness(block, shown);
    match s.as_str() {
        "current" => " (current)".to_string(),
        _ if block["cad_revision"].as_u64().is_none() => format!("; staleness {s}"),
        _ => format!("; {s}"),
    }
}

/// `cad_state.print.overlay`: each node with a print block, its safety
/// factor, whether it passes, fidelity, registry hash, staleness and
/// whether the overlay can colour it; the overlay's state and rule.
pub(super) fn state_json(doc: &CadDocument) -> Value {
    let shown = doc.shown_revision();
    let nodes: Vec<Value> = blocks(doc)
        .into_iter()
        .map(|(id, n)| {
            let b = &n.results;
            json!({
                "node": id,
                "name": doc.node_name(id),
                "safety_factor": b["safety_factor"],
                "failure_index": inputs(n).map(|i| i.hotspot.stress_pa[0]),
                "passes": b["passes"],
                "fidelity": b["fidelity"],
                "registry_sha256": b["registry_sha256"],
                "cad_revision": b["cad_revision"],
                "staleness": staleness(b, shown),
                "coloured": inputs(n).is_some(),
            })
        })
        .collect();
    json!({"on": doc.results.overlay, "rule": RULE, "nodes": nodes})
}
