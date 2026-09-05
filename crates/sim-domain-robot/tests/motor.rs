mod common;
use common::*;
use sim_core::ModelWorld;
use sim_domain_robot::{BATTERY, H_BRIDGE, MOTOR_UNIT, SERVO_FIRMWARE, THERMAL_PROBE};

const V: f64 = 6.0;
const R: f64 = 2.0;
const KT: f64 = 0.02;
const RATIO: f64 = 50.0;
const ETA: f64 = 0.8;

#[test]
fn actuator_discovery_declares_units_defaults_and_rejects_invalid_values() {
    use sim_core::BehaviorTypeId;
    let registry = registry();
    for (kind, parameter, unit) in [
        (MOTOR_UNIT, "torque_constant", "N·m/A"),
        (MOTOR_UNIT, "reference", "K"),
        (MOTOR_UNIT, "gear_damping", "N·m·s/rad"),
        (H_BRIDGE, "on_resistance", "Ω"),
        (BATTERY, "capacity_ah", "A·h"),
        (SERVO_FIRMWARE, "ki", "1/(rad·s)"),
        (THERMAL_PROBE, "initial.node.temperature", "K"),
    ] {
        let descriptor = registry.get(&BehaviorTypeId::from(kind)).unwrap();
        let declaration = descriptor.parameters.as_ref().unwrap().iter().find(|p| p.name == parameter).unwrap();
        assert_eq!(declaration.unit, unit);
    }
    for (kind, key, value) in [
        (MOTOR_UNIT, "efficiency", 1.01), (MOTOR_UNIT, "gear_stifness", 1.),
        (H_BRIDGE, "current_limit", -1.), (BATTERY, "cells", 1.5),
        (BATTERY, "initial_soc", 1.1), (BATTERY, "cutoff_voltage", 6.),
        (SERVO_FIRMWARE, "rate", 0.5), (SERVO_FIRMWARE, "latency", -0.1),
        (THERMAL_PROBE, "temperature", 300.),
    ] {
        let mut parameters = std::collections::BTreeMap::new();
        if kind == MOTOR_UNIT { parameters.extend([("resistance".into(), 2.), ("torque_constant".into(), 0.02)]); }
        parameters.insert(key.into(), value);
        let error = registry.get(&BehaviorTypeId::from(kind)).unwrap().validate_parameters(&parameters).unwrap_err();
        assert!(error.to_string().contains(key), "{kind}: {error}");
    }
}

