use serde_json::{Value, json};
use sim_domain_control::motion_parameters::Values;
use sim_runtime::{
    embedded::Config,
    environment::{EmbeddedEnvironment, Task},
    motion_parameters::*,
    session::Scene,
};

fn fixture() -> (Scene, Config, Task, MotionParameterization, Values) {
    let config = serde_json::from_str(include_str!(
        "../../../examples/wheeled-robot/imu-policy.config.json"
    ))
    .unwrap();
    let task = serde_json::from_str(include_str!(
        "../../../examples/wheeled-robot/imu-policy.task.json"
    ))
    .unwrap();
    let robot: Value = serde_json::from_str(include_str!(
        "../../../examples/wheeled-robot/baseline/robot.simrobot.json"
    ))
    .unwrap();
    let inputs: Value = serde_json::from_str(include_str!(
        "../../../examples/wheeled-robot/velocity-controller.inputs.json"
    ))
    .unwrap();
    let scene=serde_json::from_value(json!({"version":1,"robot":robot,"options":{"contact":false,"flex":false},
        "period_s":0.003,"duration_s":0.03,"controller":{"inputs":inputs,
        "parameters":{"period_s":0.003,"initial_left":0.01,"initial_right":-0.02},
        "sources":{"entry":"velocity-controller.rhai","files":{"velocity-controller.rhai":include_str!("../../../examples/wheeled-robot/velocity-controller.rhai")}}}})).unwrap();
    let recipe=serde_json::from_value(json!({"version":1,"space":{"parameters":[
        {"name":"turn","kind":"AngularVelocity","bounds":[-0.5,0.5]}]},"trajectories":[],"commands":[
        {"input":"command.right_speed","kind":"AngularVelocity","scale":{"source":"constant","value":1},
        "center":{"source":"constant","value":0},"offset":{"source":"parameter","name":"turn"}}]})).unwrap();
    (scene, config, task, recipe, [("turn".into(), 0.)].into())
}
fn stable(mut frame: Value) -> Value {
    frame.as_object_mut().unwrap().remove("stepping_wall_s");
    frame
}

#[test]
fn variants_drive_original_cad_firmware_and_replay_exactly() {
    let (s, c, t, r, mut values) = fixture();
    let actions = vec![vec![0.1, -0.1]; 10];
    let original = serde_json::to_value(&s).unwrap();
    let identity = r.materialize(&s, &actions, &values).unwrap();
    assert_eq!(serde_json::to_value(identity.scene).unwrap(), original);
    assert_eq!(identity.actions, actions);
    values.insert("turn".into(), 0.2);
    let variant = r.materialize(&s, &actions, &values).unwrap();
    variant.validate().unwrap();
    let mut tampered = variant.clone();
    tampered.actions[0][1] += 0.01;
    assert!(tampered.validate().is_err());
    assert_eq!(serde_json::to_value(&s).unwrap(), original);
    let mut a = EmbeddedEnvironment::new(s, c.clone(), t.clone(), 7).unwrap();
    let mut b = EmbeddedEnvironment::new(variant.scene, c, t, 7).unwrap();
    for (a_row, b_row) in actions.iter().zip(&variant.actions) {
        a.step(a_row).unwrap();
        b.step(b_row).unwrap();
    }
    assert_ne!(a.transition().observations, b.transition().observations);
    let frame = stable(b.frame().unwrap());
    let (mut replay, rows) = b.prepare_replay(b.episode_recording()).unwrap();
    assert_eq!(rows, variant.actions);
    for row in &rows {
        replay.step(row).unwrap();
    }
    assert_eq!(stable(replay.frame().unwrap()), frame);
    assert_eq!(
        serde_json::to_value(replay.transition()).unwrap(),
        serde_json::to_value(b.transition()).unwrap()
    );
}

