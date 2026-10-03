//! CAD mode (native-viewer.md §9 phase 1): a RoboCAD document shown and
//! edited in the native viewer through RoboCAD's REST service
//! (`sim_runtime::cad_client`). RoboCAD's kernel and command layer own the
//! document; this mode shows its tree, inspector and tessellations and sends
//! every edit to RoboCAD's routes, so undo, provenance and the `.rcad` file
//! stay RoboCAD's.
//!
//! - [`document`]: the [`CadDocument`] resource (target, client, connection,
//!   snapshot, work in flight) and the helpers the panels read.
//! - [`sync`]: the connect job (a self-started service is a
//!   `jobs::ChildProcess`), the poll `RunThread` and every result (JobResults).
//! - [`mesh`]: tessellations fetched and built on jobs, drawn and picked.
//! - [`scene`]: cameras (on the shared `crate::camera`), light, the Z-up
//!   root, the framed bounds and the camera's gesture gates.
//! - [`actions`]: [`CadAction`] and its one handler (Actions), `system_ui`,
//!   `cad_state` and the REST snapshot (Present).
//! - [`keys`]: RoboCAD's shortcuts (Input).
//! - `panel`, `tree`, `inspector`: the header, model tree and inspector.
//! - [`view`]: the camera as plain matrices (projection and cursor rays in
//!   RoboCAD's model frame), [`topology`]: faces, edges (sampled polylines)
//!   and vertices by (node, revision).
//! - `selection`, `pick`, `overlay`: sub-body selection (modes, hover, box
//!   select, the Alt menu, the selection commands), picking and the
//!   display-only highlights (cad-select-transform, native-viewer.md "CAD
//!   selection and transform").
//! - `transform`, `numeric`, `snap`, `measure`: the tools (gizmo move,
//!   rotate and scale; push/pull and offset; measure), the numeric bar
//!   with unit expressions, snapping and live dimensions. Previews are
//!   display only; each commit is one RoboCAD Ops call through
//!   `actions::edit`.
//!
//! - `robot`, `materials`, `results` and the inspector's physical rows
//!   (cad-physical-inspect, native-viewer.md "CAD physical properties"):
//!   the Robot panel and robot tools and glyphs over `GET /robot`, the
//!   materials panel, joint and material edits, results with the stress
//!   overlay, physical export and the live link to Robot mode. Every edit
//!   is one RoboCAD call through `actions::edit_at`.
//! - `print` (cad-print, native-viewer.md "CAD print"): RoboCAD's Print
//!   menu: wall check, validate, overhang shading, fastener hole and
//!   clearance, the print studies, the one poller of RoboCAD's print jobs,
//!   the Print jobs section and the print overlay.
//! - `tree`, `threads`, `references` (cad-organize, native-viewer.md "CAD
//!   organize"): the outliner's organization (search, collapse, multi-select,
//!   rename, drag-and-drop, the context menu, the active group, New group);
//!   RoboCAD's comment threads as the fourth `annotations::ThreadSource`
//!   drawn by the one `ui_kit::threads` panel (Annotate, pins, part links,
//!   Show on model, Fit in view, temporary isolation); the References dock
//!   with textured image planes and the calibrate tool, and the linked
//!   system file with Open in builder (an in-window switch to Build mode).
//!   Every edit is one RoboCAD call through `actions::edit_at`.
//!
//! Teardown is the one-app pattern: `app::switch`'s OnExit(ModeScope::Cad)
//! removes the document, releases its self-started service at once
//! (`CadDocument::release_child`: stopped, also while still starting, or
//! left running when it may hold unsaved edits), drops the rest off the UI
//! thread (the poll joins) and [`clear`]s the other resources; entities go
//! by `DespawnOnExit`.
mod actions;
mod edit;
mod ui_api;
pub(crate) mod experiments;
pub(crate) mod experiment_review;
pub(crate) mod motion;
mod analysis_overlay;
mod attach;
pub(crate) mod components;
pub(crate) mod composition;
mod display;
mod document;
mod files;
mod inspector;
mod keys;
mod materials;
mod measure;
mod mesh;
mod numeric;
mod ops;
mod overlay;
mod panel;
mod pick;
mod print;
mod references;
mod rest_form;
mod results;
mod robot;
mod scene;
mod selection;
mod sketch;
mod snapshot;
mod snap;
mod specs;
mod surfaces;
mod sync;
pub(crate) mod threads;
mod topology;
mod transform;
mod tree;
mod view;
mod views;
#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod tests;

pub(crate) use actions::CadAction;
pub use actions::{Dimension, MeasurePick};
pub use document::{CadDocument, CadTarget, CadTool, Candidates, ChildSlot, Connection, Edit, EditDone, PollCommand, PollSnapshot, SelectMode, TreeRow};
pub use mesh::{BODY_KINDS, CadBody, CadMeshes, MeshCounts};
pub use ops::{FormState, OpsState};
pub use sketch::{ActivePlane, BasePlane, CadActivePlane, CadSketches};
pub(crate) use sketch::blocker as sketch_blocker;
pub use surfaces::Surface;
pub use topology::{CadTopology, NodeTopology};
pub use view::CadView;

