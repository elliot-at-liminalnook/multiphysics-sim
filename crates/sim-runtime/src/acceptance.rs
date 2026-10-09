//! Acceptance tests of a robot: run the system the robot is composed in,
//! headless, for a stated time, and judge each stated criterion pass, fail
//! or not assessed. A criterion the run cannot assess (no stall torque, a
//! rigid link with no stress analysis, an unloaded bearing) is never a pass.
//!
//! **One execution path.** The run is the composed system's own
//! (docs/architecture/composition.md): the document is flattened with the
//! host's generators, compiled, and its blocks bound and scheduled exactly
//! as in a system test or a live run. Controller blocks in the system (FMUs)
//! run as they are; nothing is substituted for them. What the test commands
//! (its trajectory) drives only signal inputs the system leaves open, through
//! a test-bench block on the same scheduler, at the robot model's control
//! period and latency; an input a controller already drives is refused. The
//! robot's own measurements (motor torque against stall, winding
//! temperature, bearing pressure, limits, falls) are read from the compiled
//! model by the same code Robot mode reports with (`PhysicalRobot::attach`).
//!
//! **Evidence.** The verdict is `passed` only when every criterion passed;
//! `failed` when any failed; otherwise `incomplete`. A passed run counts as
//! evidence only when the model lists no blocking assumption
//! (`source.assumptions`, the CAD export's). The report carries a
//! fingerprint of exactly what ran (`system_evidence::Fingerprint`: the
//! system's model, every file it read by SHA-256, the run settings, the
//! test); [`standing`] says whether a kept report still describes the
//! system, and what changed when it does not.
//!
//! The test (trajectory and criteria) is a task condition, not part of the
//! robot: it lives in the project file.
use crate::physical::{BuildOptions, PhysicalRobot};
use crate::system_evidence::{Fingerprint, Standing, Verdict};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sim_core::{BehaviorRegistry, BlockImplementation, BlockInterface, BlockPort, BlockTiming, Checkpoint, PortSchema};
use sim_system::{BlockSource, Command, InstanceKind, InstanceSpec, ParameterBinding, SystemDocument, Terminal};
use std::collections::BTreeMap;
use std::path::Path;

/// The host implementation name of the test bench's stimulus block.
pub const STIMULUS: &str = "test_stimulus";
/// The generated robot's instance name in a system made by [`default_system`].
pub const ROBOT: &str = "robot";

/// How often the run is sampled for the criteria and the traces (s).
pub const SAMPLE_S: f64 = 0.01;
pub const VERDICT_RULE: &str = "passed only when every criterion passed; failed when any failed; otherwise incomplete (a criterion that could not be assessed is never a pass). Evidence additionally needs a model with no blocking assumption.";

/// One commanded point at `t`. A key is a driven joint of the robot (its
/// servo target, rad, or m on a prismatic joint) or `instance.port`, an open
/// signal input at the system's top level (a controller's setpoint).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Waypoint {
    pub t: f64,
    pub targets: BTreeMap<String, f64>,
}

/// What must hold.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Criterion {
    /// `joint` is within `tolerance` of `target` from `by_s` to the end.
    Reaches { joint: String, target: f64, tolerance: f64, by_s: f64 },
    /// `joint` follows its command within `max_error` after `after_s`.
    Tracking { joint: String, max_error: f64, after_s: f64 },
    /// Every motor that drives a joint keeps peak torque below
    /// (1 − `min`) × its stall torque.
    TorqueMargin { min: f64 },
    /// Every motor's winding peaks at least `margin_c` below its rated maximum.
    WindingTemperature { margin_c: f64 },
    /// Every loaded joint bearing's pressure margin is at least `min`.
    BearingMargin { min: f64 },
    /// Every link's stress margin to yield is at least `min` (needs stress:
    /// flexible links; a rigid model cannot assess it).
    YieldMargin { min: f64 },
    /// Every printed part's layer-aware safety factor under the run's peak
    /// loads is at least `min_safety_factor` (`part_strength`; judged after
    /// the run from the CAD's part meshes: a bare model cannot assess it).
    PartStrength { min_safety_factor: f64 },
    /// A free-standing robot stays upright.
    NoFall,
    /// No joint runs past its travel limits.
    NoLimitHits,
}
impl Criterion {
    /// A plain sentence for people.
    pub fn describe(&self) -> String {
        match self {
            Criterion::Reaches { joint, target, tolerance, by_s } => format!("{joint} reaches {:.1}° (±{:.1}°) by {by_s} s and stays there", target.to_degrees(), tolerance.to_degrees()),
            Criterion::Tracking { joint, max_error, after_s } => format!("{joint} follows its command within {:.1}° after {after_s} s", max_error.to_degrees()),
            Criterion::TorqueMargin { min } => format!("every motor keeps at least {:.0} % of its stall torque in reserve", min * 100.0),
            Criterion::WindingTemperature { margin_c } => format!("every motor winding stays at least {margin_c} °C below its rated maximum"),
            Criterion::BearingMargin { min } => format!("every joint bearing keeps a pressure margin of at least {min}"),
            Criterion::YieldMargin { min } => format!("every part keeps a stress margin to yield of at least {min}"),
            Criterion::PartStrength { min_safety_factor } => format!("every printed part keeps a safety factor of at least {min_safety_factor} under the run's peak loads"),
            Criterion::NoFall => "the robot stays upright".into(),
            Criterion::NoLimitHits => "no joint runs past its travel limits".into(),
        }
    }
}

/// A test: how long, what is commanded, what must hold.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Test {
    pub name: String,
    pub duration_s: f64,
    /// Per key: linear between the waypoints that name it, held at its
    /// first value before the first and at its last after the last.
    #[serde(default)]
    pub trajectory: Vec<Waypoint>,
    pub criteria: Vec<Criterion>,
}

