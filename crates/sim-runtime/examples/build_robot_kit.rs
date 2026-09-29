//! Builds the robot kit: named, realistic parts in the library (presets
//! built from annotated components, values marked as representative
//! estimates) and five starter robots that use them.
//!
//!     cargo run -p sim-runtime --example build_robot_kit
//!
//! Presets go to library/systems; robots to examples/robot-kit.
use sim_system::{library, BoundaryPort, Command, Definition, InstanceSpec, ParameterBinding, SystemDocument, Terminal};
use std::path::{Path, PathBuf};

const S: f32 = std::f32::consts::FRAC_1_SQRT_2;
const Y_TO_X: [f32; 4] = [0., 0., -S, S];
const Y_TO_Z: [f32; 4] = [S, 0., 0., S];
const ID: [f32; 4] = [0., 0., 0., 1.];

fn est(value: f64, why: &str) -> ParameterBinding {
    ParameterBinding::Value { value, unit: None, provenance: Some(sim_inspect::Provenance::Estimated { explanation: why.into() }), uncertainty: None }
}
thread_local! {
    static REGISTRY: std::cell::RefCell<Option<sim_core::BehaviorRegistry>> = const { std::cell::RefCell::new(None) };
}
/// An element starting from its notes' typical values (recorded as estimates),
/// with these values on top.
fn part(t: &str, label: &str, params: &[(&str, f64)], why: &str) -> InstanceSpec {
    let mut s = REGISTRY.with(|r| r.borrow().as_ref().map(|r| sim_system::snap::starter(r, &sim_system::InstanceKind::Element { component_type: t.into() }, label))).unwrap_or_else(|| InstanceSpec::element(t).labeled(label));
    for (k, v) in params {
        s.parameters.insert(k.to_string(), est(*v, why));
    }
    s
}
fn placed(mut s: InstanceSpec, position: [f32; 3], rotation: [f32; 4]) -> InstanceSpec {
    s.placement.position = position;
    s.placement.rotation_xyzw = rotation;
    s
}
fn add(at: &str, name: &str, s: InstanceSpec) -> Command {
    Command::AddInstance { at: at.into(), name: name.into(), instance: s }
}
fn join(at: &str, terminals: &[(&str, &str)]) -> Command {
    Command::Connect { at: at.into(), terminals: terminals.iter().map(|(i, p)| Terminal::port(i, p)).collect(), label: String::new() }
}
fn port(at: &str, name: &str, label: &str, inner: (&str, &str)) -> Command {
    Command::AddBoundaryPort { at: at.into(), name: name.into(), port: BoundaryPort { label: label.into(), schema: None }, connect: Some(Terminal::port(inner.0, inner.1)) }
}

struct Kit<'a> {
    registry: &'a sim_core::BehaviorRegistry,
    scratch: SystemDocument,
    ids: Vec<String>,
}

