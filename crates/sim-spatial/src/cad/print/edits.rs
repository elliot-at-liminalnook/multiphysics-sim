//! "Fastener hole…" and "Clearance offset…" (ui/app.py:889-908,
//! ui/widgets.py:988-1021, ui/tools.py:1120-1155, commands.py:903-933).
//!
//! - **Fastener hole** (`tool.fastener`, `Flow::PrintPick`): the Fastener
//!   hole dialog's values stay in the form beside the view while the tool
//!   is active; each face click (`fastener_tool`) is one run on the
//!   clicked face at the click's point: [`build`] makes RoboCAD's
//!   `FastenerSpec` (`depth` 0 is "through": `None`, as the dialog's
//!   `self.depth.value() or None`), [`send`] one `fastener_hole` call, its
//!   RoboCAD undo step the spec's label ("M3 clearance"). The dialog's
//!   values are remembered once a run starts (RoboCAD's `last_fastener`)
//!   and preset the next Fastener hole form ([`seed`]).
//! - **Clearance offset** (`tool.clearance`, `Flow::Form`): the selected
//!   faces grouped by node in selection order (RoboCAD's dict insertion
//!   order), one `clearance(node, faces, amount)` per node in one edit job,
//!   each its own RoboCAD undo step "Clearance"; the amount is remembered
//!   once the run starts, as RoboCAD's `ops.last_clearance` is (0.2 at first,
//!   commands.py:271), and presets the next form as its 2-decimal spin box
//!   shows it. The memory belongs to global viewer preferences: RoboCAD's own stays in the
//!   service and is not served, so the state says so ([`LAST_CLEARANCE_NOTE`]).
//!
//! Each commit goes through `actions::edit_at`: refused by name with
//! nothing sent while an edit is in flight, when not connected, when the
//! shown document is behind RoboCAD's or RoboCAD's revision moved since the
//! pick or the form. Nothing local is changed: the hole and the offset show
//! once RoboCAD's document is read back.
use super::PrintCall;
use crate::app::actions::Call;
use crate::cad::actions::edit_at;
use crate::cad::document::{CadDocument, EditDone};
use crate::cad::ops::{Env, OpEntry, Resolved};
use crate::cad::sync::value;
use serde_json::{Map, Value, json};
use sim_api::Outcome;
use crate::cad::types::{FastenerSpec, SelectionItem};

/// RoboCAD's first `ops.last_clearance` (commands.py:271).
#[cfg(test)]
pub(crate) const FIRST_CLEARANCE: f64 = 0.2;

/// Whose remembered clearance `last_clearance` is: RoboCAD keeps its own
/// `ops.last_clearance` in the service and does not serve it.
pub(crate) const LAST_CLEARANCE_NOTE: &str = "the viewer's persisted last accepted clearance (0.2 at first); RoboCAD's own last clearance stays in the service and is not read, so a run from RoboCAD's desktop does not change it";

/// The Fastener hole dialog's last values (RoboCAD's `last_fastener`,
/// ui/app.py:87 and :893: depth 0 is "through").
pub(crate) use crate::app::settings::FastenerDefaults as LastFastener;

/// A 3D click of the fastener tool, noted for the pick it writes.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FastenerClick {
    pub item: SelectionItem,
    pub picked_at: u64,
    /// The hit point (mm, RoboCAD's frame: `result["world"]`).
    pub point: [f64; 3],
    /// RoboCAD's snap there when it is a vertex, midpoint, centre or endpoint.
    pub snap: Option<[f64; 3]>,
}

/// The edits' state on the document (reset with it).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EditsState {
    /// The click the next pick is for (taken by the pick).
    pub click: Option<FastenerClick>,
    /// The last pick applied: the face and the revision it was read at.
    pub last_pick: Option<(SelectionItem, u64)>,
}
impl Default for EditsState {
    fn default() -> Self {
        EditsState { click: None, last_pick: None }
    }
}
impl EditsState {
    /// The fastener tool's picks start over (a tool started, cancelled or ended).
    pub(crate) fn reset_picks(&mut self) {
        self.click = None;
    }
}

