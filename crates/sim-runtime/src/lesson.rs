//! Lessons on the shared runtime: resolve a lesson's systems, record each
//! scene's run on the same `SystemSession` the viewers use, check the
//! lesson's `expect` claims against that run, and run its comparisons as
//! saved studies. Nothing here has physics of its own.
//!
//! A scene run is deterministic: the system document (with the scene's `set`
//! overrides applied), the scene's timeline (script + YAML cues), the run
//! settings and the fidelity choice are hashed into a cache key. Runs are
//! cached under `runs/lessons/cache/<key>.json` (override with
//! `SIM_LESSON_CACHE`) so the viewer can replay and scrub instantly and CI
//! can reuse a run when nothing changed.
use crate::system_builder::{self, Series};
use crate::system_session::{Command as SessionCommand, SystemSession};
use serde::{Deserialize, Serialize};
use sim_core::BehaviorRegistry;
use sim_inspect::SampleFrame;
use sim_lesson::{BlockKind, Compare, Fidelity, Lesson, Scene};
use sim_script::presentation::{Action, Timeline, split_parameter};
use sim_system::{Command, ParameterBinding, Resolver, SystemDocument, SystemStore};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Bumped whenever the recorded format or run semantics change.
pub const VERSION: u32 = 2;

pub fn cache_dir() -> PathBuf {
    std::env::var_os("SIM_LESSON_CACHE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("runs/lessons/cache"))
}

/// Load and validate a system file (same checks as the builder).
pub fn load_system(path: &Path, registry: &BehaviorRegistry) -> Result<SystemDocument, String> {
    if !path.is_file() {
        return Err(format!("{}: no such system file", path.display()));
    }
    SystemStore::new(path).load_valid(registry).map_err(|e| format!("{}: {e}", path.display()))
}

/// A scene as the lesson runs it: quantities its prediction questions ask
/// about are recorded and charted too.
pub fn lesson_scene(lesson: &Lesson, scene: &Scene) -> Scene {
    let mut s = scene.clone();
    for (_, q) in lesson.quizzes().filter(|(_, q)| q.scene.as_deref() == Some(scene.id.as_str())) {
        if let Some(o) = &q.observe {
            if !s.plots.contains(o) {
                s.plots.push(o.clone());
            }
        }
    }
    let plots = s.plots.clone();
    let mut record = |o: &String| {
        if !plots.contains(o) && !s.record.contains(o) {
            s.record.push(o.clone());
        }
    };
    // Equations read their observed terms at the playhead; prose values reduce them.
    for (_, e) in lesson.equations().filter(|(_, e)| e.scene.as_deref() == Some(scene.id.as_str())) {
        for o in e.terms.values().filter_map(|t| t.observe.as_ref()).chain(e.result.observe.iter()).chain(e.holds.iter().map(|h| &h.observe)) {
            record(o);
        }
    }
    for (_, r) in lesson.inline_refs() {
        if let sim_lesson::refs::Source::Value { scene: id, observe, .. } = &r.source {
            if id == &scene.id {
                record(observe);
            }
        }
    }
    s
}



/// The simulated value a numeric prediction is compared with.
pub fn predicted_value(run: &SceneRun, quiz: &sim_lesson::quiz::Quiz) -> Option<f64> {
    let observe = quiz.observe.as_ref()?;
    let metric = sim_system::Metric { label: observe.clone(), observable: observe.clone(), reduce: quiz.reduce.unwrap_or(sim_system::Reduce::Final), window: quiz.window };
    run.series(observe).and_then(|s| crate::system_study::reduce(s, &metric))
}

/// One `SetParameter` command for `instance/path.parameter = value`.
pub fn set_command(key: &str, value: f64) -> Result<Command, String> {
    let (at, name, parameter) = split_parameter(key)?;
    Ok(Command::SetParameter { at, name, parameter, binding: Some(ParameterBinding::value(value)) })
}

/// The document a scene runs: the system with the scene's `set` overrides,
/// in the fidelity the scene asks for.
pub fn scene_document(document: &SystemDocument, registry: &BehaviorRegistry, scene: &Scene) -> Result<SystemDocument, String> {
    let mut doc = document.clone();
    doc.studies.clear();
    doc.discussions = Default::default();
    let commands = scene.set.iter().map(|(k, v)| set_command(k, *v)).collect::<Result<Vec<_>, _>>()?;
    if !commands.is_empty() {
        sim_system::apply(&mut doc, registry, &commands).map_err(|e| format!("set: {e}"))?;
    }
    if scene.fidelity == Fidelity::Realtime {
        if doc.realtime.is_none() {
            return Err("fidelity: realtime, but the system has no realtime profile (run `sim-system realtime FILE --publish`)".into());
        }
        doc = sim_system::profile::realtime(&doc, registry).map_err(|e| format!("realtime profile: {e}"))?;
    }
    Ok(doc)
}

/// Human label for the model and settings a run used.
pub fn fidelity_label(document: &SystemDocument, fidelity: Fidelity) -> String {
    let config = system_builder::config_for(document);
    let method = match config.integrator {
        sim_dynamics::Integrator::BackwardEuler(_) => "backward Euler",
        _ => "implicit midpoint",
    };
    let step = if config.interval >= 1e-3 { format!("{:.3} ms", config.interval * 1e3) } else { format!("{:.0} µs", config.interval * 1e6) };
    let model = match fidelity {
        Fidelity::Detailed => "Detailed model",
        Fidelity::Realtime => "Realtime profile",
    };
    format!("{model} · {method} · {} step", step.trim_end_matches('0').trim_end_matches('.'))
}

/// Cache key of a scene run.
pub fn run_key(document: &SystemDocument, scene: &Scene, timeline: &Timeline) -> String {
    let mut doc = document.clone();
    doc.revision = 0;
    let value = serde_json::json!({
        "version": VERSION,
        "document": sim_system::display::scene_hash(&doc),
        "run": scene.run,
        "fidelity": scene.fidelity,
        "timeline": timeline,
        "plots": scene.plots,
        "phase": scene.phase,
        "expect": scene.expect.iter().map(|e| &e.observe).chain(scene.challenge.iter().flat_map(|c| c.win.iter().map(|e| &e.observe))).collect::<Vec<_>>(),
    });
    let mut value = value;
    if !scene.record.is_empty() {
        value["record"] = serde_json::json!(scene.record);
    }
    // Authored parts (`library/parts/*.part`) are equations outside the
    // document: editing one must not replay an older recording.
    let parts = part_hashes(document);
    if !parts.is_empty() {
        value["parts"] = serde_json::json!(parts);
    }
    blake3::hash(&serde_json::to_vec(&value).unwrap_or_default()).to_hex()[..24].to_string()
}

/// A hash of each authored part's loaded definition that the document uses.
fn part_hashes(document: &SystemDocument) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    for d in document.definitions.values() {
        for i in d.instances.values() {
            if let sim_system::InstanceKind::Element { component_type } = &i.kind {
                if let Some(def) = sim_parts::definition(component_type) {
                    out.entry(component_type.clone()).or_insert_with(|| blake3::hash(format!("{def:?}").as_bytes()).to_hex()[..16].to_string());
                }
            }
        }
    }
    out
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub observe: String,
    pub reduce: sim_system::Reduce,
    pub window: Option<[f64; 2]>,
    pub value: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub passed: bool,
    pub why: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneRun {
    pub version: u32,
    pub key: String,
    pub scene: String,
    pub fidelity: String,
    pub duration_s: f64,
    /// Description the frames belong to (the compiled scene document).
    pub description_id: String,
    /// Animation frames at the scene's frame rate (subscribed values only).
    pub frames: Vec<SampleFrame>,
    /// Every recorded observable at step resolution (plots, checks, bindings).
    pub series: Vec<Series>,
    pub checks: Vec<CheckResult>,
    /// Physics cues that were applied, with the time they took effect.
    pub applied: Vec<(f64, String, f64)>,
    /// The run stopped early (frames up to the failure are kept).
    pub error: Option<String>,
    pub wall_seconds: f64,
}
impl SceneRun {
    /// How this run plays on screen under the shared pacing rules
    /// ([`sim_script::pacing`]): reading and looking holds, plus automatic
    /// slow motion where the scene's plotted quantities change fast.
    pub fn pacing(&self, scene: &Scene, timeline: &Timeline) -> Result<sim_script::pacing::PlaybackPlan, String> {
        self.pacing_with(scene, timeline, &Default::default())
    }
    /// As [`SceneRun::pacing`] under other rules (e.g. free exploration: no reading holds).
    pub fn pacing_with(&self, scene: &Scene, timeline: &Timeline, rules: &sim_script::pacing::PacingRules) -> Result<sim_script::pacing::PlaybackPlan, String> {
        let series: Vec<_> = plot_keys(scene, timeline).iter().filter_map(|k| self.series(k)).map(|s| (s.times.as_slice(), s.values.as_slice())).collect();
        let duration = self.frames.last().map(|f| f.time).unwrap_or(self.duration_s).max(1e-9);
        sim_script::pacing::PlaybackPlan::new(timeline, duration, &series, rules)
    }
    /// Latest frame at or before `t` (the first frame before the run starts).
    pub fn frame_at(&self, t: f64) -> Option<&SampleFrame> {
        let i = self.frames.partition_point(|f| f.time <= t + 1e-12);
        self.frames.get(i.saturating_sub(1))
    }
    /// The frame shown at `t` for smooth playback: the latest recorded
    /// frame, with every value that has a step-resolution series replaced by
    /// that series interpolated linearly at `t` (between solver steps, not
    /// between the coarser animation frames).
    pub fn frame_interpolated(&self, t: f64) -> Option<SampleFrame> {
        let mut frame = self.frame_at(t)?.clone();
        for (id, value) in frame.values.iter_mut() {
            // Values not yet sampled in this frame (the first frame, before
            // the first step) are filled too wherever the series covers `t`:
            // otherwise slow motion over the first frame shows a frozen view.
            let Some(s) = self.series.iter().find(|s| &s.observable == id) else { continue };
            if let Some(v) = interpolate(&s.times, &s.values, t) {
                *value = sim_inspect::SampleValue::Committed { value: v, sample_time: t };
            }
        }
        frame.time = t;
        Some(frame)
    }
    pub fn series(&self, key: &str) -> Option<&Series> {
        self.series.iter().find(|s| s.label == key || s.observable == key).or_else(|| self.series.iter().find(|s| s.label.contains(key)))
    }
    pub fn passed(&self) -> bool {
        self.error.is_none() && self.checks.iter().all(|c| c.passed)
    }
}

#[test]
fn playback_interpolates_between_solver_steps() {
    let (t, v) = ([0., 0.1, 0.2], [0., 1., 3.]);
    assert!((interpolate(&t, &v, 0.15).unwrap() - 2.).abs() < 1e-12);
    assert_eq!(interpolate(&t, &v, 0.2), Some(3.));
    assert_eq!(interpolate(&t, &v, -0.01), None);
    assert_eq!(interpolate(&t, &v, 0.3), None);
}

#[test]
fn the_first_frame_is_filled_from_the_series_so_slow_motion_moves() {
    let frame = SampleFrame { version: sim_inspect::SAMPLE_FRAME_VERSION, description_id: "d".into(), model_revision: 1, run_id: "r".into(), generation: 1, sequence: 0, step: 0, time: 0., values: [("i".to_string(), sim_inspect::SampleValue::Unavailable { reason: "not sampled yet".into() })].into() };
    let series = Series { observable: "i".into(), label: "motor.p.current".into(), unit: "A".into(), times: vec![0., 0.001, 0.002], values: vec![0., 2., 3.5] };
    let run = SceneRun { version: VERSION, key: "k".into(), scene: "s".into(), fidelity: String::new(), duration_s: 0.01, description_id: "d".into(), frames: vec![frame], series: vec![series], checks: vec![], applied: vec![], error: None, wall_seconds: 0. };
    let f = run.frame_interpolated(0.0015).unwrap();
    assert!(matches!(f.values["i"], sim_inspect::SampleValue::Committed { value, .. } if (value - 2.75).abs() < 1e-12), "{:?}", f.values["i"]);
}

/// Linear interpolation of a sampled series at `t` (None outside its span).
fn interpolate(times: &[f64], values: &[f64], t: f64) -> Option<f64> {
    let i = times.partition_point(|x| *x <= t);
    if i == 0 || times.len() != values.len() {
        return None;
    }
    if i == times.len() {
        return (t - times[i - 1] < 1e-12).then(|| values[i - 1]);
    }
    let (t0, t1) = (times[i - 1], times[i]);
    let f = if t1 > t0 { (t - t0) / (t1 - t0) } else { 0. };
    Some(values[i - 1] + f * (values[i] - values[i - 1]))
}

pub fn load_cached(key: &str) -> Option<SceneRun> {
    let bytes = std::fs::read(cache_dir().join(format!("{key}.json"))).ok()?;
    serde_json::from_slice::<SceneRun>(&bytes).ok().filter(|r| r.version == VERSION && r.key == key)
}
pub fn save_cached(run: &SceneRun) -> Result<(), String> {
    let dir = cache_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec(run).map_err(|e| e.to_string())?;
    sim_annotate::store::write_atomic(&dir.join(format!("{}.json", run.key)), &bytes)
}

/// Observable IDs matching `key`: an ID, an exact readable key, else a
/// unique substring of a readable key.
pub fn resolve_observable(description: &sim_inspect::SystemDescription, key: &str) -> Result<String, String> {
    if description.observables.contains_key(key) {
        return Ok(key.into());
    }
    let keys: Vec<(String, String)> = description.observables.keys().map(|id| (system_builder::observable_key(description, id), id.clone())).collect();
    if let Some((_, id)) = keys.iter().find(|(k, _)| k == key) {
        return Ok(id.clone());
    }
    let matches: Vec<&(String, String)> = keys.iter().filter(|(k, _)| k.contains(key)).collect();
    match matches.as_slice() {
        [one] => Ok(one.1.clone()),
        [] => Err(format!("no observable `{key}`")),
        many => Err(format!("`{key}` is ambiguous: {}", many.iter().take(5).map(|m| m.0.as_str()).collect::<Vec<_>>().join(", "))),
    }
}

/// Record a scene. `document` is the scene document ([`scene_document`]).
/// `progress` receives the fraction done. Cancelling returns an error.
pub fn run_scene(document: &SystemDocument, registry: &BehaviorRegistry, scene: &Scene, timeline: &Timeline, cancel: Option<&AtomicBool>, progress: &dyn Fn(f64)) -> Result<SceneRun, String> {
    let started = std::time::Instant::now();
    let key = run_key(document, scene, timeline);
    let config = system_builder::config_for(document);
    let compiled = system_builder::compile(document, registry, config.clone())?;
    let source = system_builder::source(&compiled, registry, document);
    let mut session = SystemSession::new(compiled.launch.run_id.clone(), config.clone(), move |c| source.build(c)).map_err(|e| system_builder::locate(&compiled.flat, e))?;
    let description = session.description().clone();
    // What to record: animation bindings, plots, checks and plot cues.
    let mut wanted: Vec<String> = compiled.animation.as_ref().map(|a| a.observables().into_iter().collect()).unwrap_or_default();
    let mut keys: Vec<String> = scene.plots.clone();
    keys.extend(scene.record.iter().cloned());
    keys.extend(scene.phase.iter().flat_map(|p| [p.x.clone(), p.y.clone()]));
    keys.extend(scene.expect.iter().map(|e| e.observe.clone()));
    keys.extend(scene.challenge.iter().flat_map(|c| c.win.iter().map(|e| e.observe.clone())));
    for c in &timeline.cues {
        if let Action::Plot { observables } = &c.action {
            keys.extend(observables.iter().cloned());
        }
    }
    for k in &keys {
        wanted.push(resolve_observable(&description, k)?);
    }
    let available: BTreeSet<&String> = description.observables.iter().filter(|(_, o)| o.availability == sim_inspect::Availability::Available).map(|(id, _)| id).collect();
    wanted.retain(|id| available.contains(id));
    wanted.sort();
    wanted.dedup();
    session.subscribe(wanted.clone())?;
    session.execute(SessionCommand::Start)?;
    let duration = scene.run.duration_s;
    let frame_dt = 1.0 / scene.run.frame_rate;
    let mut frames: Vec<SampleFrame> = vec![session.latest().clone()];
    let mut next_frame = frame_dt;
    let mut points: BTreeMap<String, (Vec<f64>, Vec<f64>)> = BTreeMap::new();
    let record = |frame: &SampleFrame, points: &mut BTreeMap<String, (Vec<f64>, Vec<f64>)>| {
        for id in &wanted {
            if let Some(v) = sim_inspect::animation::scalar(Some(frame), id) {
                let e = points.entry(id.clone()).or_default();
                if e.0.last().is_none_or(|t| v.time > *t) {
                    e.0.push(v.time);
                    e.1.push(v.value);
                }
            }
        }
    };
    record(session.latest(), &mut points);
    let physics: Vec<(f64, String, f64)> = timeline.physics().map(|(t, k, v)| (t, k.to_string(), v)).collect();
    let mut applied = Vec::new();
    let mut current = document.clone();
    let mut next_cue = 0;
    let mut error = None;
    let mut last_progress = 0.0;
    while session.status().time + 0.5 * config.interval < duration {
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            return Err("cancelled".into());
        }
        // Physics cues take effect at the first step boundary at or after their time.
        let now = session.status().time;
        let mut due = Vec::new();
        while next_cue < physics.len() && physics[next_cue].0 <= now + 0.5 * config.interval {
            due.push(physics[next_cue].clone());
            next_cue += 1;
        }
        if !due.is_empty() {
            let commands = due.iter().map(|(_, k, v)| set_command(k, *v)).collect::<Result<Vec<_>, _>>()?;
            sim_system::apply(&mut current, registry, &commands).map_err(|e| format!("cue at {:.3} s: {e}", due[0].0))?;
            let next = system_builder::compile(&current, registry, config.clone())?;
            let source = system_builder::source(&next, registry, &current);
            let preserved = session.hot_swap(move |c| source.build(c))?;
            if !preserved {
                return Err(format!("cue at {:.3} s changed the system's structure; lesson cues may only change parameter values", due[0].0));
            }
            session.execute(SessionCommand::Start)?;
            applied.extend(due.into_iter().map(|(_, k, v)| (now, k, v)));
        }
        if let Err(e) = session.tick() {
            error = Some(system_builder::locate(&compiled.flat, e));
            break;
        }
        if session.status().phase == sim_inspect::live::Phase::Failed {
            error = Some(session.status().message.clone().unwrap_or_else(|| "run failed".into()));
            break;
        }
        let frame = session.latest();
        record(frame, &mut points);
        if frame.time + 1e-12 >= next_frame {
            frames.push(frame.clone());
            while next_frame <= frame.time + 1e-12 {
                next_frame += frame_dt;
            }
        }
        let fraction = (frame.time / duration).clamp(0.0, 1.0);
        if fraction - last_progress >= 0.01 {
            last_progress = fraction;
            progress(fraction);
        }
    }
    let series: Vec<Series> = points
        .into_iter()
        .map(|(id, (times, values))| {
            let o = &description.observables[&id];
            Series { label: system_builder::observable_key(&description, &id), unit: sim_inspect::plot::unit(&description, o).to_string(), observable: id, times, values }
        })
        .collect();
    let mut run = SceneRun {
        version: VERSION,
        key,
        scene: scene.id.clone(),
        fidelity: fidelity_label(document, scene.fidelity),
        duration_s: duration,
        description_id: description.id.clone(),
        frames,
        series,
        checks: Vec::new(),
        applied,
        error,
        wall_seconds: started.elapsed().as_secs_f64(),
    };
    run.checks = check_run(&run, scene);
    progress(1.0);
    Ok(run)
}

