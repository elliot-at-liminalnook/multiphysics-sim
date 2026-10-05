//! Blocks: executable controllers joined to the coupled model through typed
//! signals and run by the runtime's scheduler at committed clock ticks
//! (docs/architecture/composition.md). A proportional law reaches the plant
//! through a block exactly as through the library's own sampled controller;
//! the contract carries names and units; timing (first tick, delays, end-of-
//! step outputs, schedules) is exact; faults name the block; units, loops and
//! start values are checked before anything runs; checkpoints are honest.

use sim_core::{
    BlockImplementation, BlockInterface, BlockPort, BlockTiming, Checkpoint, Clock, Contract, Coupler, CouplerError, FnCoupler, ImplementationRef, ModelWorld,
    QuantityKind as Q,
};
use sim_domain_bridges::elements as bridge;
use sim_domain_control::elements as ctl;
use sim_domain_electrical::elements as el;
use sim_domain_rotational::elements as rot;
use sim_phenomena::world::{registry, runtime};
use std::sync::{Arc, Mutex};

const PERIOD: f64 = 2.0e-3;
const GAIN: f64 = 0.15;

fn host(name: &str) -> ImplementationRef {
    ImplementationRef::Host { name: name.into() }
}

/// Voltage-driven motor speed loop; the controller is either the library's
/// sampled proportional element or a block.
fn plant(block: bool) -> (sim_compile::Runtime, sim_core::BehaviorId, sim_core::StateId) {
    let registry = registry();
    let mut m = ModelWorld::default();
    let source = m.part(&registry, "source", el::CONTROLLED_VOLTAGE_SOURCE, []).unwrap();
    let ground = m.part(&registry, "ground", el::GROUND, []).unwrap();
    let motor = m.part(&registry, "motor", bridge::BRUSHED_MOTOR, [("resistance", 0.6), ("inductance", 0.0), ("torque_constant", 0.05), ("back_emf_constant", 0.05)]).unwrap();
    let rotor = m.part(&registry, "rotor", rot::INERTIA, [("inertia", 2.0e-4), ("damping", 2.0e-4), ("initial.speed", 10.0)]).unwrap();
    let mount = m.part(&registry, "mount", rot::GROUND, []).unwrap();
    let tacho = m.part(&registry, "tacho", rot::SPEED_SENSOR, []).unwrap();
    m.connect([source.port("p"), motor.port("p")]);
    m.connect([source.port("n"), motor.port("n"), ground.port("pin")]);
    m.connect([motor.port("shaft"), rotor.port("shaft"), tacho.port("shaft")]);
    m.connect([motor.port("case"), mount.port("flange")]);
    let controller = if block {
        m.add_wired_block("controller", BlockTiming::periodic(PERIOD), true, host("p"), &[("speed", tacho.port("speed"))], &[("voltage", source.port("voltage"))]).unwrap()
    } else {
        let c = m.part(&registry, "controller", ctl::SAMPLED_PROPORTIONAL, [("gain", GAIN), ("period", PERIOD), ("limit", 1.0e9)]).unwrap();
        m.connect([tacho.port("speed"), c.port("measured")]);
        m.connect([c.port("command"), source.port("voltage")]);
        c
    };
    let rt = runtime(m, &registry);
    let speed = rt.state_id(rotor.behavior, "speed");
    (rt, controller.behavior, speed)
}

fn proportional() -> Box<dyn Coupler> {
    Box::new(FnCoupler(|_t: f64, sensors: &[f64], actuators: &mut [f64]| actuators[0] = -GAIN * sensors[0]))
}

#[test]
fn a_block_reproduces_the_library_controller() {
    let (mut native, _, native_speed) = plant(false);
    let (mut blocked, controller, block_speed) = plant(true);
    blocked.bind_coupler(controller, proportional(), false).unwrap();
    let a = native.advance_recording(0.1, 5.0e-4, 1, &[native_speed]).unwrap();
    let b = blocked.advance_recording(0.1, 5.0e-4, 1, &[block_speed]).unwrap();
    let worst = a.column(0).iter().zip(b.column(0)).map(|(x, y)| (x - y).abs()).fold(0.0, f64::max);
    assert!(worst < 1.0e-9, "block and native traces differ by {worst}");
    assert!(b.column(0).last().unwrap().abs() < 1.0, "the loop regulates the speed down: {:?}", b.column(0).last());
}

