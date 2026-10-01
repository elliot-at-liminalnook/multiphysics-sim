//! CAD mode's actions (native-viewer.md §3, §9 phase 1). Every intent in CAD
//! mode is a [`CadAction`]: the tree's rows, 3D picks, the panel's buttons,
//! keys (`keys`), `system_ui` and REST all write `Act<CadAction>`, and
//! [`apply`] (ViewerSet::Actions) is the one handler. Mutations go to
//! RoboCAD's command layer through its REST routes on jobs, so RoboCAD's
//! undo, provenance and `.rcad` file stay its own.
use crate::app::actions::{self, Spec, spec};
use crate::app::ViewerMode;
use super::document::{CadTool, SelectMode};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sim_runtime::cad_client::SelectionItem;
use std::path::PathBuf;

/// One end of a measurement (RoboCAD's `MeasureTool` pick): the picked item
/// (`[node, kind, index]`, or none for a point in space) and the point
/// (mm, RoboCAD's frame), snapped as the measure tool snaps.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MeasurePick {
    #[serde(default)]
    pub item: Option<SelectionItem>,
    pub point: [f64; 3],
}

/// A live dimension's kind (RoboCAD's `live_dimensions`): a cylinder's
/// diameter (`set_diameter`), the distance between two parallel planar
/// faces (`set_distance`) or the angle between two planar faces (`set_angle`).
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Dimension {
    Diameter,
    Distance,
    Angle,
}

/// CAD mode's REST modes.
pub const CAD: &[ViewerMode] = &[ViewerMode::Cad];

