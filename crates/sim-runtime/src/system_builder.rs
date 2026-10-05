//! Hierarchical system files on the shared runtime: compile a `sim.system/2`
//! document into the description, physical presentation, animation bindings
//! and live capture that both viewers already use, check that it compiles,
//! and run it headlessly. Nothing here duplicates physics; the document is
//! flattened and handed to the one compiler.
use crate::system_session::{ModelSource, SessionConfig, SystemSession};
use crate::system_launch::{Launch, SourceBinding};
use serde::Serialize;
use sim_core::BehaviorRegistry;
use sim_inspect::animation::AnimationDescription;
use sim_inspect::spatial::SpatialDescription;
use sim_inspect::{ObservationLocation, SystemDescription};
use sim_system::{Finding, Flattened, SystemDocument};
use std::path::{Path, PathBuf};

pub struct Compiled {
    pub flat: Flattened,
    pub description: SystemDescription,
    /// Absent when the system has no parts yet.
    pub spatial: Option<SpatialDescription>,
    pub animation: Option<AnimationDescription>,
    pub launch: Launch,
}

/// Builder default: implicit midpoint with tolerances suited to volt/amp/kelvin
/// scale circuits (absolute 1e-8 in each equation's units, relative 1e-7).
pub fn default_config() -> SessionConfig {
    let newton = sim_solve::NewtonConfig { absolute_tolerance: 1e-8, relative_tolerance: 1e-7, max_iterations: 80, ..Default::default() };
    // System documents schedule switching events (PWM edges, pulses) on step
    // ends, so every host runs them with the clock snapped to the step grid.
    SessionConfig { interval: 1e-4, integrator: sim_dynamics::Integrator::ImplicitMidpoint(newton), seed: 1, grid_snapping: true }
}

/// The settings a document records, else the builder default.
pub fn config_for(document: &SystemDocument) -> SessionConfig {
    let mut config = default_config();
    let Some(run) = &document.run else { return config };
    config.interval = run.interval;
    let sim_dynamics::Integrator::ImplicitMidpoint(mut newton) = config.integrator else { return config };
    if let Some(v) = run.absolute_tolerance {
        newton.absolute_tolerance = v;
    }
    if let Some(v) = run.relative_tolerance {
        newton.relative_tolerance = v;
    }
    if let Some(v) = run.max_iterations {
        newton.max_iterations = v;
    }
    config.integrator = match run.integrator {
        sim_system::IntegratorChoice::ImplicitMidpoint => sim_dynamics::Integrator::ImplicitMidpoint(newton),
        sim_system::IntegratorChoice::BackwardEuler => sim_dynamics::Integrator::BackwardEuler(newton),
    };
    config
}

/// Flatten and describe a document that came from no file: generated
/// instances and relative FMU paths are refused (see [`compile_at`]).
/// Does not construct the numerical runtime.
pub fn compile(document: &SystemDocument, registry: &BehaviorRegistry, config: SessionConfig) -> Result<Compiled, String> {
    compile_inner(document, registry, config, None)
}

/// Flatten and describe the document stored in directory `base`: generated
/// instances (robots) are built by the host's generators from files next
/// to it, and the model's FMU paths resolve against it.
pub fn compile_at(document: &SystemDocument, registry: &BehaviorRegistry, config: SessionConfig, base: &Path) -> Result<Compiled, String> {
    compile_inner(document, registry, config, Some(base))
}

/// Flatten a document, with the host's generators when it has a directory.
pub fn flatten(document: &SystemDocument, registry: &BehaviorRegistry, base: Option<&Path>) -> Result<Flattened, String> {
    match base {
        #[cfg(not(target_arch = "wasm32"))]
        Some(base) => sim_system::flatten_with(document, registry, &crate::robot_generator::generators(base)),
        _ => sim_system::flatten(document, registry),
    }
    .map_err(|e| e.to_string())
}

/// The model of a compiled document as a session source (blocks bound at every build).
pub fn source(compiled: &Compiled, registry: &BehaviorRegistry, document: &SystemDocument) -> ModelSource {
    ModelSource {
        model: compiled.flat.model.clone(),
        registry: registry.clone(),
        identities: compiled.flat.identities.clone(),
        source_hash: compiled.flat.source_hash.clone(),
        revision: document.revision.max(1),
        base: compiled.launch.base.as_ref().map(PathBuf::from),
    }
}