#[test]
fn command_names_survive_reordering_and_reject_units_bounds_and_unbound_parameters() {
    let (mut s, _, _, r, mut v) = fixture();
    v.insert("turn".into(), 0.2);
    let actions = vec![vec![0.1, -0.1]];
    let first = r.materialize(&s, &actions, &v).unwrap();
    s.controller.as_mut().unwrap().inputs.reverse();
    assert_eq!(
        r.materialize(&s, &[vec![-0.1, 0.1]], &v).unwrap().actions[0],
        first.actions[0].iter().copied().rev().collect::<Vec<_>>()
    );
    let before = serde_json::to_value(&s).unwrap();
    let mut bad = r.clone();
    bad.commands[0].kind = sim_core::QuantityKind::LinearVelocity;
    assert!(
        bad.materialize(&s, &actions, &v)
            .unwrap_err()
            .contains("kind")
    );
    let mut bad = r.clone();
    bad.commands[0].input = "absent".into();
    assert!(bad.materialize(&s, &actions, &v).is_err());
    let mut bad = r.clone();
    bad.commands.clear();
    assert!(
        bad.materialize(&s, &actions, &v)
            .unwrap_err()
            .contains("bound")
    );
    assert!(r.materialize(&s, &[vec![0.95, 0.]], &v).is_err());
    assert!(r.materialize(&s, &[vec![f64::NAN, 0.]], &v).is_err());
    assert_eq!(serde_json::to_value(&s).unwrap(), before);
}

#[test]
fn controller_reference_binding_checks_original_curve_and_named_index_map() {
    let (mut s, _, _, mut r, mut v) = fixture();
    let curve = json!({"interpolation":"linear","keyframes":[{"time_s":0,"values":[0,0]},{"time_s":1,"values":[0.2,0.3]}]});
    let controller = s.controller.as_mut().unwrap();
    controller.parameters["curve"] = curve.clone();
    controller.parameters["indices"] = json!({"a":0,"b":1});
    r.commands.clear();
    r.space.parameters[0].kind = sim_core::QuantityKind::Angle;
    r.trajectories=vec![serde_json::from_value(json!({"parameter":"curve","channel_indices_parameter":"indices",
        "template":{"channels":[{"name":"a","kind":"Angle"},{"name":"b","kind":"Angle"}],"reference":curve,
        "transforms":[{"operation":"affine","channels":["b"],"scale":{"source":"constant","value":1},
        "center":{"source":"constant","value":0},"offset":{"source":"parameter","name":"turn"}}]}})).unwrap()];
    v.insert("turn".into(), 0.1);
    let a = r.materialize(&s, &[vec![0., 0.]], &v).unwrap();
    assert_eq!(
        a.scene.controller.unwrap().parameters["curve"]["keyframes"][1]["values"][1],
        json!(0.4)
    );
    let mut bad = s.clone();
    bad.controller.as_mut().unwrap().parameters["indices"] = json!({"a":1,"b":0});
    assert!(r.materialize(&bad, &[], &v).unwrap_err().contains("index"));
    let mut bad = s.clone();
    bad.controller.as_mut().unwrap().parameters["curve"]["keyframes"][1]["values"][0] = json!(0.9);
    assert!(
        r.materialize(&bad, &[], &v)
            .unwrap_err()
            .contains("reference")
    );
    r.trajectories.push(r.trajectories[0].clone());
    assert!(r.materialize(&s, &[], &v).is_err());
}

fn scalar_recipe() -> MotionParameterization {
    serde_json::from_value(json!({"version":1,"space":{"parameters":[
        {"name":"time_factor","kind":"Dimensionless","bounds":[0.25,2]}]},"trajectories":[],"commands":[
        {"input":"command.left_speed","kind":"AngularVelocity","scale":{"source":"scaled","value":{"source":"constant","value":1},"factor":{"source":"parameter","name":"time_factor"},"power":-1},
        "center":{"source":"constant","value":0},"offset":{"source":"constant","value":0}}],
        "scalars":[{"pointer":"/initial_left","kind":"Angle","reference":0.01,"value":{"source":"scaled","value":{"source":"constant","value":0.01},"factor":{"source":"parameter","name":"time_factor"},"power":1}}],
        "checks":[{"name":"integration_clock","left":{"source":"controller_parameter","pointer":"/period_s","kind":"Time"},"right":{"source":"scene_period"},"tolerance":0}]})).unwrap()
}

