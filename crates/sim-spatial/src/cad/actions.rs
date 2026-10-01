//! CAD mode's actions (native-viewer.md §3, §9 phase 1). Every intent in CAD
//! mode is a [`CadAction`]: the tree's rows, 3D picks, the panel's buttons,
//! keys (`keys`), `system_ui` and REST all write `Act<CadAction>`, and
//! [`apply`] (ViewerSet::Actions) is the one handler. Mutations go to
//! RoboCAD's command layer through its REST routes on jobs, so RoboCAD's
//! undo, provenance and `.rcad` file stay its own.
use crate::app::actions;
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
        /// The shown revision a 3D pick was made at (the window's picks
        /// only; not a REST argument): its face, edge, vertex, point and
        /// curve items carry it into the shared selection, so a pick made
        /// against a tree that has since advanced is refused by name.
        #[serde(skip_deserializing)]
        picked_at: Option<u64>,
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
    /// `POST /save/thumbnail` (RoboCAD writes its own file with the
    /// desktop's thumbnail; `path`, absolute, saves as: `files::save`).
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
    /// Edit a sketch with RoboCAD's sketch calls (`[method, [args…],
    /// {kwargs}?]`, kernel/sketch.py's names; curves by index), checked by
    /// `cad_client::SketchCall` (a refusal names the call and the
    /// argument): one `POST /nodes/{node}/sketch`; without `node`, the
    /// sketch RoboCAD's sketch tools pick on `plane` ("xy" | "xz" | "yz" |
    /// a plane node id; the active plane when absent, else XY; tools.py:675-686:
    /// the selected sketch on that plane, else the first visible one), or one
    /// new sketch on it carrying the calls (`POST /nodes {"kind": "sketch"}`).
    /// The sketch tools' finished shapes are this action. `revision`:
    /// RoboCAD's revision the points and indices were read at; refused by
    /// name when it changed.
    CadSketch {
        #[serde(default)]
        node: Option<String>,
        #[serde(default)]
        plane: Option<String>,
        calls: Vec<Value>,
        #[serde(default)]
        revision: Option<u64>,
    },
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
    /// The display state (display mode, grid, build plate, view cube,
    /// high contrast, comment pins): display only, never a document edit
    /// (`display`).
    CadDisplay(super::display::DisplayArgs),
    /// The section tool: the live display-triangle preview and the exact
    /// section from `GET /nodes/{id}/section` on a job (`display::section`).
    CadSection(super::display::SectionArgs),
    /// Saved views through RoboCAD's `/views` in its view-state schema
    /// (list, save, rename, replace, delete, restore onto the native camera).
    CadViews(super::views::ViewsArgs),
    /// New, open, save as and import (with units); new and open under
    /// `cad_open`'s rule, so no edits are lost (`files`).
    CadFile(super::files::FileArgs),
    /// `POST /export` in any RoboCAD format, and the drawing, on a job.
    CadExport(super::files::ExportArgs),
    /// `GET /render` (headless) to a PNG file, on a job.
    CadRender(super::files::RenderArgs),
    /// cad-physical-inspect: the Robot panel (shown or hidden, refreshed),
    /// RoboCAD's "Robot: validate" and the motor library (`robot`).
    CadRobot(super::robot::RobotArgs),
    /// cad-physical-inspect: the materials panel (search, apply to the
    /// selection, a new material, engineering properties; `materials`).
    CadMaterials(super::materials::MaterialsArgs),
    /// cad-physical-inspect: the inspector's physical rows (colour, joint
    /// physics overrides, the exact measurement; `inspector::physical`).
    CadInspector(super::inspector::InspectorArgs),
    /// cad-physical-inspect: results and identification, the stress
    /// overlay, physical export and the live link (`results`).
    CadResults(super::results::ResultsArgs),
    /// `system_ui` in CAD mode: `{action: {operation: controls | activate, id?, ui_revision?}}`.
    SystemUi(Map<String, Value>),
}

// ---- The one handler -------------------------------------------------------

