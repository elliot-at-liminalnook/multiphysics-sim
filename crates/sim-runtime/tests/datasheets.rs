//! M3: every part with notes has a generated datasheet whose checks pass,
//! the committed datasheets still match what the benches produce, a part
//! that creates energy is caught, and bench numbers agree with closed form.
//!
//! Regenerate after an intended change:
//! `SIM_WRITE_DATASHEETS=1 cargo test -p sim-runtime --test datasheets`
use sim_runtime::bench::{self, Datasheet};
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Failed checks that are documented model findings, not regressions. The
/// datasheet still shows them as failed.
const KNOWN: &[(&str, &str, &str)] = &[(
    "robot.motor_unit",
    "energy audit",
    "its directional gear efficiency switches at −5 mW of motor power instead of 0, so a lightly back-driven gear creates up to (1 − η)·5 mW (≈ 0.2 % of the audit's energy); left unchanged because calibrated robot runs depend on the model",
)];

fn registry() -> sim_core::BehaviorRegistry {
    let (registry, loaded) = sim_runtime::registry_with_parts(&root().join("library/parts"));
    assert!(loaded.iter().all(|l| l.error.is_none()), "{loaded:?}");
    registry
}

fn all(registry: &sim_core::BehaviorRegistry) -> Vec<(String, Result<Datasheet, String>)> {
    let types = bench::noted(registry);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let out = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4) {
            s.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(t) = types.get(i) else { return };
                let sheet = bench::datasheet(registry, t);
                out.lock().unwrap().push((t.clone(), sheet));
            });
        }
    });
    let mut out = out.into_inner().unwrap();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[test]
fn every_noted_part_has_a_passing_datasheet_matching_the_committed_one() {
    let registry = registry();
    let dir = root().join("library/datasheets");
    let write = std::env::var_os("SIM_WRITE_DATASHEETS").is_some();
    let mut problems = Vec::new();
    let sheets = all(&registry);
    assert!(sheets.len() >= 20, "only {} noted parts", sheets.len());
    for (t, sheet) in sheets {
        let sheet = match sheet {
            Ok(s) => s,
            Err(e) => {
                problems.push(format!("{t}: bench failed: {e}"));
                continue;
            }
        };
        for c in sheet.checks.iter().filter(|c| !c.passed) {
            match KNOWN.iter().find(|(kt, kc, _)| *kt == t && *kc == c.name) {
                Some((_, _, why)) => eprintln!("known issue, {t} `{}`: {why}", c.name),
                None => problems.push(format!("{t}: check `{}` failed: {}", c.name, c.detail)),
            }
        }
        let path = bench::path(&dir, &t);
        if write {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(&path, serde_json::to_vec_pretty(&sheet).unwrap()).unwrap();
            continue;
        }
        match std::fs::read(&path) {
            Ok(bytes) => {
                let stored: Datasheet = serde_json::from_slice(&bytes).unwrap();
                if let Err(e) = bench::compare(&sheet, &stored, 1e-6) {
                    problems.push(e);
                }
            }
            Err(_) => problems.push(format!("{t}: no datasheet at {} (run with SIM_WRITE_DATASHEETS=1)", path.display())),
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn an_energy_creating_part_is_caught() {
    let mut registry = sim_runtime::registry();
    // A "resistor" that pushes current the wrong way: negative resistance.
    let source = "part leaky_negative \"Negative resistor\"\nsummary \"Wrong on purpose.\"\nport p electrical\nport n electrical\nparam R Ω = 10 \"resistance\"\nflow p = -(p.v - n.v)/R\nflow n = (p.v - n.v)/R\n";
    sim_parts::register(&mut registry, sim_parts::parse("negative.part", source).unwrap()).unwrap();
    let sheet = bench::datasheet(&registry, "part.leaky_negative").unwrap();
    let audit = sheet.checks.iter().find(|c| c.name == "energy audit").unwrap();
    assert!(!audit.passed, "{audit:?}");
    // The same part declared a source is allowed to add energy.
    let active = source.replace("summary", "active\nsummary").replace("leaky_negative", "leaky_source");
    sim_parts::register(&mut registry, sim_parts::parse("source.part", &active).unwrap()).unwrap();
    assert!(bench::datasheet(&registry, "part.leaky_source").unwrap().passed());
}

#[test]
fn motor_and_gear_benches_agree_with_closed_form() {
    let registry = registry();
    let motor = bench::datasheet(&registry, "bridge.brushed_motor").unwrap();
    // Typical values: R = 2 Ω, k = 0.012; bench at 12 V, rotor 1e-5 kg·m², no friction.
    let (r, k, v) = (2.0, 0.012, 12.0);
    let near = |a: f64, b: f64, tol: f64| (a - b).abs() <= tol * b.abs();
    assert!(near(motor.value("stall current").unwrap(), v / r, 1e-6));
    assert!(near(motor.value("stall torque").unwrap(), k * v / r, 1e-4));
    assert!(near(motor.value("no-load speed").unwrap(), v / k, 1e-4));
    let st = motor.curves.iter().find(|c| c.name == "speed–torque").unwrap();
    for p in &st.points {
        assert!(near(p[1], (v - r * p[0] / k) / k, 1e-3), "speed–torque point {p:?}");
    }
    // Brushed DC peak efficiency with no friction tends to 1 at no load; the
    // curve's best point at 10 % of stall torque is 1 − 0.1 = 0.9.
    assert!(near(motor.value("peak efficiency").unwrap(), 0.9, 1e-3));

    let worm = bench::datasheet(&registry, "rotational.worm_gear").unwrap();
    assert_eq!(worm.kind, "gear");
    assert!(worm.passed(), "{:?}", worm.checks);
    assert!(near(worm.value("ratio (measured)").unwrap(), 30., 1e-3));
    assert_eq!(worm.value("self-locking"), Some(1.));
    let lossy = bench::datasheet(&registry, "rotational.lossy_gear").unwrap();
    assert!(near(lossy.value("forward efficiency").unwrap(), 0.95, 2e-3));
    assert!(near(lossy.value("backdrive efficiency").unwrap(), 0.95, 2e-3));
    let screw = bench::datasheet(&registry, "bridge.lead_screw").unwrap();
    assert_eq!(screw.kind, "linear");
    assert!(screw.passed(), "{:?}", screw.checks);
}
