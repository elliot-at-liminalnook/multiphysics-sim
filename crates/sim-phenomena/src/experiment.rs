//! Captured experiment runner shared by the desktop worker and the CLI.
use crate::scenarios::cad_physical::{BuildOptions, PhysicalRobot};
use crate::world::{newton, registry};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sim_compile::Runtime;
use sim_core::{Contract, Coupler, CouplerError, ModelWorld, PortSchema, StateId};
use sim_dynamics::Integrator;
use sim_script::{RhaiController, Sources};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Default, Serialize)]
struct FlexBoundaryTrace {
    id: Option<String>,
    name: String,
    point_m: Vec<[f64; 3]>,
    displacement_m: Vec<[f64; 3]>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    #[serde(default = "seconds")]
    pub seconds: f64,
    #[serde(default = "step")]
    pub step: f64,
    #[serde(default = "sample")]
    pub sample: f64,
    #[serde(default)]
    pub flex: bool,
    #[serde(default)]
    pub contact: bool,
    #[serde(default)]
    pub planar: bool,
    /// Enable captured CAD sensor noise and bias walk. Explicit scripted
    /// components remain controlled by their own declared parameters.
    #[serde(default)]
    pub noise: bool,
}
fn seconds() -> f64 {
    3.2
}
fn step() -> f64 {
    0.0005
}
fn sample() -> f64 {
    0.01
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            seconds: seconds(),
            step: step(),
            sample: sample(),
            flex: false,
            contact: false,
            planar: false,
            noise: false,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Controller {
    pub language: String,
    #[serde(default)]
    pub sources: Option<Sources>,
    #[serde(default = "object")]
    pub parameters: Value,
    #[serde(default)]
    pub command: Vec<String>,
    /// Original immutable source/artifact bundle, materialized by the worker.
    #[serde(default)]
    pub process: Option<Value>,
    #[serde(default)]
    pub directory: Option<String>,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
    #[serde(default)]
    pub seam: Option<String>,
    #[serde(default)]
    pub interface: ControlInterface,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlInterface {
    #[default]
    PositionTarget,
    DriverDuty,
}
fn object() -> Value {
    json!({})
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Specification {
    pub version: u32,
    pub run_id: String,
    /// Compile and open the controller contract without stepping the plant.
    #[serde(default)]
    pub preflight: bool,
    #[serde(default)]
    pub seed: u64,
    #[serde(default = "profile")]
    pub profile: String,
    pub system: Sources,
    #[serde(default = "object")]
    pub parameters: Value,
    /// Captured, already-derived CAD assemblies keyed by script alias.
    #[serde(default)]
    pub cad: BTreeMap<String, Value>,
    #[serde(default)]
    pub controller: Option<Controller>,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default = "object")]
    pub provenance: Value,
}
fn profile() -> String {
    "quick_check".into()
}

struct RecordingController {
    inner: Box<dyn Coupler>,
    contract: Option<Contract>,
    samples: Arc<Mutex<Vec<Value>>>,
    interface: ControlInterface,
}
impl Coupler for RecordingController {
    fn open(&mut self, c: &Contract) -> Result<(), CouplerError> {
        self.inner.open(c)?;
        self.contract = Some(c.clone());
        Ok(())
    }
    fn sample(&mut self, t: f64, s: &[f64], a: &mut [f64]) -> Result<(), CouplerError> {
        self.inner.sample(t, s, a)?;
        if self.interface == ControlInterface::DriverDuty
            && a.iter().any(|v| !v.is_finite() || v.abs() > 1.)
        {
            return Err(CouplerError::Malformed(
                "driver_duty commands must be finite and within [-1, 1]".into(),
            ));
        }
        let c = self.contract.as_ref().unwrap();
        self.samples.lock().unwrap().push(json!({"t":t,
            "sensors":c.sensors.iter().zip(s).map(|(c,v)|(c.name.clone(),json!(v))).collect::<BTreeMap<_,_>>(),
            "commands":c.actuators.iter().zip(a.iter()).map(|(c,v)|(c.name.clone(),json!(v))).collect::<BTreeMap<_,_>>()
        }));
        Ok(())
    }
    fn close(&mut self) {
        self.inner.close()
    }
}
fn controller(
    spec: &Controller,
    samples: Arc<Mutex<Vec<Value>>>,
    seed: u64,
) -> Result<Box<dyn Coupler>, String> {
    let inner: Box<dyn Coupler> = match spec.language.as_str() {
        "rhai" => Box::new(
            RhaiController::with_seed(
                spec.sources
                    .clone()
                    .ok_or("Rhai controller requires captured sources")?,
                sim_script::parameter_map(&spec.parameters).map_err(|e| e.to_string())?,
                seed,
            )
            .map_err(|e| e.to_string())?,
        ),
        "process" => {
            let (exe, args) = spec
                .command
                .split_first()
                .ok_or("controller command is empty")?;
            let mut command = std::process::Command::new(exe);
            command.args(args);
            if let Some(directory) = &spec.directory {
                command.current_dir(directory);
            }
            if spec.process.is_some() {
                command.env_clear();
            }
            command.envs(&spec.environment);
            Box::new(sim_couple::FrameCoupler::spawn_command(command).map_err(|e| e.to_string())?)
        }
        other => return Err(format!("unsupported controller language `{other}`")),
    };
    Ok(Box::new(RecordingController {
        inner,
        contract: None,
        samples,
        interface: spec.interface,
    }))
}

fn write_json(path: &Path, data: &Value) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    let mut f = File::create(&tmp).map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut f, data).map_err(|e| e.to_string())?;
    f.flush().map_err(|e| e.to_string())?;
    std::fs::rename(tmp, path).map_err(|e| e.to_string())
}

