//! The `robot` generator: a system instance generated from a
//! `.simrobot.json` is the same assembly Robot mode runs
//! (`physical::assemble`), composed into the system with its motor bus and
//! thermal environment as boundary ports and its controller contract's
//! channels as typed signals (docs/architecture/composition.md, "Robots"):
//!
//! - `supply_p`, `supply_n`: the motor bus (electrical), for a battery or
//!   supply (with `own_supply = 1` the robot keeps the supply its model
//!   defines, as in Robot mode, and offers no bus);
//! - `ambient`: the thermal environment the motors and mounts shed heat to
//!   (with `own_ambient = 1`, the model's own fixed ambient instead);
//! - inputs `<joint>.target` (servo setpoint, rad) or, with
//!   `driver_control = 1`, `<motor>.duty` (H-bridge duty);
//! - outputs `<joint>.angle`, `<joint>.speed` (encoders, tachometers),
//!   `imu.*`, and with driver control `<motor>.current|torque|speed`.
use crate::physical::{assemble, AssemblyOptions, BuildOptions};
use sim_domain_robot::PhysicalModel;
use sim_core::{BehaviorRegistry, ModelWorld, PortSchema};
use sim_system::{Generated, Generator};
use std::collections::BTreeMap;
use std::path::Path;

pub const NAME: &str = "robot";

#[derive(Default)]
pub struct RobotGenerator;

/// The assembly a `robot` instance was generated as, for the host code
/// that measures it in the compiled model (`acceptance`): taken once.
pub type Handle = std::sync::Mutex<Option<crate::physical::RobotAssembly>>;

/// How the instance's parameters ask for the robot to be built.
pub fn options(prefix: &str, parameters: &BTreeMap<String, f64>) -> Result<(BuildOptions, AssemblyOptions), String> {
    let mut opts = BuildOptions::default();
    let mut how = AssemblyOptions { prefix: prefix.to_owned(), external_supply: true, external_ambient: true };
    for (name, value) in parameters {
        match name.as_str() {
            "driver_control" => opts.driver_control = *value != 0.0,
            "own_supply" => how.external_supply = *value == 0.0,
            "own_ambient" => how.external_ambient = *value == 0.0,
            other => return Err(format!("the robot generator has no parameter `{other}` (it takes `driver_control`, `own_supply`, `own_ambient`)")),
        }
    }
    Ok((opts, how))
}

impl Generator for RobotGenerator {
    fn ports(&self, registry: &BehaviorRegistry, source: &Path, parameters: &BTreeMap<String, f64>) -> Result<BTreeMap<String, PortSchema>, String> {
        let model = PhysicalModel::load(&source.display().to_string())?;
        let mut scratch = ModelWorld::default();
        let (opts, how) = options("", parameters)?;
        let assembly = assemble(&mut scratch, registry, model, &opts, &how)?;
        Ok(assembly.boundary.into_iter().map(|(name, (schema, _))| (name, schema)).collect())
    }

    fn generate(&self, world: &mut ModelWorld, registry: &BehaviorRegistry, prefix: &str, source: &Path, parameters: &BTreeMap<String, f64>) -> Result<Generated, String> {
        let model = PhysicalModel::load(&source.display().to_string())?;
        let (opts, how) = options(prefix, parameters)?;
        let assembly = assemble(world, registry, model, &opts, &how)?;
        let boundary = assembly.boundary.iter().map(|(name, (_, ports))| (name.clone(), ports.clone())).collect();
        let warnings = assembly.warnings.clone();
        Ok(Generated { boundary, warnings, detail: Some(std::sync::Arc::new(Handle::new(Some(assembly)))) })
    }
}

/// The generators every host offers, resolving against `base` (the system
/// file's directory).
pub fn generators(base: &Path) -> sim_system::Generators {
    sim_system::Generators::new(base).with(NAME, std::sync::Arc::new(RobotGenerator))
}
