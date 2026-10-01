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
//! - [`scene`]: cameras, light, the Z-up root, orbit and framing.
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
//! Teardown is the one-app pattern: `app::switch`'s OnExit(ModeScope::Cad)
//! removes the document, releases its self-started service at once
//! (`CadDocument::release_child`: stopped, also while still starting, or
//! left running when it may hold unsaved edits), drops the rest off the UI
//! thread (the poll joins) and [`clear`]s the other resources; entities go
//! by `DespawnOnExit`.
mod actions;
mod analysis_overlay;
mod document;
mod inspector;
mod keys;
mod measure;
mod mesh;
mod numeric;
mod ops;
mod overlay;
mod panel;
mod pick;
mod scene;
mod selection;
mod snap;
mod surfaces;
mod sync;
mod topology;
mod transform;
mod tree;
mod view;
#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod tests;

pub use actions::{CadAction, Dimension, MeasurePick};
pub use document::{CadDocument, CadInputFocus, CadTarget, CadTool, Candidates, ChildSlot, Connection, Edit, EditDone, PollCommand, PollSnapshot, SelectMode, TreeRow};
pub use mesh::{BODY_KINDS, CadBody, CadMeshes, MeshCounts};
pub use ops::{FormState, OpsState};
pub use surfaces::Surface;
pub use topology::{CadTopology, NodeTopology};
pub use view::CadView;

use crate::app::{ModeScope, ViewerMode, ViewerSet};
use bevy::prelude::*;

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
        app.init_resource::<CadInputFocus>()
            .add_systems(
                OnEnter(ModeScope::Cad),
                (sync::enter, |mut commands: Commands| {
                    commands.insert_resource(CadMeshes::default());
                    commands.insert_resource(CadTopology::default());
                }),
            )
            .add_systems(
                Update,
                (
                    actions::apply.in_set(ViewerSet::Actions),
                    sync::receive.in_set(ViewerSet::JobResults),
                    topology::sync.in_set(ViewerSet::SimSync),
                    actions::publish.in_set(ViewerSet::Present),
                )
                    .run_if(in_state(ViewerMode::Cad)),
            )
            // At window close, a self-started service is stopped (or left running with unsaved edits).
            // After ExitSystems, which writes AppExit in Last: before it, the
            // message would be read only on a frame that never comes, and the
            // dropped World would kill a dirty service instead of detaching it.
            .add_systems(Last, sync::on_exit.after(bevy::window::ExitSystems));
    }
}

/// CAD mode in the window: the core, the 3D view, the meshes, the keys and
/// the panels.
pub struct CadPlugin;
impl Plugin for CadPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(CadCorePlugin)
            .add_systems(OnEnter(ModeScope::Cad), (scene::setup, |mut commands: Commands| commands.insert_resource(CadView::default())))
            .add_systems(OnExit(ModeScope::Cad), scene::teardown)
            .add_systems(
                Update,
                (
                    // After the name field, so a key that opens or ends it this frame is its own (`CadInputFocus`).
                    keys::keys.after(crate::app::actions::serve).after(panel::name_entry).in_set(ViewerSet::Input),
                    (mesh::sync, mesh::highlight, scene::fit, scene::orbit, scene::viewport, view::update).chain().in_set(ViewerSet::SimSync),
                )
                    .run_if(in_state(ViewerMode::Cad)),
            );
        panel::build(app);
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
    }
}

/// OnExit(ModeScope::Cad), after the document is taken (`app::switch`):
/// CAD mode's other resources.
pub(crate) fn clear(world: &mut World) {
    world.remove_resource::<CadMeshes>();
    world.remove_resource::<CadTopology>();
    world.remove_resource::<CadView>();
    world.remove_resource::<mesh::CadMaterials>();
    if let Some(mut focus) = world.get_resource_mut::<CadInputFocus>() {
        focus.0 = false;
    }
}