/// Evaluate a scene's claims against a recorded run.
pub fn check_run(run: &SceneRun, scene: &Scene) -> Vec<CheckResult> {
    check_claims(run, &scene.expect)
}

/// Judge claims (a scene's `expect`, a challenge's `win`) on a run.
pub fn check_claims(run: &SceneRun, claims: &[sim_lesson::Expect]) -> Vec<CheckResult> {
    claims
        .iter()
        .map(|e| {
            let metric = sim_system::Metric { label: e.observe.clone(), observable: e.observe.clone(), reduce: e.reduce, window: e.window };
            let value = run.series(&e.observe).and_then(|s| crate::system_study::reduce(s, &metric));
            let passed = value.is_some_and(|v| v.is_finite() && e.min.is_none_or(|m| v >= m) && e.max.is_none_or(|m| v <= m));
            let bounds = match (e.min, e.max) {
                (Some(a), Some(b)) => format!("in [{a}, {b}]"),
                (Some(a), None) => format!("≥ {a}"),
                (None, Some(b)) => format!("≤ {b}"),
                (None, None) => String::new(),
            };
            let window = e.window.map(|[a, b]| format!(" over {a}–{b} s")).unwrap_or_default();
            let message = match value {
                Some(v) => format!("{:?} of {}{window} = {v:.6} (expected {bounds})", e.reduce, e.observe),
                None => format!("{:?} of {}{window}: no samples{}", e.reduce, e.observe, run.error.as_ref().map(|x| format!(" (run failed: {x})")).unwrap_or_default()),
            };
            CheckResult { observe: e.observe.clone(), reduce: e.reduce, window: e.window, value, min: e.min, max: e.max, passed, why: e.why.clone(), message: message.to_lowercase_first() }
        })
        .collect()
}