impl Kit<'_> {
    fn apply(&mut self, commands: Vec<Command>) {
        if let Err(e) = sim_system::apply(&mut self.scratch, self.registry, &commands) {
            panic!("{e}");
        }
    }
    /// A new preset definition, placed once in the scratch document at `at`.
    fn define(&mut self, id: &str, label: &str, description: &str, interface: &str) -> String {
        let mut d = Definition::new(label);
        d.description = description.into();
        d.interface = Some(interface.into());
        let at = format!("p_{}", self.ids.len());
        self.apply(vec![Command::AddDefinitions { definitions: [(id.to_string(), d)].into() }, add("", &at, InstanceSpec::subsystem(id))]);
        self.ids.push(id.into());
        at
    }
    /// Brushed gearmotor: motor, rotor, lossy gear, output friction, mount.
    #[allow(clippy::too_many_arguments)]
    fn gearmotor(&mut self, id: &str, label: &str, description: &str, r: f64, k: f64, rotor: f64, ratio: f64, eta: f64, friction: f64, why: &str) {
        let g = self.define(id, label, description, "gearmotor.dc");
        self.apply(vec![
            add(&g, "motor", placed(part("bridge.brushed_motor", "Motor", &[("resistance", r), ("torque_constant", k), ("back_emf_constant", k), ("inductance", 0.0)], why), [-0.02, 0., 0.], Y_TO_X)),
            add(&g, "rotor", placed(part("rotational.inertia", "Rotor", &[("inertia", rotor)], why), [0., 0., 0.], Y_TO_X)),
            add(&g, "gear", placed(part("rotational.lossy_gear", "Gearbox", &[("ratio", ratio), ("efficiency", eta)], why), [0.015, 0., 0.], ID)),
            add(&g, "friction", placed(part("rotational.coulomb_friction", "Gear friction", &[("torque", friction)], why), [0.025, -0.01, 0.], ID)),
            add(&g, "mount", placed(InstanceSpec::element("rotational.ground").labeled("Mount"), [-0.02, -0.015, 0.], ID)),
            join(&g, &[("motor", "case"), ("mount", "flange")]),
            join(&g, &[("motor", "shaft"), ("rotor", "shaft"), ("gear", "input")]),
            join(&g, &[("gear", "output"), ("friction", "shaft")]),
            port(&g, "p", "Supply +", ("motor", "p")),
            port(&g, "n", "Supply −", ("motor", "n")),
            port(&g, "output", "Output shaft", ("gear", "output")),
        ]);
    }
    fn presets(&mut self) {
        let n20 = "Representative N20 6 V 100:1 gearmotor (300 rpm no-load, ≈ 0.2 N·m stall); catalogue-style values, not a measured unit.";
        self.gearmotor("n20_gearmotor_100", "N20 gearmotor 6 V 100:1", "Tiny metal-gear motor for small rovers and grippers: 300 rpm, ≈ 0.2 N·m stall at 6 V.", 3.75, 0.0019, 5e-9, 100.0, 0.6, 0.003, n20);
        let m37 = "Representative 37 mm 12 V 30:1 gearmotor (333 rpm no-load, ≈ 1.5 N·m stall); catalogue-style values.";
        self.gearmotor("gearmotor_37mm_30", "37 mm gearmotor 12 V 30:1", "A workhorse for rovers and arms: 333 rpm, ≈ 1.5 N·m stall at 12 V, 80 % gearbox.", 2.2, 0.0115, 2e-6, 30.0, 0.8, 0.02, m37);

        // NEMA 17 stepper with its driver.
        let why = "Representative NEMA 17 (42 × 42 × 40 mm): 0.45 N·m holding, 54 g·cm² rotor.";
        let g = self.define("nema17_stepper", "NEMA 17 stepper + driver", "The 3D-printer stepper: 200 steps/rev, 0.45 N·m holding torque, with a microstepping current driver. Give it a target angle.", "stepper");
        self.apply(vec![
            add(&g, "stepper", placed(part("part.stepper_motor", "Stepper", &[("holding_torque", 0.45), ("detent_torque", 0.02), ("damping", 0.002)], why), [0., 0., 0.], Y_TO_X)),
            add(&g, "rotor", placed(part("rotational.inertia", "Rotor", &[("inertia", 5.4e-6)], why), [0.02, 0., 0.], Y_TO_X)),
            add(&g, "frame", placed(InstanceSpec::element("rotational.ground").labeled("Frame"), [0., -0.03, 0.], ID)),
            join(&g, &[("stepper", "case"), ("frame", "flange")]),
            join(&g, &[("stepper", "shaft"), ("rotor", "shaft")]),
            port(&g, "target", "Target angle", ("stepper", "target")),
            port(&g, "shaft", "Shaft", ("stepper", "shaft")),
        ]);

        // Drone power unit: 2212 920 KV motor + 10" propeller, winding cooled to air.
        let why = "Representative 2212 920 KV outrunner with a 10 × 4.5 in propeller; catalogue-style values.";
        let g = self.define("drone_unit_2212", "Drone motor 2212 920 KV + 10″ prop", "One drone arm: a brushless outrunner and its ESC input, a 10-inch propeller, and the winding's heat path to the air. About 400 g of thrust per unit at 5000 rpm.", "propulsion");
        self.apply(vec![
            add(&g, "motor", placed(part("part.bldc_motor", "2212 motor", &[("kv", 96.3), ("resistance", 0.12), ("no_load_current", 0.5)], why), [0., 0., 0.], ID)),
            add(&g, "rotor", placed(part("rotational.inertia", "Rotor and prop", &[("inertia", 4e-5)], why), [0., 0.02, 0.], ID)),
            add(&g, "prop", placed(part("part.propeller", "10″ propeller", &[("diameter", 0.254), ("thrust_coefficient", 0.11), ("torque_coefficient", 0.006)], why), [0., 0.03, 0.], ID)),
            add(&g, "mount", placed(InstanceSpec::element("rotational.ground").labeled("Arm mount"), [0., -0.02, 0.], ID)),
            add(&g, "winding", placed(part("thermal.capacitance", "Winding", &[("heat_capacity", 15.0)], why).with("initial.temperature", 293.15), [0.02, 0., 0.], ID)),
            add(&g, "cooling", placed(part("thermal.conductance", "Prop wash cooling", &[("conductance", 0.4)], why), [0.03, 0., 0.], ID)),
            add(&g, "air", placed(InstanceSpec::element("thermal.ambient").labeled("Air").with("temperature", 293.15), [0.04, 0., 0.], ID)),
            join(&g, &[("motor", "case"), ("mount", "flange")]),
            join(&g, &[("motor", "shaft"), ("rotor", "shaft"), ("prop", "shaft")]),
            join(&g, &[("motor", "heat"), ("winding", "node"), ("cooling", "a")]),
            join(&g, &[("cooling", "b"), ("air", "node")]),
            port(&g, "p", "ESC +", ("motor", "p")),
            port(&g, "n", "ESC −", ("motor", "n")),
            port(&g, "airframe", "Airframe", ("prop", "airframe")),
        ]);

        // Harmonic drive: high ratio, near-zero backlash, a springy flexspline.
        let why = "Representative size-14 harmonic drive, 100:1: ≈ 75 % efficiency, flexspline stiffness order 5 kN·m/rad.";
        let g = self.define("harmonic_drive_100", "Harmonic drive 100:1", "Strain-wave gear for robot joints: 100:1 in one flat stage with almost no backlash, but a springy flexspline and ≈ 75 % efficiency.", "gearbox.rotational");
        self.apply(vec![
            add(&g, "wave", placed(part("rotational.lossy_gear", "Wave generator → flexspline", &[("ratio", 100.0), ("efficiency", 0.75)], why), [0., 0., 0.], ID)),
            add(&g, "flex", placed(part("rotational.spring", "Flexspline compliance", &[("stiffness", 5000.0)], why), [0.02, 0., 0.], ID)),
            add(&g, "hub", placed(part("rotational.inertia", "Output hub", &[("inertia", 1e-4)], why), [0.03, 0., 0.], Y_TO_X)),
            join(&g, &[("wave", "output"), ("flex", "a")]),
            join(&g, &[("flex", "b"), ("hub", "shaft")]),
            port(&g, "input", "Wave generator", ("wave", "input")),
            port(&g, "output", "Output", ("hub", "shaft")),
        ]);

        // Screws: a ball screw and a fast (back-drivable) T8×8 lead screw.
        for (id, label, desc, lead, d, phi, mu) in [
            ("ball_screw_1204", "Ball screw 1204", "12 mm ball screw, 4 mm lead: ≈ 90 % efficient, precise, back-drives (needs a brake on vertical axes).", 0.004, 0.012, 0.785, 0.005),
            ("lead_screw_t8x8", "Lead screw T8×8", "The 3D-printer Z screw: 8 mm, 8 mm lead (4 starts). Fast per turn, ≈ 60 % efficient, and it back-drives.", 0.008, 0.0072, 0.26, 0.15),
        ] {
            let g = self.define(id, label, desc, "screw");
            let why = format!("Representative {label}; catalogue-style values.");
            self.apply(vec![
                add(&g, "screw", placed(part("bridge.lead_screw", label, &[("lead", lead), ("pitch_diameter", d), ("pressure_angle", phi), ("friction", mu)], &why), [0., 0., 0.], Y_TO_X)),
                port(&g, "screw", "Screw shaft", ("screw", "screw")),
                port(&g, "nut", "Nut", ("screw", "nut")),
            ]);
        }

        // Battery.
        let g = self.define("lipo_3s_2200", "LiPo 3S 2200 mAh", "A common robot and drone pack: 11.1 V nominal (12.6 V full), 2.2 Ah, ≈ 30 mΩ.", "battery");
        self.apply(vec![
            add(&g, "pack", placed(part("robot.battery", "3S pack", &[("cells", 3.0), ("capacity_ah", 2.2), ("internal_resistance", 0.03)], "Representative 3S 2200 mAh 30C pack."), [0., 0., 0.], ID)),
            port(&g, "p", "+", ("pack", "p")),
            port(&g, "n", "−", ("pack", "n")),
        ]);

        // Hobby servo: motor unit + position loop + potentiometer, as sold.
        let why = "Representative MG996R-class servo at 6 V: ≈ 1.1 N·m stall, 0.17 s/60°, 50 Hz position loop.";
        let g = self.define("hobby_servo_mg996r", "Hobby servo (MG996R-class)", "A standard RC servo: DC motor, 300:1 plastic-and-metal gears, a potentiometer and a 50 Hz position loop inside one case. Give it power and a target angle.", "servo");
        self.apply(vec![
            add(&g, "motor", placed(part("robot.motor_unit", "Motor and gears", &[("resistance", 2.6), ("torque_constant", 0.0032), ("ratio", 300.0), ("efficiency", 0.5), ("rotor_inertia", 1e-7), ("gear_friction", 0.02)], why), [0., 0., 0.], ID)),
            add(&g, "driver", placed(part("robot.h_bridge", "Driver", &[("on_resistance", 0.2)], why), [-0.02, 0., 0.], ID)),
            add(&g, "loop", placed(part("robot.servo_firmware", "Position loop", &[("rate", 50.0), ("kp", 8.0), ("kd", 0.2), ("deadband", 0.005)], why), [-0.02, 0.01, 0.], ID)),
            add(&g, "pot", placed(part("sensor.encoder", "Potentiometer", &[], why), [0.01, 0.01, 0.], ID)),
            add(&g, "tach", placed(part("sensor.tachometer", "Speed estimate", &[], why), [0.01, 0.015, 0.], ID)),
            add(&g, "case_heat", placed(part("thermal.capacitance", "Case", &[("heat_capacity", 20.0)], why).with("initial.temperature", 293.15), [0.02, -0.01, 0.], ID)),
            add(&g, "to_air", placed(part("thermal.conductance", "Case to air", &[("conductance", 0.1)], why), [0.03, -0.01, 0.], ID)),
            add(&g, "air", placed(InstanceSpec::element("thermal.ambient").labeled("Air").with("temperature", 293.15), [0.04, -0.01, 0.], ID)),
            join(&g, &[("driver", "p"), ("motor", "p")]),
            join(&g, &[("driver", "n"), ("motor", "n")]),
            join(&g, &[("motor", "shaft"), ("pot", "shaft"), ("tach", "shaft")]),
            join(&g, &[("pot", "angle"), ("loop", "measured")]),
            join(&g, &[("tach", "speed"), ("loop", "rate")]),
            join(&g, &[("loop", "command"), ("driver", "command")]),
            join(&g, &[("motor", "winding"), ("case_heat", "node"), ("to_air", "a")]),
            join(&g, &[("to_air", "b"), ("air", "node")]),
            port(&g, "p", "Power +", ("driver", "supply_p")),
            port(&g, "n", "Power −", ("driver", "supply_n")),
            port(&g, "target", "Target angle", ("loop", "target")),
            port(&g, "horn", "Output horn", ("motor", "shaft")),
        ]);
    }
}