/// An edit run's call.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum EditPlan {
    /// One `fastener_hole(node, face, point, spec)`.
    Fastener { node: String, face: i64, point: [f64; 3], spec: FastenerSpec, revision: u64 },
    /// One `clearance(node, faces, amount)` per node, in selection order.
    Clearance { groups: Vec<(String, Vec<i64>)>, amount: f64, revision: u64 },
}

/// `[x, y, z]` from a value as `ops::values` gives a point.
fn point3(v: &Value) -> Option<[f64; 3]> {
    let a = v.as_array()?;
    match a.as_slice() {
        [x, y, z] => Some([x.as_f64()?, y.as_f64()?, z.as_f64()?]),
        _ => None,
    }
}

/// `print::build_plan` for `PrintCall::Fastener` and `PrintCall::Clearance`.
pub(super) fn build(call: PrintCall, entry: &OpEntry, r: &Resolved, values: &Map<String, Value>, _doc: &CadDocument, _env: &Env) -> Result<EditPlan, String> {
    let number = |name: &str| values.get(name).and_then(Value::as_f64).ok_or_else(|| format!("{}: {name} is missing", entry.id));
    match call {
        PrintCall::Fastener => {
            // The clicked face (or a scripted run's), and the point a click gives.
            let Some((node, face)) = r.faces.first().cloned() else { return Err(entry.refusal.to_string()) };
            let Some(point) = values.get("point").and_then(point3) else { return Err(entry.refusal.to_string()) };
            let text = |name: &str| values.get(name).and_then(Value::as_str).map(str::to_string).ok_or_else(|| format!("{}: {name} is missing", entry.id));
            let depth = number("depth")?;
            // `FastenerSpec(size, kind, extra.value(), depth.value() or None)`.
            let spec = FastenerSpec { size: text("size")?, kind: text("kind")?, extra_clearance: number("extra")?, depth: (depth != 0.0).then_some(depth) };
            Ok(EditPlan::Fastener { node, face, point, spec, revision: r.revision })
        }
        PrintCall::Clearance => {
            if r.faces.is_empty() {
                return Err(entry.refusal.to_string());
            }
            // `by.setdefault(nid, []).append(fi)`: nodes in first-seen order.
            let mut groups: Vec<(String, Vec<i64>)> = Vec::new();
            for (node, face) in &r.faces {
                match groups.iter_mut().find(|(n, _)| n == node) {
                    Some((_, faces)) => faces.push(*face),
                    None => groups.push((node.clone(), vec![*face])),
                }
            }
            Ok(EditPlan::Clearance { groups, amount: number("amount")?, revision: r.revision })
        }
        _ => Err(format!("{} is not a print edit", entry.id)),
    }
}

