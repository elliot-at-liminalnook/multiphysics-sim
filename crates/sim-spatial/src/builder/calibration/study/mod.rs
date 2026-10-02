//! Offline measured-PWM and controller-refinement authoring within the existing Actuators surface.
//! Global retained state outlives docks and modes. Input occurrences drain in
//! Actions; job receipts publish globally in JobResults. No frame system runs physics.
pub mod actions;
pub mod jobs;
pub mod state;
pub mod ui;
pub mod forms;
pub mod chart;
pub use actions::StudyAction;
pub use state::{StudyOwner, StudyStamp};
use bevy::prelude::*;
use crate::app::{ViewerSet, actions as action_layer};

pub struct StudyPlugin;
impl Plugin for StudyPlugin {
    fn build(&self, app:&mut App) {
        action_layer::register::<StudyAction>(app);
        app.init_resource::<StudyOwner>()
            .add_systems(Update, actions::apply.in_set(ViewerSet::Actions))
            .add_systems(Update, jobs::poll.in_set(ViewerSet::JobResults));
        // Presentation owns kit input bindings and chart work, never study state.
        ui::register(app);
    }
}
#[cfg(test)]
mod tests;

#[cfg(test)]
mod refinement_lifecycle;

mod recording_jobs;
#[cfg(test)]
mod recording_lifecycle;

#[cfg(test)]
mod publication_lifecycle;

#[cfg(test)]
mod electrical_lifecycle;
