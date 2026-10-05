//! FMI 3 Co-Simulation FMUs as blocks, end to end, with the repository's
//! fixture FMUs built from C (examples/fmi): inspection, a thermostat
//! regulating a room, independent instances, reproducible reset, timing,
//! exact units, refusal of everything outside the profile, fault handling
//! and cleanup, artifact identity, checkpoints.

use sim_compile::Runtime;
use sim_core::{BlockTiming, ModelWorld, QuantityKind as Q, StateId};
use sim_fmi::{Cache, Fmu, FmiError};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

fn fixtures() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("sim-fmi-fixtures-{}", std::process::id()));
        let examples = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/fmi");
        for model in ["thermostat", "joint-controller"] {
            sim_fmi::pack::pack(&examples.join(model), &dir.join(format!("{model}.fmu"))).expect("the fixture builds");
        }
        dir
    })
}

fn thermostat() -> PathBuf {
    fixtures().join("thermostat.fmu")
}

/// A room (heat capacity, loss to a cold outside) with a heater and a
/// thermometer, and one thermostat block per room (`setpoints`).
struct Rooms {
    runtime: Runtime,
    temperatures: Vec<StateId>,
    heaters: Vec<StateId>,
}

fn rooms(setpoints: &[f64], period: f64, extra: impl Fn(&str) -> BTreeMap<String, f64>) -> Result<Rooms, FmiError> {
    let registry = sim_runtime::registry();
    let fmu = Fmu::load(thermostat())?;
    let mut m = ModelWorld::default();
    let outside = m.part(&registry, "outside", sim_domain_thermal::AMBIENT, [("temperature", 278.15)]).unwrap();
    let mut outside_ports = vec![outside.port("node")];
    let mut handles = Vec::new();
    for (k, setpoint) in setpoints.iter().enumerate() {
        let room = format!("room{k}");
        let air = m.part(&registry, &format!("{room}.air"), sim_domain_thermal::CAPACITANCE, [("heat_capacity", 2.0e4), ("initial.temperature", 290.15)]).unwrap();
        let wall = m.part(&registry, &format!("{room}.wall"), sim_domain_thermal::CONDUCTANCE, [("conductance", 8.0)]).unwrap();
        let heater = m.part(&registry, &format!("{room}.heater"), sim_domain_thermal::CONTROLLED_HEAT_SOURCE, []).unwrap();
        let thermometer = m.part(&registry, &format!("{room}.thermometer"), sim_domain_thermal::TEMPERATURE_SENSOR, []).unwrap();
        m.connect([air.port("node"), wall.port("a"), heater.port("node"), thermometer.port("node")]);
        outside_ports.push(wall.port("b"));
        let mut parameters = BTreeMap::from([("setpoint".to_owned(), *setpoint)]);
        parameters.extend(extra(&room));
        let block = sim_fmi::add_block(&mut m, &format!("{room}/thermostat"), &fmu, &thermostat().display().to_string(), &BTreeMap::new(), BlockTiming::periodic(period), parameters)?;
        m.connect([thermometer.port("temperature"), block.port("temperature")]);
        m.connect([block.port("heater_power"), heater.port("power")]);
        m.connect([block.port("heating")]);
        handles.push((air, block));
    }
    m.connect(outside_ports);
    let mut runtime = Runtime::new(m, &registry, sim_dynamics::Integrator::BackwardEuler(sim_runtime::newton())).map_err(|e| FmiError::Binding(e.to_string()))?;
    sim_fmi::bind(&mut runtime, Path::new("/"), &mut Cache::default())?;
    let temperatures = handles.iter().map(|(air, _)| runtime.across_id(air.port("node"))).collect();
    let heaters = handles.iter().map(|(_, block)| runtime.signal_id(block.port("heater_power"))).collect();
    Ok(Rooms { runtime, temperatures, heaters })
}

fn run(rooms: &mut Rooms, seconds: f64, every: f64) -> Vec<Vec<f64>> {
    let mut rows = Vec::new();
    let n = (seconds / every).round() as usize;
    for _ in 0..n {
        rooms.runtime.advance(every, every / 2.0).unwrap();
        let mut row = vec![rooms.runtime.time];
        row.extend(rooms.temperatures.iter().map(|id| rooms.runtime.get(*id)));
        row.extend(rooms.heaters.iter().map(|id| rooms.runtime.get(*id)));
        rows.push(row);
    }
    rows
}

