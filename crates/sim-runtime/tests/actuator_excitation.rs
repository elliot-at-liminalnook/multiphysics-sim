use serde_json::json;
use sim_core::{Channel, Contract, Coupler, QuantityKind as Q};
use sim_script::{RhaiController, Sources, parameter_map};

#[test]
fn authored_excitation_is_independent_of_sensors_and_obeys_stop_expiry_and_reset() {
    let source = format!(
        "{}\n{}",
        r#"fn excitation_baseline_control(t,sensors,commands,state) {
            if !state.contains("calls") { state.calls = 0; }
            state.calls += 1;
            #{commands: #{motor: sensors["measured"]}, state: state}
        }"#,
        include_str!("../../../examples/interactive/actuator_excitation_adapter.rhai"),
    );
    let parameters = json!({
        "excitation_actuator_names":["motor"], "excitation_forward_input":"forward",
        "excitation_lateral_input":"lateral", "excitation_sequence_input":"sequence",
        "command_lease":{"period_s":0.02,"timeout_s":0.04},
        "excitation_targets":{"keyframes":[
            {"time_s":0.,"values":[0.2]}, {"time_s":0.04,"values":[0.4]}
        ]}
    });
    let contract = Contract {
        element: "actuator_excitation_test".into(),
        period: 0.02,
        sensors: [
            ("measured", Q::Angle),
            ("forward", Q::LinearVelocity),
            ("lateral", Q::LinearVelocity),
            ("sequence", Q::Dimensionless),
        ]
        .into_iter()
        .map(|(name, kind)| Channel {
            name: name.into(),
            kind,
        })
        .collect(),
        actuators: vec![Channel {
            name: "motor".into(),
            kind: Q::Angle,
        }],
    };
    let make = |parameters: &serde_json::Value| {
        let mut controller = RhaiController::new(
            Sources::single("excitation.rhai", &source),
            parameter_map(parameters).unwrap(),
        )
        .unwrap();
        controller.open(&contract).unwrap();
        controller
    };
    let mut a = make(&parameters);
    let mut b = make(&parameters);
    let mut output_a = [0.];
    let mut output_b = [0.];
    for (i, sequence) in [1., 2., 2.].into_iter().enumerate() {
        a.sample(i as f64 * 0.02, &[0.5, 1., 0., sequence], &mut output_a)
            .unwrap();
        b.sample(i as f64 * 0.02, &[-0.5, 1., 0., sequence], &mut output_b)
            .unwrap();
        assert_eq!(output_a, output_b);
        assert!((output_a[0] - (0.2 + i as f64 * 0.1)).abs() < 1e-14);
    }
    a.sample(0.06, &[0.5, 1., 0., 2.], &mut output_a).unwrap();
    assert_eq!(output_a, [0.5]); // expired packet returns to baseline
    a.sample(0.08, &[0.5, 0., 0., 3.], &mut output_a).unwrap();
    assert_eq!(output_a, [0.5]); // explicit stop returns to baseline
    a.sample(0.10, &[0.5, 1., 0., 4.], &mut output_a).unwrap();
    assert_eq!(output_a, [0.4]);
    assert_eq!(a.state_json().unwrap()["calls"], 6);
    let mut replay = make(&parameters);
    for (i, (forward, sequence)) in [(1., 1.), (1., 2.), (1., 2.), (1., 2.), (0., 3.), (1., 4.)]
        .into_iter()
        .enumerate()
    {
        replay
            .sample(
                i as f64 * 0.02,
                &[0.5, forward, 0., sequence],
                &mut output_b,
            )
            .unwrap();
    }
    assert_eq!(a.state_json().unwrap(), replay.state_json().unwrap());
    a.open(&contract).unwrap();
    a.sample(0., &[0.5, 1., 0., 1.], &mut output_a).unwrap();
    assert_eq!(output_a, [0.2]);
    assert_eq!(a.state_json().unwrap()["calls"], 1);
    let mut wrong = parameters.clone();
    wrong["excitation_actuator_names"] = json!(["missing"]);
    assert!(
        make(&wrong)
            .sample(0., &[0.5, 1., 0., 1.], &mut output_a)
            .is_err()
    );
}
