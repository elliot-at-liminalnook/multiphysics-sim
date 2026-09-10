use sim_core::{Channel, Contract, Coupler, QuantityKind};
use sim_script::{RhaiController, Sources, parameter_map};

#[test]
fn parameterized_references_use_the_shared_typed_materializer() {
    let parameters=serde_json::json!({"template":{"channels":[{"name":"q","kind":"Angle"}],
        "reference":{"interpolation":"linear","keyframes":[{"time_s":0,"values":[0]},{"time_s":1,"values":[1]}]},
        "transforms":[{"operation":"affine","channels":["q"],"scale":{"source":"parameter","name":"gain"},
            "center":{"source":"constant","value":0},"offset":{"source":"constant","value":0}},
            {"operation":"time_scale","factor":{"source":"scaled","value":{"source":"constant","value":1},"factor":{"source":"parameter","name":"duration"},"power":1}}]},
        "space":{"parameters":[{"name":"gain","kind":"Dimensionless","bounds":[-3,3]},
            {"name":"duration","kind":"Dimensionless","bounds":[0.1,2]}]},"values":{"gain":-2,"duration":0.5}});
    let source=Sources::single("motion.rhai",r#"
fn control(t,s,a,state) {
    let p=parameters();
    let curve=parameterized_trajectory(p.template,p.space,p.values);
    a.q=trajectory_sample(curve,t).values[0];
    #{commands:a,state:state}
}"#);
    let contract=Contract {element:"motion".into(),period:0.1,sensors:vec![],
        actuators:vec![Channel {name:"q".into(),kind:QuantityKind::Angle}]};
    let mut controller=RhaiController::new(source.clone(),parameter_map(&parameters).unwrap()).unwrap();
    controller.open(&contract).unwrap();
    let mut output=[0.];controller.sample(0.25,&[],&mut output).unwrap();assert_eq!(output,[-1.]);
    let mut invalid=parameters;invalid["space"]["parameters"][0]["kind"]=serde_json::json!("Angle");
    let mut controller=RhaiController::new(source,parameter_map(&invalid).unwrap()).unwrap();controller.open(&contract).unwrap();
    assert!(controller.sample(0.25,&[],&mut output).is_err());assert_eq!(output,[-1.]);
}

#[test]
fn named_trajectories_match_shared_samples_and_are_immutable_derived_state() {
    let curve = serde_json::json!({"interpolation":"periodic_cubic_b_spline","keyframes":[
        {"time_s":0,"values":[0]},{"time_s":1,"values":[1]},
        {"time_s":2,"values":[0]},{"time_s":3,"values":[-1]},
        {"time_s":4,"values":[0]}]});
    let parameters = serde_json::json!({"curve":curve,"gain":2});
    let expected = sim_domain_control::trajectory::Trajectory::new(
        serde_json::from_value(curve.clone()).unwrap(),
    )
    .unwrap();
    let source = Sources::single(
        "named.rhai",
        r#"
fn control(t,s,a,state) {
    let small=parameters_except(["curve"]);
    if small.contains("curve") || !parameter_exists("curve") {throw "projection changed";}
    let local=parameters(); local.curve.keyframes[1].values[0]=999.0;
    let sample=trajectory_parameter_sample("curve",t);
    a.q=small.gain*sample.values[0];a.v=sample.rates[0];a.acc=sample.accelerations[0];
    #{commands:a,state:state}
}"#,
    );
    let contract = Contract {
        element: "test".into(),
        period: 0.02,
        sensors: vec![],
        actuators: [
            ("q", QuantityKind::Angle),
            ("v", QuantityKind::AngularVelocity),
            ("acc", QuantityKind::Dimensionless),
        ]
        .into_iter()
        .map(|(name, kind)| Channel {
            name: name.into(),
            kind,
        })
        .collect(),
    };
    // A fresh controller reconstructs the same derived cache. Neither cache
    // population nor mutation of a returned parameter copy changes script state.
    for _ in 0..2 {
        let mut controller =
            RhaiController::new(source.clone(), parameter_map(&parameters).unwrap()).unwrap();
        controller.open(&contract).unwrap();
        let mut output = [0.0; 3];
        for t in [0.0, 0.02, 0.7, 1.5, 4.0, 7.3] {
            controller.sample(t, &[], &mut output).unwrap();
            let sample = expected.sample(t).unwrap();
            assert_eq!(
                output,
                [
                    2.0 * sample.values[0],
                    sample.rates[0],
                    sample.accelerations[0]
                ]
            );
            assert_eq!(controller.state_json().unwrap(), serde_json::json!({}));
        }
        let before = output;
        assert!(controller.sample(-1.0, &[], &mut output).is_err());
        assert_eq!(output, before);
    }
    for code in [
        "let sample=trajectory_parameter_sample(\"missing\",t);",
        "let sample=trajectory_parameter_sample(\"gain\",t);",
        "let sample=parameters_except([123]);",
    ] {
        let source = Sources::single(
            "invalid.rhai",
            &format!("fn control(t,s,a,state) {{{code} #{{commands:a,state:state}}}}"),
        );
        let mut controller =
            RhaiController::new(source, parameter_map(&parameters).unwrap()).unwrap();
        controller.open(&contract).unwrap();
        let mut output = [7.0; 3];
        assert!(controller.sample(0.0, &[], &mut output).is_err());
        assert_eq!(output, [7.0; 3]);
    }
}

#[test]
fn trajectory_samples_shared_derivatives_without_state_objects() {
    let recipe = serde_json::json!({
        "interpolation":"periodic_cubic_b_spline",
        "keyframes":[
            {"time_s":0,"values":[0]}, {"time_s":1,"values":[1]},
            {"time_s":2,"values":[0]}, {"time_s":3,"values":[-1]},
            {"time_s":4,"values":[0]}
        ]
    });
    let source = Sources::single(
        "reference.rhai",
        r#"
fn control(t,s,a,state) {
    let sample=trajectory_sample(parameters(),t);
    a.q=sample.values[0];a.v=sample.rates[0];a.acc=sample.accelerations[0];
    #{commands:a,state:state}
}"#,
    );
    let mut controller =
        RhaiController::new(source.clone(), parameter_map(&recipe).unwrap()).unwrap();
    controller
        .open(&Contract {
            element: "test".into(),
            period: 0.02,
            sensors: vec![],
            actuators: vec![
                Channel {
                    name: "q".into(),
                    kind: QuantityKind::Angle,
                },
                Channel {
                    name: "v".into(),
                    kind: QuantityKind::AngularVelocity,
                },
                Channel {
                    name: "acc".into(),
                    kind: QuantityKind::Dimensionless,
                },
            ],
        })
        .unwrap();
    let mut out = [0.; 3];
    controller.sample(0., &[], &mut out).unwrap();
    assert_eq!(out, [0., 1., 0.]);
    controller.sample(4., &[], &mut out).unwrap();
    assert_eq!(out, [0., 1., 0.]);
    assert_eq!(controller.state_json().unwrap(), serde_json::json!({}));
    assert!(controller.sample(-1., &[], &mut out).is_err());
    assert_eq!(out, [0., 1., 0.]);
    let mut invalid = recipe;
    invalid["keyframes"][4]["values"][0] = serde_json::json!(1);
    let mut invalid_controller =
        RhaiController::new(source, parameter_map(&invalid).unwrap()).unwrap();
    invalid_controller
        .open(&Contract {
            element: "invalid".into(),
            period: 0.02,
            sensors: vec![],
            actuators: vec![
                Channel {
                    name: "q".into(),
                    kind: QuantityKind::Angle,
                },
                Channel {
                    name: "v".into(),
                    kind: QuantityKind::AngularVelocity,
                },
                Channel {
                    name: "acc".into(),
                    kind: QuantityKind::Dimensionless,
                },
            ],
        })
        .unwrap();
    assert!(invalid_controller.sample(0., &[], &mut out).is_err());
    assert_eq!(out, [0., 1., 0.]);
}
