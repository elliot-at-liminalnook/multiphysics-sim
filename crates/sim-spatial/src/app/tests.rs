//! Mode switching and the action layer without a window (MinimalPlugins +
//! StatesPlugin): the one switch handler, the teardown on exit, what
//! survives a switch, the Build → Lessons refusal, and the action registry
//! cross-checked against what the action types parse.
use super::actions::{self, Act, Call, Origin, Replies, Reply};
use bevy::ecs::message::Messages;
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
    // The headless server has no window to switch. (`Feature` is not Debug, so no `unwrap_err`.)
    assert!(route::route(ViewerMode::Inspect, false, &command("viewer_mode")).err().is_some_and(|e| e.contains("headless")));
    // Every capability names its modes; screenshot and viewer_mode apply to all.
    let caps = actions::capabilities();
    assert!(caps.iter().all(|c| c["modes"].as_array().is_some_and(|m| !m.is_empty())));
    for name in ["screenshot", "viewer_mode", "state", "system_ui"] {
        assert_eq!(actions::command_modes(name).map(|m| m.len()), Some(ViewerMode::ALL.len()), "{name}");
    }
    // CAD mode has no camera command of its own (cad_fit frames its view).
    assert_eq!(actions::command_modes("camera").map(|m| m.contains(&ViewerMode::Cad)), Some(false));
    assert_eq!(route::route(ViewerMode::Cad, true, &command("cad_undo")).map(|f| f.name), Ok("cad"));
    assert_eq!(route::route(ViewerMode::Cad, true, &command("system_ui")).map(|f| f.name), Ok("cad"));
    assert!(refused(ViewerMode::Build, "cad_save").contains("cad mode"));
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
        assert_eq!(controls.len(), ViewerMode::ALL.len());
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

