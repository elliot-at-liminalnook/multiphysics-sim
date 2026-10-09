//! One composition model for every system (docs/architecture/composition.md):
//! a thermostat room and a rover (a generated robot assembly with its own
//! battery, an enclosure on its thermal boundary and two independent
//! controller FMUs) authored through the command path, saved, reloaded,
//! flattened with path identities and run headlessly through the session
//! Build mode uses. Refusals are checked where they happen: units when
//! wiring, artifacts and host blocks when binding.

use sim_runtime::{composition_examples, registry, system_builder};
use sim_system::{Command, InstanceKind, ParameterBinding, ParameterDecl, SystemStore, Terminal};
use std::path::{Path, PathBuf};

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("composition-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn run(path: &Path, seconds: f64, keys: &[&str]) -> Result<Vec<system_builder::Series>, String> {
    let document = SystemStore::new(path).load().map_err(|e| e.to_string())?;
    let config = system_builder::config_for(&document);
    system_builder::simulate_at(&document, &registry(), path.parent().unwrap(), seconds, config, &keys.iter().map(|k| k.to_string()).collect::<Vec<_>>())
}

fn series<'a>(all: &'a [system_builder::Series], key: &str) -> &'a system_builder::Series {
    all.iter().find(|s| s.label.contains(key)).unwrap_or_else(|| panic!("no series {key}; have {:?}", all.iter().map(|s| &s.label).collect::<Vec<_>>()))
}

#[test]
fn a_thermostat_fmu_regulates_a_room_built_from_ordinary_parts() {
    let d = dir("room");
    let path = composition_examples::thermostat(&d, &registry()).unwrap();
    let all = run(&path, 3600.0, &["thermometer.temperature", "thermostat.heater_power"]).unwrap();
    let t = series(&all, "thermometer.temperature");
    let settled: Vec<f64> = t.times.iter().zip(&t.values).filter(|(time, _)| **time > 1800.0).map(|(_, v)| *v).collect();
    let (lo, hi) = settled.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
    assert!(lo > 294.15 - 0.55 && hi < 294.15 + 0.55, "the room stays in the thermostat's band: {lo}..{hi}");
    let heater = series(&all, "heater_power");
    assert!(heater.values.iter().any(|p| *p == 500.0) && heater.values.iter().any(|p| *p == 0.0), "the heater cycles");
}

#[test]
fn a_rover_composes_with_a_battery_an_enclosure_and_two_controller_fmus() {
    let d = dir("rover");
    let path = composition_examples::rover(&d, &registry()).unwrap();
    // Saved and reloaded: the same document, the blocks and the generated
    // assembly flatten with their instance paths.
    let document = SystemStore::new(&path).load().unwrap();
    assert!(matches!(document.definitions["root"].instances["rover"].kind, InstanceKind::Generated { .. }));
    let flat = system_builder::flatten(&document, &registry(), Some(&d)).unwrap();
    let blocks: Vec<&str> = flat.model.blocks.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(blocks, ["left_controller", "right_controller"]);
    assert!(flat.components.keys().any(|k| k.starts_with("rover/") && k.ends_with("robot")), "the assembly keeps its path: {:?}", flat.components.keys().take(5).collect::<Vec<_>>());
    assert!(flat.components.contains_key("battery") && flat.components.contains_key("enclosure"));

    let keys = ["left_controller.target", "right_controller.target", "rover/joint.left axle.encoder.angle", "rover/joint.right axle.encoder.angle", "battery.soc", "enclosure.node"];
    let all = run(&path, 2.0, &keys).unwrap();
    let soc = series(&all, "battery.soc");
    assert!(soc.values.last().unwrap() < &1.0, "the battery supplies the motors: soc {:?}", soc.values.last());
    let enclosure = series(&all, "enclosure.node");
    assert!(enclosure.values.iter().all(|t| t.is_finite() && *t >= 293.15 - 1e-9), "motor heat reaches the enclosure, never cools it");
    // Two instances of one FMU with their own parameters: different targets.
    let (left, right) = (series(&all, "left_controller.target"), series(&all, "right_controller.target"));
    let peak = |s: &system_builder::Series| s.values.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
    assert!(peak(left) > 0.5 && peak(right) < 0.45, "left follows amplitude 0.6, right 0.4: {} {}", peak(left), peak(right));
    // The wheels follow their own controllers.
    let angle = series(&all, "rover/joint.left axle.encoder.angle");
    assert!(angle.values.iter().fold(0.0_f64, |m, v| m.max(v.abs())) > 0.2, "the left wheel turns");
}

#[test]
fn reruns_from_the_file_are_reproducible() {
    let d = dir("rerun");
    let path = composition_examples::thermostat(&d, &registry()).unwrap();
    let a = run(&path, 600.0, &["thermometer.temperature"]).unwrap();
    let b = run(&path, 600.0, &["thermometer.temperature"]).unwrap();
    assert_eq!(a[0].values, b[0].values);
}

