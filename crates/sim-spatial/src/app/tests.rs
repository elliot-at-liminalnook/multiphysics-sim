//! Mode switching and the action layer without a window (MinimalPlugins +
//! StatesPlugin): the one switch handler, the teardown on exit, what
//! survives a switch, the Build → Lessons refusal, and the action registry
//! cross-checked against what the action types parse.
use super::actions::{self, Act, Origin, Replies, Reply};
use super::switch::{Document, Documents, ModeSwitch, WindowAction};
use super::*;
use crate::builder::Builder;
use crate::robot::RobotView;
use crate::SpatialScene;
use bevy::state::app::StatesPlugin;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn mode(app: &App) -> ViewerMode {
    *app.world().resource::<State<ViewerMode>>().get()
}

/// Run frames until `reply` is answered (a load runs on a pool).
fn settle(app: &mut App, reply: Reply) -> Result<Value, String> {
    for _ in 0..1000 {
        app.update();
        if let Some(outcome) = app.world_mut().resource_mut::<Replies>().take(reply) {
            return match outcome {
                sim_api::Outcome::Done(result) => result,
                _ => panic!("a mode switch answers Done"),
            };
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("mode switch {reply:?} did not settle");
}

/// A switch as a REST caller asks for it: the action, with a reply token.
fn submit(app: &mut App, mode: ViewerMode, document: Option<Document>) -> Reply {
    let reply = app.world_mut().resource_mut::<Replies>().open();
    app.world_mut().write_message(Act { action: WindowAction::Switch(ModeSwitch { mode, document }), origin: Origin::Rest(reply) });
    reply
}

fn scoped(app: &mut App, entity: Entity) -> Option<ModeScope> {
    app.world().get::<DespawnOnExit<ModeScope>>(entity).map(|d| d.0)
}

/// The motor-driver board in a temp dir, opened in the builder, and its scene.
fn board(dir: &std::path::Path) -> (PathBuf, Builder, SpatialScene) {
    let root = root();
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).unwrap();
    let board = dir.join("board.system.json");
    std::fs::copy(root.join("examples/systems-builder/motor-driver-board/board.system.json"), &board).unwrap();
    let registry = sim_runtime::registry_with_parts(&root.join("library/parts")).0;
    let builder = Builder::open(board.clone(), root.join("library/systems"), registry).unwrap();
    let scene = crate::builder::compiled_scene(&builder).unwrap();
    (board, builder, scene)
}

#[test]
fn build_robot_build_tears_down_the_robot_and_keeps_shared_state() {
    let root = root();
    let dir = std::env::temp_dir().join(format!("mode-switch-{}", std::process::id()));
    let (board, builder, scene) = self::board(&dir);
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

/// Entering Lessons from Build goes through the switch's validation, like
/// leaving Build: refused, naming the blocker, with the mode and the draft
/// kept; a live run is paused and kept, not dropped.
#[test]
fn entering_lessons_from_build_refuses_on_a_draft_and_keeps_a_live_run() {
    let root = root();
    let dir = std::env::temp_dir().join(format!("lessons-switch-{}", std::process::id()));
    let (_, builder, scene) = board(&dir);
    let registry = sim_runtime::registry_with_parts(&root.join("library/parts")).0;
    // A lesson is open in the window: Build ↔ Lessons shares this builder.
    let learn = crate::lesson::Learn::new(dir.join("lessons"), root.join("library/systems"), registry);

    let mut app = App::new();
    app.add_plugins((MinimalPlugins, StatesPlugin))
        .insert_resource(builder)
        .insert_resource(scene)
        .insert_resource(learn)
        .insert_resource(crate::models::ModelLibrary::default())
        .add_plugins(ModesPlugin { initial: ViewerMode::Build });
    app.update();
    assert_eq!(mode(&app), ViewerMode::Build);

    // Refused by name while a builder draft is open; the mode and the draft stay.
    app.world_mut().resource_mut::<Builder>().test_open_draft("motor");
    let reply = submit(&mut app, ViewerMode::Lessons, None);
    let e = settle(&mut app, reply).unwrap_err();
    assert!(e.contains("Not switching to Lessons mode") && e.contains("text field draft") && e.contains("Build mode stays"), "{e}");
    assert_eq!(mode(&app), ViewerMode::Build);
    assert!(app.world().resource::<Builder>().typing(), "the draft is kept");
    // The switcher shows the same refusal.
    assert!(matches!(&app.world().resource::<super::switch::Switcher>().message, Some(Err(m)) if m.contains("text field draft")));
    app.world_mut().resource_mut::<Builder>().test_drop_draft();

    // A live run is paused and kept.
    app.world_mut().resource_scope(|world, mut builder: Mut<Builder>| builder.run_start(world.resource::<SpatialScene>()));
    let running = |app: &App| app.world().resource::<Builder>().test_run();
    for _ in 0..500 {
        if running(&app) == Some(true) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(running(&app), Some(true), "the run started");
    let reply = submit(&mut app, ViewerMode::Lessons, None);
    settle(&mut app, reply).unwrap();
    assert_eq!(mode(&app), ViewerMode::Lessons);
    for _ in 0..500 {
        if running(&app) == Some(false) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(running(&app), Some(false), "the run is kept, paused");
    drop(app);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn rest_refuses_commands_of_another_mode_by_name() {
    let command = |name: &str| sim_api::Command { command: name.into(), args: json!({}) };
    let refused = |mode, name: &str| match route::route(mode, true, &command(name)) {
        Err(e) => e,
        Ok(f) => panic!("{name} in {mode:?} was not refused (went to {})", f.name),
    };
    let e = refused(ViewerMode::Inspect, "robot_run");
    assert!(e.contains("`robot_run`") && e.contains("robot mode") && e.contains("active mode is inspect"), "{e}");
    let e = refused(ViewerMode::Robot, "system_state");
    assert!(e.contains("build or lessons") && e.contains("active mode is robot"), "{e}");
    // Commands of the active mode go to its action type.
    for (mode, name, feature) in [(ViewerMode::Robot, "robot_state", "robot"), (ViewerMode::Build, "system_state", "build"), (ViewerMode::Lessons, "lesson_state", "lessons"), (ViewerMode::Place, "camera", "place"), (ViewerMode::Inspect, "state", "inspect"), (ViewerMode::Lessons, "state", "inspect"), (ViewerMode::Robot, "system_ui", "robot"), (ViewerMode::Place, "system_ui", "switcher"), (ViewerMode::Build, "viewer_mode", "window")] {
        assert_eq!(route::route(mode, true, &command(name)).map(|f| f.name), Ok(feature), "{name} in {mode:?}");
    }
    // The headless server has no window to switch.
    assert!(route::route(ViewerMode::Inspect, false, &command("viewer_mode")).unwrap_err().contains("headless"));
    // Every capability names its modes; screenshot and viewer_mode apply to all.
    let caps = actions::capabilities();
    assert!(caps.iter().all(|c| c["modes"].as_array().is_some_and(|m| !m.is_empty())));
    for name in ["screenshot", "viewer_mode", "state", "camera", "system_ui"] {
        assert_eq!(actions::command_modes(name).map(|m| m.len()), Some(5), "{name}");
    }
}

/// The REST surface cannot drift from the actions: every registered
/// command's example parses into its action type, and each action type's
/// registered commands are exactly the commands its parser accepts
/// (serde's own variant list), with no orphan either way.
#[test]
fn every_capability_parses_into_its_action_and_every_parsed_command_is_registered() {
    let mut registered: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    let mut accepted: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for feature in actions::registry() {
        for spec in (feature.commands)() {
            let command = sim_api::Command { command: spec.name.into(), args: spec.example.clone() };
            if let Err(e) = (feature.parses)(&command) {
                panic!("{} ({}): the example {} does not parse: {e}", spec.name, feature.name, spec.example);
            }
            assert!(!spec.modes.is_empty() && !spec.description.is_empty(), "{}", spec.name);
            registered.entry(feature.action).or_default().insert(spec.name);
        }
        let names = (feature.accepts)();
        assert!(!names.is_empty(), "{}: its parser lists no commands", feature.name);
        accepted.entry(feature.action).or_default().extend(names);
    }
    assert_eq!(registered, accepted, "registered commands (left) and parsed commands (right), by action type");
    // Each (mode, command) has one action type.
    let mut seen = BTreeSet::new();
    for c in actions::capabilities() {
        for m in c["modes"].as_array().unwrap() {
            assert!(seen.insert((m.as_str().unwrap().to_string(), c["command"].as_str().unwrap().to_string())), "{m} {} is registered twice", c["command"]);
        }
    }
    // A command no action type accepts is refused by its mode's parser, naming it.
    for mode in ViewerMode::ALL {
        let bogus = sim_api::Command { command: "no_such_command".into(), args: json!({}) };
        let feature = route::route(mode, true, &bogus).unwrap();
        assert!((feature.parses)(&bogus).unwrap_err().contains("no_such_command"), "{mode:?}");
    }
}

/// Every `system_ui` control the mode switcher lists (in every mode) fits a
/// registered control pattern and resolves to the switch action for its mode.
/// (Robot's controls are checked in `robot::actions::tests`; the builder's,
/// collected from its buttons, in `builder::ui_api::tests`.)
#[test]
fn every_mode_control_resolves_to_a_switch() {
    let patterns = <WindowAction as actions::Action>::controls();
    for active in ViewerMode::ALL {
        let controls = route::mode_controls(active);
        assert_eq!(controls.len(), 5);
        for control in controls {
            let id = control["id"].as_str().unwrap();
            assert!(patterns.iter().any(|p| actions::control_matches(p, id)), "{id}");
            let args = json!({"action": {"operation": "activate", "id": id, "ui_revision": 0}});
            let Value::Object(args) = args else { unreachable!() };
            let switch = WindowAction::mode_control(&args).expect("a mode control").expect("a known mode");
            assert_eq!(switch.mode.name(), id.trim_start_matches("mode:"));
            assert!(switch.document.is_none());
        }
    }
    // The parsed REST form reaches the same action.
    let command = sim_api::Command { command: "system_ui".into(), args: json!({"action": {"operation": "activate", "id": "mode:robot", "ui_revision": 3}}) };
    assert_eq!(route::route(ViewerMode::Build, true, &command).map(|f| f.name), Ok("window"));
    assert!(matches!(<WindowAction as actions::Action>::parse(&command), Ok(WindowAction::SystemUi(_))));
}
