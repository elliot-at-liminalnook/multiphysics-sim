//! Acceptance case for the worm gear: a 12 V brushed motor winding a 2 kg
//! load through a 30:1 worm gearbox (`examples/systems-builder/worm-drive`).
//!
//! Every check is against a closed-form result, run through the shared
//! runtime and the committed system file:
//! - steady lifting speed and current from the DC-motor equations with the
//!   textbook worm forward efficiency η = (cos φₙ − μ tan λ)/(cos φₙ + μ cot λ);
//! - measured efficiency (load power ÷ motor shaft power) equals η;
//! - with the supply off, the self-locking worm holds the load (creep below
//!   a bound set by the friction regularisation);
//! - swapping in the lossless spur gearbox (same ports), the load back-drives
//!   the shorted motor at ω = τ·R/(k²·N²) at the wheel;
//! - halving the step changes the steady speed by < 0.5 %.
use sim_domain_rotational::helical::worm_contact;
use sim_runtime::system_builder::{self, Series};
use sim_system::{Command, InstanceKind, SystemDocument};
use std::collections::BTreeMap;
use std::path::PathBuf;

const V: f64 = 12.0;
const R: f64 = 2.0;
const K: f64 = 0.012;
const LOAD: f64 = 0.19613; // 2 kg · g · 10 mm
const OFF_AT: f64 = 1.2;

fn winch() -> SystemDocument {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/systems-builder/worm-drive/winch.system.json");
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn run(document: &SystemDocument, duration: f64, interval: f64) -> Vec<Series> {
    let registry = winch_registry();
    let mut config = system_builder::config_for(document);
    config.interval = interval;
    system_builder::simulate(document, &registry, duration, config, &["drum.shaft.angle".into(), "drum.shaft.speed".into(), "rotor.shaft.speed".into(), "motor.p.current".into()]).unwrap()
}

fn series<'a>(all: &'a [Series], key: &str) -> &'a Series {
    all.iter().find(|s| s.label == key).unwrap_or_else(|| panic!("no series {key}; have {:?}", all.iter().map(|s| &s.label).collect::<Vec<_>>()))
}
fn mean_between(s: &Series, a: f64, b: f64) -> f64 {
    let v: Vec<f64> = s.times.iter().zip(&s.values).filter(|(t, _)| **t >= a && **t <= b).map(|(_, v)| *v).collect();
    v.iter().sum::<f64>() / v.len() as f64
}
fn at(s: &Series, t: f64) -> f64 {
    let i = s.times.iter().position(|x| *x >= t - 1e-9).unwrap();
    s.values[i]
}

fn forward_efficiency() -> f64 {
    let mut p = BTreeMap::new();
    p.insert("friction".to_string(), 0.07);
    worm_contact(&p).forward_efficiency()
}

#[test]
fn worm_winch_lifts_at_the_analytic_speed_and_efficiency() {
    let eta = forward_efficiency();
    assert!((0.40..0.50).contains(&eta), "η = {eta}");
    let worm_torque = LOAD / (30. * eta);
    let current = worm_torque / K;
    let motor_speed = (V - R * current) / K;

    let out = run(&winch(), 1.0, 5e-4);
    let (w1, w2, i) = (mean_between(series(&out, "rotor.shaft.speed"), 0.7, 1.0), mean_between(series(&out, "drum.shaft.speed"), 0.7, 1.0), mean_between(series(&out, "motor.p.current"), 0.7, 1.0).abs());
    eprintln!("motor {w1:.2} rad/s (analytic {motor_speed:.2}), drum {w2:.4} rad/s, current {i:.4} A (analytic {current:.4})");
    assert!((w1 - motor_speed).abs() / motor_speed < 0.005, "motor speed {w1} vs {motor_speed}");
    assert!((w1 / w2 - 30.).abs() < 1e-3, "ratio {}", w1 / w2);
    assert!((i - current).abs() / current < 0.005, "current {i} vs {current}");
    // Efficiency from the simulated powers alone.
    let measured = LOAD * w2 / (K * i * w1);
    assert!((measured - eta).abs() < 0.005, "measured efficiency {measured} vs textbook {eta}");
}

#[test]
fn self_locking_worm_holds_but_the_spur_swap_backdrives() {
    let document = winch();
    let out = run(&document, 2.0, 5e-4);
    let angle = series(&out, "drum.shaft.angle");
    let (before, after) = (at(angle, 1.4), at(angle, 2.0));
    eprintln!("worm: drum angle {before:.5} → {after:.5} rad after power-off");
    assert!(at(angle, OFF_AT) > 10.0, "the load was lifted first");
    assert!((after - before).abs() < 2e-3, "self-locked drum crept {} rad in 0.6 s", after - before);

    // Swap the gearbox for the lossless spur alternative: same ports.
    let registry = winch_registry();
    let mut spur = document.clone();
    sim_system::commands::apply(&mut spur, &registry, &[Command::Swap { at: String::new(), name: "gearbox".into(), kind: InstanceKind::Subsystem { definition: "spur_gearbox_30".into() }, keep_parameters: false }]).unwrap();
    let out = run(&spur, 2.0, 5e-4);
    let speed = mean_between(series(&out, "drum.shaft.speed"), 1.6, 2.0);
    // Shorted motor as a brake: k²·ω₁/R = τ_load/N, ω₂ = ω₁/N.
    let expected = -LOAD / 30. * R / (K * K) / 30.;
    eprintln!("spur: drum speed after power-off {speed:.4} rad/s (analytic {expected:.4})");
    assert!((speed - expected).abs() / expected.abs() < 0.01, "back-drive speed {speed} vs {expected}");
}

