use sim_core::{Channel, Contract, Coupler, QuantityKind};
use sim_script::{RhaiController, Sources, parameter_map};

#[test]
fn scripts_consume_shared_independent_contact_phases_and_world_units() {
    let recipe = serde_json::json!({
        "period_s":1,"displacement_world_m":[0.1,0,0],
        "body":{"interpolation":"periodic_cubic_b_spline",
            "keyframes":(0..=4).map(|i| serde_json::json!({"time_s":i as f64/4.0,
                "values":[0,0,0,0,0,0]})).collect::<Vec<_>>()},
        "feet":[
            {"center_world_m":[0,0,0],"phase_offset":0.17,"stance_fraction":0.6,
                "swing_offset_world_m":[0,0,0.02]},
            {"center_world_m":[0,1,0],"phase_offset":0.67,"stance_fraction":0.6,
                "swing_offset_world_m":[0,0,0.02]}
        ]
    });
    let source = Sources::single(
        "contact.rhai",
        r#"
fn control(t,s,a,state) {
    let sample=contact_phase_sample(parameters(),t);
    a.first=if sample.feet[0].in_contact {1.0}else{0.0};
    a.second=if sample.feet[1].in_contact {1.0}else{0.0};
    a.velocity=sample.feet[0].velocity_world_m_s[0];
    a.body=sample.body.values[0];
    #{commands:a,state:state}
}"#,
    );
    let contract = Contract {
        element: "test".into(),
        period: 0.02,
        sensors: vec![],
        actuators: [
            ("first", QuantityKind::Dimensionless),
            ("second", QuantityKind::Dimensionless),
            ("velocity", QuantityKind::LinearVelocity),
            ("body", QuantityKind::Length),
        ]
        .into_iter()
        .map(|(name, kind)| Channel {
            name: name.into(),
            kind,
        })
        .collect(),
    };
    let mut controller =
        RhaiController::new(source.clone(), parameter_map(&recipe).unwrap()).unwrap();
    controller.open(&contract).unwrap();
    let mut output = [0.0; 4];
    controller.sample(0.5, &[], &mut output).unwrap();
    assert_eq!(output, [1.0, 0.0, 0.0, 0.05]);
    controller.sample(0.85, &[], &mut output).unwrap();
    assert_eq!(&output[..2], &[0.0, 1.0]);
    assert!((output[2] - 0.192).abs() < 1e-12);
    assert!((output[3] - 0.085).abs() < 1e-12);
    controller.sample(-0.15, &[], &mut output).unwrap();
    assert_eq!(&output[..2], &[0.0, 1.0]);
    assert!((output[3] + 0.015).abs() < 1e-12);
    let mut smooth = recipe.clone();
    smooth["feet"][0]["return_ramp_fraction"] = serde_json::json!(0.25);
    let mut smooth_controller = RhaiController::new(source.clone(), parameter_map(&smooth).unwrap()).unwrap();
    smooth_controller.open(&contract).unwrap();
    smooth_controller.sample(0.97, &[], &mut output).unwrap();
    assert!((output[2] - 1.0 / 3.0).abs() < 1e-12);
    assert!((output[3] - 0.097).abs() < 1e-12);
    smooth["feet"][0]["return_ramp_fraction"] = serde_json::json!(0.6);
    let mut invalid_return = RhaiController::new(source.clone(), parameter_map(&smooth).unwrap()).unwrap();
    invalid_return.open(&contract).unwrap();
    let before_return = output;
    assert!(invalid_return.sample(0.0, &[], &mut output).is_err());
    assert_eq!(output, before_return);
    let mut multi = recipe.clone();
    multi["feet"][0]["stance_fraction"] = serde_json::json!(0.2);
    multi["feet"][0]["additional_steps"] = serde_json::json!([
        {"center_world_m":[0.03,0,0],"phase_offset":0.6,"stance_fraction":0.1,
         "swing_offset_world_m":[0,0.02,0.03]}
    ]);
    let mut sequence = RhaiController::new(source.clone(), parameter_map(&multi).unwrap()).unwrap();
    sequence.open(&contract).unwrap();
    sequence.sample(0.65, &[], &mut output).unwrap();
    assert_eq!(output[0], 1.0);
    assert_eq!(output[2], 0.0);
    sequence.sample(0.5, &[], &mut output).unwrap();
    assert_eq!(output[0], 0.0);
    assert!(output[2] > 0.0);
    let mut invalid = recipe;
    invalid["feet"][0]["stance_fraction"] = serde_json::json!(1);
    let mut bad = RhaiController::new(source, parameter_map(&invalid).unwrap()).unwrap();
    bad.open(&contract).unwrap();
    let before = output;
    assert!(bad.sample(0.0, &[], &mut output).is_err());
    assert_eq!(output, before);
}
