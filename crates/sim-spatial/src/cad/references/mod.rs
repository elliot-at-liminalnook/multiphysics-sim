//! CAD mode's References dock (cad-organize; native-viewer.md "CAD
//! organize"): RoboCAD's reference-image workspace and its linked system
//! file (cad/robocad/ui/references.py, references.py, system_link.py,
//! commands.py:1112-1136) over the typed client calls
//! (`sim_runtime::cad_client::{references, system_link}`). RoboCAD stays the
//! kernel: it reads and embeds the image files, keeps their placement and
//! the link, and owns undo. Every edit is one RoboCAD call through
//! `actions::edit_at`.
//!
//! - [`dock`]: the dock (references.py:12-77): the system status line, Link
//!   system file…, Accept changes, Open in builder, Unlink, the intro, "＋ Add
//!   reference images…", the image list with visibility, the current image's
//!   placement form ([`form`]), Apply placement, Align view, Calibrate scale,
//!   Sketch over this, Remove reference and the perspective note. Drawn only
//!   while open (`view.references` opens it).
//! - [`input`]: the dock's kit text fields: the placement rows, the path
//!   field (`ui_kit::path_field`) of Add and Link, and the calibrate tool's
//!   distance.
//! - [`edits`]: the edits (add, visible, Apply placement, calibrate, link,
//!   accept, unlink, remove), each one RoboCAD call.
//! - [`reads`]: the windowless reads on jobs: every image node's placement
//!   (`GET /nodes/{id}`) per (generation, shown revision), the system status
//!   while the dock is open; the align after an import and Open in builder's
//!   window action.
//! - [`align`]: Align view (references.py:195-209) and Sketch over this
//!   (:216-219).
//! - [`calibrate`]: Calibrate scale's tool (ui/tools.py:1210-1241): two
//!   clicks on the image's plane, then the real distance.
//! - [`planes`]: the images textured on their planes (ui/viewport.py:659-691),
//!   display only.
//! - [`drop`]: files dropped on the window add references (ui/app.py:243-245,
//!   1801-1803; references.py:224-230).
//! - [`system_link`]: the status line (references.py:90-103) and Open in
//!   builder (:119-130) as an in-window switch to Build mode.
mod align;
pub(in crate::cad) mod calibrate;
pub(in crate::cad) mod dock;
mod drop;
mod edits;
mod form;
mod input;
mod planes;
mod reads;
mod system_link;
#[cfg(test)]
mod tests;

use super::actions::{CadAction, Cx};
use super::document::CadDocument;
use crate::app::actions::{Call, Spec};
use crate::jobs::Latest;
use crate::ui_kit::path_field::Listing;
use crate::ui_kit::text::TextDraft;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sim_api::Outcome;
use sim_runtime::cad_client::NodeSummary;
use std::collections::HashMap;
use std::path::PathBuf;

/// What `cad_references` does.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReferencesOp {
    /// The references' state (as `cad_state.references` shows it).
    #[default]
    State,
    /// The dock shown (`open: true`), hidden (`false`) or toggled.
    Dock,
    /// The path field of "＋ Add reference images…" (`kind: images`) or
    /// "Link system file…" (`kind: system`) opened (`open`, default true) or closed.
    Browse,
    /// `import_references(paths, active plane or XY)`: one undo step.
    Add,
    /// The list's current image (`id`).
    Select,
    /// `update_reference(id, visible)` (the list's checkbox).
    Visible,
    /// The placement form's plane choice (`plane`) or lock (`locked`); display state.
    Form,
    /// Apply placement: one `update_reference(id, width, opacity, origin,
    /// plane, rotation_deg, locked)`.
    Placement,
    /// Align view on image `id` (display only; sets the active plane).
    Align,
    /// Calibrate scale: align, then start the two-click tool on image `id`.
    Calibrate,
    /// The calibrate tool's click: `point` (mm, on the image's plane) at `picked_at`.
    CalibratePick,
    /// The calibrate tool's real distance (mm): one `calibrate_reference`.
    CalibrateDistance,
    /// End the calibrate tool (Escape).
    Cancel,
    /// Sketch over this: align, then the Line sketch tool on the image's plane.
    Sketch,
    /// Remove reference: RoboCAD's delete of node `id`.
    Remove,
    /// `link_system(path)`.
    Link,
    /// Accept changes: `refresh_system_link()`.
    Accept,
    /// `unlink_system()`.
    Unlink,
    /// Open in builder: switch this window to Build mode on the linked file.
    OpenBuilder,
}

