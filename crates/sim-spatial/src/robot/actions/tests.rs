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