/// Build → Cad → Build without a RoboCAD: CAD mode enters at once, says
/// truthfully that it is not connected (nothing listens on port 1), and
/// leaving removes the CAD document, its mesh cache and CAD-scoped entities
/// while the builder, its scene and the shared resources stay.
#[test]
fn build_cad_build_tears_down_the_cad_document_and_keeps_shared_state() {
    let dir = std::env::temp_dir().join(format!("mode-switch-cad-{}", std::process::id()));
    let (board, builder, scene) = self::board(&dir);
    let url = "http://127.0.0.1:1";
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, StatesPlugin))
        .insert_resource(builder)
        .insert_resource(scene)
        .insert_resource(crate::models::ModelLibrary::default())
        .insert_resource(crate::rest::Rest(crate::rest::bind(0).unwrap(), None))
        .add_plugins((ModesPlugin { initial: ViewerMode::Build }, crate::cad::CadCorePlugin));
    app.update();
    assert_eq!(mode(&app), ViewerMode::Build);

    // Refused: url is a cad-only document; a cad path must be a .rcad file.
    let seq = submit(&mut app, ViewerMode::Robot, Some(Document::Url(url.into())));
    assert!(settle(&mut app, seq).unwrap_err().contains("cad mode only"));
    let seq = submit(&mut app, ViewerMode::Cad, Some(Document::Path(board.clone())));
    assert!(settle(&mut app, seq).unwrap_err().contains(".rcad"));
    assert_eq!(mode(&app), ViewerMode::Build);

    let seq = submit(&mut app, ViewerMode::Cad, Some(Document::Url(url.into())));
    let entered = settle(&mut app, seq).unwrap();
    assert_eq!(entered["mode"], "cad");
    assert_eq!(mode(&app), ViewerMode::Cad);
    assert_eq!(*app.world().resource::<State<ModeScope>>().get(), ModeScope::Cad);
    assert!(app.world().contains_resource::<crate::cad::CadMeshes>());
    assert!(app.world().contains_resource::<Builder>(), "the builder stays in the window");
    assert!(!app.world().contains_resource::<SpatialScene>(), "the builder's scene is parked");
    let body = app.world_mut().spawn(Transform::default()).id();
    app.update();
    assert_eq!(scoped(&mut app, body), Some(ModeScope::Cad));
    // The connect job fails (connection refused); the error is shown, not hidden.
    let mut lost = None;
    for _ in 0..500 {
        app.update();
        if let crate::cad::Connection::Lost { error, .. } = &app.world().resource::<crate::cad::CadDocument>().connection {
            lost = Some(error.clone());
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let error = lost.expect("CAD mode reports the refused connection");
    assert!(error.contains("127.0.0.1:1"), "{error}");
    let doc = app.world().resource::<crate::cad::CadDocument>();
    assert!(doc.connection_line().0.starts_with("Not connected"), "{}", doc.connection_line().0);
    assert!(doc.child.pid().is_none(), "an attached service has no child process");

    // Cad → Build: the CAD document and its resources go; shared state survives.
    let seq = submit(&mut app, ViewerMode::Build, None);
    settle(&mut app, seq).unwrap();
    assert_eq!(mode(&app), ViewerMode::Build);
    let world = app.world();
    assert!(!world.contains_resource::<crate::cad::CadDocument>() && !world.contains_resource::<crate::cad::CadMeshes>());
    assert!(app.world().get_entity(body).is_err(), "cad-scoped entities are despawned on exit");
    let world = app.world();
    assert_eq!(world.resource::<Builder>().path(), board.as_path());
    assert!(world.contains_resource::<SpatialScene>() && world.contains_resource::<crate::models::ModelLibrary>() && world.contains_resource::<crate::rest::Rest>());
    assert_eq!(world.resource::<Documents>().cad, Some(crate::cad::CadTarget::Service(url.into())), "cad mode reopens what it showed");
    std::fs::remove_dir_all(&dir).ok();
}

/// A REST poll's side of an action: parse and write it once (Pending, with
/// its reply token in the continuation), then Pending until its handler answers.
fn poll(replies: &mut Replies, messages: &mut Messages<Act<u32>>, continuation: &mut Value, cancelled: bool) -> sim_api::Outcome {
    replies.submit(continuation, cancelled, || Ok(7u32), |act| {
        messages.write(act);
        true
    })
}

/// A handler that answers Pending twice (a load), then the action; a cancel stops it.
fn load(action: &u32, call: &mut Call) -> sim_api::Outcome {
    if call.cancelled {
        return sim_api::Outcome::Done(Err("cancelled".into()));
    }
    let n = call.continuation.as_u64().unwrap_or(0);
    if n < 2 {
        *call.continuation = json!(n + 1);
        return sim_api::Outcome::Pending;
    }
    sim_api::Outcome::Done(Ok(json!(*action)))
}

fn done(outcome: sim_api::Outcome) -> Result<Value, String> {
    match outcome {
        sim_api::Outcome::Done(result) => result,
        sim_api::Outcome::Pending => panic!("still pending"),
        sim_api::Outcome::Image(_) => panic!("an image"),
    }
}

/// The reply mechanism without a window: a Pending handler is re-applied
/// with its own continuation until it answers; a REST cancel reaches it as
/// `Call::cancelled`; an abandoned reply gets one cancelled application and
/// is not carried; a click's Pending is not carried.
#[test]
fn pending_actions_are_carried_until_they_answer_and_a_cancel_reaches_them() {
    let mut replies = Replies::default();
    let mut messages = Messages::<Act<u32>>::default();
    let mut in_flight = actions::InFlight::<u32>::default();
    let pending = |o: &sim_api::Outcome| matches!(o, sim_api::Outcome::Pending);

    // Answered after two Pending applications; the caller waits meanwhile.
    let mut c = Value::Null;
    assert!(pending(&poll(&mut replies, &mut messages, &mut c, false)));
    assert!(c["reply"].is_u64());
    actions::apply(&mut messages, &mut in_flight, &mut replies, load);
    assert!(!in_flight.is_empty());
    assert!(pending(&poll(&mut replies, &mut messages, &mut c, false)));
    assert!(messages.is_empty(), "written once, on the first poll");
    actions::apply(&mut messages, &mut in_flight, &mut replies, load);
    actions::apply(&mut messages, &mut in_flight, &mut replies, load);
    assert!(in_flight.is_empty());
    assert_eq!(done(poll(&mut replies, &mut messages, &mut c, false)), Ok(json!(7)));

    // A cancel while the handler works: it is told, and its answer settles the command.
    let mut c = Value::Null;
    poll(&mut replies, &mut messages, &mut c, false);
    actions::apply(&mut messages, &mut in_flight, &mut replies, load);
    assert!(pending(&poll(&mut replies, &mut messages, &mut c, true)));
    actions::apply(&mut messages, &mut in_flight, &mut replies, load);
    assert!(in_flight.is_empty());
    assert_eq!(done(poll(&mut replies, &mut messages, &mut c, true)), Err("cancelled".to_string()));

    // Abandoned (the mode changed and the poll forgot the reply): one cancelled application, not carried.
    let mut c = Value::Null;
    poll(&mut replies, &mut messages, &mut c, false);
    actions::apply(&mut messages, &mut in_flight, &mut replies, load);
    replies.forget(Reply::from_id(c["reply"].as_u64().unwrap()));
    let mut calls = 0;
    actions::apply(&mut messages, &mut in_flight, &mut replies, |_, call| {
        calls += 1;
        assert!(call.cancelled);
        sim_api::Outcome::Pending
    });
    assert_eq!(calls, 1);
    assert!(in_flight.is_empty());

    // A click's Pending is not carried (its work continues in the feature's own jobs).
    messages.write(Act::ui(3));
    actions::apply(&mut messages, &mut in_flight, &mut replies, |_, _| sim_api::Outcome::Pending);
    assert!(in_flight.is_empty() && messages.is_empty());
}

/// Phenomena mode's commands go to its action type in phenomena mode, and
/// are refused by name elsewhere.
#[test]
fn phenomena_commands_route_to_phenomena_mode_only() {
    let command = |name: &str| sim_api::Command { command: name.into(), args: json!({}) };
    for name in ["phenomena_select", "phenomena_state", "state", "system_ui", "phenomena_next"] {
        assert_eq!(route::route(ViewerMode::Phenomena, true, &command(name)).map(|f| f.name), Ok("phenomena"), "{name}");
    }
    // mode:* stays the switcher's in phenomena mode too.
    let mode = sim_api::Command { command: "system_ui".into(), args: json!({"action": {"operation": "activate", "id": "mode:build"}}) };
    assert_eq!(route::route(ViewerMode::Phenomena, true, &mode).map(|f| f.name), Ok("window"));
    let e = match route::route(ViewerMode::Build, true, &command("phenomena_next")) {
        Err(e) => e,
        Ok(f) => panic!("phenomena_next in build mode went to {}", f.name),
    };
    assert!(e.contains("`phenomena_next`") && e.contains("phenomena mode") && e.contains("active mode is build"), "{e}");
}

/// Build → Phenomena → Build: entering starts the gallery (its run thread
/// builds the built-in exhibits, opening `Documents::exhibit`), a REST
/// command waits for the run thread and answers the new state, and leaving
/// removes the gallery and phenomena-scoped entities and remembers the
/// exhibit, while the builder and the shared resources stay.
#[test]
fn build_phenomena_build_runs_the_gallery_and_remembers_the_exhibit() {
    use crate::phenomena::{Gallery, PhenomenaAction};
    let dir = std::env::temp_dir().join(format!("mode-switch-phenomena-{}", std::process::id()));
    let (board, builder, scene) = self::board(&dir);
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, StatesPlugin))
        .insert_resource(builder)
        .insert_resource(scene)
        .insert_resource(crate::models::ModelLibrary::default())
        .insert_resource(crate::rest::Rest(crate::rest::bind(0).unwrap(), None))
        .add_plugins((ModesPlugin { initial: ViewerMode::Build }, crate::phenomena::PhenomenaCorePlugin));
    app.update();
    assert_eq!(mode(&app), ViewerMode::Build);
    app.world_mut().resource_mut::<Documents>().exhibit = Some("2".into());

    // Refused: phenomena mode takes no document.
    let seq = submit(&mut app, ViewerMode::Phenomena, Some(Document::Path(board.clone())));
    assert!(settle(&mut app, seq).unwrap_err().contains("takes no document"));
    assert_eq!(mode(&app), ViewerMode::Build);

    let seq = submit(&mut app, ViewerMode::Phenomena, None);
    assert_eq!(settle(&mut app, seq).unwrap()["mode"], "phenomena");
    assert_eq!(mode(&app), ViewerMode::Phenomena);
    assert_eq!(*app.world().resource::<State<ModeScope>>().get(), ModeScope::Phenomena);
    assert!(app.world().contains_resource::<Gallery>());
    assert!(app.world().contains_resource::<Builder>(), "the builder stays in the window");
    let shape = app.world_mut().spawn(Transform::default()).id();
    app.update();
    assert_eq!(scoped(&mut app, shape), Some(ModeScope::Phenomena));

    // The exhibits are built on the run thread (not this one); --exhibit 2 opens the second.
    let started = std::time::Instant::now();
    while app.world().resource::<Gallery>().ready().is_none() {
        assert!(started.elapsed().as_secs() < 300, "the exhibits were not built in 300 s: {}", app.world().resource::<Gallery>().not_ready());
        app.update();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(app.world().resource::<Gallery>().ready().map(|f| f.current), Some(1));

    // A REST command waits for the run thread and answers the state it made.
    let act = |app: &mut App, action: PhenomenaAction| {
        let reply = app.world_mut().resource_mut::<Replies>().open();
        app.world_mut().write_message(Act { action, origin: Origin::Rest(reply) });
        reply
    };
    let reply = act(&mut app, PhenomenaAction::PhenomenaNext);
    let state = settle(&mut app, reply).unwrap();
    assert_eq!(state["current"]["number"], 3, "{state}");
    assert_eq!(state["generation"], 2);
    let reply = act(&mut app, PhenomenaAction::PhenomenaSelect { exhibit: crate::phenomena::ExhibitRef::Number(999) });
    let e = settle(&mut app, reply).unwrap_err();
    assert!(e.contains("exhibit 999 is out of range"), "{e}");

    // Phenomena → Build: the gallery and its entities go; the exhibit is remembered.
    let seq = submit(&mut app, ViewerMode::Build, None);
    settle(&mut app, seq).unwrap();
    assert_eq!(mode(&app), ViewerMode::Build);
    assert!(!app.world().contains_resource::<Gallery>(), "the gallery (its run thread) is removed");
    assert!(app.world().get_entity(shape).is_err(), "phenomena-scoped entities are despawned on exit");
    let world = app.world();
    assert_eq!(world.resource::<Builder>().path(), board.as_path());
    assert!(world.contains_resource::<SpatialScene>() && world.contains_resource::<crate::models::ModelLibrary>() && world.contains_resource::<crate::rest::Rest>());
    assert_eq!(world.resource::<Documents>().exhibit.as_deref(), Some("3"), "phenomena mode reopens the exhibit it showed");
    std::fs::remove_dir_all(&dir).ok();
}

