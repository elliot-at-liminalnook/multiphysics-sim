use sim_core::{Behavior, BehaviorRegistry, Context, Input, LocalJacobian, Output, View};
use sim_dynamics::{Integrator, JacobianParts, Simulation, System};
use std::collections::BTreeMap;

const OFFSETS: [usize; 5] = [0, 1, 2, 4, 5];
const RATE_MAP: [Option<usize>; 5] = [None, None, Some(3), None, None];
const MASS: f64 = 0.01;
const DRIVE: f64 = 0.00546;
const V0: f64 = 0.104;
const HALF: f64 = 0.005;
const Q0: f64 = HALF - 0.0005 * V0;

// Actual MotorUnit with prescribed voltage, fixed output shaft and ambient
// temperature. These boundary conditions give constant motor torque and one
// positive inertia acting against the existing backlash spring/damping law.
struct MotorSystem {
    motor: Box<dyn Behavior>,
    across: [f64; 5],
}
impl MotorSystem {
    fn new(events: bool, sign: f64) -> Self {
        let mut registry = BehaviorRegistry::default();
        sim_domain_robot::register(&mut registry).unwrap();
        let params: BTreeMap<_, _> = [
            ("resistance", 1.0),
            ("torque_constant", 1.0),
            ("jacobian.analytic", 1.0),
            ("back_emf_constant", 0.0),
            ("rotor_inertia", MASS),
            ("ratio", 1.0),
            ("efficiency", 1.0),
            ("backlash", 2.0 * HALF),
            ("gear_stiffness", 50.0),
            ("gear_damping", 0.1),
            ("temp_coeff", 0.0),
            ("derating", 0.0),
            ("initial.angle", sign * Q0),
            ("backlash.events", if events { 1.0 } else { 0.0 }),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v))
        .collect();
        let motor = (registry
            .get(&sim_domain_robot::MOTOR_UNIT.into())
            .unwrap()
            .equations
            .unwrap())(&params)
        .unwrap();
        Self {
            motor,
            across: [sign * DRIVE, 0.0, 0.0, 0.0, 293.15],
        }
    }
    fn view<'a>(&'a self, t: f64, x: &'a [f64]) -> View<'a> {
        View {
            time: t,
            states: x,
            offsets: &OFFSETS,
            rate_map: &RATE_MAP,
            across: &self.across,
            across_rates: &[0.0; 5],
            signals_in: &[],
        }
    }
    fn initial(&self, sign: f64) -> Vec<f64> {
        let mut x: Vec<_> = self.motor.states().iter().map(|s| s.initial).collect();
        x[0] = sign * DRIVE;
        x[1] = sign * V0;
        x
    }
}
impl System for MotorSystem {
    fn dimension(&self) -> usize {
        self.motor.states().len()
    }
    fn algebraic(&self) -> Option<Vec<bool>> {
        let mut a = vec![false; self.dimension()];
        a[0] = true;
        Some(a)
    }
    fn residual(&self, t: f64, x: &[f64], rate: &[f64], out: &mut [f64]) {
        let mut through = [0.0; 5];
        let mut signals = [0.0; 3];
        let mut ctx = Context::new(
            t,
            x,
            rate,
            &OFFSETS,
            &RATE_MAP,
            &self.across,
            &[0.0; 5],
            &[],
            out,
            &mut through,
            &mut signals,
        );
        self.motor.residual(&mut ctx);
    }
    fn jacobian(&self, t: f64, x: &[f64], _: &[f64], out: &mut JacobianParts) -> bool {
        let mut local = LocalJacobian::default();
        if !self.motor.jacobian(&self.view(t, x), &mut local) {
            return false;
        }
        for (output, input, value) in local.entries {
            if let Output::State(row) = output {
                match input {
                    Input::State(col) => out.dx(row, col, value),
                    Input::StateRate(col) => out.drate(row, col, value),
                    _ => {} // Prescribed boundary values are not unknowns here.
                }
            }
        }
        true
    }
    fn guards(&self, t: f64, x: &[f64], out: &mut Vec<f64>) {
        self.motor.guards(&self.view(t, x), out);
    }
    fn scheduled_events(&self, t: f64, x: &[f64], out: &mut Vec<(usize, f64)>) {
        self.motor.scheduled_events(&self.view(t, x), out);
    }
    fn jump(&mut self, i: usize, t: f64, x: &mut [f64]) {
        let before = x.to_vec();
        let v = View {
            time: t,
            states: &before,
            offsets: &OFFSETS,
            rate_map: &RATE_MAP,
            across: &self.across,
            across_rates: &[0.0; 5],
            signals_in: &[],
        };
        self.motor.jump(i, &v, x);
    }
}