/// Every intent of CAD mode. REST commands keep their JSON shape
/// (`{"command": name, ...args}`).
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum CadAction {
    /// `state` (as every mode answers it): the same as `cad_state`.
    State,
    /// The document as this window shows it: connection, service, document
    /// id and revision, dirty, stale, tree, selection, inspected node,
    /// history, commands, edit in flight.
    CadState,
    /// Open a `.rcad` file (start RoboCAD's headless service on it) or attach
    /// to a running RoboCAD at `url`. Exactly one of the two.
    CadOpen {
        #[serde(default)]
        path: Option<PathBuf>,
        #[serde(default)]
        url: Option<String>,
    },
    /// Replace the selection with `items` (RoboCAD's `[node, kind, index]`)
    /// and `ids` (body items `[id, "body", 0]`); empty clears it. `extend`
    /// adds them (Shift), `toggle` adds or removes each (Ctrl). Pushed to
    /// RoboCAD's `/selection` with the mode. Tree rows, 3D picks, box select,
    /// the Alt menu, keys, `system_ui` and REST all write this.
    CadSelect {
        #[serde(default)]
        ids: Vec<String>,
        #[serde(default)]
        items: Vec<SelectionItem>,
        #[serde(default)]
        extend: bool,
        #[serde(default)]
        toggle: bool,
    },
    /// Switch the selection mode (body, face, edge, vertex, point); as in
    /// RoboCAD, the selection is cleared.
    CadSelectMode { mode: SelectMode },
    /// The item under the pointer (display only: the hover highlight; a
    /// click pick never waits on it).
    CadHover {
        #[serde(default)]
        item: Option<SelectionItem>,
    },
    /// Box select: the rectangle `[x0, y0, x1, y1]` in window logical
    /// pixels, as RoboCAD's `_box_select` (bodies whose bounds lie inside;
    /// edges whose samples all lie inside; vertices inside). `extend` keeps
    /// the selection.
    CadBoxSelect {
        rect: [f32; 4],
        #[serde(default)]
        extend: bool,
    },
    /// Open the Alt+click menu over the stacked `items` under the cursor;
    /// choosing one writes `CadSelect` with `extend`/`toggle`.
    CadCandidates {
        items: Vec<SelectionItem>,
        #[serde(default)]
        extend: bool,
        #[serde(default)]
        toggle: bool,
    },
    /// RoboCAD's `edit.select_all`: every visible body, sheet, curve,
    /// instance and mesh as body items.
    CadSelectAll,
    /// RoboCAD's `edit.invert`.
    CadInvertSelection,
    /// RoboCAD's `edit.select_same_material` (the first selected node's material).
    CadSelectSameMaterial,
    /// RoboCAD's `edit.convert_faces`: the selected edges become the faces
    /// they bound, and the mode becomes face.
    CadEdgesToFaces,
    /// Activate a tool (select, move, rotate, scale, push_pull, offset_face, measure).
    CadTool { tool: CadTool },
    /// Commit a transform: exactly one `POST /ops/transform` (RoboCAD's
    /// `Ops.transform(ids, translation, axis, angle_deg, center, scale)`,
    /// uniform scale) on the selected nodes, or `ids`. `revision`: RoboCAD's
    /// revision the preview began at; refused when it changed.
    CadTransform {
        #[serde(default)]
        ids: Option<Vec<String>>,
        #[serde(default)]
        translation: Option<[f64; 3]>,
        #[serde(default)]
        axis: Option<[f64; 3]>,
        #[serde(default)]
        angle_deg: Option<f64>,
        #[serde(default)]
        center: Option<[f64; 3]>,
        #[serde(default)]
        scale: Option<f64>,
        #[serde(default)]
        revision: Option<u64>,
    },
    /// Push/pull one planar face `distance` mm along its normal: one
    /// `POST /ops/push_pull` (`Ops.push_pull(node_id, face, distance)`).
    CadPushPull {
        node: String,
        face: i64,
        distance: f64,
        #[serde(default)]
        revision: Option<u64>,
    },
    /// Offset faces `distance` mm: one `POST /ops/offset_faces`
    /// (`Ops.offset_faces(node_id, faces, distance)`).
    CadOffsetFaces {
        node: String,
        faces: Vec<i64>,
        distance: f64,
        #[serde(default)]
        revision: Option<u64>,
    },
    /// Set a live dimension: one `POST /ops/set_diameter` (faces `[f]`),
    /// `set_distance` (faces `[a, b]`, `b` moves) or `set_angle` (`[a, b]`).
    CadSetDimension {
        node: String,
        dimension: Dimension,
        faces: Vec<i64>,
        value: f64,
        #[serde(default)]
        revision: Option<u64>,
    },
    /// The numeric bar's Enter: each field's text, evaluated with RoboCAD's
    /// unit expressions (`sim_runtime::units::evaluate`), committed through
    /// the active tool's one call.
    CadNumeric { values: Vec<String> },
    /// Measure between two picks (RoboCAD's `measure_between`): computed
    /// here; `keep` also adds it as a measure node (one `POST
    /// /ops/add_measurement`, RoboCAD's Shift+click).
    CadMeasure {
        a: MeasurePick,
        b: MeasurePick,
        #[serde(default)]
        keep: bool,
    },
    /// Escape: cancel the preview and return to the Select tool; in the
    /// Select tool, clear the selection (RoboCAD's app.py:487-497).
    CadCancel,
    /// `PATCH /nodes/{id}` with `attrs` as RoboCAD accepts them (name,
    /// visible, locked, disabled, material, color, pivot, transform, parent,
    /// index, tessellation_tolerance, plane, sketch): one undo step there.
    CadPatch { id: String, attrs: Map<String, Value> },
    /// `DELETE /nodes/{id}` (RoboCAD's delete command, undoable there).
    CadDelete { id: String },
    CadUndo,
    CadRedo,
    /// `POST /save` (RoboCAD writes its own file; `path` saves as).
    CadSave {
        #[serde(default)]
        path: Option<String>,
    },
    /// Run one of RoboCAD's GUI registry commands (`POST /commands/{id}`;
    /// RoboCAD's desktop window only).
    CadCommand { id: String },
    /// Call an `Ops` method (`POST /ops/{name}`): RoboCAD's command layer,
    /// headless or GUI.
    CadOp {
        name: String,
        #[serde(default)]
        args: Vec<Value>,
        #[serde(default)]
        kwargs: Map<String, Value>,
    },
    /// Invoke catalogue operation `id` (`ops::CATALOGUE`, RoboCAD's command
    /// id) as its menu entry, toolbar button, palette row or key does: an
    /// immediate one runs at once on the selection with its defaults, a
    /// form one opens its parameter form, a pick or place one becomes the
    /// active interaction with its form beside the view.
    CadInvoke { id: String },
    /// Run catalogue operation `id` with `params` (each a number, a unit
    /// expression as its field takes it, a choice's option, a bool, or
    /// `[x, y, z]`; absent ones take RoboCAD's defaults) on `items`
    /// (RoboCAD's `[node, kind, index]`; the selection when absent).
    /// `revision`: RoboCAD's revision the values and picks were made at;
    /// refused by name when it changed. One edit on a job.
    CadRun {
        id: String,
        #[serde(default)]
        params: Map<String, Value>,
        #[serde(default)]
        items: Option<Vec<SelectionItem>>,
        #[serde(default)]
        revision: Option<u64>,
    },
    /// Set the open form's parameter `name` (a choice's option, a
    /// checkbox's bool, or a field's text).
    CadFormSet { name: String, value: Value },
    /// The open form's OK: its values evaluated, then `CadRun`.
    CadFormSubmit,
    /// The open form's Cancel (and Escape): closes it and ends its interaction.
    CadFormCancel,
    /// Open a command surface (the palette, a category's menu, the context
    /// menu, the view or selection-mode radial) or close it (`closed`).
    CadSurface { surface: super::surfaces::Surface },
    /// Refetch the document now.
    CadRefresh,
    /// Frame the native camera on everything, or on node `id` (display only:
    /// RoboCAD's own view is not changed).
    CadFit {
        #[serde(default)]
        id: Option<String>,
    },
    /// Fetch `GET /physical?flex=0` for the inspector's physical section
    /// (RoboCAD derives it; nothing is written).
    CadPhysical,
    /// `system_ui` in CAD mode: `{action: {operation: controls | activate, id?, ui_revision?}}`.
    SystemUi(Map<String, Value>),
}

