//! 27. Language independence — `control` `electrical` `rotational`.
//!
//! The same PI speed law closes the same compiled motor loop three ways:
//! as a Rust closure inside the process, as a Python program in a child
//! process speaking the seam's frame protocol, and as that Python program
//! again in a second run. Lockstep and simulation-time-only frames make
//! the three traces identical to the last bit; a controller that reads its
//! own clock is the falsifier — two runs of it disagree.

use crate::Report;
use crate::world::{registry, runtime};
use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, Coupler, FnCoupler, ModelWorld, StateId};
use sim_couple::FrameCoupler;
use sim_domain_bridges::elements as bridge;
use sim_domain_control::external::EXTERNAL;
use sim_domain_electrical::elements as el;
use sim_domain_rotational::elements as rot;
use std::path::PathBuf;

#[derive(Clone, Copy)]
pub struct Law {
    pub kp: f64,
    pub ki: f64,
    pub setpoint: f64,
    pub limit: f64,
    pub period: f64,
}

impl Default for Law {
    fn default() -> Self {
        Self { kp: 0.08, ki: 2.0, setpoint: 40.0, limit: 12.0, period: 2.0e-3 }
    }
}

/// The plant: voltage source → brushed motor → inertia, tachometer into
/// the seam, seam back to the source.
pub fn plant(registry: &BehaviorRegistry, period: f64) -> (Runtime, sim_core::BehaviorId, StateId, StateId) {
    let mut m = ModelWorld::default();
    let source = m.part(registry, "source", el::CONTROLLED_VOLTAGE_SOURCE, []).unwrap();
    let ground = m.part(registry, "ground", el::GROUND, []).unwrap();
    let motor = m.part(registry, "motor", bridge::BRUSHED_MOTOR, [("resistance", 0.6), ("inductance", 0.0), ("torque_constant", 0.05), ("back_emf_constant", 0.05)]).unwrap();
    let rotor = m.part(registry, "rotor", rot::INERTIA, [("inertia", 2.0e-4), ("damping", 2.0e-4)]).unwrap();
    let mount = m.part(registry, "mount", rot::GROUND, []).unwrap();
    let tacho = m.part(registry, "tacho", rot::SPEED_SENSOR, []).unwrap();
    let controller = m.part(registry, "controller", EXTERNAL, [("period", period), ("sense.speed", 0.0), ("act.voltage", 0.0)]).unwrap();
    m.connect([source.port("p"), motor.port("p")]);
    m.connect([source.port("n"), motor.port("n"), ground.port("pin")]);
    m.connect([motor.port("shaft"), rotor.port("shaft"), tacho.port("shaft")]);
    m.connect([motor.port("case"), mount.port("flange")]);
    m.connect([tacho.port("speed"), controller.port("sense.speed")]);
    m.connect([controller.port("act.voltage"), source.port("voltage")]);
    let rt = runtime(m, registry);
    let speed = rt.state_id(rotor.behavior, "speed");
    let angle = rt.across_id(rotor.port("shaft"));
    (rt, controller.behavior, speed, angle)
}

/// The PI law as a Rust closure: conditional anti-windup, like the Python example.
pub fn rust_controller(law: Law) -> Box<dyn Coupler> {
    let mut integral = 0.0;
    Box::new(FnCoupler(move |_t: f64, s: &[f64], a: &mut [f64]| {
        // The same operations in the same order as the Python example, so
        // the two agree to the last bit.
        let error = law.setpoint - s[0];
        let raw = law.kp * error + law.ki * integral;
        let out = raw.clamp(-law.limit, law.limit);
        if out == raw || (raw > 0.0) != (error > 0.0) {
            integral += error * law.period;
        }
        a[0] = out;
    }))
}

fn clients_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../clients/python")
}

