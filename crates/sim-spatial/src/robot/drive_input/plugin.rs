//! Drive input registration: its two resources and the bindings sync.
use super::{DriveBindings, DriveInput};
use crate::app::ViewerSet;
use crate::app::settings::{SettingsOwner, SettingsSet};
use bevy::prelude::*;

/// Registers [`DriveBindings`] (the committed defaults until the preferences
/// load) and [`DriveInput`], and keeps the bindings in step with the settings
/// owner. Called by `RobotPlugin`; the input system is in robot mode's chain.
pub(crate) fn build(app: &mut App) {
    app.init_resource::<DriveBindings>()
        .init_resource::<DriveInput>()
        // After the settings owner lands its load (`SettingsSet::Publish`, a
        // public set in JobResults), so the next frame's Input reads them.
        .add_systems(Update, sync_bindings.in_set(ViewerSet::JobResults).after(SettingsSet::Publish));
}

/// JobResults: [`DriveBindings`] follows the owner's `drive_bindings` (the
/// defaults while none are stored). Every mode: the bindings are global. The
/// owner validates before it accepts a value, so a refusal here can only be
/// a build whose defaults no longer validate; the previous bindings stay and
/// the error is logged.
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