impl actions::Action for CadAction {
    fn commands() -> Vec<Spec> {
        vec![
            spec("state", CAD, json!({}), "CAD mode: the same answer as cad_state, plus viewer_mode."),
            spec("cad_state", CAD, json!({}), "CAD mode: the RoboCAD document as this window shows it: target (file or service URL), service (self-started or attached; url, pid), connection (connecting | connected | lost with the error verbatim), health (RoboCAD's GET /: path, dirty, gui, nodes, document_id, revision), stale (why the shown tree may be behind RoboCAD, or null), nodes (id, kind, name, parent, depth, visible, effective_visible, locked, disabled), selection (RoboCAD's items [node, kind, index], synced with its /selection), select_mode (body | face | edge | vertex | point), hover (the item under the pointer, display only), candidates (the Alt menu while open), selection_error (why the last read of RoboCAD's /selection failed, or null), unsaved (RoboCAD's dirty flag, or null when it can't be confirmed: not connected, or an edit in flight or just finished), inspected (RoboCAD's node detail for the first selected node, exactly as returned), physical (fetched with cad_physical), history (undo and redo labels), commands (RoboCAD's GUI registry; empty headless), autosave (GUI only), edit (the mutating request in flight, by name), meshes (shown, pending, failed), tool (the active tool), tool_state (pivot, drag and preview with the revision each began at, push/pull target, numeric fields with their evaluations, last measurement, snap, readout) and status (the last outcome)."),
            spec("cad_open", CAD, json!({"path": "examples/camera-turntable/cad/turntable.rcad"}), "CAD mode: open a .rcad file (path: RoboCAD's headless service is started on it with cad/.venv/bin/python -m robocad.api PATH --port N, and stopped when the document closes) or attach to a running RoboCAD (url, loopback only, e.g. http://127.0.0.1:8420; never stopped). Refused, naming the reason, while an edit is in flight or while a self-started document has unsaved edits or edits whose saved state can't be confirmed (not connected to it; save first: the viewer never saves for you). Answers once the service is starting; poll cad_state for the connection."),
            spec("cad_select", CAD, json!({"items": [["n1", "face", 2]], "extend": false}), "CAD mode: select items as RoboCAD writes them (items: [[node, kind, index], ...], kind body | face | edge | vertex | point; ids: node ids as body items [id, \"body\", 0]; [] clears; extend adds like Shift; toggle adds or removes like Ctrl). The same action as a click on a tree row, a pick or box select in the 3D view, or a choice in the Alt menu. Pushed to RoboCAD's PUT /selection with the selection mode, so RoboCAD's window shows it too; RoboCAD's own selection changes come back on the next poll."),
            spec("cad_select_mode", CAD, json!({"mode": "face"}), "CAD mode: the selection mode (body | face | edge | vertex | point; RoboCAD's B, Shift+B, E, V, P): what a click in the 3D view picks. Clears the selection, as RoboCAD does. A headless RoboCAD does not store the mode (its GET /selection has none), so the viewer holds it; a desktop RoboCAD's mode is adopted when it changes there."),
            spec("cad_hover", CAD, json!({"item": ["n1", "face", 2]}), "CAD mode: the hover highlight (item [node, kind, index], or null to clear). Display only: nothing is sent to RoboCAD."),
            spec("cad_box_select", CAD, json!({"rect": [300.0, 200.0, 600.0, 450.0]}), "CAD mode: box select over the window rectangle rect [x0, y0, x1, y1] (logical pixels), as RoboCAD's drag: in body mode the bodies whose bounding box lies inside, in edge mode the edges whose sampled polylines lie inside, in vertex mode the vertices inside. extend keeps the selection (Shift or Ctrl)."),
            spec("cad_candidates", CAD, json!({"items": [["n1", "face", 2], ["n2", "face", 0]]}), "CAD mode: open the Alt+click menu over stacked candidates (items as cad_select's; extend, toggle say how a choice applies). Its entries are system_ui controls cad:candidate:<n>."),
            spec("cad_select_all", CAD, json!({}), "CAD mode: RoboCAD's Select All (Ctrl+A): every visible body, sheet, curve, instance and mesh, as body items."),
            spec("cad_invert_selection", CAD, json!({}), "CAD mode: RoboCAD's Invert Selection (Ctrl+Shift+I): the visible bodies, sheets, curves, instances and meshes not selected now."),
            spec("cad_select_same_material", CAD, json!({}), "CAD mode: RoboCAD's Select Same Material (Ctrl+Shift+M): every node with the first selected node's material."),
            spec("cad_edges_to_faces", CAD, json!({}), "CAD mode: RoboCAD's 'Selection: edges → bounding faces': each selected edge becomes the faces it bounds (found from RoboCAD's tessellation: faces with a triangle side along the edge), and the mode becomes face."),
            spec("cad_tool", CAD, json!({"tool": "move"}), "CAD mode: activate a tool (select | move | rotate | scale | push_pull | offset_face | measure; RoboCAD's Escape, G, R, S, D, Shift+D, M). Move, rotate and scale place the gizmo at the first selected node's pivot (else the selection's centre); push/pull and offset target the first selected face; measure takes two picks. Display only until a commit."),
            spec("cad_transform", CAD, json!({"translation": [10.0, 0.0, 0.0]}), "CAD mode: commit a transform as exactly one POST /ops/transform (RoboCAD's Ops.transform: translation [dx, dy, dz] mm, or axis [x, y, z] with angle_deg about center, or a uniform scale factor about center) on the selected nodes or ids. revision: RoboCAD's revision the preview began at (refused, naming it, when the document changed since). Refused while another edit is in flight. RoboCAD's command layer applies it as one undo step."),
            spec("cad_push_pull", CAD, json!({"node": "n1", "face": 3, "distance": 5.0}), "CAD mode: push/pull planar face index face of node by distance mm along its normal: exactly one POST /ops/push_pull (one undo step in RoboCAD). revision as cad_transform's. Refused while another edit is in flight."),
            spec("cad_offset_faces", CAD, json!({"node": "n1", "faces": [3], "distance": 1.5}), "CAD mode: offset faces of node by distance mm: exactly one POST /ops/offset_faces (one undo step). revision as cad_transform's."),
            spec("cad_set_dimension", CAD, json!({"node": "n1", "dimension": "diameter", "faces": [4], "value": 8.0}), "CAD mode: set a live dimension of the selected faces (RoboCAD's live_dimensions): diameter of a cylindrical face (faces [f], set_diameter), distance between two parallel planar faces (faces [a, b], b moves, set_distance) or angle between two planar faces (faces [a, b], degrees, set_angle). Exactly one POST /ops call; revision as cad_transform's."),
            spec("cad_numeric", CAD, json!({"values": ["20mm + 0.3"]}), "CAD mode: the numeric bar's Enter: values are the active tool's fields as typed (move dx dy dz, rotate angle, scale factor, push/pull or offset distance, a live dimension), each a unit expression RoboCAD's numeric bar accepts (20mm + 0.3, 1in, pi*10, 45deg; bare numbers are mm or degrees), evaluated by sim_runtime::units. An error names the token and its position. Commits through the tool's one call."),
            spec("cad_measure", CAD, json!({"a": {"item": ["n1", "face", 0], "point": [0.0, 0.0, 0.0]}, "b": {"item": ["n1", "face", 5], "point": [0.0, 0.0, 20.0]}}), "CAD mode: measure between two picks as RoboCAD's measure tool (distance between parallel planar faces, the angle between faces or between line edges, an edge's radius when both picks are the same circular edge, else the points' distance). Answers the measurement; keep (RoboCAD's Shift+click) also adds it as a measure node with exactly one POST /ops/add_measurement."),
            spec("cad_cancel", CAD, json!({}), "CAD mode: Escape: cancel a preview and return to the Select tool; in the Select tool, clear the selection. Nothing is sent to RoboCAD except the cleared selection."),
            spec("cad_patch", CAD, json!({"id": "n1", "attrs": {"visible": false}}), "CAD mode: PATCH /nodes/{id} with attrs exactly as RoboCAD accepts them (name, visible, locked, disabled, material, color, pivot, transform, parent, index, tessellation_tolerance, plane, sketch): RoboCAD's command layer applies it as one undo step. One edit at a time: refused while another is in flight. Answers RoboCAD's node detail, or its error verbatim."),
            spec("cad_delete", CAD, json!({"id": "n1"}), "CAD mode: DELETE /nodes/{id} through RoboCAD's delete command (undoable there). Refused while another edit is in flight."),
            spec("cad_undo", CAD, json!({}), "CAD mode: POST /undo: RoboCAD undoes its last command; answers the label undone and the history."),
            spec("cad_redo", CAD, json!({}), "CAD mode: POST /redo: RoboCAD redoes; answers the label redone and the history."),
            spec("cad_save", CAD, json!({}), "CAD mode: POST /save: RoboCAD writes its document to its own path (or path, to save as). The viewer never writes the .rcad itself."),
            spec("cad_command", CAD, json!({"id": "view.fit"}), "CAD mode: run a RoboCAD GUI registry command by id (POST /commands/{id}; ids from cad_state.commands). RoboCAD's desktop window only: a headless service answers 409 \"no GUI\", shown verbatim."),
            spec("cad_op", CAD, json!({"name": "rename", "args": ["n1", "Bracket"]}), "CAD mode: call RoboCAD's command layer (POST /ops/{name} {args, kwargs}; names and signatures from RoboCAD's GET /ops). Works headless. Refused while another edit is in flight."),
            spec("cad_refresh", CAD, json!({}), "CAD mode: refetch RoboCAD's /doc, /commands and /autosave now."),
            spec("cad_fit", CAD, json!({}), "CAD mode: frame the native 3D view on every shown body, or on node id. Display only: RoboCAD's own view and the geometry are not changed."),
            spec("cad_physical", CAD, json!({}), "CAD mode: fetch RoboCAD's physical description (GET /physical?flex=0; nothing is written) for the inspector: the link holding the selected body, its mass, centre of mass and inertia, and mass_sources as RoboCAD labels them (declared measurement source, material density, ...), never filled in."),
            spec("system_ui", CAD, json!({"action": {"operation": "controls"}}), "CAD mode: its controls (cad:undo, cad:redo, cad:save, cad:refresh, cad:fit, cad:physical, cad:delete, cad:node:<id> to select a tree row, cad:visible:<id> to toggle visibility, cad:locked:<id> to toggle locked, cad:disabled:<id> to toggle disabled, cad:material:<id>:<mat> to assign material <mat> (an id from RoboCAD's /doc materials) to node <id>, cad:command:<id> for RoboCAD's registry commands, cad:mode:<mode> for a selection mode, cad:select_all, cad:invert_selection, cad:select_same_material, cad:edges_to_faces, cad:candidate:<n> for an entry of the open Alt menu, cad:tool:<tool> to activate a tool, cad:cancel for Escape), each with enabled and disabled_reason, then the mode switcher's mode:* controls; activate {id} writes the same CAD action a click does."),
        ]
    }
    fn controls() -> &'static [&'static str] {
        &[
            "cad:undo", "cad:redo", "cad:save", "cad:refresh", "cad:fit", "cad:physical", "cad:delete", "cad:node:<id>", "cad:visible:<id>", "cad:locked:<id>", "cad:disabled:<id>", "cad:material:<id>:<mat>", "cad:command:<id>",
            "cad:mode:<mode>", "cad:select_all", "cad:invert_selection", "cad:select_same_material", "cad:edges_to_faces", "cad:candidate:<n>", "cad:tool:<tool>", "cad:cancel",
        ]
    }
}

