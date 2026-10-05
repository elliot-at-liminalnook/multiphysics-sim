//! M5: edits apply to a running session (parameters keep the state,
//! structure restarts), edit-to-rerun is fast, and runs are kept with their
//! document, seed and settings so they compare and replay exactly.
use sim_runtime::run_history;
use sim_runtime::system_builder;
use sim_runtime::system_session::{Command as SessionCommand, ModelSource, SystemSession};
use sim_system::{Command, InstanceSpec, ParameterBinding, SystemDocument};
use std::path::PathBuf;
use std::time::Instant;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn load(path: &str) -> SystemDocument {
    serde_json::from_slice(&std::fs::read(root().join(path)).unwrap()).unwrap()
}

fn source(doc: &SystemDocument, registry: &sim_core::BehaviorRegistry) -> ModelSource {
    let compiled = system_builder::compile(doc, registry, system_builder::config_for(doc)).unwrap();
    ModelSource { model: compiled.flat.model.clone(), registry: registry.clone(), identities: compiled.flat.identities.clone(), source_hash: compiled.flat.source_hash.clone(), revision: doc.revision.max(1), base: None }
}

fn session(doc: &SystemDocument, registry: &sim_core::BehaviorRegistry, observe: &str) -> (SystemSession, String) {
    let s = source(doc, registry);
    let mut session = SystemSession::new("live".into(), system_builder::config_for(doc), move |c| s.build(c)).unwrap();
    let id = session.description().observables.keys().find(|id| system_builder::observable_key(session.description(), id) == observe).unwrap().clone();
    session.subscribe(vec![id.clone()]).unwrap();
    session.execute(SessionCommand::Start).unwrap();
    (session, id)
}

fn value(session: &SystemSession, id: &str) -> f64 {
    sim_inspect::animation::scalar(Some(session.latest()), id).unwrap().value
}

#[test]
fn parameter_edits_keep_the_running_state_and_structure_edits_restart() {
    let registry = winch_registry();
    let mut doc = load("examples/systems-builder/worm-drive/winch.system.json");
    let (mut live, speed) = session(&doc, &registry, "drum.shaft.speed");
    while live.status().time < 0.6 {
        live.tick().unwrap();
    }
    let before = value(&live, &speed);
    let t = live.status().time;
    // A weaker supply: 12 V → 9 V, applied to the running session.
    sim_system::apply(&mut doc, &registry, &[Command::SetParameter { at: String::new(), name: "supply".into(), parameter: "voltage".into(), binding: Some(ParameterBinding::value(9.0)) }]).unwrap();
    let s = source(&doc, &registry);
    let started = Instant::now();
    assert!(live.hot_swap(move |c| s.build(c)).unwrap(), "a parameter edit keeps the state");
    assert!((live.status().time - t).abs() < 1e-12, "the clock carries over");
    assert!((value(&live, &speed) - before).abs() < 1e-9, "the state carries over");
    while live.status().time < 1.1 {
        live.tick().unwrap();
    }
    let after = value(&live, &speed);
    eprintln!("drum {before:.3} rad/s at 12 V → {after:.3} rad/s after the edit to 9 V (swap {:.1} ms)", 1e3 * started.elapsed().as_secs_f64());
    assert!(after < 0.8 * before, "the new voltage takes effect");

    // A structural edit restarts at t = 0.
    sim_system::apply(&mut doc, &registry, &[Command::AddInstance { at: String::new(), name: "tach".into(), instance: InstanceSpec::element("rotational.speed_sensor") }, Command::Connect { at: String::new(), terminals: vec![sim_system::Terminal::port("tach", "shaft"), sim_system::Terminal::port("drum", "shaft")], label: String::new() }]).unwrap();
    let s = source(&doc, &registry);
    assert!(!live.hot_swap(move |c| s.build(c)).unwrap());
    assert_eq!(live.status().time, 0.);
}

