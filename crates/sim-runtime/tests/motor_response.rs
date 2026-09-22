use sim_runtime::{
    controller_refinement::motor_response::*,
    experiment_comparison::{Observation, Trace},
};
fn estimator() -> Estimator {
    Estimator {
        encoder_quantum_rad: 1e-6,
        lag_samples: 1,
        maximum_sample_gap_s: 0.0101,
        maximum_sample_step_rad: 1.,
        maximum_sample_age_s: Some(0.),
    }
}
#[test]
fn group_diagnostics_keep_stationary_axes_and_reject_incomplete_captures() {
    let r: sim_runtime::controller_refinement::fpga::Recording = serde_json::from_str(include_str!("../../../examples/actuators/hx30hm/hardware/2026-09-14-unloaded-refinement/shared-check-id4/fpga-recording.json")).unwrap();
    let e: Estimator = serde_json::from_str(include_str!("../../../examples/actuators/hx30hm/hardware/2026-09-14-unloaded-refinement/motion-estimator.json")).unwrap();
    let report = group_motion(&r, &e).unwrap();
    assert_eq!(report["axes"].as_object().unwrap().len(), 9);
    assert_eq!(report["planned_moving_ids"], serde_json::json!([4]));
    let mut bad_e = e;
    bad_e.encoder_quantum_rad *= 0.5;
    assert!(group_motion(&r, &bad_e).is_err());
    let mut incomplete = r;
    incomplete.completed = false;
    assert!(group_motion(&incomplete, &estimator()).is_err());
}
fn trace(f: impl Fn(f64) -> f64) -> Trace {
    Trace {
        quantity: sim_core::QuantityKind::Angle.definition_id(),
        unit: "rad".into(),
        samples: (0..=400)
            .map(|i| {
                let t = i as f64 * 0.01;
                Observation {
                    time_s: t,
                    request_s: t,
                    completion_s: t,
                    value: f(t),
                }
            })
            .collect(),
    }
}
fn step(from: f64, to: f64, tau: f64) -> Trace {
    // Exact integral of v=to+(from-to)*exp(-(t-1)/tau), following a steady baseline.
    trace(|t| {
        if t <= 1. {
            from * t
        } else {
            from + to * (t - 1.) + (from - to) * tau * (1. - (-(t - 1.) / tau).exp())
        }
    })
}
fn transition(from: f64, to: f64) -> Transition {
    Transition {
        command_window_s: [1., 1.],
        observed_until_s: 4.,
        from_rad_s: from,
        target_rad_s: to,
        tolerance_rad_s: 0.05,
        dwell_s: 0.15,
    }
}
fn contains(interval: Option<[f64; 2]>, value: f64) {
    let [lo, hi] = interval.expect("Expected resolved interval");
    assert!(lo <= value && value <= hi, "{value} outside {lo}..{hi}");
}

