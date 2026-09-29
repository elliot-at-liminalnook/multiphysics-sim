//! Builds the M4 proving case through the shared commands and library API:
//! a DC motor subsystem, gearmotors that contain it, a joint actuator that
//! contains a gearmotor, and two systems that use them (a winch and a leg).
//!
//!     cargo run -p sim-runtime --example build_parts_from_parts
//!
//! Library definitions go to library/systems (published with versions);
//! the systems go to examples/systems-builder/parts-from-parts.
use sim_system::{library, Command, InstanceKind, InstanceSpec, ParameterBinding, SystemDocument, Terminal};
use std::path::{Path, PathBuf};

fn add(at: &str, name: &str, spec: InstanceSpec) -> Command {
    Command::AddInstance { at: at.into(), name: name.into(), instance: spec }
}
fn join(at: &str, terminals: &[(&str, &str)]) -> Command {
    Command::Connect { at: at.into(), terminals: terminals.iter().map(|(i, p)| Terminal::port(i, p)).collect(), label: String::new() }
}
/// A boundary port joined to an inner terminal, whose type it takes.
fn port(at: &str, name: &str, label: &str, inner: (&str, &str)) -> Command {
    Command::AddBoundaryPort { at: at.into(), name: name.into(), port: sim_system::BoundaryPort { label: label.into(), schema: None }, connect: Some(Terminal::port(inner.0, inner.1)) }
}
fn est(value: f64, why: &str) -> ParameterBinding {
    ParameterBinding::Value { value, unit: None, provenance: Some(sim_inspect::Provenance::Estimated { explanation: why.into() }), uncertainty: None }
}
fn with(mut spec: InstanceSpec, name: &str, binding: ParameterBinding) -> InstanceSpec {
    spec.parameters.insert(name.into(), binding);
    spec
}
fn at(mut spec: InstanceSpec, position: [f32; 3], rotation: [f32; 4]) -> InstanceSpec {
    spec.placement.position = position;
    spec.placement.rotation_xyzw = rotation;
    spec
}
const S: f32 = std::f32::consts::FRAC_1_SQRT_2;
const Y_TO_X: [f32; 4] = [0., 0., -S, S];
const Y_TO_Z: [f32; 4] = [S, 0., 0., S];
const ID: [f32; 4] = [0., 0., 0., 1.];

fn apply(doc: &mut SystemDocument, registry: &sim_core::BehaviorRegistry, commands: Vec<Command>) {
    if let Err(e) = sim_system::apply(doc, registry, &commands) {
        panic!("{e}");
    }
}

/// A new definition (in `doc`) placed once at the root so its contents can be edited.
fn definition(doc: &mut SystemDocument, registry: &sim_core::BehaviorRegistry, id: &str, label: &str, description: &str, interface: &str) {
    let mut d = sim_system::Definition::new(label);
    d.description = description.into();
    d.interface = Some(interface.into());
    apply(doc, registry, vec![Command::AddDefinitions { definitions: [(id.to_string(), d)].into() }]);
}

