use super::*;
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
    view.selected = Some(1);
    let listed = controls(&view);
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
