//! M2: parts written as equations (`library/parts/*.part`) behave exactly
//! like hand-written Rust components, carry notes and derived values, snap
//! like built-ins, report errors by line, and hot-reload.
use sim_runtime::system_builder::{self, Series};
use sim_system::{Command, InstanceKind, SystemDocument};
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn winch() -> SystemDocument {
    serde_json::from_slice(&std::fs::read(root().join("examples/systems-builder/worm-drive/winch.system.json")).unwrap()).unwrap()
}
fn registry() -> sim_core::BehaviorRegistry {
    let (registry, loaded) = sim_runtime::registry_with_parts(&root().join("library/parts"));
    for l in &loaded {
        assert!(l.error.is_none(), "{}: {:?}", l.path, l.error);
    }
    registry
}
fn swap_motor(document: &SystemDocument, registry: &sim_core::BehaviorRegistry, to: &str) -> SystemDocument {
    let mut doc = document.clone();
    sim_system::apply(&mut doc, registry, &[Command::Swap { at: String::new(), name: "motor".into(), kind: InstanceKind::Element { component_type: to.into() }, keep_parameters: true }]).unwrap();
    doc
}
fn run(doc: &SystemDocument, registry: &sim_core::BehaviorRegistry, duration: f64) -> Vec<Series> {
    system_builder::simulate(doc, registry, duration, system_builder::config_for(doc), &["motor.p.current".into(), "drum.shaft.speed".into(), "drum.shaft.angle".into()]).unwrap()
}

#[test]
fn equation_motor_matches_the_rust_motor_to_1e9() {
    let registry = registry();
    let document = winch();
    let rust = run(&document, &registry, 2.0);
    let authored = run(&swap_motor(&document, &registry, "part.brushed_motor_eq"), &registry, 2.0);
    let mut worst = 0f64;
    for (a, b) in rust.iter().zip(&authored) {
        assert_eq!(a.label, b.label);
        let scale = a.values.iter().fold(0f64, |m, v| m.max(v.abs())).max(1e-12);
        for (x, y) in a.values.iter().zip(&b.values) {
            worst = worst.max((x - y).abs() / scale);
        }
    }
    eprintln!("worst relative difference, authored vs Rust motor: {worst:.2e}");
    assert!(worst < 1e-9, "authored motor differs from Rust by {worst}");
}