fn partial(
    output: &Path,
    spec: &Specification,
    mut result: Value,
    samples: &Arc<Mutex<Vec<Value>>>,
    error: &str,
) -> Result<(), String> {
    result["run_id"] = json!(spec.run_id);
    result["provenance"] = spec.provenance.clone();
    result["partial"] = json!(true);
    result["error"] = json!(error);
    result["controller_frames"] = json!(*samples.lock().unwrap());
    write_json(&output.join("partial.json"), &result)
}

/// Explicit component/port aliases keep shared node quantities attributable to
/// every connected component, including components composed around a CAD model.
fn component_channels(runtime: &Runtime, plan: &sim_script::System, parts: &BTreeMap<String, sim_core::Instance>) -> Vec<(StateId, String, String)> {
    let mut channels = BTreeMap::new();
    for component in &plan.components {
        let instance = &parts[&component.name];
        let name = &component.name;
        let behavior = &runtime.model.behaviors[instance.behavior];
        let native = &runtime.model.objects[behavior.object].name;
        for (id, entry) in runtime.model.state.iter() {
            if let Some(suffix) = entry.name.strip_prefix(&format!("{native}.")) {
                channels.insert(format!("{name}.{suffix}"), (id, entry.quantity.unit().to_owned()));
            }
        }
        for (&id, port) in instance.ports.values().map(|id| (id, &runtime.model.ports[*id])) {
        match port.schema {
            PortSchema::Acausal(kind) => {
                for (lane_index, lane) in kind.lanes().iter().enumerate() {
                    channels.insert(format!("{name}.{}.{}", port.name, lane.across),
                        (runtime.across_lane_id(id, lane_index), lane.across_kind.unit().to_owned()));
                }
            }
            PortSchema::SignalIn(kind) | PortSchema::SignalOut(kind) => {
                channels.insert(format!("{name}.{}", port.name), (runtime.signal_id(id), kind.unit().to_owned()));
            }
        }
        }
    }
    channels.into_iter().map(|(name, (id, unit))| (id, name, unit)).collect()
}