use crate::app::{InputSet, ModeScope, ViewerMode, ViewerSet};
use bevy::prelude::*;

/// CAD's public ordering points (native-viewer.md "Public system sets"):
/// CAD's own systems, and any other feature, order against these instead of
/// CAD's functions. Configured once, in [`CadCorePlugin`].
#[derive(SystemSet, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CadSet {
    /// JobResults: the service's answers land (`sync::receive`). A job's
    /// own results that must see this frame's document run after it.
    Results,
    /// SimSync: the bodies' meshes are synced to the document (`mesh::sync`).
    Mesh,
    /// SimSync: each body's own material and highlight (`mesh::highlight`).
    Highlight,
    /// SimSync: the active sketch plane follows the plane frames (`sketch::plane::sync`).
    Plane,
    /// SimSync, after `CameraSet::Place`: this frame's view snapshot (`view::update`).
    View,
}

/// CAD's key arbitration in Input's window step (`keys.rs`, the module doc's
/// key table). Only the relations that held as function edges are
/// configured: the two-step key gate comes before RoboCAD's shortcuts and the
/// Select tool's keys, and every field-focusing reader before the shortcuts.
/// `Focus` is not ordered against `Gate` (readers that must follow the gate
/// say so), and `Keys` is not ordered against `ToolKeys`.
#[derive(SystemSet, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CadKeySet {
    /// The two-step key gate (`keys::gate`). Readers that take a key first
    /// (an open popup's Escape, the docks' typing) run before it.
    Gate,
    /// Readers that give a field the keyboard, or consume a key, this frame:
    /// before RoboCAD's shortcuts, which then stand aside.
    Focus,
    /// RoboCAD's shortcuts (`keys::keys`).
    Keys,
    /// The Select tool's keys (`transform::keys`).
    ToolKeys,
}

/// [`CadSet`] and [`CadKeySet`] in the pipeline (CadCorePlugin, and the
/// windowless schedule test).
pub(crate) fn configure_sets(app: &mut App) {
    app.configure_sets(Update, CadSet::Results.in_set(ViewerSet::JobResults))
        .configure_sets(Update, (CadSet::Mesh, CadSet::Highlight, CadSet::Plane, CadSet::View).in_set(ViewerSet::SimSync))
        .configure_sets(Update, (CadKeySet::Gate, CadKeySet::Focus, CadKeySet::Keys, CadKeySet::ToolKeys).in_set(InputSet::Window))
        .configure_sets(Update, CadKeySet::Gate.before(CadKeySet::Keys).before(CadKeySet::ToolKeys))
        .configure_sets(Update, CadKeySet::Focus.before(CadKeySet::Keys));
}

/// What CAD mode needs without a window (the switch test runs it with
/// `ModesPlugin` on MinimalPlugins + StatesPlugin): the action, its
/// handler, the connection and its results, the mesh cache's lifetime and
/// the REST snapshot. Entering CAD mode starts the connect job
/// (`sync::enter`); `sync::receive` turns its answer into
/// `CadDocument::connection` (`Connection::Lost { error, .. }` for a refused
/// connection); leaving removes `CadDocument` and `CadMeshes`.
pub struct CadCorePlugin;
impl Plugin for CadCorePlugin {
    fn build(&self, app: &mut App) {
        crate::app::actions::register::<CadAction>(app);
        configure_sets(app);
        app.init_resource::<crate::app::settings::SettingsOwner>()
            .init_resource::<CadActivePlane>()
            .add_systems(
                OnEnter(ModeScope::Cad),
                (sync::enter, |mut commands: Commands| {
                    commands.insert_resource(CadMeshes::default());
                    commands.insert_resource(CadTopology::default());
                    commands.insert_resource(CadSketches::default());
                    commands.insert_resource(CadActivePlane::default());
                }),
            )
            .add_systems(
                Update,
                (
                    actions::apply.in_set(ViewerSet::Actions),
                    sync::receive.in_set(CadSet::Results),
                    // A read's result (copy, control points, comb, continuity), with or without a window.
                    analysis_overlay::receive.in_set(ViewerSet::JobResults).after(CadSet::Results),
                    topology::sync.in_set(ViewerSet::SimSync),
                    // Sketch geometry and plane frames, then the active plane follows them (cad-sketch).
                    (sketch::cache::sync.in_set(ViewerSet::SimSync), sketch::plane::sync.in_set(CadSet::Plane)).chain(),
                    snapshot::publish.in_set(ViewerSet::Present),
                )
                    .run_if(in_state(ViewerMode::Cad)),
            )
            // cad-physical-inspect, windowless parts: the robot description's
            // reads, the exact measurement and the results and export jobs.
            .add_plugins((robot::CoreParts, inspector::CoreParts, results::CoreParts, print::CoreParts))
            // cad-organize, windowless parts: the outliner's state, the threads' and
            // references' reads and the system link's status on jobs, Open in builder's switch.
            .add_plugins((tree::CoreParts, threads::CoreParts, references::CoreParts))
            .add_plugins((components::CoreParts, composition::CoreParts))
            .add_plugins((experiments::CoreParts, experiment_review::CoreParts, motion::CoreParts))
            // At window close, a self-started service is stopped (or left running with unsaved edits).
            // After ExitSystems, which writes AppExit in Last: before it, the
            // message would be read only on a frame that never comes, and the
            // dropped World would kill a dirty service instead of detaching it.
            .add_systems(Last, sync::on_exit.after(bevy::window::ExitSystems));
    }
}

