//! Phenomena mode (native-viewer.md "Fold in sim-app"): the gallery of
//! `sim_phenomena::exhibits`, formerly `sim-app`'s default scene
//! (`crates/sim-app/src/phenomena_app.rs`), as a mode of the one app.
//!
//! - [`run`]: the run thread (`jobs::RunThread` "phenomena-run") builds and
//!   owns every `Box<dyn Exhibit>` and advances the current one on its own
//!   clock with sim-app's rules (`run::Pacing::step`: real time clamped to
//!   0.05 s per tick, × the exhibit's `time_scale()` × the speed; a gridded
//!   exhibit takes whole grid steps and carries the remainder). It publishes
//!   generation-stamped frames (`jobs::Stamped`): shapes, readouts, knob,
//!   signal and its strip chart, verdict, time and the exhibit's error.
//! - [`gallery`]: `Gallery`, the mode's resource: the run thread, the
//!   generation the UI asked for (older frames are never applied), the
//!   shown frame, the controls and `phenomena_state`.
//! - [`actions`]: [`PhenomenaAction`] and its one handler (Actions), from
//!   keys, kit buttons, the knob slider, `system_ui` and REST; the REST
//!   snapshot (Present).
//! - [`keys`]: sim-app's bindings (Input). [`scene`]: cameras, light, the
//!   entity pool and gizmos, the shared orbit camera's spawn (its rules and
//!   view area). [`panel`]: the kit docks.
//! - **Teardown.** Entities go by `DespawnOnExit<ModeScope>`; [`leave`]
//!   (OnExit, registered by `app::switch`) removes the gallery, remembers its
//!   exhibit in phenomena's document registry entry and drops it off the UI thread (its run
//!   thread joins within `jobs::JOIN_BOUND` there), and removes the pool and
//!   panel state.
mod actions;
mod gallery;
mod keys;
mod panel;
mod run;
mod scene;
#[cfg(test)]
mod tests;

pub use actions::PhenomenaAction;
pub(crate) use gallery::Gallery;

use crate::document::DocumentRegistry;
use crate::app::{ModeScope, ViewerMode, ViewerSet};
use bevy::prelude::*;
use serde::Deserialize;

/// An exhibit as `--exhibit`, `phenomena_select` and the registry's exhibit source
/// name it: a 1-based number, or a title fragment (case-insensitive; the
/// first exhibit whose title contains it). sim-app's rule.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum ExhibitRef {
    Number(usize),
    Title(String),
}

impl ExhibitRef {
    /// `--exhibit` and `PHENOMENA_EXHIBIT`: digits are a number, anything else a title fragment.
    pub fn parse(text: &str) -> Self {
        match text.trim().parse::<usize>() {
            Ok(n) => ExhibitRef::Number(n),
            Err(_) => ExhibitRef::Title(text.to_string()),
        }
    }
    /// The 0-based index among `titles`, sim-app's rule: a number in
    /// 1..=len; otherwise (a number out of range too, or any other text) the
    /// first title containing the text, case-insensitive. A `Title` of digits
    /// is read as a number first. The error names what was wanted and how
    /// many exhibits there are (sim-app opened exhibit 1 silently).
    pub fn resolve(&self, titles: &[&str]) -> Result<usize, String> {
        let (number, text) = match self {
            ExhibitRef::Number(n) => (Some(*n), n.to_string()),
            ExhibitRef::Title(t) => (t.trim().parse::<usize>().ok(), t.clone()),
        };
        // Every title contains the empty string; an empty fragment is a mistake, not exhibit 1.
        if text.trim().is_empty() {
            return Err(format!("the exhibit is empty: name a number from 1 to {} or a title fragment", titles.len()));
        }
        if let Some(index) = number.and_then(|n| n.checked_sub(1)).filter(|i| *i < titles.len()) {
            return Ok(index);
        }
        let lower = text.to_lowercase();
        if let Some(index) = titles.iter().position(|t| t.to_lowercase().contains(&lower)) {
            return Ok(index);
        }
        Err(match number {
            Some(n) => format!("exhibit {n} is out of range: there are {} exhibits, numbered 1 to {}, and no title contains `{text}`", titles.len(), titles.len()),
            None => format!("no exhibit title contains `{text}` (phenomena_state lists the {} exhibits)", titles.len()),
        })
    }
}

