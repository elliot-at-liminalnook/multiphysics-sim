use sim_core::{Channel, Contract, Coupler, QuantityKind};
use sim_domain_control::reference_governor::{Config, State};
use sim_script::{RhaiController, Sources, parameter_map};
fn config() -> Config {
    Config {
        period_s: 0.02,
        maximum_speed_rad_s: 1.,
        maximum_acceleration_rad_s2: 5.,
        response_rate_per_s: 10.,
    }
}
fn controller(parameters: serde_json::Value) -> RhaiController {
    let source = Sources::single(
        "bounded.rhai",
        r#"
fn control(t,s,a,state) {
    if !state.contains("angle_rad") {state=#{angle_rad:0.0,velocity_rad_s:0.0};}
    state=reference_governor_update(state.angle_rad,state.velocity_rad_s,s.desired,parameters());
    a.angle=state.angle_rad;
    #{commands:a,state:state}
}"#,
    );
    let mut c = RhaiController::new(source, parameter_map(&parameters).unwrap()).unwrap();
    c.open(&Contract {
        element: "test".into(),
        period: 0.02,
        sensors: vec![Channel {
            name: "desired".into(),
            kind: QuantityKind::Angle,
        }],
        actuators: vec![Channel {
            name: "angle".into(),
            kind: QuantityKind::Angle,
        }],
    })
    .unwrap();
    c
}
#[test]
fn same_kernel_bounded_reversal_and_replay() {
    let p = serde_json::to_value(config()).unwrap();
    let mut c = controller(p.clone());
    let mut replay = controller(p);
    let mut state = State::default();
    let (mut out, mut again) = ([0.], [0.]);
    for i in 0..300 {
        let target = if i < 60 {
            1.
        } else if i < 150 {
            -1.
        } else {
            0.
        };
        let next = config().update(state, target).unwrap();
        c.sample(i as f64 * 0.02, &[target], &mut out).unwrap();
        replay
            .sample(i as f64 * 0.02, &[target], &mut again)
            .unwrap();
        assert_eq!(out, again);
        assert_eq!(out[0].to_bits(), next.angle_rad.to_bits());
        assert!(next.velocity_rad_s.abs() <= 1.);
        assert!((next.velocity_rad_s - state.velocity_rad_s).abs() <= 0.10000000001);
        state = next;
    }
}
#[test]
fn invalid_update_rolls_back_and_unknown_parameters_fail() {
    let mut p = serde_json::to_value(config()).unwrap();
    p["response_rate_per_s"] = serde_json::json!(10);
    let mut c = controller(p.clone());
    let mut a = [0.];
    c.sample(0., &[1.], &mut a).unwrap();
    let before = a;
    assert!(c.sample(0.02, &[f64::NAN], &mut a).is_err());
    assert_eq!(a, before);
    c.sample(0.02, &[1.], &mut a).unwrap();
    let s = config().update(State::default(), 1.).unwrap();
    assert_eq!(a[0], config().update(s, 1.).unwrap().angle_rad);
    p["typo"] = serde_json::json!(1);
    let mut bad = controller(p);
    let mut a = [0.25];
    assert!(bad.sample(0., &[1.], &mut a).is_err());
    assert_eq!(a, [0.25]);
}