/// The shipped Python PI example in a child process.
pub fn python_controller(law: Law) -> std::io::Result<Box<dyn Coupler>> {
    let dir = clients_dir();
    let script = dir.join("examples/pi_controller.py");
    let coupler = spawn_python(&[script.to_str().unwrap(), "--kp", &law.kp.to_string(), "--ki", &law.ki.to_string(), "--setpoint", &law.setpoint.to_string(), "--limit", &law.limit.to_string(), "--sensor", "speed", "--actuator", "voltage"])?;
    Ok(Box::new(coupler))
}

/// A controller that mixes its own wall clock into the command: the seam
/// carries it faithfully, and two runs of it cannot agree.
pub fn clock_controller(law: Law) -> std::io::Result<Box<dyn Coupler>> {
    let program = format!(
        "import sys, time\nsys.path.insert(0, {:?})\nfrom simloop import Loop\nloop = Loop.stdio()\nfor f in loop:\n    loop.send(voltage=max(-{limit}, min({limit}, {kp} * ({sp} - f['speed']) + 1e-3 * (time.perf_counter() % 1.0))))\n",
        clients_dir().to_str().unwrap(), limit = law.limit, kp = law.kp, sp = law.setpoint
    );
    Ok(Box::new(spawn_python(&["-c", &program])?))
}

pub(crate) fn spawn_python(args: &[&str]) -> std::io::Result<FrameCoupler> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../clients");
    let (script, rest) = args.split_first().expect("a script");
    if *script == "-c" {
        // An inline program: run it through the same environment.
        let mut command = std::process::Command::new("python3");
        command.arg("-u").args(args).env("PYTHONPATH", clients_dir().display().to_string());
        return FrameCoupler::spawn_command(command);
    }
    sim_couple::python(root, script, rest)
}

pub fn trace(registry: &BehaviorRegistry, law: Law, controller: Box<dyn Coupler>, duration: f64) -> (Vec<f64>, Vec<f64>) {
    let (mut rt, seam, speed, _) = plant(registry, law.period);
    rt.attach(seam, controller).unwrap();
    let t = rt.advance_recording(duration, law.period / 4.0, 1, &[speed]).unwrap();
    (t.time.clone(), t.column(0).to_vec())
}

fn worst_difference(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f64::max)
}

pub fn run() -> Report {
    let mut report = Report::new("language-independence");
    let registry = registry();
    let law = Law::default();
    let duration = 0.6;
    let (time, rust) = trace(&registry, law, rust_controller(law), duration);
    report.series("speed (rad/s), Rust controller in-process", &time, &rust, 600);
    report.measure("final speed, Rust (rad/s)", *rust.last().unwrap());
    report.within("the Rust loop reaches its setpoint", *rust.last().unwrap(), law.setpoint, 0.02);
    match python_controller(law) {
        Ok(python) => {
            let (_, first) = trace(&registry, law, python, duration);
            let (_, second) = trace(&registry, law, python_controller(law).unwrap(), duration);
            report.series("speed (rad/s), Python controller over the seam", &time, &first, 600);
            report.measure("worst |Rust − Python| (rad/s)", worst_difference(&rust, &first));
            report.measure("worst |Python run 1 − run 2| (rad/s)", worst_difference(&first, &second));
            report.below("the Python controller reproduces the Rust one to the bit", worst_difference(&rust, &first), 1.0e-12);
            report.below("two Python runs agree to the bit", worst_difference(&first, &second), 1.0e-12);
        }
        Err(e) => {
            report.holds(&format!("python3 with the client available ({e})"), false);
        }
    }
    match (clock_controller(law), clock_controller(law)) {
        (Ok(a), Ok(b)) => {
            let (_, first) = trace(&registry, law, a, duration);
            let (_, second) = trace(&registry, law, b, duration);
            report.measure("falsifier: worst |run 1 − run 2| with a wall-clock term (rad/s)", worst_difference(&first, &second));
            report.above("falsifier: a controller reading its own clock cannot repeat itself", worst_difference(&first, &second), 1.0e-9);
        }
        (Err(e), _) | (_, Err(e)) => {
            report.holds(&format!("python3 available for the falsifier ({e})"), false);
        }
    }
    report
}
