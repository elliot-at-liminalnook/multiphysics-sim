//! Lesson features that read the model: parameter values and their
//! provenance, inline `{{…}}` values, `sim-equation` values and checks,
//! `sim-measured` comparisons, varied questions confirmed against the
//! simulation, and `sim-task` sandboxes judged by their runs.
//!
//! [`Model`] caches compiled systems and scene runs for one lesson so the
//! checker and the viewer's background jobs resolve everything once.
use crate::lesson::{self, CheckResult, SceneRun, instance_paths, load_system, scene_document, scene_run};
use crate::system_builder::{self, Series};
use serde::Serialize;
use sim_core::BehaviorRegistry;
use sim_inspect::{ParameterValue, Provenance, SystemDescription};
use sim_lesson::blocks::{Equation, Measured, MeasuredData, Task};
use sim_lesson::refs::{InlineRef, Source};
use sim_lesson::{Lesson, Scene};
use sim_system::SystemDocument;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;

pub struct Model<'a> {
    pub lesson: &'a Lesson,
    pub registry: &'a BehaviorRegistry,
    pub use_cache: bool,
    systems: RefCell<BTreeMap<String, Result<(SystemDocument, SystemDescription), String>>>,
    runs: RefCell<BTreeMap<String, Result<SceneRun, String>>>,
}

impl<'a> Model<'a> {
    pub fn new(lesson: &'a Lesson, registry: &'a BehaviorRegistry, use_cache: bool) -> Self {
        Self { lesson, registry, use_cache, systems: Default::default(), runs: Default::default() }
    }

    /// A system's document and compiled description (cached).
    pub fn system(&self, name: &str) -> Result<(SystemDocument, SystemDescription), String> {
        if let Some(r) = self.systems.borrow().get(name) {
            return r.clone();
        }
        let result = load_system(&self.lesson.system_path(name), self.registry).and_then(|doc| {
            let compiled = system_builder::compile(&doc, self.registry, system_builder::config_for(&doc))?;
            Ok((doc, compiled.description))
        });
        self.systems.borrow_mut().insert(name.to_string(), result.clone());
        result
    }

    /// `system/instance/path.parameter` → its value, unit and provenance.
    pub fn parameter(&self, target: &str) -> Result<ParameterValue, String> {
        let (path, name) = target.rsplit_once('.').ok_or_else(|| format!("`{target}`: write instance/path.parameter"))?;
        let link = self.lesson.resolve_part(path)?;
        let (_, description) = self.system(&link.system)?;
        let component = description.components.get(&link.path).ok_or_else(|| format!("`{target}`: no instance `{}` in system `{}`", link.path, link.system))?;
        component.parameters.get(name).cloned().ok_or_else(|| format!("`{target}`: `{}` has no parameter `{name}` ({})", link.path, component.parameters.keys().cloned().collect::<Vec<_>>().join(", ")))
    }

    /// A scene's recorded run (cached; recorded when missing).
    pub fn run(&self, scene_id: &str) -> Result<SceneRun, String> {
        if let Some(r) = self.runs.borrow().get(scene_id) {
            return r.clone();
        }
        let result = (|| {
            let scene = self.lesson.scene(scene_id).ok_or_else(|| format!("no scene `{scene_id}`"))?;
            let scene = lesson::lesson_scene(self.lesson, scene);
            let (doc, _) = self.system(&scene.system)?;
            let doc = scene_document(&doc, self.registry, &scene)?;
            let timeline = self.lesson.timeline(&scene)?;
            scene_run(&doc, self.registry, &scene, &timeline, self.use_cache, None, &|_| {})
        })();
        self.runs.borrow_mut().insert(scene_id.to_string(), result.clone());
        result
    }

