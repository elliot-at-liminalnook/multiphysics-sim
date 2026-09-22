//! Build the motor-driver-board proving case through the shared system
//! commands, and save its subsystems to the library.
//!
//! The board: a 3-cell battery feeds a closed-loop 50 kHz buck regulator
//! (5 V logic rail with a 10 Ω load) and a MOSFET H-bridge (20 kHz,
//! sign-magnitude, synchronous) driving an HX-30HM servo motor with its
//! winding/case thermal network into an inertia and viscous load. The bridge's
//! four MOSFETs heat a shared heatsink. The bridge can be swapped for the
//! averaged `robot.h_bridge` element with the same ports.
//!
//! Provenance: HX-30HM constants are the provisional endpoint fit documented
//! in examples/actuators/hx30hm/README.md (from published ratings; not an
//! identified model). Board component values are illustrative design choices.
//!
//!     cargo run --release -p sim-runtime --example build_motor_driver_board
use sim_system::*;
use std::path::PathBuf;

fn element(component_type: &str, parameters: &[(&str, f64)], position: [f32; 3], label: &str) -> InstanceSpec {
    let mut i = InstanceSpec::element(component_type).at(position).labeled(label);
    for (k, v) in parameters {
        i = i.with(k, *v);
    }
    i
}
fn add(at: &str, name: &str, instance: InstanceSpec) -> Command {
    Command::AddInstance { at: at.into(), name: name.into(), instance }
}
fn net(at: &str, label: &str, terminals: &[&str]) -> Command {
    Command::Connect {
        at: at.into(),
        label: label.into(),
        terminals: terminals
            .iter()
            .map(|t| match t.split_once('.') {
                Some((i, p)) => Terminal::port(i, p),
                None => Terminal::boundary(t),
            })
            .collect(),
    }
}
fn port(at: &str, name: &str) -> Command {
    Command::AddBoundaryPort { at: at.into(), name: name.into(), port: BoundaryPort::default(), connect: None }
}
fn subsystem(at: &str, name: &str, definition: &str, label: &str, position: [f32; 3]) -> Vec<Command> {
    vec![
        Command::AddDefinitions { definitions: [(definition.to_string(), Definition::new(label))].into() },
        add(at, name, InstanceSpec::subsystem(definition).at(position).labeled(label)),
    ]
}