#[test]
fn the_contract_names_channels_with_units() {
    let (rt, controller, _) = plant(true);
    let contract: Contract = rt.contract(controller);
    assert_eq!(contract.element, "controller");
    assert_eq!(contract.period, PERIOD);
    assert_eq!(contract.sensors.len(), 1);
    assert_eq!(contract.sensors[0].name, "speed");
    assert_eq!(contract.sensors[0].unit(), "rad/s");
    assert_eq!(contract.actuators[0].name, "voltage");
    assert_eq!(contract.actuators[0].unit(), "V");
}

#[test]
fn a_block_without_an_implementation_fails_by_name() {
    let (mut rt, _, _) = plant(true);
    let text = rt.advance(0.01, 1.0e-3).unwrap_err().to_string();
    assert!(text.contains("`controller`") && text.contains("no implementation is bound"), "{text}");
}

#[test]
fn a_controller_that_dies_ends_the_run() {
    struct Dies(u32);
    impl Coupler for Dies {
        fn sample(&mut self, _t: f64, _s: &[f64], _a: &mut [f64]) -> Result<(), CouplerError> {
            self.0 += 1;
            if self.0 > 3 { Err(CouplerError::Exited("segfault".into())) } else { Ok(()) }
        }
    }
    let (mut rt, controller, _) = plant(true);
    rt.bind_coupler(controller, Box::new(Dies(0)), false).unwrap();
    let text = rt.advance(0.05, 1.0e-3).unwrap_err().to_string();
    assert!(text.contains("`controller`") && text.contains("segfault") && text.contains("t=0.006"), "{text}");
    // The fault is sticky: the run does not continue past it.
    assert!(rt.advance(0.01, 1.0e-3).is_err());
}

#[test]
fn binding_to_a_non_block_is_refused() {
    let (mut rt, _, _) = plant(false);
    let rotor = rt.model.behaviors.keys().next().unwrap();
    let err = rt.bind_coupler(rotor, proportional(), false).unwrap_err();
    assert!(err.to_string().contains("not a block"), "{err}");
}

#[test]
fn lockstep_is_deterministic() {
    let run = || {
        let (mut rt, controller, speed) = plant(true);
        rt.bind_coupler(controller, proportional(), false).unwrap();
        rt.advance_recording(0.05, 5.0e-4, 1, &[speed]).unwrap().column(0).to_vec()
    };
    assert_eq!(run(), run());
}