#[test]
fn block_parameters_bind_to_the_enclosing_definition() {
    let d = dir("binding");
    let path = composition_examples::thermostat(&d, &registry()).unwrap();
    let store = SystemStore::new(&path);
    store.apply(&registry(), "Expose the setpoint", &[
        Command::DeclareParameter { at: String::new(), name: "comfort".into(), declaration: ParameterDecl { unit: "K".into(), default: Some(296.15), description: "Room setpoint".into() } },
        Command::SetParameter { at: String::new(), name: "thermostat".into(), parameter: "setpoint".into(), binding: Some(ParameterBinding::Parameter { parameter: "comfort".into() }) },
    ], None).unwrap();
    let document = store.load().unwrap();
    let flat = system_builder::flatten(&document, &registry(), Some(&d)).unwrap();
    let sim_core::ImplementationRef::Fmi3 { parameters, .. } = &flat.model.blocks[0].implementation else { panic!("an FMU block") };
    assert_eq!(parameters["setpoint"], 296.15);
    let all = run(&path, 3600.0, &["thermometer.temperature"]).unwrap();
    let last = *all[0].values.last().unwrap();
    assert!((last - 296.15).abs() < 0.6, "the room follows the inherited setpoint: {last}");
}

#[test]
fn a_block_in_a_subsystem_keeps_its_path() {
    let d = dir("group");
    let path = composition_examples::thermostat(&d, &registry()).unwrap();
    let store = SystemStore::new(&path);
    store.apply(&registry(), "Group the controls", &[Command::Group { at: String::new(), instances: vec!["thermostat".into(), "heater".into()], name: "controls".into(), definition: "controls".into(), label: "Controls".into() }], None).unwrap();
    let document = store.load().unwrap();
    let flat = system_builder::flatten(&document, &registry(), Some(&d)).unwrap();
    assert_eq!(flat.model.blocks[0].name, "controls/thermostat");
    let all = run(&path, 600.0, &["thermometer.temperature"]).unwrap();
    assert!(all[0].values.iter().all(|v| v.is_finite()));
}

#[test]
fn wiring_a_block_to_another_quantity_is_refused_when_connecting() {
    let d = dir("units");
    let path = composition_examples::thermostat(&d, &registry()).unwrap();
    let store = SystemStore::new(&path);
    let err = store.apply(&registry(), "Miswire", &[
        Command::Disconnect { at: String::new(), terminal: Terminal::port("thermostat", "heater_power") },
        Command::AddInstance { at: String::new(), name: "shaft".into(), instance: sim_system::InstanceSpec::element(sim_domain_rotational::elements::TORQUE_SOURCE) },
        Command::Connect { at: String::new(), terminals: vec![Terminal::port("thermostat", "heater_power"), Terminal::port("shaft", "torque")], label: String::new() },
    ], None).unwrap_err().to_string();
    assert!(err.contains("block signal") && err.contains("HeatFlow") && err.contains("Torque"), "{err}");
}

#[test]
fn a_changed_fmu_or_a_host_block_is_refused_before_the_run() {
    let d = dir("refusals");
    let path = composition_examples::thermostat(&d, &registry()).unwrap();
    // Rebuild the FMU with a different setpoint default: a different artifact.
    let fmu = d.join("fmus/thermostat.fmu");
    let mut bytes = std::fs::read(&fmu).unwrap();
    bytes.extend_from_slice(b"tampered");
    std::fs::write(&fmu, bytes).unwrap();
    let err = run(&path, 1.0, &["thermometer.temperature"]).unwrap_err();
    assert!(err.contains("not the FMU the model was built with"), "{err}");
    // A host block: this host does not supply it.
    let path = composition_examples::thermostat(&dir("host"), &registry()).unwrap();
    let store = SystemStore::new(&path);
    let document = store.load().unwrap();
    let InstanceKind::Block { interface, timing, .. } = document.definitions["root"].instances["thermostat"].kind.clone() else { panic!() };
    store.apply(&registry(), "Swap to a host policy", &[
        Command::SetParameter { at: String::new(), name: "thermostat".into(), parameter: "setpoint".into(), binding: None },
        Command::Swap { at: String::new(), name: "thermostat".into(), kind: InstanceKind::Block { implementation: sim_system::BlockSource::Host { name: "operator".into() }, interface: sim_core::BlockInterface { feedthrough: true, ..interface }, timing }, keep_parameters: false },
    ], None).unwrap();
    let err = run(&path, 1.0, &["thermometer.temperature"]).unwrap_err();
    assert!(err.contains("host implementation `operator`") && err.contains("supplies no implementation"), "{err}");
}