#[test]
fn halving_the_step_barely_changes_the_lift() {
    let document = winch();
    let a = mean_between(series(&run(&document, 0.8, 5e-4), "drum.shaft.speed"), 0.6, 0.8);
    let b = mean_between(series(&run(&document, 0.8, 2.5e-4), "drum.shaft.speed"), 0.6, 0.8);
    assert!((a - b).abs() / b.abs() < 0.005, "{a} vs {b}");
}

#[test]
fn winch_compiles_with_notes_and_animates_single_shafts() {
    let registry = winch_registry();
    let document = winch();
    let check = system_builder::check(&document, &registry).unwrap();
    assert!(check.compile_error.is_none(), "{:?}", check.compile_error);
    assert!(check.findings.is_empty(), "{:?}", check.findings);
    let compiled = system_builder::compile(&document, &registry, system_builder::config_for(&document)).unwrap();
    let animation = compiled.animation.unwrap();
    let spinning: Vec<&str> = animation.rotations.iter().map(|r| r.part.as_str()).collect();
    for part in ["part/rotor", "part/drum", "part/gearbox/worm"] {
        assert!(spinning.contains(&part), "{part} should spin; spinning: {spinning:?}");
    }
    for part in ["part/motor", "part/gearbox/mesh"] {
        assert!(!spinning.contains(&part), "{part} is a housing and must not spin");
    }
    let notes = registry.get(&"rotational.worm_gear".into()).unwrap().notes.unwrap();
    let derived = notes.derive(&BTreeMap::new());
    assert!(derived.iter().any(|d| d.name == "self-locking" && d.value == 1.));
}

/// M1: the saved studies in the winch file compare gearboxes and sweep the
/// worm's starts, reproducibly from the file alone.
#[test]
fn saved_studies_compare_gearboxes_and_sweep_worm_starts() {
    use sim_runtime::system_study;
    let registry = winch_registry();
    let document = winch();
    let value = |v: &system_study::VariantResult, k: &str| v.metrics.iter().chain(v.derived.iter().map(|d| (d.name.clone(), d.value)).collect::<Vec<_>>().iter()).find(|(n, _)| n.contains(k)).map(|(_, x)| *x).unwrap();

    let compare = system_study::run(&document, &registry, "gearboxes", &document.studies["gearboxes"], 3, None, &|_, _| {}).unwrap();
    eprintln!("{}", system_study::table(&compare));
    let [worm, spur, planetary] = &compare.variants[..] else { panic!("three variants") };
    assert!(worm.error.is_none() && spur.error.is_none() && planetary.error.is_none());
    assert!(value(worm, "travel after").abs() < 2e-3, "worm holds");
    assert!(value(spur, "travel after") < -1.0 && value(planetary, "travel after") < -1.0, "spur and planetary back-drive");
    // Efficient gearboxes lift faster on less current.
    assert!(value(spur, "lift speed") > value(planetary, "lift speed") && value(planetary, "lift speed") > value(worm, "lift speed"));
    assert!(value(spur, "motor current") < value(worm, "motor current"));

    let sweep = system_study::run(&document, &registry, "worm_starts", &document.studies["worm_starts"], 4, None, &|_, _| {}).unwrap();
    eprintln!("{}", system_study::table(&sweep));
    let efficiency: Vec<f64> = sweep.variants.iter().map(|v| value(v, "mesh: forward efficiency")).collect();
    let locking: Vec<f64> = sweep.variants.iter().map(|v| value(v, "mesh: self-locking")).collect();
    assert!(efficiency.windows(2).all(|w| w[1] > w[0]), "efficiency rises with starts: {efficiency:?}");
    assert_eq!(locking, [1., 0., 0., 0.], "only the single-start worm self-locks");
    let travel: Vec<f64> = sweep.variants.iter().map(|v| value(v, "travel after")).collect();
    assert!(travel[0].abs() < 2e-3 && travel[1..].iter().all(|t| *t < -0.1), "holds only with one start: {travel:?}");

    // Reproducible: the same file gives the same numbers.
    let again = system_study::run(&document, &registry, "worm_starts", &document.studies["worm_starts"], 2, None, &|_, _| {}).unwrap();
    assert_eq!(serde_json::to_string(&again.variants.iter().map(|v| &v.metrics).collect::<Vec<_>>()).unwrap(), serde_json::to_string(&sweep.variants.iter().map(|v| &v.metrics).collect::<Vec<_>>()).unwrap());
}

/// The winch uses authored library parts (rope and end stop).
fn winch_registry() -> sim_core::BehaviorRegistry {
    sim_runtime::registry_with_parts(&std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../library/parts")).0
}