// ---- The one handler -------------------------------------------------------

use super::document::{CadDocument, CadTarget, Connection, EditDone};
use super::mesh::CadMeshes;
use super::sync::{self, value};
use super::topology::CadTopology;
use super::view::CadView;
use crate::app::actions::{Act, Call, InFlight, Origin, Replies};
use crate::app::switch::Documents;
use bevy::ecs::message::Messages;
use bevy::prelude::*;
use sim_api::Outcome;
use sim_runtime::cad_client::CadClient;
use std::collections::HashSet;

/// Actions: CAD mode's one apply system. A click's or key's refusal is the
/// header's status line; a REST caller gets it, or the answer. Mutations
/// run on an edit job; their REST callers wait for RoboCAD's answer
/// (Pending with the edit's sequence in the continuation).
#[allow(clippy::too_many_arguments)]
pub(super) fn apply(
    mut messages: ResMut<Messages<Act<CadAction>>>,
    mut in_flight: ResMut<InFlight<CadAction>>,
    mut replies: ResMut<Replies>,
    doc: Option<ResMut<CadDocument>>,
    mut meshes: Option<ResMut<CadMeshes>>,
    mut topology: Option<ResMut<CadTopology>>,
    view: Option<Res<CadView>>,
    mut documents: ResMut<Documents>,
) {
    let Some(mut doc) = doc else {
        actions::apply(&mut messages, &mut in_flight, &mut replies, |_, _| Outcome::Done(Err("CAD mode has no document open".into())));
        return;
    };
    actions::apply(&mut messages, &mut in_flight, &mut replies, |action, call| {
        let mut cx = Cx { doc: &mut *doc, meshes: meshes.as_deref_mut(), topology: topology.as_deref_mut(), view: view.as_deref(), documents: &mut *documents };
        let outcome = handle(action, call, &mut cx);
        match call.origin {
            Origin::Rest(_) => outcome,
            origin => {
                if let (Outcome::Done(Err(e)), Origin::Ui) = (&outcome, origin) {
                    doc.show(Err(e.clone()));
                }
                Outcome::Done(Ok(Value::Null))
            }
        }
    });
}