fn run_settings(interval: f64, why: &str) -> sim_system::RunSettings {
    sim_system::RunSettings { integrator: sim_system::IntegratorChoice::BackwardEuler, interval, absolute_tolerance: None, relative_tolerance: None, max_iterations: None, rationale: why.into() }
}

fn system(title: &str, registry: &sim_core::BehaviorRegistry, lib: &Path, uses: &[&str], commands: Vec<Command>, run: sim_system::RunSettings) -> SystemDocument {
    let mut doc = SystemDocument::new(title);
    for u in uses {
        let defs = library::import(&lib.join(library::file_name(u))).unwrap();
        sim_system::apply(&mut doc, registry, &[Command::AddDefinitions { definitions: defs }]).unwrap_or_else(|e| panic!("{u}: {e}"));
    }
    sim_system::apply(&mut doc, registry, &commands).unwrap_or_else(|e| panic!("{title}: {e}"));
    doc.run = Some(run);
    doc.revision = 1;
    doc
}

fn metric(label: &str, observable: &str, reduce: sim_system::Reduce, window: [f64; 2]) -> sim_system::Metric {
    sim_system::Metric { label: label.into(), observable: observable.into(), reduce, window: Some(window) }
}
fn sweep(instance: &str, parameter: &str, values: &[f64], duration: f64, observe: &[&str], metrics: Vec<sim_system::Metric>) -> sim_system::Study {
    sim_system::Study { at: String::new(), instance: instance.into(), kind: sim_system::StudyKind::Sweep { parameter: parameter.into(), values: values.to_vec() }, duration, observe: observe.iter().map(|s| s.to_string()).collect(), metrics }
}

