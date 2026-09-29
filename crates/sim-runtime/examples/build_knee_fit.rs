//! Builds the knee-servo fit case from measured data (M7): the gearmotor
//! library definition (estimated values, marked so), a bench system that
//! reproduces the suspended-leg measurement, and a `sim.fit/1` spec that
//! `sim-system fit` runs and promotes.
//!
//!     cargo run -p sim-runtime --example build_knee_fit
use sim_runtime::part_fit::{Condition, Target, Unknown};
use sim_system::{Command, InstanceSpec, ParameterBinding, SystemDocument, Terminal};
use std::path::PathBuf;

fn est(value: f64, why: &str) -> ParameterBinding {
    ParameterBinding::Value { value, unit: None, provenance: Some(sim_inspect::Provenance::Estimated { explanation: why.into() }), uncertainty: None }
}
fn spec(t: &str, label: &str, params: &[(&str, ParameterBinding)]) -> InstanceSpec {
    let mut s = InstanceSpec::element(t).labeled(label);
    for (k, v) in params {
        s.parameters.insert(k.to_string(), v.clone());
    }
    s
}
fn add(at: &str, name: &str, s: InstanceSpec) -> Command {
    Command::AddInstance { at: at.into(), name: name.into(), instance: s }
}
fn join(at: &str, a: (&str, &str), b: (&str, &str)) -> Command {
    Command::Connect { at: at.into(), terminals: vec![Terminal::port(a.0, a.1), Terminal::port(b.0, b.1)], label: String::new() }
}
fn port(at: &str, name: &str, inner: (&str, &str)) -> Command {
    Command::AddBoundaryPort { at: at.into(), name: name.into(), port: Default::default(), connect: Some(Terminal::port(inner.0, inner.1)) }
}
fn target(at: &str, instance: &str, parameter: &str) -> Target {
    Target { at: at.into(), instance: instance.into(), parameter: parameter.into() }
}

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
    let registry = sim_runtime::registry();
    let dir = "examples/actuators/hx30hm/accepted";
    let data_path = format!("{dir}/knee-steady-speeds.json");
    let data_bytes = std::fs::read(root.join(&data_path)).unwrap();
    let data: serde_json::Value = serde_json::from_slice(&data_bytes).unwrap();
    let family = "HX-30HM provisional family estimate (examples/actuators/hx30hm/accepted/hx30hm-provisional.json); not identified.";

    // The gearmotor definition, placed once in the bench system.
    let mut doc = SystemDocument::new("HX-30HM knee servo bench (suspended leg)");
    let mut d = sim_system::Definition::new("HX-30HM knee gearmotor");
    d.description = "HX-30HM servo drive train: brushed motor, rotor, 200:1 gear at the family's estimated efficiency, output friction. k and friction are fitted to the knee servo's measured steady speeds; the rest are estimates.".into();
    d.interface = Some("gearmotor.dc".into());
    let g = "servo";
    let mut commands = vec![
        Command::AddDefinitions { definitions: [("hx30hm_knee_gearmotor".to_string(), d)].into() },
        add("", g, InstanceSpec::subsystem("hx30hm_knee_gearmotor").labeled("Knee servo")),
        add(g, "motor", spec("bridge.brushed_motor", "Motor", &[("resistance", est(3.7, family)), ("inductance", est(0.0, "neglected: electrical time constant far below the 100 Hz measurement")), ("torque_constant", est(0.00973, family)), ("back_emf_constant", est(0.00973, family))])),
        add(g, "rotor", spec("rotational.inertia", "Rotor", &[("inertia", est(3e-7, "small can motor rotor, order of magnitude"))])),
        add(g, "gear", spec("rotational.lossy_gear", "200:1 gear", &[("ratio", est(200.0, family)), ("efficiency", est(0.504, family))])),
        add(g, "friction", spec("rotational.coulomb_friction", "Output friction", &[("torque", est(0.025, family))])),
        add(g, "mount", InstanceSpec::element("rotational.ground").labeled("Case")),
        join(g, ("motor", "case"), ("mount", "flange")),
        join(g, ("motor", "shaft"), ("rotor", "shaft")),
        join(g, ("rotor", "shaft"), ("gear", "input")),
        join(g, ("gear", "output"), ("friction", "shaft")),
        port(g, "p", ("motor", "p")),
        port(g, "n", ("motor", "n")),
        port(g, "output", ("gear", "output")),
        // The rig: supply at the commanded duty, the suspended leg, gravity.
        add("", "supply", spec("electrical.voltage_source", "Supply × duty", &[("voltage", ParameterBinding::value(0.0))])),
        add("", "gnd", InstanceSpec::element("electrical.ground")),
        add("", "leg", spec("rotational.inertia", "Suspended leg", &[("inertia", est(2e-3, "printed leg about the knee, order of magnitude"))])),
        add("", "gravity", spec("rotational.load_torque", "Gravity bias (mid range)", &[("torque", ParameterBinding::value(0.0))])),
        join("", ("supply", "p"), (g, "p")),
        Command::Connect { at: String::new(), terminals: vec![Terminal::port("supply", "n"), Terminal::port(g, "n"), Terminal::port("gnd", "pin")], label: String::new() },
        Command::Connect { at: String::new(), terminals: vec![Terminal::port(g, "output"), Terminal::port("leg", "shaft"), Terminal::port("gravity", "shaft")], label: String::new() },
    ];
    sim_system::apply(&mut doc, &registry, &std::mem::take(&mut commands)).unwrap();
    doc.run = Some(sim_system::RunSettings { integrator: sim_system::IntegratorChoice::BackwardEuler, interval: 1e-3, absolute_tolerance: None, relative_tolerance: None, max_iterations: None, rationale: "Steady speeds after ≤ 0.1 s time constants; friction regularisation is stiff.".into() });
    doc.revision = 1;
    let bench = format!("{dir}/hx30hm-knee-bench.system.json");
    std::fs::write(root.join(&bench), serde_json::to_vec_pretty(&doc).unwrap()).unwrap();

    let supply = data["conditions"]["supply_v"].as_f64().unwrap();
    let conditions: Vec<Condition> = data["points"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            let duty = p["duty"].as_f64().unwrap();
            Condition { label: format!("run {} duty {duty:+.2}", p["run"]), set: vec![(target("", "supply", "voltage"), duty * supply)], observable: "leg.shaft.speed".into(), window: [1.5, 2.0], measured: p["speed_rad_s"].as_f64().unwrap() }
        })
        .collect();
    let unknowns = vec![
        Unknown { name: "k".into(), unit: "V·s/rad".into(), initial: 0.00973, targets: vec![target(g, "motor", "torque_constant"), target(g, "motor", "back_emf_constant")], minimum: Some(1e-6) },
        Unknown { name: "friction".into(), unit: "N·m".into(), initial: 0.025, targets: vec![target(g, "friction", "torque")], minimum: Some(0.0) },
        Unknown { name: "gravity_bias".into(), unit: "N·m".into(), initial: 0.0, targets: vec![target("", "gravity", "torque")], minimum: None },
    ];
    let spec = serde_json::json!({
        "schema": "sim.fit/1",
        "description": "Fit the HX-30HM knee gearmotor's motor constant and output friction (and the rig's gravity bias) to steady speeds measured on the suspended leg.",
        "model": bench,
        "data": {"path": data_path, "blake3": blake3::hash(&data_bytes).to_hex().to_string()},
        "duration": 2.0,
        "unknowns": unknowns,
        "conditions": conditions,
        "promote": {"definition": "hx30hm_knee_gearmotor", "unknowns": ["k", "friction"], "library": "library/systems"},
        "compare": {"k": {"value": 0.012132012913329035, "uncertainty": 0.001010967875889402, "source": "examples/actuators/hx30hm/accepted/hx30hm-knee-measured.derivation.json (campaign line fit)"}}
    });
    let spec_path = format!("{dir}/hx30hm-knee-gearmotor.fit.json");
    std::fs::write(root.join(&spec_path), serde_json::to_vec_pretty(&spec).unwrap()).unwrap();
    println!("wrote {bench}\nwrote {spec_path}\nrun: sim-system fit {spec_path} [--promote]");
}

