use serde_json::json;
use sim_domain_control::displacement::DisplacementAxes as Axes;
use sim_runtime::{
    environment::{Axis, Task, Transition},
    motion_response::{BodyBinding, BodySample, HeadingSample, summarize},
};

fn sample(t: f64, x: f64, v: f64) -> BodySample {
    BodySample {
        time_s: t,
        position_world_m: [x, 0., 0.],
        velocity_world_m_s: [v, 0., 0.],
        heading_world_z: None,
    }
}
#[test]
fn nonuniform_braking_and_closed_path_are_measured_without_reward_changes() {
    let r = summarize(
        &[sample(0., 0., 2.), sample(1., 1.5, 1.), sample(3., 2.5, 0.)],
        Axes::Xy,
    )
    .unwrap();
    assert_eq!(r.net_distance_m, 2.5);
    assert_eq!(r.maximum_interval_acceleration_m_s2, 1.);
    assert!((r.mean_speed_m_s - 2.5 / 3.).abs() < 1e-14);
    assert_eq!(r.end_speed_m_s, 0.);
    assert!(r.integrated_heading_change_rad.is_none());
    let r = summarize(
        &[sample(0., 0., 1.), sample(1., 1., -1.), sample(2., 0., -1.)],
        Axes::Xy,
    )
    .unwrap();
    assert_eq!(r.net_distance_m, 0.);
    assert_eq!(r.mean_speed_m_s, 1.);
    let mut bad = vec![sample(0., 0., 0.), sample(0., 0., 1.)];
    assert!(summarize(&bad, Axes::X).is_err());
    bad[1].time_s = 1.;
    bad[1].velocity_world_m_s[0] = f64::NAN;
    assert!(summarize(&bad, Axes::X).is_err());
}
#[test]
fn heading_integral_preserves_multiple_turns_and_missing_data() {
    let mut s = vec![sample(0., 0., 0.), sample(1., 0., 0.), sample(2., 0., 0.)];
    for v in &mut s {
        v.heading_world_z = Some(HeadingSample {
            angle_rad: 0.,
            rate_rad_s: std::f64::consts::TAU,
        });
    }
    assert_eq!(
        summarize(&s, Axes::Xy)
            .unwrap()
            .integrated_heading_change_rad,
        Some(2. * std::f64::consts::TAU)
    );
    // Orientation-only sampling aliases complete turns between samples; keep
    // this diagnostic separate from integrating the sampled angular velocity.
    assert_eq!(
        summarize(&s, Axes::Xy)
            .unwrap()
            .sampled_unwrapped_heading_change_rad,
        Some(0.)
    );
    for (v, degrees) in s.iter_mut().zip([170_f64, -170., -150.]) {
        v.heading_world_z.as_mut().unwrap().angle_rad = degrees.to_radians();
    }
    let measured = summarize(&s, Axes::Xy).unwrap();
    assert!(
        (measured.sampled_unwrapped_heading_change_rad.unwrap() - 40_f64.to_radians()).abs()
            < 1e-12
    );
    assert!((measured.wrapped_heading_change_rad.unwrap() - 40_f64.to_radians()).abs() < 1e-12);
    assert!(
        (measured.maximum_sampled_heading_increment_rad.unwrap() - 20_f64.to_radians()).abs()
            < 1e-12
    );
    s[1].heading_world_z = None;
    assert!(
        summarize(&s, Axes::Xy)
            .unwrap()
            .integrated_heading_change_rad
            .is_none()
    );
    assert!(
        summarize(&s, Axes::Xy)
            .unwrap()
            .sampled_unwrapped_heading_change_rad
            .is_none()
    );
}
#[test]
fn semantic_bindings_survive_channel_reordering_and_check_missing_sources_and_aliases() {
    let mut observations = vec![];
    let mut values = vec![];
    for (kind, v) in [
        ("body_position", [1., 2., 3.]),
        ("body_velocity", [4., 5., 6.]),
        ("body_angular_velocity", [0.3, 0., 2.]),
    ] {
        for (i, a) in ["x", "y", "z"].iter().enumerate() {
            observations.push(json!({"name":format!("{kind}-{a}"),"source":{"kind":kind,"link":"arbitrary body","axis":a}}));
            values.push(v[i]);
        }
    }
    for (i, a) in ["x", "y", "z"].iter().enumerate() {
        observations.push(json!({"name":format!("forward-{a}"),"source":{"kind":"body_axis","link":"arbitrary body","body_axis":"x","world_axis":a}}));
        values.push([0.6, 0., 0.8][i]);
    }
    let mut task:Task=serde_json::from_value(json!({"version":1,"period_s":0.02,"observation_source":"ideal_runtime_teacher_only","observations":observations,"rewards":[],"termination_bounds":[]})).unwrap();
    let mut t:Transition=serde_json::from_value(json!({"time_s":1.,"elapsed_s":0.02,"observations":values,"reward":0.,"reward_terms":[],"terminated":false,"truncated":false,"termination_reasons":[],"completed_steps":1})).unwrap();
    let s = BodyBinding::new(&task, "arbitrary body", Some(Axis::X))
        .unwrap()
        .sample(&t)
        .unwrap();
    assert_eq!(s.velocity_world_m_s, [4., 5., 6.]);
    assert!((s.heading_world_z.unwrap().rate_rad_s - 1.6).abs() < 1e-14);
    task.observations.reverse();
    t.observations.reverse();
    assert_eq!(
        BodyBinding::new(&task, "arbitrary body", Some(Axis::X))
            .unwrap()
            .sample(&t)
            .unwrap()
            .position_world_m,
        [1., 2., 3.]
    );
    task.observations.remove(0);
    t.observations.remove(0);
    assert!(BodyBinding::new(&task, "arbitrary body", Some(Axis::X)).is_err());
    assert!(BodyBinding::new(&task, "arbitrary body", None).is_ok());
    task.observations
        .push(task.observations.last().unwrap().clone());
    t.observations.push(*t.observations.last().unwrap());
    let binding = BodyBinding::new(&task, "arbitrary body", None).unwrap();
    assert_eq!(binding.sample(&t).unwrap().position_world_m, [1., 2., 3.]);
    *t.observations.last_mut().unwrap() = 99.;
    assert!(binding.sample(&t).is_err());
}