/// pass | fail | not_assessed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pass,
    Fail,
    NotAssessed,
}

/// One criterion's judgement.
#[derive(Clone, Debug, Serialize)]
pub struct Outcome {
    pub criterion: Criterion,
    pub description: String,
    pub status: Status,
    /// The number the judgement is on (null when not assessed).
    pub measured: Value,
    /// Why, in a sentence (what was measured, or why it could not be).
    pub detail: String,
}

impl Test {
    /// The test's own errors (named), before a model is involved.
    pub fn validate(&self) -> Result<(), String> {
        if !(self.duration_s.is_finite() && self.duration_s > 0.0 && self.duration_s <= 600.0) {
            return Err(format!("test `{}`: duration_s must be in (0, 600] s", self.name));
        }
        if self.criteria.is_empty() {
            return Err(format!("test `{}`: state at least one criterion (what must hold)", self.name));
        }
        let mut last = f64::NEG_INFINITY;
        for (i, w) in self.trajectory.iter().enumerate() {
            if !(w.t.is_finite() && w.t >= 0.0 && w.t > last) {
                return Err(format!("test `{}`: trajectory[{i}].t must be finite, nonnegative and increasing", self.name));
            }
            last = w.t;
            if let Some((j, _)) = w.targets.iter().find(|(_, v)| !v.is_finite()) {
                return Err(format!("test `{}`: trajectory[{i}].targets.{j} is not finite", self.name));
            }
        }
        for (i, c) in self.criteria.iter().enumerate() {
            let bad = match c {
                Criterion::Reaches { tolerance, by_s, target, .. } => !(tolerance.is_finite() && *tolerance > 0.0 && by_s.is_finite() && *by_s >= 0.0 && *by_s <= self.duration_s && target.is_finite()),
                Criterion::Tracking { max_error, after_s, .. } => !(max_error.is_finite() && *max_error > 0.0 && after_s.is_finite() && *after_s >= 0.0 && *after_s < self.duration_s),
                Criterion::TorqueMargin { min } => !(min.is_finite() && (0.0..1.0).contains(min)),
                Criterion::WindingTemperature { margin_c } => !(margin_c.is_finite() && *margin_c >= 0.0),
                Criterion::BearingMargin { min } | Criterion::YieldMargin { min } => !min.is_finite(),
                Criterion::PartStrength { min_safety_factor } => !(min_safety_factor.is_finite() && *min_safety_factor > 0.0),
                Criterion::NoFall | Criterion::NoLimitHits => false,
            };
            if bad {
                return Err(format!("test `{}`: criteria[{i}] ({}) has an out-of-range value", self.name, c.describe()));
            }
        }
        Ok(())
    }

    /// Every commanded key, in order.
    pub fn commanded(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.trajectory.iter().flat_map(|w| w.targets.keys().cloned()).collect();
        keys.sort();
        keys.dedup();
        keys
    }

    /// What `key` is commanded to at `t` (None: the trajectory never names it).
    pub fn command_at(&self, key: &str, t: f64) -> Option<f64> {
        let mut before: Option<(f64, f64)> = None;
        for w in &self.trajectory {
            let Some(v) = w.targets.get(key) else { continue };
            if w.t <= t {
                before = Some((w.t, *v));
            } else {
                return Some(match before {
                    Some((ta, a)) => a + (t - ta) / (w.t - ta) * (v - a),
                    None => *v,
                });
            }
        }
        before.map(|(_, v)| v)
    }

    /// Every commanded key's value at `t`.
    pub fn targets_at(&self, t: f64) -> BTreeMap<String, f64> {
        self.commanded().into_iter().filter_map(|k| self.command_at(&k, t).map(|v| (k, v))).collect()
    }
}

/// The system a robot is tested in when nothing else is composed around
/// it: the model at `model_file` (relative to the system's directory `base`)
/// as a generated assembly with its own supply and ambient (as in Robot
/// mode), run with the robot assembly's own step and Newton settings.
/// Controllers, batteries and thermal parts are added like in any system.
pub fn default_system(registry: &BehaviorRegistry, base: &Path, model_file: &str, title: &str) -> Result<SystemDocument, String> {
    let parameters = BTreeMap::from([("own_supply".to_owned(), 1.0), ("own_ambient".to_owned(), 1.0)]);
    let robot = crate::system_blocks::robot_instance_with(registry, base, model_file, &parameters)?;
    let newton = crate::newton();
    let mut document = SystemDocument::new(title);
    sim_system::commands::apply(&mut document, registry, &[
        Command::SetRunSettings { run: Some(sim_system::RunSettings { integrator: sim_system::IntegratorChoice::BackwardEuler, interval: BuildOptions::default().step, absolute_tolerance: Some(newton.absolute_tolerance), relative_tolerance: Some(newton.relative_tolerance), max_iterations: Some(newton.max_iterations), rationale: "The robot assembly's own step and Newton settings (Robot mode's defaults)".into() }) },
        Command::AddInstance { at: String::new(), name: ROBOT.into(), instance: robot },
    ]).map_err(|e| e.to_string())?;
    Ok(document)
}

/// The one generated robot at the system's top level: its instance name,
/// source file, generator parameters and recorded ports.
/// The system's one generated robot at its top level: its instance name,
/// source file, generator options and port signature.
pub(crate) fn robot_of(document: &SystemDocument) -> Result<(String, String, BTreeMap<String, f64>, BTreeMap<String, PortSchema>), String> {
    let root = document.definitions.get(&document.root).ok_or("the system has no root definition")?;
    let mut robots = root.instances.iter().filter_map(|(name, i)| match &i.kind {
        InstanceKind::Generated { generator, source, ports } if generator == crate::robot_generator::NAME => Some((name, i, source, ports)),
        _ => None,
    });
    let (name, instance, source, ports) = robots.next().ok_or("the system has no robot (a generated `robot` instance at its top level) to test")?;
    if let Some((other, ..)) = robots.next() {
        return Err(format!("the system has several robots (`{name}`, `{other}`): a robot test judges one"));
    }
    let mut parameters = BTreeMap::new();
    for (key, binding) in &instance.parameters {
        match binding {
            ParameterBinding::Value { value, .. } => parameters.insert(key.clone(), *value),
            ParameterBinding::Parameter { parameter } => return Err(format!("`{name}`.{key} is bound to the system parameter `{parameter}`; give the robot's options values")),
        };
    }
    Ok((name.clone(), source.clone(), parameters, ports.clone()))
}

