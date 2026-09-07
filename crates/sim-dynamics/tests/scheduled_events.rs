use sim_dynamics::{Integrator, Simulation, System};

struct Clocks {
    periods: Vec<f64>,
    next: Vec<f64>,
    ticks: Vec<usize>,
    crossing: Option<f64>,
    crossings: usize,
}
impl Clocks {
    fn new(periods: Vec<f64>) -> Self {
        Self { next: periods.clone(), ticks: vec![0; periods.len()], periods, crossing: None, crossings: 0 }
    }
}
impl System for Clocks {
    fn dimension(&self) -> usize { 1 }
    fn residual(&self, _: f64, _: &[f64], rates: &[f64], out: &mut [f64]) {
        out[0] = rates[0] - 1.0;
    }
    fn derivative(&self, _: f64, _: &[f64], out: &mut [f64]) -> bool {
        out[0] = 1.0;
        true
    }
    fn guards(&self, t: f64, x: &[f64], out: &mut Vec<f64>) {
        out.extend(self.next.iter().map(|next| next - t));
        if let Some(crossing) = self.crossing { out.push(crossing - x[0]); }
    }
    fn scheduled_events(&self, _: f64, _: &[f64], out: &mut Vec<(usize, f64)>) {
        out.extend(self.next.iter().copied().enumerate());
    }
    fn jump(&mut self, index: usize, _: f64, _: &mut [f64]) {
        if index == self.next.len() { self.crossings += 1; return; }
        self.ticks[index] += 1;
        self.next[index] += self.periods[index];
    }
}

#[test]
fn non_grid_clocks_and_physical_crossings_keep_their_order() {
    let mut clocks = Clocks::new(vec![0.0007, 0.0013]);
    clocks.crossing = Some(0.001);
    let mut sim = Simulation::new(clocks, Integrator::Rk4, vec![0.0]);
    sim.step(0.02).unwrap();
    assert_eq!(sim.system.ticks, vec![28, 15]);
    assert_eq!(sim.system.crossings, 1);
    assert_eq!(sim.events.len(), 44);
    assert_eq!(sim.events.iter().take(3).map(|e| e.guard).collect::<Vec<_>>(), vec![0, 2, 1]);
    assert!(sim.events.windows(2).all(|w| w[0].time <= w[1].time));
    assert!((sim.state[0] - 0.02).abs() < 1e-14);
}

#[test]
fn clocks_due_at_start_fire_once_and_invalid_clocks_fail() {
    let mut clocks = Clocks::new(vec![0.001]);
    clocks.next[0] = 0.0;
    let mut sim = Simulation::new(clocks, Integrator::Rk4, vec![0.0]);
    let event = sim.run_to_event(0.002, 0.0005).unwrap().unwrap();
    assert_eq!(event.time, 0.0);
    assert_eq!(sim.system.ticks, vec![1]);
    sim.step(0.0005).unwrap();
    assert_eq!(sim.system.ticks, vec![1]);
    for period in [0.0, f64::NAN] {
        let mut broken = Simulation::new(Clocks::new(vec![period]), Integrator::Rk4, vec![0.0]);
        assert!(matches!(broken.step(0.01), Err(sim_dynamics::DynamicsError::Schedule { .. })));
    }
}

#[test]
fn aligned_clocks_do_not_create_roundoff_sized_implicit_steps() {
    let mut sim = Simulation::new(Clocks::new(vec![0.005]), Integrator::implicit_midpoint(), vec![0.0]);
    sim.run(0.15, 0.001).unwrap();
    assert_eq!(sim.system.ticks, vec![30]);
    assert_eq!(sim.stats.steps, 150);
    assert_eq!(sim.stats.subdivided_steps, 0);
}

#[test]
fn clock_ticks_are_included_at_reporting_boundaries_for_every_timestep() {
    for h in [0.0005, 0.00025, 0.000125, 0.0000625] {
        let mut sim = Simulation::new(Clocks::new(vec![0.001; 12]), Integrator::Rk4, vec![0.0]);
        for frame in 1..=10 {
            sim.run(0.002, h).unwrap();
            assert_eq!(sim.system.ticks, vec![2 * frame; 12], "h={h}, frame={frame}");
            assert!((sim.state[0] - 0.002 * frame as f64).abs() < 1e-14);
        }
    }
}
