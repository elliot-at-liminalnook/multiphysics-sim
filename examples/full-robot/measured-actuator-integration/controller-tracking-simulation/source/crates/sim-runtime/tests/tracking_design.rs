use sim_domain_control::fixed_pd::Gains;
use sim_runtime::controller_refinement::tracking::{self, Experiment};
use std::sync::atomic::AtomicBool;
fn experiment() -> Experiment {
    Experiment {
        name: "test".into(),
        model: sim_runtime::experiment_study::baseline().unwrap().0,
        gains: Gains {
            kp_q8: 1024,
            kd_q8: 512,
            kv_q8: 0,
            limit: 100,
        },
        targets_counts: (0..50).map(|i| if i < 10 { 0 } else { 20 }).collect(),
        period_s: 0.01,
        latency_s: 0.002,
        voltage_v: 11.8,
        temperature_c: 25.,
    }
}
#[test]
fn offline_tracking_uses_own_feedback_and_shared_integer_law() {
    let e = experiment();
    let r = tracking::simulate(&e, &AtomicBool::new(false)).unwrap();
    assert_eq!(r.samples.len(), 50);
    assert!(r.samples.last().unwrap().encoder_counts > 0);
    let mut previous = 2048;
    for i in 0..r.samples.len() - 1 {
        let s = &r.samples[i];
        let position = (s.encoder_counts + 2048) as u16;
        let expected = sim_domain_control::fixed_pd::step(
            e.gains,
            (s.desired_counts + 2048) as u16,
            position,
            if i == 0 { position } else { previous },
            s.desired_counts
                - if i == 0 {
                    0
                } else {
                    r.samples[i - 1].desired_counts
                },
        )
        .unwrap();
        assert_eq!(r.samples[i + 1].applied_pwm, expected);
        previous = position;
    }
    let mut off = e.clone();
    off.gains.limit = 0;
    let r = tracking::simulate(&off, &AtomicBool::new(false)).unwrap();
    assert!(
        r.samples
            .iter()
            .all(|s| s.encoder_counts == 0 && s.applied_pwm == 0)
    );
    assert!(r.metrics.rms_degrees > 1.);
}
#[test]
fn delay_cancellation_and_timestep_are_explicit() {
    let mut e = experiment();
    e.latency_s = 0.02;
    let r = tracking::simulate(&e, &AtomicBool::new(false)).unwrap();
    assert_eq!(r.samples[11].applied_pwm, 0);
    assert!(r.samples[13].applied_pwm > 0);
    assert!(
        tracking::simulate(&e, &AtomicBool::new(true))
            .unwrap_err()
            .contains("Cancelled")
    );
    e.latency_s = 0.002;
    let a = tracking::simulate(&e, &AtomicBool::new(false)).unwrap();
    e.model.step_s *= 0.5;
    let b = tracking::simulate(&e, &AtomicBool::new(false)).unwrap();
    let max = a
        .samples
        .iter()
        .zip(&b.samples)
        .map(|(a, b)| (a.angle_rad - b.angle_rad).abs())
        .fold(0f64, f64::max);
    assert!(max < 0.002, "timestep discrepancy {max}");
}
#[test]
fn coupled_controller_matches_single_axis_and_accounts_for_supply() {
    let e = experiment();
    let source = sim_runtime::controller_refinement::power::Setup {
        source_component: sim_domain_electrical::elements::VOLTAGE_SOURCE.into(),
        source_parameters: std::collections::BTreeMap::from([("voltage".into(), e.voltage_v)]),
        auxiliary_current_a: 0.,
        evidence: "Synthetic test".into(),
        limits: Default::default(),
    };
    let single = tracking::simulate(&e, &AtomicBool::new(false)).unwrap();
    let group = tracking::simulate_group(
        &[e.clone(), e],
        source,
        0.,
        &[0., 0.],
        "Synthetic test".into(),
        &AtomicBool::new(false),
    )
    .unwrap();
    for axis in &group.axes {
        for (a, b) in axis.samples.iter().zip(&single.samples) {
            assert_eq!(a.encoder_counts, b.encoder_counts);
            assert_eq!(a.applied_pwm, b.applied_pwm);
        }
    }
    for (i, s) in group.supply_time_voltage_current_power.iter().enumerate() {
        assert!(
            (s[2]
                - group
                    .axes
                    .iter()
                    .map(|a| a.electrical[i].supply_current_a)
                    .sum::<f64>())
            .abs()
                < 1e-8
        );
        assert!((s[3] - s[1] * s[2]).abs() < 1e-10);
    }
}