fn add_component_trace(result: &mut Value, ids: &[(StateId, String, String)], signals: &BTreeMap<String, Vec<f64>>) {
    if !result["trace"]["signals"].is_object() { result["trace"]["signals"] = json!({}); }
    for (name, values) in signals { result["trace"]["signals"][name] = json!(values); }
    if !result["signal_units"].is_object() { result["signal_units"] = json!({}); }
    for (_, name, unit) in ids { result["signal_units"][name] = json!(unit); }
}

fn describe_components(world: &ModelWorld, cad_names: &[(String, String)]) -> Value {
    json!(world.behaviors.iter().map(|(id, behavior)| {
        let name = &world.objects[behavior.object].name;
        let identity = cad_names.iter().filter(|(body, _)| !body.is_empty()).find_map(|(body, display)|
            name.strip_prefix(&format!("{display}.")).map(|role| (body, format!("cad/{body}/{role}"))));
        json!({"name":name, "type":behavior.kind.0,
            "binding": identity.as_ref().map(|(_, binding)| binding.as_str()).unwrap_or(name),
            "body_id": identity.as_ref().map(|(body, _)| *body),
            "parameters":behavior.parameters.iter().map(|(key, value)|
                (key, if value.value_si.is_finite() {json!(value.value_si)} else {json!(value.value_si.to_string())})).collect::<BTreeMap<_,_>>(),
            "ports":world.ports.iter().filter(|(_, port)| port.owner == id)
                .map(|(_, port)| sim_script::describe_port(&port.name, port.schema)).collect::<Vec<_>>()})
    }).collect::<Vec<_>>())
}

/// Retain native validation while relating disposable port IDs to authored
/// names and captured source lines before the world moves into the runtime.
fn validate_composition(world: &ModelWorld, registry: &sim_core::BehaviorRegistry,
    plan: &sim_script::System, parts: &BTreeMap<String, sim_core::Instance>) -> Result<(), String> {
    sim_compile::compile(world, registry).map(|_| ()).map_err(|error| {
        use sim_compile::CompileError as E;
        let ports = match &error {
            E::TooFewPorts { connection } | E::MissingPortReference { connection }
            | E::IncompatibleConnection { connection } | E::SignalOutputCount { connection } =>
                world.connections.get(*connection).map(|c| c.ports.clone()).unwrap_or_default(),
            E::DanglingPort { port } | E::PortInTwoConnections { port }
            | E::CompositeInConnection { port } | E::MissingOwner { port } => vec![*port],
            E::PortMismatch { behavior, .. } | E::MissingPort { behavior, .. }
            | E::NoEquations { behavior, .. } | E::Equations { behavior, .. } =>
                world.ports.iter().filter(|(_, p)| p.owner == *behavior).map(|(id, _)| id).collect(),
            _ => vec![],
        };
        let mut message = error.to_string();
        for id in &ports {
            if let Some(port) = world.ports.get(*id) {
                let Some(behavior) = world.behaviors.get(port.owner) else { continue; };
                let Some(object) = world.objects.get(behavior.object) else { continue; };
                let schema = match port.schema {
                    PortSchema::Acausal(kind) => format!("{} [{}]", kind.name(), kind.lanes().iter()
                        .map(|lane| format!("{} / {}", lane.across_kind.unit(), lane.through_kind.unit()))
                        .collect::<Vec<_>>().join(", ")),
                    PortSchema::SignalIn(kind) => format!("input [{}]", kind.unit()),
                    PortSchema::SignalOut(kind) => format!("output [{}]", kind.unit()),
                };
                message.push_str(&format!("\n  {}.{} · {schema}", object.name, port.name));
            }
        }
        let mut locations = std::collections::BTreeSet::new();
        let mut add_location = |location: &sim_script::Location| {
            locations.insert(format!("{}:{}:{}", location.source, location.line.unwrap_or(0), location.column.unwrap_or(0)));
        };
        for connection in &plan.connections {
            if connection.ports.iter().any(|p| parts.get(&p.component).and_then(|part| part.try_port(&p.name))
                .is_some_and(|id| ports.contains(&id) || world.ports[id].members.iter().any(|member| ports.contains(member)))) {
                add_location(&connection.location);
            }
        }
        for component in &plan.components {
            if parts.get(&component.name).is_some_and(|part| ports.iter().any(|id|
                world.ports.get(*id).is_some_and(|port| port.owner == part.behavior))) {
                add_location(&component.location);
            }
        }
        if !locations.is_empty() { message.push_str(&format!("\nCaptured declarations: {}", locations.into_iter().collect::<Vec<_>>().join(", "))); }
        message
    })
}