#[test]
fn analytic_speedup_and_both_reversals_are_bracketed() {
    let tau = 0.2;
    for (from, to) in [(0., 2.), (0., -2.), (2., -2.), (-2., 2.)] {
        let (_, r) = response(&step(from, to, tau), &estimator(), &transition(from, to)).unwrap();
        assert!(r.baseline_established);
        contains(r.rise_10_to_90.interval_s, tau * 9_f64.ln());
        contains(
            r.command_to_settled_speed.interval_s,
            tau * ((to - from).abs() / 0.05).ln(),
        );
        if from * to < 0. {
            contains(r.command_to_zero_crossing.interval_s, tau * 2_f64.ln());
            contains(
                r.zero_to_settled_opposite_speed.interval_s,
                tau * (40_f64).ln(),
            );
            let travel = r.sampled_forward_travel_before_reversal_rad.unwrap();
            assert!((travel - 2. * tau * (1. - 2_f64.ln())).abs() < 0.001);
        } else {
            assert!(r.command_to_zero_crossing.interval_s.is_none());
        }
    }
}
#[test]
fn constant_speed_and_acceleration_have_correct_units_and_signs() {
    let m = estimate(&trace(|t| -3. * t), &estimator()).unwrap();
    assert!(m.speed_rad_s.iter().all(|s| (s.value + 3.).abs() < 1e-10));
    assert!(m.acceleration_rad_s2.iter().all(|s| s.value.abs() < 1e-8));
    let m = estimate(&trace(|t| 2. * t * t), &estimator()).unwrap();
    assert!(
        m.speed_rad_s
            .iter()
            .all(|s| (s.value - 4. * s.time_s).abs() < 1e-10)
    );
    assert!(
        m.acceleration_rad_s2
            .iter()
            .all(|s| (s.value - 4.).abs() < 1e-8 && s.bounds[0] <= 4. && 4. <= s.bounds[1])
    );
}
#[test]
fn slower_dynamics_produce_longer_rise_and_reversal() {
    let run = |tau| {
        response(&step(2., -2., tau), &estimator(), &transition(2., -2.))
            .unwrap()
            .1
    };
    let a = run(0.1);
    let b = run(0.3);
    assert!(b.rise_10_to_90.interval_s.unwrap()[0] > a.rise_10_to_90.interval_s.unwrap()[1]);
    assert!(
        b.command_to_zero_crossing.interval_s.unwrap()[0]
            > a.command_to_zero_crossing.interval_s.unwrap()[1]
    );
}
#[test]
fn unfinished_reversal_and_missing_initial_plateau_remain_unscored() {
    let mut t = transition(2., -2.);
    t.observed_until_s = 1.1;
    let (_, r) = response(&step(2., -2., 0.2), &estimator(), &t).unwrap();
    assert!(r.command_to_zero_crossing.interval_s.is_none());
    assert!(r.command_to_settled_speed.interval_s.is_none());
    t = transition(3., -2.);
    let (_, r) = response(&step(2., -2., 0.2), &estimator(), &t).unwrap();
    assert!(!r.baseline_established);
    assert!(r.rise_10_to_90.interval_s.is_none());
    assert!(r.command_to_settled_speed.interval_s.is_none());
}
#[test]
fn quantization_noise_does_not_prove_settling() {
    let mut e = estimator();
    e.encoder_quantum_rad = 0.01;
    let (_, r) = response(&step(0., 2., 0.2), &e, &transition(0., 2.)).unwrap();
    assert!(!r.baseline_established);
    assert!(r.command_to_settled_speed.interval_s.is_none());
}
#[test]
fn missing_data_wrap_and_overlapping_timestamps_are_not_smoothed_away() {
    let e = estimator();
    let original = step(2., -2., 0.2);
    let mut x = original.clone();
    x.samples.remove(110);
    assert!(estimate(&x, &e).is_err());
    let mut x = original.clone();
    x.samples[100].value += 6.;
    assert!(estimate(&x, &e).is_err());
    let mut x = original.clone();
    x.samples[100].request_s -= 0.1;
    assert!(estimate(&x, &e).is_err());
    let mut x = original.clone();
    x.unit = "rad/s".into();
    assert!(estimate(&x, &e).is_err());
    let mut e = e.clone();
    e.maximum_sample_age_s = Some(0.05);
    assert!(estimate(&original, &e).is_err());
}
#[test]
fn unknown_age_is_visible_and_larger_windows_widen_crossing_uncertainty() {
    let mut e = estimator();
    e.maximum_sample_age_s = None;
    let original = step(2., -2., 0.2);
    let (m, r) = response(&original, &e, &transition(2., -2.)).unwrap();
    assert!(!m.sample_age_bounded);
    let mut x = original.clone();
    for s in &mut x.samples {
        s.request_s -= 0.002;
        s.completion_s += 0.002;
    }
    let (_, wide) = response(&x, &e, &transition(2., -2.)).unwrap();
    // Broad timing windows also make the steady-speed band unresolved; never report
    // a tighter physical timing solely because the sampling is less certain.
    if let Some(w) = wide.command_to_zero_crossing.interval_s {
        let a = r.command_to_zero_crossing.interval_s.unwrap();
        assert!(w[1] - w[0] >= a[1] - a[0]);
    } else {
        assert!(!wide.baseline_established);
    }
}