#[test]
fn scalar_bindings_change_executed_wheel_controller_and_keep_sampling_clock() {
    let (s, c, t, _, _) = fixture();
    let r = scalar_recipe();
    let rows = vec![vec![0.1, -0.1]; 10];
    let mut v = [("time_factor".into(), 1.)].into();
    let identity = r.materialize(&s, &rows, &v).unwrap();
    identity.validate().unwrap();
    assert_eq!(
        serde_json::to_value(&identity.scene).unwrap(),
        serde_json::to_value(&s).unwrap()
    );
    assert_eq!(identity.actions, rows);
    v.insert("time_factor".into(), 0.5);
    let changed = r.materialize(&s, &rows, &v).unwrap();
    let roundtrip: MotionVariant =
        serde_json::from_slice(&serde_json::to_vec(&changed).unwrap()).unwrap();
    roundtrip.validate().unwrap();
    assert_eq!(changed.scene.period_s, s.period_s);
    assert_eq!(
        changed.scene.controller.as_ref().unwrap().parameters["period_s"],
        json!(0.003)
    );
    assert_eq!(
        changed.scene.controller.as_ref().unwrap().parameters["initial_left"],
        json!(0.005)
    );
    assert_eq!(changed.actions[0][0], 0.2);
    let mut a = EmbeddedEnvironment::new(identity.scene, c.clone(), t.clone(), 7).unwrap();
    let mut b = EmbeddedEnvironment::new(changed.scene, c, t, 7).unwrap();
    for (left, right) in rows.iter().zip(&changed.actions) {
        a.step(left).unwrap();
        b.step(right).unwrap();
    }
    assert_ne!(a.transition().observations, b.transition().observations);
    let (mut replay, rows) = b.prepare_replay(b.episode_recording()).unwrap();
    for row in &rows {
        replay.step(row).unwrap();
    }
    assert_eq!(stable(b.frame().unwrap()), stable(replay.frame().unwrap()));
    let mut bad = roundtrip.clone();
    bad.scene.controller.as_mut().unwrap().parameters["initial_left"] = json!(0.1);
    assert!(bad.validate().is_err());
    bad = roundtrip.clone();
    bad.source_scalars.clear();
    assert!(bad.validate().is_err());
    bad = roundtrip;
    bad.source_scalars
        .insert("/initial_left".into(), json!(0.2));
    assert!(bad.validate().is_err());
    let mut bad = s.clone();
    bad.controller.as_mut().unwrap().parameters["period_s"] = json!(0.0015);
    assert!(
        r.materialize(&bad, &rows, &v)
            .unwrap_err()
            .contains("integration_clock")
    );
}

