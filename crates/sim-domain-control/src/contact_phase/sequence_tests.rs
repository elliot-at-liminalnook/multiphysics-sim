use super::*;
use crate::trajectory::Keyframe;

fn config() -> ContactPhaseConfig {
    ContactPhaseConfig {
        period_s: 1.0,
        displacement_world_m: [0.2, 0.1, 0.0],
        body: TrajectoryConfig {
            interpolation: Interpolation::PeriodicCubicBSpline,
            keyframes: (0..=4)
                .map(|i| Keyframe {
                    time_s: i as f64 / 4.0,
                    values: vec![0.0; 6],
                })
                .collect(),
        },
        feet: vec![FootPhase {
            center_world_m: [0.0, 0.0, 0.0],
            phase_offset: 0.1,
            stance_fraction: 0.2,
            swing_offset_world_m: [0.01, 0.02, 0.03],
            return_ramp_fraction: None,
            additional_steps: vec![FootStep {
                center_world_m: [0.03, -0.02, 0.0],
                phase_offset: 0.6,
                stance_fraction: 0.1,
                swing_offset_world_m: [-0.02, 0.01, 0.04],
                return_ramp_fraction: Some(0.25),
            }],
        }],
    }
}

#[test]
fn rounded_touchdown_boundaries_remain_finite_and_close_at_the_next_foothold() {
    let mut c = config();
    c.period_s = 1.2669633915378704;
    for (i, k) in c.body.keyframes.iter_mut().enumerate() {
        k.time_s = c.period_s * i as f64 / 4.0;
    }
    c.feet[0].phase_offset = 0.23406583485052723;
    c.feet[0].stance_fraction = 0.2937689852336821;
    c.feet[0].additional_steps[0].phase_offset = 0.7340658348505272;
    c.feet[0].additional_steps[0].stance_fraction = 0.2937689852336821;
    let m = ContactPhaseMotion::new(c.clone()).unwrap();
    // This real search input formerly rounded the normalized swing endpoint
    // to 1.0000000000000002 and failed the SmoothReturn input contract.
    m.sample(0.29655284396536696).unwrap();
    for cycle in -2..=2 {
        let t = (cycle as f64 + c.feet[0].phase_offset) * c.period_s;
        for time in [
            t.next_down().next_down(),
            t.next_down(),
            t,
            t.next_up(),
            t.next_up().next_up(),
        ] {
            let f = m.sample(time).unwrap().feet.remove(0);
            for axis in 0..3 {
                let expected = c.feet[0].center_world_m[axis]
                    + c.displacement_world_m[axis]
                        * (cycle as f64 + c.feet[0].phase_offset + 0.5 * c.feet[0].stance_fraction);
                assert!((f.position_world_m[axis] - expected).abs() < 1e-12);
                assert!(f.velocity_world_m_s[axis].abs() < 1e-10);
                assert!(f.acceleration_world_m_s2[axis].abs() < 1e-7);
            }
        }
    }
    // Direct out-of-domain component inputs still fail; this is not a change
    // to the component's accepted phase range or any physical constraint.
    assert!(
        SmoothReturn::new(0.25)
            .unwrap()
            .sample(1.0_f64.next_up())
            .is_err()
    );
}

#[test]
fn two_independent_footholds_are_stationary_and_advance_by_common_cycle() {
    let motion = ContactPhaseMotion::new(config()).unwrap();
    // Independently calculated positions: first = D*0.2;
    // second = [0.03,-0.02,0] + D*0.65.
    for (time, expected) in [(0.15, [0.04, 0.02, 0.0]), (0.65, [0.16, 0.045, 0.0])] {
        for dt in [-0.02, 0.0, 0.02] {
            let foot = motion.sample(time + dt).unwrap().feet.remove(0);
            assert!(foot.in_contact);
            assert_eq!(foot.velocity_world_m_s, [0.0; 3]);
            assert_eq!(foot.acceleration_world_m_s2, [0.0; 3]);
            for axis in 0..3 {
                assert!((foot.position_world_m[axis] - expected[axis]).abs() < 1e-14);
            }
        }
    }
    for time in [-2.97, -0.4, 0.2, 0.45, 0.8, 1.07] {
        let a = motion.sample(time).unwrap();
        let b = motion.sample(time + 1.0).unwrap();
        assert_eq!(a.feet[0].in_contact, b.feet[0].in_contact);
        for axis in 0..3 {
            assert!(
                (b.feet[0].position_world_m[axis]
                    - a.feet[0].position_world_m[axis]
                    - [0.2, 0.1, 0.0][axis])
                    .abs()
                    < 1e-13
            );
        }
    }
}