#[test]
fn shared_motor_inertia_predicts_speedup_and_reversal_without_a_delay_parameter() {
    use sim_core::{BehaviorRegistry, ModelWorld};
    fn simulated(inertia: f64, dt: f64) -> Trace {
        let mut registry = BehaviorRegistry::default();
        sim_domain_robot::register(&mut registry).unwrap();
        sim_domain_electrical::elements::register(&mut registry).unwrap();
        sim_domain_rotational::elements::register(&mut registry).unwrap();
        sim_domain_thermal::register(&mut registry).unwrap();
        let mut m = ModelWorld::default();
        let source = m
            .part(
                &registry,
                "supply",
                sim_domain_electrical::voltage_history::VOLTAGE_HISTORY,
                [
                    ("count", 6.),
                    ("time.0", 0.),
                    ("voltage.0", 0.),
                    ("time.1", 0.5),
                    ("voltage.1", 0.),
                    ("time.2", 0.50001),
                    ("voltage.2", 2.),
                    ("time.3", 2.),
                    ("voltage.3", 2.),
                    ("time.4", 2.00001),
                    ("voltage.4", -2.),
                    ("time.5", 4.5),
                    ("voltage.5", -2.),
                ],
            )
            .unwrap();
        let ground = m
            .part(
                &registry,
                "ground",
                sim_domain_electrical::elements::GROUND,
                [],
            )
            .unwrap();
        let motor = m
            .part(
                &registry,
                "motor",
                sim_domain_robot::MOTOR_UNIT,
                [
                    ("resistance", 2.),
                    ("torque_constant", 0.2),
                    ("back_emf_constant", 0.2),
                    ("inductance", 0.),
                    ("dynamics.quasistatic_winding", 1.),
                    ("rotor_inertia", inertia),
                    ("ratio", 1.),
                    ("efficiency", 1.),
                    ("gear_stiffness", 1000.),
                    ("gear_damping", 0.1),
                    ("gear_friction", 0.),
                    ("no_load_current", 0.),
                    ("temp_coeff", 0.),
                    ("derating", 0.),
                ],
            )
            .unwrap();
        let load = m
            .part(
                &registry,
                "load",
                sim_domain_rotational::elements::INERTIA,
                [("inertia", 1e-7)],
            )
            .unwrap();
        let ambient = m
            .part(
                &registry,
                "ambient",
                sim_domain_thermal::AMBIENT,
                [("temperature", 293.15)],
            )
            .unwrap();
        m.connect([source.port("p"), motor.port("p")]);
        m.connect([source.port("n"), motor.port("n"), ground.port("pin")]);
        m.connect([motor.port("shaft"), load.port("shaft")]);
        m.connect([motor.port("winding"), ambient.port("node")]);
        for signal in ["speed", "current", "torque"] {
            m.connect([motor.port(signal)]);
        }
        let mut runtime = sim_compile::Runtime::new(
            m,
            &registry,
            sim_dynamics::Integrator::BackwardEuler(sim_solve::NewtonConfig::default()),
        )
        .unwrap();
        let angle = runtime.state_id(motor.behavior, "gear_angle");
        let mut samples = vec![];
        for i in 0..=900 {
            let time = i as f64 * 0.005;
            if i > 0 {
                runtime.advance(0.005, dt).unwrap();
            }
            samples.push(Observation {
                time_s: time,
                request_s: time,
                completion_s: time,
                value: runtime.get(angle),
            });
        }
        Trace {
            quantity: sim_core::QuantityKind::Angle.definition_id(),
            unit: "rad".into(),
            samples,
        }
    }
    let measure = |inertia, dt| {
        let trace = simulated(inertia, dt);
        let t = |time, from, to, end| Transition {
            command_window_s: [time, time + 0.00001],
            observed_until_s: end,
            from_rad_s: from,
            target_rad_s: to,
            tolerance_rad_s: 0.2,
            dwell_s: 0.05,
        };
        (
            response(&trace, &estimator(), &t(0.5, 0., 10., 1.99))
                .unwrap()
                .1,
            response(&trace, &estimator(), &t(2., 10., -10., 4.5))
                .unwrap()
                .1,
        )
    };
    let mut previous = None;
    for inertia in [0.002, 0.004] {
        let (rise, reverse) = measure(inertia, 0.0005);
        let tau = (inertia + 1e-7) * 2. / (0.2 * 0.2);
        contains(rise.rise_10_to_90.interval_s, tau * 9_f64.ln());
        contains(
            reverse.command_to_zero_crossing.interval_s,
            tau * 2_f64.ln(),
        );
        if let Some(prior) = previous {
            assert!(reverse.command_to_zero_crossing.interval_s.unwrap()[0] > prior);
        }
        previous = Some(reverse.command_to_zero_crossing.interval_s.unwrap()[1]);
        let (_, fine) = measure(inertia, 0.00025);
        let a = reverse.command_to_zero_crossing.interval_s.unwrap();
        let b = fine.command_to_zero_crossing.interval_s.unwrap();
        assert!((a[0] - b[0]).abs() <= 0.005 && (a[1] - b[1]).abs() <= 0.005);
    }
}

#[test]
fn real_nine_motor_capture_keeps_conditions_identity_and_unresolved_timings() {
    use sim_runtime::controller_refinement::fpga;
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/actuators/hx30hm/hardware/2026-09-14-speed-response");
    let recording: fpga::Recording = serde_json::from_slice(
        &std::fs::read(base.join("ramp-nine-repeat/fpga-recording.json")).unwrap(),
    )
    .unwrap();
    let request: Request =
        serde_json::from_slice(&std::fs::read(base.join("ramp-nine-repeat-request.json")).unwrap())
            .unwrap();
    let result = review(&recording, &request, None).unwrap();
    assert_eq!(result.trials.len(), 27);
    assert_eq!(result.recording_blake3, recording.fingerprint());
    assert!(
        result
            .trials
            .iter()
            .all(|t| t.conditions.simultaneously_controlled_ids.len() == 9
                && t.conditions.duty_limit_per_mille == 75)
    );
    assert!(result.trials.iter().all(|t| {
        !t.measured_motion.sample_age_bounded
            && t.comparison
                .measured
                .command_to_settled_speed
                .interval_s
                .is_none()
    }));
    assert!(fpga_trace(&recording, 3, None).is_err());
    let mut wrong = request.clone();
    wrong.estimator.encoder_quantum_rad *= 0.1;
    assert!(review(&recording, &wrong, None).is_err());
    let mut changed = recording.clone();
    changed.frames[10].pwm_readback[0] += 1;
    assert!(review(&changed, &request, None).is_err());
    let mut clipped = request.clone();
    clipped.trials.truncate(1);
    clipped.trials[0].travel_limited = true;
    let clipped = review(&recording, &clipped, None).unwrap();
    assert!(
        clipped.trials[0]
            .comparison
            .measured
            .onset_delay
            .interval_s
            .is_none()
    );
    assert!(
        clipped.trials[0]
            .comparison
            .measured
            .onset_delay
            .unresolved
            .as_ref()
            .unwrap()
            .contains("travel")
    );
}