/// Edit → recompiled → first frame of the new model. The roadmap target is
/// under 2 s for systems up to about 50 parts; measured on the winch and on
/// the motor-driver board (≈ 40 parts, switching).
#[test]
fn edit_to_rerun_is_under_two_seconds() {
    let registry = winch_registry();
    for (path, instance, parameter, value) in [
        ("examples/systems-builder/worm-drive/winch.system.json", "supply", "voltage", 9.0),
        ("examples/systems-builder/motor-driver-board/board.system.json", "", "", 0.0),
    ] {
        let mut doc = load(path);
        let observe = if path.contains("board") { "servo/motor.shaft.speed" } else { "drum.shaft.speed" };
        let (mut live, _) = session(&doc, &registry, observe);
        live.tick().unwrap();
        let started = Instant::now();
        let edit = if !instance.is_empty() {
            Command::SetParameter { at: String::new(), name: instance.into(), parameter: parameter.into(), binding: Some(ParameterBinding::value(value)) }
        } else {
            // Nudge the first explicit parameter of a top-level element by 1 %.
            let (name, spec) = doc.definitions["root"].instances.iter().find(|(_, s)| s.parameters.values().any(|b| matches!(b, ParameterBinding::Value { .. }))).unwrap();
            let (p, v) = spec.parameters.iter().find_map(|(k, b)| if let ParameterBinding::Value { value, .. } = b { Some((k.clone(), *value)) } else { None }).unwrap();
            Command::SetParameter { at: String::new(), name: name.clone(), parameter: p, binding: Some(ParameterBinding::value(v * 1.01)) }
        };
        sim_system::apply(&mut doc, &registry, &[edit]).unwrap();
        let s = source(&doc, &registry);
        live.hot_swap(move |c| s.build(c)).unwrap();
        live.tick().unwrap();
        let seconds = started.elapsed().as_secs_f64();
        let parts: usize = system_builder::compile(&doc, &registry, system_builder::config_for(&doc)).unwrap().description.components.len();
        eprintln!("{path}: {parts} parts, edit → first new frame in {:.0} ms", 1e3 * seconds);
        assert!(seconds < 2.0, "{path}: {seconds:.2} s");
    }
}

#[test]
fn runs_are_kept_compared_and_replayed_exactly() {
    let registry = winch_registry();
    let doc = load("examples/systems-builder/worm-drive/winch.system.json");
    let dir = std::env::temp_dir().join(format!("sim-runs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let select = vec!["drum.shaft.speed".to_string(), "motor.p.current".to_string()];
    let mut config = system_builder::config_for(&doc);
    config.seed = 7;
    let a = run_history::record(&doc, &registry, 1.0, config.clone(), &select, "12 V").unwrap();
    let mut weaker = doc.clone();
    sim_system::apply(&mut weaker, &registry, &[Command::SetParameter { at: String::new(), name: "supply".into(), parameter: "voltage".into(), binding: Some(ParameterBinding::value(9.0)) }]).unwrap();
    let b = run_history::record(&weaker, &registry, 1.0, config, &select, "9 V").unwrap();
    run_history::save(&dir, &a).unwrap();
    run_history::save(&dir, &b).unwrap();
    let listed = run_history::list(&dir);
    assert_eq!(listed.len(), 2);
    assert!(listed.iter().all(|(_, s)| s.seed == 7));
    let loaded: Vec<_> = listed.iter().map(|(p, _)| run_history::load(p).unwrap()).collect();
    let metric = sim_system::Metric { label: "lift speed".into(), observable: "drum.shaft.speed".into(), reduce: sim_system::Reduce::Mean, window: Some([0.8, 1.0]) };
    let table = run_history::compare(&loaded, &[metric]);
    let speeds: Vec<f64> = table.variants.iter().map(|v| v.metrics[0].1).collect();
    eprintln!("{}", sim_runtime::system_study::table(&table));
    assert!((speeds[0] - speeds[1]).abs() > 1.0, "runs differ: {speeds:?}");
    for r in &loaded {
        assert_eq!(run_history::replay(r, &registry).unwrap(), 0.0, "replay reproduces {}", r.id);
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// The winch uses authored library parts (rope and end stop).
fn winch_registry() -> sim_core::BehaviorRegistry {
    sim_runtime::registry_with_parts(&std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../library/parts")).0
}