    /// The value an inline reference stands for (SI).
    pub fn value(&self, source: &Source) -> Result<f64, String> {
        match source {
            Source::Param { target } => self.parameter(target).map(|p| p.value),
            Source::Data { block, field, at } => {
                let m = self.lesson.measured(block).ok_or_else(|| format!("no sim-measured `{block}`"))?;
                let data = sim_lesson::blocks::load_data(&self.lesson.dir().join(&m.data))?;
                let values: Vec<f64> = data.points.iter().filter(|p| p.get(field).is_some_and(|x| (x - at).abs() < 1e-9)).filter_map(|p| p.get(&m.y.field).copied()).collect();
                if values.is_empty() {
                    return Err(format!("{}: no point with {field} = {at}", m.data));
                }
                Ok(values.iter().sum::<f64>() / values.len() as f64)
            }
            Source::Value { scene, observe, reduce, window } => {
                let run = self.run(scene)?;
                let series = run.series(observe).ok_or_else(|| format!("scene `{scene}` records no `{observe}`"))?;
                crate::system_study::reduce(series, &sim_system::Metric { label: observe.clone(), observable: observe.clone(), reduce: *reduce, window: *window }).ok_or_else(|| format!("`{observe}` has no samples in the window"))
            }
        }
    }

    /// Every inline reference with its value (or why it has none).
    pub fn inline_values(&self) -> Vec<(usize, InlineRef, Result<f64, String>)> {
        self.lesson.inline_refs().into_iter().map(|(line, r)| {
            let v = self.value(&r.source);
            (line, r, v)
        }).collect()
    }

    /// A question's `given` model values, by name.
    pub fn given(&self, q: &sim_lesson::quiz::Quiz) -> Result<BTreeMap<String, f64>, String> {
        q.given.iter().map(|(name, target)| Ok((name.clone(), self.parameter(target)?.value))).collect()
    }

    /// Fixed terms of an equation (parameters and values), by term name.
    pub fn equation_constants(&self, e: &Equation) -> Result<BTreeMap<String, f64>, String> {
        let mut out = BTreeMap::new();
        for (name, t) in &e.terms {
            if let Some(p) = &t.param {
                out.insert(name.clone(), self.parameter(p)?.value);
            } else if let Some(v) = t.value {
                out.insert(name.clone(), v);
            }
        }
        Ok(out)
    }
}

/// An equation with its values at one moment.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct EquationView {
    /// (symbol, value in SI, unit) per term, in expression order.
    pub terms: Vec<(String, f64, String)>,
    pub result: f64,
    /// `τ = k·i = 0.012 N·m/A × 3.29 A = 39.5 mN·m`.
    pub text: String,
}

fn at(series: &Series, t: f64) -> Option<f64> {
    let i = series.times.partition_point(|x| *x <= t);
    if series.times.is_empty() {
        return None;
    }
    if i == 0 {
        return series.values.first().copied();
    }
    if i >= series.times.len() {
        return series.values.last().copied();
    }
    let (t0, t1) = (series.times[i - 1], series.times[i]);
    let f = if t1 > t0 { (t - t0) / (t1 - t0) } else { 0. };
    Some(series.values[i - 1] + f * (series.values[i] - series.values[i - 1]))
}

/// The equation evaluated at time `t` of `run` (observed terms), with the
/// fixed terms from [`Model::equation_constants`].
pub fn equation_at(e: &Equation, constants: &BTreeMap<String, f64>, run: Option<&SceneRun>, t: f64) -> Result<EquationView, String> {
    let mut values = constants.clone();
    for (name, term) in &e.terms {
        if let Some(o) = &term.observe {
            let run = run.ok_or("the scene has not been recorded yet")?;
            let s = run.series(o).ok_or_else(|| format!("the run records no `{o}`"))?;
            values.insert(name.clone(), at(s, t).ok_or_else(|| format!("`{o}` has no samples"))?);
        }
    }
    let result = sim_script::expr::eval(&e.expr, &values)?;
    let order = sim_script::expr::variables(&e.expr);
    let fmt = |v: f64, unit: &str| sim_lesson::units::format_si(v, unit, 3);
    let substituted = sim_script::expr::substitute(&e.expr, |n| e.terms.get(n).map(|t| fmt(values[n], &t.unit)).unwrap_or_else(|| n.to_string()));
    let show = e.show.clone().unwrap_or_else(|| format!("{} = {}", e.result.symbol, sim_script::expr::substitute(&e.expr, |n| e.terms.get(n).map(|t| t.symbol.clone()).unwrap_or_else(|| n.to_string()))));
    let text = format!("{show} = {substituted} = {}", fmt(result, &e.result.unit));
    Ok(EquationView { terms: order.iter().filter_map(|n| e.terms.get(n).map(|t| (t.symbol.clone(), values[n], t.unit.clone()))).collect(), result, text })
}