fn component_native_names(world: &ModelWorld, parts: &BTreeMap<String, sim_core::Instance>) -> BTreeMap<String, String> {
    parts.iter().map(|(name, part)| (name.clone(), world.objects[world.behaviors[part.behavior].object].name.clone())).collect()
}

/// Equation-level initialization/ownership errors name the native components.
/// Retain that context even when a CAD binding uses a different authored alias.
fn script_runtime_diagnostic(mut message: String, plan: &sim_script::System, native_names: &BTreeMap<String, String>) -> String {
    let involved: std::collections::BTreeSet<_> = plan.components.iter().filter(|component|
        native_names.get(&component.name).is_some_and(|native| message.contains(&format!("{native}."))))
        .map(|component| component.name.as_str()).collect();
    let mut locations = std::collections::BTreeSet::new();
    let mut add = |location: &sim_script::Location| {
        locations.insert(format!("{}:{}:{}", location.source, location.line.unwrap_or(0), location.column.unwrap_or(0)));
    };
    for component in &plan.components {
        if involved.contains(component.name.as_str()) { add(&component.location); }
    }
    for connection in &plan.connections {
        if connection.ports.iter().any(|port| involved.contains(port.component.as_str())) { add(&connection.location); }
    }
    if !locations.is_empty() { message.push_str(&format!("\nCaptured declarations: {}", locations.into_iter().collect::<Vec<_>>().join(", "))); }
    message
}