#[test]
fn inspection_shows_ports_units_parameters_and_capabilities() {
    let fmu = Fmu::load(thermostat()).unwrap();
    let s = fmu.summary();
    assert_eq!(s.model_name, "thermostat");
    assert_eq!(s.fmi_version, "3.0");
    assert!(s.unsupported.is_empty(), "{:?}", s.unsupported);
    assert_eq!(s.inputs.iter().map(|v| (v.name.as_str(), v.kind.clone().unwrap())).collect::<Vec<_>>(), [("temperature", "Temperature".to_owned())]);
    assert_eq!(s.outputs.iter().map(|v| (v.name.as_str(), v.kind.clone().unwrap())).collect::<Vec<_>>(), [("heater_power", "HeatFlow".to_owned()), ("heating", "Dimensionless".to_owned())]);
    assert!(s.parameters.iter().any(|p| p.name == "setpoint" && p.unit.as_deref() == Some("K")));
    assert!(s.co_simulation.unwrap().can_serialize_fmu_state);
    let interface = fmu.interface(&BTreeMap::new()).unwrap();
    assert!(!interface.feedthrough, "co-simulation outputs are end-of-step values");
    assert_eq!(interface.outputs[0].min, Some(0.0), "declared ranges become port ranges");
}

#[test]
fn a_thermostat_fmu_regulates_a_room() {
    let mut rooms = rooms(&[294.15], 0.5, |_| BTreeMap::new()).unwrap();
    let rows = run(&mut rooms, 3600.0, 5.0);
    let settled: Vec<&Vec<f64>> = rows.iter().filter(|r| r[0] > 1800.0).collect();
    let (lo, hi) = settled.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), r| (lo.min(r[1]), hi.max(r[1])));
    // Band ±0.5 K; the heater warms the air ~0.025 K/s at most, so one
    // 0.5 s communication step overshoots by at most ~0.0125 K.
    assert!(lo > 294.15 - 0.5 - 0.05 && hi < 294.15 + 0.5 + 0.05, "settled between {lo} and {hi}");
    let on = settled.iter().filter(|r| r[2] > 0.0).count();
    assert!(on > 0 && on < settled.len(), "the heater cycles ({on} of {} samples on)", settled.len());
}

#[test]
fn instances_are_independent() {
    let mut rooms = rooms(&[292.15, 296.15], 0.5, |_| BTreeMap::new()).unwrap();
    let rows = run(&mut rooms, 3600.0, 10.0);
    let last: Vec<&Vec<f64>> = rows.iter().filter(|r| r[0] > 2400.0).collect();
    let mean = |k: usize| last.iter().map(|r| r[k]).sum::<f64>() / last.len() as f64;
    assert!((mean(1) - 292.15).abs() < 0.6, "room 0 regulates its own setpoint: {}", mean(1));
    assert!((mean(2) - 296.15).abs() < 0.6, "room 1 regulates its own setpoint: {}", mean(2));
}

#[test]
fn a_fresh_runtime_reproduces_the_run() {
    let trace = || {
        let mut rooms = rooms(&[294.15], 0.5, |_| BTreeMap::new()).unwrap();
        run(&mut rooms, 900.0, 5.0)
    };
    assert_eq!(trace(), trace());
}

#[test]
fn outputs_apply_one_communication_step_later() {
    // The room starts at 290.15 K, below the band: the initial output
    // (at t = 0, from exitInitializationMode) is already "on".
    let mut rooms = rooms(&[294.15], 0.5, |_| BTreeMap::new()).unwrap();
    assert_eq!(rooms.runtime.time, 0.0);
    rooms.runtime.start_blocks().unwrap();
    assert_eq!(rooms.runtime.get(rooms.heaters[0]), 500.0, "the initial output applies at the first tick");
    // A block that ticks only every 10 s: between ticks the output holds.
    let mut slow = super_slow();
    slow.runtime.advance(9.0, 0.5).unwrap();
    let before = slow.runtime.get(slow.heaters[0]);
    assert_eq!(before, 500.0);
}

fn super_slow() -> Rooms {
    rooms(&[294.15], 10.0, |_| BTreeMap::new()).unwrap()
}

#[test]
fn units_must_match_exactly() {
    let fmu = Fmu::load(thermostat()).unwrap();
    // Declaring the temperature input an angle contradicts the FMU's unit.
    let err = fmu.interface(&BTreeMap::from([("temperature".to_owned(), Q::Angle)])).unwrap_err().to_string();
    assert!(err.contains("`temperature`") && err.contains("units must match exactly"), "{err}");
    // Wiring the (Temperature) block input to an angle sensor fails to compile.
    let registry = sim_runtime::registry();
    let mut m = ModelWorld::default();
    let shaft = m.part(&registry, "shaft", sim_domain_rotational::elements::INERTIA, [("inertia", 1.0)]).unwrap();
    let angle = m.part(&registry, "angle", sim_domain_rotational::elements::ANGLE_SENSOR, []).unwrap();
    m.connect([shaft.port("shaft"), angle.port("shaft")]);
    let block = sim_fmi::add_block(&mut m, "thermostat", &fmu, "thermostat.fmu", &BTreeMap::new(), BlockTiming::periodic(0.5), BTreeMap::new()).unwrap();
    m.connect([angle.port("angle"), block.port("temperature")]);
    m.connect([block.port("heater_power")]);
    m.connect([block.port("heating")]);
    let err = Runtime::new(m, &registry, sim_dynamics::Integrator::BackwardEuler(sim_runtime::newton())).err().unwrap().to_string();
    assert!(err.contains("`thermostat`") && err.contains("Temperature") && err.contains("Angle"), "{err}");
}

