//! Rhai authoring and sampled controllers over the existing Rust simulation API.
//! Scripts are evaluated from captured sources, never from the live filesystem.
use rhai::{
    Array, CallFnOptions, Dynamic, Engine, EvalAltResult, Map, Module, NativeCallContext, Position,
    Scope, AST,
};
use serde::{Deserialize, Serialize};
use sim_core::{
    BehaviorRegistry, BehaviorTypeId, Contract, Coupler, CouplerError, Instance, ModelWorld,
};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

pub type ScriptResult<T> = Result<T, Box<EvalAltResult>>;
fn error(message: impl Into<String>) -> Box<EvalAltResult> {
    message.into().into()
}

/// Exact source contents retained by an experiment. Imports are bundle-relative.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sources {
    pub entry: String,
    pub files: BTreeMap<String, String>,
}
impl Sources {
    pub fn single(name: &str, source: &str) -> Self {
        Self {
            entry: name.into(),
            files: [(name.into(), source.into())].into(),
        }
    }
    pub fn compile(&self, engine: &Engine, path: &str) -> ScriptResult<AST> {
        let source = self
            .files
            .get(path)
            .ok_or_else(|| error(format!("source `{path}` is not in the captured experiment")))?;
        let mut ast = engine
            .compile(source)
            .map_err(|e| error(format!("{path}: {e}")))?;
        ast.set_source(path);
        Ok(ast)
    }
}

#[derive(Clone)]
struct CapturedResolver {
    sources: Sources,
    active: Arc<Mutex<Vec<String>>>,
}
impl rhai::ModuleResolver for CapturedResolver {
    fn resolve(
        &self,
        engine: &Engine,
        source: Option<&str>,
        path: &str,
        pos: Position,
    ) -> ScriptResult<rhai::Shared<Module>> {
        // Import identities are portable across hosts; absolute paths and parent
        // traversal are not part of the captured module namespace.
        if path.starts_with('/') || path.contains('\\') || path.split('/').any(|p| p == "..") {
            return Err(Box::new(EvalAltResult::ErrorModuleNotFound(
                path.into(),
                pos,
            )));
        }
        let parent = source.and_then(|s| s.rsplit_once('/').map(|(p, _)| p));
        let relative = if let Some(parent) = parent {
            format!("{parent}/{path}")
        } else {
            path.into()
        };
        let relative = if relative.ends_with(".rhai") {
            relative
        } else {
            format!("{relative}.rhai")
        };
        {
            let mut active = self.active.lock().unwrap();
            if active.contains(&relative) {
                return Err(error(format!(
                    "captured import cycle: {} -> {relative}",
                    active.join(" -> ")
                )));
            }
            if active.len() >= 64 {
                return Err(error("captured import nesting exceeds 64 modules"));
            }
            active.push(relative.clone());
        }
        let result = self
            .sources
            .compile(engine, &relative)
            .and_then(|ast| Module::eval_ast_as_new(Scope::new(), &ast, engine)
                .map(Into::into).map_err(|e| error(format!("{relative}: {e}"))));
        self.active.lock().unwrap().pop();
        result
    }
}

