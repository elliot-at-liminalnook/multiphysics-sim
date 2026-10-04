//! Native CAD opening/display/body selection/exact mass inspection are local
//! through sim-cad and direct OCCT. The retained modelling UI is migration
//! scaffolding: unsupported controls refuse by name and never attach a service.
//! Local jobs preserve the active source until an accepted replacement succeeds.
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
pub(crate) mod local;
mod local_client;
mod materials;
mod measure;
mod guide;
mod model;
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
/// say so), and `Keys` is not ordered against `ToolKeys`. Escape's consumers
/// after the gate are two public steps, so no feature orders against another
/// feature's Escape system: `Gate` → `EscapeTool` → `Escape` → `ToolKeys`
/// (`EscapeTool` also before `Keys`). Each consumer clears the key only when
/// it acted, so one press ends at most one thing.
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
    /// The numeric bar's entry (`numeric::entry`), inside [`CadKeySet::Focus`]:
    /// the surfaces' form Tab runs before it, the Select tool's keys and the
    /// 3D view's pick after it (its Tab may take the keyboard first).
    NumericEntry,
    /// Escape for a tool that holds the pointer (the reference calibrate
    /// tool, `references::calibrate::escape`): after the gate, before
    /// RoboCAD's shortcuts and before [`CadKeySet::Escape`].
    EscapeTool,
    /// Escape that ends a lighter state (Annotate or Reattach, the linked
    /// parts shown alone: `threads::input::escape`): after
    /// [`CadKeySet::EscapeTool`], before the Select tool's Escape
    /// (`ToolKeys`).
    Escape,
}

/// [`CadSet`] and [`CadKeySet`] in the pipeline (CadCorePlugin, and the
/// windowless schedule test).
pub(crate) fn configure_sets(app: &mut App) {
    app.configure_sets(Update, CadSet::Results.in_set(ViewerSet::JobResults))
        .configure_sets(Update, (CadSet::Mesh, CadSet::Highlight, CadSet::Plane, CadSet::View).in_set(ViewerSet::SimSync))
        .configure_sets(Update, (CadKeySet::Gate, CadKeySet::Focus, CadKeySet::Keys, CadKeySet::ToolKeys, CadKeySet::EscapeTool, CadKeySet::Escape).in_set(InputSet::Window))
        .configure_sets(Update, CadKeySet::Gate.before(CadKeySet::Keys).before(CadKeySet::ToolKeys))
        .configure_sets(Update, CadKeySet::Focus.before(CadKeySet::Keys))
        .configure_sets(Update, (CadKeySet::Gate, CadKeySet::EscapeTool, CadKeySet::Escape, CadKeySet::ToolKeys).chain())
        .configure_sets(Update, CadKeySet::EscapeTool.before(CadKeySet::Keys))
        .configure_sets(Update, CadKeySet::NumericEntry.in_set(CadKeySet::Focus));
}

/// Windowless local CAD ownership/actions/jobs/snapshot. OnEnter submits
/// typed CadOpen, CadSet::Results accepts its lifetime-checked result, and
/// mode exit cancels pending work before dropping the source off thread.
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
        // Service attachment is intentionally unavailable in local CAD mode.
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