pub fn commands() -> Vec<Command> {
    let mut c = vec![
        Command::SetTitle { title: "Motor driver board".into() },
        Command::SetRunSettings {
            run: Some(RunSettings {
                integrator: IntegratorChoice::BackwardEuler,
                interval: 1e-5,
                absolute_tolerance: None,
                relative_tolerance: None,
                max_iterations: None,
                rationale: "Switching edges: backward Euler enforces algebraic constraints at step ends; the implicit midpoint rule leaves endpoint values (e.g. battery terminal voltage) alternating after each edge. 10 µs resolves 20/50 kHz PWM edges exactly as scheduled events.".into(),
            }),
        },
    ];
    // Top level: battery, ground, command profile, load.
    c.push(add("", "battery", element("robot.battery", &[("cells", 3.), ("internal_resistance", 0.05), ("capacity_ah", 2.2)], [-0.090, 0.0, 0.0], "3S battery")));
    c.push(add("", "gnd", element("electrical.ground", &[], [-0.050, 0.0, 0.034], "Ground")));
    c.push(add("", "command", element("control.pulse", &[("amplitude", 0.8), ("start", 0.005), ("duration", 1e6)], [0.018, 0.0, 0.034], "Drive command")));
    c.push(add("", "logic", element("electrical.resistor", &[("resistance", 10.)], [-0.012, 0.0, 0.042], "5 V logic load (MCU + sensors)")));
    c.push(add("", "load", element("rotational.inertia", &[("inertia", 2e-5), ("damping", 0.2)], [0.085, 0.0435, 0.0], "Output inertia + viscous load")));

    // Regulator: closed-loop synchronous-free buck (MOSFET + catch diode).
    c.extend(subsystem("", "regulator", "buck_5v", "5 V buck regulator", [-0.030, 0., 0.018]));
    let r = "regulator";
    c.push(Command::SetDefinitionInfo { id: "buck_5v".into(), label: None, description: Some("Voltage-mode buck: 50 kHz PWM, PI on the sensed output (reading 1.0 = 5 V), MOSFET high side, catch diode, 47 µH / 100 µF.".into()), interface: Some(Some("regulator.5v".into())) });
    for p in ["vin", "gnd", "vout"] {
        c.push(port(r, p));
    }
    c.push(add(r, "q", element("electrical.mosfet", &[("on_resistance", 0.03)], [-0.014, 0.0, -0.004], "High-side switch")));
    c.push(add(r, "d", element("electrical.diode", &[("forward_voltage", 0.45)], [-0.003, 0.0, 0.008], "Catch diode (Schottky)")));
    c.push(add(r, "l", element("electrical.inductor", &[("inductance", 47e-6)], [0.008, 0.0, -0.002], "47 µH")));
    c.push(add(r, "c", element("electrical.capacitor", &[("capacitance", 100e-6)], [0.024, 0.0, -0.002], "100 µF")));
    c.push(add(r, "sense", element("electrical.voltage_sense", &[("scale", 5.)], [0.024, 0.0, 0.010], "Output sense")));
    c.push(add(r, "pi", element("control.pi", &[("kp", 0.1), ("ki", 200.), ("setpoint", 1.)], [0.006, 0.0, 0.012], "Voltage loop")));
    c.push(add(r, "pwm", element("control.pwm", &[("frequency", 50e3)], [-0.012, 0.0, 0.012], "50 kHz PWM")));
    c.push(add(r, "sink", element("thermal.capacitance", &[("heat_capacity", 0.5), ("initial.temperature", 298.15)], [-0.014, 0.008, -0.010], "Switch copper")));
    c.push(add(r, "air", element("thermal.conductance", &[("conductance", 0.05)], [-0.014, 0.012, -0.012], "Copper to air")));
    c.push(add(r, "ambient", element("thermal.ambient", &[("temperature", 298.15)], [-0.014, 0.016, -0.014], "Ambient")));
    c.push(net(r, "VIN", &["vin", "q.drain"]));
    c.push(net(r, "SW", &["q.source", "d.n", "l.p"]));
    c.push(net(r, "VOUT", &["l.n", "c.p", "sense.p", "vout"]));
    c.push(net(r, "GND", &["gnd", "d.p", "c.n", "sense.n"]));
    c.push(net(r, "", &["sense.reading", "pi.measured"]));
    c.push(net(r, "", &["pi.command", "pwm.duty"]));
    c.push(net(r, "", &["pwm.gate", "q.gate"]));
    c.push(net(r, "", &["q.heat", "sink.node", "air.a"]));
    c.push(net(r, "", &["air.b", "ambient.node"]));

    // H-bridge: four MOSFETs, gate driver, shared heatsink.
    c.extend(subsystem("", "bridge", "h_bridge_mosfet", "MOSFET H-bridge", [0.018, 0., -0.008]));
    let b = "bridge";
    c.push(Command::SetDefinitionInfo { id: "h_bridge_mosfet".into(), label: None, description: Some("Four 20 mΩ MOSFETs with body diodes, 20 kHz sign-magnitude synchronous drive, shared heatsink (2 J/K, 0.5 W/K to air). Same ports as robot.h_bridge.".into()), interface: Some(Some("robot.h_bridge".into())) });
    for p in ["supply_p", "supply_n", "p", "n", "command"] {
        c.push(port(b, p));
    }
    for (name, x, label) in [("q_hi_a", -0.018, "High A"), ("q_lo_a", -0.006, "Low A"), ("q_hi_b", 0.006, "High B"), ("q_lo_b", 0.018, "Low B")] {
        c.push(add(b, name, element("electrical.mosfet", &[("on_resistance", 0.02)], [x, 0.0, 0.0], label)));
    }
    c.push(add(b, "driver", element("control.h_bridge_pwm", &[("frequency", 20e3)], [0.0, 0.0, 0.014], "Gate driver 20 kHz")));
    let mut heatsink = element("thermal.capacitance", &[("heat_capacity", 2.), ("initial.temperature", 298.15)], [0.0, 0.0, -0.016], "Heatsink");
    heatsink.appearance = Some(Appearance { shape: sim_inspect::spatial::SpatialShape::Box { size: [0.03, 0.015, 0.02] }, color_srgb: [0.74, 0.76, 0.79], model: Some("heatsink".into()) });
    c.push(add(b, "heatsink", heatsink));
    c.push(add(b, "air", element("thermal.conductance", &[("conductance", 0.5)], [0.019, 0.012, -0.016], "Heatsink to air")));
    c.push(add(b, "ambient", element("thermal.ambient", &[("temperature", 298.15)], [0.024, 0.016, -0.016], "Ambient")));
    c.push(net(b, "V+", &["supply_p", "q_hi_a.drain", "q_hi_b.drain"]));
    c.push(net(b, "V−", &["supply_n", "q_lo_a.source", "q_lo_b.source"]));
    c.push(net(b, "OUT A", &["p", "q_hi_a.source", "q_lo_a.drain"]));
    c.push(net(b, "OUT B", &["n", "q_hi_b.source", "q_lo_b.drain"]));
    c.push(net(b, "", &["command", "driver.command"]));
    for (gate, q) in [("gate_hi_a", "q_hi_a"), ("gate_lo_a", "q_lo_a"), ("gate_hi_b", "q_hi_b"), ("gate_lo_b", "q_lo_b")] {
        c.push(net(b, "", &[&format!("driver.{gate}"), &format!("{q}.gate")]));
    }
    c.push(net(b, "", &["q_hi_a.heat", "q_lo_a.heat", "q_hi_b.heat", "q_lo_b.heat", "heatsink.node", "air.a"]));
    c.push(net(b, "", &["air.b", "ambient.node"]));

    // Servo: HX-30HM motor with winding → case → air thermal path.
    c.extend(subsystem("", "servo", "hx30hm_servo", "HX-30HM servo", [0.075, 0., 0.]));
    let s = "servo";
    c.push(Command::SetDefinitionInfo { id: "hx30hm_servo".into(), label: None, description: Some("HX-30HM motor (provisional endpoint fit: R 3.7 Ω, internal ratio 200, Kt = Ke 0.009734, efficiency 0.504) with winding (2.40 J/K) → case (41.2 J/K) → air thermal path (12 K/W, 45 K/W). Not identified from hardware.".into()), interface: Some(Some("servo.hx30hm".into())) });
    for p in ["p", "n", "shaft"] {
        c.push(port(s, p));
    }
    c.push(add(s, "motor", element("robot.motor_unit", &[
        ("resistance", 3.7), ("torque_constant", 0.00973408), ("back_emf_constant", 0.00973408), ("inductance", 0.00148),
        ("no_load_current", 0.1), ("rotor_inertia", 2.2324546e-8), ("ratio", 200.), ("efficiency", 0.5037), ("gear_stiffness", 50.),
        ("temp_coeff", 0.0039), ("reference", 298.15), ("initial.winding.temperature", 298.15),
    ], [0.0, 0.0, 0.], "HX-30HM motor + gearbox")));
    c.push(add(s, "winding", element("thermal.capacitance", &[("heat_capacity", 2.4024)], [-0.006, 0.030, 0.014], "Winding heat capacity")));
    c.push(add(s, "winding_case", element("thermal.conductance", &[("resistance", 12.)], [0.0, 0.030, 0.014], "Winding → case")));
    c.push(add(s, "case", element("thermal.capacitance", &[("heat_capacity", 41.184), ("initial.temperature", 298.15)], [0.006, 0.030, 0.014], "Case heat capacity")));
    c.push(add(s, "case_air", element("thermal.conductance", &[("resistance", 45.)], [0.012, 0.030, 0.014], "Case → air")));
    c.push(add(s, "ambient", element("thermal.ambient", &[("temperature", 298.15)], [0.018, 0.030, 0.014], "Ambient")));
    c.push(net(s, "", &["p", "motor.p"]));
    c.push(net(s, "", &["n", "motor.n"]));
    c.push(net(s, "", &["shaft", "motor.shaft"]));
    c.push(net(s, "", &["motor.winding", "winding.node", "winding_case.a"]));
    c.push(net(s, "", &["winding_case.b", "case.node", "case_air.a"]));
    c.push(net(s, "", &["case_air.b", "ambient.node"]));

    // Board wiring.
    c.push(net("", "VBAT", &["battery.p", "regulator.vin", "bridge.supply_p"]));
    c.push(net("", "GND", &["battery.n", "gnd.pin", "regulator.gnd", "bridge.supply_n", "logic.n"]));
    c.push(net("", "5V", &["regulator.vout", "logic.p"]));
    c.push(net("", "MOTOR A", &["bridge.p", "servo.p"]));
    c.push(net("", "MOTOR B", &["bridge.n", "servo.n"]));
    c.push(net("", "", &["command.value", "bridge.command"]));
    c.push(net("", "", &["servo.shaft", "load.shaft"]));
    c
}