fn room_test(min: f64, max: f64) -> sim_system::SystemTest {
    sim_system::SystemTest {
        duration_s: 3600.0,
        description: "Holds the room near its setpoint".into(),
        requirements: vec![
            sim_system::Requirement { id: "comfort".into(), observable: "thermometer.temperature".into(), reduce: sim_system::Reduce::Mean, window: Some([1800.0, 3600.0]), min: Some(min), max: Some(max), description: String::new() },
            sim_system::Requirement { id: "heater_rating".into(), observable: "thermostat.heater_power".into(), reduce: sim_system::Reduce::Max, window: None, min: None, max: Some(500.0), description: String::new() },
        ],
    }
}

#[test]
fn evidence_is_bound_to_the_model_the_artifacts_the_settings_and_the_test() {
    use sim_runtime::system_evidence::{self, Standing, Status, Verdict};
    let d = dir("evidence");
    let path = composition_examples::thermostat(&d, &registry()).unwrap();
    let store = SystemStore::new(&path);
    store.apply(&registry(), "Add a test", &[Command::SetTest { name: "comfort".into(), test: Some(room_test(293.6, 294.7)) }], None).unwrap();
    let document = store.load().unwrap();
    // Never run: not assessed.
    assert_eq!(system_evidence::standing(&document, &path).unwrap()["comfort"], Standing::NotAssessed);
    let evidence = system_evidence::assess(&document, &registry(), &d, "comfort", None).unwrap();
    assert_eq!(evidence.verdict, Verdict::Passed, "{:?}", evidence.results);
    system_evidence::save(&path, &evidence).unwrap();
    assert_eq!(system_evidence::standing(&document, &path).unwrap()["comfort"], Standing::Current { verdict: Verdict::Passed });

    // A display-only edit keeps it current; a controller parameter does not.
    store.apply(&registry(), "Move", &[Command::MoveInstance { at: String::new(), name: "thermostat".into(), placement: sim_system::Placement { position: [3.0, 0.0, 0.0], ..Default::default() } }], None).unwrap();
    assert!(matches!(system_evidence::standing(&store.load().unwrap(), &path).unwrap()["comfort"], Standing::Current { .. }));
    store.apply(&registry(), "Retune", &[Command::SetParameter { at: String::new(), name: "thermostat".into(), parameter: "setpoint".into(), binding: Some(ParameterBinding::value(295.15)) }], None).unwrap();
    let Standing::Stale { changed, .. } = &system_evidence::standing(&store.load().unwrap(), &path).unwrap()["comfort"] else { panic!("stale after retuning") };
    assert!(changed.iter().any(|c| c.contains("model changed")), "{changed:?}");
    store.undo().unwrap();
    assert!(matches!(system_evidence::standing(&store.load().unwrap(), &path).unwrap()["comfort"], Standing::Current { .. }), "undone: current again");

    // A different controller artifact on disk.
    let fmu = d.join("fmus/thermostat.fmu");
    let original = std::fs::read(&fmu).unwrap();
    std::fs::write(&fmu, [original.as_slice(), b"x"].concat()).unwrap();
    let Standing::Stale { changed, .. } = &system_evidence::standing(&store.load().unwrap(), &path).unwrap()["comfort"] else { panic!("stale after the FMU changed") };
    assert!(changed.iter().any(|c| c.contains("artifact of `thermostat`")), "{changed:?}");
    std::fs::write(&fmu, &original).unwrap();

    // A changed test.
    store.apply(&registry(), "Tighten", &[Command::SetTest { name: "comfort".into(), test: Some(room_test(294.0, 294.3)) }], None).unwrap();
    let Standing::Stale { changed, .. } = &system_evidence::standing(&store.load().unwrap(), &path).unwrap()["comfort"] else { panic!("stale after the test changed") };
    assert_eq!(changed, &vec!["the test changed".to_owned()]);

    // A requirement the run cannot judge is never a pass.
    let mut unjudgeable = room_test(293.6, 294.7);
    unjudgeable.requirements[0].observable = "no_such.signal".into();
    store.apply(&registry(), "Unjudgeable", &[Command::SetTest { name: "comfort".into(), test: Some(unjudgeable) }], None).unwrap();
    let evidence = system_evidence::assess(&store.load().unwrap(), &registry(), &d, "comfort", None).unwrap();
    assert_eq!(evidence.verdict, Verdict::Incomplete);
    assert_eq!(evidence.results[0].status, Status::NotAssessed);
    // And a failing one fails.
    store.apply(&registry(), "Impossible", &[Command::SetTest { name: "comfort".into(), test: Some(room_test(300.0, 301.0)) }], None).unwrap();
    let evidence = system_evidence::assess(&store.load().unwrap(), &registry(), &d, "comfort", None).unwrap();
    assert_eq!(evidence.verdict, Verdict::Failed);
}

