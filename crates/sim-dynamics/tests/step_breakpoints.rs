use sim_dynamics::{Integrator, Simulation, System};

struct ClockDecay;
impl System for ClockDecay {
    fn dimension(&self) -> usize {
        2
    }
    fn residual(&self, _: f64, x: &[f64], rate: &[f64], out: &mut [f64]) {
        out[0] = rate[0] + x[0];
        out[1] = rate[1];
    }
    fn derivative(&self, _: f64, x: &[f64], out: &mut [f64]) -> bool {
        out[0] = -x[0];
        out[1] = 0.0;
        true
    }
    fn guards(&self, t: f64, x: &[f64], out: &mut Vec<f64>) {
        out.push(x[1] - t);
    }
    fn scheduled_events(&self, _: f64, x: &[f64], out: &mut Vec<(usize, f64)>) {
        out.push((0, x[1]));
    }
    fn jump(&mut self, _: usize, _: f64, x: &mut [f64]) {
        x[1] += 0.25;
    }
}

fn simulation() -> Simulation<ClockDecay> {
    Simulation::new(
        ClockDecay,
        Integrator::BackwardEuler(Default::default()),
        vec![1.0, 0.25],
    )
}

#[test]
fn requested_boundaries_match_explicit_steps_without_extra_clock_events() {
    let mut split = simulation();
    split.set_step_breakpoints(vec![0.1, 0.25, 0.4]).unwrap();
    split.step(0.5).unwrap();
    let mut explicit = simulation();
    for dt in [0.1, 0.15, 0.15, 0.1] {
        explicit.step(dt).unwrap();
    }
    let expected = 1.0 / (1.1 * 1.15 * 1.15 * 1.1);
    assert!((split.state[0] - expected).abs() < 1e-12);
    assert!((split.state[0] - explicit.state[0]).abs() < 1e-12);
    assert_eq!(split.events, explicit.events);
    assert_eq!(
        split.events.iter().map(|e| e.time).collect::<Vec<_>>(),
        vec![0.25, 0.5]
    );
    assert_eq!(split.trace.time, vec![0.0, 0.1, 0.25, 0.4, 0.5]);
    assert_eq!(split.stats.steps, 4);
}

#[test]
fn invalid_replacement_is_atomic_and_snapshot_restore_replays_boundaries() {
    let mut sim = simulation();
    sim.set_step_breakpoints(vec![0.0, 0.1, 0.25, 0.4]).unwrap();
    for bad in [
        vec![f64::NAN],
        vec![f64::INFINITY],
        vec![-0.1],
        vec![0.2, 0.1],
        vec![0.1, 0.1],
    ] {
        assert!(sim.set_step_breakpoints(bad).is_err());
    }
    let start = sim.snapshot();
    sim.step(0.5).unwrap();
    let end = sim.state.clone();
    sim.restore(&start).unwrap();
    sim.step(0.5).unwrap();
    assert_eq!(sim.state, end);
    assert_eq!(sim.stats.steps, 8);
}