/// RoboCAD's plane choices (references.py:48, 189).
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PlaneChoice {
    /// 'Keep current plane': no plane is sent.
    #[default]
    Keep,
    /// 'Front (XZ)'.
    Front,
    /// 'Side (YZ)'.
    Side,
    /// 'Top (XY)'.
    Top,
    /// 'Active construction plane' (the active plane, else XY).
    Active,
}
impl PlaneChoice {
    pub const ALL: [PlaneChoice; 5] = [PlaneChoice::Keep, PlaneChoice::Front, PlaneChoice::Side, PlaneChoice::Top, PlaneChoice::Active];
    /// RoboCAD's combo box text.
    pub fn label(self) -> &'static str {
        match self {
            PlaneChoice::Keep => "Keep current plane",
            PlaneChoice::Front => "Front (XZ)",
            PlaneChoice::Side => "Side (YZ)",
            PlaneChoice::Top => "Top (XY)",
            PlaneChoice::Active => "Active construction plane",
        }
    }
    /// The REST name (and the control id's suffix).
    pub fn name(self) -> &'static str {
        match self {
            PlaneChoice::Keep => "keep",
            PlaneChoice::Front => "front",
            PlaneChoice::Side => "side",
            PlaneChoice::Top => "top",
            PlaneChoice::Active => "active",
        }
    }
}

/// What the dock's path field is for.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum BrowseKind {
    /// "＋ Add reference images…" (RoboCAD's filter: png, jpg, jpeg, webp, bmp).
    #[default]
    Images,
    /// "Link system file…" (System files (*.system.json), JSON (*.json)).
    System,
}

/// `cad_references`' arguments (each op names the ones it takes; others are refused).
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct ReferencesArgs {
    #[serde(default)]
    pub op: ReferencesOp,
    /// Shown or hidden (dock, browse).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<bool>,
    /// What the path field is for (browse).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<BrowseKind>,
    /// Image files, absolute (add; RoboCAD reads them).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paths: Option<Vec<String>>,
    /// A system file, absolute (link).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// An image node (select, visible, placement, align, calibrate, sketch,
    /// remove; absent: the list's current image).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    /// The plane choice (form, placement).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plane: Option<PlaneChoice>,
    /// mm, 0.001–1e7 (placement).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    /// mm, each ±1e7 (placement).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<[f64; 3]>,
    /// Degrees, ±360 (placement).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation_deg: Option<f64>,
    /// Percent, 0–100 (placement; RoboCAD stores 0..1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity_pct: Option<f64>,
    /// 'Lock reference against selection' (form, placement).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locked: Option<bool>,
    /// RoboCAD's revision the values or the list were read at (visible, placement).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,
    /// mm, on the image's plane (calibrate_pick).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point: Option<[f64; 3]>,
    /// The shown revision the point was picked at (calibrate_pick).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picked_at: Option<u64>,
    /// mm, positive (calibrate_distance).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance: Option<f64>,
}
impl ReferencesArgs {
    /// `op` with nothing else.
    pub(crate) fn of(op: ReferencesOp) -> Self {
        ReferencesArgs { op, ..Default::default() }
    }
    /// `op` on image `id`.
    pub(crate) fn on(op: ReferencesOp, id: &str) -> Self {
        ReferencesArgs { op, id: Some(id.to_string()), ..Default::default() }
    }
    pub(crate) fn action(self) -> CadAction {
        CadAction::CadReferences(self)
    }
}

/// A dock field that can have the keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    /// A placement row (`form::ROWS`).
    Row(usize),
    /// The path field (Add, Link).
    Path,
    /// The calibrate tool's distance.
    Distance,
}

