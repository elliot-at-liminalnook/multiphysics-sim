//! Binding a model's blocks to their implementations before a run, the one
//! path every host uses (system sessions, headless runs, studies, tests):
//! FMI 3 FMUs are imported and instantiated per block (`sim_fmi`), relative
//! paths resolved against the system file's directory; a host block needs
//! the host that runs the system to supply it, so a generic host refuses it.
use sim_compile::Runtime;
use sim_core::ImplementationRef;
use std::path::Path;

/// Host implementations by name (`ImplementationRef::Host`): each builds
/// the implementation for one block that names it.
pub type Hosts<'a> = std::collections::BTreeMap<String, Box<dyn FnMut(&sim_core::BlockDecl) -> Result<Box<dyn sim_core::BlockImplementation>, String> + 'a>>;

/// Bind every block of `runtime`. `base` is the system file's directory
/// (None: the model came from no file, so a relative FMU path is refused).
pub fn bind(runtime: &mut Runtime, base: Option<&Path>) -> Result<(), String> {
    bind_with(runtime, base, &mut Hosts::new())
}

/// [`bind`] where the host supplies the implementations in `hosts` (a test
/// bench's stimulus, a teleoperation source). A host block nothing supplies
/// is refused by name.
pub fn bind_with(runtime: &mut Runtime, base: Option<&Path>, hosts: &mut Hosts) -> Result<(), String> {
    let mut missing = Vec::new();
    for decl in runtime.model.blocks.clone() {
        let ImplementationRef::Host { name } = &decl.implementation else { continue };
        match hosts.get_mut(name) {
            Some(make) => {
                let implementation = make(&decl).map_err(|e| format!("block `{}` (host implementation `{name}`): {e}", decl.name))?;
                runtime.bind_block(&decl.name, implementation).map_err(|e| e.to_string())?;
            }
            None => missing.push(format!("`{}` (host implementation `{name}`)", decl.name)),
        }
    }
    if !missing.is_empty() {
        return Err(format!("this host supplies no implementation for {}: run the system where they are provided, or replace them with FMU blocks", missing.join(", ")));
    }
    if !runtime.model.blocks.iter().any(|b| matches!(b.implementation, ImplementationRef::Fmi3 { .. })) {
        return Ok(());
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = base;
        Err("FMU blocks run in the native viewer and headless tools, not in the browser".into())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let relative = runtime.model.blocks.iter().find_map(|b| match &b.implementation {
            ImplementationRef::Fmi3 { path, .. } if !Path::new(path).is_absolute() && base.is_none() => Some(b.name.clone()),
            _ => None,
        });
        if let Some(block) = relative {
            return Err(format!("block `{block}` names its FMU relative to the system file, but this model came from no file"));
        }
        sim_fmi::bind(runtime, base.unwrap_or(Path::new("/")), &mut sim_fmi::Cache::default()).map(|_| ()).map_err(|e| e.to_string())
    }
}

/// A block instance for the FMU at `path` (relative to the system file's
/// directory `base`): its interface read from the FMU (ports typed by
/// `kinds` where given, else by unit), its SHA-256 recorded, `parameters`
/// checked against its parameter variables. The same instance the Build
/// mode actions and REST `system_add_fmu` add.
#[cfg(not(target_arch = "wasm32"))]
pub fn fmu_instance(base: &Path, path: &str, timing: sim_core::BlockTiming, kinds: &std::collections::BTreeMap<String, sim_core::QuantityKind>, parameters: &std::collections::BTreeMap<String, f64>) -> Result<sim_system::InstanceSpec, String> {
    sim_system::check_relative_path(path)?;
    timing.validate()?;
    let fmu = sim_fmi::Fmu::load(base.join(path)).map_err(|e| e.to_string())?;
    let interface = fmu.interface(kinds).map_err(|e| e.to_string())?;
    fmu.check("new block", &interface, parameters).map_err(|e| e.to_string())?;
    let mut spec = sim_system::InstanceSpec::block(sim_system::BlockSource::Fmu { path: path.to_owned(), sha256: fmu.sha256.clone() }, interface, timing);
    for (name, value) in parameters {
        spec = spec.with(name, *value);
    }
    spec.label = fmu.description.model_name.clone();
    Ok(spec)
}

/// A generated robot instance from the `.simrobot.json` at `source`
/// (relative to `base`), its port signature recorded. `parameters` are the
/// robot generator's (`driver_control`: duty inputs per motor instead of
/// servo targets per joint; `own_supply`, `own_ambient`: the model's own
/// supply and ambient instead of boundary ports).
#[cfg(not(target_arch = "wasm32"))]
pub fn robot_instance_with(registry: &sim_core::BehaviorRegistry, base: &Path, source: &str, parameters: &std::collections::BTreeMap<String, f64>) -> Result<sim_system::InstanceSpec, String> {
    let ports = crate::robot_generator::generators(base).ports(registry, crate::robot_generator::NAME, source, parameters)?;
    let mut spec = sim_system::InstanceSpec::generated(crate::robot_generator::NAME, source, ports);
    for (name, value) in parameters {
        spec = spec.with(name, *value);
    }
    Ok(spec)
}

/// [`robot_instance_with`] with boundary supply and ambient.
#[cfg(not(target_arch = "wasm32"))]
pub fn robot_instance(registry: &sim_core::BehaviorRegistry, base: &Path, source: &str, driver_control: bool) -> Result<sim_system::InstanceSpec, String> {
    let parameters: std::collections::BTreeMap<String, f64> = if driver_control { [("driver_control".to_owned(), 1.0)].into() } else { Default::default() };
    robot_instance_with(registry, base, source, &parameters)
}