/// Does the equation hold on the run? Compares its result with
/// `holds.observe` at up to 200 times in the window.
pub fn check_equation(model: &Model, e: &Equation) -> Result<CheckResult, String> {
    let holds = e.holds.as_ref().ok_or("no `holds`")?;
    let scene = e.scene.as_ref().ok_or("no scene")?;
    let run = model.run(scene)?;
    let constants = model.equation_constants(e)?;
    let observed = run.series(&holds.observe).ok_or_else(|| format!("the run records no `{}`", holds.observe))?;
    let [a, b] = holds.window.unwrap_or([observed.times.first().copied().unwrap_or(0.), observed.times.last().copied().unwrap_or(0.)]);
    let times: Vec<f64> = observed.times.iter().copied().filter(|t| *t >= a && *t <= b).collect();
    let stride = (times.len() / 200).max(1);
    let mut worst: (f64, f64) = (0., 0.);
    let mut scale: f64 = 0.;
    for &t in times.iter().step_by(stride) {
        let v = equation_at(e, &constants, Some(&run), t)?;
        let o = at(observed, t).unwrap_or(f64::NAN);
        scale = scale.max(o.abs());
        if (v.result - o).abs() > worst.0 {
            worst = ((v.result - o).abs(), t);
        }
    }
    let allowed = match holds.tolerance {
        Some(sim_lesson::quiz::Tolerance::Absolute(v)) => v,
        Some(sim_lesson::quiz::Tolerance::Relative(r)) => r * scale,
        None => 0.02 * scale,
    };
    let passed = worst.0 <= allowed + 1e-12 && !times.is_empty();
    Ok(CheckResult {
        observe: holds.observe.clone(),
        reduce: sim_system::Reduce::Max,
        window: Some([a, b]),
        value: Some(worst.0),
        min: None,
        max: Some(allowed),
        passed,
        why: format!("{} holds on the run", e.show.clone().unwrap_or_else(|| e.expr.clone())),
        message: format!("largest gap between {} and {} is {:.4} at {:.3} s (allowed {:.4})", e.expr, holds.observe, worst.0, worst.1, allowed),
    })
}

/// One measured point beside the simulation of it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MeasuredPoint {
    pub x: f64,
    pub measured: f64,
    pub simulated: f64,
}
/// A parameter's origin, for the labels next to a measured comparison.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ParameterOrigin {
    pub target: String,
    pub value: f64,
    pub unit: String,
    /// measured, derived, estimated or unspecified.
    pub kind: String,
    pub detail: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct MeasuredReport {
    pub id: String,
    pub data: MeasuredData,
    pub points: Vec<MeasuredPoint>,
    pub rms: f64,
    pub max_gap: f64,
    pub origins: Vec<ParameterOrigin>,
    /// Some model parameters were fitted to this very data: agreement shows
    /// the fit, not an independent prediction.
    pub fitted_to_data: bool,
    pub checks: Vec<CheckResult>,
    pub fidelity: String,
}

fn origin(target: String, p: &ParameterValue) -> ParameterOrigin {
    let (kind, detail) = match &p.provenance {
        Provenance::Measured { source } => ("measured", format!("{}", source.path)),
        Provenance::Derived { rule, .. } => ("derived", rule.clone()),
        Provenance::Estimated { explanation } => ("estimated", explanation.clone()),
        Provenance::Unspecified => ("unspecified", String::new()),
    };
    ParameterOrigin { target, value: p.value, unit: p.unit.clone().unwrap_or_default(), kind: kind.into(), detail }
}