fn engine(sources: Sources, parameters: Map, seed: u64) -> Engine {
    let mut engine = Engine::new();
    engine.set_max_operations(2_000_000);
    engine.set_max_call_levels(64);
    engine.set_max_expr_depths(64, 64);
    engine.set_max_modules(128);
    engine.set_module_resolver(CapturedResolver {
        sources,
        active: Arc::new(Mutex::new(Vec::new())),
    });
    engine.on_print(|s| eprintln!("{s}"));
    engine.register_fn("parameters", move || parameters.clone());
    engine.register_fn("seed", move || seed as rhai::INT);
    engine
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Location {
    pub source: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
}
impl Location {
    fn of(ctx: &NativeCallContext) -> Self {
        Self {
            source: ctx.call_source().unwrap_or("<script>").into(),
            line: ctx.call_position().line(),
            column: ctx.call_position().position(),
        }
    }
    fn message(&self, text: impl std::fmt::Display) -> String {
        format!(
            "{}:{}:{}: {text}",
            self.source,
            self.line.unwrap_or(0),
            self.column.unwrap_or(0)
        )
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Component {
    pub name: String,
    pub kind: String,
    /// Existing native component whose parameters/ports this declaration owns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding: Option<String>,
    pub parameters: BTreeMap<String, f64>,
    pub location: Location,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Port {
    pub component: String,
    pub name: String,
}
#[derive(Clone, Debug)]
struct Part(String);
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Connection {
    pub ports: Vec<Port>,
    pub location: Location,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct System {
    pub components: Vec<Component>,
    pub connections: Vec<Connection>,
    /// CAD imports refer to aliases supplied by the captured run specification.
    pub cad: Vec<String>,
    pub configuration: serde_json::Value,
    pub configuration_location: Option<Location>,
}
impl System {
    /// Compose into a fresh or CAD-derived ModelWorld. Native and scripted
    /// components go through the same registry and compiler. The caller owns
    /// this candidate world and publishes it only after a successful build.
    pub fn apply(
        &self,
        world: &mut ModelWorld,
        registry: &BehaviorRegistry,
        mut parts: BTreeMap<String, Instance>,
    ) -> Result<BTreeMap<String, Instance>, String> {
        let imported = parts.clone();
        let mut bound = std::collections::HashSet::new();
        for component in &self.components {
            if parts.contains_key(&component.name) {
                return Err(component
                    .location
                    .message(format!("duplicate component `{}`", component.name)));
            }
            let instance = if let Some(target) = &component.binding {
                let instance = imported.get(target).ok_or_else(|| component.location.message(
                    format!("cannot bind `{}`: imported component `{target}` does not exist", component.name)))?;
                if !bound.insert(instance.behavior) {
                    return Err(component.location.message(format!("imported component `{target}` is bound more than once")));
                }
                let behavior = &mut world.behaviors[instance.behavior];
                if behavior.kind.0 != component.kind {
                    return Err(component.location.message(format!("`{target}` has type {}, expected {}", behavior.kind.0, component.kind)));
                }
                let mut parameters: BTreeMap<_, _> = behavior.parameters.iter()
                    .map(|(name, value)| (name.clone(), value.value_si)).collect();
                parameters.extend(component.parameters.clone());
                let descriptor = registry.get(&behavior.kind).map_err(|e| component.location.message(e))?;
                descriptor.validate_parameters(&parameters).map_err(|e| component.location.message(e))?;
                if let Some(factory) = descriptor.equations {
                    factory(&parameters).map_err(|e| component.location.message(e))?;
                }
                // Bindings cannot add dynamic ports; those belong to CAD's
                // declared interface. A new system component can own new ports.
                for name in component.parameters.keys() {
                    if descriptor.ports.iter().any(|p| p.name.contains('*') && p.matches(name))
                        && !instance.ports.contains_key(name) {
                        return Err(component.location.message(format!("binding cannot add dynamic port `{name}`")));
                    }
                }
                for (name, value) in &component.parameters {
                    behavior.parameters.entry(name.clone())
                        .and_modify(|q| q.value_si = *value)
                        .or_insert(sim_core::Quantity::new(*value, sim_core::QuantityKind::Dimensionless));
                }
                instance.clone()
            } else { world
                .part(
                    registry,
                    &component.name,
                    &component.kind,
                    component.parameters.iter().map(|(k, v)| (k.as_str(), *v)),
                )
                .map_err(|e| component.location.message(e))? };
            parts.insert(component.name.clone(), instance);
        }
        for connection in &self.connections {
            let ports: Vec<_> = connection
                .ports
                .iter()
                .map(|p| {
                    let instance = parts.get(&p.component).ok_or_else(|| {
                        connection
                            .location
                            .message(format!("unknown component `{}`", p.component))
                    })?;
                    instance.try_port(&p.name).ok_or_else(|| {
                        connection.location.message(format!(
                            "{}.{} is not a port; available: {:?}",
                            p.component,
                            p.name,
                            instance.ports.keys().collect::<Vec<_>>()
                        ))
                    })
                })
                .collect::<Result<_, _>>()?;
            // Script connections extend a net, including one supplied by CAD.
            // Expand composite ports through the native API before joining so
            // attaching to a plug's thermal member cannot join its other lanes.
            let previous = world.connections.len();
            world.connect(ports);
            let expanded = world.connections.split_off(previous);
            for mut added in expanded {
                let mut seen = std::collections::HashSet::new();
                if added.ports.iter().any(|p| !seen.insert(*p)) {
                    return Err(connection.location.message("a connection repeats the same port"));
                }
                let overlaps: Vec<_> = world.connections.iter().enumerate()
                    .filter(|(_, old)| old.ports.iter().any(|p| added.ports.contains(p)))
                    .map(|(i, _)| i).collect();
                for &i in &overlaps {
                    for &port in &world.connections[i].ports {
                        if !added.ports.contains(&port) { added.ports.push(port); }
                    }
                }
                let insertion = overlaps.first().copied().unwrap_or(world.connections.len());
                for i in overlaps.into_iter().rev() { world.connections.remove(i); }
                world.connections.insert(insertion, added);
            }
        }
        Ok(parts)
    }
}

fn numeric(value: Dynamic) -> ScriptResult<f64> {
    let n = if value.is::<rhai::INT>() {
        value.cast::<rhai::INT>() as f64
    } else if value.is::<rhai::FLOAT>() {
        value.cast::<rhai::FLOAT>()
    } else {
        return Err(error("component parameters must be numbers"));
    };
    if !n.is_finite() {
        return Err(error("component parameters must be finite"));
    }
    Ok(n)
}

/// Evaluate reusable subsystem functions and produce a source-mapped build plan.
pub fn evaluate(
    sources: &Sources,
    registry: &BehaviorRegistry,
    parameters: Map,
) -> ScriptResult<System> {
    evaluate_seeded(sources, registry, parameters, 0)
}

pub fn evaluate_seeded(
    sources: &Sources,
    registry: &BehaviorRegistry,
    parameters: Map,
    seed: u64,
) -> ScriptResult<System> {
    if seed > (1_u64 << 53) - 1 {
        return Err(error("seed exceeds the exact numeric range"));
    }
    let plan = Arc::new(Mutex::new(System::default()));
    let mut engine = engine(sources.clone(), parameters, seed);
    engine.register_type_with_name::<Part>("Component");
    engine.register_type_with_name::<Port>("Port");
    // A typed reference to another declaration, graph component or imported
    // native component. Resolution occurs after every component is instantiated.
    engine.register_fn("component", |name: &str| Part(name.into()));
    engine.register_fn("port", |part: &mut Part, name: &str| Port {
        component: part.0.clone(),
        name: name.into(),
    });
    let state = plan.clone();
    let binding_registry = registry.clone();
    engine.register_fn("bind_component", move |ctx: NativeCallContext, name: &str, target: &str, kind: &str, params: Map| -> ScriptResult<Part> {
        binding_registry.get(&BehaviorTypeId::from(kind)).map_err(|e| error(e.to_string()))?;
        let parameters = params.into_iter().map(|(k, v)| Ok((k.to_string(), numeric(v)?)))
            .collect::<ScriptResult<BTreeMap<_, _>>>()?;
        let mut state = state.lock().unwrap();
        if state.components.iter().any(|p| p.name == name) || state.cad.iter().any(|p| p == name) {
            return Err(error(format!("duplicate component `{name}`")));
        }
        state.components.push(Component { name: name.into(), kind: kind.into(), binding: Some(target.into()),
            parameters, location: Location::of(&ctx) });
        Ok(Part(name.into()))
    });
    let state = plan.clone();
    let registry = registry.clone();
    engine.register_fn(
        "part",
        move |ctx: NativeCallContext, name: &str, kind: &str, params: Map| -> ScriptResult<Part> {
            let descriptor = registry
                .get(&BehaviorTypeId::from(kind))
                .map_err(|e| error(e.to_string()))?;
            let parameters: BTreeMap<_, _> = params
                .into_iter()
                .map(|(k, v)| Ok((k.to_string(), numeric(v)?)))
                .collect::<ScriptResult<_>>()?;
            descriptor
                .validate_parameters(&parameters)
                .map_err(|e| error(format!("{name} ({kind}): {e}")))?;
            if let Some(factory) = descriptor.equations {
                factory(&parameters).map_err(|e| error(format!("{name} ({kind}): {e}")))?;
            }
            let mut state = state.lock().unwrap();
            if state.components.iter().any(|p| p.name == name)
                || state.cad.iter().any(|p| p == name)
            {
                return Err(error(format!("duplicate component `{name}`")));
            }
            state.components.push(Component {
                name: name.into(),
                kind: kind.into(),
                binding: None,
                parameters,
                location: Location::of(&ctx),
            });
            Ok(Part(name.into()))
        },
    );
    let state = plan.clone();
    engine.register_fn("cad", move |name: &str| -> ScriptResult<Part> {
        let mut state = state.lock().unwrap();
        if state.cad.iter().any(|p| p == name) || state.components.iter().any(|p| p.name == name) {
            return Err(error(format!("duplicate component `{name}`")));
        }
        state.cad.push(name.into());
        Ok(Part(name.into()))
    });
    let state = plan.clone();
    engine.register_fn(
        "connect",
        move |ctx: NativeCallContext, ports: Array| -> ScriptResult<()> {
            if ports.is_empty() {
                return Err(error("connect needs at least one port"));
            }
            let ports = ports
                .into_iter()
                .map(|p| {
                    p.try_cast::<Port>()
                        .ok_or_else(|| error("connect expects typed port handles"))
                })
                .collect::<ScriptResult<_>>()?;
            state.lock().unwrap().connections.push(Connection {
                ports,
                location: Location::of(&ctx),
            });
            Ok(())
        },
    );
    let state = plan.clone();
    engine.register_fn("configure", move |ctx: NativeCallContext, configuration: Map| -> ScriptResult<()> {
        let mut state = state.lock().unwrap();
        if let Some(location) = &state.configuration_location {
            return Err(error(location.message("configure was already declared here; combine settings and expectations in one declaration")));
        }
        state.configuration = rhai::serde::from_dynamic(&Dynamic::from(configuration))?;
        state.configuration_location = Some(Location::of(&ctx));
        Ok(())
    });
    let ast = sources.compile(&engine, &sources.entry)?;
    engine.run_ast(&ast).map_err(|e| error(format!("{}: {e}", sources.entry)))?;
    let result = plan.lock().unwrap().clone();
    Ok(result)
}

/// Stateful sampled Rhai control. `control(t, sensors, commands, state)` returns
/// a map containing exactly the named actuator channels. State is per run.
pub struct RhaiController {
    source: String,
    engine: Engine,
    ast: AST,
    scope: Scope<'static>,
    contract: Option<Contract>,
    state: Map,
}
impl RhaiController {
    pub fn new(sources: Sources, parameters: Map) -> ScriptResult<Self> {
        Self::with_seed(sources, parameters, 0)
    }

    pub fn with_seed(sources: Sources, parameters: Map, seed: u64) -> ScriptResult<Self> {
        if seed > (1_u64 << 53) - 1 {
            return Err(error("seed exceeds the exact numeric range"));
        }
        let mut engine = engine(sources.clone(), parameters, seed);
        let program = sources.compile(&engine, &sources.entry)?;
        // Modules encapsulate imported namespaces and constants into their
        // functions. A bare call_fn(eval_ast=false) loses top-level imports;
        // evaluating the whole program every sample would repeat side effects.
        let module = Module::eval_ast_as_new(Scope::new(), &program, &engine)
            .map_err(|e| error(format!("{}: {e}", sources.entry)))?;
        engine.register_static_module("controller_program", module.into());
        let ast = engine
            .compile("fn control(t,s,a,state) { controller_program::control(t,s,a,state) }")?;
        let scope = Scope::new();
        Ok(Self {
            source: sources.entry,
            engine,
            ast,
            scope,
            contract: None,
            state: Map::new(),
        })
    }
}
impl Coupler for RhaiController {
    fn open(&mut self, contract: &Contract) -> Result<(), CouplerError> {
        self.contract = Some(contract.clone());
        self.state.clear();
        Ok(())
    }
    fn sample(
        &mut self,
        t: f64,
        sensors: &[f64],
        actuators: &mut [f64],
    ) -> Result<(), CouplerError> {
        let c = self.contract.as_ref().ok_or_else(|| {
            CouplerError::Other("Rhai controller has not received a contract".into())
        })?;
        let sensed: Map = c
            .sensors
            .iter()
            .zip(sensors)
            .map(|(c, v)| (c.name.clone().into(), Dynamic::from_float(*v)))
            .collect();
        let commanded: Map = c
            .actuators
            .iter()
            .zip(actuators.iter())
            .map(|(c, v)| (c.name.clone().into(), Dynamic::from_float(*v)))
            .collect();
        let result: Map = self
            .engine
            .call_fn_with_options(
                CallFnOptions::new().eval_ast(false),
                &mut self.scope,
                &self.ast,
                "control",
                (t, sensed, commanded, self.state.clone()),
            )
            .map_err(|e| CouplerError::Other(format!("{}: {e}", self.source)))?;
        let commands = result
            .get("commands")
            .and_then(|v| v.clone().try_cast::<Map>())
            .ok_or_else(|| {
                CouplerError::Malformed(
                    "control must return #{commands: #{...}, state: #{...}}".into(),
                )
            })?;
        if commands.len() != c.actuators.len()
            || c.actuators
                .iter()
                .any(|a| !commands.contains_key(a.name.as_str()))
        {
            return Err(CouplerError::Malformed(
                "controller command names do not match the actuator contract".into(),
            ));
        }
        let values: Vec<_> = c
            .actuators
            .iter()
            .map(|a| {
                numeric(commands[a.name.as_str()].clone())
                    .map_err(|e| CouplerError::Malformed(e.to_string()))
            })
            .collect::<Result<_, _>>()?;
        self.state = result
            .get("state")
            .and_then(|v| v.clone().try_cast::<Map>())
            .ok_or_else(|| CouplerError::Malformed("controller must return a state map".into()))?;
        actuators.copy_from_slice(&values);
        Ok(())
    }
}

pub fn parameter_map(value: &serde_json::Value) -> ScriptResult<Map> {
    let dynamic = rhai::serde::to_dynamic(value).map_err(|e| error(e.to_string()))?;
    dynamic
        .try_cast()
        .ok_or_else(|| error("parameters must be an object"))
}

/// All native instances, with their declared ports; used to expose an imported
/// CAD assembly's surrounding components without duplicating them.
pub fn instances(world: &ModelWorld) -> BTreeMap<String, Instance> {
    world
        .behaviors
        .iter()
        .map(|(id, b)| {
            (
                world.objects[b.object].name.clone(),
                Instance {
                    behavior: id,
                    ports: world
                        .ports
                        .iter()
                        .filter(|(_, p)| p.owner == id)
                        .map(|(id, p)| (p.name.clone(), id))
                        .collect(),
                },
            )
        })
        .collect()
}

pub fn describe_port(name: &str, schema: sim_core::PortSchema) -> serde_json::Value {
            let mut port = serde_json::json!({"name":name,"schema":schema});
            match schema {
                sim_core::PortSchema::Acausal(kind) => {
                    port["direction"] = serde_json::json!("acausal");
                    port["lanes"] = serde_json::json!(kind.lanes().iter().map(|lane|
                        serde_json::json!({"across":lane.across,"across_unit":lane.across_kind.unit(),
                            "through":lane.through,"through_unit":lane.through_kind.unit()})).collect::<Vec<_>>());
                },
                sim_core::PortSchema::SignalIn(kind) => { port["direction"] = serde_json::json!("input"); port["unit"] = serde_json::json!(kind.unit()); },
                sim_core::PortSchema::SignalOut(kind) => { port["direction"] = serde_json::json!("output"); port["unit"] = serde_json::json!(kind.unit()); },
            }
            port
}

pub fn catalogue(registry: &BehaviorRegistry) -> serde_json::Value {
    serde_json::Value::Array(registry.descriptors().map(|d|serde_json::json!({
        "type":d.type_id.0,"name":d.display_name,
        "parameters":d.parameters,"parameters_complete":d.parameters.is_some(),
        "ports":d.ports.iter().map(|p| describe_port(p.name, p.schema)).collect::<Vec<_>>()
    })).collect())
}
