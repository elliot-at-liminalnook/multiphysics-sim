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
pub(crate) enum CadAction {
    /// Internal window occurrence; never a public REST command.
    #[serde(skip)]
    Captured { source: super::activation::SourceStamp, action: Box<CadAction> },
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
    /// Inspect fresh source/history after an ambiguous edit; explicit acknowledgment never retries it.
    CadReconcileEdit { #[serde(default)] acknowledge: bool, #[serde(default)] revision: Option<u64> },
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
    /// cad-print: RoboCAD's print jobs (the jobs section, cancel), the
    /// fastener tool's picks and the wall check's points (`print`); the
    /// print tools and studies are catalogue entries.
    CadPrint(super::print::PrintArgs),
    /// cad-organize: the outliner's search, expand and collapse, multi-select,
    /// inline rename, drag-and-drop, context menu, active group and New
    /// group (`tree`); each edit is one RoboCAD call through `edit_at`.
    CadTree(super::tree::TreeArgs),
    /// cad-organize: RoboCAD's comment threads (`threads`), the fourth
    /// `annotations::ThreadSource`: the Comments dock, Annotate, pins, part
    /// links, Show on model, Fit in view and the temporary isolation.
    CadThreads(super::threads::ThreadsArgs),
    /// cad-organize: the References dock, reference image planes, the
    /// calibrate tool and the linked system file with Open in builder
    /// (`references`).
    CadReferences(super::references::ReferencesArgs),
    CadComponents(super::components::ComponentsArgs),
    CadComposition(super::composition::CadCompositionArgs),
    CadExperiments(super::experiments::ExperimentsArgs),
    CadExperimentReview(super::experiment_review::ReviewArgs),
    CadMotion(super::motion::MotionArgs),
    /// `system_ui` in CAD mode: `{action: {operation: controls | activate, id?, ui_revision?}}`.
    SystemUi(Map<String, Value>),
    /// Direct modelling in process (`model`): primitives, extrude, booleans,
    /// fillet, chamfer, move, groups, and the topology listing.
    CadModel(super::model::ModelArgs),
    /// How the editor works, for an agent starting cold (`guide`); also `GET /v1/cad_guide`.
    CadGuide {
        #[serde(default)]
        topic: Option<String>,
    },
}

// ---- The one handler -------------------------------------------------------

use super::document::{CadDocument, EditDone};
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
pub(super) use super::edit::{edit, edit_at, edit_auxiliary_at};
pub(crate) use super::edit::local_edit_at;
use std::collections::HashSet;

/// Actions: CAD mode's one apply system. A click's or key's refusal is the
/// header's status line; a REST caller gets it, or the answer. Mutations
/// run on an edit job; their REST callers wait for RoboCAD's answer
/// (Pending with the edit's sequence in the continuation).
#[allow(clippy::too_many_arguments)]
pub(super) fn apply(
    mut settings: ResMut<crate::app::settings::SettingsOwner>,
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
    (mut components, mut composition): (ResMut<super::components::ComponentsState>, ResMut<super::composition::CadCompositionState>),
    (mut experiments, mut review, mut motion): (ResMut<super::experiments::ExperimentsState>, ResMut<super::experiment_review::ReviewState>, ResMut<super::motion::MotionState>),
) {
    let Some(mut doc) = doc else {
        actions::apply(&mut messages, &mut in_flight, &mut replies, |_, _| Outcome::Done(Err("CAD mode has no document open".into())));
        return;
    };
    let preview = review.active || motion.active;
    if doc.preview_read_only != preview { doc.preview_read_only = preview; doc.touch(); }
    // Read first: a `ResMut` deref would mark the registry changed every frame.
    if super::selection::cad_id(&registry).is_none() {
        super::selection::ensure_registered(&mut registry, &mut selection, &doc.target);
    }
    let mut camera: Vec<crate::camera::CameraAction> = Vec::new();
    actions::apply(&mut messages, &mut in_flight, &mut replies, |action, call| {
        let mut cx = Cx {
            settings: &mut settings,
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
            components: &mut *components,
            composition: &mut *composition,
            experiments: &mut *experiments, review: &mut *review, motion: &mut *motion,
            camera: Vec::new(),
        };
        cx.doc.preview_read_only = cx.review.active || cx.motion.active;
        let outcome = handle(action, call, &mut cx);
        cx.doc.preview_read_only = cx.review.active || cx.motion.active;
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
pub(crate) struct Cx<'a> {
    pub settings: &'a mut crate::app::settings::SettingsOwner,
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
    pub components: &'a mut super::components::ComponentsState,
    pub composition: &'a mut super::composition::CadCompositionState,
    pub experiments: &'a mut super::experiments::ExperimentsState,
    pub review: &'a mut super::experiment_review::ReviewState,
    pub motion: &'a mut super::motion::MotionState,
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
    if matches!(action, CadAction::CadCancel) {
        if let Some(seq) = cx.doc.local_load.as_ref().map(|load| load.sequence) {
            sync::cancel_load(cx.doc, seq);
            return Outcome::Done(Ok(json!({"message": "Local open cancelled; current document preserved"})));
        }
        if cx.motion.active || cx.motion.export.is_some() {
            return super::motion::handle(&super::motion::MotionArgs::of(super::motion::MotionOp::Return), call, cx);
        }
        if cx.review.active {
            return super::experiment_review::handle(&super::experiment_review::ReviewArgs::of(super::experiment_review::ReviewOp::Return), call, cx);
        }
    }
    let done = |r: Result<Value, String>| Outcome::Done(r);
    let doc = &mut *cx.doc;
    if let CadAction::Captured { source, action } = action {
        if !super::activation::current(source, doc) || !super::activation::files_current(source, cx.files.as_deref()) { return done(Err("CAD control belongs to a replaced source or form".into())); }
        return handle(action, call, cx);
    }
    match action {
        CadAction::Captured { .. } => unreachable!("captured intent handled above"),
        CadAction::State | CadAction::CadState => done(Ok(state_json(doc, &cx.shared.items(), cx.meshes.as_deref(), Some(&*cx.plane), Parts::of(cx.display.as_deref(), cx.views.as_deref(), cx.files.as_deref()).authoring(cx.components, cx.composition).experiments(cx.experiments, cx.review, cx.motion).defaults(&cx.settings.cad)))),
        CadAction::CadOpen { path, url } => {
            cx.review.request_cancel();
            cx.experiments.request_cancel();
            cx.motion.request_cancel();
            let blockers = cx.experiments.mode_blockers().into_iter().chain(cx.motion.mode_blockers()).collect::<Vec<_>>();
            if !blockers.is_empty() { return done(Err(blockers.join("; "))); }
            open(doc, call, path.as_ref(), url.as_deref())
        },
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
            // A chip, name or transform field clicked in the window computed
            // its value from the shown tree: applied only at that revision.
            let began = (!call.rest()).then(|| doc.shown_revision());
            local_edit_at(doc, call, began, format!("Patch {name}: {keys}"), false, move |ws| {
                let changed = sim_cad::nodes::patch(&mut ws.edit, &id, &attrs)?;
                Ok(EditDone { message: format!("Patched {name}: {}", changed.join(", ")), result: json!({"id": id, "changed": changed}) })
            })
        }
        CadAction::CadDelete { id } => {
            if !doc.has_node(id) {
                return done(Err(format!("no node {id} in the shown tree")));
            }
            let name = doc.node_name(id);
            let id = id.clone();
            local_edit_at(doc, call, None, format!("Delete {name}"), false, move |ws| {
                let deleted = sim_cad::nodes::delete(&mut ws.edit, std::slice::from_ref(&id))?;
                Ok(EditDone { message: format!("Deleted {name}"), result: json!({"deleted": deleted}) })
            })
        }
        CadAction::CadUndo | CadAction::CadRedo => {
            let redo = matches!(action, CadAction::CadRedo);
            match super::local::step(doc, redo) {
                Err(e) => done(Err(e)),
                Ok(label) => {
                    if let Some(tree) = doc.doc.clone() {
                        super::selection::follow_tree(&mut cx.shared, tree.revision, &tree, true);
                    }
                    let message = match (&label, redo) {
                        (None, false) => "Nothing to undo".to_string(),
                        (None, true) => "Nothing to redo".to_string(),
                        (Some(l), false) => format!("Undid {l}"),
                        (Some(l), true) => format!("Redid {l}"),
                    };
                    let doc = &mut *cx.doc;
                    doc.show(Ok(message.clone()));
                    done(Ok(json!({"message": message, if redo { "redone" } else { "undone" }: label, "history": doc.history.labels(), "revision": doc.shown_revision(), "unsaved": doc.unsaved()})))
                }
            }
        }
        // POST /save/thumbnail, as RoboCAD's desktop saves (`files::save`).
        // A path is absolute (~/ expanded): RoboCAD would resolve a relative
        // one against its own working directory, and the document follows it.
        CadAction::CadSave { path } => match path.as_deref().map(|p| super::files::absolute(p, "cad_save")).transpose() {
            Ok(path) => done(super::local::save(doc, path.map(PathBuf::from))),
            Err(e) => done(Err(e)),
        },
        CadAction::CadModel(args) => super::model::handle(args, call, cx),
        CadAction::CadGuide { topic } => done(super::guide::guide(topic.as_deref())),
        CadAction::CadCommand { id } => {
            if let Some(action) = super::surfaces::registry::organize_action(id) {
                return handle(&action, call, cx);
            }
            let id = id.clone();
            edit(doc, call, format!("Command {id}"), move |c| c.run_command(&id).map(|r| EditDone { message: format!("Ran RoboCAD command {}", r.ran), result: value(&r) }))
        }
        CadAction::CadOp { name, args, kwargs } => {
            if ["create_component_family", "link_component_family", "create_component", "make_component", "new_parametric_component", "place_component", "set_component_parameters", "set_component_overrides", "detach_component", "import_component", "export_component", "transform_components"].contains(&name.as_str()) {
                return done(Err(format!("{name}: use the typed cad_components operation so rebuild progress and cancellation remain tracked")));
            }
            if name == "set_component_graph" {
                return done(Err("set_component_graph: use cad_composition so source identities and typed connections are validated".into()));
            }
            let (name, args, kwargs) = (name.clone(), args.clone(), kwargs.clone());
            edit(doc, call, format!("Op {name}"), move |c| c.op(&name, &args, &kwargs).map(|r| EditDone { message: format!("Ran {name}"), result: value(&r) }))
        }
        CadAction::CadInvoke { .. } | CadAction::CadRun { .. } | CadAction::CadFormSet { .. } | CadAction::CadFormSubmit | CadAction::CadFormCancel | CadAction::CadSketch { .. } => super::ops::handle(action, call, cx),
        CadAction::CadSurface { .. } => super::surfaces::handle(action, call, cx),
        CadAction::CadRefresh => {
            let blockers = doc.switch_blockers();
            if !blockers.is_empty() { done(Err(blockers.join("; "))) } else { done(Ok(refresh(doc))) }
        },
        CadAction::CadReconcileEdit { acknowledge, revision } => {
            if !acknowledge { return done(Ok(refresh(doc))); }
            if *revision != Some(doc.shown_revision()) || doc.dirty_known_at.is_some() || doc.stale.is_some() || !doc.connected() {
                return done(Err("Inspect a fresh document and history before acknowledging the unknown edit outcome".into()));
            }
            match doc.uncertain_edit.take() {
                Some(error) => {
                    doc.uncertain_history.push(json!({"error":error,"acknowledged_revision":revision,"note":"Explicit acknowledgment of an unknown outcome; no request retried"}));
                    doc.show(Ok("Unknown source outcome acknowledged after inspection; no request retried".into()));
                    done(Ok(json!({"acknowledged":true,"retried":false})))
                }
                None => done(Err("No unknown source edit outcome is pending".into())),
            }
        },
        CadAction::CadFit { id } => done(fit(doc, cx.meshes.as_deref_mut(), id.as_deref())),
        CadAction::CadPhysical => done(sync::fetch_physical(doc).map(|()| json!({"message": "Local exact mass properties are available in cad_state.local_mass."}))),
        CadAction::CadDisplay(_) | CadAction::CadSection(_) => super::display::handle(action, call, cx),
        CadAction::CadViews(_) => super::views::handle(action, call, cx),
        CadAction::CadFile(_) | CadAction::CadExport(_) | CadAction::CadRender(_) => super::files::handle(action, call, cx),
        CadAction::CadRobot(_) => super::robot::handle(action, call, cx),
        CadAction::CadMaterials(_) => super::materials::handle(action, call, cx),
        CadAction::CadInspector(_) => super::inspector::handle_physical(action, call, cx),
        CadAction::CadResults(_) => super::results::handle(action, call, cx),
        CadAction::CadPrint(_) => super::print::handle(action, call, cx),
        CadAction::CadTree(_) => super::tree::handle(action, call, cx),
        CadAction::CadThreads(_) => super::threads::handle(action, call, cx),
        CadAction::CadReferences(_) => super::references::handle(action, call, cx),
        CadAction::CadComponents(args) => super::components::handle(args, call, cx),
        CadAction::CadComposition(args) => super::composition::handle(args, call, cx),
        CadAction::CadExperiments(args) => super::experiments::handle(args, call, cx),
        CadAction::CadExperimentReview(args) => super::experiment_review::handle(args, call, cx),
        CadAction::CadMotion(args) => super::motion::handle(args, call, cx),
        CadAction::SystemUi(args) => super::ui_api::system_ui(call, cx, args),
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
fn open(doc: &mut CadDocument, call: &mut Call, path: Option<&PathBuf>, url: Option<&str>) -> Outcome {
    if let Some(seq) = call.continuation.get("local_open").and_then(Value::as_u64) {
        if call.cancelled { sync::cancel_load(doc, seq); }
        if let Some(result) = doc.load_outcomes.remove(&seq) { return Outcome::Done(result); }
        if doc.local_load.as_ref().is_none_or(|load| load.sequence != seq) {
            return Outcome::Done(Err("Local CAD open was superseded or cancelled; current document preserved".into()));
        }
        return Outcome::Pending;
    }
    if url.is_some() { return Outcome::Done(Err("CAD service attachment awaiting Rust migration; open a local .rcad file".into())); }
    let Some(path) = path else { return Outcome::Done(Err("cad_open needs path to a .rcad archive".into())); };
    if path.extension().is_none_or(|e| e != "rcad") { return Outcome::Done(Err(format!("{}: expected .rcad archive", path.display()))); }
    let blockers = doc.switch_blockers();
    if !blockers.is_empty() { return Outcome::Done(Err(blockers.join("; "))); }
    let seq = sync::request_load(doc, path.clone());
    if call.rest() { *call.continuation = json!({"local_open": seq}); Outcome::Pending }
    else { Outcome::Done(Ok(json!({"loading": path, "request": seq, "message": "Loading archive locally; current document retained until success"}))) }
}

fn refresh(doc: &mut CadDocument) -> Value {
    sync::start(doc);
    json!({"message": "Reload requested locally; current document retained until success", "request": doc.load_sequence})
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
    Ok(json!({"framed": id.map_or_else(|| "every drawn body".to_string(), |id| doc.node_name(id)), "note": "display only: source geometry is unchanged"}))
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
