//! Each sensor and actuator on a plant small enough to know the answer.

use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, ModelWorld, StateId};
use sim_domain_control::elements as ctl;
use sim_domain_electrical::elements as el;
use sim_domain_multibody::contact as body;
use sim_domain_rotational::elements as rot;
use sim_domain_sensing as sense;
use sim_domain_translational::elements as tr;
use sim_dynamics::{Integrator, Trace};
use sim_solve::NewtonConfig;
use std::f64::consts::TAU;

fn registry() -> BehaviorRegistry {
    let mut registry = BehaviorRegistry::default();
    ctl::register(&mut registry).unwrap();
    el::register(&mut registry).unwrap();
    rot::register(&mut registry).unwrap();
    tr::register(&mut registry).unwrap();
    sim_domain_multibody::elements::register(&mut registry).unwrap();
    sim_domain_multibody::planar::register(&mut registry).unwrap();
    body::register(&mut registry).unwrap();
    sense::register(&mut registry).unwrap();
    registry
}

fn runtime(model: ModelWorld, registry: &BehaviorRegistry) -> Runtime {
    let newton = NewtonConfig { max_iterations: 40, min_line_search: 1.0 / 4096.0, ..NewtonConfig::default() };
    Runtime::new(model, registry, Integrator::ImplicitMidpoint(newton)).expect("model compiles")
}

/// The recorded value of `column` nearest to time `t`.
fn at(trace: &Trace, column: usize, t: f64) -> f64 {
    let k = (0..trace.len()).min_by(|a, b| (trace.time[*a] - t).abs().total_cmp(&(trace.time[*b] - t).abs())).unwrap();
    trace.state[k][column]
}

/// A first-order lag reading `output`. The runtime cannot initialise an
/// island with no differential state at all, so purely algebraic plants
/// get one through their reading — a data logger's own input filter.
fn sink(m: &mut ModelWorld, registry: &BehaviorRegistry, output: sim_core::PortId) {
    let logger = m.part(registry, "logger", ctl::LAG_CHAIN, [("delay", 1.0), ("stages", 1.0)]).unwrap();
    m.connect([output, logger.port("input")]);
}

fn approx(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol, "{a} != {b} (tol {tol})");
}

