//! The external-controller seam: a proportional law reaches the plant
//! through `control.external` exactly as it does through the library's own
//! sampled controller, the contract carries names and units, a missing or
//! dead controller is an error naming the element, and lockstep is
//! deterministic.

use sim_core::{Contract, Coupler, CouplerError, FnCoupler, ModelWorld};
use sim_domain_bridges::elements as bridge;
use sim_domain_control::elements as ctl;
use sim_domain_control::external::EXTERNAL;
use sim_domain_electrical::elements as el;
use sim_domain_rotational::elements as rot;
use sim_phenomena::world::{registry, runtime};

const PERIOD: f64 = 2.0e-3;
const GAIN: f64 = 0.15;

/// Voltage-driven motor speed loop; the controller is either the library's
/// sampled proportional element or the seam.
fn plant(external: bool) -> (sim_compile::Runtime, sim_core::BehaviorId, sim_core::StateId) {
    let registry = registry();
    let mut m = ModelWorld::default();
    let source = m.part(&registry, "source", el::CONTROLLED_VOLTAGE_SOURCE, []).unwrap();
    let ground = m.part(&registry, "ground", el::GROUND, []).unwrap();
    let motor = m.part(&registry, "motor", bridge::BRUSHED_MOTOR, [("resistance", 0.6), ("inductance", 0.0), ("torque_constant", 0.05), ("back_emf_constant", 0.05)]).unwrap();
    let rotor = m.part(&registry, "rotor", rot::INERTIA, [("inertia", 2.0e-4), ("damping", 2.0e-4), ("initial.speed", 10.0)]).unwrap();
    let mount = m.part(&registry, "mount", rot::GROUND, []).unwrap();
    let tacho = m.part(&registry, "tacho", rot::SPEED_SENSOR, []).unwrap();
    let controller = if external {
        m.part(&registry, "controller", EXTERNAL, [("period", PERIOD), ("sense.speed", 0.0), ("act.voltage", 0.0)]).unwrap()
    } else {
        m.part(&registry, "controller", ctl::SAMPLED_PROPORTIONAL, [("gain", GAIN), ("period", PERIOD), ("limit", 1.0e9)]).unwrap()
    };
    m.connect([source.port("p"), motor.port("p")]);
    m.connect([source.port("n"), motor.port("n"), ground.port("pin")]);
    m.connect([motor.port("shaft"), rotor.port("shaft"), tacho.port("shaft")]);
    m.connect([motor.port("case"), mount.port("flange")]);
    m.connect([tacho.port("speed"), controller.port(if external { "sense.speed" } else { "measured" })]);
    m.connect([controller.port(if external { "act.voltage" } else { "command" }), source.port("voltage")]);
    let rt = runtime(m, &registry);
    let speed = rt.state_id(rotor.behavior, "speed");
    (rt, controller.behavior, speed)
}

fn proportional() -> Box<dyn Coupler> {
    Box::new(FnCoupler(|_t: f64, sensors: &[f64], actuators: &mut [f64]| actuators[0] = -GAIN * sensors[0]))
}

#[test]
fn the_seam_reproduces_the_library_controller() {
    let (mut native, _, native_speed) = plant(false);
    let (mut seam, controller, seam_speed) = plant(true);
    seam.attach(controller, proportional()).unwrap();
    let a = native.advance_recording(0.1, 5.0e-4, 1, &[native_speed]).unwrap();
    let b = seam.advance_recording(0.1, 5.0e-4, 1, &[seam_speed]).unwrap();
    let worst = a.column(0).iter().zip(b.column(0)).map(|(x, y)| (x - y).abs()).fold(0.0, f64::max);
    assert!(worst < 1.0e-9, "seam and native traces differ by {worst}");
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
fn a_seam_without_a_controller_fails_by_name() {
    let (mut rt, _, _) = plant(true);
    let err = rt.advance(0.01, 1.0e-3).unwrap_err();
    let text = err.to_string();
    assert!(text.contains("`controller`") && text.contains("no coupler"), "{text}");
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
    rt.attach(controller, Box::new(Dies(0))).unwrap();
    let err = rt.advance(0.05, 1.0e-3).unwrap_err();
    let text = err.to_string();
    assert!(text.contains("`controller`") && text.contains("segfault") && text.contains("t=0.006"), "{text}");
}

#[test]
fn attaching_to_a_non_seam_is_refused() {
    let (mut rt, _, _) = plant(false);
    let rotor = rt.model.behaviors.keys().next().unwrap();
    let err = rt.attach(rotor, proportional()).unwrap_err();
    assert!(err.to_string().contains("not an external control element"), "{err}");
}

#[test]
fn lockstep_is_deterministic() {
    let run = || {
        let (mut rt, controller, speed) = plant(true);
        rt.attach(controller, proportional()).unwrap();
        rt.advance_recording(0.05, 5.0e-4, 1, &[speed]).unwrap().column(0).to_vec()
    };
    assert_eq!(run(), run());
}

#[test]
fn delays_are_whole_samples() {
    let registry = registry();
    let mut m = ModelWorld::default();
    let source = m.part(&registry, "ramp", ctl::SINE, [("amplitude", 1.0), ("frequency", 0.0)]).unwrap();
    let seam = m.part(&registry, "seam", EXTERNAL, [("period", 0.01), ("output_delay", 2.0), ("input_delay", 1.0), ("sense.x", 0.0), ("act.y", 0.0)]).unwrap();
    let sink = m.part(&registry, "sink", ctl::LAG_CHAIN, [("delay", 1.0), ("stages", 1.0)]).unwrap();
    m.connect([source.port("value"), seam.port("sense.x")]);
    m.connect([seam.port("act.y"), sink.port("input")]);
    let mut rt = runtime(m, &registry);
    // Pass the sensor straight through; the sensor is a constant 1.
    rt.attach(seam.behavior, Box::new(FnCoupler(|_t: f64, s: &[f64], a: &mut [f64]| a[0] = s[0]))).unwrap();
    let held = rt.state_id(seam.behavior, "act.y");
    // Sample 0 at t=0 sees the input queue's initial zero and queues its
    // command; the held output becomes 1 only after input_delay +
    // output_delay = 3 more samples.
    let mut seen = Vec::new();
    for _ in 0..5 {
        rt.advance(0.01, 0.005).unwrap();
        seen.push(rt.get(held));
    }
    assert_eq!(seen, vec![0.0, 0.0, 0.0, 1.0, 1.0], "{seen:?}");
}