#[test]
fn multiple_step_derivatives_close_at_every_event_and_reverse_clock() {
    let motion = ContactPhaseMotion::new(config()).unwrap();
    let h = 1e-6;
    for time in [-0.05, 0.1, 0.3, 0.41, 0.6, 0.7, 0.81, 1.0, 1.1] {
        let a = motion.sample(time - h).unwrap();
        let b = motion.sample(time).unwrap();
        let c = motion.sample(time + h).unwrap();
        let reverse = motion.sample_retimed(time, -1.7, 0.3).unwrap();
        for axis in 0..3 {
            assert!(
                ((c.feet[0].position_world_m[axis] - a.feet[0].position_world_m[axis]) / (2.0 * h)
                    - b.feet[0].velocity_world_m_s[axis])
                    .abs()
                    < 1e-7
            );
            assert!(
                ((c.feet[0].velocity_world_m_s[axis] - a.feet[0].velocity_world_m_s[axis])
                    / (2.0 * h)
                    - b.feet[0].acceleration_world_m_s2[axis])
                    .abs()
                    < 2e-3
            );
            assert!(
                (reverse.feet[0].velocity_world_m_s[axis]
                    + 1.7 * b.feet[0].velocity_world_m_s[axis])
                    .abs()
                    < 1e-12
            );
            assert!(
                (reverse.feet[0].acceleration_world_m_s2[axis]
                    - (2.89 * b.feet[0].acceleration_world_m_s2[axis]
                        + 0.3 * b.feet[0].velocity_world_m_s[axis]))
                    .abs()
                    < 1e-12
            );
        }
    }
}

#[test]
fn all_contact_intervals_and_midpoints_cover_unequal_step_counts() {
    let mut config = config();
    let mut other = config.feet[0].clone();
    other.additional_steps.clear();
    other.phase_offset = 0.9;
    other.stance_fraction = 0.3;
    config.feet.push(other);
    let motion = ContactPhaseMotion::new(config).unwrap();
    assert_eq!(motion.phase_midpoints().len(), 6);
    let intervals = motion.contact_intervals();
    assert_eq!(intervals.len(), 6);
    assert!(
        (intervals
            .iter()
            .map(|i| i.end_phase - i.start_phase)
            .sum::<f64>()
            - 1.0)
            .abs()
            < 1e-14
    );
    for interval in intervals {
        let contacts = |u| {
            motion
                .sample(interval.start_phase + u * (interval.end_phase - interval.start_phase))
                .unwrap()
                .feet
                .iter()
                .map(|f| f.in_contact)
                .collect::<Vec<_>>()
        };
        assert_eq!(contacts(0.001), contacts(0.999));
    }
    let mut wrapped = config_for_wrapped_stance();
    let motion = ContactPhaseMotion::new(wrapped.clone()).unwrap();
    assert!(motion.sample(0.01).unwrap().feet[0].in_contact);
    assert!(!motion.sample(0.15).unwrap().feet[0].in_contact);
    wrapped.feet[0].additional_steps[0].stance_fraction = 0.5;
    assert!(ContactPhaseMotion::new(wrapped).is_err());
}

fn config_for_wrapped_stance() -> ContactPhaseConfig {
    let mut c = config();
    c.feet[0].phase_offset = 0.9;
    c.feet[0].stance_fraction = 0.2;
    c.feet[0].additional_steps[0].phase_offset = 0.5;
    c
}

#[test]
fn expanded_cycle_preserves_motion_and_derivatives_for_single_and_multiple_steps() {
    for multiple in [false, true] {
        let mut c = config();
        if !multiple {
            c.feet[0].additional_steps.clear();
        }
        // Nonconstant body exercises spline control repetition, including wrap.
        for (i, k) in c.body.keyframes.iter_mut().take(4).enumerate() {
            k.values = (0..6)
                .map(|axis| 0.01 * ((i + axis) as f64).sin())
                .collect();
        }
        c.body.keyframes[4].values = c.body.keyframes[0].values.clone();
        let expanded = c.repeated_cycle(3).unwrap();
        assert_eq!(
            expanded.feet[0].steps().count(),
            if multiple { 6 } else { 3 }
        );
        let original = ContactPhaseMotion::new(c).unwrap();
        let expanded = ContactPhaseMotion::new(expanded).unwrap();
        for i in -321..987 {
            let t = i as f64 * 0.00413;
            let a = original.sample_retimed(t, -1.3, 0.2).unwrap();
            let b = expanded.sample_retimed(t, -1.3, 0.2).unwrap();
            assert_eq!(a.feet[0].in_contact, b.feet[0].in_contact);
            let values = |s: &ContactPhaseSample| {
                s.body
                    .values
                    .iter()
                    .chain(&s.body.rates)
                    .chain(&s.body.accelerations)
                    .chain(&s.feet[0].position_world_m)
                    .chain(&s.feet[0].velocity_world_m_s)
                    .chain(&s.feet[0].acceleration_world_m_s2)
                    .copied()
                    .collect::<Vec<_>>()
            };
            for (x, y) in values(&a).into_iter().zip(values(&b)) {
                assert!((x - y).abs() < 1e-10, "{t}: {x} != {y}");
            }
        }
    }
}

#[test]
fn invalid_step_geometry_and_timing_are_rejected_without_legacy_schema_changes() {
    for bad in [0.1, 0.2, f64::NAN] {
        let mut c = config();
        c.feet[0].additional_steps[0].phase_offset = bad;
        assert!(ContactPhaseMotion::new(c).is_err());
    }
    let mut c = config();
    c.feet[0].additional_steps[0].center_world_m[0] = f64::INFINITY;
    assert!(ContactPhaseMotion::new(c).is_err());
    let mut c = config();
    c.feet[0].additional_steps[0].return_ramp_fraction = Some(0.7);
    assert!(ContactPhaseMotion::new(c).is_err());
    let mut c = config();
    c.feet[0].additional_steps.clear();
    let value = serde_json::to_value(c).unwrap();
    assert!(value["feet"][0].get("additional_steps").is_none());
    let decoded: ContactPhaseConfig = serde_json::from_value(value).unwrap();
    assert!(decoded.feet[0].additional_steps.is_empty());
}
