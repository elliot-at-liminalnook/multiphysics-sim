//! Per-frame housekeeping: watching the file and parts, installing loads
//! finished in the background, and frame timing.
use super::*;

/// Install a system opened in the background (scene, annotations, models follow it).
pub(super) fn open_system(mut builder: ResMut<Builder>, mut scene: ResMut<SpatialScene>, models: Option<ResMut<crate::models::ModelLibrary>>) {
    if builder.open.job.is_some() {
        builder.finish_open(&mut scene, models.map(|m| m.into_inner()));
    }
}

/// Install a finished actuator registry load and check.
pub(super) fn finish_actuators(mut builder: ResMut<Builder>) {
    if builder.actuators.pending().is_some() {
        builder.finish_actuators();
    }
}

/// Install a finished identification archive load.
pub(super) fn finish_calibration(mut builder: ResMut<Builder>) {
    if builder.calibration.pending().is_some() {
        builder.finish_calibration();
    }
}

/// Install a finished gait-lab results scan.
pub(super) fn finish_gait_reports(mut builder: ResMut<Builder>) {
    if builder.gait_lab.pending().is_some() {
        builder.finish_gait_reports();
    }
}

pub(super) fn watch(time: Res<Time>, mut builder: ResMut<Builder>) {
    // Defer directory scans and disk reloads during direct manipulation. The
    // worker's expected-revision transaction still rejects external edits.
    if builder.drag.is_some() { return; }
    let now = time.elapsed_secs_f64();
    if now - builder.checked < 0.4 {
        return;
    }
    builder.checked = now;
    builder.reload_parts();
    if (now / 2.0).floor() != ((now - 0.4) / 2.0).floor() {
        let updates = builder.library_updates();
        if updates != builder.updates {
            builder.updates = updates;
            builder.panel_dirty = true;
        }
    }
    if builder.store.stamp() != builder.stamp {
        builder.reload();
        builder.status = "Reloaded: the system file changed in another editor.".into();
    }
}

/// Measure UI-thread frame times: the worst frame and the count over 50 ms
/// in each 10 s window (reported by `system_state`).
pub(super) fn frame_timing(time: Res<Time>, mut builder: ResMut<Builder>) {
    let dt = time.delta_secs_f64();
    let now = time.elapsed_secs_f64();
    let frames = builder.bypass_change_detection();
    if now - frames.frames.2 > 10. {
        frames.frames = (0., 0, now);
    }
    frames.frames.0 = frames.frames.0.max(dt);
    if dt > 0.05 {
        frames.frames.1 += 1;
    }
}