/// What the one handler works on: the document and CAD mode's caches. The
/// selection arms (`selection::handle`) and the tool arms
/// (`transform::handle`) take the same context.
pub(super) struct Cx<'a> {
    pub doc: &'a mut CadDocument,
    /// The drawn bodies (None without a window).
    pub meshes: Option<&'a mut CadMeshes>,
    /// Faces, edges and vertices of the shown bodies (None without CAD mode's caches).
    pub topology: Option<&'a mut CadTopology>,
    /// The camera as last drawn (None without a window).
    pub view: Option<&'a CadView>,
    pub documents: &'a mut Documents,
}

/// One action, from any entry point.
pub(super) fn handle(action: &CadAction, call: &mut Call, cx: &mut Cx) -> Outcome {
    // A REST caller waiting for the edit it started (also through system_ui activate).
    if let Some(seq) = call.continuation.get("edit").and_then(Value::as_u64) {
        return wait_edit(cx.doc, call, seq);
    }
    let done = |r: Result<Value, String>| Outcome::Done(r);
    let doc = &mut *cx.doc;
    match action {
        CadAction::State | CadAction::CadState => done(Ok(state_json(doc, cx.meshes.as_deref()))),
        CadAction::CadOpen { path, url } => done(open(doc, cx.documents, path.as_ref(), url.as_deref())),
        CadAction::CadSelect { .. }
        | CadAction::CadSelectMode { .. }
        | CadAction::CadHover { .. }
        | CadAction::CadBoxSelect { .. }
        | CadAction::CadCandidates { .. }
        | CadAction::CadSelectAll
        | CadAction::CadInvertSelection
        | CadAction::CadSelectSameMaterial
        | CadAction::CadEdgesToFaces => super::selection::handle(action, call, cx),
        CadAction::CadTool { .. }
        | CadAction::CadTransform { .. }
        | CadAction::CadPushPull { .. }
        | CadAction::CadOffsetFaces { .. }
        | CadAction::CadSetDimension { .. }
        | CadAction::CadNumeric { .. }
        | CadAction::CadMeasure { .. }
        | CadAction::CadCancel => super::transform::handle(action, call, cx),
        CadAction::CadPatch { id, attrs } => {
            if !doc.has_node(id) {
                return done(Err(format!("no node {id} in the shown tree")));
            }
            let name = doc.node_name(id);
            let keys = attrs.keys().cloned().collect::<Vec<_>>().join(", ");
            let (id, attrs) = (id.clone(), attrs.clone());
            let message = format!("Patched {name}: {keys}");
            edit(doc, call, format!("Patch {name}: {keys}"), move |c| c.patch(&id, &attrs).map(|d| EditDone { message, result: value(&d) }))
        }
        CadAction::CadDelete { id } => {
            if !doc.has_node(id) {
                return done(Err(format!("no node {id} in the shown tree")));
            }
            let name = doc.node_name(id);
            let id = id.clone();
            let message = format!("Deleted {name}");
            edit(doc, call, format!("Delete {name}"), move |c| c.delete(&id).map(|d| EditDone { message, result: value(&d) }))
        }
        CadAction::CadUndo => edit(doc, call, "Undo".into(), |c| {
            c.undo().map(|u| EditDone { message: u.undone.as_ref().map_or_else(|| "Nothing to undo".to_string(), |l| format!("Undid {l}")), result: value(&u) })
        }),
        CadAction::CadRedo => edit(doc, call, "Redo".into(), |c| {
            c.redo().map(|r| EditDone { message: r.redone.as_ref().map_or_else(|| "Nothing to redo".to_string(), |l| format!("Redid {l}")), result: value(&r) })
        }),
        CadAction::CadSave { path } => {
            let label = match path {
                Some(p) => format!("Save as {p}"),
                None => "Save".to_string(),
            };
            let path = path.clone();
            edit(doc, call, label, move |c| c.save(path.as_deref()).map(|s| EditDone { message: format!("Saved {}", s.saved), result: value(&s) }))
        }
        CadAction::CadCommand { id } => {
            let id = id.clone();
            edit(doc, call, format!("Command {id}"), move |c| c.run_command(&id).map(|r| EditDone { message: format!("Ran RoboCAD command {}", r.ran), result: value(&r) }))
        }
        CadAction::CadOp { name, args, kwargs } => {
            let (name, args, kwargs) = (name.clone(), args.clone(), kwargs.clone());
            edit(doc, call, format!("Op {name}"), move |c| c.op(&name, &args, &kwargs).map(|r| EditDone { message: format!("Ran {name}"), result: value(&r) }))
        }
        CadAction::CadInvoke { .. } | CadAction::CadRun { .. } | CadAction::CadFormSet { .. } | CadAction::CadFormSubmit | CadAction::CadFormCancel => super::ops::handle(action, call, cx),
        CadAction::CadSurface { .. } => super::surfaces::handle(action, call, cx),
        CadAction::CadRefresh => done(Ok(refresh(doc))),
        CadAction::CadFit { id } => done(fit(doc, cx.meshes.as_deref_mut(), id.as_deref())),
        CadAction::CadPhysical => done(sync::fetch_physical(doc).map(|()| json!({"message": "Fetching RoboCAD's physical description (GET /physical?flex=0); it shows in cad_state.physical."}))),
        CadAction::SystemUi(args) => system_ui(call, cx, args),
    }
}

