use super::*;
use super::overlay_view::stress_label;
#[test]
fn stress_label_keeps_three_significant_figures_across_units() {
    assert_eq!(stress_label(0.0), "0 Pa");
    assert_eq!(stress_label(12_345.0), "12.3 kPa");
    assert_eq!(stress_label(4_560.0), "4.56 kPa");
    assert_eq!(stress_label(999_960.0), "1.00 MPa");
    assert_eq!(stress_label(23_456_789.0), "23.5 MPa");
    assert_eq!(stress_label(250e6), "250 MPa");
    assert_eq!(stress_label(7.25), "7.25 Pa");
    assert_eq!(stress_label(f64::NAN), "—");
}
#[test]
fn robot_mode_loads_wheeled_baseline_and_names_bad_paths() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let loaded = load(&root.join("examples/wheeled-robot/baseline/robot.simrobot.json")).unwrap();
    assert_eq!(loaded.model.links.len(), 4);
    assert_eq!(loaded.geometry.len(), 4);
    assert!(loaded.geometry.iter().all(|g| g.as_ref().is_some_and(|g| g.triangles() > 0)));
    let missing = root.join("examples/wheeled-robot/baseline/no-such.simrobot.json");
    let err = load(&missing).err().unwrap();
    assert!(err.contains(&*missing.to_string_lossy()), "{err}");
}

/// The drive poller's run identity for Robot mode (`controls::run_identity`,
/// `LiveTarget::run`): the file and the run controller's generation, so a
/// Reset and a reload (`RunController::replace`) each change it and the
/// poller disarms a key held across them. A replay start bumps the
/// generation too, and a replay in progress adds ", replaying", so its end is
/// a change as well (a replay needs a recording; not exercised here).
#[test]
fn robot_run_identity_changes_with_reset_and_reload() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = root.join("examples/wheeled-robot/baseline/robot.simrobot.json");
    let model = load(&path).unwrap().model;
    let mut run = RunController::spawn_at(model.clone(), 3);
    let first = controls::run_identity(&path, &run);
    assert_eq!(first, format!("{} (run generation 3)", path.display()));
    assert_eq!(run.replay_state().phase, ReplayPhase::Idle);
    run.act(RunAction::Reset).unwrap();
    let reset = controls::run_identity(&path, &run);
    assert_ne!(reset, first, "a Reset is a new run identity");
    let (reloaded, _) = RunController::replace(Some(run), model);
    let after = controls::run_identity(&path, &reloaded);
    assert_ne!(after, reset, "a reload (even of the same file) is a new run identity");
    assert_eq!(after, format!("{} (run generation 5)", path.display()));
}