/// The open path field.
#[derive(Clone, Debug, PartialEq)]
pub struct Browse {
    pub kind: BrowseKind,
    pub draft: TextDraft,
    /// Why the last submit sent nothing, or RoboCAD's refusal.
    pub error: Option<String>,
    /// The listing last asked for (`path_field::listing_key`).
    pub listing_asked: Option<String>,
}

/// An edit this part started, by its sequence (`edit_answered`).
#[derive(Clone, Debug, PartialEq)]
pub enum Pending {
    /// `import_references` of these paths (refused: the path field reopens with them).
    Import(Vec<String>),
    Placement,
    Calibrate,
    /// `link_system` of this path (refused: the path field reopens with it).
    Link(String),
    /// Accept changes or Unlink.
    Status,
}

/// cad-organize's references state on the document (reset with it).
#[derive(Default)]
pub struct ReferencesState {
    /// The dock is shown.
    pub(crate) open: bool,
    /// The list's current image (RoboCAD's `current_id`).
    pub(crate) current: Option<String>,
    /// The current image's placement form, as loaded and typed.
    pub(crate) form: Option<form::PlacementForm>,
    /// The dock field with the keyboard (mirrored from the kit's focus), and
    /// whether its text is selected.
    pub(crate) focus: Option<Focus>,
    pub(crate) select_all: bool,
    /// A field the handler opened takes the keyboard next frame.
    pub(crate) claim: Option<Focus>,
    /// The open path field.
    pub(crate) browse: Option<Browse>,
    pub(crate) listing: Latest<Listing>,
    pub(crate) listed: Option<Listing>,
    /// Placements and the system status as read (`reads`).
    pub(crate) reads: reads::Reads,
    /// The calibrate tool, while active.
    pub(crate) calibrate: Option<calibrate::Calibrate>,
    /// The edit this part waits for.
    pub(crate) pending: Option<(u64, Pending)>,
    /// Align on this image once its placement is read (after an import),
    /// with the shown revision when the import answered (a newer tree
    /// without it means it is gone: the align is dropped).
    pub(crate) align_after: Option<(String, u64)>,
    /// Open in builder: the switch to write as a window action.
    pub(crate) switch_to: Option<PathBuf>,
    /// Each image's pixels: read and decoded in the window (`planes`).
    pub(crate) pixels: HashMap<String, planes::Pixels>,
}

/// The image nodes of the shown tree, in tree order.
pub(crate) fn images(doc: &CadDocument) -> Vec<&NodeSummary> {
    doc.doc.as_ref().map(|d| d.nodes.iter().filter(|n| n.kind == "image").collect()).unwrap_or_default()
}

/// Image node `id` of the shown tree.
pub(crate) fn image<'a>(doc: &'a CadDocument, id: &str) -> Option<&'a NodeSummary> {
    images(doc).into_iter().find(|n| n.id == id)
}

/// The image an op acts on: `id`, else the list's current one; refused by name.
pub(crate) fn target(doc: &CadDocument, id: Option<&String>) -> Result<String, String> {
    let id = id.cloned().or_else(|| doc.references.current.clone()).ok_or("Select a reference image")?;
    if image(doc, &id).is_none() {
        return Err(if doc.has_node(&id) { format!("{} is not a reference image", doc.node_name(&id)) } else { format!("no node {id} in the shown tree") });
    }
    Ok(id)
}

/// The args each op takes besides `op` (others are refused by name).
fn allowed(op: ReferencesOp) -> &'static [&'static str] {
    use ReferencesOp as O;
    match op {
        O::State | O::Accept | O::Unlink | O::OpenBuilder | O::Cancel => &[],
        O::Dock => &["open"],
        O::Browse => &["open", "kind"],
        O::Add => &["paths"],
        O::Select | O::Align | O::Calibrate | O::Sketch | O::Remove => &["id"],
        O::Visible => &["id", "visible", "revision"],
        O::Form => &["plane", "locked"],
        O::Placement => &["id", "plane", "width", "origin", "rotation_deg", "opacity_pct", "locked", "revision"],
        O::CalibratePick => &["point", "picked_at"],
        O::CalibrateDistance => &["distance"],
        O::Link => &["path"],
    }
}