pub fn build(root: &Path, registry: &sim_core::BehaviorRegistry) -> Result<(), String> {
    let lib = root.join("library/systems");
    let out = root.join("examples/systems-builder/parts-from-parts");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let motor_why = "Representative 12 V can motor (370 size); not a measured part.";

    // --- Library parts, built in a scratch document and published. ---
    let mut scratch = SystemDocument::new("parts");
    // Worm and planetary gearboxes already exist in the library.
    for g in ["worm_gearbox_30", "planetary_gearbox_30"] {
        apply(&mut scratch, registry, vec![Command::AddDefinitions { definitions: library::import(&lib.join(library::file_name(g))).map_err(|e| e.to_string())? }]);
    }
    definition(&mut scratch, registry, "dc_motor_12v", "DC motor 12 V", "Brushed motor with its rotor inertia and mount (case grounded inside). Exposes winding resistance and motor constant k (= k_t = k_e).", "motor.dc");
    apply(&mut scratch, registry, vec![
        add("", "m", InstanceSpec::subsystem("dc_motor_12v")),
        add("m", "winding", at(with(with(with(InstanceSpec::element("bridge.brushed_motor").labeled("Brushed motor"), "resistance", est(2.0, motor_why)), "torque_constant", est(0.012, motor_why)), "inductance", est(0.0005, motor_why)), [0., 0., 0.], Y_TO_X)),
        add("m", "rotor", at(with(InstanceSpec::element("rotational.inertia").labeled("Rotor"), "inertia", est(3e-6, motor_why)), [0.022, 0., 0.], Y_TO_X)),
        add("m", "mount", at(InstanceSpec::element("rotational.ground").labeled("Mount"), [-0.008, -0.018, 0.], ID)),
        join("m", &[("winding", "case"), ("mount", "flange")]),
        join("m", &[("winding", "shaft"), ("rotor", "shaft")]),
        port("m", "p", "Supply +", ("winding", "p")),
        port("m", "n", "Supply −", ("winding", "n")),
        port("m", "shaft", "Output shaft", ("winding", "shaft")),
        Command::ExposeParameter { definition: "dc_motor_12v".into(), instance: "winding".into(), inner: "resistance".into(), parameter: "resistance".into(), description: "Winding resistance".into() },
        Command::ExposeParameter { definition: "dc_motor_12v".into(), instance: "winding".into(), inner: "torque_constant".into(), parameter: "k".into(), description: "Motor constant k = k_t = k_e".into() },
        Command::SetParameter { at: "m".into(), name: "winding".into(), parameter: "back_emf_constant".into(), binding: Some(ParameterBinding::Parameter { parameter: "k".into() }) },
    ]);
    for (id, label, gearbox, description) in [
        ("worm_gearmotor_30", "Worm gearmotor 30:1", "worm_gearbox_30", "DC motor 12 V on a self-locking 30:1 worm gearbox."),
        ("planetary_gearmotor_30", "Planetary gearmotor 30:1", "planetary_gearbox_30", "DC motor 12 V on a 94 %-efficient 30:1 planetary gearbox (back-drivable)."),
    ] {
        definition(&mut scratch, registry, id, label, description, "gearmotor.dc");
        let a = format!("g_{id}");
        apply(&mut scratch, registry, vec![
            add("", &a, InstanceSpec::subsystem(id)),
            add(&a, "motor", at(InstanceSpec::subsystem("dc_motor_12v").labeled("Motor"), [-0.05, 0., 0.], ID)),
            add(&a, "gearbox", at(InstanceSpec::subsystem(gearbox).labeled("Gearbox"), [0., 0., 0.], ID)),
            join(&a, &[("motor", "shaft"), ("gearbox", "input")]),
            port(&a, "p", "Supply +", ("motor", "p")),
            port(&a, "n", "Supply −", ("motor", "n")),
            port(&a, "output", "Output shaft", ("gearbox", "output")),
        ]);
    }
    definition(&mut scratch, registry, "joint_actuator", "Joint actuator", "A gearmotor driving a joint through an output bearing (viscous loss 1 mN·m·s).", "actuator.rotary");
    apply(&mut scratch, registry, vec![
        add("", "j", InstanceSpec::subsystem("joint_actuator")),
        add("j", "drive", at(InstanceSpec::subsystem("worm_gearmotor_30").labeled("Gearmotor"), [0., 0., 0.], ID)),
        add("j", "bearing", at(InstanceSpec::element("rotational.damper").labeled("Output bearing").with("damping", 0.001), [0.0, -0.01, 0.02], ID)),
        add("j", "housing", at(InstanceSpec::element("rotational.ground").labeled("Housing"), [0.0, -0.02, 0.02], ID)),
        join("j", &[("bearing", "b"), ("housing", "flange")]),
        join("j", &[("drive", "output"), ("bearing", "a")]),
        port("j", "p", "Supply +", ("drive", "p")),
        port("j", "n", "Supply −", ("drive", "n")),
        port("j", "joint", "Joint", ("drive", "output")),
    ]);
    for id in ["dc_motor_12v", "worm_gearmotor_30", "planetary_gearmotor_30", "joint_actuator"] {
        for p in library::publish(&scratch, id, &lib).map_err(|e| e.to_string())? {
            println!("published {} v{} ({}) → {}", p.id, p.version, if p.changed { "changed" } else { "unchanged" }, p.path);
        }
    }

    // --- Two systems that import them. ---
    let mut winch = SystemDocument::new("Gearmotor winch");
    apply(&mut winch, registry, vec![Command::AddDefinitions { definitions: library::import(&lib.join(library::file_name("worm_gearmotor_30"))).map_err(|e| e.to_string())? }]);
    apply(&mut winch, registry, vec![
        add("", "supply", at(InstanceSpec::element("electrical.switched_voltage_source").labeled("12 V supply").with("voltage", 12.0).with("off_at", 1.2), [-0.08, 0., 0.04], ID)),
        add("", "gnd", at(InstanceSpec::element("electrical.ground"), [-0.08, 0., 0.], ID)),
        add("", "gearmotor", at(InstanceSpec::subsystem("worm_gearmotor_30").labeled("Gearmotor"), [0., 0.03, 0.], ID)),
        add("", "drum", at(with(InstanceSpec::element("rotational.inertia").labeled("Drum and 2 kg load"), "inertia", est(2.1e-4, "2 kg on a Ø20 drum: m·r² plus wheel")), [0., 0.0185, 0.], Y_TO_Z)),
        add("", "weight", at(with(InstanceSpec::element("rotational.load_torque").labeled("2 kg on the drum"), "torque", est(-0.19613, "−m·g·r")), [0.01, -0.012, 0.008], ID)),
        join("", &[("supply", "p"), ("gearmotor", "p")]),
        join("", &[("supply", "n"), ("gearmotor", "n"), ("gnd", "pin")]),
        join("", &[("gearmotor", "output"), ("drum", "shaft"), ("weight", "shaft")]),
    ]);
    winch.run = Some(run_settings());
    winch.realtime = Some(realtime(&["drum.shaft.speed", "drum.shaft.angle"], 2.0));
    let mut leg = SystemDocument::new("One-joint leg");
    apply(&mut leg, registry, vec![Command::AddDefinitions { definitions: library::import(&lib.join(library::file_name("joint_actuator"))).map_err(|e| e.to_string())? }]);
    apply(&mut leg, registry, vec![
        add("", "supply", at(InstanceSpec::element("electrical.switched_voltage_source").labeled("6 V pulse").with("voltage", 6.0).with("off_at", 0.12), [-0.08, 0., 0.04], ID)),
        add("", "gnd", at(InstanceSpec::element("electrical.ground"), [-0.08, 0., 0.], ID)),
        add("", "hip", at(InstanceSpec::subsystem("joint_actuator").labeled("Hip actuator"), [0., 0.1, 0.], ID)),
        add("", "thigh", at(with(InstanceSpec::element("rotational.inertia").labeled("Thigh link"), "inertia", est(0.3 * 0.2 * 0.2 / 3., "Uniform 0.3 kg, 0.2 m rod about its end: m·L²/3")), [0., 0.1, 0.02], Y_TO_Z)),
        add("", "gravity", at(with(with(InstanceSpec::element("part.pendulum_gravity").labeled("Gravity on the thigh"), "mass", est(0.3, "thigh mass")), "arm", est(0.1, "centre of a 0.2 m rod")), [0., 0.05, 0.02], ID)),
        join("", &[("supply", "p"), ("hip", "p")]),
        join("", &[("supply", "n"), ("hip", "n"), ("gnd", "pin")]),
        join("", &[("hip", "joint"), ("thigh", "shaft"), ("gravity", "shaft")]),
    ]);
    leg.run = Some(run_settings());
    leg.realtime = Some(realtime(&["thigh.shaft.angle", "thigh.shaft.speed"], 1.5));
    for (doc, name) in [(&winch, "winch"), (&leg, "leg")] {
        let path = out.join(format!("{name}.system.json"));
        let mut doc = doc.clone();
        doc.revision = 1;
        // Library paths are recorded relative to the repository root.
        for d in doc.definitions.values_mut() {
            if let Some(s) = &mut d.source {
                s.path = Path::new(&s.path).strip_prefix(root).map(|p| p.display().to_string()).unwrap_or(s.path.clone());
            }
        }
        std::fs::write(&path, serde_json::to_vec_pretty(&doc).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        println!("wrote {}", path.display());
    }
    Ok(())
}

/// Realtime profile: every part's realtime model at a 2 ms step; the bound
/// is 5 % of each observed quantity's range until a measurement tightens it.
fn realtime(observe: &[&str], duration: f64) -> sim_system::RealtimeProfile {
    sim_system::RealtimeProfile { interval: 0.002, integrator: sim_system::IntegratorChoice::BackwardEuler, observe: observe.iter().map(|s| s.to_string()).collect(), duration, bound: 0.05, bounds: Default::default(), notes: "Realtime models: motor winding inductance dropped (current algebraic), friction smoothing 0.05 rad/s; 2 ms step.".into(), measured: None }
}

fn run_settings() -> sim_system::RunSettings {
    sim_system::RunSettings { integrator: sim_system::IntegratorChoice::BackwardEuler, interval: 5e-4, absolute_tolerance: None, relative_tolerance: None, max_iterations: None, rationale: "Scheduled supply edge and a self-locking worm (stiff regularised friction): backward Euler, 0.5 ms as in the worm-drive winch.".into() }
}

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
    let (registry, loaded) = sim_runtime::registry_with_parts(&root.join("library/parts"));
    assert!(loaded.iter().all(|l| l.error.is_none()), "{loaded:?}");
    let _ = InstanceKind::Subsystem { definition: String::new() };
    if let Err(e) = build(&root, &registry) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