/// Where every parameter of a system comes from, as its document records
/// it (bound values with provenance, by instance path).
pub fn origins(doc: &SystemDocument) -> Vec<ParameterOrigin> {
    fn walk(doc: &SystemDocument, def: &str, prefix: &str, out: &mut Vec<ParameterOrigin>, depth: usize) {
        let Some(d) = doc.definitions.get(def).filter(|_| depth < 64) else { return };
        for (name, i) in &d.instances {
            let path = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            for (k, b) in &i.parameters {
                if let sim_system::ParameterBinding::Value { value, unit, provenance: Some(p), uncertainty } = b {
                    if !matches!(p, Provenance::Unspecified) {
                        out.push(origin(format!("{path}.{k}"), &ParameterValue { value: *value, unit: unit.clone(), provenance: p.clone(), uncertainty: *uncertainty }));
                    }
                }
            }
            if let sim_system::InstanceKind::Subsystem { definition } = &i.kind {
                walk(doc, definition, &path, out, depth + 1);
            }
        }
    }
    let mut out = Vec::new();
    walk(doc, &doc.root, "", &mut out, 0);
    out
}

/// Simulate every measured point (distinct inputs once) and compare.
pub fn measured(model: &Model, m: &Measured) -> Result<MeasuredReport, String> {
    let data_path = model.lesson.dir().join(&m.data);
    let data = sim_lesson::blocks::load_data(&data_path)?;
    let points = data.select(m);
    if points.is_empty() {
        return Err(format!("{}: no points with {} and {} (after `only`)", m.data, m.x.field, m.y.field));
    }
    let (doc, description) = model.system(&m.system)?;
    let mut sims: BTreeMap<String, f64> = BTreeMap::new();
    let mut out = Vec::new();
    let mut fidelity = String::new();
    for (x, y, vars) in &points {
        let mut set = BTreeMap::new();
        for (k, e) in &m.set {
            set.insert(k.clone(), sim_script::expr::eval(e, vars).map_err(|err| format!("set {k}: {err}"))?);
        }
        let key = serde_json::to_string(&set).unwrap_or_default();
        let simulated = match sims.get(&key) {
            Some(v) => *v,
            None => {
                let scene: Scene = serde_norway::from_str(&format!("id: {}\nsystem: {}", m.id, m.system)).map_err(|e| e.to_string())?;
                let scene = Scene { set: set.clone(), run: m.run.clone(), plots: vec![m.y.observe.clone()], ..scene };
                let sdoc = scene_document(&doc, model.registry, &scene)?;
                let timeline = sim_script::presentation::Timeline::new(vec![])?;
                let run = scene_run(&sdoc, model.registry, &scene, &timeline, model.use_cache, None, &|_| {})?;
                if let Some(e) = &run.error {
                    return Err(format!("point {} = {x}: run stopped: {e}", m.x.field));
                }
                fidelity = run.fidelity.clone();
                let series = run.series(&m.y.observe).ok_or_else(|| format!("the run records no `{}`", m.y.observe))?;
                let v = crate::system_study::reduce(series, &sim_system::Metric { label: m.y.observe.clone(), observable: m.y.observe.clone(), reduce: m.y.reduce, window: m.y.window }).ok_or("no samples in the window")?;
                sims.insert(key, v);
                v
            }
        };
        out.push(MeasuredPoint { x: *x, measured: *y, simulated });
    }
    let gaps: Vec<f64> = out.iter().map(|p| p.simulated - p.measured).collect();
    let rms = (gaps.iter().map(|g| g * g).sum::<f64>() / gaps.len() as f64).sqrt();
    let max_gap = gaps.iter().fold(0f64, |a, g| a.max(g.abs()));
    let mut origins = origins(&doc);
    // Units the document leaves implicit are the components' declared ones.
    for o in origins.iter_mut().filter(|o| o.unit.is_empty()) {
        if let Some((path, name)) = o.target.rsplit_once('.') {
            if let Some(u) = description.components.get(path).and_then(|c| c.parameters.get(name)).and_then(|p| p.unit.clone()) {
                o.unit = u;
            }
        }
    }
    let file = std::path::Path::new(&m.data).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
    let fitted_to_data = origins.iter().any(|o| o.kind == "measured" && o.detail.ends_with(&file));
    let mut checks = Vec::new();
    let claim = |label: &str, value: f64, limit: Option<f64>| limit.map(|l| CheckResult { observe: m.y.observe.clone(), reduce: sim_system::Reduce::Max, window: m.y.window, value: Some(value), min: None, max: Some(l), passed: value <= l, why: format!("{label} between the measured points and the simulation"), message: format!("{label} = {value:.4} {} (allowed {l})", m.y.unit) });
    checks.extend(claim("RMS gap", rms, m.max_rms));
    checks.extend(claim("largest gap", max_gap, m.max_gap));
    Ok(MeasuredReport { id: m.id.clone(), data, points: out, rms, max_gap, origins, fitted_to_data, checks, fidelity })
}

