//! The composition examples (docs/architecture/composition.md), authored
//! through the same command path as Build mode and REST (`SystemStore::apply`):
//!
//! - `thermostat`: a room (air, wall, outside, heater, thermometer) regulated
//!   by the thermostat FMU (examples/fmi/thermostat).
//! - `rover`: the wheeled robot as a generated assembly, its motor bus fed
//!   by its own battery pack, its motors and mounts shedding heat into an
//!   enclosure vented to the outside, and one joint-controller FMU per drive
//!   wheel (examples/fmi/joint-controller), two independent instances.
//!
//! Each builds its FMUs from source into `<dir>/fmus` (`sim_fmi::pack`),
//! writes `<dir>/<name>.system.json` and returns its path. Nothing here runs
//! a model: callers run the file like any other (`system_builder::simulate_at`,
//! Build mode's Run).
use crate::system_blocks::{fmu_instance, robot_instance};
use sim_core::{BehaviorRegistry, BlockTiming};
use sim_system::{Command, InstanceSpec, IntegratorChoice, RunSettings, SystemDocument, SystemStore, Terminal};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Build the fixture FMU `model` (a directory under examples/fmi) into
/// `<dir>/fmus/<file>`; returns its path relative to `dir`.
pub fn pack_fmu(dir: &Path, model: &str, file: &str) -> Result<String, String> {
    let relative = format!("fmus/{file}");
    sim_fmi::pack::pack(&repository().join("examples/fmi").join(model), &dir.join(&relative))?;
    Ok(relative)
}

fn connect(terminals: &[(&str, &str)]) -> Command {
    Command::Connect { at: String::new(), terminals: terminals.iter().map(|(i, p)| Terminal::port(i, p)).collect(), label: String::new() }
}

fn add(name: &str, instance: InstanceSpec) -> Command {
    Command::AddInstance { at: String::new(), name: name.into(), instance }
}

/// Instances laid out in a row, in the order added (display only).
fn laid_out(mut commands: Vec<Command>) -> Vec<Command> {
    let mut slot = 0.0_f32;
    for command in &mut commands {
        if let Command::AddInstance { instance, .. } = command {
            instance.placement.position = [0.25 * slot, 0.0, 0.0];
            slot += 1.0;
        }
    }
    commands
}

fn create(path: &Path, title: &str, registry: &BehaviorRegistry, commands: Vec<Command>) -> Result<PathBuf, String> {
    let _ = std::fs::remove_file(path);
    let store = SystemStore::create(path, &SystemDocument::new(title)).map_err(|e| e.to_string())?;
    store.apply(registry, &format!("Build {title}"), &laid_out(commands), None).map_err(|e| e.to_string())?;
    Ok(path.to_path_buf())
}

/// The thermostat room in `dir` (see the module documentation).
pub fn thermostat(dir: &Path, registry: &BehaviorRegistry) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let fmu = pack_fmu(dir, "thermostat", "thermostat.fmu")?;
    let block = fmu_instance(dir, &fmu, BlockTiming::periodic(0.5), &BTreeMap::new(), &BTreeMap::from([("setpoint".to_owned(), 294.15)]))?;
    let commands = vec![
        Command::SetRunSettings { run: Some(RunSettings { integrator: IntegratorChoice::BackwardEuler, interval: 0.5, absolute_tolerance: None, relative_tolerance: None, max_iterations: None, rationale: "Thermal time constants of minutes; the thermostat's communication step is 0.5 s".into() }) },
        add("outside", InstanceSpec::element(sim_domain_thermal::AMBIENT).with("temperature", 278.15)),
        add("air", InstanceSpec::element(sim_domain_thermal::CAPACITANCE).with("heat_capacity", 2.0e4).with("initial.temperature", 290.15)),
        add("wall", InstanceSpec::element(sim_domain_thermal::CONDUCTANCE).with("conductance", 8.0)),
        add("heater", InstanceSpec::element(sim_domain_thermal::CONTROLLED_HEAT_SOURCE)),
        add("thermometer", InstanceSpec::element(sim_domain_thermal::TEMPERATURE_SENSOR)),
        add("thermostat", block),
        connect(&[("air", "node"), ("wall", "a"), ("heater", "node"), ("thermometer", "node")]),
        connect(&[("wall", "b"), ("outside", "node")]),
        connect(&[("thermometer", "temperature"), ("thermostat", "temperature")]),
        connect(&[("thermostat", "heater_power"), ("heater", "power")]),
    ];
    create(&dir.join("thermostat.system.json"), "Thermostat room", registry, commands)
}

/// The rover system in `dir` (see the module documentation).
pub fn rover(dir: &Path, registry: &BehaviorRegistry) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    std::fs::copy(repository().join("examples/wheeled-robot/baseline/robot.simrobot.json"), dir.join("rover.simrobot.json")).map_err(|e| e.to_string())?;
    let fmu = pack_fmu(dir, "joint-controller", "joint_controller.fmu")?;
    let controller = |amplitude: f64, frequency: f64| -> Result<InstanceSpec, String> {
        let parameters = BTreeMap::from([("amplitude".to_owned(), amplitude), ("frequency".to_owned(), frequency), ("ki".to_owned(), 0.5)]);
        fmu_instance(dir, &fmu, BlockTiming::periodic(0.02), &BTreeMap::new(), &parameters)
    };
    let commands = vec![
        Command::SetRunSettings { run: Some(RunSettings { integrator: IntegratorChoice::BackwardEuler, interval: 5.0e-4, absolute_tolerance: Some(1.0e-10), relative_tolerance: Some(1.0e-8), max_iterations: Some(40), rationale: "The robot assembly's own step and Newton settings (Robot mode's defaults)".into() }) },
        add("rover", robot_instance(registry, dir, "rover.simrobot.json", false)?),
        add("battery", InstanceSpec::element(sim_domain_robot::BATTERY).with("cells", 5.0).with("nominal_voltage", 6.0).with("internal_resistance", 0.15).with("capacity_ah", 0.5).with("initial_soc", 1.0)),
        add("ground", InstanceSpec::element(sim_domain_electrical::elements::GROUND)),
        // At the robot model's ambient (20 °C), where its windings, cases and mounts start.
        add("enclosure", InstanceSpec::element(sim_domain_thermal::CAPACITANCE).with("heat_capacity", 200.0).with("initial.temperature", 293.15)),
        add("vent", InstanceSpec::element(sim_domain_thermal::CONDUCTANCE).with("conductance", 2.0)),
        add("outside", InstanceSpec::element(sim_domain_thermal::AMBIENT).with("temperature", 293.15)),
        add("left_controller", controller(0.6, 0.5)?),
        add("right_controller", controller(0.4, 0.25)?),
        connect(&[("battery", "p"), ("rover", "supply_p")]),
        connect(&[("battery", "n"), ("rover", "supply_n"), ("ground", "pin")]),
        connect(&[("rover", "ambient"), ("enclosure", "node"), ("vent", "a")]),
        connect(&[("vent", "b"), ("outside", "node")]),
        connect(&[("rover", "left axle.angle"), ("left_controller", "angle")]),
        connect(&[("left_controller", "target"), ("rover", "left axle.target")]),
        connect(&[("rover", "right axle.angle"), ("right_controller", "angle")]),
        connect(&[("right_controller", "target"), ("rover", "right axle.target")]),
    ];
    create(&dir.join("rover.system.json"), "Rover with battery, enclosure and two controller FMUs", registry, commands)
}