/// The thermostat archive with its model description edited.
fn variant(name: &str, edit: impl Fn(String) -> String, keep_binary: bool) -> PathBuf {
    let source = std::fs::read(thermostat()).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(source)).unwrap();
    let out = fixtures().join(format!("{name}.fmu"));
    let mut writer = zip::ZipWriter::new(std::fs::File::create(&out).unwrap());
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).unwrap();
        let entry_name = entry.name().to_owned();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut bytes).unwrap();
        if entry_name.starts_with("binaries/") && !keep_binary {
            continue;
        }
        if entry_name == "modelDescription.xml" {
            bytes = edit(String::from_utf8(bytes).unwrap()).into_bytes();
        }
        writer.start_file(entry_name, zip::write::SimpleFileOptions::default()).unwrap();
        writer.write_all(&bytes).unwrap();
    }
    writer.finish().unwrap();
    out
}

fn unsupported(path: &Path) -> Vec<String> {
    match Fmu::load(path) {
        Err(FmiError::Unsupported { reasons, .. }) => reasons,
        other => panic!("expected the FMU to be refused, got {other:?}"),
    }
}

#[test]
fn everything_outside_the_profile_is_refused_by_name() {
    let fmi2 = variant("fmi2", |x| x.replace("fmiVersion=\"3.0\"", "fmiVersion=\"2.0\""), true);
    assert!(unsupported(&fmi2)[0].contains("only FMI 3.0"));
    let me_only = variant("me-only", |x| x.replace("<CoSimulation", "<ModelExchange"), true);
    assert!(unsupported(&me_only)[0].contains("Model Exchange but not Co-Simulation"), "{:?}", unsupported(&me_only));
    let tool = variant("tool", |x| x.replace("modelIdentifier=\"thermostat\"", "modelIdentifier=\"thermostat\" needsExecutionTool=\"true\""), true);
    assert!(unsupported(&tool).iter().any(|r| r.contains("needsExecutionTool")));
    let clocked = variant("clocked", |x| x.replace("<ModelVariables>", "<ModelVariables>\n<Clock name=\"tick\" valueReference=\"50\" causality=\"input\" intervalVariability=\"constant\"/>"), true);
    assert!(unsupported(&clocked).iter().any(|r| r.contains("clocks (`tick`)")));
    let no_binary = variant("no-binary", |x| x, false);
    assert!(unsupported(&no_binary).iter().any(|r| r.contains("no binary for this platform")), "{:?}", unsupported(&no_binary));

    // Variables a block cannot use: refused when the interface is derived.
    let array = variant("array", |x| x.replace("start=\"293.15\" description=\"Measured room temperature\"/>", "start=\"293.15 293.15\"><Dimension start=\"2\"/></Float64>"), true);
    let err = Fmu::load(&array).unwrap().interface(&BTreeMap::new()).unwrap_err().to_string();
    assert!(err.contains("`temperature` is an array"), "{err}");
    let string = variant("string", |x| x.replace("<ModelVariables>", "<ModelVariables>\n<String name=\"label\" valueReference=\"51\" causality=\"input\"><Start value=\"a\"/></String>"), true);
    let err = Fmu::load(&string).unwrap().interface(&BTreeMap::new()).unwrap_err().to_string();
    assert!(err.contains("`label` is String"), "{err}");
    let structural = variant("structural", |x| x.replace("name=\"band\" valueReference=\"11\" causality=\"parameter\"", "name=\"band\" valueReference=\"11\" causality=\"structuralParameter\""), true);
    let fmu = Fmu::load(&structural).unwrap();
    let interface = fmu.interface(&BTreeMap::new()).unwrap();
    let err = fmu.check("t", &interface, &BTreeMap::from([("band".to_owned(), 2.0)])).unwrap_err().to_string();
    assert!(err.contains("structural parameter"), "{err}");
    let err = fmu.check("t", &interface, &BTreeMap::from([("bandwidth".to_owned(), 2.0)])).unwrap_err().to_string();
    assert!(err.contains("`bandwidth`: the FMU has no variable"), "{err}");
    let mut renamed = interface.clone();
    renamed.inputs[0].name = "temp".into();
    let err = fmu.check("t", &renamed, &BTreeMap::new()).unwrap_err().to_string();
    assert!(err.contains("port `temp`: the FMU has no variable"), "{err}");
}

