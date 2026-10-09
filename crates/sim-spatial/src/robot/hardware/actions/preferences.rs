//! Remembered panel choices: seeding the form from the saved settings and the
//! settings paths each action writes.
use super::*;

/// Seed remembered choices and publish the form's host-only Inputs through
/// the same path as an accepted form edit. This does not request hardware,
/// connect, confirm an operator action or activate a session.
pub(in crate::robot::hardware) fn seed_preferences(hw: &mut Hardware, settings: &crate::robot::hardware::settings::Settings) {
    if let Some(mode) = settings.calibration.drive_mode {
        hw.form.inputs.drive_mode = mode;
    }
    if let Some(on) = settings.calibration.hold_others {
        hw.form.inputs.hold_others = on;
    }
    // Connection can precede settings readiness. Keep the connected session's
    // send-time configuration coherent with the displayed form, in FIFO order
    // before any explicit Select/Press handled later in this apply system.
    inputs_changed(hw);
    hw.mirror.load_preferences(&settings.mirror);
    hw.sync.load_preferences(&settings.sync);
    hw.settings = settings.clone();
    hw.preferences_loaded = true;
    hw.ui_revision += 1;
}

pub(in crate::robot::hardware) fn preference_paths(action: &HardwareAction) -> Vec<String> {
    use HardwareAction as H;
    let paths: Vec<String> = match action {
        H::DriveMode { .. } => vec!["/calibration/drive_mode".into()],
        H::HoldOthers { .. } => vec!["/calibration/hold_others".into()],
        H::MirrorEnabled { .. } => vec!["/mirror/enabled".into()],
        H::MirrorLeg { .. } => vec!["/mirror/leg".into()],
        H::MirrorJoint { id, .. } => vec![format!("/mirror/bindings/{id}/joint")],
        H::MirrorPolarity { id, .. } => vec![format!("/mirror/bindings/{id}/polarity")],
        H::MirrorAlign { id, .. } => vec![format!("/mirror/bindings/{id}/align")],
        H::SyncLeg { .. } => vec!["/sync/leg".into(), "/sync/bindings".into()],
        H::SyncMotor { .. } | H::SyncPolarity { .. } => vec!["/sync/bindings".into()],
        H::SyncScale { .. } => vec!["/sync/amplitude".into()],
        H::SyncStart => vec!["/sync".into()],
        _ => Vec::new(),
    };
    paths
}
