use serde_json::json;
use sim_domain_robot::{Articulated, Options};
use sim_runtime::walking_task::{WalkingMonitor, WalkingTaskConfig};

fn fixture() -> (Articulated, WalkingTaskConfig) {
    let model=serde_json::from_value(json!({"links":[{"name":"body"},{"name":"a"},{"name":"b"}],
        "joints":[{"name":"ba","parent":"body","child":"a","type":"revolute"},{"name":"bb","parent":"body","child":"b","type":"revolute"}]})).unwrap();
    let mut art = Articulated::new(
        std::sync::Arc::new(model),
        &Options {
            flex: false,
            ..Default::default()
        },
    )
    .unwrap();
    for l in &mut art.links {
        l.contact = vec![nalgebra::Vector3::zeros()];
    }
    let c = WalkingTaskConfig {
        version: 1,
        minimum_clearance_m: 0.001,
        maximum_swing_force_n: 0.1,
        minimum_support_force_n: 1.,
        qualifying_duration_s: 0.2,
        body_position_scale_m: 0.005,
        body_position_weight_per_s: 0.5,
        body_position_cost_cap: 1.0,
        qualified_step_reward: 0.1,
        failed_step_penalty: 1.,
    };
    (art, c)
}
fn frame(time: f64, phase: &str, support: f64) -> serde_json::Value {
    let pose = |name: &str, p: [f64; 3]| json!({"name":name,"position_m":p,"rotation":[[1,0,0],[0,1,0],[0,0,1]]});
    json!({"time_s":time,"poses":[pose("body",[0.003,0.004,0.]),pose("a",[0.,0.,0.002]),pose("b",[0.,0.,0.])],
        "contacts":[{"link":2,"other":null,"force_n":[0,0,support]}],
        "policy":{"step_reference":{"reference":{"sample":(time*10.)as u64,"step":1,"foot":0,"phase":phase,"body_world_m":[0,0,0]}}}})
}
#[test]
fn objective_checks_simultaneous_support_and_awards_each_completed_step_once() {
    let (art, c) = fixture();
    for fail in [false, true] {
        let mut m = WalkingMonitor::new(
            c.clone(),
            0.1,
            "body".into(),
            vec!["a".into(), "b".into()],
            &art,
        )
        .unwrap();
        assert_eq!(
            m.observe(&art, &json!({}), 0., false).unwrap().body_reward,
            0.
        );
        for i in 1..=3 {
            let f = frame(
                i as f64 * 0.1,
                if i == 3 { "lower" } else { "raise" },
                if fail && i == 2 { 0. } else { 2. },
            );
            let r = m.observe(&art, &f, 0.1, false).unwrap();
            assert!((r.body_reward + 0.05).abs() < 1e-12);
            assert!(r.outcome.is_none());
        }
        let r = m
            .observe(&art, &frame(0.4, "return", 2.), 0.1, false)
            .unwrap();
        assert_eq!(r.outcome.unwrap().passed, !fail);
        assert_eq!(r.step_reward, if fail { -1. } else { 0.1 });
        let again = m
            .observe(&art, &frame(0.5, "return", 2.), 0.1, false)
            .unwrap();
        assert_eq!(again.step_reward, 0.);
        assert!(again.outcome.is_none());
        assert_eq!(again.qualified_steps, usize::from(!fail));
        assert_eq!(again.failed_steps, usize::from(fail));
    }
}
#[test]
fn truncated_swing_is_not_rewarded_as_a_completed_step() {
    let (art, c) = fixture();
    let mut m =
        WalkingMonitor::new(c, 0.1, "body".into(), vec!["a".into(), "b".into()], &art).unwrap();
    for i in 1..=3 {
        let r = m
            .observe(&art, &frame(i as f64 * 0.1, "raise", 2.), 0.1, i == 3)
            .unwrap();
        if i == 3 {
            let o = r.outcome.unwrap();
            assert!(!o.complete && !o.passed);
            assert!(o.lift.unwrap().passed);
            assert_eq!(r.step_reward, -1.);
        }
    }
}
#[test]
fn invalid_scales_and_missing_geometry_are_rejected() {
    let (art, mut c) = fixture();
    c.body_position_scale_m = 0.;
    assert!(
        WalkingMonitor::new(c, 0.1, "body".into(), vec!["a".into(), "b".into()], &art).is_err()
    );
    let (_, c) = fixture();
    let mut m =
        WalkingMonitor::new(c, 0.1, "body".into(), vec!["a".into(), "b".into()], &art).unwrap();
    assert!(m.observe(&art, &json!({}), 0.1, false).is_err());
}

#[test]
fn far_body_errors_have_an_explicit_bounded_cost() {
    let (art, c) = fixture();
    let mut m =
        WalkingMonitor::new(c, 0.1, "body".into(), vec!["a".into(), "b".into()], &art).unwrap();
    let mut f = frame(0.1, "hold", 2.0);
    f["poses"][0]["position_m"] = json!([0.1, 0., 0.]);
    let r = m.observe(&art, &f, 0.1, false).unwrap();
    assert_eq!(r.body_error_world_m, [0.1, 0., 0.]);
    assert_eq!(r.body_reward, -0.05);
}