/// One signal the test bench drives.
struct Driven {
    /// The test's key, or the input's own name for one it only holds.
    key: String,
    terminal: Terminal,
    kind: sim_core::QuantityKind,
    /// The value when the trajectory does not name it.
    hold: f64,
    commanded: bool,
}

/// The test bench: every open signal input of the robot and every input the
/// trajectory names. Refuses a commanded input the system already drives.
fn bench(document: &SystemDocument, registry: &BehaviorRegistry, test: &Test, robot: &str, ports: &BTreeMap<String, PortSchema>, model: &sim_domain_robot::PhysicalModel) -> Result<Vec<Driven>, String> {
    let root = &document.definitions[&document.root];
    let resolver = sim_system::Resolver::new(document, registry);
    let connected = |t: &Terminal| root.nets.iter().any(|n| n.terminals.contains(t));
    let mut out: Vec<Driven> = Vec::new();
    for key in test.commanded() {
        let (terminal, kind, hold) = if let Some(PortSchema::SignalIn(kind)) = ports.get(&format!("{key}.target")) {
            (Terminal::port(robot, &format!("{key}.target")), kind.clone(), 0.0)
        } else {
            let unknown = || format!("test `{}` commands `{key}`, which is neither a driven joint of the robot ({}) nor `instance.port`, a signal input at the system's top level", test.name, ports.keys().filter_map(|p| p.strip_suffix(".target")).collect::<Vec<_>>().join(", "));
            let (instance, port) = key.split_once('.').ok_or_else(unknown)?;
            let spec = root.instances.get(instance).ok_or_else(unknown)?;
            match resolver.instance_ports(spec).map_err(|e| e.to_string())?.get(port) {
                Some(Some(PortSchema::SignalIn(kind))) => (Terminal::port(instance, port), kind.clone(), 0.0),
                _ => return Err(unknown()),
            }
        };
        if connected(&terminal) {
            return Err(format!("test `{}` commands `{key}`, but the system already drives {terminal}: a test commands only inputs the system leaves open (command the controller's own inputs instead)", test.name));
        }
        out.push(Driven { key, terminal, kind, hold, commanded: true });
    }
    // The robot's other open inputs are held: a servo target at the model's
    // control target, anything else at zero.
    for (port, schema) in ports {
        let PortSchema::SignalIn(kind) = schema else { continue };
        let terminal = Terminal::port(robot, port);
        if connected(&terminal) || out.iter().any(|d| d.terminal == terminal) {
            continue;
        }
        let hold = port.strip_suffix(".target").and_then(|joint| model.control.targets.get(joint)).copied().unwrap_or(0.0);
        out.push(Driven { key: port.clone(), terminal, kind: kind.clone(), hold, commanded: false });
    }
    Ok(out)
}

/// The test bench's block: each output is its key's commanded value at the
/// tick (a pure function of time), or its hold value.
struct Stimulus {
    interface: BlockInterface,
    test: Test,
    outputs: Vec<(String, f64, bool)>,
}
impl Stimulus {
    fn write(&self, t: f64, outputs: &mut [f64]) {
        for (out, (key, hold, commanded)) in outputs.iter_mut().zip(&self.outputs) {
            *out = if *commanded { self.test.command_at(key, t).unwrap_or(*hold) } else { *hold };
        }
    }
}
impl BlockImplementation for Stimulus {
    fn label(&self) -> String {
        format!("the test bench of `{}`", self.test.name)
    }
    fn interface(&self) -> BlockInterface {
        self.interface.clone()
    }
    fn initialize(&mut self, t: f64, _period: f64, _inputs: &[f64], outputs: &mut [f64]) -> Result<(), String> {
        self.write(t, outputs);
        Ok(())
    }
    fn step(&mut self, t: f64, _dt: f64, _inputs: &[f64], outputs: &mut [f64]) -> Result<(), String> {
        self.write(t, outputs);
        Ok(())
    }
    fn checkpoint(&self) -> Checkpoint {
        Checkpoint::Stateless
    }
}

/// How a robot test steps the compiled system, beyond the document's own
/// run settings (both are in the fingerprint).
fn harness() -> Value {
    json!({"sample_s": SAMPLE_S, "retry_halvings": 4, "grid_clock": true})
}

/// What a run of `test` on `document` (stored in `base`) would be now.
/// `extra` names further files the judgement reads (the CAD file and the
/// print registry for part strength) with their SHA-256.
pub fn fingerprint(document: &SystemDocument, base: &Path, test: &Test, extra: &BTreeMap<String, String>) -> Fingerprint {
    let mut artifacts = crate::system_evidence::artifacts(document, base);
    artifacts.extend(extra.iter().map(|(k, v)| (k.clone(), v.clone())));
    let config = crate::system_builder::config_for(document);
    Fingerprint {
        model: document.physics_hash(),
        artifacts,
        settings: crate::system_evidence::hash(&(serde_json::to_value(&config).unwrap_or(Value::Null), harness())),
        test: crate::system_evidence::hash(test),
    }
}

fn verdict_of(report: &Value) -> Verdict {
    match report["verdict"].as_str() {
        Some("passed") => Verdict::Passed,
        Some("failed") => Verdict::Failed,
        _ => Verdict::Incomplete,
    }
}

