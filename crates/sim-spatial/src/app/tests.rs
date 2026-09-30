//! Mode switching without a window (MinimalPlugins + StatesPlugin): the one
//! handler, the teardown on exit and what survives a switch.
use super::switch::{Document, Documents, ModeSwitch, Switcher};
use super::*;
use crate::builder::Builder;
use crate::robot::RobotView;
use crate::SpatialScene;
use bevy::state::app::StatesPlugin;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn mode(app: &App) -> ViewerMode {
    *app.world().resource::<State<ViewerMode>>().get()
}

/// Run frames until request `seq` has an outcome (a load runs on a pool).
fn settle(app: &mut App, seq: u64) -> Result<serde_json::Value, String> {
    for _ in 0..1000 {
        app.update();
        if let Some(outcome) = app.world().resource::<Switcher>().outcome(seq) {
            return outcome;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("mode switch {seq} did not settle");
}

fn submit(app: &mut App, mode: ViewerMode, document: Option<Document>) -> u64 {
    app.world_mut().resource_mut::<Switcher>().submit(ModeSwitch { mode, document })
}

fn scoped(app: &mut App, entity: Entity) -> Option<ModeScope> {
    app.world().get::<DespawnOnExit<ModeScope>>(entity).map(|d| d.0)
}

#[test]
fn build_robot_build_tears_down_the_robot_and_keeps_shared_state() {
    let root = root();
    let dir = std::env::temp_dir().join(format!("mode-switch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let board = dir.join("board.system.json");
    std::fs::copy(root.join("examples/systems-builder/motor-driver-board/board.system.json"), &board).unwrap();
    let registry = sim_runtime::registry_with_parts(&root.join("library/parts")).0;
    let builder = Builder::open(board.clone(), root.join("library/systems"), registry).unwrap();
    let scene = crate::builder::compiled_scene(&builder).unwrap();
    let robot = root.join("examples/wheeled-robot/baseline/robot.simrobot.json");

    let mut app = App::new();
    app.add_plugins((MinimalPlugins, StatesPlugin))
        .insert_resource(builder)
        .insert_resource(scene)
        .insert_resource(crate::models::ModelLibrary::default())
        .insert_resource(crate::rest::Rest(crate::rest::bind(0).unwrap(), None))
        .add_plugins(ModesPlugin { initial: ViewerMode::Build });
    app.update();
    assert_eq!(mode(&app), ViewerMode::Build);
    assert_eq!(*app.world().resource::<State<ModeScope>>().get(), ModeScope::Builder);
    let build_part = app.world_mut().spawn(Transform::default()).id();
    app.update();
    assert_eq!(scoped(&mut app, build_part), Some(ModeScope::Builder));

    // Refused: no document for place mode; the state and the builder stay.
    let seq = submit(&mut app, ViewerMode::Place, None);
    let e = settle(&mut app, seq).unwrap_err();
    assert!(e.contains("place mode needs") && e.contains("Build mode stays"), "{e}");
    assert_eq!(mode(&app), ViewerMode::Build);
    assert!(app.world().contains_resource::<SpatialScene>() && app.world().get_entity(build_part).is_ok());

    // Refused: a draft is one of open.rs's system_open blockers (the shared check).
    app.world_mut().resource_mut::<Builder>().test_open_draft("motor");
    let seq = submit(&mut app, ViewerMode::Robot, Some(Document::Path(robot.clone())));
    let e = settle(&mut app, seq).unwrap_err();
    assert!(e.contains("text field draft") && e.contains("Build mode stays"), "{e}");
    assert_eq!(mode(&app), ViewerMode::Build);
    app.world_mut().resource_mut::<Builder>().test_drop_draft();

    // Refused: a file that fails to load names it; the state stays.
    let missing = dir.join("missing.simrobot.json");
    let seq = submit(&mut app, ViewerMode::Robot, Some(Document::Path(missing.clone())));
    let e = settle(&mut app, seq).unwrap_err();
    assert!(e.contains(&missing.display().to_string()), "{e}");
    assert_eq!(mode(&app), ViewerMode::Build);

    // Build → Robot: the robot loads off the UI thread, then the mode changes.
    let seq = submit(&mut app, ViewerMode::Robot, Some(Document::Path(robot.clone())));
    let entered = settle(&mut app, seq).unwrap();
    assert_eq!(entered["mode"], "robot");
    assert_eq!(mode(&app), ViewerMode::Robot);
    assert!(app.world().contains_resource::<RobotView>());
    assert!(app.world().get_entity(build_part).is_err(), "build-scoped entities are despawned on exit");
    assert!(!app.world().contains_resource::<SpatialScene>(), "the builder's scene is parked while robot mode is shown");
    assert!(app.world().contains_resource::<Builder>(), "the builder stays in the window");
    let link = app.world_mut().spawn(Transform::default()).id();
    let child = app.world_mut().spawn(Transform::default()).id();
    app.world_mut().entity_mut(link).add_child(child);
    app.update();
    assert_eq!(scoped(&mut app, link), Some(ModeScope::Robot));

    // Robot → Build: robot entities and the view go; shared state survives.
    let seq = submit(&mut app, ViewerMode::Build, None);
    settle(&mut app, seq).unwrap();
    assert_eq!(mode(&app), ViewerMode::Build);
    assert!(app.world().get_entity(link).is_err() && app.world().get_entity(child).is_err(), "robot-scoped entities (and children) are despawned");
    assert!(!app.world().contains_resource::<RobotView>(), "the robot view (its run threads) is removed");
    let world = app.world();
    assert_eq!(world.resource::<Builder>().path(), board.as_path());
    assert!(world.contains_resource::<SpatialScene>() && world.contains_resource::<crate::models::ModelLibrary>() && world.contains_resource::<crate::rest::Rest>());
    assert_eq!(world.resource::<Documents>().robot, Some(Document::Path(robot)), "robot mode reopens what it showed");
    // Unchanged: the same mode again.
    let seq = submit(&mut app, ViewerMode::Build, None);
    assert_eq!(settle(&mut app, seq).unwrap()["unchanged"], true);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn rest_refuses_commands_of_another_mode_by_name() {
    let command = |name: &str| sim_api::Command { command: name.into(), args: serde_json::json!({}) };
    let mut continuation = serde_json::Value::Null;
    let refused = |mode, name: &str, continuation: &mut serde_json::Value| match route::route(mode, None, &command(name), continuation, false) {
        Some(sim_api::Outcome::Done(Err(e))) => e,
        _ => panic!("{name} in {mode:?} was not refused"),
    };
    let e = refused(ViewerMode::Inspect, "robot_run", &mut continuation);
    assert!(e.contains("`robot_run`") && e.contains("robot mode") && e.contains("active mode is inspect"), "{e}");
    let e = refused(ViewerMode::Robot, "system_state", &mut continuation);
    assert!(e.contains("build or lessons") && e.contains("active mode is robot"), "{e}");
    // Commands of the active mode (and ones no capability names) pass to its handler.
    for (mode, name) in [(ViewerMode::Robot, "robot_state"), (ViewerMode::Build, "system_state"), (ViewerMode::Lessons, "lesson_state"), (ViewerMode::Place, "camera"), (ViewerMode::Inspect, "state")] {
        assert!(route::route(mode, None, &command(name), &mut continuation, false).is_none(), "{name} in {mode:?}");
    }
    // Every capability names its modes; screenshot and viewer_mode apply to all.
    let caps = crate::rest::capabilities();
    assert!(caps.iter().all(|c| c["modes"].as_array().is_some_and(|m| !m.is_empty())));
    for name in ["screenshot", "viewer_mode", "state", "camera", "system_ui"] {
        assert_eq!(route::command_modes(name).map(|m| m.len()), Some(5), "{name}");
    }
}