/// A saved study per robot: the trade-off it is best at showing.
fn studies(name: &str, doc: &mut SystemDocument, registry: &sim_core::BehaviorRegistry, lib: &Path) {
    use sim_system::Reduce::*;
    let study = match name {
        "rover" => ("hills", sweep("slope", "force", &[0.0, -3.4, -6.7, -9.8], 3.0, &["chassis.axis.velocity", "left_motor/motor.p.current"], vec![metric("cruise speed", "chassis.axis.velocity", Mean, [2.5, 3.0]), metric("motor current", "left_motor/motor.p.current", Mean, [2.5, 3.0])])),
        "drone" => ("prop_size", sweep("arm1", "prop/diameter", &[0.2, 0.23, 0.254, 0.28], 5.0, &["airframe.axis.position", "battery/pack.p.current", "arm1/motor.p.current"], vec![metric("altitude", "airframe.axis.position", Mean, [4.5, 5.0]), metric("battery current at hover (all four)", "battery/pack.p.current", Mean, [4.5, 5.0]), metric("motor current (one arm)", "arm1/motor.p.current", Mean, [4.5, 5.0])])),
        "sea_arm" => ("spring", sweep("spring", "stiffness", &[5.0, 20.0, 100.0, 1000.0], 3.0, &["link.shaft.angle"], vec![metric("peak angle", "link.shaft.angle", Max, [0.0, 3.0]), metric("final angle", "link.shaft.angle", Final, [0.0, 3.0])])),
        "belt_axis" => ("move_speed", sweep("profile", "rate", &[31.4, 100.0, 200.0, 300.0], 1.5, &["carriage.axis.position"], vec![metric("final position (0.2 m if no steps lost)", "carriage.axis.position", Final, [0.0, 1.5])])),
        "gripper" => {
            for lib_def in ["lead_screw_t8x8", "ball_screw_1204"] {
                let defs = library::import(&lib.join(library::file_name(lib_def))).unwrap();
                sim_system::apply(doc, registry, &[Command::AddDefinitions { definitions: defs }]).unwrap();
            }
            ("screws", sim_system::Study {
                at: String::new(),
                instance: "screw".into(),
                kind: sim_system::StudyKind::Compare { alternatives: vec![sim_system::InstanceKind::Subsystem { definition: "lead_screw_t8x8".into() }, sim_system::InstanceKind::Subsystem { definition: "ball_screw_1204".into() }] },
                duration: 3.0,
                observe: vec!["load_cell.force".into(), "motor/motor.p.current".into()],
                metrics: vec![metric("grip force while powered", "load_cell.force", Mean, [1.3, 1.5]), metric("grip force after power-off", "load_cell.force", Mean, [2.8, 3.0])],
            })
        }
        _ => return,
    };
    doc.studies.insert(study.0.into(), study.1);
}