#[test]
fn scalar_pointer_validation_and_identity_preserve_authored_number_types() {
    let (mut s, _, _, _, _) = fixture();
    let mut r = scalar_recipe();
    let v = [("time_factor".into(), 1.)].into();
    let rows = [vec![0., 0.]];
    s.controller.as_mut().unwrap().parameters["nested/name~"] = json!({"values":[0,-0.0,3]});
    for (i, reference) in [0., -0., 3.].into_iter().enumerate() {
        r.scalars.push(serde_json::from_value(json!({"pointer":format!("/nested~1name~0/values/{i}"),"kind":"Time","reference":reference,"integer":i != 1,
            "value":{"source":"constant","value":reference}})).unwrap());
    }
    let identity = r.materialize(&s, &rows, &v).unwrap();
    assert_eq!(
        serde_json::to_string(&identity.scene).unwrap(),
        serde_json::to_string(&s).unwrap()
    );
    identity.validate().unwrap();
    for pointer in [
        "",
        "initial_left",
        "/nested~2name",
        "/absent",
        "/nested~1name~0/values/01",
        "/nested~1name~0/values/-",
        "/nested~1name~0/values",
    ] {
        let mut bad = r.clone();
        bad.scalars[0].pointer = pointer.into();
        assert!(bad.materialize(&s, &rows, &v).is_err(), "{pointer}");
    }
    let mut bad = r.clone();
    bad.scalars.push(bad.scalars[0].clone());
    assert!(
        bad.materialize(&s, &rows, &v)
            .unwrap_err()
            .contains("overlapping")
    );
    let mut bad = r.clone();
    bad.scalars[0].reference = 0.2;
    assert!(
        bad.materialize(&s, &rows, &v)
            .unwrap_err()
            .contains("reference")
    );
    let mut bad = r.clone();
    bad.scalars[1].value = sim_domain_control::motion_parameters::Scalar::Constant { value: 0.5 };
    assert!(
        bad.materialize(&s, &rows, &v)
            .unwrap_err()
            .contains("integer")
    );
    let mut bad = r.clone();
    bad.checks[0].left = CheckedScalar::ControllerParameter {
        pointer: "/period_s".into(),
        kind: sim_core::QuantityKind::Angle,
    };
    assert!(bad.materialize(&s, &rows, &v).is_err());
    let mut bad = r.clone();
    bad.checks.push(bad.checks[0].clone());
    assert!(bad.materialize(&s, &rows, &v).is_err());
    let mut bad = s.clone();
    bad.controller.as_mut().unwrap().parameters["initial_left"] = json!(9007199254740993u64);
    assert!(
        r.materialize(&bad, &rows, &v)
            .unwrap_err()
            .contains("exact scalar")
    );
}

#[test]
fn coordinated_reference_period_checks_catch_partial_retiming_and_overlapping_writes() {
    let (mut s, _, _, _, _) = fixture();
    let curve = json!({"interpolation":"linear","keyframes":[{"time_s":0,"values":[0]},{"time_s":1,"values":[1]}]});
    s.controller.as_mut().unwrap().parameters["curve"] = curve.clone();
    s.controller.as_mut().unwrap().parameters["indices"] = json!({"joint":0});
    s.controller.as_mut().unwrap().parameters["cycle_s"] = json!(1.);
    let mut r = scalar_recipe();
    r.trajectories = vec![serde_json::from_value(json!({"parameter":"curve","channel_indices_parameter":"indices","template":{
        "channels":[{"name":"joint","kind":"Angle"}],"reference":curve,"transforms":[{"operation":"time_scale","factor":{"source":"parameter","name":"time_factor"}}]}})).unwrap()];
    r.scalars.push(serde_json::from_value(json!({"pointer":"/cycle_s","kind":"Time","reference":1,"value":{"source":"parameter","name":"time_factor"}})).unwrap());
    // Units must be explicit; a time parameter cannot directly receive a factor.
    let v = [("time_factor".into(), 0.5)].into();
    assert!(r.materialize(&s, &[vec![0., 0.]], &v).is_err());
    r.scalars.last_mut().unwrap().value = serde_json::from_value(json!({"source":"scaled","value":{"source":"constant","value":1},"factor":{"source":"parameter","name":"time_factor"},"power":1})).unwrap();
    r.checks.push(serde_json::from_value(json!({"name":"reference_cycle","left":{"source":"controller_parameter","pointer":"/cycle_s","kind":"Time"},
        "right":{"source":"controller_parameter","pointer":"/curve/keyframes/1/time_s","kind":"Time"},"tolerance":0})).unwrap());
    let good = r.materialize(&s, &[vec![0., 0.]], &v).unwrap();
    good.validate().unwrap();
    let mut bad = r.clone();
    bad.scalars.pop();
    assert!(
        bad.materialize(&s, &[vec![0., 0.]], &v)
            .unwrap_err()
            .contains("reference_cycle")
    );
    for pointer in ["/curve/keyframes/1/time_s", "/indices/joint"] {
        let mut bad = r.clone();
        bad.scalars.last_mut().unwrap().pointer = pointer.into();
        assert!(
            bad.materialize(&s, &[vec![0., 0.]], &v)
                .unwrap_err()
                .contains("overlapping")
        );
    }
}