#[test]
fn coreless_motor_loads_with_notes_snaps_and_heats() {
    let registry = registry();
    let d = registry.get(&"part.coreless_motor".into()).unwrap();
    let notes = d.notes.unwrap();
    assert!(notes.summary.contains("ironless"));
    assert!(notes.equations.iter().any(|e| e.contains("der(i)")));
    let derived = notes.derive(&sim_system::library::effective_parameters(&registry, "part.coreless_motor", &Default::default()));
    let hot = derived.iter().find(|d| d.name == "torque constant when hot").unwrap();
    assert!((hot.value - 0.012 * (1. - 0.0012 * 80.)).abs() < 1e-12);
    let source = std::fs::read_to_string(root().join("library/parts/coreless_motor.part")).unwrap();
    assert!(source.lines().filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#')).count() <= 50, "about 50 lines");

    // It appears in the palette and snaps like a built-in.
    assert!(sim_system::library::elements(&registry).iter().any(|e| e.component_type == "part.coreless_motor" && e.notes.is_some()));
    let mut doc = SystemDocument::new("coreless");
    let spec = sim_system::snap::starter(&registry, &InstanceKind::Element { component_type: "part.coreless_motor".into() }, "motor");
    sim_system::apply(&mut doc, &registry, &[Command::AddInstance { at: String::new(), name: "motor".into(), instance: spec }]).unwrap();
    let ports = sim_system::snap::suggestions(&doc, &registry, None, "", "motor").unwrap();
    let heat = ports.iter().find(|p| p.port == "heat").unwrap();
    assert!(heat.candidates.iter().any(|c| c.label.contains("capacitance") || c.label.contains("Thermal")), "thermal parts snap onto the heat port");

    // In the winch, with a winding thermal mass: it heats, and heating costs speed.
    let mut hot = swap_motor(&winch(), &registry, "part.coreless_motor");
    sim_system::apply(&mut hot, &registry, &[
        Command::AddInstance { at: String::new(), name: "winding".into(), instance: sim_system::InstanceSpec::element("thermal.capacitance").with("heat_capacity", 2.0).with("initial.temperature", 293.15) },
        Command::Connect { at: String::new(), terminals: vec![sim_system::Terminal::port("motor", "heat"), sim_system::Terminal::port("winding", "node")], label: String::new() },
    ]).unwrap();
    let out = system_builder::simulate(&hot, &registry, 1.0, system_builder::config_for(&hot), &["winding.node.temperature".into(), "motor.p.current".into()]).unwrap();
    let temperature = out.iter().find(|s| s.label.contains("temperature")).unwrap();
    let current = out.iter().find(|s| s.label.contains("current")).unwrap();
    let rise = temperature.values.last().unwrap() - temperature.values[0];
    // Copper loss integral ≈ C·ΔT (no path to ambient). Backward Euler
    // applies each step's heat at the step's end values, so sum those.
    let heat: f64 = (1..current.times.len())
        .map(|k| {
            // The swap kept the winch motor's R = 2.0 Ω (same parameter name).
            let r = 2.0 * (1. + 0.00393 * (temperature.values[k] - 293.15));
            r * current.values[k].powi(2) * (current.times[k] - current.times[k - 1])
        })
        .sum();
    eprintln!("coreless winding: ΔT = {rise:.4} K, copper heat {heat:.4} J vs C·ΔT {:.4} J", 2.0 * rise);
    assert!(rise > 0.1);
    assert!((heat - 2.0 * rise).abs() / heat < 0.01, "energy balance: heat {heat} J vs {} J", 2.0 * rise);
}

#[test]
fn errors_name_the_line_and_hot_reload_replaces_the_part() {
    let bad_unit = "part bad \"Bad\"\nport p electrical\nport n electrical\nparam R Ω = 1\nstate i Current\neq i = p.v - n.v\n";
    let e = sim_parts::parse("bad.part", bad_unit).unwrap_err();
    assert!(e.starts_with("bad.part:6:") && e.contains("differ in dimension"), "{e}");
    let bad_port = "part bad \"Bad\"\nport shaft rotational\nstate x Angle\neq x = shaft.v\n";
    let e = sim_parts::parse("bad.part", bad_port).unwrap_err();
    assert!(e.starts_with("bad.part:4:") && e.contains("phi, w"), "{e}");
    let bad_flow = "part bad \"Bad\"\nport p electrical\nparam R Ω = 1\nflow p = p.v\n";
    let e = sim_parts::parse("bad.part", bad_flow).unwrap_err();
    assert!(e.starts_with("bad.part:4:") && e.contains("must be A"), "{e}");
    let prefix = "part bad \"Bad\"\nport p electrical\nparam L mH = 1\n";
    assert!(sim_parts::parse("bad.part", prefix).unwrap_err().contains("prefixes"));

    // Hot reload: edit a file in place, refresh, and the registry changes.
    let dir = std::env::temp_dir().join(format!("sim-parts-reload-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("load.part");
    let write = |ohms: f64| std::fs::write(&file, format!("part reload_load \"Load\"\nport p electrical\nport n electrical\nparam R Ω = {ohms} \"Resistance\"\nflow p = (p.v - n.v)/R\nflow n = -(p.v - n.v)/R\n")).unwrap();
    write(10.);
    let mut registry = sim_runtime::registry();
    let mut library = sim_parts::PartLibrary::new(&dir);
    assert_eq!(library.refresh(&mut registry).unwrap()[0].type_id.as_deref(), Some("part.reload_load"));
    assert!(library.refresh(&mut registry).is_none(), "nothing changed");
    let default = |r: &sim_core::BehaviorRegistry| r.get(&"part.reload_load".into()).unwrap().parameters.as_ref().unwrap().iter().find(|p| p.name == "R").unwrap().default;
    assert_eq!(default(&registry), Some(10.));
    write(22.);
    assert!(library.refresh(&mut registry).is_some());
    assert_eq!(default(&registry), Some(22.));
    std::fs::remove_dir_all(&dir).ok();
}