/// `print::send` for an edit: one edit job (see the module doc).
pub(super) fn send(doc: &mut CadDocument, call: &mut Call, plan: EditPlan, settings: &mut crate::app::settings::SettingsOwner) -> Outcome {
    match plan {
        EditPlan::Fastener { node, face, point, spec, revision } => {
            let label = spec.label();
            let message = format!("{label} hole in {}", doc.node_name(&node));
            let remembered = LastFastener { size: spec.size.clone(), kind: spec.kind.clone(), extra: spec.extra_clearance, depth: spec.depth.unwrap_or(0.0) };
            let mut defaults = settings.cad.clone();
            defaults.fastener = remembered;
            if let Err(e) = defaults.validate() { return Outcome::Done(Err(e)); }
            let outcome = edit_at(doc, call, Some(revision), label, move |c| c.fastener_hole(&node, face, point, &spec).map(|r| EditDone { message, result: value(&r) }));
            if !matches!(outcome, Outcome::Done(Err(_))) {
                let _ = settings.set_cad(defaults);
                settings.claim_cad("fastener");
            }
            outcome
        }
        EditPlan::Clearance { groups, amount, revision } => {
            let mut defaults = settings.cad.clone();
            defaults.clearance = amount;
            if let Err(e) = defaults.validate() { return Outcome::Done(Err(e)); }
            let names: Vec<String> = groups.iter().map(|(n, _)| doc.node_name(n)).collect();
            let n = groups.len();
            let faces: usize = groups.iter().map(|(_, f)| f.len()).sum();
            let mut message = format!("Clearance {} mm on {faces} face(s) of {}", crate::cad::ops::g(amount), names.join(", "));
            if n > 1 {
                message.push_str(&format!(" ({n} calls, each its own RoboCAD undo step)"));
            }
            let outcome = edit_at(doc, call, Some(revision), "Clearance".to_string(), move |c| {
                let mut results = Vec::with_capacity(n);
                for (i, (node, faces)) in groups.iter().enumerate() {
                    match c.clearance(node, faces, amount) {
                        Ok(r) => results.push(value(&r)),
                        Err(mut e) => {
                            if n > 1 {
                                e.message = format!("{} (call {} of {n}: Clearance on {}; the {i} before it ran, each its own RoboCAD undo step)", e.message, i + 1, names[i]);
                            }
                            return Err(e);
                        }
                    }
                }
                let result = if n == 1 { results.pop().unwrap_or(Value::Null) } else { Value::Array(results) };
                Ok(EditDone { message, result })
            });
            if !matches!(outcome, Outcome::Done(Err(_))) {
                let _ = settings.set_cad(defaults);
                settings.claim_cad("clearance");
            }
            outcome
        }
    }
}

/// A spin box's value: rounded to its decimals (`ops::robot_form`'s rule).
fn fixed(v: f64, decimals: i32) -> String {
    let k = 10f64.powi(decimals);
    crate::cad::transform::num((v * k).round() / k)
}

/// Parameter `name`'s draft in `texts` (`OpEntry::params` order).
fn put(entry: &OpEntry, texts: &mut [String], name: &str, text: String) {
    if let Some(slot) = entry.params.iter().position(|p| p.name == name).and_then(|i| texts.get_mut(i)) {
        *slot = text;
    }
}

/// A newly opened Fastener hole or Clearance form's presets: the
/// remembered values (the point stays empty: a click gives it).
pub(super) fn seed(entry: &OpEntry, _doc: &CadDocument, env: &Env, texts: &mut [String]) {
    let initial = crate::app::settings::CadDefaults::default();
    let e = env.defaults.unwrap_or(&initial);
    match entry.id {
        "tool.fastener" => {
            let f = &e.fastener;
            put(entry, texts, "size", f.size.clone());
            put(entry, texts, "kind", f.kind.clone());
            put(entry, texts, "extra", fixed(f.extra, 2));
            put(entry, texts, "depth", fixed(f.depth, 2));
        }
        "tool.clearance" => put(entry, texts, "amount", fixed(e.clearance, 2)),
        _ => {}
    }
}

/// `cad_state.print.edits`.
pub(super) fn state_json(doc: &CadDocument, defaults: Option<&crate::app::settings::CadDefaults>) -> Value {
    let e = &doc.print.edits;
    let initial = crate::app::settings::CadDefaults::default();
    let defaults = defaults.unwrap_or(&initial);
    let f = &defaults.fastener;
    json!({
        "fastener_active": super::fastener_tool::active_tool(doc).is_some(),
        "last_fastener": {"size": f.size, "kind": f.kind, "extra": f.extra, "depth": f.depth},
        "last_clearance": defaults.clearance,
        "last_clearance_note": LAST_CLEARANCE_NOTE,
        "last_pick": e.last_pick.as_ref().map(|(item, revision)| json!({"item": item, "revision": revision})),
        "click_pending": e.click.as_ref().map(|c| json!({"item": c.item, "picked_at": c.picked_at})),
    })
}
