use sim_dynamics::{Integrator, Simulation, System};

struct OrderedRoots {
    later_enabled: bool,
    fired: Vec<usize>,
}
impl System for OrderedRoots {
    fn dimension(&self) -> usize {
        1
    }
    fn residual(&self, _: f64, _: &[f64], rate: &[f64], out: &mut [f64]) {
        out[0] = rate[0] - 1.0;
    }
    fn derivative(&self, _: f64, _: &[f64], out: &mut [f64]) -> bool {
        out[0] = 1.0;
        true
    }
    fn guards(&self, _: f64, x: &[f64], out: &mut Vec<f64>) {
        out.push(if self.later_enabled { 0.8 - x[0] } else { 1.0 });
        out.push(0.2 - x[0]);
    }
    fn jump(&mut self, index: usize, _: f64, _: &mut [f64]) {
        self.fired.push(index);
        if index == 1 {
            self.later_enabled = false;
        }
    }
}
#[test]
fn earliest_physical_crossing_wins_over_guard_declaration_order() {
    for integrator in [
        Integrator::Rk4,
        Integrator::BackwardEuler(Default::default()),
    ] {
        let mut sim = Simulation::new(
            OrderedRoots {
                later_enabled: true,
                fired: vec![],
            },
            integrator,
            vec![0.0],
        );
        sim.step(1.0).unwrap();
        assert_eq!(
            sim.system.fired,
            vec![1],
            "the earlier mode change disables the later crossing"
        );
        assert!((sim.events[0].time - 0.2).abs() < 2e-6);
        assert!((sim.state[0] - 1.0).abs() < 1e-12);
    }
}

struct DecaySwitch {
    rate: f64,
    switched: bool,
    builds: std::cell::Cell<usize>,
}
impl System for DecaySwitch {
    fn dimension(&self) -> usize {
        1
    }
    fn residual(&self, _: f64, x: &[f64], rate: &[f64], out: &mut [f64]) {
        out[0] = rate[0] + self.rate * x[0];
    }
    fn jacobian(
        &self,
        _: f64,
        _: &[f64],
        _: &[f64],
        out: &mut sim_dynamics::JacobianParts,
    ) -> bool {
        self.builds.set(self.builds.get() + 1);
        out.dx(0, 0, self.rate);
        out.drate(0, 0, 1.0);
        true
    }
    fn guards(&self, _: f64, x: &[f64], out: &mut Vec<f64>) {
        out.push(if self.switched { 1.0 } else { x[0] - 0.3 });
    }
    fn jump(&mut self, _: usize, _: f64, _: &mut [f64]) {
        self.switched = true;
        self.rate = 4000.0;
    }
}
#[test]
fn guarded_event_matrix_reuse_saves_builds_and_refreshes_after_a_mode_change() {
    let run = |reuse| {
        let system = DecaySwitch {
            rate: 40.0,
            switched: false,
            builds: 0.into(),
        };
        let mut sim = Simulation::new(
            system,
            Integrator::BackwardEuler(Default::default()),
            vec![1.0],
        );
        sim.event_jacobian_reuse = reuse;
        sim.set_attempt_audit_limit(128);
        sim.step(0.1).unwrap();
        assert_eq!(sim.stats.subdivided_steps, 0);
        assert_eq!(sim.events.len(), 1);
        // Independent backward-Euler solution within each fixed-rate branch.
        let event = (1.0 / 0.3 - 1.0) / 40.0;
        assert!((sim.events[0].time - event).abs() < 2e-7);
        assert!((sim.state[0] - 0.3 / (1.0 + 4000.0 * (0.1 - event))).abs() < 2e-8);
        let after = sim
            .implicit_attempts
            .iter()
            .find(|a| a.start_time >= sim.events[0].time)
            .unwrap();
        assert!(
            after.newton.iterations[0].fresh_jacobian,
            "mode jump must invalidate the trial matrix"
        );
        sim
    };
    let ordinary = run(false);
    let reused = run(true);
    assert!(
        reused.system.builds.get() < ordinary.system.builds.get(),
        "{} vs {}",
        reused.system.builds.get(),
        ordinary.system.builds.get()
    );
    assert!((ordinary.state[0] - reused.state[0]).abs() < 2e-9);
    assert!(
        reused
            .implicit_attempts
            .iter()
            .any(|a| !a.newton.iterations[0].fresh_jacobian)
    );
    let mut built_at: Option<f64> = None;
    for attempt in &reused.implicit_attempts {
        if !attempt.newton.iterations[0].fresh_jacobian {
            assert!(
                (built_at.unwrap() - attempt.step).abs() <= 0.1 * attempt.step,
                "reuse must remain bounded relative to the actual matrix build step"
            );
        }
        if attempt.newton.iterations.iter().any(|i| i.fresh_jacobian) {
            built_at = Some(attempt.step);
        }
    }
}