/// CAD mode in the window: the core, the 3D view, the meshes, the keys and
/// the panels.
mod activation;

pub struct CadPlugin;
impl Plugin for CadPlugin {
    fn build(&self, app: &mut App) {
        activation::install(app);
        app.add_plugins(CadCorePlugin)
            .add_systems(OnEnter(ModeScope::Cad), (scene::setup, |mut commands: Commands| commands.insert_resource(CadView::default())))
            .add_systems(OnExit(ModeScope::Cad), scene::teardown)
            .add_systems(
                Update,
                (
                    // Not while a text field has the keyboard (`keys::keys` reads `Typing`); after
                    // the two-step key gate, which gives a pending chord the frame's key.
                    keys::keys.in_set(CadKeySet::Keys),
                    // The shared camera (`crate::camera`) navigates, sets the viewport and places
                    // the view in its sets: the bounds, a node fit and the gesture gates go in
                    // before the place step (`scene::fit`), and the snapshot is taken after it.
                    (mesh::sync.in_set(CadSet::Mesh), mesh::highlight.in_set(CadSet::Highlight), scene::fit.in_set(ViewerSet::SimSync)).chain().before(crate::camera::CameraSet::Place),
                    view::update.after(crate::camera::CameraSet::Place).in_set(CadSet::View),
                )
                    .run_if(in_state(ViewerMode::Cad)),
            );
        panel::build(app);
        components::build(app);
        composition::build(app);
        experiments::build(app);
        experiment_review::build(app);
        motion::build(app);
        // Sub-body selection (picking, hover, box select, the Alt menu, mode
        // buttons, overlays) and the tools (gizmo, push/pull, offset, measure,
        // numeric bar, snapping): each registers its own systems, all writing
        // `CadAction`s that `actions::apply` applies.
        selection::build(app);
        transform::build(app);
        // The op catalogue (cad-modify): its interactions (placing
        // primitives, pick-then-form tools, the cursor snap), the command
        // surfaces (toolbar, context menu, radials, palette, menus, the
        // parameter form) and the analysis overlays.
        ops::interact::build(app);
        surfaces::build(app);
        analysis_overlay::build(app);
        // cad-sketch: the plane tools and quads, the sketch tools, the
        // sketches drawn, extrude and revolve.
        sketch::build(app);
        // cad-views-export: the display state, grid, build plate, view cube
        // and section (display only); saved views over RoboCAD's /views;
        // new, open, save as, import, export and render on jobs.
        display::build(app);
        views::build(app);
        files::build(app);
        // window-first-usability: the attach-URL field of an unconnected document.
        attach::build(app);
        // cad-physical-inspect: the Robot panel, robot tools and glyphs, the
        // materials panel, the inspector's physical rows' input, the stress
        // overlay and the result, export and live-link forms.
        robot::build(app);
        materials::build(app);
        inspector::build_physical(app);
        results::build(app);
        // cad-print: the wall check's points, the fastener tool's clicks
        // and the Print jobs section's input.
        print::build(app);
        // cad-organize: the outliner's search, rename and drag input and its
        // context menu; the Comments dock's typing, Annotate's and Reattach's
        // face clicks and the pins; the References dock's typing, dropped
        // files, the calibrate tool's clicks and the image planes.
        tree::build(app);
        threads::build(app);
        references::build(app);
    }
}

/// OnExit(ModeScope::Cad), after the document is taken (`app::switch`):
/// CAD mode's other resources.
pub(crate) fn clear(world: &mut World) {
    world.remove_resource::<CadMeshes>();
    world.remove_resource::<CadTopology>();
    world.remove_resource::<CadSketches>();
    if let Some(mut plane) = world.get_resource_mut::<CadActivePlane>() {
        *plane = CadActivePlane::default();
    }
    world.remove_resource::<CadView>();
    world.remove_resource::<display::CadDisplay>();
    world.remove_resource::<views::CadViews>();
    world.remove_resource::<files::CadFiles>();
    world.remove_resource::<mesh::CadMaterials>();
}
