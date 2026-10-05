//! M4: a motor subsystem inside gearmotors inside a joint actuator, used by
//! two systems (a winch and a leg). One edit to the motor, published to the
//! library, reaches both; same-interface parts swap anywhere; where-used
//! finds every file.
use sim_runtime::system_builder;
use sim_system::{library, Command, InstanceKind, ParameterDecl, SystemDocument};
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A private copy of the library and the two systems (tests never edit the repo).
fn workspace(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sim-parts-from-parts-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for sub in ["library/systems", "examples/systems-builder/parts-from-parts"] {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
        for e in std::fs::read_dir(repo().join(sub)).unwrap().flatten().filter(|e| e.path().is_file()) {
            std::fs::copy(e.path(), dir.join(sub).join(e.file_name())).unwrap();
        }
    }
    dir
}

fn load(root: &Path, name: &str) -> SystemDocument {
    serde_json::from_slice(&std::fs::read(root.join(format!("examples/systems-builder/parts-from-parts/{name}.system.json"))).unwrap()).unwrap()
}

fn registry() -> sim_core::BehaviorRegistry {
    let (r, loaded) = sim_runtime::registry_with_parts(&repo().join("library/parts"));
    assert!(loaded.iter().all(|l| l.error.is_none()));
    r
}

fn mean_after(doc: &SystemDocument, r: &sim_core::BehaviorRegistry, duration: f64, key: &str, from: f64, to: f64) -> f64 {
    let s = system_builder::simulate(doc, r, duration, system_builder::config_for(doc), &[key.to_string()]).unwrap();
    let s = s.iter().find(|s| s.label == key).unwrap_or_else(|| panic!("no {key}"));
    let v: Vec<f64> = s.times.iter().zip(&s.values).filter(|(t, _)| **t >= from && **t <= to).map(|(_, v)| *v).collect();
    v.iter().sum::<f64>() / v.len() as f64
}

#[test]
fn nested_parts_run_and_one_motor_edit_reaches_both_systems() {
    let r = registry();
    let root = workspace("edit");
    let (winch, leg) = (load(&root, "winch"), load(&root, "leg"));
    for doc in [&winch, &leg] {
        let check = system_builder::check(doc, &r).unwrap();
        assert!(check.compile_error.is_none() && check.findings.is_empty(), "{:?} {:?}", check.compile_error, check.findings);
    }
    // Three levels deep: leg → hip (joint_actuator) → drive (gearmotor) → motor (dc_motor_12v).
    let compiled = system_builder::compile(&leg, &r, system_builder::config_for(&leg)).unwrap();
    assert!(compiled.description.components.contains_key("hip/drive/motor/winding"));

    let lift = mean_after(&winch, &r, 1.0, "drum.shaft.speed", 0.8, 1.0);
    let leg_up = mean_after(&leg, &r, 1.5, "thigh.shaft.angle", 1.4, 1.5);
    let leg_at_off = mean_after(&leg, &r, 0.5, "thigh.shaft.angle", 0.45, 0.5);
    eprintln!("winch lift {lift:.3} rad/s; leg raised to {leg_up:.3} rad, {leg_at_off:.3} rad when the power went off");
    assert!(lift > 20.0);
    assert!(leg_up > 0.3 && leg_up < 1.5 && (leg_up - leg_at_off).abs() < 2e-3, "the worm raises the leg and holds it after power-off");

    // Where-used: both files place the motor (through their nested parts).
    let files = library::system_files(&root.join("examples"));
    let used = library::where_used(&files, "dc_motor_12v");
    assert_eq!(used.len(), 2, "{used:?}");

    // Edit the motor inside the winch (a stronger magnet: k 0.012 → 0.015) and publish.
    let mut edited = winch.clone();
    sim_system::apply(&mut edited, &r, &[Command::DeclareParameter { at: "gearmotor/motor".into(), name: "k".into(), declaration: ParameterDecl { unit: "N·m/A".into(), default: Some(0.015), description: "Motor constant k = k_t = k_e".into() } }]).unwrap();
    let published = library::publish(&edited, "dc_motor_12v", &root.join("library/systems")).unwrap();
    let motor = published.iter().find(|p| p.id == "dc_motor_12v").unwrap();
    assert!(motor.changed && motor.version == 2, "{published:?}");
    // Files that bundle the motor were republished too.
    for id in ["worm_gearmotor_30", "joint_actuator"] {
        assert!(published.iter().any(|p| p.id == id && p.changed), "{id} not republished: {published:?}");
    }

    // Both systems now see a stale import and sync to it (one undoable edit each).
    let mut results = Vec::new();
    for (name, key, from, to, dur) in [("winch", "drum.shaft.speed", 0.8, 1.0, 1.0), ("leg", "thigh.shaft.angle", 1.4, 1.5, 1.5)] {
        let mut doc = load(&root, name);
        let stale = library::stale(&doc, &root);
        assert!(stale.iter().any(|s| s.id == "dc_motor_12v" || s.id == "worm_gearmotor_30" || s.id == "joint_actuator"), "{name} not stale: {stale:?}");
        let commands = library::sync(&doc, &root).unwrap();
        sim_system::apply(&mut doc, &r, &commands).unwrap();
        assert!(library::stale(&doc, &root).is_empty(), "{name} still stale after sync");
        assert_eq!(doc.definitions["dc_motor_12v"].parameters["k"].default, Some(0.015));
        results.push(mean_after(&doc, &r, dur, key, from, to));
    }
    eprintln!("after the motor edit: winch lift {:.3} rad/s (was {lift:.3}); leg {:.3} rad (was {leg_up:.3})", results[0], results[1]);
    // A larger k lowers no-load speed (ω ≈ V/k): the winch lifts slower.
    assert!(results[0] < lift * 0.9, "winch changed");
    assert!((results[1] - leg_up).abs() > 1e-3, "leg changed");
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn same_interface_parts_swap_inside_nested_parts() {
    let r = registry();
    let root = workspace("swap");
    let mut leg = load(&root, "leg");
    sim_system::apply(&mut leg, &r, &[Command::AddDefinitions { definitions: library::import(&root.join("library/systems/planetary_gearmotor_30.definition.json")).unwrap() }]).unwrap();
    // The swap offer at hip/drive lists the planetary gearmotor as the same interface.
    let offers = library::alternatives(&leg, &r, Some(&root.join("library/systems")), "hip", "drive").unwrap();
    assert!(offers.iter().any(|a| a.same_interface && a.kind == InstanceKind::Subsystem { definition: "planetary_gearmotor_30".into() }), "{offers:?}");
    // Swapping edits the shared joint_actuator definition; make it unique first to keep the library copy.
    sim_system::apply(&mut leg, &r, &[
        Command::MakeUnique { at: String::new(), name: "hip".into(), definition: "joint_actuator_planetary".into() },
        Command::Swap { at: "hip".into(), name: "drive".into(), kind: InstanceKind::Subsystem { definition: "planetary_gearmotor_30".into() }, keep_parameters: true },
    ]).unwrap();
    let raised = mean_after(&leg, &r, 0.12, "thigh.shaft.angle", 0.115, 0.12);
    let later = mean_after(&leg, &r, 1.5, "thigh.shaft.angle", 1.45, 1.5);
    eprintln!("planetary leg: {raised:.3} rad at power-off, {later:.3} rad 1.4 s later");
    assert!(later < raised - 0.05, "a planetary gearmotor lets the leg fall");
    std::fs::remove_dir_all(&root).ok();
}
