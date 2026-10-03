use super::*;
use crate::document::{DocumentKind, Source};
use crate::selection::Item;

/// The wheeled baseline, loaded, with its run thread idle and panels ready.
fn loaded_view() -> RobotView {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = root.join("examples/wheeled-robot/baseline/robot.simrobot.json");
    let loaded = load(&path).unwrap();
    let mut view = RobotView::new(path, None, None);
    view.run = Some(RunController::replace(None, loaded.model.clone()).0);
    view.model = Some(loaded.model);
    view.panels_ready = true;
    view
}

/// The shared selection and a registry with the robot open as Robot mode's
/// document (what the window's Robot arrival makes).
fn documents(view: &RobotView) -> (Selection, DocumentRegistry) {
    let mut registry = DocumentRegistry::default();
    registry.open(ViewerMode::Robot, DocumentKind::Robot, Source::path(view.path.clone()));
    (Selection::default(), registry)
}
/// Every `system_ui` control robot mode lists for a loaded file fits one
/// of its registered control patterns, once, and REST activation parses
/// into the action that resolves it (`Activate`, through `controls`).
#[test]
fn every_listed_control_fits_a_registered_pattern() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = root.join("examples/wheeled-robot/baseline/robot.simrobot.json");
    let loaded = load(&path).unwrap();
    let mut view = RobotView::new(path, None, None);
    view.run = Some(RunController::replace(None, loaded.model.clone()).0);
    view.model = Some(loaded.model);
    view.panels_ready = true;
    // Link 1 selected: its joints' jog controls are listed too.
    let listed = controls(&view, Some(1));
    let patterns = <RobotAction as actions::Action>::controls();
    let mut ids = std::collections::BTreeSet::new();
    for (id, _, _) in &listed {
        assert!(patterns.iter().any(|p| actions::control_matches(p, id)), "{id} fits no registered control pattern");
        assert!(ids.insert(id.clone()), "{id} is listed twice");
        let command = sim_api::Command { command: "system_ui".into(), args: json!({"action": {"operation": "activate", "id": id, "ui_revision": view.ui_revision}}) };
        assert_eq!(<RobotAction as actions::Action>::parse(&command), Ok(RobotAction::Activate { id: id.clone(), ui_revision: view.ui_revision }));
    }
    for prefix in ["link:", "section:", "run:", "overlay:", "graphs:toggle"] {
        assert!(ids.iter().any(|id| id.starts_with(prefix)), "no {prefix} control");
    }
}
/// A REST command keeps its argument errors through the action's REST form.
#[test]
fn rest_argument_errors_are_unchanged() {
    let parse = |name: &str, args: Value| <RobotAction as actions::Action>::parse(&sim_api::Command { command: name.into(), args });
    assert_eq!(parse("robot_jog", json!({"joint": "j"})).unwrap_err(), "robot_jog needs exactly one of target (absolute, rad or m) or delta");
    assert_eq!(parse("robot_overlay", json!({})).unwrap_err(), "robot_overlay needs at least one of contacts, joints, deflections, stress (true | false)");
    assert_eq!(parse("robot_replay", json!({"action": "list", "file": "x.json"})).unwrap_err(), "robot_replay action `list` takes no file or path");
    assert!(parse("robot_run", json!({"action": "start", "extra": 1})).unwrap_err().contains("unknown field `extra`"));
    assert_eq!(parse("robot_speed", json!({"action": "up"})), Ok(RobotAction::Speed { speed: SpeedRequest::Up }));
    assert_eq!(parse("fit", json!({})), Ok(RobotAction::Fit));
}

/// A link clicked in the 3D view (`pick_link` writes `SelectLink`, as the
/// list rows do) and the same link activated through REST `system_ui`
/// (`link:<index>`) leave the same shared selection; `clear_selection` and
/// the click path's `ClearSelection` clear it alike.
#[test]
fn a_link_clicked_and_activated_through_system_ui_is_the_same_selection() {
    let mut view = loaded_view();
    let mut orbit = Orbit::default();
    let (mut clicked, mut clicked_documents) = documents(&view);
    let (mut rest, mut rest_documents) = documents(&view);
    let name = view.link_name(1).unwrap().to_string();
    handle(&mut view, &mut orbit, &mut clicked, &mut clicked_documents, &RobotAction::SelectLink { index: 1, name: name.clone() }).unwrap();
    let ui_revision = view.ui_revision;
    handle(&mut view, &mut orbit, &mut rest, &mut rest_documents, &RobotAction::Activate { id: "link:1".into(), ui_revision }).unwrap();
    assert_eq!(clicked.all(), rest.all());
    let document = picked::document(&rest_documents).unwrap();
    assert_eq!(rest.items(document), vec![Item::Link { index: 1, name: name.clone() }]);
    let link = picked::link(&rest, &rest_documents);
    assert_eq!(link, Some(1));
    assert_eq!(view.state_json(link)["selected"]["name"], json!(name));

    // A row whose name no longer matches the model is refused, naming both.
    let e = handle(&mut view, &mut orbit, &mut clicked, &mut clicked_documents, &RobotAction::SelectLink { index: 1, name: "gone".into() }).unwrap_err();
    assert!(e.contains("gone") && e.contains(&name), "{e}");
    assert_eq!(clicked.all(), rest.all());

    handle(&mut view, &mut orbit, &mut clicked, &mut clicked_documents, &RobotAction::ClearSelection).unwrap();
    handle(&mut view, &mut orbit, &mut rest, &mut rest_documents, &RobotAction::Activate { id: "clear_selection".into(), ui_revision }).unwrap();
    assert!(clicked.all().is_empty() && rest.all().is_empty());
    assert!(view.state_json(picked::link(&rest, &rest_documents))["selected"].is_null());
}

