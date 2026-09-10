use serde_json::Value;
use sim_runtime::robot_contract::inspect;

#[test]
fn same_inspection_api_handles_quadruped_and_wheeled_cad_with_registry_units() {
    let inputs = [
        include_str!("../../../examples/full-robot/teacher-baseline/scene.json"),
        include_str!("../../../examples/wheeled-robot/baseline/robot.simrobot.json"),
    ];
    let inspections = inputs
        .iter()
        .map(|input| {
            let document: Value = serde_json::from_str(input).unwrap();
            let unchanged = document.clone();
            let result = inspect(document.clone()).unwrap();
            assert_eq!(document, unchanged);
            assert!(!result.robot.has_errors(), "{:?}", result.robot.issues);
            assert!(!result.robot.entities.is_empty());
            let registered = result.registered_robot_components.as_array().unwrap();
            assert!(registered.iter().any(|c| c["type"] == "robot.articulated"));
            assert!(registered.iter().any(|c| c["parameters_complete"] == true
                && c["ports"].as_array().is_some_and(|p| !p.is_empty())));
            result
        })
        .collect::<Vec<_>>();
    assert_eq!(
        inspections[0]
            .robot
            .entities
            .iter()
            .filter(|e| e.category == "links")
            .count(),
        29
    );
    let wheeled = &inspections[1].robot;
    assert_eq!(
        wheeled
            .entities
            .iter()
            .filter(|e| e.category == "links")
            .count(),
        4
    );
    assert_eq!(
        wheeled
            .entities
            .iter()
            .filter(|e| e.category == "motors")
            .count(),
        2
    );
    let passive = wheeled
        .entities
        .iter()
        .find(|e| e.name == "passive axle")
        .unwrap();
    assert!(
        passive
            .properties
            .iter()
            .any(|p| p.name == "limits" && p.unit == "rad" && p.value == Some(Value::Null))
    );
    assert!(
        !wheeled
            .relations
            .iter()
            .any(|r| r.owner == passive.key && r.role == "motor")
    );
    assert_eq!(
        inspections[0].registered_robot_components,
        inspections[1].registered_robot_components
    );
}