/// Start a mutating request; a REST caller waits for its answer. The one
/// path every document edit takes (the tools' commits too).
pub(super) fn edit(doc: &mut CadDocument, call: &mut Call, label: String, work: impl FnOnce(&CadClient) -> Result<EditDone, sim_runtime::cad_client::CadError> + Send + 'static) -> Outcome {
    match sync::start_edit(doc, label, call.rest(), work) {
        Err(e) => Outcome::Done(Err(e)),
        Ok(seq) if call.rest() => {
            *call.continuation = json!({"edit": seq, "generation": doc.generation});
            Outcome::Pending
        }
        Ok(_) => Outcome::Done(Ok(Value::Null)),
    }
}

/// A REST caller's edit: RoboCAD's answer (or error) verbatim once it lands.
/// A cancel does not abort it: the request was already sent.
fn wait_edit(doc: &mut CadDocument, call: &mut Call, seq: u64) -> Outcome {
    if call.continuation.get("generation").and_then(Value::as_u64) != Some(doc.generation) {
        return Outcome::Done(Err("the CAD document was replaced or reconnected while this edit waited; the request had already been sent to RoboCAD: see cad_state for the document as it is now".into()));
    }
    if let Some(result) = doc.edit_results.remove(&seq) {
        return Outcome::Done(result);
    }
    let running = doc.edit.is_some() && doc.edit_seq == seq;
    if call.cancelled {
        if running {
            doc.edit_waited = false;
        }
        return Outcome::Done(Err("cancelled waiting, but the request was already sent to RoboCAD and is not aborted: its outcome shows in cad_state.status".into()));
    }
    if !running {
        return Outcome::Done(Err("the edit ended without an answer for this request; see cad_state.status".into()));
    }
    Outcome::Pending
}

/// `cad_open`: replace the document (refused on an edit in flight or a
/// self-started document's unsaved edits, or edits it cannot confirm saved).
fn open(doc: &mut CadDocument, documents: &mut Documents, path: Option<&PathBuf>, url: Option<&str>) -> Result<Value, String> {
    let target = match (path, url) {
        (Some(p), None) => {
            if !p.to_string_lossy().ends_with(".rcad") {
                return Err(format!("{}: cad_open opens a *.rcad file", p.display()));
            }
            // Known cost: one stat on the UI thread, as the other modes'
            // opens do (the switch's `prepare` too); the file is read by
            // RoboCAD's service, never here.
            if !p.is_file() {
                return Err(format!("{}: no such file", p.display()));
            }
            CadTarget::File(p.clone())
        }
        (None, Some(u)) => {
            CadClient::new(u).map_err(|e| e.to_string())?;
            CadTarget::Service(u.to_string())
        }
        _ => return Err("cad_open takes path (a *.rcad file) or url (a loopback RoboCAD service), exactly one".into()),
    };
    let blockers = doc.switch_blockers();
    if !blockers.is_empty() {
        return Err(format!("Not opening {}: {}", target.describe(), blockers.join("; ")));
    }
    let note = doc.leaving_note();
    // The old document's self-started service is stopped now (also one
    // still starting), or left running if edits appeared since the check.
    let left_running = doc.release_child("opening another CAD document");
    let mut next = CadDocument::new(target.clone());
    next.revision = doc.revision + 1;
    sync::start(&mut next);
    let old = std::mem::replace(doc, next);
    // Its poll joins off the UI thread.
    crate::jobs::drop_off_thread(old, "the previous CAD document");
    documents.cad = Some(target.clone());
    let mut message = format!("Opening {}", target.describe());
    if let Some(note) = &note {
        message.push_str(&format!("; {note}"));
    }
    if let Some(url) = &left_running {
        message.push_str(&format!("; the RoboCAD service the previous document started may hold unsaved edits and is left running at {url} (open it there and save, or stop it)"));
    }
    doc.show(Ok(message.clone()));
    Ok(json!({"opened": target.json(), "message": message, "generation": doc.generation}))
}

/// `cad_refresh`: refetch now; reconnect when the connection is gone (a
/// self-started service that exited is started again).
fn refresh(doc: &mut CadDocument) -> Value {
    if doc.connect.is_some() {
        return json!({"message": "Already connecting."});
    }
    if doc.client.is_none() || doc.child_exit.is_some() {
        sync::start(doc);
        doc.show(Ok(format!("Reconnecting: {}", doc.target.describe())));
        return json!({"message": "Reconnecting.", "generation": doc.generation});
    }
    // Bodies whose mesh failed are fetched again (`mesh::sync`).
    doc.mesh_retry += 1;
    sync::refresh(doc, false);
    json!({"message": "Refetching RoboCAD's document."})
}