/// The op's name as REST writes it.
fn op_name(op: ReferencesOp) -> String {
    serde_json::to_value(op).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

/// `CadReferences`, from any entry point.
pub(in crate::cad) fn handle(action: &CadAction, call: &mut Call, cx: &mut Cx) -> Outcome {
    let CadAction::CadReferences(args) = action else { return Outcome::Done(Err("not a references action".into())) };
    let given: Vec<String> = match serde_json::to_value(args) {
        Ok(Value::Object(map)) => map.keys().filter(|k| *k != "op").cloned().collect(),
        _ => Vec::new(),
    };
    let ok = allowed(args.op);
    if let Some(extra) = given.iter().find(|k| !ok.contains(&k.as_str())) {
        return Outcome::Done(Err(format!("{extra} does not belong to op {}; it takes {}", op_name(args.op), if ok.is_empty() { "nothing else".to_string() } else { ok.join(", ") })));
    }
    let done = Outcome::Done;
    use ReferencesOp as O;
    match args.op {
        O::State => done(Ok(state_json(cx.doc))),
        O::Dock => done(Ok(dock_shown(cx.doc, args.open))),
        O::Browse => done(Ok(browse(cx.doc, args.kind.unwrap_or_default(), args.open.unwrap_or(true)))),
        O::Select => done(select(cx.doc, args.id.as_ref())),
        O::Form => done(form::set(cx.doc, args.plane, args.locked)),
        O::Align => done(target(cx.doc, args.id.as_ref()).and_then(|id| align::align(cx, &id))),
        O::Sketch => align::sketch(args, call, cx),
        O::Calibrate => done(calibrate::start(cx, call, args.id.as_ref())),
        O::CalibratePick => done(calibrate::pick(cx.doc, args)),
        O::Cancel => done(Ok(calibrate::cancel(cx.doc))),
        O::CalibrateDistance => calibrate::distance(cx.doc, call, args.distance),
        O::Add | O::Visible | O::Placement | O::Remove | O::Link | O::Accept | O::Unlink => edits::handle(args, call, cx),
        O::OpenBuilder => done(system_link::open_builder(cx.doc, cx.view.is_some())),
    }
}

/// `op: dock`: shown, hidden or toggled. Opening reads the system status again
/// (the linked file may have changed on disk without a revision in RoboCAD).
pub(crate) fn dock_shown(doc: &mut CadDocument, open: Option<bool>) -> Value {
    let open = open.unwrap_or(!doc.references.open);
    if open && !doc.references.open {
        doc.references.reads.status = None;
    }
    doc.references.open = open;
    if !open {
        doc.references.browse = None;
    }
    doc.touch();
    json!({"open": open})
}

/// `op: browse`: the path field of Add or Link, opened (the dock with it) or closed.
pub(crate) fn browse(doc: &mut CadDocument, kind: BrowseKind, open: bool) -> Value {
    if !open {
        doc.references.browse = None;
        doc.touch();
        return json!({"browse": null});
    }
    let dir = super::results::doc_path(doc).and_then(|p| p.parent().map(|d| d.display().to_string())).filter(|d| d.starts_with('/'));
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
    let text = format!("{}/", dir.unwrap_or(home).trim_end_matches('/'));
    if !doc.references.open {
        doc.references.reads.status = None;
    }
    let st = &mut doc.references;
    st.open = true;
    st.browse = Some(Browse { kind, draft: TextDraft::new(text, false), error: None, listing_asked: None });
    st.claim = Some(Focus::Path);
    doc.touch();
    json!({"browse": browse_json(doc)})
}

/// `op: select`: the list's current image; its form is loaded from its placement.
fn select(doc: &mut CadDocument, id: Option<&String>) -> Result<Value, String> {
    let id = id.ok_or("select takes id: an image node")?;
    let id = target(doc, Some(id))?;
    if doc.references.current.as_deref() != Some(id.as_str()) {
        doc.references.current = Some(id.clone());
        doc.references.form = None;
        if matches!(doc.references.focus, Some(Focus::Row(_))) {
            doc.references.focus = None;
        }
        reads::follow_form(doc);
        doc.touch();
    }
    Ok(json!({"current": id, "form": doc.references.form.as_ref().map(form::PlacementForm::json)}))
}

fn browse_json(doc: &CadDocument) -> Value {
    doc.references.browse.as_ref().map_or(Value::Null, |b| json!({"kind": b.kind, "path": b.draft.text, "error": b.error}))
}

/// `cad_state.references`.
pub(in crate::cad) fn state_json(doc: &CadDocument) -> Value {
    let st = &doc.references;
    let list: Vec<Value> = images(doc)
        .into_iter()
        .map(|n| {
            let placed = st.reads.placements.get(&n.id);
            json!({
                "id": n.id, "name": n.name, "visible": n.visible, "locked": n.locked,
                "placement": placed.map(|p| match &p.placement { Ok(pl) => json!(pl), Err(e) => json!({"error": e}) }),
                "placement_at": placed.map(|p| p.revision),
                "placement_current": placed.is_some_and(|p| p.revision == doc.shown_revision()),
                "texture": st.pixels.get(&n.id).map(planes::Pixels::json),
            })
        })
        .collect();
    json!({
        "open": st.open,
        "current": st.current,
        "images": list,
        "form": st.form.as_ref().map(form::PlacementForm::json),
        "focus": st.focus.map(|f| format!("{f:?}")),
        "browse": browse_json(doc),
        "system": system_link::json(doc),
        "calibrate": st.calibrate.as_ref().map(calibrate::Calibrate::json),
        "pending_edit": st.pending.as_ref().map(|(seq, p)| json!({"edit": seq, "kind": format!("{p:?}")})),
        "placements_reading": st.reads.placement_job.is_some(),
    })
}

/// `sync::finish_edit`: edit `seq` answered (`result`: RoboCAD's answer, or
/// its refusal). An import selects its last image, shows the dock and
/// aligns on it once its placement is read (references.py:174-184); a link
/// edit rereads the status; a calibration ends the tool (tools.py:1238-1241).
/// A refusal shows where the request was made: under Apply placement, in the
/// calibrate tool (which stays, as RoboCAD's `_safe` keeps it), or in the
/// path field, reopened with the import's or link's path.
pub(in crate::cad) fn edit_answered(doc: &mut CadDocument, seq: u64, result: Result<&Value, &String>) {
    let Some((at, _)) = &doc.references.pending else { return };
    if *at != seq {
        return;
    }
    let Some((_, kind)) = doc.references.pending.take() else { return };
    // The path field of a refused import or link: (kind, the path typed).
    let refused_path = match &kind {
        Pending::Import(paths) => paths.first().map(|p| (BrowseKind::Images, p.clone())),
        Pending::Link(path) => Some((BrowseKind::System, path.clone())),
        _ => None,
    };
    match (kind, result) {
        (Pending::Import(_), Ok(r)) => {
            let last = r.get("result").and_then(Value::as_array).and_then(|ids| ids.last()).and_then(Value::as_str).map(str::to_string);
            if let Some(id) = last {
                doc.references.current = Some(id.clone());
                doc.references.form = None;
                doc.references.open = true;
                doc.references.align_after = Some((id, doc.shown_revision()));
            }
        }
        (Pending::Link(_) | Pending::Status, _) => doc.references.reads.status = None,
        (Pending::Calibrate, Ok(_)) => doc.references.calibrate = None,
        (Pending::Placement, Err(e)) => {
            if let Some(f) = doc.references.form.as_mut() {
                f.error = Some(e.clone());
            }
        }
        (Pending::Calibrate, Err(e)) => {
            if let Some(c) = doc.references.calibrate.as_mut() {
                c.error = Some(e.clone());
            }
        }
        (Pending::Placement, Ok(_)) | (Pending::Import(_), Err(_)) => {}
    }
    if let (Some(kind), Err(e)) = (refused_path, result) {
        browse(doc, kind.0, true);
        if let Some(b) = doc.references.browse.as_mut() {
            b.draft = TextDraft::new(kind.1, false);
            b.error = Some(e.clone());
        }
    }
    // The answered revision is read again (placements and form follow it).
    doc.touch();
}

/// The connection restarted (`sync::start`): what waited on the old one ends:
/// the edit this part waits for, the calibrate tool, the align after an
/// import and the path field.
pub(crate) fn restarted(doc: &mut CadDocument) {
    let st = &mut doc.references;
    st.pending = None;
    st.calibrate = None;
    st.align_after = None;
    st.browse = None;
    st.claim = None;
    if matches!(st.focus, Some(Focus::Path | Focus::Distance)) {
        st.focus = None;
    }
    doc.touch();
}

/// The selection's click stands aside while the calibrate tool takes clicks.
pub(in crate::cad) fn takes_clicks(doc: &CadDocument) -> bool {
    doc.references.calibrate.as_ref().is_some_and(|c| c.picks.len() < 2)
}

/// The action a RoboCAD command id stands for (`surfaces::registry`'s
/// `Do::Organize`): References (the dock shown) and Add reference images…
/// (the dock with its path field).
pub(in crate::cad) fn command_action(id: &str) -> Option<CadAction> {
    match id {
        "view.references" => Some(ReferencesArgs { open: Some(true), ..ReferencesArgs::of(ReferencesOp::Dock) }.action()),
        "reference.import" => Some(ReferencesArgs { open: Some(true), kind: Some(BrowseKind::Images), ..ReferencesArgs::of(ReferencesOp::Browse) }.action()),
        _ => None,
    }
}

/// A control: (id, label, action, ready).
pub(crate) type Control = (String, String, CadAction, Result<(), String>);

/// The references' `system_ui` controls (`cad:references:<id>`).
pub(in crate::cad) fn controls(cx: &Cx) -> Vec<Control> {
    controls_of(cx.doc)
}

/// The controls of `doc` (the dock's buttons take their action and enabled state from here).
pub(crate) fn controls_of(doc: &CadDocument) -> Vec<Control> {
    let st = &doc.references;
    let c = |id: &str, label: &str, args: ReferencesArgs, ready: Result<(), String>| (format!("cad:references:{id}"), label.to_string(), args.action(), ready);
    let connected = || doc.edit_refusal().map_or(Ok(()), Err);
    let mut out = vec![
        c("dock", if st.open { "Close" } else { "References" }, ReferencesArgs { open: Some(!st.open), ..ReferencesArgs::of(ReferencesOp::Dock) }, Ok(())),
        c("add", "＋ Add reference images…", ReferencesArgs { open: Some(true), kind: Some(BrowseKind::Images), ..ReferencesArgs::of(ReferencesOp::Browse) }, Ok(())),
        c("link", "Link system file…", ReferencesArgs { open: Some(true), kind: Some(BrowseKind::System), ..ReferencesArgs::of(ReferencesOp::Browse) }, Ok(())),
        c("accept", "Accept changes", ReferencesArgs::of(ReferencesOp::Accept), system_link::accept_ready(doc).and_then(|()| connected())),
        c("open_builder", "Open in builder", ReferencesArgs::of(ReferencesOp::OpenBuilder), system_link::open_ready(doc)),
        c("unlink", "Unlink", ReferencesArgs::of(ReferencesOp::Unlink), system_link::unlink_ready(doc).and_then(|()| connected())),
    ];
    if st.browse.is_some() {
        out.push(c("browse_close", "Cancel", ReferencesArgs { open: Some(false), ..ReferencesArgs::of(ReferencesOp::Browse) }, Ok(())));
    }
    let shown = doc.shown_revision();
    for n in images(doc) {
        out.push(c(&format!("image-{}", n.id), &n.name, ReferencesArgs::on(ReferencesOp::Select, &n.id), Ok(())));
        let label = if n.visible { "Shown" } else { "Hidden" };
        out.push(c(&format!("visible-{}", n.id), label, ReferencesArgs { visible: Some(!n.visible), revision: Some(shown), ..ReferencesArgs::on(ReferencesOp::Visible, &n.id) }, connected()));
    }
    if let Some(id) = st.current.clone().filter(|id| image(doc, id).is_some()) {
        let f = st.form.as_ref().filter(|f| f.id == id);
        for choice in PlaneChoice::ALL {
            out.push(c(&format!("plane-{}", choice.name()), choice.label(), ReferencesArgs { plane: Some(choice), ..ReferencesArgs::of(ReferencesOp::Form) }, f.map_or(Err("the placement is being read".into()), |_| Ok(()))));
        }
        let locked = f.is_some_and(|f| f.locked);
        out.push(c("locked", "Lock reference against selection", ReferencesArgs { locked: Some(!locked), ..ReferencesArgs::of(ReferencesOp::Form) }, f.map_or(Err("the placement is being read".into()), |_| Ok(()))));
        let (apply, ready) = match f.map(form::PlacementForm::apply) {
            Some(Ok(args)) => (args, connected()),
            Some(Err(e)) => (ReferencesArgs::on(ReferencesOp::Placement, &id), Err(e)),
            None => (ReferencesArgs::on(ReferencesOp::Placement, &id), Err(format!("the placement of {} is being read from RoboCAD", doc.node_name(&id)))),
        };
        out.push(c("apply", "Apply placement", apply, ready));
        let placed = || reads::current_placement(doc, &id).map(|_| ());
        out.push(c("align", "Align view", ReferencesArgs::on(ReferencesOp::Align, &id), placed()));
        out.push(c("calibrate", "Calibrate scale", ReferencesArgs::on(ReferencesOp::Calibrate, &id), placed()));
        out.push(c("sketch", "Sketch over this", ReferencesArgs::on(ReferencesOp::Sketch, &id), placed()));
        out.push(c("remove", "Remove reference", ReferencesArgs::on(ReferencesOp::Remove, &id), connected()));
    }
    if st.calibrate.is_some() {
        out.push(c("calibrate_cancel", "Cancel calibration", ReferencesArgs::of(ReferencesOp::Cancel), Ok(())));
    }
    out
}

/// The references' REST command.
pub(in crate::cad) fn specs() -> Vec<Spec> {
    vec![crate::app::actions::spec(
        "cad_references",
        super::actions::CAD,
        json!({"op": "placement", "id": "3f2a9c1b7d4e", "plane": "front", "width": 120.0, "origin": [0.0, 0.0, 0.0], "rotation_deg": 0.0, "opacity_pct": 60.0, "locked": true, "revision": 12}),
        "CAD mode: RoboCAD's reference images and linked system file (cad_state.references). op: state; dock (open true | false, absent toggles the References dock); browse (kind images | system, open true | false: the dock's path field of \"＋ Add reference images…\" or \"Link system file…\"); add (paths: absolute image files; one import_references on the active plane, else XY; the last becomes current and the view aligns on it); select (id: the list's current image, whose placement fills the form); visible (id, visible, revision: one update_reference); form (plane keep | front | side | top | active, locked: the form's choices, display only); placement (Apply placement: id, plane, width mm 0.001–1e7, origin [x, y, z] mm, rotation_deg ±360, opacity_pct 0–100, locked, revision the values were read at; absent values are the image's current ones; one update_reference, keep sends no plane); align (id: the camera square to the image, orthographic, and the image's plane active; display only); calibrate (id: align, then two clicks on the image's plane); calibrate_pick (point [x, y, z] mm on the plane, picked_at the shown revision); calibrate_distance (distance mm: one calibrate_reference keeping the first point still); cancel (ends the calibrate tool); sketch (id: align, then the Line sketch tool); remove (id: RoboCAD's delete); link (path: an absolute *.system.json: one link_system); accept (one refresh_system_link, when the file changed since it was linked); unlink (one unlink_system); open_builder (this window switches to Build mode on the linked file). Each edit is refused by name when another edit is in flight or the values are from an older revision. system_ui lists cad:references:*.",
    )]
}

/// CadCorePlugin's windowless references systems: the placement and status
/// reads, the align after an import and Open in builder's switch (JobResults,
/// after `sync::receive`).
pub(crate) struct CoreParts;
impl Plugin for CoreParts {
    fn build(&self, app: &mut App) {
        reads::build_core(app);
    }
}

/// CadPlugin: the dock's fields, dropped files, the calibrate tool's clicks
/// and markers, and the image planes.
pub(in crate::cad) fn build(app: &mut App) {
    input::build(app);
    drop::build(app);
    calibrate::build(app);
    planes::build(app);
}