trait LowerFirst {
    fn to_lowercase_first(self) -> String;
}
impl LowerFirst for String {
    fn to_lowercase_first(self) -> String {
        let mut c = self.chars();
        match c.next() {
            Some(f) => f.to_lowercase().collect::<String>() + c.as_str(),
            None => self,
        }
    }
}

/// Cached run, or a fresh one (saved to the cache).
pub fn scene_run(document: &SystemDocument, registry: &BehaviorRegistry, scene: &Scene, timeline: &Timeline, use_cache: bool, cancel: Option<&AtomicBool>, progress: &dyn Fn(f64)) -> Result<SceneRun, String> {
    let key = run_key(document, scene, timeline);
    if use_cache {
        if let Some(mut run) = load_cached(&key) {
            // Claims may be edited without changing the run.
            run.checks = check_run(&run, scene);
            return Ok(run);
        }
    }
    let run = run_scene(document, registry, scene, timeline, cancel, progress)?;
    let _ = save_cached(&run);
    Ok(run)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompareRun {
    pub version: u32,
    pub key: String,
    pub id: String,
    pub study: String,
    pub table: String,
    /// Per variant: label, (metric, value) pairs, error.
    pub variants: Vec<(String, Vec<(String, f64)>, Option<String>)>,
    /// Per variant: recorded series (for overlaid plots).
    pub series: Vec<Vec<Series>>,
}

pub fn compare_key(document: &SystemDocument, compare: &Compare) -> String {
    let study = document.studies.get(&compare.study);
    let mut doc = document.clone();
    doc.discussions = Default::default();
    doc.revision = 0;
    let value = serde_json::json!({"version": VERSION, "document": doc.content_hash(), "study": study, "name": compare.study});
    blake3::hash(&serde_json::to_vec(&value).unwrap_or_default()).to_hex()[..24].to_string()
}

pub fn compare_run(document: &SystemDocument, registry: &BehaviorRegistry, compare: &Compare, use_cache: bool, cancel: Option<&AtomicBool>, progress: &(dyn Fn(usize, usize) + Sync)) -> Result<CompareRun, String> {
    let key = compare_key(document, compare);
    let path = cache_dir().join(format!("compare-{key}.json"));
    if use_cache {
        if let Some(run) = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<CompareRun>(&b).ok()).filter(|r| r.version == VERSION) {
            return Ok(run);
        }
    }
    let study = document.studies.get(&compare.study).ok_or_else(|| format!("the system has no saved study `{}` ({})", compare.study, document.studies.keys().cloned().collect::<Vec<_>>().join(", ")))?;
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).min(4);
    let result = crate::system_study::run(document, registry, None, &compare.study, study, threads, cancel, progress)?;
    let run = CompareRun {
        version: VERSION,
        key,
        id: compare.id.clone(),
        study: compare.study.clone(),
        table: crate::system_study::table(&result),
        variants: result.variants.iter().map(|v| (v.label.clone(), v.metrics.clone(), v.error.clone())).collect(),
        series: result.variants.iter().map(|v| v.series.clone()).collect(),
    };
    if std::fs::create_dir_all(cache_dir()).is_ok() {
        if let Ok(bytes) = serde_json::to_vec(&run) {
            let _ = sim_annotate::store::write_atomic(&path, &bytes);
        }
    }
    Ok(run)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub path: String,
    pub line: usize,
    pub severity: Severity,
    pub message: String,
}
impl std::fmt::Display for Finding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}: {}", self.path, self.line, if self.severity == Severity::Error { "error" } else { "warning" }, self.message)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Report {
    pub lesson: String,
    pub findings: Vec<Finding>,
    /// Estimated minutes to work through it: reading, questions and scenes.
    #[serde(default)]
    pub minutes: f64,
    /// Scene ID → run summary (fidelity, wall time, checks).
    pub scenes: BTreeMap<String, SceneSummary>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneSummary {
    pub key: String,
    pub fidelity: String,
    pub wall_seconds: f64,
    pub checks: Vec<CheckResult>,
    pub error: Option<String>,
    /// How long the scene plays on screen under the pacing rules, holds
    /// included and authored pauses excluded (s).
    #[serde(default)]
    pub screen_seconds: f64,
}
impl Report {
    pub fn ok(&self) -> bool {
        !self.findings.iter().any(|f| f.severity == Severity::Error)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CheckOptions {
    /// Record scenes and evaluate claims (else structure only).
    pub run: bool,
    pub use_cache: bool,
    /// Also run `sim-compare` studies.
    pub compares: bool,
}
impl Default for CheckOptions {
    fn default() -> Self {
        Self { run: true, use_cache: true, compares: false }
    }
}

/// Every instance path of a document (for `part:` links, focus and highlight).
pub fn instance_paths(document: &SystemDocument) -> BTreeSet<String> {
    sim_system::display::paths(document).into_keys().collect()
}

fn path_known(paths: &BTreeSet<String>, p: &str) -> bool {
    p.is_empty() || paths.contains(p)
}

/// Check a lesson: every system loads, every reference resolves, every
/// script evaluates and (with `options.run`) every claim holds.
pub fn check(lesson: &Lesson, registry: &BehaviorRegistry, options: CheckOptions) -> Report {
    let file = lesson.path.display().to_string();
    let mut findings: Vec<Finding> = Vec::new();
    let mut summaries: BTreeMap<String, SceneSummary> = BTreeMap::new();
    let mut error = |line: usize, message: String| findings.push(Finding { path: file.clone(), line, severity: Severity::Error, message });
    let mut systems: BTreeMap<String, Result<SystemDocument, String>> = BTreeMap::new();
    let mut system = |name: &str| systems.entry(name.to_string()).or_insert_with(|| load_system(&lesson.system_path(name), registry)).clone();
    for (name, _) in &lesson.meta.systems.clone() {
        if let Err(e) = system(name) {
            error(2, format!("system `{name}`: {e}"));
        }
    }
    for link in lesson.part_links() {
        if let Ok(doc) = system(&link.system) {
            if !instance_paths(&doc).contains(&link.path) {
                error(link.line, format!("part:{}/{}: no instance `{}` in {}", link.system, link.path, link.path, lesson.system_path(&link.system).display()));
            }
        }
    }
    for e in sim_lesson::figure::check(lesson) {
        findings.push(Finding { path: file.clone(), line: e.line, severity: Severity::Error, message: e.message });
    }
    // The narrated explainer, if any: its cues must point at real things.
    let explainer_path = sim_lesson::narration::Explainer::path_for(lesson);
    if explainer_path.is_file() {
        let file = explainer_path.display().to_string();
        match sim_lesson::narration::Explainer::load(&explainer_path) {
            Err(e) => findings.push(Finding { path: file.clone(), line: e.line, severity: Severity::Error, message: e.message }),
            Ok(ex) => {
                for e in sim_lesson::narration::check(&ex, lesson) {
                    findings.push(Finding { path: file.clone(), line: e.line, severity: Severity::Error, message: e.message });
                }
                // Recorded voice must match the text: a stale section plays silently.
                let manifest = ex.manifest();
                let voiced = ex.sections.iter().filter(|sec| ex.audio(&manifest, sec).is_some()).count();
                if voiced > 0 {
                    for sec in ex.sections.iter().filter(|sec| ex.audio(&manifest, sec).is_none()) {
                        findings.push(Finding { path: file.clone(), line: sec.line, severity: Severity::Warning, message: format!("section `{}` has no audio for its current text (it plays silently, subtitles only): run `sim-narrate generate {}`", sec.id, lesson.dir().display()) });
                    }
                }
                let paths: BTreeSet<String> = lesson.meta.systems.keys().filter_map(|name| system(name).ok()).flat_map(|d| instance_paths(&d)).collect();
                let charted: BTreeSet<String> = lesson.scenes().flat_map(|(_, s)| {
                    let mut keys = s.plots.clone();
                    if let Ok(t) = lesson.timeline(s) {
                        for c in &t.cues {
                            if let Action::Plot { observables } = &c.action {
                                keys.extend(observables.iter().cloned());
                            }
                        }
                    }
                    keys
                }).collect();
                for section in &ex.sections {
                    for c in &section.cues {
                        use sim_lesson::narration::{Cue, Target};
                        let target = match &c.cue {
                            Cue::Box { target, .. } | Cue::Arrow { target, .. } | Cue::Scroll { target } => Some(target),
                            Cue::Highlight { target: Some(t) } => Some(t),
                            _ => None,
                        };
                        let problem = match target {
                            Some(Target::Part { path }) if !paths.contains(path) => Some(format!("part `{path}` is not an instance in the lesson's systems")),
                            Some(Target::Plot { key, .. }) if !charted.contains(key) => Some(format!("plot `{key}` is not charted by any scene (add it to a scene's plots)")),
                            _ => None,
                        };
                        if let Some(m) = problem {
                            findings.push(Finding { path: file.clone(), line: c.line, severity: Severity::Error, message: m });
                        }
                    }
                }
            }
        }
    }
    let mut pacing_warnings: Vec<(usize, String)> = Vec::new();
    let mut error = |line: usize, message: String| findings.push(Finding { path: file.clone(), line, severity: Severity::Error, message });
    let mut scenes = Vec::new();
    for block in &lesson.blocks {
        match &block.kind {
            BlockKind::Component(card) => {
                if registry.get(&card.component.as_str().into()).is_err() {
                    error(block.line, format!("sim-component: no registered component `{}`", card.component));
                }
            }
            BlockKind::Compare(c) => match system(&c.system) {
                Err(_) => {}
                Ok(doc) => {
                    if !doc.studies.contains_key(&c.study) {
                        error(block.line, format!("sim-compare {}: the system has no saved study `{}` ({})", c.id, c.study, doc.studies.keys().cloned().collect::<Vec<_>>().join(", ")));
                    } else if options.compares && options.run {
                        if let Err(e) = compare_run(&doc, registry, c, options.use_cache, None, &|_, _| {}) {
                            error(block.line, format!("sim-compare {}: {e}", c.id));
                        }
                    }
                }
            },
            BlockKind::Scene(scene) => {
                let scene = &Box::new(lesson_scene(lesson, scene));
                let Ok(doc) = system(&scene.system) else { continue };
                let paths = instance_paths(&doc);
                if let Err(e) = Resolver::new(&doc, registry).definition_id_at(&scene.level) {
                    error(block.line, format!("sim-scene {}: level `{}`: {e}", scene.id, scene.level));
                }
                if let Some(f) = scene.camera.as_ref().and_then(|c| c.focus.as_ref()).filter(|f| !path_known(&paths, f)) {
                    error(block.line, format!("sim-scene {}: camera focus `{f}` is not an instance path", scene.id));
                }
                let timeline = match lesson.timeline(scene) {
                    Ok(t) => t,
                    Err(e) => {
                        error(block.line, format!("sim-scene {}: {e}", scene.id));
                        continue;
                    }
                };
                for c in &timeline.cues {
                    let bad = match &c.action {
                        Action::Highlight { paths: p } => p.iter().find(|x| !path_known(&paths, x)).map(|x| format!("highlight `{x}` is not an instance path")),
                        Action::Camera { camera } => camera.focus.as_ref().filter(|f| !path_known(&paths, f)).map(|f| format!("camera focus `{f}` is not an instance path")),
                        Action::Set { parameter, .. } => split_parameter(parameter).ok().and_then(|(at, name, _)| {
                            let p = if at.is_empty() { name } else { format!("{at}/{name}") };
                            (!paths.contains(&p)).then(|| format!("set `{parameter}`: no instance `{p}`"))
                        }),
                        Action::View { view } => match view {
                            sim_script::presentation::View::Zoom { focus: Some(f), .. } if !path_known(&paths, f) => Some(format!("zoom `{f}` is not an instance path")),
                            sim_script::presentation::View::Spotlight { paths: p } => p.iter().find(|x| !path_known(&paths, x)).map(|x| format!("spotlight `{x}` is not an instance path")),
                            sim_script::presentation::View::Pin { path, .. } if !path_known(&paths, path) => Some(format!("pin `{path}` is not an instance path")),
                            sim_script::presentation::View::Inset { path: Some(p), .. } if !path_known(&paths, p) => Some(format!("inset `{p}` is not an instance path")),
                            _ => None,
                        },
                        _ => None,
                    };
                    if let Some(m) = bad {
                        error(block.line, format!("sim-scene {} (cue at {} s): {m}", scene.id, c.at_s));
                    }
                }
                // Sliders and companion runs change parameters like `set` does.
                let instance = |parameter: &str| -> Option<String> {
                    let (at, name, _) = split_parameter(parameter).ok()?;
                    let p = if at.is_empty() { name } else { format!("{at}/{name}") };
                    (!paths.contains(&p)).then(|| format!("`{parameter}`: no instance `{p}`"))
                };
                for sl in &scene.sliders {
                    if let Some(m) = split_parameter(&sl.parameter).err().or_else(|| instance(&sl.parameter)) {
                        error(block.line, format!("sim-scene {}: slider {m}", scene.id));
                    }
                    if !(sl.min.is_finite() && sl.max.is_finite() && sl.min < sl.max) || sl.step.is_some_and(|s| !(s > 0.)) {
                        error(block.line, format!("sim-scene {}: slider `{}` needs min < max and a positive step", scene.id, sl.parameter));
                    }
                }
                if let Some(c) = &scene.companion {
                    for k in c.set.keys() {
                        if let Some(m) = split_parameter(k).err().or_else(|| instance(k)) {
                            error(block.line, format!("sim-scene {}: companion {m}", scene.id));
                        }
                    }
                }
                if scene.challenge.is_some() && scene.sliders.is_empty() {
                    error(block.line, format!("sim-scene {}: a challenge needs sliders to reach it with", scene.id));
                }
                let doc = match scene_document(&doc, registry, scene) {
                    Ok(d) => d,
                    Err(e) => {
                        error(block.line, format!("sim-scene {}: {e}", scene.id));
                        continue;
                    }
                };
                scenes.push((block.line, scene.clone(), doc, timeline));
            }
            _ => {}
        }
    }
    for (line, scene, doc, timeline) in scenes {
        // Observables must exist even when not running.
        match system_builder::compile(&doc, registry, system_builder::config_for(&doc)) {
            Err(e) => {
                error(line, format!("sim-scene {}: does not compile: {e}", scene.id));
                continue;
            }
            Ok(compiled) => {
                let mut keys: Vec<&String> = scene.plots.iter().chain(scene.phase.iter().flat_map(|p| [&p.x, &p.y])).chain(scene.expect.iter().map(|e| &e.observe)).chain(scene.challenge.iter().flat_map(|c| c.win.iter().map(|e| &e.observe))).collect();
                let predictions: Vec<&String> = lesson.quizzes().filter(|(_, q)| q.scene.as_deref() == Some(scene.id.as_str())).filter_map(|(_, q)| q.observe.as_ref()).collect();
                keys.extend(predictions);
                for c in &timeline.cues {
                    if let Action::Plot { observables } = &c.action {
                        keys.extend(observables.iter());
                    }
                }
                let mut bad = false;
                for k in keys {
                    if let Err(e) = resolve_observable(&compiled.description, k) {
                        error(line, format!("sim-scene {}: {e}", scene.id));
                        bad = true;
                    }
                }
                if bad {
                    continue;
                }
            }
        }
        if !options.run {
            continue;
        }
        match scene_run(&doc, registry, &scene, &timeline, options.use_cache, None, &|_| {}) {
            Err(e) => error(line, format!("sim-scene {}: {e}", scene.id)),
            Ok(run) => {
                if let Some(e) = &run.error {
                    error(line, format!("sim-scene {}: run stopped: {e}", scene.id));
                }
                for c in run.checks.iter().filter(|c| !c.passed) {
                    error(line, format!("sim-scene {}: claim failed: {}{}", scene.id, c.message, if c.why.is_empty() { String::new() } else { format!(" — {}", c.why) }));
                }
                let plan = run.pacing(&scene, &timeline);
                if let Ok(plan) = &plan {
                    pacing_warnings.extend(plan.warnings.iter().map(|w| (line, format!("sim-scene {} pacing: {w}", scene.id))));
                }
                summaries.insert(scene.id.clone(), SceneSummary { key: run.key.clone(), fidelity: run.fidelity.clone(), wall_seconds: run.wall_seconds, checks: run.checks.clone(), error: run.error.clone(), screen_seconds: plan.map(|p| p.duration()).unwrap_or(0.) });
            }
        }
    }
    findings.extend(pacing_warnings.into_iter().map(|(line, message)| Finding { path: file.clone(), line, severity: Severity::Warning, message }));
    // Numbers, equations, measured data, varied questions and tasks: each
    // must agree with the model.
    if options.run {
        findings.extend(check_model(lesson, registry, options.use_cache).into_iter().map(|(line, severity, message)| Finding { path: file.clone(), line, severity, message }));
    }
    for (line, message) in check_concepts(lesson) {
        findings.push(Finding { path: file.clone(), line, severity: Severity::Warning, message });
    }
    // Concept pacing of the text: new terms, equations and words per check.
    let rules = sim_lesson::density::DensityRules::default();
    findings.extend(sim_lesson::density::warnings(lesson, &rules).into_iter().map(|(line, message)| Finding { path: file.clone(), line, severity: Severity::Warning, message }));
    let scene_minutes = summaries.values().map(|s| s.screen_seconds).sum::<f64>() / 60.;
    let minutes = sim_lesson::density::reading_minutes(lesson, &rules) + scene_minutes;
    Report { lesson: lesson.slug.clone(), findings, scenes: summaries, minutes }
}

/// Model-backed claims of a lesson (see [`crate::lesson_model`]).
pub fn check_model(lesson: &Lesson, registry: &BehaviorRegistry, use_cache: bool) -> Vec<(usize, Severity, String)> {
    use crate::lesson_model as lm;
    let model = lm::Model::new(lesson, registry, use_cache);
    let mut out = Vec::new();
    for (line, r, value) in model.inline_values() {
        match value {
            Err(e) => out.push((line, Severity::Error, format!("{{{{… | {}}}}}: {e}", r.shown))),
            Ok(v) => match sim_lesson::refs::agrees(&r, v) {
                Err(e) => out.push((line, Severity::Error, format!("{{{{… | {}}}}}: {e}", r.shown))),
                Ok(false) => out.push((line, Severity::Error, format!("the text says {} but the model gives {} (update the number, or the model)", r.shown, sim_lesson::refs::format_like(&r.shown, v)))),
                Ok(true) => {}
            },
        }
    }
    for b in &lesson.blocks {
        match &b.kind {
            BlockKind::Equation(e) => {
                match model.equation_constants(e) {
                    Err(m) => out.push((b.line, Severity::Error, format!("sim-equation {}: {m}", e.id))),
                    Ok(constants) => {
                        let run = e.scene.as_ref().map(|s| model.run(s));
                        match run {
                            Some(Err(m)) => out.push((b.line, Severity::Error, format!("sim-equation {}: {m}", e.id))),
                            other => {
                                let run = other.and_then(Result::ok);
                                let t = run.as_ref().map(|r| r.duration_s * 0.5).unwrap_or(0.);
                                if let Err(m) = lm::equation_at(e, &constants, run.as_ref(), t) {
                                    out.push((b.line, Severity::Error, format!("sim-equation {}: {m}", e.id)));
                                } else if e.holds.is_some() {
                                    match lm::check_equation(&model, e) {
                                        Ok(c) if !c.passed => out.push((b.line, Severity::Error, format!("sim-equation {}: does not hold: {}", e.id, c.message))),
                                        Err(m) => out.push((b.line, Severity::Error, format!("sim-equation {}: {m}", e.id))),
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                }
            }
            BlockKind::Measured(m) => match lm::measured(&model, m) {
                Err(e) => out.push((b.line, Severity::Error, format!("sim-measured {}: {e}", m.id))),
                Ok(r) => {
                    for c in r.checks.iter().filter(|c| !c.passed) {
                        out.push((b.line, Severity::Error, format!("sim-measured {}: {}", m.id, c.message)));
                    }
                }
            },
            BlockKind::Quiz(q) if q.check_with.is_some() => match lm::check_variant(&model, q) {
                Err(e) => out.push((b.line, Severity::Error, format!("sim-quiz {} check_with: {e}", q.id))),
                Ok(rows) => {
                    let tol = q.check_with.as_ref().and_then(|c| c.tolerance).unwrap_or(sim_lesson::quiz::Tolerance::Relative(0.05));
                    for (values, formula, simulated) in rows {
                        if !tol.accepts(simulated, formula) {
                            out.push((b.line, Severity::Error, format!("sim-quiz {}: answer_expr gives {formula:.4} but the simulation gives {simulated:.4} at {values:?}", q.id)));
                        }
                    }
                }
            },
            BlockKind::Lab(l) if l.compare.is_some() => {
                if let Err(e) = crate::lesson_lab::prediction(&model, l) {
                    out.push((b.line, Severity::Error, format!("sim-lab {}: {e}", l.id)));
                }
            }
            BlockKind::Task(t) => {
                if let Some(scene) = lesson.scene(&t.scene) {
                    if let Ok((doc, _)) = model.system(&scene.system) {
                        for m in lm::task_paths(&doc, t) {
                            out.push((b.line, Severity::Error, format!("sim-task {}: {m}", t.id)));
                        }
                    }
                }
                match lm::prove_task(&model, t) {
                    Err(e) => out.push((b.line, Severity::Error, format!("sim-task {}: {e}", t.id))),
                    Ok((start, solved)) => {
                        if start.passed {
                            out.push((b.line, Severity::Error, format!("sim-task {}: the starting values already meet the goal; there is nothing to {}", t.id, if t.kind == sim_lesson::blocks::TaskKind::Fault { "fix" } else { "design" })));
                        }
                        match solved {
                            Some(s) if !s.passed => out.push((b.line, Severity::Error, format!("sim-task {}: the solution does not meet the goal: {}", t.id, s.checks.iter().filter(|c| !c.passed).map(|c| c.message.clone()).collect::<Vec<_>>().join("; ")))),
                            None => out.push((b.line, Severity::Warning, format!("sim-task {}: give a `solution` so check can prove the task is solvable", t.id))),
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Concept IDs used by the lesson but not defined in `concepts.yaml`.
pub fn check_concepts(lesson: &Lesson) -> Vec<(usize, String)> {
    let dir = lesson.dir().parent().map(Path::to_path_buf).unwrap_or_default();
    match sim_lesson::concepts::load(&dir) {
        Err(e) => vec![(1, e)],
        Ok(map) if map.is_empty() && lesson.meta.teaches.is_empty() && lesson.meta.needs.is_empty() => vec![],
        Ok(map) => sim_lesson::concepts::unknown(&map, lesson).into_iter().map(|c| (2, format!("concept `{c}` is not defined in concepts.yaml"))).collect(),
    }
}

/// Observables a scene plots: its `plots` and any a `plot` cue adds.
pub fn plot_keys(scene: &Scene, timeline: &Timeline) -> Vec<String> {
    let mut keys: Vec<String> = scene.plots.clone();
    for c in &timeline.cues {
        if let Action::Plot { observables } = &c.action {
            keys.extend(observables.iter().cloned());
        }
    }
    let mut seen = BTreeSet::new();
    keys.retain(|k| seen.insert(k.clone()));
    keys
}

/// Lessons under `dir` (or the one file), checked in order; index problems
/// (broken files, unknown prerequisites) become findings too.
pub fn check_path(path: &Path, registry: &BehaviorRegistry, options: CheckOptions) -> Vec<Report> {
    if path.is_file() {
        return vec![match Lesson::load(path) {
            Ok(l) => check(&l, registry, options),
            Err(e) => Report { lesson: path.display().to_string(), findings: vec![Finding { path: e.path.display().to_string(), line: e.line, severity: Severity::Error, message: e.message }], scenes: BTreeMap::new(), minutes: 0. },
        }];
    }
    let entries = sim_lesson::index::scan(path);
    let mut reports = Vec::new();
    for (slug, missing) in sim_lesson::index::unknown_requires(&entries) {
        reports.push(Report { lesson: slug.clone(), findings: vec![Finding { path: path.join(&slug).join("lesson.md").display().to_string(), line: 1, severity: Severity::Warning, message: format!("requires `{missing}`, which is not a lesson here") }], scenes: BTreeMap::new(), minutes: 0. });
    }
    match sim_lesson::categories::load(path) {
        Err(e) => reports.push(Report { lesson: "categories".into(), findings: vec![Finding { path: path.join("categories.yaml").display().to_string(), line: 1, severity: Severity::Error, message: e }], scenes: BTreeMap::new(), minutes: 0. }),
        Ok(categories) => {
            for (slug, name) in sim_lesson::categories::unknown(&entries, &categories) {
                let file = path.join(&slug).join("lesson.md");
                let line = std::fs::read_to_string(&file).ok().and_then(|t| t.lines().position(|l| l.trim_start().starts_with("category:"))).map_or(1, |i| i + 1);
                reports.push(Report { lesson: slug.clone(), findings: vec![Finding { path: file.display().to_string(), line, severity: Severity::Error, message: format!("category `{name}` is not in categories.yaml ({})", categories.iter().map(|c| c.id.as_str()).collect::<Vec<_>>().join(", ")) }], scenes: BTreeMap::new(), minutes: 0. });
            }
        }
    }
    for e in entries {
        reports.extend(check_path(&e.path, registry, options));
    }
    reports
}

/// Where "open in builder" copies of lesson systems live
/// (`<workspace root>/runs/lessons/sandbox/<lesson>/<scene>/`, see [`crate::workspace`];
/// override with `SIM_LESSON_SANDBOX`).
pub fn sandbox_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("SIM_LESSON_SANDBOX") {
        return PathBuf::from(dir);
    }
    match crate::workspace::get() {
        Ok(root) => root.path.join("runs/lessons/sandbox"),
        Err(e) => {
            // Scratch copies only: never written into an unrelated launch directory.
            let dir = std::env::temp_dir().join("sim-lessons-sandbox");
            eprintln!("lesson sandboxes go to {} ({e})", dir.display());
            dir
        }
    }
}

/// A learner's copy of a scene's system: the authored system with the
/// scene's `set` values applied. Edits in the builder change only this copy.
#[derive(Debug, Clone, Serialize)]
pub struct Sandbox {
    pub path: PathBuf,
    /// The authored system file it was copied from.
    pub source: PathBuf,
    /// The learner changed the copy.
    pub modified: bool,
    /// The authored system changed after the copy was made (only reported
    /// while the copy is modified; an unmodified copy is refreshed).
    pub outdated: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SandboxMeta {
    source: PathBuf,
    source_hash: String,
    base_hash: String,
}

fn content(doc: &SystemDocument) -> String {
    let mut d = doc.clone();
    d.discussions = Default::default();
    d.content_hash()
}

/// Prepare (or with `reset`, recreate) the sandbox for a scene.
pub fn sandbox(lesson: &Lesson, scene: &Scene, registry: &BehaviorRegistry, reset: bool) -> Result<Sandbox, String> {
    let source = lesson.system_path(&scene.system);
    let authored = load_system(&source, registry)?;
    let dir = sandbox_root().join(&lesson.slug).join(&scene.id);
    let path = dir.join(source.file_name().ok_or("system path has no file name")?);
    let meta_path = dir.join("sandbox.json");
    let meta: Option<SandboxMeta> = std::fs::read(&meta_path).ok().and_then(|b| serde_json::from_slice(&b).ok());
    let source_hash = content(&authored);
    let current = path.is_file().then(|| load_system(&path, registry).ok()).flatten().map(|d| content(&d));
    let modified = match (&meta, &current) {
        (Some(m), Some(c)) => *c != m.base_hash,
        _ => false,
    };
    let stale = meta.as_ref().is_none_or(|m| m.source_hash != source_hash || m.source != source);
    if reset || current.is_none() || meta.is_none() || (stale && !modified) {
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let mut doc = authored.clone();
        doc.discussions = Default::default();
        let commands = scene.set.iter().map(|(k, v)| set_command(k, *v)).collect::<Result<Vec<_>, _>>()?;
        if !commands.is_empty() {
            sim_system::apply(&mut doc, registry, &commands).map_err(|e| format!("set: {e}"))?;
        }
        for asset in authored.assets.values() {
            let from = sim_system::assets::resolve(&source, asset);
            let to = sim_system::assets::resolve(&path, asset);
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::copy(&from, &to).map_err(|e| format!("copy {}: {e}", from.display()))?;
        }
        sim_system::SystemStore::create(&path, &doc).map_err(|e| e.to_string())?;
        let meta = SandboxMeta { source: source.clone(), source_hash, base_hash: content(&doc) };
        sim_annotate::store::write_atomic(&meta_path, &serde_json::to_vec_pretty(&meta).map_err(|e| e.to_string())?)?;
        return Ok(Sandbox { path, source, modified: false, outdated: false });
    }
    Ok(Sandbox { path, source, modified, outdated: modified && stale })
}

/// The scene as run from its sandbox: the copy already holds the scene's
/// `set` values (and the learner's edits), so they are not applied again.
pub fn sandbox_scene(scene: &Scene) -> Scene {
    let mut s = scene.clone();
    s.set.clear();
    s
}

/// Write the learner's copy over the authored system (the scene's `set`
/// values included). Refuses when the authored file changed meanwhile.
pub fn save_sandbox(lesson: &Lesson, scene: &Scene, registry: &BehaviorRegistry) -> Result<PathBuf, String> {
    let sb = sandbox(lesson, scene, registry, false)?;
    if sb.outdated {
        return Err("the lesson's system changed after you opened it; reset the copy first".into());
    }
    let mut doc = load_system(&sb.path, registry)?;
    let authored = load_system(&sb.source, registry)?;
    doc.discussions = authored.discussions.clone();
    let bytes = serde_json::to_vec_pretty(&doc).map_err(|e| e.to_string())?;
    sim_annotate::store::write_atomic(&sb.source, &bytes)?;
    // The copy now equals the source: record that as the new base.
    sandbox(lesson, scene, registry, true)?;
    Ok(sb.source)
}