/// `robot.h_bridge` defines only the differential output; 10 MΩ bleeds to
/// the negative rail give the outputs a common-mode reference, as the
/// switching bridge's low-side switches do.
fn averaged_bridge() -> Definition {
    let mut d = Definition::new("Averaged H-bridge");
    d.description = "robot.h_bridge (on-resistance 2 × 20 mΩ, matching two conducting MOSFETs) with 10 MΩ common-mode bleeds. Real-time profile of h_bridge_mosfet; no switching ripple or heat.".into();
    d.interface = Some("robot.h_bridge".into());
    for p in ["supply_p", "supply_n", "p", "n", "command"] {
        d.ports.insert(p.into(), BoundaryPort::default());
    }
    d.instances.insert("bridge".into(), InstanceSpec::element("robot.h_bridge").with("on_resistance", 0.04).labeled("Averaged bridge"));
    d.instances.insert("bleed_p".into(), InstanceSpec::element("electrical.resistor").with("resistance", 1e7).at([0., 0., 0.01]).labeled("Common-mode bleed"));
    d.instances.insert("bleed_n".into(), InstanceSpec::element("electrical.resistor").with("resistance", 1e7).at([0., 0., -0.01]).labeled("Common-mode bleed"));
    let t = |i: &str, p: &str| Terminal::port(i, p);
    d.nets = vec![
        Net { label: "V+".into(), terminals: vec![Terminal::boundary("supply_p"), t("bridge", "supply_p")] },
        Net { label: "V−".into(), terminals: vec![Terminal::boundary("supply_n"), t("bridge", "supply_n"), t("bleed_p", "n"), t("bleed_n", "n")] },
        Net { label: "OUT A".into(), terminals: vec![Terminal::boundary("p"), t("bridge", "p"), t("bleed_p", "p")] },
        Net { label: "OUT B".into(), terminals: vec![Terminal::boundary("n"), t("bridge", "n"), t("bleed_n", "p")] },
        Net { label: String::new(), terminals: vec![Terminal::boundary("command"), t("bridge", "command")] },
    ];
    d
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let registry = sim_runtime::registry();
    let mut document = SystemDocument::new("Motor driver board");
    apply(&mut document, &registry, &commands())?;
    // Revision 1 records the construction as one reviewed batch.
    document.revision = 1;
    let directory = root.join("examples/systems-builder/motor-driver-board");
    std::fs::create_dir_all(&directory)?;
    let path = directory.join("board.system.json");
    store::write_atomic(&path, &serde_json::to_vec_pretty(&document)?)?;
    let library_dir = root.join("library/systems");
    for id in ["buck_5v", "h_bridge_mosfet", "hx30hm_servo"] {
        library::save(&document, id, &library_dir)?;
    }
    // The averaged alternative, with the same ports as the MOSFET bridge.
    let mut with_averaged = document.clone();
    apply(&mut with_averaged, &registry, &[Command::AddDefinitions { definitions: [("h_bridge_averaged".to_string(), averaged_bridge())].into() }])?;
    library::save(&with_averaged, "h_bridge_averaged", &library_dir)?;
    let check = sim_runtime::system_builder::check(&document, &registry)?;
    println!("{}", serde_json::to_string_pretty(&check)?);
    println!("wrote {}", path.display());
    Ok(())
}