/// `cad_fit`: frame every drawn body, or node `id` and its descendants.
fn fit(doc: &CadDocument, meshes: Option<&mut CadMeshes>, id: Option<&str>) -> Result<Value, String> {
    let meshes = meshes.ok_or("CAD mode's 3D view is not available in this window")?;
    let ids = match id {
        None => None,
        Some(id) => {
            if !doc.has_node(id) {
                return Err(format!("no node {id} in the shown tree"));
            }
            Some(descendants(doc, id))
        }
    };
    let bounds = meshes.bounds(ids.as_ref()).ok_or_else(|| match id {
        Some(id) => format!("nothing to frame: no body of {} is drawn", doc.node_name(id)),
        None => "nothing to frame: no bodies are drawn yet".to_string(),
    })?;
    meshes.frame(bounds);
    Ok(json!({"framed": id.map_or_else(|| "every drawn body".to_string(), |id| doc.node_name(id)), "note": "display only: RoboCAD's view and the geometry are unchanged"}))
}

/// Node `id` and everything under it in the shown tree.
fn descendants(doc: &CadDocument, id: &str) -> HashSet<String> {
    let mut out: HashSet<String> = HashSet::from([id.to_string()]);
    let Some(state) = &doc.doc else { return out };
    // Walk order lists parents before children.
    for n in &state.nodes {
        if n.parent.as_ref().is_some_and(|p| out.contains(p)) {
            out.insert(n.id.clone());
        }
    }
    out
}

/// CAD mode's `system_ui` controls: the panel's own list (`panel::controls`),
/// so a control's label, enabled state and action are the button's.
fn controls(doc: &CadDocument) -> Vec<(String, String, CadAction, Result<(), String>)> {
    super::panel::controls(doc).into_iter().map(|c| (c.id, c.label, c.action, c.ready)).collect()
}

/// An action as its REST command (what `system_ui` lists as a control's action).
pub(super) fn rest_form(action: &CadAction) -> Value {
    match action {
        CadAction::CadUndo => json!({"command": "cad_undo"}),
        CadAction::CadRedo => json!({"command": "cad_redo"}),
        CadAction::CadSave { path } => json!({"command": "cad_save", "path": path}),
        CadAction::CadRefresh => json!({"command": "cad_refresh"}),
        CadAction::CadFit { id } => json!({"command": "cad_fit", "id": id}),
        CadAction::CadPhysical => json!({"command": "cad_physical"}),
        CadAction::CadDelete { id } => json!({"command": "cad_delete", "id": id}),
        CadAction::CadSelect { ids, items, extend, toggle } => json!({"command": "cad_select", "ids": ids, "items": items, "extend": extend, "toggle": toggle}),
        CadAction::CadSelectMode { mode } => json!({"command": "cad_select_mode", "mode": mode}),
        CadAction::CadHover { item } => json!({"command": "cad_hover", "item": item}),
        CadAction::CadBoxSelect { rect, extend } => json!({"command": "cad_box_select", "rect": rect, "extend": extend}),
        CadAction::CadCandidates { items, extend, toggle } => json!({"command": "cad_candidates", "items": items, "extend": extend, "toggle": toggle}),
        CadAction::CadSelectAll => json!({"command": "cad_select_all"}),
        CadAction::CadInvertSelection => json!({"command": "cad_invert_selection"}),
        CadAction::CadSelectSameMaterial => json!({"command": "cad_select_same_material"}),
        CadAction::CadEdgesToFaces => json!({"command": "cad_edges_to_faces"}),
        CadAction::CadTool { tool } => json!({"command": "cad_tool", "tool": tool}),
        CadAction::CadTransform { ids, translation, axis, angle_deg, center, scale, revision } => {
            json!({"command": "cad_transform", "ids": ids, "translation": translation, "axis": axis, "angle_deg": angle_deg, "center": center, "scale": scale, "revision": revision})
        }
        CadAction::CadPushPull { node, face, distance, revision } => json!({"command": "cad_push_pull", "node": node, "face": face, "distance": distance, "revision": revision}),
        CadAction::CadOffsetFaces { node, faces, distance, revision } => json!({"command": "cad_offset_faces", "node": node, "faces": faces, "distance": distance, "revision": revision}),
        CadAction::CadSetDimension { node, dimension, faces, value, revision } => {
            json!({"command": "cad_set_dimension", "node": node, "dimension": dimension, "faces": faces, "value": value, "revision": revision})
        }
        CadAction::CadNumeric { values } => json!({"command": "cad_numeric", "values": values}),
        CadAction::CadMeasure { a, b, keep } => json!({"command": "cad_measure", "a": a, "b": b, "keep": keep}),
        CadAction::CadCancel => json!({"command": "cad_cancel"}),
        CadAction::CadPatch { id, attrs } => json!({"command": "cad_patch", "id": id, "attrs": attrs}),
        CadAction::CadCommand { id } => json!({"command": "cad_command", "id": id}),
        CadAction::CadOp { name, args, kwargs } => json!({"command": "cad_op", "name": name, "args": args, "kwargs": kwargs}),
        CadAction::CadOpen { path, url } => json!({"command": "cad_open", "path": path, "url": url}),
        CadAction::CadInvoke { id } => json!({"command": "cad_invoke", "id": id}),
        CadAction::CadRun { id, params, items, revision } => json!({"command": "cad_run", "id": id, "params": params, "items": items, "revision": revision}),
        CadAction::CadFormSet { name, value } => json!({"command": "cad_form_set", "name": name, "value": value}),
        CadAction::CadFormSubmit => json!({"command": "cad_form_submit"}),
        CadAction::CadFormCancel => json!({"command": "cad_form_cancel"}),
        CadAction::CadSurface { surface } => json!({"command": "cad_surface", "surface": surface}),
        CadAction::State => json!({"command": "state"}),
        CadAction::CadState => json!({"command": "cad_state"}),
        CadAction::SystemUi(args) => json!({"command": "system_ui", "action": args.get("action")}),
    }
}