/// A flattened system is a self-contained model world: the generated
/// robot's physical model travels with it as a resource (the articulated
/// element's `model` parameter is its key), so the world serialises, reads
/// back, compiles and runs exactly as the original.
#[test]
fn a_flattened_robot_system_serialises_and_runs_the_same() {
    let d = dir("serialise");
    let path = composition_examples::rover(&d, &registry()).unwrap();
    let document = SystemStore::new(&path).load().unwrap();
    let registry = registry();
    let flat = sim_system::flatten_with(&document, &registry, &sim_runtime::robot_generator::generators(&d)).unwrap();
    assert_eq!(flat.model.resources.len(), 1, "the robot's model is carried by the world");
    let key = *flat.model.resources.keys().next().unwrap();
    let robot = flat.model.behaviors.values().find(|b| b.kind.0 == sim_domain_robot::ARTICULATED).expect("an articulated robot");
    assert_eq!(robot.parameters["model"].value_si, key as f64, "the element names its model by key");
    let copy: sim_core::ModelWorld = serde_json::from_str(&serde_json::to_string(&flat.model).unwrap()).unwrap();
    assert_eq!(copy.resources, flat.model.resources);
    let config = system_builder::config_for(&document);
    let run = |model: sim_core::ModelWorld| -> Vec<(String, f64)> {
        let mut runtime = sim_compile::Runtime::new(model, &registry, config.integrator).unwrap();
        sim_runtime::system_blocks::bind(&mut runtime, Some(&d)).unwrap();
        runtime.advance(0.05, config.interval.min(1.0e-3)).unwrap();
        runtime.model.state.iter().map(|(_, s)| (s.name.clone(), s.committed)).collect()
    };
    let (a, b) = (run(flat.model.clone()), run(copy));
    assert_eq!(a.len(), b.len());
    for ((name, x), (other, y)) in a.iter().zip(&b) {
        assert_eq!(name, other);
        assert!(x == y || (x.is_nan() && y.is_nan()), "{name}: {x} vs {y}");
    }
}

/// Robot mode runs a robot project's system as composed. Where a controller
/// owns a joint's servo target, a person moves the joint through the
/// controller's free setpoint (the joint-controller FMU's `setpoint` input,
/// annotated `sim.setpoint`): the jog plan names it, and moving it moves
/// the joint. Nothing is refused here: both axles have a setpoint.
#[test]
fn a_controlled_joint_is_jogged_through_its_controllers_setpoint() {
    use sim_runtime::teleop::{Via, compose_robot, plan_for};
    let d = dir("jog");
    let path = composition_examples::rover(&d, &registry()).unwrap();
    let document = SystemStore::new(&path).load().unwrap();
    let (robot, plan) = plan_for(&document, &d).unwrap();
    assert_eq!(robot, "rover");
    assert!(plan.refused.is_empty(), "{:?}", plan.refused);
    let left = plan.channels.iter().find(|c| c.joint == "left axle").expect("the left axle can be moved");
    assert_eq!(left.via, Via::Setpoint { block: "left_controller".into(), input: "setpoint".into() });
    let run = |offset: f64| -> f64 {
        let mut composed = compose_robot(&document, &d, &registry(), &Default::default()).unwrap();
        let index = composed.robot.joint_names.iter().position(|n| n.trim_start_matches("joint.") == "left axle").unwrap();
        composed.robot.set_target(index, offset);
        composed.robot.advance(1.0).unwrap();
        composed.robot.joint_angles()[index]
    };
    let (still, moved) = (run(0.0), run(0.5));
    assert!((moved - still) > 0.2, "the setpoint did not move the joint: {still} → {moved}");
}

/// A joint whose target a block drives without a setpoint input is refused
/// by name, and a free target is moved directly.
#[test]
fn a_joint_without_a_free_setpoint_is_refused_by_name() {
    use sim_runtime::teleop::{Via, plan_for};
    let d = dir("jog-refused");
    let path = composition_examples::rover(&d, &registry()).unwrap();
    let mut document = SystemStore::new(&path).load().unwrap();
    // Clear the right controller's setpoint, and free the left axle's target.
    let commands = vec![
        Command::SetBlockSetpoint { at: String::new(), name: "right_controller".into(), input: "setpoint".into(), joint: None },
        Command::Disconnect { at: String::new(), terminal: sim_system::Terminal::port("rover", "left axle.target") },
    ];
    sim_system::commands::apply(&mut document, &registry(), &commands).unwrap();
    let (_, plan) = plan_for(&document, &d).unwrap();
    assert!(plan.refused.get("right axle").is_some_and(|why| why.contains("right_controller") && why.contains("no setpoint")), "{:?}", plan.refused);
    assert!(plan.channels.iter().any(|c| c.joint == "left axle" && c.via == Via::Target), "{:?}", plan.channels);
}