/// `report` as stale for the stated reason.
pub fn stale(report: &Value, why: &str) -> Standing {
    Standing::Stale { verdict: verdict_of(report), changed: vec![why.to_owned()] }
}

/// Whether `report` (None: never run) still describes what `now` would run.
pub fn standing(report: Option<&Value>, now: &Fingerprint) -> Standing {
    let Some(report) = report else { return Standing::NotAssessed };
    let verdict = verdict_of(report);
    match serde_json::from_value::<Fingerprint>(report["fingerprint"].clone()) {
        Ok(was) => {
            let changed = crate::system_evidence::changes(&was, now);
            if changed.is_empty() { Standing::Current { verdict } } else { Standing::Stale { verdict, changed } }
        }
        Err(_) => Standing::Stale { verdict, changed: vec!["the report records no fingerprint of what it ran".into()] },
    }
}

/// Run `test` on the system `document` stored in `base` (see the module
/// doc); `cancelled` is polled between samples and `progress` gets the
/// fraction done. The report: verdict, evidence, outcomes, the fingerprint,
/// the model's identity and assumptions, the controllers that ran, the
/// run's results (`PhysicalRobot::results`) and sampled traces.
pub fn run(document: &SystemDocument, base: &Path, registry: &BehaviorRegistry, test: &Test, extra: &BTreeMap<String, String>, cancelled: &dyn Fn() -> bool, progress: &dyn Fn(f64)) -> Result<Value, String> {
    test.validate()?;
    let fingerprint = fingerprint(document, base, test, extra);
    let (robot_name, source, parameters, ports) = robot_of(document)?;
    let model_path = base.join(&source);
    let model_text = std::fs::read_to_string(&model_path).map_err(|e| format!("{}: {e}", model_path.display()))?;
    let model_json: Value = serde_json::from_str(&model_text).map_err(|e| format!("{}: {e}", model_path.display()))?;
    let model = sim_domain_robot::PhysicalModel::parse(&model_text).map_err(|e| format!("{}: {e}", model_path.display()))?;
    // The test bench, composed like any block.
    let driven = bench(document, registry, test, &robot_name, &ports, &model)?;
    let period = model.control.period_s.max(1e-4);
    let mut timing = BlockTiming::periodic(period);
    timing.output_delay = (model.control.latency_s / period).round().max(0.0) as usize;
    let mut composed = document.clone();
    let bench_name = (0..).map(|k| if k == 0 { "test_bench".to_owned() } else { format!("test_bench_{k}") }).find(|n| !composed.definitions[&composed.root].instances.contains_key(n)).expect("a free name");
    if !driven.is_empty() {
        let interface = BlockInterface {
            inputs: Vec::new(),
            outputs: driven.iter().map(|d| BlockPort::new(d.key.clone(), d.kind.clone()).start(if d.commanded { test.command_at(&d.key, 0.0).unwrap_or(d.hold) } else { d.hold })).collect(),
            feedthrough: true,
        };
        let mut commands = vec![Command::AddInstance { at: String::new(), name: bench_name.clone(), instance: InstanceSpec::block(BlockSource::Host { name: STIMULUS.into() }, interface, timing.clone()) }];
        commands.extend(driven.iter().map(|d| Command::Connect { at: String::new(), terminals: vec![Terminal::port(&bench_name, &d.key), d.terminal.clone()], label: String::new() }));
        sim_system::commands::apply(&mut composed, registry, &commands).map_err(|e| format!("the test bench does not fit the system: {e}"))?;
    }
    let flat = sim_system::flatten_with(&composed, registry, &crate::robot_generator::generators(base)).map_err(|e| e.to_string())?;
    let assembly = flat.generated_details.get(&robot_name).and_then(|d| d.downcast_ref::<crate::robot_generator::Handle>()).and_then(|h| h.lock().unwrap_or_else(|p| p.into_inner()).take()).ok_or("the robot generator gave no assembly to measure")?;
    let config = crate::system_builder::config_for(document);
    let mut runtime = sim_compile::Runtime::new(flat.model.clone(), registry, config.integrator).map_err(|e| crate::system_builder::locate(&flat, format!("the system does not compile: {e}")))?;
    runtime.set_grid_clock(true);
    runtime.retry_halvings = 4;
    {
        let outputs: Vec<(String, f64, bool)> = driven.iter().map(|d| (d.key.clone(), d.hold, d.commanded)).collect();
        let mut hosts = crate::system_blocks::Hosts::new();
        hosts.insert(STIMULUS.into(), Box::new(|decl: &sim_core::BlockDecl| Ok(Box::new(Stimulus { interface: decl.interface.clone(), test: test.clone(), outputs: outputs.clone() }) as Box<dyn BlockImplementation>)));
        crate::system_blocks::bind_with(&mut runtime, Some(base), &mut hosts)?;
    }
    let controllers: Vec<Value> = runtime.model.blocks.iter().filter(|b| b.name != bench_name).map(|b| json!({"block": b.name, "implementation": b.implementation.describe(), "period_s": b.timing.clock.nominal_period()})).collect();
    let (opts, _) = crate::robot_generator::options("", &parameters)?;
    let mut robot = PhysicalRobot::attach(runtime, assembly, &BuildOptions { step: config.interval, ..opts })?;
    let index: BTreeMap<String, usize> = robot.joint_names.iter().enumerate().map(|(i, n)| (n.trim_start_matches("joint.").trim_start_matches("slide.").to_string(), i)).collect();
    for c in &test.criteria {
        if let Criterion::Reaches { joint, .. } | Criterion::Tracking { joint, .. } = c
            && !index.contains_key(joint)
        {
            return Err(format!("test `{}` names joint `{joint}`, which is not a driven joint of this model (its driven joints: {})", test.name, index.keys().cloned().collect::<Vec<_>>().join(", ")));
        }
    }
    // Every series has one value per sample, so none can fall out of step.
    let steps = (test.duration_s / SAMPLE_S - 1e-9).ceil().max(1.0) as usize;
    let commanded = test.commanded();
    let mut t_trace = Vec::with_capacity(steps + 1);
    let mut angles: BTreeMap<String, Vec<f64>> = index.keys().map(|k| (k.clone(), Vec::new())).collect();
    let mut commands: BTreeMap<String, Vec<f64>> = commanded.iter().map(|k| (k.clone(), Vec::new())).collect();
    let mut received: BTreeMap<String, Vec<f64>> = index.keys().filter(|j| robot.boundary_signals.contains_key(&format!("{j}.target"))).map(|k| (k.clone(), Vec::new())).collect();
    let mut sample = |robot: &PhysicalRobot, t: f64| {
        t_trace.push(t);
        let q = robot.joint_angles();
        for (name, &i) in &index {
            angles.get_mut(name).expect("indexed").push(q.get(i).copied().unwrap_or(f64::NAN));
        }
        for (key, series) in commands.iter_mut() {
            series.push(test.command_at(key, t).unwrap_or(f64::NAN));
        }
        for (joint, series) in received.iter_mut() {
            series.push(robot.runtime.get(robot.boundary_signals[&format!("{joint}.target")]));
        }
    };
    sample(&robot, 0.0);
    let mut failure = None;
    for k in 1..=steps {
        if cancelled() {
            return Err("cancelled".into());
        }
        if let Err(e) = robot.advance(SAMPLE_S) {
            failure = Some(format!("the simulation stopped at t = {:.2} s: {}", (k - 1) as f64 * SAMPLE_S, crate::system_builder::locate(&flat, e)));
            break;
        }
        sample(&robot, k as f64 * SAMPLE_S);
        if k % 10 == 0 {
            progress(k as f64 / steps as f64);
        }
    }
    let results = robot.results(&model_path.display().to_string());
    let run = Run { t: &t_trace, angles: &angles, commands: &commands, received: &received, failure: failure.as_deref() };
    let outcomes: Vec<Outcome> = test.criteria.iter().map(|c| judge(c, test, &results, &run)).collect();
    // Traces at most ~500 points, the last sample always among them.
    let thin = |v: &Vec<f64>| -> Vec<f64> {
        let stride = (v.len() / 500).max(1);
        let mut out: Vec<f64> = v.iter().step_by(stride).copied().collect();
        if v.len() > 1 && (v.len() - 1) % stride != 0 {
            out.push(v[v.len() - 1]);
        }
        out
    };
    let series = |m: &BTreeMap<String, Vec<f64>>| m.iter().map(|(k, v)| (k.clone(), json!(thin(v)))).collect::<serde_json::Map<_, _>>();
    let assumptions = model_json["source"]["assumptions"].as_array().cloned().unwrap_or_default();
    let blocking = assumptions.iter().filter(|a| a["blocking"] == json!(true)).count();
    let motor_t: Vec<f64> = results["trace"]["t"].as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
    let thin_value = |v: &Value| -> Value { json!(thin(&v.as_array().map(|a| a.iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect()).unwrap_or_default())) };
    let mut report = json!({
        "test": test, "verdict_rule": VERDICT_RULE,
        "outcomes": outcomes,
        "failure": failure,
        "fingerprint": fingerprint,
        "system": {"title": document.title, "robot": robot_name, "model_file": source, "controllers": controllers,
            "test_bench": {"block": bench_name, "commands": driven.iter().filter(|d| d.commanded).map(|d| d.terminal.to_string()).collect::<Vec<_>>(), "holds": driven.iter().filter(|d| !d.commanded).map(|d| json!({"input": d.terminal.to_string(), "value": d.hold})).collect::<Vec<_>>(), "period_s": period, "latency_s": timing.output_delay as f64 * period,
                "rule": "the test drives only signal inputs the system leaves open, at the robot model's control period and latency; controller blocks in the system run as they are"},
            "run": {"step_s": config.interval, "harness": harness()}},
        "model": {"cad_revision": model_json["source"]["cad_revision"], "cad_sha256": model_json["source"]["cad_sha256"], "archive_identity": model_json["source"]["archive_identity"], "exporter": model_json["source"]["exporter"], "file": model_json["source"]["file"]},
        "assumptions": assumptions, "blocking_assumptions": blocking,
        "not_modelled": model_json["source"]["not_modelled"],
        "sample_s": SAMPLE_S,
        "results": {"joints": results["joints"], "motors": results["motors"], "links": results["links"].as_object().map(|l| l.iter().map(|(k, v)| (k.clone(), json!({"peak_stress_pa": v["peak_stress_pa"], "yield_margin": v["yield_margin"], "max_deflection_m": v["max_deflection_m"]}))).collect::<serde_json::Map<_, _>>()), "base": {"fell": results["base"]["fell"]}, "warnings": results["warnings"], "wall_s": results["wall_s"], "steps": results["steps"], "duration_s": results["duration_s"]},
        "trace": {"t": thin(&t_trace), "joints": series(&angles), "commands": series(&commands), "targets_received": series(&received)},
        "motor_trace": {"t": thin(&motor_t), "torque_nm": results["trace"]["motors"].as_object().map(|m| m.iter().map(|(k, v)| (k.clone(), thin_value(&v["torque_nm"]))).collect::<serde_json::Map<_, _>>())},
    });
    conclude(&mut report);
    Ok(report)
}

