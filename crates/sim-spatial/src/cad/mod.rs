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
//!
//! Teardown is the one-app pattern: `app::switch`'s OnExit(ModeScope::Cad)
//! removes the document, releases its self-started service at once
//! (`CadDocument::release_child`: stopped, also while still starting, or
//! left running when it may hold unsaved edits), drops the rest off the UI
//! thread (the poll joins) and [`clear`]s the other resources; entities go
//! by `DespawnOnExit`.
mod actions;
mod document;
mod inspector;
mod keys;
mod mesh;
mod panel;
mod scene;
mod sync;
mod tree;
#[cfg(test)]
mod tests;

pub use actions::CadAction;
pub use document::{CadDocument, CadInputFocus, CadTarget, ChildSlot, Connection, Edit, EditDone, PollCommand, PollSnapshot, TreeRow};
pub use mesh::{BODY_KINDS, CadBody, CadMeshes, MeshCounts};

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
            .add_systems(OnEnter(ModeScope::Cad), (sync::enter, |mut commands: Commands| commands.insert_resource(CadMeshes::default())))
            .add_systems(
                Update,
                (
                    actions::apply.in_set(ViewerSet::Actions),
                    sync::receive.in_set(ViewerSet::JobResults),
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
            .add_systems(OnEnter(ModeScope::Cad), scene::setup)
            .add_systems(OnExit(ModeScope::Cad), scene::teardown)
            .add_systems(
                Update,
                (
                    // After the name field, so a key that opens or ends it this frame is its own (`CadInputFocus`).
                    keys::keys.after(crate::app::actions::serve).after(panel::name_entry).in_set(ViewerSet::Input),
                    (mesh::sync, mesh::highlight, scene::fit, scene::orbit, scene::viewport).chain().in_set(ViewerSet::SimSync),
                )
                    .run_if(in_state(ViewerMode::Cad)),
            );
        panel::build(app);
    }
}

/// OnExit(ModeScope::Cad), after the document is taken (`app::switch`):
/// CAD mode's other resources.
pub(crate) fn clear(world: &mut World) {
    world.remove_resource::<CadMeshes>();
    world.remove_resource::<mesh::CadMaterials>();
    if let Some(mut focus) = world.get_resource_mut::<CadInputFocus>() {
        focus.0 = false;
    }
}
