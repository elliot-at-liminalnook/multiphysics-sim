//! Proving case for hierarchical system files: the motor-driver board.
//!
//! Checks, all through the shared runtime and the committed system file:
//! - structure: three subsystems, path identities, compiles;
//! - the closed-loop buck regulates its 5 V rail;
//! - MOSFET heating equals conduction loss I²·2R_on(T) (sync rectification
//!   puts exactly two switches in the motor path at every instant);
//! - swapping the switching bridge for the averaged library implementation
//!   (same ports) preserves the mean motor speed;
//! - halving the step changes the regulated rail and speed negligibly.
use sim_runtime::system_builder::{self, Series};
use sim_system::{Command, InstanceKind, SystemDocument};
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn board() -> SystemDocument {
    let path = root().join("examples/systems-builder/motor-driver-board/board.system.json");
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}
const DURATION: f64 = 0.06;
const WINDOW: (f64, f64) = (0.045, 0.06);

fn series<'a>(all: &'a [Series], key: &str) -> &'a Series {
    all.iter().find(|s| s.label == key).unwrap_or_else(|| panic!("no series {key}; have {:?}", all.iter().map(|s| &s.label).collect::<Vec<_>>()))
}
fn window(s: &Series) -> Vec<f64> {
    s.times.iter().zip(&s.values).filter(|(t, _)| **t >= WINDOW.0 && **t <= WINDOW.1).map(|(_, v)| *v).collect()
}
fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}

fn run(document: &SystemDocument, interval: f64) -> Vec<Series> {
    let registry = sim_runtime::registry();
    let mut config = system_builder::config_for(document);
    config.interval = interval;
    system_builder::simulate(
        document,
        &registry,
        DURATION,
        config,
        &["regulator/c.p.voltage".into(), "servo/motor.shaft.speed".into(), "servo/motor.p.current".into(), ".heat.heat_flow".into(), "bridge/heatsink.node.temperature".into()],
    )
    .unwrap()
}

#[test]
fn board_structure_compiles_with_path_identities() {
    let registry = sim_runtime::registry();
    let document = board();
    let check = system_builder::check(&document, &registry).unwrap();
    assert!(check.compile_error.is_none(), "{:?}", check.compile_error);
    assert_eq!(check.subsystems, 3);
    assert!(check.findings.is_empty(), "{:?}", check.findings);
    let compiled = system_builder::compile(&document, &registry, system_builder::config_for(&document)).unwrap();
    for id in ["regulator/q", "bridge/q_hi_a", "servo/motor", "battery"] {
        assert!(compiled.description.components.contains_key(id), "missing {id}");
    }
    assert_eq!(compiled.description.components["bridge/q_lo_b"].group.as_deref(), Some("bridge"));
    assert!(compiled.animation.as_ref().is_some_and(|a| !a.colors.is_empty()), "temperatures animate");
    // Every subsystem is in the library, plus the averaged bridge alternative.
    let library = sim_system::library::list(&root().join("library/systems"), &registry).unwrap();
    for id in ["buck_5v", "h_bridge_mosfet", "hx30hm_servo", "h_bridge_averaged"] {
        assert!(library.iter().any(|e| e.id == id), "library lacks {id}");
    }
}

#[test]
fn switching_board_regulates_heats_consistently_and_matches_its_averaged_swap() {
    let document = board();
    let detailed = run(&document, 1e-5);

    // 5 V rail within 1 % once settled.
    let vout = mean(&window(series(&detailed, "regulator/c.p.voltage")));
    assert!((vout - 5.0).abs() < 0.05, "regulated rail {vout} V");

    // Heatsink input equals conduction loss through two on-switches.
    let current = window(series(&detailed, "servo/motor.p.current"));
    let i2 = mean(&current.iter().map(|i| i * i).collect::<Vec<_>>());
    let temperature = mean(&window(series(&detailed, "bridge/heatsink.node.temperature")));
    let r_on = 0.02 * (1. + 0.004 * (temperature - 298.15));
    let heat: f64 = ["q_hi_a", "q_lo_a", "q_hi_b", "q_lo_b"].iter().map(|q| -mean(&window(series(&detailed, &format!("bridge/{q}.heat.heat_flow"))))).sum();
    let expected = i2 * 2. * r_on;
    assert!(expected > 1e-3, "motor draws current: I²={i2}");
    assert!((heat - expected).abs() < 0.05 * expected, "heat {heat} W vs I²·2R_on {expected} W");

    // Averaged implementation from the library: same contract, same mean speed.
    let speed = mean(&window(series(&detailed, "servo/motor.shaft.speed")));
    assert!(speed > 1.0, "motor turns: {speed} rad/s");
    let registry = sim_runtime::registry();
    let mut averaged = document.clone();
    let definitions = sim_system::library::import(&root().join("library/systems/h_bridge_averaged.definition.json")).unwrap();
    sim_system::apply(
        &mut averaged,
        &registry,
        &[Command::AddDefinitions { definitions }, Command::Swap { at: String::new(), name: "bridge".into(), kind: InstanceKind::Subsystem { definition: "h_bridge_averaged".into() }, keep_parameters: true }],
    )
    .unwrap();
    let fast = run(&averaged, 1e-5);
    let averaged_speed = mean(&window(series(&fast, "servo/motor.shaft.speed")));
    assert!((averaged_speed - speed).abs() < 0.01 * speed, "averaged {averaged_speed} vs switching {speed} rad/s");
}

#[test]
fn halving_the_step_leaves_rail_and_speed_unchanged() {
    let document = board();
    let coarse = run(&document, 1e-5);
    let fine = run(&document, 5e-6);
    for (key, tolerance) in [("regulator/c.p.voltage", 0.005), ("servo/motor.shaft.speed", 0.005)] {
        let a = mean(&window(series(&coarse, key)));
        let b = mean(&window(series(&fine, key)));
        assert!((a - b).abs() < tolerance * b.abs(), "{key}: {a} at 10 µs vs {b} at 5 µs");
    }
}