/// A varied question's formula against the simulation at the corners of
/// its ranges: (values, formula answer, simulated).
pub fn check_variant(model: &Model, q: &sim_lesson::quiz::Quiz) -> Result<Vec<(BTreeMap<String, f64>, f64, f64)>, String> {
    let with = q.check_with.as_ref().ok_or("no check_with")?;
    let scene = model.lesson.scene(&with.scene).ok_or_else(|| format!("no scene `{}`", with.scene))?;
    let (doc, _) = model.system(&scene.system)?;
    let given = model.given(q)?;
    let corners: Vec<BTreeMap<String, f64>> = [true, false].into_iter().map(|low| {
        let mut v: BTreeMap<String, f64> = q.vary.iter().map(|(k, r)| (k.clone(), if low { r.min } else { r.max })).collect();
        v.extend(given.clone());
        v
    }).collect();
    let mut out = Vec::new();
    for values in corners {
        let formula = q.correct_answer(&values)?.ok_or("no answer")?;
        let mut s = lesson::lesson_scene(model.lesson, scene);
        for (k, e) in &with.set {
            s.set.insert(k.clone(), sim_script::expr::eval(e, &values)?);
        }
        if !s.plots.contains(&with.observe) {
            s.plots.push(with.observe.clone());
        }
        let sdoc = scene_document(&doc, model.registry, &s)?;
        let timeline = model.lesson.timeline(&s)?;
        let run = scene_run(&sdoc, model.registry, &s, &timeline, model.use_cache, None, &|_| {})?;
        let series = run.series(&with.observe).ok_or_else(|| format!("the run records no `{}`", with.observe))?;
        let simulated = crate::system_study::reduce(series, &sim_system::Metric { label: with.observe.clone(), observable: with.observe.clone(), reduce: with.reduce.unwrap_or(sim_system::Reduce::Final), window: with.window }).ok_or("no samples")?;
        out.push((values, formula, simulated));
    }
    Ok(out)
}

/// Where a task's sandbox lives (`runs/lessons/sandbox/<lesson>/<task>/`).
pub fn task_dir(lesson: &Lesson, task: &Task) -> PathBuf {
    lesson::sandbox_root().join(&lesson.slug).join(format!("task-{}", task.id))
}

/// The scene a task runs: the lesson scene with the task's starting values
/// (`extra` on top), recording its claims and metrics; its ID is `task-<id>`.
pub fn task_scene(lesson: &Lesson, task: &Task, extra: &BTreeMap<String, f64>) -> Result<Scene, String> {
    let scene = lesson.scene(&task.scene).ok_or_else(|| format!("no scene `{}`", task.scene))?;
    let mut s = lesson::lesson_scene(lesson, scene);
    s.set.extend(task.start.iter().map(|(k, v)| (k.clone(), *v)));
    s.set.extend(extra.iter().map(|(k, v)| (k.clone(), *v)));
    // Claims and metrics are recorded with the run.
    for o in task.win.iter().map(|e| &e.observe).chain(task.report.iter().map(|m| &m.observe)) {
        if !s.plots.contains(o) {
            s.plots.push(o.clone());
        }
    }
    s.id = format!("task-{}", task.id);
    // The task is done in the builder: no sliders, challenge or companion here.
    s.sliders.clear();
    s.challenge = None;
    s.companion = None;
    s.hints.clear();
    Ok(s)
}