fn motor_params(extra: &[(&'static str, f64)]) -> Vec<(&'static str, f64)> {
    let mut p = vec![("resistance", R), ("inductance", 1e-4), ("torque_constant", KT), ("back_emf_constant", KT), ("rotor_inertia", 1e-7), ("ratio", RATIO), ("efficiency", ETA), ("gear_stiffness", 500.0), ("gear_damping", 0.02)];
    p.extend_from_slice(extra);
    p
}

#[test]
fn stall_torque_and_no_load_speed_match_the_spec() {
    let registry = registry();
    // Stalled: the shaft is held by a rotational ground.
    let mut m = ModelWorld::default();
    let src = m.part(&registry, "src", sim_domain_electrical::elements::VOLTAGE_SOURCE, [("voltage", V)]).unwrap();
    let gnd = m.part(&registry, "gnd", sim_domain_electrical::elements::GROUND, []).unwrap();
    let motor = m.part(&registry, "motor", MOTOR_UNIT, motor_params(&[])).unwrap();
    let hold = m.part(&registry, "hold", sim_domain_rotational::elements::GROUND, []).unwrap();
    let amb = m.part(&registry, "amb", sim_domain_thermal::AMBIENT, [("temperature", 293.15)]).unwrap();
    m.connect([src.port("p"), motor.port("p")]);
    m.connect([src.port("n"), motor.port("n"), gnd.port("pin")]);
    m.connect([motor.port("shaft"), hold.port("flange")]);
    m.connect([motor.port("winding"), amb.port("node")]);
    for s in ["current", "torque", "speed"] {
        m.connect([motor.port(s)]);
    }
    let mut rt = sim_compile::Runtime::new(m, &registry, euler()).unwrap();
    let torque_id = rt.signal_id(motor.port("torque"));
    let current_id = rt.state_id(motor.behavior, "current");
    rt.advance(0.5, 1e-3).unwrap();
    let stall = rt.get(torque_id);
    let expected = RATIO * ETA * KT * V / R;
    println!("stall: current {:.3} A, output torque {stall:.4} N·m vs spec {expected:.4}", rt.get(current_id));
    assert!((stall - expected).abs() / expected < 0.02);

    // Free: the shaft carries a small inertia only.
    let mut m = ModelWorld::default();
    let src = m.part(&registry, "src", sim_domain_electrical::elements::VOLTAGE_SOURCE, [("voltage", V)]).unwrap();
    let gnd = m.part(&registry, "gnd", sim_domain_electrical::elements::GROUND, []).unwrap();
    let motor = m.part(&registry, "motor", MOTOR_UNIT, motor_params(&[])).unwrap();
    let load = m.part(&registry, "load", sim_domain_rotational::elements::INERTIA, [("inertia", 1e-5)]).unwrap();
    let amb = m.part(&registry, "amb", sim_domain_thermal::AMBIENT, [("temperature", 293.15)]).unwrap();
    m.connect([src.port("p"), motor.port("p")]);
    m.connect([src.port("n"), motor.port("n"), gnd.port("pin")]);
    m.connect([motor.port("shaft"), load.port("shaft")]);
    m.connect([motor.port("winding"), amb.port("node")]);
    for s in ["current", "torque", "speed"] {
        m.connect([motor.port(s)]);
    }
    let mut rt = sim_compile::Runtime::new(m, &registry, euler()).unwrap();
    let speed_id = rt.signal_id(motor.port("speed"));
    rt.advance(2.0, 1e-3).unwrap();
    let w = rt.get(speed_id);
    let expected = V / KT / RATIO;
    println!("no load: output speed {w:.3} rad/s vs spec {expected:.3}");
    assert!((w - expected).abs() / expected < 0.02);
}

#[test]
fn winding_heats_with_the_thermal_time_constant() {
    let registry = registry();
    let (c, g) = (5.0, 0.25); // J/K, W/K  → τ = 20 s
    let mut m = ModelWorld::default();
    let src = m.part(&registry, "src", sim_domain_electrical::elements::VOLTAGE_SOURCE, [("voltage", V)]).unwrap();
    let gnd = m.part(&registry, "gnd", sim_domain_electrical::elements::GROUND, []).unwrap();
    let motor = m.part(&registry, "motor", MOTOR_UNIT, motor_params(&[("temp_coeff", 0.0), ("efficiency", 1.0)])).unwrap();
    let hold = m.part(&registry, "hold", sim_domain_rotational::elements::GROUND, []).unwrap();
    let cap = m.part(&registry, "cap", sim_domain_thermal::CAPACITANCE, [("heat_capacity", c), ("initial.temperature", 293.15)]).unwrap();
    let cond = m.part(&registry, "cond", sim_domain_thermal::CONDUCTANCE, [("conductance", g)]).unwrap();
    let amb = m.part(&registry, "amb", sim_domain_thermal::AMBIENT, [("temperature", 293.15)]).unwrap();
    let probe = m.part(&registry, "probe", THERMAL_PROBE, []).unwrap();
    m.connect([src.port("p"), motor.port("p")]);
    m.connect([src.port("n"), motor.port("n"), gnd.port("pin")]);
    m.connect([motor.port("shaft"), hold.port("flange")]);
    m.connect([motor.port("winding"), cap.port("node"), cond.port("a"), probe.port("node")]);
    m.connect([cond.port("b"), amb.port("node")]);
    for s in ["current", "torque", "speed"] {
        m.connect([motor.port(s)]);
    }
    m.connect([probe.port("temperature")]);
    let mut rt = sim_compile::Runtime::new(m, &registry, euler()).unwrap();
    let temp = rt.signal_id(probe.port("temperature"));
    let tau = c / g;
    rt.advance(tau, 1e-2).unwrap();
    let rise = rt.get(temp) - 293.15;
    let power = V * V / R;
    let final_rise = power / g;
    println!("winding rise after one time constant: {rise:.2} K of {final_rise:.2} K final ({:.1} %)", 100.0 * rise / final_rise);
    assert!((rise / final_rise - (1.0 - (-1.0f64).exp())).abs() < 0.02);
}

#[test]
fn backlash_opens_a_dead_zone() {
    let registry = registry();
    let b = 0.2;
    let mut m = ModelWorld::default();
    let src = m.part(&registry, "src", sim_domain_electrical::elements::VOLTAGE_SOURCE, [("voltage", V)]).unwrap();
    let gnd = m.part(&registry, "gnd", sim_domain_electrical::elements::GROUND, []).unwrap();
    let motor = m.part(&registry, "motor", MOTOR_UNIT, motor_params(&[("backlash", b)])).unwrap();
    let hold = m.part(&registry, "hold", sim_domain_rotational::elements::GROUND, []).unwrap();
    let amb = m.part(&registry, "amb", sim_domain_thermal::AMBIENT, [("temperature", 293.15)]).unwrap();
    m.connect([src.port("p"), motor.port("p")]);
    m.connect([src.port("n"), motor.port("n"), gnd.port("pin")]);
    m.connect([motor.port("shaft"), hold.port("flange")]);
    m.connect([motor.port("winding"), amb.port("node")]);
    for s in ["current", "torque", "speed"] {
        m.connect([motor.port(s)]);
    }
    let mut rt = sim_compile::Runtime::new(m, &registry, euler()).unwrap();
    let angle = rt.state_id(motor.behavior, "gear_angle");
    let torque = rt.signal_id(motor.port("torque"));
    rt.advance(1.0, 1e-3).unwrap();
    let expected = b / 2.0 + rt.get(torque) / 500.0;
    println!("gear angle {:.4} rad against a held shaft: half the backlash {:.3} plus the wind-up {:.4}", rt.get(angle), b / 2.0, rt.get(torque) / 500.0);
    assert!((rt.get(angle) - expected).abs() < 1e-3);
}

#[test]
fn battery_bridge_and_firmware_drive_a_motor() {
    let registry = registry();
    let mut m = ModelWorld::default();
    let bat = m.part(&registry, "bat", BATTERY, [("cells", 2.0), ("nominal_voltage", 7.4), ("internal_resistance", 0.05), ("capacity_ah", 1.0)]).unwrap();
    let gnd = m.part(&registry, "gnd", sim_domain_electrical::elements::GROUND, []).unwrap();
    let bridge = m.part(&registry, "bridge", H_BRIDGE, [("on_resistance", 0.1), ("current_limit", 10.0)]).unwrap();
    let motor = m.part(&registry, "motor", MOTOR_UNIT, motor_params(&[])).unwrap();
    let load = m.part(&registry, "load", sim_domain_rotational::elements::INERTIA, [("inertia", 1e-4), ("damping", 1e-3)]).unwrap();
    let amb = m.part(&registry, "amb", sim_domain_thermal::AMBIENT, [("temperature", 293.15)]).unwrap();
    let target = m.part(&registry, "target", sim_domain_control::elements::CONSTANT, [("value", 1.0)]).unwrap();
    let sensor = m.part(&registry, "sensor", sim_domain_rotational::elements::ANGLE_SENSOR, []).unwrap();
    let tacho = m.part(&registry, "tacho", sim_domain_rotational::elements::SPEED_SENSOR, []).unwrap();
    let fw = m.part(&registry, "fw", SERVO_FIRMWARE, [("rate", 100.0), ("latency", 0.02), ("kp", 4.0), ("deadband", 0.01), ("resolution", 0.002), ("limit", 1.0)]).unwrap();
    m.connect([bat.port("p"), bridge.port("supply_p")]);
    m.connect([bat.port("n"), bridge.port("supply_n"), gnd.port("pin"), motor.port("n"), bridge.port("n")]);
    m.connect([bridge.port("p"), motor.port("p")]);
    m.connect([motor.port("shaft"), load.port("shaft"), sensor.port("shaft"), tacho.port("shaft")]);
    m.connect([tacho.port("speed"), fw.port("rate")]);
    m.connect([motor.port("winding"), amb.port("node")]);
    m.connect([target.port("value"), fw.port("target")]);
    m.connect([sensor.port("angle"), fw.port("measured")]);
    m.connect([fw.port("command"), bridge.port("command")]);
    for s in ["current", "torque", "speed"] {
        m.connect([motor.port(s)]);
    }
    m.connect([bat.port("soc")]);
    let mut rt = sim_compile::Runtime::new(m, &registry, euler()).unwrap();
    let angle = rt.signal_id(sensor.port("angle"));
    let cmd = rt.state_id(fw.behavior, "command");
    rt.advance(0.015, 1e-3).unwrap();
    assert!(rt.get(cmd).abs() < 1e-9, "latency holds the first command back: {}", rt.get(cmd));
    rt.advance(2.0, 1e-3).unwrap();
    let a = rt.get(angle);
    println!("servo settled at {a:.4} rad for a 1 rad target (dead band 0.01); last command {:.3}", rt.get(cmd));
    assert!((a - 1.0).abs() < 0.02);
}