/// Every non-test source file of this crate stays within the size cap
/// (native-viewer.md "Split large files"). CAP = 750 non-test lines. A
/// file's non-test lines are the lines before its first column-0
/// `#[cfg(test)]` line that is immediately followed by a column-0 line
/// starting with `mod ` and ending with `{` (an inline test module); with no
/// such pair, every line counts. Lines are counted as `str::lines` counts
/// them (trailing whitespace is ignored when matching the two lines). Files
/// named `tests.rs` or ending in `_tests.rs`, and every file under a
/// directory named `tests`, are skipped. `ALLOWED` lists files (relative to
/// `src`, `/` separators) allowed over the cap, each with its reason; an
/// entry whose file no longer exists or is back within the cap fails too, so
/// the list stays short and truthful.
#[test]
fn source_files_stay_small() {
    const CAP: usize = 750;
    const ALLOWED: &[(&str, &str)] = &[];
    let src = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src"));
    let mut sizes: Vec<(String, usize)> = Vec::new();
    let mut dirs = vec![src.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
            let path = entry.unwrap_or_else(|e| panic!("{}: {e}", dir.display())).path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
            if path.is_dir() {
                if name != "tests" {
                    dirs.push(path);
                }
                continue;
            }
            if !name.ends_with(".rs") || name == "tests.rs" || name.ends_with("_tests.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let lines: Vec<&str> = text.lines().collect();
            let count = lines
                .windows(2)
                .position(|pair| pair[0].trim_end() == "#[cfg(test)]" && pair[1].starts_with("mod ") && pair[1].trim_end().ends_with('{'))
                .unwrap_or(lines.len());
            let relative = path.strip_prefix(src).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            sizes.push((relative, count));
        }
    }
    let mut offenders: Vec<String> = sizes
        .iter()
        .filter(|(path, count)| *count > CAP && !ALLOWED.iter().any(|(allowed, _)| *allowed == path.as_str()))
        .map(|(path, count)| format!("{path}: {count} non-test lines (cap {CAP})"))
        .collect();
    for (allowed, _) in ALLOWED {
        match sizes.iter().find(|(path, _)| path.as_str() == *allowed) {
            None => offenders.push(format!("{allowed}: allowlisted but no longer exists; remove it from ALLOWED")),
            Some((_, count)) if *count <= CAP => offenders.push(format!("{allowed}: allowlisted but {count} non-test lines is within the cap ({CAP}); remove it from ALLOWED")),
            Some(_) => {}
        }
    }
    offenders.sort();
    assert!(offenders.is_empty(), "source files over the size cap; split each along a seam (native-viewer.md \"Split large files\"):\n{}", offenders.join("\n"));
}