/// A tachometer on a free inertia (`damping` lets the speed decay), giving
/// (runtime, held output, true speed).
fn tacho(params: Vec<(&'static str, f64)>, damping: f64) -> (Runtime, StateId, StateId) {
    let registry = registry();
    let mut m = ModelWorld::default();
    let rotor = m.part(&registry, "rotor", rot::INERTIA, [("inertia", 0.1), ("damping", damping), ("initial.speed", 5.0)]).unwrap();
    let tacho = m.part(&registry, "tacho", sense::TACHOMETER, params).unwrap();
    m.connect([rotor.port("shaft"), tacho.port("shaft")]);
    let rt = runtime(m, &registry);
    let held = rt.state_id(tacho.behavior, "held");
    let speed = rt.state_id(rotor.behavior, "speed");
    (rt, held, speed)
}

#[test]
fn everything_registers() {
    let registry = registry();
    for id in [sense::ENCODER, sense::TACHOMETER, sense::IMU, sense::CURRENT_SENSOR, sense::VOLTAGE_SENSOR, sense::FORCE_SENSOR, sense::PWM_DRIVER, sense::SERVO, sense::QUANTISER] {
        assert!(registry.contains(&id.into()), "{id}");
        assert!(registry.get(&id.into()).unwrap().parameters.is_some(), "{id}");
    }
}

#[test]
fn declared_sensor_units_and_bounds_match_their_native_pipelines() {
    use sim_core::{PortSchema, QuantityKind as Q};
    let registry = registry();
    for (kind, key, unit) in [
        (sense::ENCODER, "quantum", "rad"), (sense::TACHOMETER, "noise", "rad/s"),
        (sense::CURRENT_SENSOR, "noise", "A"), (sense::VOLTAGE_SENSOR, "quantum", "V"),
        (sense::FORCE_SENSOR, "noise", "N"), (sense::IMU, "noise.ax", "m/s²"),
        (sense::IMU, "quantum.gyro", "rad/s"), (sense::PWM_DRIVER, "supply", "V"),
        (sense::SERVO, "torque_constant", "N·m/A"), (sense::QUANTISER, "step", "1"),
    ] {
        let descriptor = registry.get(&kind.into()).unwrap();
        let parameter = descriptor.parameters.as_ref().unwrap().iter().find(|p| p.name == key).unwrap();
        assert_eq!(parameter.unit, unit);
    }
    let imu = registry.get(&sense::IMU.into()).unwrap();
    assert_eq!(imu.ports.iter().find(|p| p.name == "ax").unwrap().schema, PortSchema::SignalOut(Q::LinearAcceleration));
    let servo = registry.get(&sense::SERVO.into()).unwrap();
    let limit = servo.parameters.as_ref().unwrap().iter().find(|p| p.name == "current_limit").unwrap();
    assert_eq!(limit.default, None);
    assert_eq!(limit.default_label.as_deref(), Some("inf"));
    for (kind, key, value) in [
        (sense::ENCODER, "counts", 2.5), (sense::TACHOMETER, "stages", 0.),
        (sense::TACHOMETER, "period", -1.), (sense::TACHOMETER, "seed", 1.5),
        (sense::TACHOMETER, "seed", 9_007_199_254_740_992.),
        (sense::CURRENT_SENSOR, "fault.mode", 4.), (sense::VOLTAGE_SENSOR, "noise", -1.),
        (sense::FORCE_SENSOR, "fault.samples", 0.5), (sense::IMU, "noise", 0.1),
        (sense::IMU, "noise.ax", -1.), (sense::PWM_DRIVER, "dead_band", 1.1),
        (sense::SERVO, "torque_constant", 0.), (sense::QUANTISER, "step", 0.),
    ] {
        let mut parameters = std::collections::BTreeMap::new();
        if kind == sense::PWM_DRIVER { parameters.insert("supply".into(), 24.); }
        if kind == sense::SERVO { parameters.insert("bandwidth".into(), 50.); }
        parameters.insert(key.into(), value);
        let error = registry.get(&kind.into()).unwrap().validate_parameters(&parameters).unwrap_err();
        assert!(error.to_string().contains(key), "{kind}: {error}");
    }
    for (kind, parameter) in [(sense::ENCODER, "counts"), (sense::TACHOMETER, "noise"),
        (sense::FORCE_SENSOR, "quantum"), (sense::CURRENT_SENSOR, "fault.mode")] {
        let descriptor = registry.get(&kind.into()).unwrap();
        let parameters = [(parameter.to_owned(), 1.)].into_iter().collect();
        let error = descriptor.equations.unwrap()(&parameters).err().unwrap().to_string();
        assert!(error.contains("period > 0"), "{kind}: {error}");
    }
}

#[test]
fn encoder_counts_are_whole_and_track_the_shaft() {
    let registry = registry();
    let mut m = ModelWorld::default();
    let rotor = m.part(&registry, "rotor", rot::INERTIA, [("inertia", 0.1), ("initial.speed", 1.0)]).unwrap();
    let encoder = m.part(&registry, "encoder", sense::ENCODER, [("counts", 1024.0), ("period", 1.0e-3)]).unwrap();
    m.connect([rotor.port("shaft"), encoder.port("shaft")]);
    let mut rt = runtime(m, &registry);
    let angle = rt.across_id(rotor.port("shaft"));
    let output = rt.signal_id(encoder.port("angle"));
    let trace = rt.advance_recording(0.5, 2.0e-4, 1, &[angle, output]).unwrap();
    let count = TAU / 1024.0;
    let mut distinct = std::collections::BTreeSet::new();
    for (t, row) in trace.time.iter().zip(&trace.state).skip(2) {
        let (truth, read) = (row[0], row[1]);
        approx(read, (read / count).round() * count, 1.0e-12);
        assert!((read - truth).abs() < count, "t={t}: read {read}, true {truth}");
        distinct.insert((read / count).round() as i64);
    }
    assert!(distinct.len() > 50, "{}", distinct.len());
}

#[test]
fn sample_and_hold_changes_only_at_sample_instants() {
    let (period, h) = (1.0e-3, 2.5e-4);
    let (mut rt, held, _) = tacho(vec![("period", period)], 2.0);
    let trace = rt.advance_recording(0.1, h, 1, &[held]).unwrap();
    let values = trace.column(0);
    let mut changes = 0;
    for k in 1..values.len() {
        if values[k] != values[k - 1] {
            changes += 1;
            let t = trace.time[k];
            let since = t - (t / period).floor() * period;
            assert!(since < h + 1.0e-9 || period - since < 1.0e-9, "changed at t={t}, {since} after a sample instant");
        }
    }
    assert!((90..=101).contains(&changes), "{changes} changes");
}

#[test]
fn noise_is_deterministic_per_seed_with_the_right_spread() {
    let samples = |seed: f64| {
        let (mut rt, held, _) = tacho(vec![("period", 1.0e-3), ("noise", 0.2), ("seed", seed)], 0.0);
        let mut values = rt.advance_recording(2.0, 5.0e-4, 1, &[held]).unwrap().column(0);
        values.dedup();
        values.split_off(1)
    };
    let (a, b, c) = (samples(1.0), samples(1.0), samples(2.0));
    assert_eq!(a, b, "same seed, same trace");
    assert_ne!(a, c, "different seed, different trace");
    assert!(a.len() >= 1990, "{} samples", a.len());
    let mean = a.iter().sum::<f64>() / a.len() as f64;
    let std = (a.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / a.len() as f64).sqrt();
    approx(mean, 5.0, 0.03);
    assert!((std - 0.2).abs() < 0.04, "std {std}");
}

#[test]
fn current_and_voltage_sensors_read_the_circuit() {
    let registry = registry();
    let mut m = ModelWorld::default();
    let source = m.part(&registry, "source", el::VOLTAGE_SOURCE, [("voltage", 12.0)]).unwrap();
    let load = m.part(&registry, "load", el::RESISTOR, [("resistance", 4.0)]).unwrap();
    let ground = m.part(&registry, "ground", el::GROUND, []).unwrap();
    let ammeter = m.part(&registry, "ammeter", sense::CURRENT_SENSOR, []).unwrap();
    let voltmeter = m.part(&registry, "voltmeter", sense::VOLTAGE_SENSOR, []).unwrap();
    m.connect([source.port("p"), ammeter.port("p")]);
    m.connect([ammeter.port("n"), load.port("p"), voltmeter.port("p")]);
    m.connect([load.port("n"), source.port("n"), ground.port("pin"), voltmeter.port("n")]);
    sink(&mut m, &registry, ammeter.port("current"));
    let mut rt = runtime(m, &registry);
    rt.advance(1.0e-3, 1.0e-4).unwrap();
    approx(rt.get(rt.signal_id(ammeter.port("current"))), 3.0, 1.0e-9);
    approx(rt.get(rt.signal_id(voltmeter.port("voltage"))), 12.0, 1.0e-9);
}

#[test]
fn force_sensor_reads_the_spring_force() {
    let registry = registry();
    let mut m = ModelWorld::default();
    let wall = m.part(&registry, "wall", tr::GROUND, []).unwrap();
    let spring = m.part(&registry, "spring", tr::SPRING, [("stiffness", 50.0), ("rest", -0.1)]).unwrap();
    let cell = m.part(&registry, "cell", sense::FORCE_SENSOR, []).unwrap();
    let mass = m.part(&registry, "mass", tr::MASS, [("mass", 0.5)]).unwrap();
    m.connect([wall.port("axis"), spring.port("a")]);
    m.connect([spring.port("b"), cell.port("a")]);
    m.connect([cell.port("b"), mass.port("axis")]);
    let mut rt = runtime(m, &registry);
    let ids = [rt.signal_id(cell.port("force")), rt.across_id(spring.port("a")), rt.across_id(spring.port("b"))];
    let trace = rt.advance_recording(0.5, 1.0e-3, 1, &ids).unwrap();
    // The midpoint rule commits a multiplier at the step's midpoint while
    // positions land at its end, so the cell agrees with the spring force
    // at the midpoint of consecutive records — exactly, not to O(h²).
    let mut largest = 0.0f64;
    for pair in trace.state.windows(2) {
        let (before, after) = (&pair[0], &pair[1]);
        let stretch = 0.5 * (before[1] + after[1]) - 0.5 * (before[2] + after[2]) + 0.1;
        approx(after[0], 50.0 * stretch, 1.0e-6);
        largest = largest.max(after[0].abs());
    }
    assert!(largest > 4.0, "the spring is loaded: {largest}");
}

#[test]
fn imu_reads_specific_force_and_rate() {
    let run = |gravity: f64, theta: f64, omega: f64| {
        let registry = registry();
        let mut m = ModelWorld::default();
        let b = m.part(&registry, "body", body::PLANAR_RIGID_BODY, [("mass", 1.0), ("inertia", 0.1), ("gravity", gravity), ("initial.theta", theta), ("initial.omega", omega)]).unwrap();
        let imu = m.part(&registry, "imu", sense::IMU, []).unwrap();
        m.connect([b.port("frame"), imu.port("frame")]);
        let mut rt = runtime(m, &registry);
        rt.advance(0.01, 1.0e-3).unwrap();
        [rt.get(rt.signal_id(imu.port("ax"))), rt.get(rt.signal_id(imu.port("ay"))), rt.get(rt.signal_id(imu.port("gyro")))]
    };
    // Held at rest: no acceleration, so the unit feels gravity up its y-axis.
    let [ax, ay, gyro] = run(0.0, 0.0, 0.0);
    approx(ax, 0.0, 1.0e-9);
    approx(ay, 9.81, 1.0e-9);
    approx(gyro, 0.0, 1.0e-9);
    // Rolled by a quarter turn, gravity lies along the body's x-axis.
    let [ax, ay, _] = run(0.0, TAU / 4.0, 0.0);
    approx(ax, 9.81, 1.0e-9);
    approx(ay, 0.0, 1.0e-9);
    // Spinning: the gyro reads the body rate.
    let [_, _, gyro] = run(0.0, 0.0, 2.0);
    approx(gyro, 2.0, 1.0e-9);
    // Free fall: weightless.
    let [ax, ay, _] = run(9.81, 0.0, 0.0);
    approx(ax, 0.0, 1.0e-9);
    approx(ay, 0.0, 1.0e-6);
}

#[test]
fn imu_channel_noise_and_resolution_have_distinct_units_and_seeded_streams() {
    use sim_core::QuantityKind as Q;
    let run = |extra: Vec<(&str, f64)>| {
        let registry = registry();
        let mut model = ModelWorld::default();
        let body = model.part(&registry, "body", body::PLANAR_RIGID_BODY,
            [("mass", 1.), ("inertia", 0.1), ("gravity", 0.)]).unwrap();
        let mut parameters = vec![("period", 0.001)];
        parameters.extend(extra);
        let imu = model.part(&registry, "imu", sense::IMU, parameters).unwrap();
        model.connect([body.port("frame"), imu.port("frame")]);
        let mut rt = runtime(model, &registry);
        let ids = [rt.signal_id(imu.port("ax")), rt.signal_id(imu.port("ay")), rt.signal_id(imu.port("gyro"))];
        for (id, kind) in ids.iter().zip([Q::LinearAcceleration, Q::LinearAcceleration, Q::AngularVelocity]) {
            assert_eq!(rt.model.state.entry(*id).unwrap().quantity, kind);
        }
        rt.advance_recording(0.02, 0.00025, 1, &ids).unwrap()
    };
    let quantised = run(vec![("bias.ax", 0.14), ("bias.gyro", 0.2), ("quantum.ax", 0.1), ("quantum.gyro", 0.3)]);
    let last = quantised.state.last().unwrap();
    approx(last[0], 0.1, 1e-12);
    approx(last[1], 9.81, 1e-9);
    approx(last[2], 0.3, 1e-12);
    let noise = |seed| run(vec![("seed", seed), ("noise.ax", 0.2), ("noise.ay", 0.2)]);
    let (a, b, c) = (noise(17.), noise(17.), noise(18.));
    assert_eq!(a.state, b.state);
    assert_ne!(a.state, c.state);
    assert!(a.state.iter().any(|row| (row[0] - (row[1] - 9.81)).abs() > 0.01), "axes must have independent streams");
    assert!(a.state.iter().all(|row| row[2].abs() < 1e-12), "acceleration noise must not affect the gyro");
}

#[test]
fn faults_are_events_on_the_held_output() {
    let (period, h, onset) = (1.0e-3, 1.0e-4, 0.0505);
    let run = |mode: f64, extra: (&'static str, f64)| {
        let (mut rt, held, speed) = tacho(vec![("period", period), ("fault.mode", mode), ("fault.time", onset), extra], 2.0);
        rt.advance_recording(0.1, h, 1, &[held, speed]).unwrap()
    };
    // Stuck: frozen at the last good sample.
    let trace = run(1.0, ("fault.duration", 0.0));
    let frozen = at(&trace, 0, onset);
    assert!(frozen > 0.0);
    for (t, row) in trace.time.iter().zip(&trace.state) {
        if *t > onset {
            assert_eq!(row[0], frozen, "t={t}");
        } else if *t < 0.0499 {
            assert_ne!(row[0], frozen, "t={t}");
        }
    }
    // Dropout: zero for its duration, then the next sample restores the reading.
    let trace = run(2.0, ("fault.duration", 0.01));
    for (t, row) in trace.time.iter().zip(&trace.state) {
        if *t > onset + h && *t < onset + 0.01 {
            assert_eq!(row[0], 0.0, "t={t}");
        }
    }
    assert_ne!(at(&trace, 0, 0.0503), 0.0, "reading before the dropout");
    let restored = at(&trace, 0, 0.0615);
    approx(restored, at(&trace, 1, 0.0615), 0.1);
    // Latency spike: exactly five samples skipped, then sampling resumes.
    let trace = run(3.0, ("fault.samples", 5.0));
    let stale = at(&trace, 0, onset);
    assert_ne!(at(&trace, 0, 0.0495), stale, "samples were arriving before the fault");
    for (t, row) in trace.time.iter().zip(&trace.state) {
        if *t > onset && *t < 0.056 {
            assert_eq!(row[0], stale, "t={t}");
        }
    }
    assert_ne!(at(&trace, 0, 0.0562), stale, "the sample at 56 ms lands");
    assert_ne!(at(&trace, 0, 0.0572), at(&trace, 0, 0.0562), "and sampling continues");
}

#[test]
fn pwm_driver_drives_a_load_with_a_dead_band() {
    let run = |duty: f64, dead_band: f64| {
        let registry = registry();
        let mut m = ModelWorld::default();
        let command = m.part(&registry, "command", ctl::CONSTANT, [("value", duty)]).unwrap();
        let driver = m.part(&registry, "driver", sense::PWM_DRIVER, [("supply", 24.0), ("dead_band", dead_band)]).unwrap();
        let ammeter = m.part(&registry, "ammeter", sense::CURRENT_SENSOR, []).unwrap();
        let load = m.part(&registry, "load", el::RESISTOR, [("resistance", 2.0)]).unwrap();
        let ground = m.part(&registry, "ground", el::GROUND, []).unwrap();
        m.connect([command.port("value"), driver.port("duty")]);
        m.connect([driver.port("p"), ammeter.port("p")]);
        m.connect([ammeter.port("n"), load.port("p")]);
        m.connect([load.port("n"), driver.port("n"), ground.port("pin")]);
        sink(&mut m, &registry, ammeter.port("current"));
        let mut rt = runtime(m, &registry);
        rt.advance(1.0e-3, 1.0e-4).unwrap();
        rt.get(rt.signal_id(ammeter.port("current")))
    };
    approx(run(0.5, 0.0), 6.0, 1.0e-9);
    approx(run(-2.0, 0.0), -12.0, 1.0e-9);
    approx(run(0.05, 0.1), 0.0, 1.0e-9);
    approx(run(0.5, 0.1), 6.0, 1.0e-9);
}

#[test]
fn servo_torque_accelerates_a_free_inertia_within_its_limits() {
    let run = |command: f64, torque_limit: f64, current_limit: f64| {
        let registry = registry();
        let mut m = ModelWorld::default();
        let cmd = m.part(&registry, "command", ctl::CONSTANT, [("value", command)]).unwrap();
        let servo = m.part(&registry, "servo", sense::SERVO, [("bandwidth", 50.0), ("torque_limit", torque_limit), ("torque_constant", 0.05), ("current_limit", current_limit)]).unwrap();
        let rotor = m.part(&registry, "rotor", rot::INERTIA, [("inertia", 0.1)]).unwrap();
        m.connect([cmd.port("value"), servo.port("command")]);
        m.connect([servo.port("shaft"), rotor.port("shaft")]);
        let mut rt = runtime(m, &registry);
        let ids = [rt.state_id(rotor.behavior, "speed"), rt.signal_id(servo.port("current"))];
        let trace = rt.advance_recording(0.2, 1.0e-3, 1, &ids).unwrap();
        let acceleration = (at(&trace, 0, 0.2) - at(&trace, 0, 0.15)) / 0.05;
        (acceleration, at(&trace, 1, 0.2))
    };
    let (acceleration, current) = run(0.2, f64::INFINITY, f64::INFINITY);
    approx(acceleration, 2.0, 1.0e-6);
    approx(current, 4.0, 1.0e-6);
    let (acceleration, current) = run(5.0, 1.0, f64::INFINITY);
    approx(acceleration, 10.0, 1.0e-6);
    approx(current, 20.0, 1.0e-6);
    let (acceleration, current) = run(5.0, f64::INFINITY, 10.0);
    approx(acceleration, 5.0, 1.0e-6);
    approx(current, 10.0, 1.0e-6);
}

#[test]
fn quantiser_rounds_and_clamps() {
    let run = |input: f64, limit: f64| {
        let registry = registry();
        let mut m = ModelWorld::default();
        let source = m.part(&registry, "source", ctl::CONSTANT, [("value", input)]).unwrap();
        let q = m.part(&registry, "q", sense::QUANTISER, [("step", 0.1), ("limit", limit)]).unwrap();
        m.connect([source.port("value"), q.port("input")]);
        sink(&mut m, &registry, q.port("output"));
        let mut rt = runtime(m, &registry);
        rt.advance(1.0e-3, 1.0e-3).unwrap();
        rt.get(rt.signal_id(q.port("output")))
    };
    approx(run(0.37, f64::INFINITY), 0.4, 1.0e-12);
    approx(run(-0.24, f64::INFINITY), -0.2, 1.0e-12);
    approx(run(3.0, 1.0), 1.0, 1.0e-12);
}
