//! Teleoperation as blocks (docs/cad-migration-and-composition-plan.md, B3):
//! a drive input block turns a person's requests into a body twist under the
//! robot's drive profile, on simulation time, at its ticks; a host that
//! cannot drive refuses it by name.

use sim_core::{BlockInterface, BlockPort, BlockTiming, ImplementationRef, ModelWorld, QuantityKind};
use sim_runtime::drive_host::DriveRequest;
use sim_runtime::teleop::{DriveLink, TWIST_OUTPUTS, drive_host_name, drive_hosts};
use std::path::PathBuf;

fn profile_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/wheeled-robot/baseline")
}

/// A model of one drive input block (the rover's profile), its outputs left open.
fn model() -> (ModelWorld, sim_core::BehaviorId) {
    let mut m = ModelWorld::default();
    let kinds = [QuantityKind::LinearVelocity, QuantityKind::LinearVelocity, QuantityKind::AngularVelocity];
    let interface = BlockInterface { inputs: Vec::new(), outputs: TWIST_OUTPUTS.iter().zip(kinds).map(|(n, k)| BlockPort::new(*n, k).start(0.0)).collect(), feedthrough: true };
    let block = m.add_block("teleop", interface, BlockTiming::periodic(0.02), ImplementationRef::Host { name: drive_host_name("robot.drive.json") }).unwrap();
    for name in TWIST_OUTPUTS {
        m.connect([block.ports[name]]);
    }
    (m, block.behavior)
}

#[test]
fn a_drive_input_ramps_under_the_profile_and_stops_on_the_deadman() {
    let (m, behavior) = model();
    let registry = sim_runtime::registry();
    let mut runtime = sim_compile::Runtime::new(m, &registry, sim_dynamics::Integrator::implicit_midpoint()).unwrap();
    let link = DriveLink::default();
    let mut hosts = drive_hosts(&runtime.model, Some(&profile_dir()), &link).unwrap();
    sim_runtime::system_blocks::bind_with(&mut runtime, Some(&profile_dir()), &mut hosts).unwrap();
    assert_eq!(link.bound().as_deref(), Some("robot.drive.json"));
    assert_eq!(link.supported(), Some([true, false, true]), "the rover's profile drives forward and yaw only");
    // Full forward once: 0.26 m/s at most, 0.5 m/s² up, then the 0.5 s deadman ramps it back to zero.
    link.drive(&DriveRequest::Axes { forward: 1.0, lateral: 0.0, yaw: 0.0 }).unwrap();
    let vx = runtime.state_id(behavior, "out.0");
    runtime.advance(0.4, 1.0e-3).unwrap();
    let ramping = runtime.model.state.get(vx).unwrap();
    assert!((ramping - 0.2).abs() < 0.02, "vx at 0.4 s: {ramping} (0.5 m/s² from rest)");
    runtime.advance(1.0, 1.0e-3).unwrap();
    assert_eq!(runtime.model.state.get(vx).unwrap(), 0.0, "the deadman stopped the twist");
    assert!(link.status().unwrap().expired);
    // A lateral request the profile does not support is refused by name.
    assert!(link.drive(&DriveRequest::Axes { forward: 0.0, lateral: 0.5, yaw: 0.0 }).unwrap_err().contains("lateral"));
}

#[test]
fn a_host_that_cannot_drive_refuses_the_block_by_name() {
    let (m, _) = model();
    let registry = sim_runtime::registry();
    let mut runtime = sim_compile::Runtime::new(m, &registry, sim_dynamics::Integrator::implicit_midpoint()).unwrap();
    let error = sim_runtime::system_blocks::bind(&mut runtime, Some(&profile_dir())).unwrap_err();
    assert!(error.contains("teleop") && error.contains("drive_input:robot.drive.json"), "{error}");
    // Nothing bound: a request has nothing to drive.
    assert!(DriveLink::default().drive(&DriveRequest::Stop).unwrap_err().contains("drive input"));
}