fn analytic(t: f64) -> (f64, f64, f64) {
    let ci = 0.005;
    let free = |t: f64| {
        let vinf = DRIVE / ci;
        (
            Q0 + vinf * t + (V0 - vinf) * (1.0 - (-ci * t / MASS).exp()) * MASS / ci,
            vinf + (V0 - vinf) * (-ci * t / MASS).exp(),
        )
    };
    let (mut lo, mut hi) = (0.0, 0.001);
    for _ in 0..64 {
        let mid = 0.5 * (lo + hi);
        if free(mid).0 < HALF {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let event = 0.5 * (lo + hi);
    if t <= event {
        let (q, v) = free(t);
        return (q, v, event);
    }
    let dt = t - event;
    let decay = 0.1 / (2.0 * MASS);
    let omega = (50.0 / MASS - decay * decay).sqrt();
    let equilibrium = DRIVE / 50.0;
    let a = -equilibrium;
    let b = (free(event).1 + decay * a) / omega;
    let y = a * (omega * dt).cos() + b * (omega * dt).sin();
    let yd = -a * omega * (omega * dt).sin() + b * omega * (omega * dt).cos();
    (
        HALF + equilibrium + (-decay * dt).exp() * y,
        (-decay * dt).exp() * (yd - decay * y),
        event,
    )
}

#[test]
fn engagement_events_resolve_the_missing_implicit_root_and_converge_to_analytic_motion() {
    for numerical in [false, true] {
        for sign in [-1.0, 1.0] {
            let mut errors = Vec::new();
            for h in [
                0.0005,
                0.00025,
                0.000125,
                0.0000625,
                0.00003125,
                0.000015625,
                0.0000078125,
            ] {
                let system = MotorSystem::new(true, sign);
                let initial = system.initial(sign);
                let mut sim = Simulation::new(
                    system,
                    Integrator::BackwardEuler(Default::default()),
                    initial,
                );
                sim.set_numerical_jacobian(numerical);
                sim.run(0.002, h).unwrap();
                assert_eq!(sim.stats.subdivided_steps, 0);
                assert_eq!(sim.events.len(), 2, "initialization then one engagement");
                assert_eq!(sim.state[3], sign);
                let (q, v, event) = analytic(0.002);
                errors.push(
                    (sim.state[2] - sign * q).abs() + (sim.state[1] - sign * v).abs() * 0.002,
                );
                assert!((sim.events[1].time - event).abs() < h * 0.002);
            }
            assert!(
                errors.windows(2).all(|w| w[1] < 0.6 * w[0]),
                "first-order convergence: {errors:?}"
            );
            assert!(*errors.last().unwrap() < 2e-8, "{errors:?}");
        }
    }
}

#[test]
fn event_mode_is_opt_in_and_initializes_from_the_actual_shaft_configuration() {
    assert_eq!(MotorSystem::new(false, 1.0).dimension(), 3);
    for (q, shaft, expected) in [(-0.02, 0.0, -1.0), (0.0, 0.0, 0.0), (0.02, 0.0, 1.0), (0.0, 0.02, -1.0), (0.0, -0.02, 1.0)] {
        let mut system = MotorSystem::new(true, 1.0);
        system.across[2] = shaft;
        let mut x = system.initial(1.0);
        x[2] = q;
        system.jump(2, 0.0, &mut x);
        assert_eq!(x[3], expected);
        let mut schedule = Vec::new();
        system.scheduled_events(0.0, &x, &mut schedule);
        assert!(schedule.is_empty());
        let mut ordinary = MotorSystem::new(false, 1.0);
        ordinary.across[2] = shaft;
        let mut a = [0.0; 4];
        let mut b = [0.0; 3];
        system.residual(0.0, &x, &[0.0; 4], &mut a);
        ordinary.residual(0.0, &x[..3], &[0.0; 3], &mut b);
        assert_eq!(
            &a[..3],
            &b,
            "same branch law before and after explicit mode selection"
        );
    }
}

#[test]
fn repeated_engagement_and_release_preserve_modes_and_dissipate_mechanical_energy() {
    let mut system = MotorSystem::new(true, 1.0);
    system.across[0] = 0.0;
    let mut initial = system.initial(1.0);
    initial[0] = 0.0;
    let mut sim = Simulation::new(
        system,
        Integrator::BackwardEuler(Default::default()),
        initial,
    );
    let energy = |x: &[f64]| 0.5 * MASS * x[1] * x[1] + 25.0 * (x[2].abs() - HALF).max(0.0).powi(2);
    let mut previous = energy(&sim.state);
    let mut modes = vec![];
    for _ in 0..5000 {
        sim.step(0.0001).unwrap();
        let current = energy(&sim.state);
        assert!(
            current <= previous + 1e-12,
            "unforced mechanism gained energy"
        );
        previous = current;
        let mode = sim.state[3];
        let q = sim.state[2];
        if mode == 0.0 {
            assert!(q.abs() <= HALF + 1e-9);
        } else {
            assert!(mode * q >= HALF - 1e-9);
        }
        if modes.last().copied() != Some(mode) {
            modes.push(mode);
        }
    }
    assert!(
        modes.windows(4).any(|m| m == [1.0, 0.0, -1.0, 0.0]),
        "{modes:?}"
    );
    assert_eq!(sim.stats.subdivided_steps, 0);
}