/// [`run`] for a model on its own: `model_json` is written beside a
/// [`default_system`] in a temporary directory and tested there.
pub fn run_model(model_json: &Value, test: &Test, cancelled: &dyn Fn() -> bool, progress: &dyn Fn(f64)) -> Result<Value, String> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!("sim-acceptance-{}-{}", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let result = (|| {
        std::fs::write(dir.join("robot.simrobot.json"), model_json.to_string()).map_err(|e| e.to_string())?;
        let registry = crate::registry();
        let document = default_system(&registry, &dir, "robot.simrobot.json", &test.name)?;
        run(&document, &dir, &registry, test, &BTreeMap::new(), cancelled, progress)
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// The report's verdict, evidence and summary from its outcomes, its
/// failure and its model's blocking assumptions ([`VERDICT_RULE`]); called
/// again by a caller that judges a criterion after the run (part strength).
pub fn conclude(report: &mut Value) {
    let outcomes = report["outcomes"].as_array().cloned().unwrap_or_default();
    let status = |o: &Value| o["status"].as_str().unwrap_or("not_assessed").to_string();
    let described = |want: &str| outcomes.iter().filter(|o| status(o) == want).filter_map(|o| o["description"].as_str().map(str::to_string)).collect::<Vec<_>>();
    let failure = report["failure"].as_str().map(str::to_string);
    let verdict = if !described("fail").is_empty() || failure.is_some() {
        "failed"
    } else if outcomes.iter().all(|o| status(o) == "pass") {
        "passed"
    } else {
        "incomplete"
    };
    let blocking = report["blocking_assumptions"].as_u64().unwrap_or(0);
    let evidence = verdict == "passed" && blocking == 0;
    let summary = match (verdict, evidence) {
        ("passed", true) => format!("Passed all {} criteria.", outcomes.len()),
        ("passed", false) => format!("Passed all {} criteria, but the model has {blocking} blocking assumption(s), so this is not evidence yet.", outcomes.len()),
        ("failed", _) => format!("Failed: {}.", described("fail").into_iter().chain(failure).collect::<Vec<_>>().join("; ")),
        _ => format!("Incomplete: {} could not be assessed.", described("not_assessed").join("; ")),
    };
    report["verdict"] = json!(verdict);
    report["evidence"] = json!(evidence);
    report["summary"] = json!(summary);
}

fn pass_if(ok: bool) -> Status {
    if ok { Status::Pass } else { Status::Fail }
}

/// What a run sampled: one value per sample time in every series.
struct Run<'a> {
    t: &'a [f64],
    angles: &'a BTreeMap<String, Vec<f64>>,
    /// The test's own commands, by key.
    commands: &'a BTreeMap<String, Vec<f64>>,
    /// The servo target each joint received (from the test bench or a controller).
    received: &'a BTreeMap<String, Vec<f64>>,
    failure: Option<&'a str>,
}

/// One criterion against the run.
fn judge(c: &Criterion, test: &Test, results: &Value, run: &Run) -> Outcome {
    let out = |status, measured: Value, detail: String| Outcome { criterion: c.clone(), description: c.describe(), status, measured, detail };
    let t = run.t;
    let ended = t.last().copied().unwrap_or(0.0);
    let complete = run.failure.is_none() && ended + 1e-9 >= test.duration_s;
    let stopped = || format!("the run ended at {ended:.2} s of {} s{}", test.duration_s, run.failure.map(|f| format!(": {f}")).unwrap_or_default());
    match c {
        Criterion::Reaches { joint, target, tolerance, by_s } => {
            let Some(q) = run.angles.get(joint).filter(|q| q.len() == t.len()) else {
                return out(Status::NotAssessed, Value::Null, format!("{joint} was not sampled"));
            };
            let window: Vec<f64> = t.iter().zip(q).filter(|(ti, _)| **ti + 1e-9 >= *by_s).map(|(_, a)| (a - target).abs()).collect();
            if window.is_empty() || !complete {
                return out(Status::NotAssessed, Value::Null, format!("{}, before the window from {by_s} s was complete", stopped()));
            }
            let worst = window.iter().cloned().fold(0.0, f64::max);
            let final_angle = q.last().copied().unwrap_or(f64::NAN);
            out(pass_if(worst <= *tolerance), json!({"worst_error_rad": worst, "final_rad": final_angle}), format!("worst error after {by_s} s was {:.2}° (allowed {:.2}°); it ended at {:.1}°", worst.to_degrees(), tolerance.to_degrees(), final_angle.to_degrees()))
        }
        Criterion::Tracking { joint, max_error, after_s } => {
            // The command is the test's own when it commands the joint,
            // else the target the joint's servo received from the system.
            let (cmd, whose) = match (run.commands.get(joint), run.received.get(joint)) {
                (Some(cmd), _) => (cmd, "the test's command"),
                (None, Some(cmd)) => (cmd, "the target it received from the system's controller"),
                (None, None) => return out(Status::NotAssessed, Value::Null, format!("{joint} has no command to follow: the test does not command it and the robot offers no target input for it")),
            };
            let Some(q) = run.angles.get(joint).filter(|q| q.len() == t.len() && cmd.len() == t.len()) else {
                return out(Status::NotAssessed, Value::Null, format!("{joint} and its command were not sampled at the same instants"));
            };
            if !complete {
                return out(Status::NotAssessed, Value::Null, stopped());
            }
            let errs: Vec<f64> = t.iter().zip(q.iter().zip(cmd)).filter(|(ti, _)| **ti + 1e-9 >= *after_s).map(|(_, (a, b))| (a - b).abs()).collect();
            let worst = errs.iter().cloned().fold(0.0, f64::max);
            if errs.is_empty() || !worst.is_finite() {
                return out(Status::NotAssessed, Value::Null, format!("no finite samples of {joint} against {whose} after {after_s} s"));
            }
            out(pass_if(worst <= *max_error), json!({"worst_error_rad": worst, "samples": errs.len(), "against": whose}), format!("worst tracking error against {whose} after {after_s} s was {:.2}° over {} samples (allowed {:.2}°)", worst.to_degrees(), errs.len(), max_error.to_degrees()))
        }
        Criterion::TorqueMargin { min } => {
            let motors = results["motors"].as_object();
            let margins: Vec<(String, Option<f64>)> = motors.into_iter().flatten().map(|(k, m)| (k.clone(), m["stall_margin"].as_f64())).collect();
            if margins.is_empty() {
                return out(Status::NotAssessed, Value::Null, "the model has no motor driving a joint".into());
            }
            if let Some((name, _)) = margins.iter().find(|(_, m)| m.is_none()) {
                return out(Status::NotAssessed, Value::Null, format!("{name} has no stall torque to compare with"));
            }
            let (name, worst) = margins.iter().map(|(n, m)| (n.clone(), m.unwrap())).min_by(|a, b| a.1.total_cmp(&b.1)).expect("nonempty");
            let peak = results["motors"][&name]["peak_torque_nm"].as_f64().unwrap_or(f64::NAN);
            // When the peak happened, and the torque while holding at the end (the move's start often dominates).
            let times: Vec<f64> = results["trace"]["t"].as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
            let torque: Vec<f64> = results["trace"]["motors"][&name]["torque_nm"].as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
            let at = torque.iter().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).and_then(|(i, _)| times.get(i).copied());
            let tail: Vec<f64> = times.iter().zip(&torque).filter(|(t, _)| **t >= ended - 0.25).map(|(_, x)| x.abs()).collect();
            let holding = (!tail.is_empty()).then(|| tail.iter().sum::<f64>() / tail.len() as f64);
            let when = match (at, holding) {
                (Some(at), Some(h)) => format!("; the peak came at t = {at:.2} s, and holding at the end took {h:.3} N·m"),
                (Some(at), None) => format!("; the peak came at t = {at:.2} s"),
                _ => String::new(),
            };
            out(pass_if(worst >= *min && complete), json!({"worst_margin": worst, "motor": name, "peak_torque_nm": peak, "peak_at_s": at, "holding_torque_nm": holding}), format!("{name} used {:.0} % of its stall torque (peak {:.3} N·m), leaving {:.0} % (needed {:.0} %){when}", (1.0 - worst) * 100.0, peak, worst * 100.0, min * 100.0))
        }
        Criterion::WindingTemperature { margin_c } => {
            let motors: Vec<(String, f64, f64)> = results["motors"].as_object().into_iter().flatten().filter_map(|(k, m)| Some((k.clone(), m["winding_margin_c"].as_f64()?, m["peak_winding_c"].as_f64()?))).collect();
            let Some((name, worst, peak)) = motors.iter().cloned().min_by(|a, b| a.1.total_cmp(&b.1)) else {
                return out(Status::NotAssessed, Value::Null, "the model has no motor with a thermal model".into());
            };
            out(pass_if(worst >= *margin_c && complete), json!({"worst_margin_c": worst, "motor": name, "peak_winding_c": peak}), format!("{name}'s winding peaked at {peak:.1} °C, {worst:.1} °C below its rating (needed {margin_c} °C) over {ended:.1} s"))
        }
        Criterion::BearingMargin { min } => {
            let js: Vec<(String, f64)> = results["joints"].as_object().into_iter().flatten().filter_map(|(k, j)| Some((k.clone(), j["bearing_margin"].as_f64()?))).collect();
            let Some((name, worst)) = js.into_iter().min_by(|a, b| a.1.total_cmp(&b.1)) else {
                return out(Status::NotAssessed, Value::Null, "no joint bearing carried load".into());
            };
            out(pass_if(worst >= *min && complete), json!({"worst_margin": worst, "joint": name}), format!("{name}'s bearing margin was {worst:.2} (needed {min}); the bearing itself is estimated unless set in CAD"))
        }
        Criterion::YieldMargin { min } => {
            let ls: Vec<(String, f64)> = results["links"].as_object().into_iter().flatten().filter_map(|(k, l)| Some((k.clone(), l["yield_margin"].as_f64()?))).collect();
            let Some((name, worst)) = ls.into_iter().min_by(|a, b| a.1.total_cmp(&b.1)) else {
                return out(Status::NotAssessed, Value::Null, "no part was stress-analysed: the model's links are rigid (flexible-link export is not ported), so part strength is not assessed".into());
            };
            out(pass_if(worst >= *min && complete), json!({"worst_margin": worst, "link": name}), format!("{name}'s stress margin to yield was {worst:.2} (needed {min})"))
        }
        Criterion::PartStrength { .. } => out(Status::NotAssessed, Value::Null, "printed-part strength is checked after the run against the CAD's part meshes (a project test does this); this run had none".into()),
        Criterion::NoFall => {
            let fell = results["base"]["fell"].as_bool().unwrap_or(false);
            out(pass_if(!fell && complete), json!({"fell": fell}), if fell { "the robot tipped over".into() } else { format!("upright for {ended:.1} s") })
        }
        Criterion::NoLimitHits => {
            let hits: Vec<(String, u64)> = results["joints"].as_object().into_iter().flatten().filter_map(|(k, j)| Some((k.clone(), j["limit_hits"].as_u64()?))).filter(|(_, h)| *h > 0).collect();
            out(pass_if(hits.is_empty() && complete), json!({"limit_hits": hits.iter().map(|(k, h)| (k.clone(), json!(h))).collect::<serde_json::Map<_, _>>()}), if hits.is_empty() { "no joint passed its limits".into() } else { format!("past limits: {}", hits.iter().map(|(k, h)| format!("{k} ({h} samples)")).collect::<Vec<_>>().join(", ")) })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test() -> Test {
        serde_json::from_value(json!({
            "name": "lift", "duration_s": 3.0,
            "trajectory": [{"t": 0.0, "targets": {"shoulder": 0.0}}, {"t": 1.0, "targets": {"shoulder": 1.0}}],
            "criteria": [{"kind": "reaches", "joint": "shoulder", "target": 1.0, "tolerance": 0.05, "by_s": 2.0}, {"kind": "torque_margin", "min": 0.3}, {"kind": "yield_margin", "min": 1.0}],
        })).unwrap()
    }

    #[test]
    fn targets_interpolate_and_hold_and_tests_validate_by_name() {
        let t = test();
        t.validate().unwrap();
        assert_eq!(t.targets_at(0.5)["shoulder"], 0.5);
        assert_eq!(t.targets_at(2.5)["shoulder"], 1.0);
        let mut bad = t.clone();
        bad.criteria.clear();
        assert!(bad.validate().unwrap_err().contains("at least one criterion"));
        let mut bad = t.clone();
        bad.trajectory[1].t = 0.0;
        assert!(bad.validate().unwrap_err().contains("trajectory[1].t"));
        assert!(serde_json::from_value::<Criterion>(json!({"kind": "reaches", "joint": "a"})).is_err());
    }

    #[test]
    fn an_unassessed_margin_is_never_a_pass() {
        let t = test();
        let results = json!({"motors": {"servo": {"stall_margin": null, "peak_torque_nm": 0.1}}, "links": {"arm": {"yield_margin": null}}, "joints": {}, "base": {"fell": false}});
        let times: Vec<f64> = (0..=300).map(|k| k as f64 * 0.01).collect();
        let angles = BTreeMap::from([("shoulder".to_string(), times.iter().map(|t| t.min(1.0)).collect::<Vec<f64>>())]);
        let none = BTreeMap::new();
        let run = Run { t: &times, angles: &angles, commands: &angles, received: &none, failure: None };
        let o: Vec<Outcome> = t.criteria.iter().map(|c| judge(c, &t, &results, &run)).collect();
        assert_eq!(o[0].status, Status::Pass);
        assert_eq!(o[1].status, Status::NotAssessed);
        assert_eq!(o[2].status, Status::NotAssessed);
        assert!(o[2].detail.contains("rigid"));
        // A run that stopped early cannot pass a reach.
        let short: Vec<f64> = times[..100].to_vec();
        let a = BTreeMap::from([("shoulder".to_string(), short.clone())]);
        let run = Run { t: &short, angles: &a, commands: &a, received: &none, failure: Some("stopped") };
        assert_eq!(judge(&t.criteria[0], &t, &results, &run).status, Status::NotAssessed);
        // Nor one that ended a sample before the stated duration.
        let run = Run { t: &times[..300], angles: &BTreeMap::from([("shoulder".to_string(), angles["shoulder"][..300].to_vec())]), commands: &none, received: &none, failure: None };
        assert_eq!(judge(&t.criteria[0], &t, &results, &run).status, Status::NotAssessed);
    }

    #[test]
    fn tracking_compares_every_sample_with_its_own_command() {
        // `elbow` is first named at 1 s: it is commanded from the start (its
        // first value held), so its series stays in step with the samples.
        let t: Test = serde_json::from_value(json!({
            "name": "follow", "duration_s": 2.0,
            "trajectory": [{"t": 0.0, "targets": {"shoulder": 0.0}}, {"t": 1.0, "targets": {"shoulder": 1.0, "elbow": 0.5}}, {"t": 2.0, "targets": {"elbow": 1.5}}],
            "criteria": [{"kind": "tracking", "joint": "elbow", "max_error": 0.01, "after_s": 0.0}],
        })).unwrap();
        assert_eq!(t.commanded(), ["elbow", "shoulder"]);
        assert_eq!(t.command_at("elbow", 0.2), Some(0.5));
        assert_eq!(t.command_at("elbow", 1.5), Some(1.0));
        assert_eq!(t.command_at("shoulder", 1.7), Some(1.0));
        assert_eq!(t.command_at("wrist", 1.0), None);
        let times: Vec<f64> = (0..=200).map(|k| k as f64 * 0.01).collect();
        let command: Vec<f64> = times.iter().map(|x| t.command_at("elbow", *x).unwrap()).collect();
        let results = json!({});
        let none = BTreeMap::new();
        let commands = BTreeMap::from([("elbow".to_string(), command.clone())]);
        // The joint follows until 1.5 s, then stops: the error grows to 0.5 rad at the end.
        let angles = BTreeMap::from([("elbow".to_string(), times.iter().map(|x| t.command_at("elbow", x.min(1.5)).unwrap()).collect::<Vec<f64>>())]);
        let o = judge(&t.criteria[0], &t, &results, &Run { t: &times, angles: &angles, commands: &commands, received: &none, failure: None });
        assert_eq!(o.status, Status::Fail);
        assert!((o.measured["worst_error_rad"].as_f64().unwrap() - 0.5).abs() < 1e-9, "{}", o.measured);
        assert_eq!(o.measured["samples"], 201);
        // A command series shorter than the run is never compared sample for sample.
        let cut = BTreeMap::from([("elbow".to_string(), command[..150].to_vec())]);
        let o = judge(&t.criteria[0], &t, &results, &Run { t: &times, angles: &angles, commands: &cut, received: &none, failure: None });
        assert_eq!(o.status, Status::NotAssessed);
        // Not commanded by the test: judged against the target a controller sent.
        let o = judge(&t.criteria[0], &t, &results, &Run { t: &times, angles: &angles, commands: &none, received: &commands, failure: None });
        assert_eq!(o.status, Status::Fail);
        assert!(o.detail.contains("controller"), "{}", o.detail);
        let o = judge(&t.criteria[0], &t, &results, &Run { t: &times, angles: &angles, commands: &none, received: &none, failure: None });
        assert_eq!(o.status, Status::NotAssessed);
    }
}
