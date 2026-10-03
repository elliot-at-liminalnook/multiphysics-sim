//! [`DriveInputPlugin`]: the drive input's resources, its message, the
//! bindings sync and the one device poller.
use super::{DriveBindings, DriveDevice, DriveInput, DriveTarget};
use crate::app::actions::Act;
use crate::app::settings::{SettingsOwner, SettingsSet};
use crate::app::{InputSet, ViewerSet};
use bevy::prelude::*;

/// Registers [`DriveBindings`] (the committed defaults until the preferences
/// load), [`DriveInput`], [`DriveTarget`] and `Act<DriveDevice>`; keeps the
/// bindings in step with the settings owner; and runs the one device poller
/// (`input::devices`) in `InputSet::Window` in every mode (it reads nothing
/// unless the current mode's target is live). The modes write the target
/// and drain the messages in their own systems.
pub struct DriveInputPlugin;
impl Plugin for DriveInputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DriveBindings>()
            .init_resource::<DriveInput>()
            .init_resource::<DriveTarget>()
            .add_message::<Act<DriveDevice>>()
            .add_systems(Update, super::input::devices.in_set(InputSet::Window))
            // After the settings owner lands its load (`SettingsSet::Publish`, a
            // public set in JobResults), so the next frame's Input reads them.
            .add_systems(Update, sync_bindings.in_set(ViewerSet::JobResults).after(SettingsSet::Publish));
    }
}

/// JobResults: [`DriveBindings`] follows the owner's `drive_bindings` (the
/// defaults while none are stored). Every mode: the bindings are global. The
/// owner validates before it accepts a value, so a refusal here can only be
/// a build whose defaults no longer validate (or whose Bevy mapping misses a
/// name); the previous bindings stay and the error is logged.
pub(crate) fn sync_bindings(owner: Option<Res<SettingsOwner>>, mut bindings: ResMut<DriveBindings>) {
    let Some(owner) = owner else { return };
    if !owner.is_changed() {
        return;
    }
    let stored = owner.drive_bindings.clone();
    let is_stored = stored.is_some();
    match DriveBindings::new(stored.unwrap_or_default(), is_stored) {
        // Unchanged bindings leave the resource unchanged (no false change for the inspector).
        Ok(next) => {
            bindings.set_if_neq(next);
        }
        Err(e) => bevy::log::warn!("drive bindings not applied: {e}"),
    }
}