/// What an implementation saw and when, shared with the test.
#[derive(Default)]
struct Log {
    calls: Vec<(&'static str, f64, f64, Vec<f64>)>,
    terminated: usize,
}

/// A test implementation: output = input + `bias` (feedthrough or end-of-step).
struct Probe {
    interface: BlockInterface,
    bias: f64,
    log: Arc<Mutex<Log>>,
    fail_at: Option<usize>,
    output: Option<f64>,
}

impl BlockImplementation for Probe {
    fn label(&self) -> String {
        "probe".into()
    }
    fn interface(&self) -> BlockInterface {
        self.interface.clone()
    }
    fn initialize(&mut self, t: f64, dt: f64, inputs: &[f64], outputs: &mut [f64]) -> Result<(), String> {
        self.log.lock().unwrap().calls.push(("init", t, dt, inputs.to_vec()));
        outputs[0] = self.output.unwrap_or(inputs[0] + self.bias);
        Ok(())
    }
    fn step(&mut self, t: f64, dt: f64, inputs: &[f64], outputs: &mut [f64]) -> Result<(), String> {
        let mut log = self.log.lock().unwrap();
        log.calls.push(("step", t, dt, inputs.to_vec()));
        if self.fail_at.is_some_and(|n| log.calls.len() >= n) {
            return Err("deliberate".into());
        }
        outputs[0] = self.output.unwrap_or(inputs[0] + self.bias);
        Ok(())
    }
    fn terminate(&mut self) {
        self.log.lock().unwrap_or_else(|p| p.into_inner()).terminated += 1;
    }
    fn checkpoint(&self) -> Checkpoint {
        Checkpoint::Stateless
    }
    fn restore(&mut self, _state: &[u8]) -> Result<(), String> {
        Ok(())
    }
}

/// A shaft turning at exactly 1 rad/s (angle = t) read by a block whose
/// output drives a torque source on a second, free shaft.
fn ramp(timing: BlockTiming, feedthrough: bool, output_kind: Q) -> Result<(sim_compile::Runtime, sim_core::Instance), String> {
    let registry = registry();
    let mut m = ModelWorld::default();
    let shaft = m.part(&registry, "shaft", rot::INERTIA, [("inertia", 1.0), ("initial.speed", 1.0)]).unwrap();
    let angle = m.part(&registry, "angle", rot::ANGLE_SENSOR, []).unwrap();
    m.connect([shaft.port("shaft"), angle.port("shaft")]);
    let load = m.part(&registry, "load", rot::INERTIA, [("inertia", 1.0)]).unwrap();
    let torque = m.part(&registry, "torque", rot::TORQUE_SOURCE, []).unwrap();
    m.connect([load.port("shaft"), torque.port("shaft")]);
    let interface = BlockInterface { inputs: vec![BlockPort::new("x", Q::Angle)], outputs: vec![BlockPort::new("y", output_kind).start(-1.0)], feedthrough };
    let block = m.add_block("probe", interface, timing, host("probe")).map_err(|e| e.to_string())?;
    m.connect([angle.port("angle"), block.port("x")]);
    m.connect([block.port("y"), torque.port("torque")]);
    let rt = sim_compile::Runtime::new(m, &registry, sim_dynamics::Integrator::BackwardEuler(sim_phenomena::world::newton())).map_err(|e| e.to_string())?;
    Ok((rt, block))
}

fn probe(rt: &mut sim_compile::Runtime, feedthrough: bool, bias: f64) -> Arc<Mutex<Log>> {
    let log = Arc::new(Mutex::new(Log::default()));
    let interface = BlockInterface { inputs: vec![BlockPort::new("x", Q::Angle)], outputs: vec![BlockPort::new("y", Q::Torque)], feedthrough };
    rt.bind_block("probe", Box::new(Probe { interface, bias, log: log.clone(), fail_at: None, output: None })).unwrap();
    log
}

/// The block's output signal after each of `n` advances of `chunk`.
fn outputs(rt: &mut sim_compile::Runtime, block: &sim_core::Instance, chunk: f64, n: usize) -> Vec<f64> {
    let y = rt.signal_id(block.port("y"));
    (0..n).map(|_| {
        rt.advance(chunk, chunk / 4.0).unwrap();
        (rt.get(y) * 1e9).round() / 1e9
    }).collect()
}

#[test]
fn feedthrough_outputs_apply_at_the_tick_that_computed_them() {
    let (mut rt, block) = ramp(BlockTiming::periodic(0.01), true, Q::Torque).unwrap();
    let log = probe(&mut rt, true, 100.0);
    // Ticks at 0, 0.01, ...: after the advance ending at t the output is t + 100.
    assert_eq!(outputs(&mut rt, &block, 0.01, 3), vec![100.01, 100.02, 100.03]);
    let log = log.lock().unwrap();
    assert_eq!(log.calls[0].0, "init");
    assert!(log.calls[0].1 == 0.0 && log.calls[0].3[0].abs() < 1e-12, "the first tick is at the start and sees x(0): {:?}", log.calls[0]);
    assert!(log.calls[1..].iter().all(|c| c.0 == "step" && (c.2 - 0.01).abs() < 1e-15));
}

#[test]
fn end_of_step_outputs_apply_one_period_later() {
    // FMI co-simulation semantics: the step from t computes the value at t + T.
    let (mut rt, block) = ramp(BlockTiming::periodic(0.01), false, Q::Torque).unwrap();
    let log = probe(&mut rt, false, 100.0);
    // Initial outputs (x(0) + 100) apply at 0; the step from 0 (input x(0))
    // applies at 0.01; the step from 0.01 (input 0.01) at 0.02.
    assert_eq!(outputs(&mut rt, &block, 0.01, 3), vec![100.0, 100.01, 100.02]);
    let log = log.lock().unwrap();
    assert_eq!(log.calls.iter().map(|c| c.0).collect::<Vec<_>>()[..3], ["init", "step", "step"]);
    assert_eq!(log.calls[1].1, 0.0, "the first step starts at the first tick");
}

#[test]
fn delays_are_whole_samples() {
    let mut timing = BlockTiming::periodic(0.01);
    timing.input_delay = 1;
    timing.output_delay = 2;
    let (mut rt, block) = ramp(timing, true, Q::Torque).unwrap();
    let log = probe(&mut rt, true, 0.0);
    // Output delay 2: the start value holds until the third tick (0.02),
    // which applies what the first tick (0) computed. Input delay 1: the
    // tick at 0 sees x(0) (before the line fills, its oldest frame), the
    // tick at 0.01 sees x(0), the tick at 0.02 sees x(0.01).
    assert_eq!(outputs(&mut rt, &block, 0.01, 5), vec![-1.0, 0.0, 0.0, 0.01, 0.02]);
    let seen: Vec<f64> = log.lock().unwrap().calls.iter().map(|c| (c.3[0] * 1e9).round() / 1e9).collect();
    assert_eq!(seen[..4], [0.0, 0.0, 0.01, 0.02]);
}

#[test]
fn an_offset_clock_needs_start_values_and_ticks_on_its_grid() {
    let mut timing = BlockTiming::periodic(0.01);
    timing.clock = Clock::Periodic { period: 0.01, offset: 0.005 };
    let (mut rt, block) = ramp(timing.clone(), true, Q::Torque).unwrap();
    let log = probe(&mut rt, true, 0.0);
    assert_eq!(outputs(&mut rt, &block, 0.01, 2), vec![0.005, 0.015]);
    assert_eq!(log.lock().unwrap().calls.iter().map(|c| c.1).collect::<Vec<_>>(), vec![0.005, 0.015]);

    // Without a start value, a late first tick leaves the output undefined: refused.
    let registry = registry();
    let mut m = ModelWorld::default();
    let c = m.part(&registry, "c", ctl::CONSTANT, [("value", 1.0)]).unwrap();
    let interface = BlockInterface { inputs: vec![BlockPort::new("x", Q::Dimensionless)], outputs: vec![BlockPort::new("y", Q::Dimensionless)], feedthrough: true };
    let b = m.add_block("late", interface, timing, host("late")).unwrap();
    m.connect([c.port("value"), b.port("x")]);
    m.connect([b.port("y")]);
    let err = sim_compile::Runtime::new(m, &registry, sim_dynamics::Integrator::BackwardEuler(sim_phenomena::world::newton())).err().unwrap().to_string();
    assert!(err.contains("`late`") && err.contains("no start value"), "{err}");
}

#[test]
fn a_schedule_ticks_at_its_times_and_stops() {
    let timing = BlockTiming::with_clock(Clock::Times { times: vec![0.0, 0.003, 0.01] });
    let (mut rt, block) = ramp(timing, true, Q::Torque).unwrap();
    let log = probe(&mut rt, true, 0.0);
    assert_eq!(outputs(&mut rt, &block, 0.01, 2), vec![0.01, 0.01], "no tick after the last time: the output holds");
    let log = log.lock().unwrap();
    assert_eq!(log.calls.iter().map(|c| c.1).collect::<Vec<_>>(), vec![0.0, 0.003, 0.01]);
    assert!((log.calls[1].2 - 0.007).abs() < 1e-15, "dt is the interval to the next scheduled tick");
}

#[test]
fn units_must_match_exactly() {
    // The block's output says angle; the torque source takes a torque.
    let err = ramp(BlockTiming::periodic(0.01), true, Q::Angle).err().unwrap();
    assert!(err.contains("`probe`") && err.contains("signal `y`") && err.contains("Torque"), "{err}");
}

#[test]
fn an_implementation_with_another_interface_is_refused() {
    let (mut rt, _) = ramp(BlockTiming::periodic(0.01), true, Q::Torque).unwrap();
    let log = Arc::new(Mutex::new(Log::default()));
    let wrong = BlockInterface { inputs: vec![BlockPort::new("x", Q::Angle)], outputs: vec![BlockPort::new("y", Q::Torque)], feedthrough: false };
    let err = rt.bind_block("probe", Box::new(Probe { interface: wrong, bias: 0.0, log, fail_at: None, output: None })).unwrap_err().to_string();
    assert!(err.contains("feedthrough"), "{err}");
}

#[test]
fn a_feedthrough_cycle_between_blocks_is_an_algebraic_loop() {
    let registry = registry();
    let build = |delay: usize| {
        let mut m = ModelWorld::default();
        let interface = || BlockInterface { inputs: vec![BlockPort::new("x", Q::Dimensionless)], outputs: vec![BlockPort::new("y", Q::Dimensionless).start(0.0)], feedthrough: true };
        let a = m.add_block("a", interface(), BlockTiming::periodic(0.01), host("a")).unwrap();
        let mut timing = BlockTiming::periodic(0.01);
        timing.output_delay = delay;
        let b = m.add_block("b", interface(), timing, host("b")).unwrap();
        m.connect([a.port("y"), b.port("x")]);
        m.connect([b.port("y"), a.port("x")]);
        sim_compile::Runtime::new(m, &registry, sim_dynamics::Integrator::BackwardEuler(sim_phenomena::world::newton()))
    };
    let err = build(0).err().unwrap().to_string();
    assert!(err.contains("algebraic loop") && err.contains("a → b → a") || err.contains("b → a → b"), "{err}");
    assert!(build(1).is_ok(), "a sample of delay breaks the loop");
}

#[test]
fn faults_name_the_block_and_terminate_every_implementation() {
    for (case, fail_at, output, expected) in [
        ("error", Some(3), None, "deliberate"),
        ("non-finite", None, Some(f64::NAN), "NaN"),
        ("range", None, Some(5.0), "outside its declared range"),
    ] {
        let registry = registry();
        let mut m = ModelWorld::default();
        let c = m.part(&registry, "c", ctl::CONSTANT, [("value", 1.0)]).unwrap();
        let interface = BlockInterface { inputs: vec![BlockPort::new("x", Q::Dimensionless)], outputs: vec![BlockPort::new("y", Q::Dimensionless).range(Some(-2.0), Some(2.0))], feedthrough: true };
        let b = m.add_block("faulty", interface.clone(), BlockTiming::periodic(0.01), host("faulty")).unwrap();
        m.connect([c.port("value"), b.port("x")]);
        m.connect([b.port("y")]);
        let mut rt = sim_compile::Runtime::new(m, &registry, sim_dynamics::Integrator::BackwardEuler(sim_phenomena::world::newton())).unwrap();
        let log = Arc::new(Mutex::new(Log::default()));
        let mut interface = interface;
        interface.outputs[0] = BlockPort::new("y", Q::Dimensionless);
        rt.bind_block("faulty", Box::new(Probe { interface, bias: 0.0, log: log.clone(), fail_at, output })).unwrap();
        let err = rt.advance(0.1, 0.005).unwrap_err().to_string();
        assert!(err.contains("`faulty`") && err.contains(expected), "{case}: {err}");
        drop(rt);
        assert_eq!(log.lock().unwrap().terminated, 1, "{case}: terminated exactly once");
    }
}

#[test]
fn a_missed_deadline_is_a_fault() {
    struct Slow;
    impl Coupler for Slow {
        fn sample(&mut self, _t: f64, _s: &[f64], a: &mut [f64]) -> Result<(), CouplerError> {
            std::thread::sleep(std::time::Duration::from_millis(5));
            a[0] = 0.0;
            Ok(())
        }
    }
    let mut timing = BlockTiming::periodic(0.01);
    timing.deadline_s = Some(1.0e-3);
    let (mut rt, block) = ramp(timing, true, Q::Torque).unwrap();
    rt.bind_coupler(block.behavior, Box::new(Slow), false).unwrap();
    let err = rt.advance(0.05, 0.005).unwrap_err().to_string();
    assert!(err.contains("missed its deadline"), "{err}");
}

#[test]
fn checkpoints_include_blocks_or_refuse() {
    // A stateless implementation: snapshot, run on, restore, rerun: identical.
    let (mut rt, block) = ramp(BlockTiming::periodic(0.01), false, Q::Torque).unwrap();
    probe(&mut rt, false, 1.0);
    rt.advance(0.025, 0.005).unwrap();
    let saved = rt.snapshot().unwrap();
    let a = outputs(&mut rt, &block, 0.01, 3);
    rt.restore(&saved).unwrap();
    let b = outputs(&mut rt, &block, 0.01, 3);
    assert_eq!(a, b);
    // A host coupler with state of its own: the runtime cannot capture it.
    let (mut rt, block) = ramp(BlockTiming::periodic(0.01), true, Q::Torque).unwrap();
    rt.bind_coupler(block.behavior, Box::new(FnCoupler(|_t: f64, s: &[f64], a: &mut [f64]| a[0] = s[0])), false).unwrap();
    let err = rt.snapshot().unwrap_err().to_string();
    assert!(err.contains("cannot capture"), "{err}");
}

/// The ramp (angle = t) read by block `p`, whose output block `c` passes
/// on to a torque source: p → c, declared in either order.
fn chain(p_first: bool, p_feedthrough: bool, p_delay: usize) -> Vec<f64> {
    let registry = registry();
    let mut m = ModelWorld::default();
    let shaft = m.part(&registry, "shaft", rot::INERTIA, [("inertia", 1.0), ("initial.speed", 1.0)]).unwrap();
    let angle = m.part(&registry, "angle", rot::ANGLE_SENSOR, []).unwrap();
    m.connect([shaft.port("shaft"), angle.port("shaft")]);
    let load = m.part(&registry, "load", rot::INERTIA, [("inertia", 1.0)]).unwrap();
    let torque = m.part(&registry, "torque", rot::TORQUE_SOURCE, []).unwrap();
    m.connect([load.port("shaft"), torque.port("shaft")]);
    let p_interface = BlockInterface { inputs: vec![BlockPort::new("x", Q::Angle)], outputs: vec![BlockPort::new("y", Q::Dimensionless).start(-1.0)], feedthrough: p_feedthrough };
    let c_interface = BlockInterface { inputs: vec![BlockPort::new("x", Q::Dimensionless)], outputs: vec![BlockPort::new("y", Q::Torque).start(-2.0)], feedthrough: true };
    let mut p_timing = BlockTiming::periodic(0.01);
    p_timing.output_delay = p_delay;
    let (p, c) = if p_first {
        let p = m.add_block("p", p_interface.clone(), p_timing, host("p")).unwrap();
        (p, m.add_block("c", c_interface.clone(), BlockTiming::periodic(0.01), host("c")).unwrap())
    } else {
        let c = m.add_block("c", c_interface.clone(), BlockTiming::periodic(0.01), host("c")).unwrap();
        (m.add_block("p", p_interface.clone(), p_timing, host("p")).unwrap(), c)
    };
    m.connect([angle.port("angle"), p.port("x")]);
    m.connect([p.port("y"), c.port("x")]);
    m.connect([c.port("y"), torque.port("torque")]);
    let mut rt = sim_compile::Runtime::new(m, &registry, sim_dynamics::Integrator::BackwardEuler(sim_phenomena::world::newton())).unwrap();
    for (name, interface) in [("p", p_interface), ("c", c_interface)] {
        rt.bind_block(name, Box::new(Probe { interface, bias: 0.0, log: Default::default(), fail_at: None, output: None })).unwrap();
    }
    outputs(&mut rt, &c, 0.01, 5)
}

#[test]
fn a_connection_s_delay_does_not_depend_on_declaration_order() {
    // What `c` passes on after the ticks at 0.01 … 0.05, whichever block
    // was declared first.
    for (feedthrough, delay, expected) in [
        // Same instant: c sees what p computed at this tick.
        (true, 0, [0.01, 0.02, 0.03, 0.04, 0.05]),
        // One sample of output delay: c sees p's output of the tick before.
        (true, 1, [0.0, 0.01, 0.02, 0.03, 0.04]),
        (true, 2, [-1.0, 0.0, 0.01, 0.02, 0.03]),
        // End of step (no feedthrough): p's step from the tick before.
        (false, 0, [0.0, 0.01, 0.02, 0.03, 0.04]),
        (false, 1, [0.0, 0.0, 0.01, 0.02, 0.03]),
    ] {
        for p_first in [true, false] {
            assert_eq!(chain(p_first, feedthrough, delay), expected, "feedthrough {feedthrough}, output delay {delay}, p declared first: {p_first}");
        }
    }
}

#[test]
fn an_adaptive_advance_ending_just_past_a_tick_takes_the_short_segment() {
    // Ticks every 0.01 s; the advance ends 0.005 s after one, less than the
    // smallest adaptive step asked for.
    let (mut rt, block) = ramp(BlockTiming::periodic(0.01), true, Q::Torque).unwrap();
    probe(&mut rt, true, 100.0);
    rt.advance_adaptive(0.015, 1.0e-3, 1.0e-6, 8.0e-3, 1.0e-2).unwrap();
    assert!((rt.time - 0.015).abs() < 1e-12, "{}", rt.time);
    assert!((rt.get(rt.signal_id(block.port("y"))) - 100.01).abs() < 1e-9);
    // Bounds that cannot hold are an error, not a panic.
    let err = rt.advance_adaptive(0.01, 1.0e-3, 1.0e-6, 1.0e-2, 1.0e-3).unwrap_err().to_string();
    assert!(err.contains("h_min"), "{err}");
}
