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

/// The drive poller's run identity for Robot mode (`controls::drive::run_identity`,
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

/// The Section SubStates pilot: the state exists only inside Robot mode,
/// mirrors `RobotView::section` (still the authoritative value), spawns
/// the Comments body on entering the section (rereading as Refresh does)
/// and despawns it on leaving (`DespawnOnExit`).
#[test]
fn section_substate_mirrors_view_and_scopes_the_comments_body() {
    let mut app = App::new();
    app.add_plugins((bevy::MinimalPlugins, bevy::state::app::StatesPlugin))
        .insert_state(ViewerMode::Build)
        .insert_resource(crate::ui_kit::UiFonts { regular: Default::default(), italic: Default::default(), mono: Default::default(), medium: Default::default(), semibold: Default::default(), icons: Default::default() })
        .insert_resource(RobotView::new(PathBuf::from("pilot.simrobot.json"), None, None));
    section_state(&mut app);
    threads::build(&mut app);
    let root = app.world_mut().spawn((Node::default(), threads::ThreadsRoot)).id();

    let section = |app: &App| app.world().get_resource::<State<Section>>().map(|s| *s.get());
    let bodies = |app: &mut App| {
        let world = app.world_mut();
        let mut query = world.query_filtered::<Entity, With<threads::ThreadsSectionBody>>();
        query.iter(world).collect::<Vec<_>>()
    };

    app.update();
    assert_eq!(section(&app), None, "no Section state outside Robot mode");

    app.world_mut().resource_mut::<NextState<ViewerMode>>().set(ViewerMode::Robot);
    app.update();
    app.update();
    assert_eq!(section(&app), Some(Section::Link), "the file default section, mirrored");
    assert!(bodies(&mut app).is_empty());

    // ShowSection's effect on the view, as the action handler leaves it;
    // the mirror projects it and the next transition enters Comments.
    app.world_mut().resource_mut::<RobotView>().section = Section::Comments;
    app.world_mut().resource_mut::<threads::RobotThreads>().error = Some("stale".into());
    app.update();
    app.update();
    assert_eq!(section(&app), Some(Section::Comments));
    assert!(app.world().resource::<threads::RobotThreads>().error.is_none(), "entering Comments rereads (Refresh clears the error)");
    let live = bodies(&mut app);
    assert_eq!(live.len(), 1, "OnEnter spawned the scoped Comments body");
    let body = live[0];
    assert_eq!(app.world().get::<ChildOf>(body).map(|c| c.parent()), Some(root));
    assert!(app.world().get::<Children>(body).is_some_and(|c| !c.is_empty()), "the section drew into its scoped body");

    // Leaving the section despawns the body; leaving Robot mode removes
    // the SubState entirely.
    app.world_mut().resource_mut::<RobotView>().section = Section::Link;
    app.update();
    app.update();
    assert_eq!(section(&app), Some(Section::Link));
    assert!(bodies(&mut app).is_empty(), "DespawnOnExit took the Comments body");
    app.world_mut().resource_mut::<NextState<ViewerMode>>().set(ViewerMode::Build);
    app.update();
    assert_eq!(section(&app), None, "the SubState leaves with Robot mode");
}