fn compile_inner(document: &SystemDocument, registry: &BehaviorRegistry, config: SessionConfig, base: Option<&Path>) -> Result<Compiled, String> {
    let flat = flatten(document, registry, base)?;
    let description = sim_inspect::model::describe(&flat.model, registry, &flat.source_hash, document.revision.max(1), &flat.identities)
        .map_err(|e| e.to_string())?
        .description;
    let spatial = (!flat.parts.is_empty()).then(|| flat.spatial(&description.id, &document.title));
    if let Some(s) = &spatial {
        s.validate(&description).map_err(|e| e.to_string())?;
    }
    let animation = spatial.as_ref().and_then(|s| animation(&description, s));
    let mut launch = Launch {
        version: 1,
        run_id: format!("system-{}-r{}", &flat.source_hash[..12], document.revision),
        model: flat.model.clone(),
        source_hash: flat.source_hash.clone(),
        revision: document.revision.max(1),
        config,
        binding: None,
        base: base.map(|b| b.display().to_string()),
    };
    launch.binding = Some(SourceBinding { model_hash: launch.model_hash()?, description_id: description.id.clone(), identities: flat.identities.clone() });
    Ok(Compiled { flat, description, spatial, animation, launch })
}

/// Display bindings for a compiled system; see [`crate::system_display`].
pub fn animation(description: &SystemDescription, spatial: &SpatialDescription) -> Option<AnimationDescription> {
    crate::system_display::animation(description, spatial)
}

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub revision: u64,
    pub content_hash: String,
    pub components: usize,
    pub nets: usize,
    pub subsystems: usize,
    pub observables: usize,
    pub findings: Vec<Finding>,
    /// `None` when the numerical runtime constructs successfully.
    pub compile_error: Option<String>,
}

/// Validate, flatten, describe and construct the numerical runtime.
pub fn check(document: &SystemDocument, registry: &BehaviorRegistry) -> Result<Check, String> {
    check_inner(document, registry, None)
}

/// [`check`] for the document stored in `base` (generated instances built,
/// FMU blocks imported and bound).
pub fn check_at(document: &SystemDocument, registry: &BehaviorRegistry, base: &Path) -> Result<Check, String> {
    check_inner(document, registry, Some(base))
}

fn check_inner(document: &SystemDocument, registry: &BehaviorRegistry, base: Option<&Path>) -> Result<Check, String> {
    let compiled = compile_inner(document, registry, default_config(), base)?;
    let compile_error = source(&compiled, registry, document).build(&default_config()).err().map(|e| locate(&compiled.flat, e));
    Ok(Check {
        revision: document.revision,
        content_hash: document.content_hash(),
        components: compiled.description.components.len(),
        nets: compiled.description.nets.len(),
        subsystems: compiled.description.groups.len(),
        observables: compiled.description.observables.len(),
        findings: compiled.flat.findings,
        compile_error,
    })
}

/// Replace internal behavior keys in compiler messages with instance paths.
pub fn locate(flat: &Flattened, mut message: String) -> String {
    for (path, id) in &flat.components {
        message = message.replace(&format!("behavior {id:?}"), &format!("`{path}`")).replace(&format!("{id:?}"), &format!("`{path}`"));
    }
    message
}

/// Files the viewers open: description, spatial, animation and live capture.
pub struct Bundle {
    pub description: PathBuf,
    pub spatial: Option<PathBuf>,
    pub animation: Option<PathBuf>,
    pub live: PathBuf,
}