use super::document::{CadDocument, CadTarget, EditDone};
use super::mesh::CadMeshes;
use super::sketch::{CadActivePlane, CadSketches};
use super::sync::{self, value};
use super::topology::CadTopology;
use super::view::CadView;
use super::selection::Shared;
use super::snapshot::{Parts, state_json};
use crate::app::actions::{Act, Call, InFlight, Origin, Replies};
use crate::document::DocumentRegistry;
use crate::selection::Selection;
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
    mut plane: ResMut<CadActivePlane>,
    sketches: Option<Res<CadSketches>>,
    (mut display, mut views, mut files): (Option<ResMut<super::display::CadDisplay>>, Option<ResMut<super::views::CadViews>>, Option<ResMut<super::files::CadFiles>>),
    camera_out: Option<ResMut<Messages<Act<crate::camera::CameraAction>>>>,
    (mut selection, mut registry): (ResMut<Selection>, ResMut<DocumentRegistry>),
) {
    let Some(mut doc) = doc else {
        actions::apply(&mut messages, &mut in_flight, &mut replies, |_, _| Outcome::Done(Err("CAD mode has no document open".into())));
        return;
    };
    // Read first: a `ResMut` deref would mark the registry changed every frame.
    if super::selection::cad_id(&registry).is_none() {
        super::selection::ensure_registered(&mut registry, &mut selection, &doc.target);
    }
    let mut camera: Vec<crate::camera::CameraAction> = Vec::new();
    actions::apply(&mut messages, &mut in_flight, &mut replies, |action, call| {
        let mut cx = Cx {
            shared: Shared { selection: &mut *selection, registry: &mut *registry },
            doc: &mut *doc,
            meshes: meshes.as_deref_mut(),
            topology: topology.as_deref_mut(),
            view: view.as_deref(),
            plane: &mut *plane,
            sketches: sketches.as_deref(),
            display: display.as_deref_mut(),
            views: views.as_deref_mut(),
            files: files.as_deref_mut(),
            camera: Vec::new(),
        };
        let outcome = handle(action, call, &mut cx);
        camera.append(&mut cx.camera);
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
    // The camera intents CAD commands stood for (a named view, ortho, a saved view's restore).
    if let Some(mut out) = camera_out {
        for action in camera {
            out.write(Act::quiet(action));
        }
    }
}

/// What the one handler works on: the document and CAD mode's caches. The
/// selection arms (`selection::handle`) and the tool arms
/// (`transform::handle`) take the same context.
pub(super) struct Cx<'a> {
    pub doc: &'a mut CadDocument,
    /// The one selection and the document registry (CAD's items are the
    /// shared selection's; `selection::Shared`).
    pub shared: Shared<'a>,
    /// The drawn bodies (None without a window).
    pub meshes: Option<&'a mut CadMeshes>,
    /// Faces, edges and vertices of the shown bodies (None without CAD mode's caches).
    pub topology: Option<&'a mut CadTopology>,
    /// The camera as last drawn (None without a window).
    pub view: Option<&'a CadView>,
    /// The active plane and 2D snapping (cad-sketch; display state).
    pub plane: &'a mut CadActivePlane,
    /// Sketch geometry and plane frames (None without CAD mode's caches).
    pub sketches: Option<&'a CadSketches>,
    /// The display state (display mode, grid, section…; display only).
    pub display: Option<&'a mut super::display::CadDisplay>,
    /// Saved views as last listed, and their jobs.
    pub views: Option<&'a mut super::views::CadViews>,
    /// The file dialogs, exports and renders in flight.
    pub files: Option<&'a mut super::files::CadFiles>,
    /// Camera intents a CAD command stands for (a named view, ortho, a
    /// saved view's restore), written as `Act<CameraAction>` after the
    /// handler (the shared camera applies them).
    pub camera: Vec<crate::camera::CameraAction>,
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
        CadAction::State | CadAction::CadState => done(Ok(state_json(doc, &cx.shared.items(), cx.meshes.as_deref(), Some(&*cx.plane), Parts::of(cx.display.as_deref(), cx.views.as_deref(), cx.files.as_deref())))),
        CadAction::CadOpen { path, url } => done(open(doc, &mut cx.shared, path.as_ref(), url.as_deref())),
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
        // POST /save/thumbnail, as RoboCAD's desktop saves (`files::save`).
        // A path is absolute (~/ expanded): RoboCAD would resolve a relative
        // one against its own working directory, and the document follows it.
        CadAction::CadSave { path } => match path.as_deref().map(|p| super::files::absolute(p, "cad_save")).transpose() {
            Ok(path) => super::files::save(doc, call, path),
            Err(e) => done(Err(e)),
        },
        CadAction::CadCommand { id } => {
            let id = id.clone();
            edit(doc, call, format!("Command {id}"), move |c| c.run_command(&id).map(|r| EditDone { message: format!("Ran RoboCAD command {}", r.ran), result: value(&r) }))
        }
        CadAction::CadOp { name, args, kwargs } => {
            let (name, args, kwargs) = (name.clone(), args.clone(), kwargs.clone());
            edit(doc, call, format!("Op {name}"), move |c| c.op(&name, &args, &kwargs).map(|r| EditDone { message: format!("Ran {name}"), result: value(&r) }))
        }
        CadAction::CadInvoke { .. } | CadAction::CadRun { .. } | CadAction::CadFormSet { .. } | CadAction::CadFormSubmit | CadAction::CadFormCancel | CadAction::CadSketch { .. } => super::ops::handle(action, call, cx),
        CadAction::CadSurface { .. } => super::surfaces::handle(action, call, cx),
        CadAction::CadRefresh => done(Ok(refresh(doc))),
        CadAction::CadFit { id } => done(fit(doc, cx.meshes.as_deref_mut(), id.as_deref())),
        CadAction::CadPhysical => done(sync::fetch_physical(doc).map(|()| json!({"message": "Fetching RoboCAD's physical description (GET /physical?flex=0); it shows in cad_state.physical."}))),
        CadAction::CadDisplay(_) | CadAction::CadSection(_) => super::display::handle(action, call, cx),
        CadAction::CadViews(_) => super::views::handle(action, call, cx),
        CadAction::CadFile(_) | CadAction::CadExport(_) | CadAction::CadRender(_) => super::files::handle(action, call, cx),
        CadAction::CadRobot(_) => super::robot::handle(action, call, cx),
        CadAction::CadMaterials(_) => super::materials::handle(action, call, cx),
        CadAction::CadInspector(_) => super::inspector::handle_physical(action, call, cx),
        CadAction::CadResults(_) => super::results::handle(action, call, cx),
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

/// One edit through [`edit`], refused by name with nothing sent when
/// `CadDocument::commit_refusal(began)` names a reason: an edit in flight,
/// not connected, the shown document behind RoboCAD's, or RoboCAD's
/// revision changed since `began` (the revision a form opened, a row was
/// read or a pick was made at; None checks only the first three). The one
/// helper cad-physical-inspect's panels, forms and inspector rows commit through.
pub(super) fn edit_at(doc: &mut CadDocument, call: &mut Call, began: Option<u64>, label: String, work: impl FnOnce(&CadClient) -> Result<EditDone, sim_runtime::cad_client::CadError> + Send + 'static) -> Outcome {
    if let Some(why) = doc.commit_refusal(began) {
        return Outcome::Done(Err(why));
    }
    edit(doc, call, label, work)
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
fn open(doc: &mut CadDocument, shared: &mut Shared, path: Option<&PathBuf>, url: Option<&str>) -> Result<Value, String> {
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
    // The registry's CAD entry follows, and is what CAD mode reopens (a new
    // source gets a new id and the replaced document's items go; the same
    // source again keeps its id).
    super::selection::reopen(shared.registry, shared.selection, &target);
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
fn controls(cx: &Cx) -> Vec<(String, String, CadAction, Result<(), String>)> {
    let mut out: Vec<_> = super::panel::controls(cx.doc, &cx.shared.items()).into_iter().map(|c| (c.id, c.label, c.action, c.ready)).collect();
    // cad-views-export: cad:display:*, cad:section:*, cad:view:*, cad:file:*.
    out.extend(super::display::controls(cx));
    out.extend(super::views::controls(cx));
    out.extend(super::files::controls(cx));
    // cad-physical-inspect: cad:robot:*, cad:materials:*, cad:inspect:*, cad:results:*.
    out.extend(super::robot::controls(cx));
    out.extend(super::materials::controls(cx));
    out.extend(super::inspector::physical_controls(cx));
    out.extend(super::results::controls(cx));
    out
}

/// `system_ui`: the controls, or one activated through this handler (the
/// same action a click writes). Ids are stable names, so `ui_revision` is
/// reported but not required.
fn system_ui(call: &mut Call, cx: &mut Cx, args: &Map<String, Value>) -> Outcome {
    let action = args.get("action").cloned().unwrap_or(Value::Null);
    match action["operation"].as_str() {
        Some("controls") => {
            let items: Vec<Value> = controls(cx)
                .into_iter()
                .map(|(id, label, action, ready)| json!({"id": id, "label": label, "enabled": ready.is_ok(), "disabled_reason": ready.err(), "action": super::rest_form::rest_form(&action)}))
                .collect();
            Outcome::Done(Ok(json!({"ui_revision": cx.doc.revision, "ready": true, "controls": items, "state": state_json(cx.doc, &cx.shared.items(), cx.meshes.as_deref(), Some(&*cx.plane), Parts::of(cx.display.as_deref(), cx.views.as_deref(), cx.files.as_deref()))})))
        }
        Some("activate") => {
            let Some(id) = action["id"].as_str() else { return Outcome::Done(Err("system_ui activate needs an id; request controls".into())) };
            let found = controls(cx).into_iter().find(|(i, ..)| i == id);
            match found {
                None => Outcome::Done(Err(format!("unknown control {id}; request controls"))),
                Some((id, _, _, Err(why))) => Outcome::Done(Err(format!("{id} is disabled: {why}"))),
                Some((_, _, action, Ok(()))) => handle(&action, call, cx),
            }
        }
        _ => Outcome::Done(Err("system_ui in CAD mode: operation controls, or activate with a control id (cad:* or mode:*)".into())),
    }
}