/// Runs only captured inputs. The caller owns cancellation of this process and
/// controller descendants. Progress events are flushed as newline-delimited JSON.
pub fn run(
    mut spec: Specification,
    output: &Path,
    mut progress: impl FnMut(Value),
) -> Result<Value, String> {
    if spec.version != 1 {
        return Err("unsupported experiment specification version".into());
    }
    std::fs::create_dir_all(output).map_err(|e| e.to_string())?;
    let begin = Instant::now();
    let registry = registry();
    progress(json!({"state":"building","stage":"script"}));
    let plan = sim_script::evaluate_seeded(
        &spec.system,
        &registry,
        sim_script::parameter_map(&spec.parameters).map_err(|e| e.to_string())?,
        spec.seed,
    )
    .map_err(|e| e.to_string())?;
    let script_seconds = begin.elapsed().as_secs_f64();
    // Script settings explicitly override captured defaults and are retained in
    // resolved.json. Unknown settings are errors instead of silently ignored.
    if let Some(settings) = plan.configuration.get("settings") {
        let mut merged = serde_json::to_value(&spec.settings).unwrap();
        for (k, v) in settings
            .as_object()
            .ok_or("configure.settings must be an object")?
        {
            merged[k] = v.clone();
        }
        spec.settings = serde_json::from_value(merged).map_err(|e| e.to_string())?;
    }
    let s = &spec.settings;
    if ![s.seconds, s.step, s.sample]
        .iter()
        .all(|v| v.is_finite() && *v > 0.)
        || s.sample < s.step
    {
        return Err(
            "seconds, step and sample must be positive finite values; sample must be at least step"
                .into(),
        );
    }
    write_json(
        &output.join("resolved.json"),
        &json!({"specification":spec,"system":plan}),
    )?;
    let samples = Arc::new(Mutex::new(Vec::<Value>::new()));
    let compile = Instant::now();
    let mut contract = None;
    let mut result;
    let mut stepping_seconds = 0.0;
    let mut recording_seconds = 0.0;
    if plan.cad.is_empty() {
        let mut world = ModelWorld::default();
        write_json(&output.join("imported_components.json"), &json!([]))?;
        let parts = plan.apply(&mut world, &registry, BTreeMap::new())?;
        validate_composition(&world, &registry, &plan, &parts)?;
        let native_names = component_native_names(&world, &parts);
        let mut runtime = Runtime::new(world, &registry, Integrator::BackwardEuler(newton()))
            .map_err(|e| script_runtime_diagnostic(e.to_string(), &plan, &native_names))?;
        runtime.seed(spec.seed);
        write_json(&output.join("resolved_components.json"), &describe_components(&runtime.model, &[]))?;
        if let Some(c) = &spec.controller {
            let name = c.seam.as_deref().unwrap_or("controller");
            let seam = parts
                .get(name)
                .ok_or_else(|| format!("controller seam `{name}` does not exist"))?
                .behavior;
            contract = Some(runtime.contract(seam));
            runtime
                .attach(seam, controller(c, samples.clone(), spec.seed)?)
                .map_err(|e| e.to_string())?;
        }
        let compile_seconds = compile.elapsed().as_secs_f64();
        progress(json!({"state":if spec.preflight {"building"} else {"running"},"stage":if spec.preflight {"check"} else {"simulation"},"fraction":0}));
        let mut ids: Vec<_> = runtime
            .model
            .state
            .iter()
            .map(|(id, e)| (id, e.name.clone(), e.quantity.unit().to_owned()))
            .collect();
        let aliases = component_channels(&runtime, &plan, &parts);
        for alias in aliases {
            if !ids.iter().any(|(_, name, _)| name == &alias.1) { ids.push(alias); }
        }
        let mut t = Vec::new();
        let mut signals = BTreeMap::<String, Vec<f64>>::new();
        let mut k = 0;
        while !spec.preflight && runtime.time < s.seconds - 1e-10 {
            let stepping = Instant::now();
            if let Err(error) = runtime.advance(s.sample.min(s.seconds - runtime.time), s.step) {
                let error = error.to_string();
                partial(
                    output,
                    &spec,
                    json!({"trace":{"t":t,"signals":signals}, "signal_units":ids.iter().map(|(_,n,u)|(n,u)).collect::<BTreeMap<_,_>>()}),
                    &samples,
                    &error,
                )?;
                return Err(error);
            }
            stepping_seconds += stepping.elapsed().as_secs_f64();
            let recording = Instant::now();
            t.push(runtime.time);
            for (id, name, _) in &ids {
                signals
                    .entry(name.clone())
                    .or_default()
                    .push(runtime.get(*id));
            }
            recording_seconds += recording.elapsed().as_secs_f64();
            k += 1;
            if k % 10 == 0 {
                progress(json!({"state":"running","fraction":runtime.time/s.seconds}));
            }
        }
        result = json!({"trace":{"t":t,"signals":signals},"signal_units":ids.iter().map(|(_,n,u)|(n,u)).collect::<BTreeMap<_,_>>(),"timing":{"compile_s":compile_seconds},"duration_s":runtime.time});
    } else {
        if plan.cad.len() != 1 {
            return Err("an experiment currently accepts one CAD assembly; group connected parts in that document".into());
        }
        let alias = &plan.cad[0];
        let model_value = spec
            .cad
            .get(alias)
            .ok_or_else(|| format!("CAD alias `{alias}` was not captured"))?;
        let model: sim_domain_robot::model::PhysicalModel =
            serde_json::from_value(model_value.clone()).map_err(|e| format!("CAD model: {e}"))?;
        let options = BuildOptions {
            step: s.step,
            sample: s.sample,
            flex: s.flex,
            contact: s.contact,
            planar: s.planar,
            driver_control: spec
                .controller
                .as_ref()
                .is_some_and(|c| c.interface == ControlInterface::DriverDuty),
            ..Default::default()
        };
        let cad_names: Vec<_> = model.motors.iter().map(|m| (m.id.clone(), m.name.clone()))
            .chain(model.links.iter().map(|l| (l.id.clone(), l.name.clone()))).collect();
        let mut composed_parts = BTreeMap::new();
        let mut native_names = BTreeMap::new();
        let mut robot =
            PhysicalRobot::build_with(model, &registry, &options, |world, assembly| {
                write_json(&output.join("imported_components.json"), &describe_components(world, &cad_names))?;
                let mut parts = sim_script::instances(world);
                // Stable CAD IDs survive display-name changes. Roles retain
                // their native suffix (case, winding, unit, g_wc, mount, ...).
                let mut identities = BTreeMap::new();
                for (id, name) in &cad_names {
                    if id.is_empty() { continue; }
                    for (native, instance) in &parts {
                        if let Some(role) = native.strip_prefix(&format!("{name}.")) {
                            identities.insert(format!("cad/{id}/{role}"), instance.clone());
                        }
                    }
                }
                parts.extend(identities);
                let mut exposed = assembly.clone();
                for (name, part) in &parts {
                    for (port, id) in &part.ports {
                        exposed.ports.insert(format!("{name}.{port}"), *id);
                    }
                }
                parts.insert(alias.clone(), exposed);
                composed_parts = plan.apply(world, &registry, parts)?;
                native_names = component_native_names(world, &composed_parts);
                validate_composition(world, &registry, &plan, &composed_parts)?;
                Ok(())
            }).map_err(|error| script_runtime_diagnostic(error, &plan, &native_names))?;
        robot.runtime.seed(spec.seed);
        write_json(&output.join("resolved_components.json"), &describe_components(&robot.runtime.model, &cad_names))?;
        if let Some(c) = &spec.controller {
            let seam = robot
                .seam
                .ok_or("CAD model has no external controller seam")?;
            contract = Some(robot.runtime.contract(seam));
            robot
                .runtime
                .attach(seam, controller(c, samples.clone(), spec.seed)?)
                .map_err(|e| e.to_string())?;
        }
        let compile_seconds = compile.elapsed().as_secs_f64();
        progress(json!({"state":if spec.preflight {"building"} else {"running"},"stage":if spec.preflight {"check"} else {"simulation"},"fraction":0}));
        let mut poses = BTreeMap::<String, Vec<Value>>::new();
        let mut flex = BTreeMap::<String, Vec<FlexBoundaryTrace>>::new();
        let ids = component_channels(&robot.runtime, &plan, &composed_parts);
        let mut component_signals = BTreeMap::<String, Vec<f64>>::new();
        let mut k = 0;
        while !spec.preflight && robot.runtime.time < s.seconds - 1e-10 {
            let stepping = Instant::now();
            let count = robot.recorded_samples();
            if let Err(error) = robot.advance(s.sample.min(s.seconds - robot.runtime.time)) {
                let mut result = robot.results("captured CAD assembly");
                result["trace"]["poses"] = json!(poses);
                result["trace"]["flex"] = json!(flex);
                add_component_trace(&mut result, &ids, &component_signals);
                partial(output, &spec, result, &samples, &error)?;
                return Err(error);
            }
            stepping_seconds += stepping.elapsed().as_secs_f64();
            if robot.recorded_samples() == count {
                continue;
            }
            let recording = Instant::now();
            for (id, name, _) in &ids {
                component_signals.entry(name.clone()).or_default().push(robot.runtime.get(*id));
            }
            for (link, (rotation, position)) in robot.model.links.iter().zip(robot.poses()) {
                // Delta transforms map original CAD world millimetres into the
                // simulated pose. One merged link maps all its member CAD IDs.
                let com = sim_domain_robot::math::v(link.com);
                let offset = (position - rotation * com) * 1000.;
                let matrix = json!([
                    [
                        rotation[(0, 0)],
                        rotation[(0, 1)],
                        rotation[(0, 2)],
                        offset[0]
                    ],
                    [
                        rotation[(1, 0)],
                        rotation[(1, 1)],
                        rotation[(1, 2)],
                        offset[1]
                    ],
                    [
                        rotation[(2, 0)],
                        rotation[(2, 1)],
                        rotation[(2, 2)],
                        offset[2]
                    ],
                    [0., 0., 0., 1.]
                ]);
                poses.entry(link.name.clone()).or_default().push(matrix);
            }
            let mut boundary_indices = BTreeMap::<usize, usize>::new();
            for (li, point, displacement) in robot.deflections() {
                let link = &robot.model.links[li];
                let boundaries = flex.entry(link.name.clone()).or_insert_with(|| {
                    link.flex.as_ref().unwrap().boundary_frames.iter().map(|frame|
                        FlexBoundaryTrace { id: frame.id.clone(), name: frame.name.clone(), ..Default::default() }
                    ).collect()
                });
                let bi = boundary_indices.entry(li).or_default();
                boundaries[*bi].point_m.push(point.into());
                boundaries[*bi].displacement_m.push(displacement.into());
                *bi += 1;
            }
            recording_seconds += recording.elapsed().as_secs_f64();
            k += 1;
            if k % 10 == 0 {
                progress(json!({"state":"running","fraction":robot.runtime.time/s.seconds}));
            }
        }
        result = robot.results("captured CAD assembly");
        add_component_trace(&mut result, &ids, &component_signals);
        result["components"] = json!(robot.runtime.model.behaviors.iter().map(|(_,b)|
            json!({"name": robot.runtime.model.objects[b.object].name, "kind": b.kind.0})
        ).collect::<Vec<_>>());
        result["trace"]["poses"] = json!(poses);
        result["trace"]["flex"] = json!(flex);
        result["cad_mapping"]=Value::Array(model_value["links"].as_array().ok_or("CAD links must be an array")?.iter().map(|l|json!({"name":l["name"],"id":l["id"],"members":l["members"],"member_names":l["member_names"]})).collect());
        result["timing"] = json!({"compile_s":compile_seconds});
    }
    result["version"] = json!(1);
    result["preflight"] = json!(spec.preflight);
    if spec.preflight {
        result["trace"] = json!({});
        result["duration_s"] = json!(0.);
    }
    result["run_id"] = json!(spec.run_id);
    result["provenance"] = spec.provenance;
    result["settings"] = serde_json::to_value(s).unwrap();
    result["seed"] = json!(spec.seed);
    result["profile"] = json!(spec.profile);
    result["units"] =
        json!({"cad_length":"mm","physical_length":"m","mass":"kg","time":"s","angle":"rad"});
    let newton_settings = newton();
    result["solver"] = json!({"integrator":"backward_euler","newton":{
        "absolute_tolerance":newton_settings.absolute_tolerance,
        "relative_tolerance":newton_settings.relative_tolerance,
        "max_iterations":newton_settings.max_iterations,
        "min_line_search":newton_settings.min_line_search}});
    result["controller_frames"] = json!(*samples.lock().unwrap());
    result["controller_interface"] = if plan.cad.is_empty() {
        json!("declared_seam")
    } else {
        json!(spec
            .controller
            .as_ref()
            .map(|c| c.interface)
            .unwrap_or_default())
    };
    if let Some(c) = contract {
        result["controller_contract"] = json!({"period":c.period,"sensors":c.sensors.iter().map(|c|json!({"name":c.name,"unit":c.unit()})).collect::<Vec<_>>(),"actuators":c.actuators.iter().map(|c|json!({"name":c.name,"unit":c.unit()})).collect::<Vec<_>>()});
    }
    result["timing"]["script_s"] = json!(script_seconds);
    result["timing"]["step_s"] = json!(stepping_seconds);
    result["timing"]["record_s"] = json!(recording_seconds);
    result["timing"]["total_s"] = json!(begin.elapsed().as_secs_f64());
    write_json(&output.join("result.json"), &result)?;
    progress(json!({"state":"completed","fraction":1,"timing":result["timing"]}));
    Ok(result)
}