#[test]
fn an_fmu_instantiable_once_per_process_cannot_be_placed_twice() {
    let once = variant("once", |x| x.replace("canBeInstantiatedOnlyOncePerProcess=\"false\"", "canBeInstantiatedOnlyOncePerProcess=\"true\""), true);
    let fmu = Fmu::load(&once).unwrap();
    let interface = fmu.interface(&BTreeMap::new()).unwrap();
    let binding = fmu.check("a", &interface, &BTreeMap::new()).unwrap();
    let first = sim_fmi::FmuBlock::new(&fmu, "a", interface.clone(), binding.clone()).unwrap();
    let err = sim_fmi::FmuBlock::new(&fmu, "b", interface.clone(), binding.clone()).err().unwrap().to_string();
    assert!(err.contains("only once per process"), "{err}");
    drop(first);
    sim_fmi::FmuBlock::new(&fmu, "c", interface, binding).expect("the slot is free again once the first instance is gone");
}

#[test]
fn an_fmu_error_stops_the_run_and_everything_is_released() {
    let mut rooms = rooms(&[294.15], 0.5, |_| BTreeMap::from([("fail_after".to_owned(), 2.0)])).unwrap();
    let err = rooms.runtime.advance(10.0, 0.25).unwrap_err().to_string();
    assert!(err.contains("`room0/thermostat`") && err.contains("fmi3Error") && err.contains("deliberate failure"), "{err}");
    assert!(err.contains("t=2"), "the step from t = 2 is the one that fails: {err}");
    drop(rooms);
    // A new run after the failed one is unaffected.
    let mut again = rooms_ok();
    again.runtime.advance(10.0, 0.25).unwrap();
}

fn rooms_ok() -> Rooms {
    rooms(&[294.15], 0.5, |_| BTreeMap::new()).unwrap()
}

#[test]
fn a_non_finite_output_is_a_fault() {
    let mut rooms = rooms(&[294.15], 0.5, |_| BTreeMap::from([("nan_after".to_owned(), 1.0)])).unwrap();
    let err = rooms.runtime.advance(10.0, 0.25).unwrap_err().to_string();
    assert!(err.contains("heater_power") && err.contains("NaN"), "{err}");
}

#[test]
fn a_changed_artifact_is_refused() {
    let registry = sim_runtime::registry();
    let fmu = Fmu::load(thermostat()).unwrap();
    let mut m = ModelWorld::default();
    let air = m.part(&registry, "air", sim_domain_thermal::CAPACITANCE, [("heat_capacity", 1.0e3), ("initial.temperature", 290.0)]).unwrap();
    let sensor = m.part(&registry, "t", sim_domain_thermal::TEMPERATURE_SENSOR, []).unwrap();
    let heater = m.part(&registry, "h", sim_domain_thermal::CONTROLLED_HEAT_SOURCE, []).unwrap();
    m.connect([air.port("node"), sensor.port("node"), heater.port("node")]);
    let block = sim_fmi::add_block(&mut m, "thermostat", &fmu, &thermostat().display().to_string(), &BTreeMap::new(), BlockTiming::periodic(0.5), BTreeMap::new()).unwrap();
    m.connect([sensor.port("temperature"), block.port("temperature")]);
    m.connect([block.port("heater_power"), heater.port("power")]);
    m.connect([block.port("heating")]);
    if let sim_core::ImplementationRef::Fmi3 { sha256, .. } = &mut m.blocks[0].implementation {
        *sha256 = "0".repeat(64);
    }
    let mut runtime = Runtime::new(m, &registry, sim_dynamics::Integrator::BackwardEuler(sim_runtime::newton())).unwrap();
    let err = sim_fmi::bind(&mut runtime, Path::new("/"), &mut Cache::default()).unwrap_err().to_string();
    assert!(err.contains("not the FMU the model was built with"), "{err}");
}

#[test]
fn checkpoints_include_the_fmu_state() {
    let mut rooms = rooms(&[294.15], 0.5, |_| BTreeMap::new()).unwrap();
    run(&mut rooms, 600.0, 5.0);
    let saved = rooms.runtime.snapshot().expect("the thermostat serializes its state");
    let a = run(&mut rooms, 300.0, 5.0);
    rooms.runtime.restore(&saved).unwrap();
    let b = run(&mut rooms, 300.0, 5.0);
    assert_eq!(a, b);
}

#[test]
fn packing_the_same_sources_gives_the_same_archive() {
    let examples = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/fmi/thermostat");
    let a = sim_fmi::pack::pack(&examples, &fixtures().join("again-a.fmu")).unwrap();
    let b = sim_fmi::pack::pack(&examples, &fixtures().join("again-b.fmu")).unwrap();
    assert_eq!(a, b, "same sources, same compiler: same SHA-256");
}