/// What phenomena mode needs without a window (the switch test runs it with
/// `ModesPlugin` on MinimalPlugins + StatesPlugin): the action and its
/// handler, the gallery (its run thread) started on entering, its frames
/// taken (JobResults) and the REST snapshot. Leaving is `leave`, registered
/// by `app::switch`.
pub struct PhenomenaCorePlugin;
impl Plugin for PhenomenaCorePlugin {
    fn build(&self, app: &mut App) {
        crate::app::actions::register::<PhenomenaAction>(app);
        app.add_systems(OnEnter(ModeScope::Phenomena), enter).add_systems(
            Update,
            (actions::apply.in_set(ViewerSet::Actions), receive.in_set(ViewerSet::JobResults), actions::publish.in_set(ViewerSet::Present)).run_if(in_state(ViewerMode::Phenomena)),
        );
    }
}

/// Phenomena mode in the window: the core, the 3D view, the keys and the panels.
pub struct PhenomenaPlugin;
impl Plugin for PhenomenaPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(PhenomenaCorePlugin)
            .add_systems(OnEnter(ModeScope::Phenomena), (scene::setup, panel::setup))
            .add_systems(
                Update,
                (
                    (keys::keys, panel::buttons, panel::slider).chain().after(crate::app::actions::serve).in_set(ViewerSet::Input),
                    panel::scroll.in_set(ViewerSet::SimSync),
                    (scene::render, panel::rebuild, panel::refresh, panel::chart).chain().in_set(ViewerSet::Present),
                )
                    .run_if(in_state(ViewerMode::Phenomena)),
            );
    }
}

/// OnEnter(ModeScope::Phenomena): the gallery, its run thread building the
/// exhibits off the UI thread and opening the exhibit phenomena's registry
/// entry names (`--exhibit`'s text, or the number remembered on leaving; an
/// unknown one opens exhibit 1 and says so in the frame's notice).
fn enter(mut commands: Commands, registry: Res<DocumentRegistry>) {
    commands.insert_resource(Gallery::open(crate::app::switch::sources::exhibit_of(&registry)));
}

/// JobResults: the newest frame at or after the requested generation.
fn receive(gallery: Option<ResMut<Gallery>>) {
    if let Some(mut gallery) = gallery {
        gallery.receive();
    }
}

/// OnExit(ModeScope::Phenomena), registered by `app::switch`: the gallery is
/// removed, its exhibit remembered on phenomena's registry entry, which is
/// closed (as its 1-based number; unchanged if the exhibits were never
/// built), and the gallery
/// dropped off the UI thread with `jobs::drop_off_thread`: its run thread
/// checks its channel between ticks and so joins within `jobs::JOIN_BOUND`,
/// but one `Exhibit::advance` call (a heavy exhibit at ×64) or the exhibits'
/// construction can outlast that bound, and the switch must never wait on
/// it. The pool's meshes and the panel state go too. At window close the
/// World's drop drops the gallery in place: `RunThread`'s drop waits at most
/// `JOIN_BOUND`, then detaches the thread (jobs/run_thread.rs `Drop`).
pub(crate) fn leave(world: &mut World) {
    // A REST call waiting on the run thread is answered now: on return the
    // new gallery's `seq` starts again at 0 and could not settle it.
    if world.contains_resource::<crate::app::actions::Replies>() {
        world.resource_scope(|world, mut replies: Mut<crate::app::actions::Replies>| {
            if let Some(mut in_flight) = world.get_resource_mut::<crate::app::actions::InFlight<PhenomenaAction>>() {
                in_flight.abandon(&mut replies, "left phenomena mode before the command was applied; it was sent to a run thread that has now stopped");
            }
        });
    }
    let mut shown = None;
    if let Some(gallery) = world.remove_resource::<Gallery>() {
        shown = gallery.shown_number().map(|number| crate::document::Source::Exhibit { exhibit: Some(number.to_string()) });
        crate::jobs::drop_off_thread(gallery, "the phenomena gallery");
    }
    crate::app::switch::sources::left(world, ViewerMode::Phenomena, shown);
    world.remove_resource::<scene::Pool>();
    world.remove_resource::<panel::Panels>();
}
