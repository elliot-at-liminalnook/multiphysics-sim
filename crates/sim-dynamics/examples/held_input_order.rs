//! Exact held-input relaxation benchmark; this is not a robot or actuator model.
use sim_dynamics::{Integrator, JacobianParts, Simulation, System, sdirk};
use sim_solve::NewtonConfig;
use std::cell::Cell;
struct Relaxation {
    lambda: f64,
    target: f64,
    calls: Cell<usize>,
}
impl System for Relaxation {
    fn dimension(&self) -> usize {
        1
    }
    fn residual(&self, _: f64, x: &[f64], v: &[f64], r: &mut [f64]) {
        self.calls.set(self.calls.get() + 1);
        r[0] = v[0] + self.lambda * (x[0] - self.target);
    }
    fn derivative(&self, _: f64, x: &[f64], v: &mut [f64]) -> bool {
        v[0] = -self.lambda * (x[0] - self.target);
        true
    }
    fn jacobian(&self, _: f64, _: &[f64], _: &[f64], j: &mut JacobianParts) -> bool {
        j.dx(0, 0, self.lambda);
        j.drate(0, 0, 1.);
        true
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = NewtonConfig {
        absolute_tolerance: 1e-12,
        relative_tolerance: 1e-12,
        ..Default::default()
    };
    let period = 0.02;
    let mut cases = vec![];
    for lambda in [50., 500.] {
        for subdivisions in [1, 2, 4, 8] {
            for method in [
                "backward_euler",
                "implicit_midpoint",
                "bdf2_cross_updates",
                "bdf2_restart_updates",
                "sdirk2",
            ] {
                let h = period / subdivisions as f64;
                let mut sim = Simulation::new(
                    Relaxation {
                        lambda,
                        target: 0.,
                        calls: Cell::new(0),
                    },
                    if method == "implicit_midpoint" {
                        Integrator::ImplicitMidpoint(cfg)
                    } else {
                        Integrator::BackwardEuler(cfg)
                    },
                    vec![0.],
                );
                sim.record_every = 0;
                let (mut x, mut previous, mut exact, mut maximum, mut sum) =
                    (0., None, 0., 0f64, 0.);
                for tick in 0..100 {
                    let t = tick as f64 * period;
                    let target = 0.2 * (2. * std::f64::consts::PI * 0.7 * t).sin()
                        + if (7..12).contains(&tick) { 0.1 } else { 0. };
                    sim.system.target = target;
                    for j in 0..subdivisions {
                        let old = x;
                        x = match method {
                            "bdf2_cross_updates" | "bdf2_restart_updates" => {
                                if method == "bdf2_restart_updates" && j == 0 {
                                    previous = None;
                                }
                                // Analytic discrete equations solely to screen the proposed stencil.
                                match previous {
                                    Some(p) => {
                                        (4. * old - p + 2. * h * lambda * target)
                                            / (3. + 2. * h * lambda)
                                    }
                                    None => (old + h * lambda * target) / (1. + h * lambda),
                                }
                            }
                            "sdirk2" => {
                                sdirk::step(&sim.system, t + j as f64 * h, h, &[old], cfg)?.state[0]
                            }
                            _ => {
                                sim.step(h)?;
                                sim.state[0]
                            }
                        };
                        previous = Some(old);
                    }
                    exact = target + (exact - target) * (-lambda * period).exp();
                    let error = (x - exact).abs();
                    maximum = maximum.max(error);
                    sum += error * error;
                }
                cases.push(serde_json::json!({"method":method,"decay_rate_per_s":lambda,"step_s":h,"maximum_endpoint_error":maximum,"rms_endpoint_error":(sum/100.).sqrt(),"residual_evaluations":if method.starts_with("bdf2"){None}else{Some(sim.system.calls.get())},"execution":if method.starts_with("bdf2"){"analytic_discrete_equation"}else{"shared_residual_solver"},"implicit_stages_per_step":if method=="sdirk2"{2}else{1}}));
            }
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"version":1,"period_s":period,"cases":cases,
        "scope":"Exact scalar relaxation under deterministic 50 Hz held inputs, including two command jumps. BE/midpoint and SDIRK use shared Rust residual solvers. BDF2 rows are analytic stencil proposals, not an integrated robot implementation. Error units are the scalar state unit; no CAD actuator, robot accuracy or realtime claim."})
        )?
    );
    Ok(())
}