/// `system_ui`: the controls, or one activated through this handler (the
/// same action a click writes). Ids are stable names, so `ui_revision` is
/// reported but not required.
fn system_ui(call: &mut Call, cx: &mut Cx, args: &Map<String, Value>) -> Outcome {
    let action = args.get("action").cloned().unwrap_or(Value::Null);
    match action["operation"].as_str() {
        Some("controls") => {
            let items: Vec<Value> = controls(cx.doc)
                .into_iter()
                .map(|(id, label, action, ready)| json!({"id": id, "label": label, "enabled": ready.is_ok(), "disabled_reason": ready.err(), "action": rest_form(&action)}))
                .collect();
            Outcome::Done(Ok(json!({"ui_revision": cx.doc.revision, "ready": true, "controls": items, "state": state_json(cx.doc, cx.meshes.as_deref())})))
        }
        Some("activate") => {
            let Some(id) = action["id"].as_str() else { return Outcome::Done(Err("system_ui activate needs an id; request controls".into())) };
            let found = controls(cx.doc).into_iter().find(|(i, ..)| i == id);
            match found {
                None => Outcome::Done(Err(format!("unknown control {id}; request controls"))),
                Some((id, _, _, Err(why))) => Outcome::Done(Err(format!("{id} is disabled: {why}"))),
                Some((_, _, action, Ok(()))) => handle(&action, call, cx),
            }
        }
        _ => Outcome::Done(Err("system_ui in CAD mode: operation controls, or activate with a control id (cad:* or mode:*)".into())),
    }
}

/// `cad_state`: the document as this window shows it. Nothing is invented:
/// absent values are null.
pub(super) fn state_json(doc: &CadDocument, meshes: Option<&CadMeshes>) -> Value {
    let connection = match &doc.connection {
        Connection::Connecting { what, since } => json!({"state": "connecting", "what": what, "seconds": since.elapsed().as_secs()}),
        Connection::Connected => json!({"state": "connected"}),
        Connection::Lost { error, since } => json!({"state": "lost", "error": error, "seconds": since.elapsed().as_secs()}),
    };
    let nodes: Vec<Value> = match &doc.doc {
        Some(state) => state
            .nodes
            .iter()
            .zip(doc.rows())
            .map(|(n, row)| json!({"id": n.id, "kind": n.kind, "name": n.name, "parent": n.parent, "depth": row.depth, "visible": n.visible, "effective_visible": n.effective_visible, "locked": n.locked, "disabled": n.disabled}))
            .collect(),
        None => Vec::new(),
    };
    let result = |r: &Result<Value, String>| match r {
        Ok(v) => json!({"ok": true, "value": v}),
        Err(e) => json!({"ok": false, "error": e}),
    };
    let meshes = meshes.map(|m| {
        json!({"shown": m.counts.shown, "pending": m.counts.pending, "no_mesh": m.counts.no_mesh,
            "failed": m.counts.failed.iter().map(|(id, e)| json!({"id": id, "error": e})).collect::<Vec<_>>()})
    });
    json!({
        "target": doc.target.json(),
        "document": doc.document_name(),
        "service": {
            "kind": if doc.child.is_some() || matches!(doc.target, CadTarget::File(_)) { "self-started" } else { "attached" },
            "url": doc.url(),
            "pid": doc.child.pid(),
            "exited": doc.child_exit,
            "line": doc.service_line(),
        },
        "connection": connection,
        "health": doc.health.as_ref().map(value),
        "unsaved": doc.unsaved(),
        "document_key": doc.doc_key.as_ref().map(|(id, revision)| json!({"document_id": id, "revision": revision})),
        "stale": doc.stale,
        "nodes": nodes,
        "selection": doc.selection,
        "select_mode": doc.select_mode,
        "hover": doc.hover,
        "candidates": doc.candidates.as_ref().map(|c| json!({"items": c.items, "extend": c.extend, "toggle": c.toggle})),
        "selection_error": doc.selection_error,
        "tool": doc.tool,
        "tool_state": super::transform::state_json(doc),
        "inspected": doc.detail.as_ref().map(|(id, revision, r)| json!({"id": id, "revision": revision, "detail": result(&r.as_ref().map(value).map_err(Clone::clone))})),
        "physical": doc.physical.as_ref().map(|(revision, r)| json!({"revision": revision, "result": result(r)})).unwrap_or(Value::Null),
        "physical_pending": doc.physical_job.is_some(),
        "history": doc.doc.as_ref().map(|d| value(&d.history)),
        "commands": doc.commands.as_ref().map(|c| result(&c.as_ref().map(value).map_err(Clone::clone))),
        "autosave": doc.autosave.as_ref().map(|a| result(&a.as_ref().map(value).map_err(Clone::clone))),
        "edit": doc.edit.as_ref().map(|e| json!({"label": e.label, "seconds": e.started.elapsed().as_secs()})),
        "meshes": meshes,
        "status": doc.status.as_ref().map(|s| match s { Ok(t) => json!({"ok": true, "text": t}), Err(e) => json!({"ok": false, "text": e}) }),
        "revision": doc.revision,
        "generation": doc.generation,
    })
}

/// Present: `/v1/state` (with `viewer_mode`) and `/v1/cad_state`, at most every 100 ms.
pub(super) fn publish(rest: Option<ResMut<crate::rest::Rest>>, doc: Option<Res<CadDocument>>, meshes: Option<Res<CadMeshes>>) {
    let (Some(mut rest), Some(doc)) = (rest, doc) else { return };
    if rest.0.snapshot_due() {
        let state = state_json(&doc, meshes.as_deref());
        let mut shown = state.clone();
        shown["viewer_mode"] = json!(ViewerMode::Cad.name());
        rest.0.publish("cad_state", state);
        rest.0.publish("state", shown);
    }
}