/// The learner's copy for a task (made from the scene's system with the
/// task's starting values; `reset` starts over).
pub fn task_sandbox(lesson: &Lesson, task: &Task, registry: &BehaviorRegistry, reset: bool) -> Result<lesson::Sandbox, String> {
    let scene = task_scene(lesson, task, &BTreeMap::new())?;
    lesson::sandbox(lesson, &scene, registry, reset)
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskResult {
    pub passed: bool,
    pub checks: Vec<CheckResult>,
    /// (label, value, unit) for the task's report.
    pub metrics: Vec<(String, f64, String)>,
    pub design: String,
    pub error: Option<String>,
}

fn judge(lesson: &Lesson, task: &Task, doc: &SystemDocument, scene: &Scene, registry: &BehaviorRegistry, use_cache: bool) -> Result<TaskResult, String> {
    let timeline = lesson.timeline(scene)?;
    let sdoc = scene_document(doc, registry, scene)?;
    let run = scene_run(&sdoc, registry, scene, &timeline, use_cache, None, &|_| {})?;
    let checks = lesson::check_claims(&run, &task.win);
    let metrics = task.report.iter().filter_map(|m| {
        let s = run.series(&m.observe)?;
        let v = crate::system_study::reduce(s, &sim_system::Metric { label: m.label.clone(), observable: m.observe.clone(), reduce: m.reduce, window: m.window })?;
        Some((m.label.clone(), v, m.unit.clone()))
    }).collect();
    let passed = run.error.is_none() && checks.iter().all(|c| c.passed);
    let mut d = sdoc.clone();
    d.discussions = Default::default();
    Ok(TaskResult { passed, checks, metrics, design: d.content_hash(), error: run.error.clone() })
}

/// Run the learner's sandbox for a task and judge it.
pub fn judge_task(lesson: &Lesson, task: &Task, registry: &BehaviorRegistry) -> Result<TaskResult, String> {
    let sb = task_sandbox(lesson, task, registry, false)?;
    let doc = load_system(&sb.path, registry)?;
    let scene = lesson::sandbox_scene(&task_scene(lesson, task, &BTreeMap::new())?);
    judge(lesson, task, &doc, &scene, registry, true)
}

/// For `check`: the task's start must not meet the goal, and start plus
/// `solution` must (when a solution is given).
pub fn prove_task(model: &Model, task: &Task) -> Result<(TaskResult, Option<TaskResult>), String> {
    let scene = model.lesson.scene(&task.scene).ok_or_else(|| format!("no scene `{}`", task.scene))?;
    let (doc, _) = model.system(&scene.system)?;
    let start = judge(model.lesson, task, &doc, &task_scene(model.lesson, task, &BTreeMap::new())?, model.registry, model.use_cache)?;
    let solved = if task.solution.is_empty() { None } else { Some(judge(model.lesson, task, &doc, &task_scene(model.lesson, task, &task.solution)?, model.registry, model.use_cache)?) };
    Ok((start, solved))
}

/// Paths a task names that are not instances.
pub fn task_paths(doc: &SystemDocument, task: &Task) -> Vec<String> {
    let paths = instance_paths(doc);
    task.start.keys().chain(task.solution.keys()).filter_map(|k| {
        let (at, name, _) = sim_script::presentation::split_parameter(k).ok()?;
        let p = if at.is_empty() { name } else { format!("{at}/{name}") };
        (!paths.contains(&p)).then(|| format!("`{k}`: no instance `{p}`"))
    }).collect()
}
