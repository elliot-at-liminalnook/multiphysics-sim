use sim_core::{Channel, Contract, Coupler, QuantityKind};
use sim_script::{RhaiController, Sources, parameter_map};

#[test]
fn state_snapshot_is_detached_and_does_not_advance_or_change_control() {
    let mut c = RhaiController::new(
        Sources::single(
            "state.rhai",
            r#"
        fn control(t,s,a,state) {
            if !state.contains("n") { state.n=0; }
            state.n+=1; a.out=state.n.to_float();
            #{commands:a,state:state}
        }
    "#,
        ),
        parameter_map(&serde_json::json!({})).unwrap(),
    )
    .unwrap();
    let contract = Contract {
        element: "test".into(),
        period: 0.02,
        sensors: vec![],
        actuators: vec![Channel {
            name: "out".into(),
            kind: QuantityKind::Dimensionless,
        }],
    };
    c.open(&contract).unwrap();
    let mut out = [0.0];
    c.sample(0.0, &[], &mut out).unwrap();
    let mut snapshot = c.state_json().unwrap();
    assert_eq!(snapshot["n"], 1);
    snapshot["n"] = serde_json::json!(99);
    assert_eq!(c.state_json().unwrap()["n"], 1);
    c.sample(0.02, &[], &mut out).unwrap();
    assert_eq!(out, [2.0]);
    c.open(&contract).unwrap();
    assert_eq!(c.state_json().unwrap(), serde_json::json!({}));
}
