use sim_core::{BehaviorRegistry, View};
use sim_domain_robot::SERVO_FIRMWARE;

#[test]
fn servo_deadlines_retain_phase_over_long_runs_and_state_restore() {
    let mut registry = BehaviorRegistry::default();
    sim_domain_robot::motor::register(&mut registry).unwrap();
    let descriptor = registry.get(&SERVO_FIRMWARE.into()).unwrap();
    for (rate, offset) in [(1000.0, 0.001), (333.0, 0.00037), (1000.0, 0.0)] {
        let parameters = [("rate".into(), rate), ("offset".into(), offset)]
            .into_iter()
            .collect();
        let mut firmware = descriptor.equations.unwrap()(&parameters).unwrap();
        let mut state: Vec<_> = firmware.states().iter().map(|s| s.initial).collect();
        let clock = state.len() - 1;
        let period = 1.0 / rate;
        for tick in 0..1_000_000 {
            let expected = offset + tick as f64 * period;
            // Absolute clock error must remain a few ulps, rather than grow
            // with the number of additions. The old += period fails this.
            let tolerance = 4.0 * f64::EPSILON * expected.abs().max(period);
            assert!(
                (state[clock] - expected).abs() <= tolerance,
                "rate={rate}, tick={tick}: {} vs {expected}",
                state[clock]
            );
            let old = state.clone();
            let view = View {
                time: expected,
                states: &old,
                offsets: &[0; 5],
                rate_map: &[None; 4],
                across: &[],
                across_rates: &[],
                signals_in: &[0.1, 0.0, 0.0],
            };
            firmware.jump(0, &view, &mut state);
            assert!(state[clock] > old[clock]);
            if tick == 1354 || tick == 999_999 {
                let mut restored = old.clone();
                firmware.jump(0, &view, &mut restored);
                assert_eq!(restored, state, "clock must live in rollback state");
            }
        }
    }
}