fn robots(registry: &sim_core::BehaviorRegistry, root: &Path) -> Vec<(&'static str, SystemDocument)> {
    let lib = root.join("library/systems");
    let e = "Representative value for the starter robot; not a measured machine.";
    let mut out = Vec::new();

    // 1. Rover: straight-line drive of a 2 kg two-wheel rover.
    let mut c = vec![
        add("", "battery", placed(InstanceSpec::subsystem("lipo_3s_2200").labeled("3S LiPo"), [0., 0.03, 0.], ID)),
        add("", "gnd", placed(InstanceSpec::element("electrical.ground").labeled("0 V"), [0., 0.01, 0.], ID)),
        join("", &[("battery", "n"), ("gnd", "pin")]),
        add("", "throttle", placed(part("control.pulse", "Throttle: 80 % for 3 s", &[("amplitude", 0.8), ("start", 0.2), ("duration", 3.0)], e), [-0.05, 0.05, 0.], ID)),
        add("", "chassis", placed(part("translational.mass", "Chassis 2 kg", &[("mass", 2.0)], e), [0., 0.06, 0.], ID)),
        add("", "slope", placed(part("translational.load_force", "Slope (0°: try −3.4 N for 10°)", &[("force", 0.0)], e), [0.05, 0.06, 0.], ID)),
        add("", "speed", placed(InstanceSpec::element("sensor.linear_velocity").labeled("Speedometer"), [0.05, 0.08, 0.], ID)),
    ];
    for (side, x) in [("left", -0.08f32), ("right", 0.08)] {
        let (drv, gm, whl, hub) = (format!("{side}_driver"), format!("{side}_motor"), format!("{side}_wheel"), format!("{side}_hub"));
        c.extend([
            add("", &drv, placed(part("robot.h_bridge", &format!("{side} driver"), &[("on_resistance", 0.05)], e), [x, 0.03, -0.04], ID)),
            add("", &gm, placed(InstanceSpec::subsystem("gearmotor_37mm_30").labeled(&format!("{side} gearmotor")), [x, 0.03, 0.], Y_TO_X)),
            add("", &hub, placed(part("rotational.inertia", "Wheel inertia", &[("inertia", 2e-4)], e), [x * 1.3, 0.04, 0.], Y_TO_X)),
            add("", &whl, placed(part("part.drive_wheel", &format!("{side} wheel"), &[("radius", 0.04), ("normal_load", 8.0), ("grip", 0.8)], e), [x * 1.3, 0.04, 0.], Y_TO_X)),
            join("", &[("battery", "p"), (&drv, "supply_p")]),
            join("", &[("battery", "n"), (&drv, "supply_n"), (&gm, "n"), (&drv, "n")]),
            join("", &[(&drv, "p"), (&gm, "p")]),
            join("", &[("throttle", "value"), (&drv, "command")]),
            join("", &[(&gm, "output"), (&hub, "shaft"), (&whl, "axle")]),
            join("", &[(&whl, "chassis"), ("chassis", "axis"), ("slope", "axis"), ("speed", "axis")]),
        ]);
    }
    // The two drivers share ground through the battery's negative terminal.
    out.push(("rover", system("Two-wheel rover", registry, &lib, &["lipo_3s_2200", "gearmotor_37mm_30"], c, run_settings(1e-3, "Averaged drivers; friction smoothing in the tyres and gears is stiff: backward Euler, 1 ms."))));

    // 2. Drone altitude hold: four power units, PID on altitude with hover feed-forward.
    let mut c = vec![
        add("", "battery", placed(InstanceSpec::subsystem("lipo_3s_2200").labeled("3S LiPo"), [0., 0.1, 0.], ID)),
        add("", "gnd", placed(InstanceSpec::element("electrical.ground").labeled("0 V"), [0., 0.09, 0.], ID)),
        join("", &[("battery", "n"), ("gnd", "pin")]),
        add("", "airframe", placed(part("translational.mass", "Airframe 1.2 kg", &[("mass", 1.2)], e), [0., 0.12, 0.], ID)),
        add("", "weight", placed(part("translational.load_force", "Weight (1.2 kg × g)", &[("force", -1.2 * 9.80665)], e), [0.03, 0.12, 0.], ID)),
        add("", "ground", placed(part("part.hard_stop_linear", "Ground", &[("lower", 0.0), ("upper", 100.0)], e), [0., 0.0, 0.], ID)),
        add("", "altimeter", placed(part("sensor.linear_encoder", "Altimeter", &[], e), [0.03, 0.14, 0.], ID)),
        add("", "setpoint", placed(part("part.position_setpoint", "Climb to 1 m", &[("rate", 0.5), ("ramp_from", 0.5), ("ramp_for", 2.0)], e), [-0.05, 0.16, 0.], ID)),
        add("", "autopilot", placed(part("part.pid_position", "Altitude PID", &[("kp", 0.4), ("ki", 0.2), ("kd", 0.25), ("filter", 0.02), ("offset", 0.48), ("limit", 1.0)], e), [-0.03, 0.16, 0.], ID)),
        join("", &[("airframe", "axis"), ("weight", "axis"), ("ground", "axis"), ("altimeter", "axis")]),
        join("", &[("setpoint", "position"), ("autopilot", "target")]),
        join("", &[("altimeter", "position"), ("autopilot", "measured")]),
    ];
    for (k, (x, z)) in [(0.2f32, 0.2f32), (-0.2, 0.2), (0.2, -0.2), (-0.2, -0.2)].iter().enumerate() {
        let (esc, unit) = (format!("esc{}", k + 1), format!("arm{}", k + 1));
        c.extend([
            add("", &esc, placed(part("robot.h_bridge", &format!("ESC {}", k + 1), &[("on_resistance", 0.01)], e), [*x * 0.6, 0.11, *z * 0.6], ID)),
            add("", &unit, placed(InstanceSpec::subsystem("drone_unit_2212").labeled(&format!("Arm {}", k + 1)), [*x, 0.13, *z], ID)),
            join("", &[("battery", "p"), (&esc, "supply_p")]),
            join("", &[("battery", "n"), (&esc, "supply_n"), (&unit, "n"), (&esc, "n")]),
            join("", &[(&esc, "p"), (&unit, "p")]),
            join("", &[("autopilot", "command"), (&esc, "command")]),
            join("", &[(&unit, "airframe"), ("airframe", "axis")]),
        ]);
    }
    out.push(("drone", system("Quadcopter altitude hold", registry, &lib, &["lipo_3s_2200", "drone_unit_2212"], c, run_settings(1e-3, "Stiff ground contact and fast motor currents: backward Euler, 1 ms."))));

    // 3. Series-elastic arm joint lifting a link to a ramped target.
    let c = vec![
        add("", "supply", placed(part("electrical.voltage_source", "12 V bench supply", &[("voltage", 12.0)], e), [-0.1, 0.05, 0.], ID)),
        add("", "gnd", placed(InstanceSpec::element("electrical.ground"), [-0.1, 0.03, 0.], ID)),
        add("", "driver", placed(part("robot.h_bridge", "Driver", &[("on_resistance", 0.05)], e), [-0.07, 0.05, 0.], ID)),
        add("", "gearmotor", placed(InstanceSpec::subsystem("gearmotor_37mm_30").labeled("Joint gearmotor"), [-0.03, 0.05, 0.], Y_TO_X)),
        add("", "gear_out", placed(part("rotational.inertia", "Gear output", &[("inertia", 1e-4)], e), [0., 0.05, 0.], Y_TO_X)),
        add("", "spring", placed(part("rotational.spring", "Series spring (SEA)", &[("stiffness", 20.0)], e), [0.02, 0.05, 0.], Y_TO_X)),
        add("", "link", placed(part("rotational.inertia", "Link 0.5 kg × 0.25 m", &[("inertia", 0.5 * 0.25 * 0.25 / 3.0)], e), [0.05, 0.05, 0.], Y_TO_Z)),
        add("", "gravity", placed(part("part.pendulum_gravity", "Gravity on the link", &[("mass", 0.5), ("arm", 0.125)], e), [0.05, 0.02, 0.], ID)),
        add("", "stops", placed(part("part.hard_stop_rotary", "Joint limits", &[("lower", -0.5), ("upper", 2.5)], e), [0.07, 0.05, 0.], ID)),
        add("", "encoder", placed(part("sensor.encoder", "Joint encoder", &[], e), [0.07, 0.07, 0.], ID)),
        add("", "target", placed(part("part.angle_setpoint", "Raise to 1.2 rad", &[("rate", 0.8), ("ramp_from", 0.2), ("ramp_for", 1.5)], e), [-0.07, 0.09, 0.], ID)),
        add("", "pid", placed(part("part.pid_angle", "Joint PID", &[("kp", 3.0), ("ki", 6.0), ("kd", 0.15)], e), [-0.05, 0.09, 0.], ID)),
        join("", &[("supply", "p"), ("driver", "supply_p")]),
        join("", &[("supply", "n"), ("gnd", "pin"), ("driver", "supply_n"), ("driver", "n"), ("gearmotor", "n")]),
        join("", &[("driver", "p"), ("gearmotor", "p")]),
        join("", &[("gearmotor", "output"), ("gear_out", "shaft"), ("spring", "a")]),
        join("", &[("spring", "b"), ("link", "shaft"), ("gravity", "shaft"), ("stops", "shaft"), ("encoder", "shaft")]),
        join("", &[("target", "angle"), ("pid", "target")]),
        join("", &[("encoder", "angle"), ("pid", "measured")]),
        join("", &[("pid", "command"), ("driver", "command")]),
    ];
    out.push(("sea_arm", system("Series-elastic arm joint", registry, &lib, &["gearmotor_37mm_30"], c, run_settings(5e-4, "SEA spring against link inertia rings at ≈ 70 rad/s; 0.5 ms resolves it."))));

    // 4. Belt-driven stepper axis with a limit switch and end stops.
    let c = vec![
        add("", "stepper", placed(InstanceSpec::subsystem("nema17_stepper").labeled("NEMA 17"), [0., 0.03, 0.], ID)),
        add("", "profile", placed(part("part.angle_setpoint", "Move 200 mm in 1 s", &[("rate", 0.2 / 0.00637), ("ramp_from", 0.2), ("ramp_for", 1.0)], e), [-0.04, 0.05, 0.], ID)),
        add("", "belt", placed(part("part.timing_belt", "GT2 belt", &[], e), [0.03, 0.03, 0.], ID)),
        add("", "carriage", placed(part("translational.mass", "Carriage 0.5 kg", &[("mass", 0.5)], e), [0.08, 0.03, 0.], ID)),
        add("", "rail", placed(part("translational.damper", "Linear rail drag", &[("damping", 2.0)], e), [0.08, 0.01, 0.], ID)),
        add("", "rail_ground", placed(InstanceSpec::element("translational.ground").labeled("Frame"), [0.1, 0.0, 0.], ID)),
        add("", "stops", placed(part("part.hard_stop_linear", "End stops", &[("lower", -0.005), ("upper", 0.3)], e), [0.12, 0.03, 0.], ID)),
        add("", "home", placed(part("part.limit_switch", "Far-end switch", &[("position", 0.19)], e), [0.12, 0.05, 0.], ID)),
        add("", "scale", placed(part("sensor.linear_encoder", "Carriage position", &[], e), [0.1, 0.05, 0.], ID)),
        join("", &[("profile", "angle"), ("stepper", "target")]),
        join("", &[("stepper", "shaft"), ("belt", "pulley")]),
        join("", &[("belt", "carriage"), ("carriage", "axis"), ("rail", "a"), ("stops", "axis"), ("home", "axis"), ("scale", "axis")]),
        join("", &[("rail", "b"), ("rail_ground", "axis")]),
    ];
    out.push(("belt_axis", system("Stepper belt axis", registry, &lib, &["nema17_stepper"], c, run_settings(2e-4, "Stepper torque ripple (50 cycles/rev) and belt resonance need ≤ 0.2 ms."))));

    // 5. Lead-screw gripper squeezing a soft object; the power is cut at 1.5 s.
    let c = vec![
        add("", "supply", placed(part("electrical.switched_voltage_source", "6 V, off at 1.5 s", &[("voltage", 6.0), ("off_at", 1.5)], e), [-0.06, 0.03, 0.], ID)),
        add("", "gnd", placed(InstanceSpec::element("electrical.ground"), [-0.06, 0.01, 0.], ID)),
        add("", "motor", placed(InstanceSpec::subsystem("n20_gearmotor_100").labeled("N20 gearmotor"), [-0.03, 0.03, 0.], Y_TO_X)),
        add("", "screw_inertia", placed(part("rotational.inertia", "Screw", &[("inertia", 1e-6)], e), [0., 0.03, 0.], Y_TO_X)),
        add("", "screw", placed(part("bridge.lead_screw", "Tr8×2 screw (self-locking)", &[], e), [0.02, 0.03, 0.], Y_TO_X)),
        add("", "finger", placed(part("translational.mass", "Finger 50 g", &[("mass", 0.05)], e), [0.05, 0.03, 0.], ID)),
        add("", "load_cell", placed(part("sensor.force", "Load cell", &[], e), [0.06, 0.03, 0.], ID)),
        add("", "pad", placed(part("translational.mass", "Pad 5 g", &[("mass", 0.005)], e), [0.07, 0.03, 0.], ID)),
        add("", "object", placed(part("part.hard_stop_linear", "Soft object at 5 mm", &[("lower", -1.0), ("upper", 0.005), ("stiffness", 5000.0), ("damping", 50000.0)], e), [0.08, 0.03, 0.], ID)),
        join("", &[("supply", "p"), ("motor", "p")]),
        join("", &[("supply", "n"), ("gnd", "pin"), ("motor", "n")]),
        join("", &[("motor", "output"), ("screw_inertia", "shaft"), ("screw", "screw")]),
        join("", &[("screw", "nut"), ("finger", "axis"), ("load_cell", "a")]),
        join("", &[("load_cell", "b"), ("pad", "axis"), ("object", "axis")]),
    ];
    out.push(("gripper", system("Lead-screw gripper", registry, &lib, &["n20_gearmotor_100"], c, run_settings(5e-4, "Stiff contact with the object and a self-locking screw: backward Euler, 0.5 ms."))));
    out
}

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
    let (registry, loaded) = sim_runtime::registry_with_parts(&root.join("library/parts"));
    assert!(loaded.iter().all(|l| l.error.is_none()), "{loaded:?}");
    REGISTRY.with(|r| *r.borrow_mut() = Some(registry.clone()));
    let mut kit = Kit { registry: &registry, scratch: SystemDocument::new("kit"), ids: Vec::new() };
    kit.presets();
    let lib = root.join("library/systems");
    for id in kit.ids.clone() {
        for p in library::publish(&kit.scratch, &id, &lib).unwrap() {
            if p.changed {
                println!("published {} v{}", p.id, p.version);
            }
        }
    }
    let out = root.join("examples/robot-kit");
    std::fs::create_dir_all(&out).unwrap();
    for (name, mut doc) in robots(&registry, &root) {
        studies(name, &mut doc, &registry, &lib);
        for d in doc.definitions.values_mut() {
            if let Some(s) = &mut d.source {
                s.path = Path::new(&s.path).strip_prefix(&root).map(|p| p.display().to_string()).unwrap_or(s.path.clone());
            }
        }
        let path = out.join(format!("{name}.system.json"));
        std::fs::write(&path, serde_json::to_vec_pretty(&doc).unwrap()).unwrap();
        println!("wrote {}", path.display());
    }
}