pub fn write_bundle(compiled: &Compiled, directory: &Path, stem: &str) -> Result<Bundle, String> {
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    fn write<T: Serialize>(directory: &Path, name: String, value: &T) -> Result<PathBuf, String> {
        let path = directory.join(name);
        let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
        sim_system::store::write_atomic(&path, &bytes).map_err(|e| e.to_string())?;
        Ok(path)
    }
    Ok(Bundle {
        description: write(directory, format!("{stem}.description.json"), &compiled.description)?,
        spatial: compiled.spatial.as_ref().map(|s| write(directory, format!("{stem}.spatial.json"), s)).transpose()?,
        animation: compiled.animation.as_ref().map(|a| write(directory, format!("{stem}.animation.json"), a)).transpose()?,
        live: write(directory, format!("{stem}.live.json"), &compiled.launch)?,
    })
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct Series {
    pub observable: String,
    pub label: String,
    pub unit: String,
    pub times: Vec<f64>,
    pub values: Vec<f64>,
}

/// Readable observable key: `path.port.lane`, `path.port` or `path.state`.
pub fn observable_key(description: &SystemDescription, id: &str) -> String {
    let Some(o) = description.observables.get(id) else { return id.to_string() };
    let port = |p: &str| description.ports.get(p).map(|p| format!("{}.{}", p.component, p.name)).unwrap_or_else(|| p.to_string());
    match &o.location {
        ObservationLocation::Across { port: p, lane } | ObservationLocation::Through { port: p, lane } => format!("{}.{lane}", port(p)),
        ObservationLocation::Signal { port: p } => port(p),
        ObservationLocation::State { component, state } => format!("{component}.{state}"),
        ObservationLocation::Diagnostic { component, name } => format!("{}.{name}", component.clone().unwrap_or_default()),
    }
}

/// Run headlessly through the same session the live viewers use and return
/// the selected observables. `select` matches observable IDs or labels by
/// substring; empty selects everything available.
pub fn simulate(document: &SystemDocument, registry: &BehaviorRegistry, duration: f64, config: SessionConfig, select: &[String]) -> Result<Vec<Series>, String> {
    simulate_cancellable(document, registry, duration, config, select, None)
}

/// [`simulate`] for the document stored in `base`.
pub fn simulate_at(document: &SystemDocument, registry: &BehaviorRegistry, base: &Path, duration: f64, config: SessionConfig, select: &[String]) -> Result<Vec<Series>, String> {
    run_recorded(document, registry, Some(base), duration, config, select, None)
}

/// Error returned by [`simulate_cancellable`] when `cancel` was raised.
pub const CANCELLED: &str = "cancelled";

/// [`simulate`] that stops between steps once `cancel` is set, returning
/// [`CANCELLED`].
pub fn simulate_cancellable(document: &SystemDocument, registry: &BehaviorRegistry, duration: f64, config: SessionConfig, select: &[String], cancel: Option<&std::sync::atomic::AtomicBool>) -> Result<Vec<Series>, String> {
    run_recorded(document, registry, None, duration, config, select, cancel)
}

/// [`simulate_cancellable`] for the document stored in `base`.
pub fn simulate_cancellable_at(document: &SystemDocument, registry: &BehaviorRegistry, base: &Path, duration: f64, config: SessionConfig, select: &[String], cancel: Option<&std::sync::atomic::AtomicBool>) -> Result<Vec<Series>, String> {
    run_recorded(document, registry, Some(base), duration, config, select, cancel)
}

fn run_recorded(document: &SystemDocument, registry: &BehaviorRegistry, base: Option<&Path>, duration: f64, config: SessionConfig, select: &[String], cancel: Option<&std::sync::atomic::AtomicBool>) -> Result<Vec<Series>, String> {
    let compiled = compile_inner(document, registry, config.clone(), base)?;
    let source = source(&compiled, registry, document);
    let flat_for_errors = flatten(document, registry, base)?;
    let mut session = SystemSession::new(compiled.launch.run_id.clone(), config.clone(), move |c| source.build(c)).map_err(|e| locate(&flat_for_errors, e))?;
    let description = session.description().clone();
    let chosen: Vec<String> = description
        .observables
        .iter()
        .filter(|(_, o)| o.availability == sim_inspect::Availability::Available)
        .filter(|(id, o)| {
            let key = observable_key(&description, id);
            select.is_empty() || select.iter().any(|s| key.contains(s.as_str()) || id.contains(s.as_str()) || o.label.contains(s.as_str()))
        })
        .map(|(id, _)| id.clone())
        .collect();
    if chosen.is_empty() {
        return Err("no available observable matches the selection".into());
    }
    let steps = (duration / config.interval).round() as usize;
    session.begin_recording(chosen.clone(), steps + 2)?;
    session.execute(crate::system_session::Command::Start)?;
    while session.status().time + 0.5 * config.interval < duration {
        if cancel.is_some_and(|c| c.load(std::sync::atomic::Ordering::Relaxed)) {
            return Err(CANCELLED.into());
        }
        session.tick().map_err(|e| locate(&flat_for_errors, e))?;
        if session.status().phase == sim_inspect::live::Phase::Failed {
            return Err(session.status().message.clone().unwrap_or_else(|| "run failed".into()));
        }
    }
    let recording = session.take_recording().ok_or("no recording")?;
    Ok(chosen
        .iter()
        .map(|id| {
            let o = &description.observables[id];
            let mut times = Vec::new();
            let mut values = Vec::new();
            for frame in &recording.frames {
                match frame.values.get(id) {
                    Some(sim_inspect::SampleValue::Committed { value, sample_time }) | Some(sim_inspect::SampleValue::AcceptedStage { value, sample_time, .. }) => {
                        times.push(*sample_time);
                        values.push(*value);
                    }
                    _ => {}
                }
            }
            Series { observable: id.clone(), label: observable_key(&description, id), unit: sim_inspect::plot::unit(&description, o).to_string(), times, values }
        })
        .collect())
}