/// A reload advances the Robot document's revision and finds the selected
/// link again by name at its new index; a reload without it drops it, named.
#[test]
fn a_reload_keeps_the_selected_link_by_name_at_its_new_index() {
    let view = RobotView::new(PathBuf::from("robot.simrobot.json"), None, None);
    let (mut selection, mut registry) = documents(&view);
    let document = picked::document(&registry).unwrap();
    picked::select(&mut selection, &registry, 1, "b".into()).unwrap();
    let before = registry.revision(document).unwrap();
    // The file's links reordered: `b` is now the third.
    let kept = picked::reloaded(&mut selection, &mut registry, true, |n| ["c", "a", "b"].iter().position(|l| *l == n));
    assert_eq!(kept, Some(("b".to_string(), true)));
    assert_eq!(registry.revision(document), Some(before + 1));
    assert_eq!(selection.items(document), vec![Item::Link { index: 2, name: "b".into() }]);
    assert_eq!(picked::link(&selection, &registry), Some(2));
    // A reload whose file no longer has `b`.
    let kept = picked::reloaded(&mut selection, &mut registry, true, |n| ["c", "a"].iter().position(|l| *l == n));
    assert_eq!(kept, Some(("b".to_string(), false)));
    assert_eq!(selection.dropped, vec!["link b".to_string()]);
    assert_eq!(picked::link(&selection, &registry), None);
}

/// REST `robot_drive`'s three forms parse into `RobotAction::Drive`; absent
/// axes are 0; mixed forms and a false stop are refused naming the fields.
#[test]
fn robot_drive_forms_parse_and_ambiguous_ones_are_refused() {
    let parse = |args: Value| <RobotAction as actions::Action>::parse(&sim_api::Command { command: "robot_drive".into(), args });
    assert_eq!(parse(json!({"forward": 0.5, "lateral": 0, "yaw": 0})), Ok(RobotAction::Drive { request: DriveRequest::Axes { forward: 0.5, lateral: 0.0, yaw: 0.0 } }));
    assert_eq!(parse(json!({"yaw": -1})), Ok(RobotAction::Drive { request: DriveRequest::Axes { forward: 0.0, lateral: 0.0, yaw: -1.0 } }));
    assert_eq!(parse(json!({"action": "halt"})), Ok(RobotAction::Drive { request: DriveRequest::Action { name: "halt".into() } }));
    assert_eq!(parse(json!({"stop": true})), Ok(RobotAction::Drive { request: DriveRequest::Stop }));
    let e = parse(json!({"forward": 1, "action": "halt"})).unwrap_err();
    assert!(e.starts_with("robot_drive needs exactly one of") && e.contains("axes (forward/lateral/yaw) and action"), "{e}");
    let e = parse(json!({"action": "halt", "stop": true})).unwrap_err();
    assert!(e.contains("given: action and stop"), "{e}");
    assert!(parse(json!({})).unwrap_err().contains("given: none"));
    assert!(parse(json!({"stop": false})).unwrap_err().contains("stop must be true"));
    assert!(parse(json!({"forward": 1, "speed": 2})).unwrap_err().contains("unknown field `speed`"));
}

/// A robot without a drive profile (the wheeled baseline has no controller
/// binding loaded here) refuses every drive request by name, lists no
/// drive:* controls, and its planar refusal names the planar file.
#[test]
fn drive_is_refused_without_a_drive_profile() {
    let view = loaded_view();
    for request in [DriveRequest::Axes { forward: 1.0, lateral: 0.0, yaw: 0.0 }, DriveRequest::Action { name: "halt".into() }, DriveRequest::Stop] {
        let e = check(&view, &RobotAction::Drive { request }).unwrap_err();
        assert!(e.starts_with(NOT_CONTROLLED) && e.contains("controller binding"), "{e}");
    }
    assert!(!controls(&view, None).iter().any(|(id, ..)| id.starts_with("drive:")));
    // Applied (not only checked): `dispatch` hands a drive request to its own
    // handler before `check`, and it is refused by the same name.
    let mut view = view;
    let mut orbit = Orbit::default();
    let (mut selection, mut registry) = documents(&view);
    for request in [DriveRequest::Axes { forward: 0.0, lateral: 0.0, yaw: 1.0 }, DriveRequest::Stop] {
        let e = handle(&mut view, &mut orbit, &mut selection, &mut registry, &RobotAction::Drive { request }).unwrap_err();
        assert!(e.starts_with(NOT_CONTROLLED), "{e}");
    }
    // A hold-controller file neither records nor replays (its PhysicalRobot run
    // keeps no recording), so those controls are only a preset's or a controlled run's;
    // its servo-target jog stays (only a controlled run's external controller drops it).
    let listed = controls(&view, Some(1));
    assert!(!listed.iter().any(|(id, ..)| id.starts_with("recording:") || id.starts_with("replay:")));
    assert!(listed.iter().any(|(id, ..)| id.starts_with("jog:")));
    // Streaming sync refuses remote driving but not a stop.
    assert!(moves_synced_motors(&view, None, &RobotAction::Drive { request: DriveRequest::Axes { forward: 1.0, lateral: 0.0, yaw: 0.0 } }));
    assert!(!moves_synced_motors(&view, None, &RobotAction::Drive { request: DriveRequest::Stop }));
}
