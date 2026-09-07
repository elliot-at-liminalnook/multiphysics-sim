use sim_domain_control::motion_clock::*;
fn clock() -> MotionClock {
    MotionClock::new(MotionClockConfig {
        period_s: 0.01,
        duration_s: 0.1,
        guard_start_s: 0.02,
        guard_end_s: 0.08,
        qualification_s: 0.02,
        maximum_pause_s: 0.04,
    })
    .unwrap()
}
#[test]
fn qualified_progress_pause_resume_and_completion_follow_simulation_time() {
    let c = clock();
    let mut s = MotionClockState::default();
    for i in 0..=2 {
        c.sample(&mut s, i as f64 * 0.01, true).unwrap();
    }
    assert!(s.advancing);
    assert!((c.reference_at(&s, 0.025).unwrap() - 0.025).abs() < 1e-12);
    c.sample(&mut s, 0.03, false).unwrap();
    assert!(!s.advancing);
    assert!((c.reference_at(&s, 0.035).unwrap() - 0.03).abs() < 1e-12);
    for i in 4..=5 {
        c.sample(&mut s, i as f64 * 0.01, true).unwrap();
        assert!(!s.advancing);
    }
    c.sample(&mut s, 0.06, true).unwrap();
    assert!(s.advancing);
    assert!(!s.timed_out);
    for i in 7..=13 {
        c.sample(&mut s, i as f64 * 0.01, true).unwrap();
    }
    assert_eq!(s.reference_tick, 10);
    assert!(!s.advancing);
}
#[test]
fn timeout_latches_and_invalid_calls_leave_checkpoint_unchanged() {
    let c = clock();
    let mut s = MotionClockState::default();
    for i in 0..=6 {
        c.sample(&mut s, i as f64 * 0.01, false).unwrap();
    }
    assert!(s.timed_out);
    assert_eq!(s.reference_tick, 2);
    for i in 7..=12 {
        c.sample(&mut s, i as f64 * 0.01, true).unwrap();
    }
    assert!(s.timed_out);
    assert_eq!(s.reference_tick, 2);
    let before = format!("{s:?}");
    assert!(c.sample(&mut s, 1.0, true).is_err());
    assert_eq!(format!("{s:?}"), before);
    assert!(c.reference_at(&s, 1.0).is_err());
    let mut bad = MotionClockConfig {
        period_s: 0.01,
        duration_s: 0.1,
        guard_start_s: 0.02,
        guard_end_s: 0.08,
        qualification_s: 0.02,
        maximum_pause_s: 0.04,
    };
    bad.guard_start_s = 0.021;
    assert!(MotionClock::new(bad).is_err());
}

#[test]
fn registry_exposes_units_and_checkpointed_clock_matches_direct_use() {
    use sim_core::{BehaviorRegistry, Context, View};
    let mut registry = BehaviorRegistry::default();
    sim_domain_control::elements::register(&mut registry).unwrap();
    let descriptor = registry.get(&MOTION_CLOCK.into()).unwrap();
    let parameters = [
        ("period_s", 0.01),
        ("duration_s", 0.1),
        ("guard_start_s", 0.02),
        ("guard_end_s", 0.08),
        ("qualification_s", 0.02),
        ("maximum_pause_s", 0.04),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v))
    .collect();
    descriptor.validate_parameters(&parameters).unwrap();
    let mut component = descriptor.equations.unwrap()(&parameters).unwrap();
    let mut state = vec![0.0; 6];
    let mut direct = MotionClockState::default();
    let clock = clock();
    for i in 0..14 {
        let time = i as f64 * 0.01;
        let ready = i != 3;
        let old = state.clone();
        let input = [if ready { 1.0 } else { 0.0 }];
        let view = View {
            time,
            states: &old,
            offsets: &[0],
            rate_map: &[],
            across: &[],
            across_rates: &[],
            signals_in: &input,
        };
        component.jump(0, &view, &mut state);
        let mut replay = old.clone();
        component.jump(0, &view, &mut replay);
        assert_eq!(state, replay);
        clock.sample(&mut direct, time, ready).unwrap();
        let mut residual = vec![0.0; 6];
        let mut signals = [0.0; 3];
        component.residual(&mut Context::new(
            time + 0.005,
            &state,
            &[0.0; 6],
            &[0],
            &[],
            &[],
            &[],
            &input,
            &mut residual,
            &mut [],
            &mut signals,
        ));
        assert!((signals[0] - clock.reference_at(&direct, time + 0.005).unwrap()).abs() < 1e-12);
        assert_eq!(signals[1], u8::from(direct.advancing) as f64);
        assert_eq!(signals[2], u8::from(direct.timed_out) as f64);
    }
}

#[test]
fn progress_explains_waiting_qualification_timeout_and_completion_without_advancing() {
    let c = clock();
    let mut s = MotionClockState::default();
    assert_eq!(c.progress(&s, 0.0).unwrap().phase, "initial");
    for i in 0..=2 {
        c.sample(&mut s, i as f64 * 0.01, false).unwrap();
    }
    let before = format!("{s:?}");
    let status = c.progress(&s, 0.025).unwrap();
    assert_eq!(status.phase, "waiting_for_condition");
    assert_eq!(status.reference_time_s, 0.02);
    assert_eq!(status.qualified_duration_s, 0.0);
    assert_eq!(before, format!("{s:?}"));
    for i in 3..=5 {
        c.sample(&mut s, i as f64 * 0.01, true).unwrap();
    }
    let status = c.progress(&s, 0.05).unwrap();
    assert_eq!(status.phase, "condition_qualified");
    assert_eq!(status.qualified_duration_s, 0.02);
    for i in 6..=13 {
        c.sample(&mut s, i as f64 * 0.01, true).unwrap();
    }
    assert_eq!(c.progress(&s, 0.13).unwrap().phase, "complete");
    let mut timed = MotionClockState::default();
    for i in 0..=6 {
        c.sample(&mut timed, i as f64 * 0.01, false).unwrap();
    }
    assert_eq!(c.progress(&timed, 0.06).unwrap().phase, "timed_out");
}
