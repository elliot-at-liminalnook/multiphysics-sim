//! Learn-screen parts backed by the model and the bench:
//!
//! - numbers in prose, equation terms and measured comparisons, resolved
//!   off the UI thread by [`sim_runtime::lesson_model`] (cached runs);
//! - `sim-equation` cards whose values follow the playhead;
//! - `sim-measured` cards: measured points beside the simulation, the gaps,
//!   and where every parameter came from;
//! - `sim-task` cards: the task's own scene and sandbox, judged on each run;
//! - `sim-lab` cards: a prediction, the operator checklist, and the bench
//!   step through the shared in-process calibration worker;
//! - time on screen, rewinds and narration skips per block, for authors.
use super::*;
use crate::builder::ui::num;
use crate::ui_kit::{ACCENT, BORDER, FAINT, Kit, Look, OK, RAISED, SUBTLE, SURFACE, TEXT, WARN, wrap};
use sim_lesson::blocks::{Equation, Lab, Measured, Task, TaskKind};
use sim_runtime::lesson_lab::{self, LabPrediction};
use sim_runtime::lesson_model::{self as lm, MeasuredReport, TaskResult};

enum ModelUpdate {
    Value(String, Result<f64, String>),
    Constants(String, Result<BTreeMap<String, f64>, String>),
    Given(String, Result<BTreeMap<String, f64>, String>),
    Measured(String, Result<MeasuredReport, String>),
    Lab(String, Result<LabPrediction, String>),
}

/// What the model says about the open lesson (filled in as it arrives).
#[derive(Default)]
pub(crate) struct ModelState {
    /// Inline reference key → value (SI).
    pub values: BTreeMap<String, f64>,
    pub errors: BTreeMap<String, String>,
    /// Equation ID → its fixed terms.
    pub constants: BTreeMap<String, Result<BTreeMap<String, f64>, String>>,
    /// Question ID → its `given` model values.
    pub given: BTreeMap<String, BTreeMap<String, f64>>,
    pub measured: BTreeMap<String, Result<MeasuredReport, String>>,
    pub measured_images: BTreeMap<String, Handle<Image>>,
    pub labs: BTreeMap<String, Result<LabPrediction, String>>,
    /// Streams values as they resolve; dropping it (a new lesson) stops it.
    job: Option<crate::jobs::Job<(), ModelUpdate>>,
    pub loading: bool,
}

/// A task's latest judgement and how many hints are showing.
#[derive(Default)]
pub(crate) struct TaskState {
    pub last: Option<Result<TaskResult, String>>,
    pub hints: usize,
}

/// A lab card: checklist, prediction, and the bench conversation.
#[derive(Default)]
pub(crate) struct LabState {
    pub ticks: [bool; 4],
    pub prediction: String,
    job: Option<crate::jobs::Job<serde_json::Value>>,
    pub running: bool,
    pub(super) control: Option<std::sync::Arc<super::lab::Control>>,
    pub result: Option<Result<serde_json::Value, String>>,
}

impl Learn {
    /// Resolve everything the open lesson reads from the model, in the background.
    pub(crate) fn start_model(&mut self) {
        let Some(lesson) = self.lesson.clone() else { return };
        let registry = self.registry.clone();
        // Model runs may take a while (cached after the first): a dedicated thread.
        let job = crate::jobs::Job::streaming(crate::jobs::Pool::Dedicated, 0, "the lesson model", move |ctx| {
            let model = lm::Model::new(&lesson, &registry, true);
            for (_, r) in lesson.inline_refs() {
                if !ctx.emit(ModelUpdate::Value(r.key(), model.value(&r.source))) {
                    return Ok(());
                }
            }
            for (_, q) in lesson.quizzes().filter(|(_, q)| !q.given.is_empty()) {
                ctx.emit(ModelUpdate::Given(q.id.clone(), model.given(q)));
            }
            for (_, e) in lesson.equations() {
                ctx.emit(ModelUpdate::Constants(e.id.clone(), model.equation_constants(e)));
            }
            for b in &lesson.blocks {
                match &b.kind {
                    BlockKind::Measured(m) => {
                        ctx.emit(ModelUpdate::Measured(m.id.clone(), lm::measured(&model, m)));
                    }
                    BlockKind::Lab(l) if l.compare.is_some() => {
                        ctx.emit(ModelUpdate::Lab(l.id.clone(), lesson_lab::prediction(&model, l)));
                    }
                    _ => {}
                }
            }
            Ok(())
        });
        self.model = ModelState { job: Some(job), loading: true, ..Default::default() };
    }

    /// Collect model results; true when something changed.
    pub(crate) fn poll_model(&mut self, images: &mut Assets<Image>) -> bool {
        let mut changed = false;
        // The result is taken before the updates: every update of a finished job is then drained with it.
        let finished = self.model.job.as_ref().and_then(crate::jobs::Job::poll);
        let updates = self.model.job.as_ref().map(crate::jobs::Job::updates).unwrap_or_default();
        for update in updates {
            changed = true;
            match update {
                ModelUpdate::Value(k, Ok(v)) => {
                    self.model.values.insert(k, v);
                }
                ModelUpdate::Value(k, Err(e)) => {
                    self.model.errors.insert(k, e);
                }
                ModelUpdate::Given(q, Ok(v)) => {
                    self.model.given.insert(q, v);
                }
                ModelUpdate::Given(q, Err(e)) => {
                    self.model.errors.insert(format!("given:{q}"), e);
                }
                ModelUpdate::Constants(id, r) => {
                    self.model.constants.insert(id, r);
                }
                ModelUpdate::Measured(id, r) => {
                    if let Ok(report) = &r {
                        self.model.measured_images.insert(id.clone(), images.add(measured_image(report)));
                    }
                    self.model.measured.insert(id, r);
                }
                ModelUpdate::Lab(id, r) => {
                    self.model.labs.insert(id, r);
                }
            }
        }
        if let Some(result) = finished {
            if let Err(e) = result {
                self.model.errors.insert("model".into(), e);
            }
            self.model.job = None;
            self.model.loading = false;
            changed = true;
        }
        // Lab steps in progress on the bench.
        for state in self.labs.values_mut() {
            let got = state.job.as_ref().and_then(crate::jobs::Job::poll);
            if let Some(r) = got {
                state.job = None;
                state.running = false;
                state.result = Some(r);
                changed = true;
            }
        }
        changed
    }

    /// A text block's Markdown with its `{{…}}` numbers from the model.
    pub(crate) fn resolved(&self, text: &str) -> String {
        sim_lesson::refs::render(text, |r| self.model.values.get(&r.key()).copied())
    }

    /// Make a task's scene live: its own sandbox, starting from the task's values.
    pub(crate) fn start_task(&mut self, id: &str) -> Result<(), String> {
        let lesson = self.lesson.clone().ok_or("no lesson")?;
        let task = lesson.task(id).ok_or("no such task")?.clone();
        let scene = lm::task_scene(&lesson, &task, &BTreeMap::new())?;
        self.activate_scene(scene);
        Ok(())
    }

    /// Judge the task scene's latest run and remember the attempt.
    pub(crate) fn judge_task_run(&mut self, run: &SceneRun) {
        let Some(a) = self.scene.as_ref() else { return };
        let Some(task_id) = a.id.strip_prefix("task-").map(String::from) else { return };
        let Some(task) = self.lesson.as_ref().and_then(|l| l.task(&task_id)).cloned() else { return };
        let checks = runtime::check_claims(run, &task.win);
        let metrics: Vec<(String, f64, String)> = task.report.iter().filter_map(|m| {
            let s = run.series(&m.observe)?;
            let v = sim_runtime::system_study::reduce(s, &sim_system::Metric { label: m.label.clone(), observable: m.observe.clone(), reduce: m.reduce, window: m.window })?;
            Some((m.label.clone(), v, m.unit.clone()))
        }).collect();
        let passed = run.error.is_none() && checks.iter().all(|c| c.passed);
        let modified = a.sandbox.as_ref().is_some_and(|s| s.modified);
        let result = TaskResult { passed, checks: checks.clone(), metrics: metrics.clone(), design: run.key.clone(), error: run.error.clone() };
        self.tasks.entry(task_id.clone()).or_default().last = Some(Ok(result));
        // Attempts are the reader's changes; the untouched start is not one.
        if let (true, Some(slug)) = (modified, self.slug().map(String::from)) {
            let attempts = self.progress.lessons.entry(slug).or_default().tasks.entry(task_id).or_default();
            if attempts.last().is_none_or(|x| x.design != run.key) {
                attempts.push(sim_lesson::progress::TaskAttempt { at: sim_lesson::progress::now(), passed, metrics: metrics.iter().map(|(l, v, _)| (l.clone(), *v)).collect(), failed: checks.iter().filter(|c| !c.passed).map(|c| if c.why.is_empty() { c.message.clone() } else { c.why.clone() }).collect(), design: run.key.clone() });
                let path = self.progress_path.clone();
                let _ = self.progress.save(&path);
                if passed {
                    self.status = format!("Task solved{}.", if task.kind == TaskKind::Fault { ": the fault is fixed" } else { ": the design meets the goal" });
                }
            }
        }
    }

    /// Record a finished lab step in progress (once).
    pub(crate) fn record_lab(&mut self, id: &str) {
        let Some(slug) = self.slug().map(String::from) else { return };
        let Some(Ok(v)) = self.labs.get(id).and_then(|s| s.result.clone()) else { return };
        let Some(value) = v["steady_rad_s"].as_f64() else { return };
        let unit = self.lesson.as_ref().and_then(|l| l.lab(id)).map(|l| l.unit.clone()).unwrap_or_default();
        let trace = v["receipt"].as_str().unwrap_or_default().to_string();
        let records = self.progress.lessons.entry(slug).or_default().labs.entry(id.into()).or_default();
        if records.last().is_none_or(|r| r.trace != trace) {
            records.push(sim_lesson::progress::LabRecord { at: sim_lesson::progress::now(), rig: "bench".into(), value, unit, trace });
            let path = self.progress_path.clone();
            let _ = self.progress.save(&path);
        }
    }
}

/// Measured points (orange) beside the simulation (teal), on shared axes.
fn measured_image(r: &MeasuredReport) -> Image {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let mut sim: Vec<[f64; 2]> = r.points.iter().map(|p| [p.x, p.simulated]).collect();
    sim.sort_by(|a, b| a[0].total_cmp(&b[0]));
    sim.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-12);
    let measured: Vec<Vec<[f64; 2]>> = r.points.iter().map(|p| vec![[p.x, p.measured]]).collect();
    let mut traces: Vec<(&[[f64; 2]], [u8; 3])> = vec![(&sim, [77, 212, 191])];
    traces.extend(measured.iter().map(|m| (m.as_slice(), [240, 150, 60])));
    let (pixels, _, _) = crate::chart::rasterize_span(&traces, None);
    let (w, h) = crate::chart::RASTER;
    Image::new(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, pixels, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default())
}

fn frame() -> impl Bundle {
    frame_on(RAISED)
}
fn frame_on(background: Color) -> impl Bundle {
    (Node { border_radius: BorderRadius::all(Val::Px(9.)), flex_direction: FlexDirection::Column, row_gap: Val::Px(10.), padding: UiRect::all(Val::Px(16.)), border: UiRect::all(Val::Px(1.)), margin: UiRect::vertical(Val::Px(8.)), flex_shrink: 0., ..default() }, BackgroundColor(background), BorderColor::all(BORDER))
}
fn badge(k: &Kit, text: &str, color: Color) -> impl Bundle {
    (Node { border_radius: BorderRadius::all(Val::Px(10.)), padding: UiRect::axes(Val::Px(8.), Val::Px(2.)), border: UiRect::all(Val::Px(1.)), align_self: AlignSelf::FlexStart, ..default() }, BorderColor::all(color), children![k.text(text, 10.5, color, 2)])
}
const MEASURED: Color = Color::srgb(0.94, 0.59, 0.24);

/// The live line of an equation card (updated every frame).
#[derive(Component)]
pub(crate) struct EquationText(pub String);

pub(super) fn equation_card(col: &mut ChildSpawnerCommands, k: &Kit, l: &Learn, b: &sim_lesson::Block, e: &Equation) {
    col.spawn((frame(), super::ui::BlockNode(b.id.clone()))).with_children(|c| {
        c.spawn(badge(k, "Equation, live", ACCENT));
        let text = equation_text(l, e);
        c.spawn((k.mono(text, 15., TEXT), EquationText(e.id.clone())));
        let symbols: Vec<String> = e.terms.values().map(|t| {
            let from = match (&t.param, &t.observe) {
                (Some(p), _) => format!("from the model ({p})"),
                (_, Some(o)) => format!("from the run ({o}) at the playhead"),
                _ => "fixed".into(),
            };
            format!("{} {}", t.symbol, from)
        }).collect();
        c.spawn(k.text(symbols.join(" · "), 11., FAINT, 0));
        if !e.caption.is_empty() {
            c.spawn(k.text(&e.caption, 12.5, SUBTLE, 0));
        }
        if let Some(scene) = &e.scene {
            if l.scene.as_ref().is_none_or(|a| &a.id != scene) {
                c.spawn(k.button("Show the scene", LessonAction::Activate(scene.clone()), Look::Ghost, true));
            }
        }
    });
}

/// The equation with its values at the live scene's time (or why not yet).
pub(crate) fn equation_text(l: &Learn, e: &Equation) -> String {
    let constants = match l.model.constants.get(&e.id) {
        Some(Ok(c)) => c,
        Some(Err(err)) => return format!("{}: {err}", e.show.clone().unwrap_or_else(|| e.expr.clone())),
        None => return format!("{}   (reading the model…)", e.show.clone().unwrap_or_else(|| e.expr.clone())),
    };
    let active = l.scene.as_ref().filter(|a| e.scene.as_ref() == Some(&a.id));
    let observed = e.terms.values().any(|t| t.observe.is_some());
    match (active.and_then(|a| a.run.as_ref().map(|r| (r, a.time))), observed) {
        (Some((run, t)), _) => lm::equation_at(e, constants, Some(run), t).map(|v| format!("{}   at t = {:.3} s", v.text, t)).unwrap_or_else(|err| err),
        (None, false) => lm::equation_at(e, constants, None, 0.).map(|v| v.text).unwrap_or_else(|err| err),
        (None, true) => format!("{}   (show the scene to fill in the values)", e.show.clone().unwrap_or_else(|| e.expr.clone())),
    }
}

/// Keep equation lines in step with the playhead.
pub(super) fn live_equations(learn: Res<Learn>, mut texts: Query<(&mut Text, &EquationText)>) {
    let Some(lesson) = learn.lesson.as_ref() else { return };
    for (mut text, eq) in &mut texts {
        if let Some((_, e)) = lesson.equations().find(|(_, e)| e.id == eq.0) {
            let t = equation_text(&learn, e);
            if text.0 != t {
                text.0 = t;
            }
        }
    }
}

pub(super) fn measured_card(col: &mut ChildSpawnerCommands, k: &Kit, l: &Learn, b: &sim_lesson::Block, m: &Measured, theme: &crate::markdown::Theme) {
    col.spawn((frame(), super::ui::BlockNode(b.id.clone()))).with_children(|c| {
        c.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, ..default() }).with_children(|r| {
            r.spawn(k.text(if m.title.is_empty() { &m.id } else { &m.title }, 14., TEXT, 2));
            r.spawn(badge(k, "Measured beside simulated", MEASURED));
        });
        match l.model.measured.get(&m.id) {
            None => {
                c.spawn(k.text("Simulating each measured point on the detailed model…", 12., FAINT, 0));
            }
            Some(Err(e)) => {
                c.spawn(k.text(format!("Could not compare: {e}"), 12., WARN, 0));
            }
            Some(Ok(r)) => {
                c.spawn(k.text(format!("{} ({}) against {} · orange: measured · teal: simulated", if m.y.label.is_empty() { &m.y.field } else { &m.y.label }, m.y.unit, if m.x.label.is_empty() { &m.x.field } else { &m.x.label }), 11.5, SUBTLE, 1));
                if let Some(image) = l.model.measured_images.get(&m.id) {
                    c.spawn(k.chart_image(image.clone(), Node { border_radius: BorderRadius::all(Val::Px(4.)), width: Val::Percent(100.), aspect_ratio: Some(720. / 200.), flex_shrink: 0., ..default() }, false));
                }
                let (lo, hi) = r.points.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p.x), b.max(p.x)));
                c.spawn(k.text(format!("{} {} … {}", m.x.field, num(lo), num(hi)), 10.5, FAINT, 0));
                // The table: every point, its simulation and the gap.
                let mut rows = vec![vec![m.x.field.clone(), "measured".into(), "simulated".into(), "gap".into()]];
                for p in &r.points {
                    rows.push(vec![num(p.x), format!("{:.3}", p.measured), format!("{:.3}", p.simulated), format!("{:+.3}", p.simulated - p.measured)]);
                }
                let md = rows.iter().enumerate().map(|(i, row)| {
                    let line = format!("| {} |", row.join(" | "));
                    if i == 0 { format!("{line}\n|---:|---:|---:|---:|") } else { line }
                }).collect::<Vec<_>>().join("\n");
                crate::markdown::render(c, &sim_markdown::parse(&md), theme, |_| None::<LessonAction>);
                c.spawn(k.text(format!("RMS gap {:.3} {} · largest {:.3} {} · {}", r.rms, m.y.unit, r.max_gap, m.y.unit, r.fidelity), 12., TEXT, 1));
                for check in &r.checks {
                    c.spawn(k.text(format!("{} {}", if check.passed { "holds:" } else { "fails:" }, check.message), 11.5, if check.passed { OK } else { WARN }, 0));
                }
                if r.fitted_to_data {
                    c.spawn((Node { padding: UiRect::all(Val::Px(10.)), border: UiRect::left(Val::Px(3.)), flex_shrink: 0., ..default() }, BorderColor::all(MEASURED), BackgroundColor(SURFACE), children![k.text("Some of the model's parameters were fitted to this very data. Agreement here shows the fit worked; it is not an independent test of the model.", 12., TEXT, 0)]));
                }
                c.spawn(k.text(format!("Data: {} · source {}{}", r.data.description, r.data.source_path, if r.data.source_hash.is_empty() { String::new() } else { format!(" ({}…)", &r.data.source_hash[..r.data.source_hash.len().min(12)]) }), 10.5, FAINT, 0));
                // Where each number came from.
                c.spawn(k.section("Where the model's numbers come from"));
                for kind in ["measured", "derived", "estimated"] {
                    let rows: Vec<&lm::ParameterOrigin> = r.origins.iter().filter(|o| o.kind == kind).collect();
                    if rows.is_empty() {
                        continue;
                    }
                    let color = match kind {
                        "measured" => OK,
                        "derived" => ACCENT,
                        _ => MEASURED,
                    };
                    c.spawn(Node { column_gap: Val::Px(8.), align_items: AlignItems::FlexStart, flex_wrap: FlexWrap::Wrap, row_gap: Val::Px(4.), ..default() }).with_children(|row| {
                        row.spawn(badge(k, kind, color));
                        for o in rows {
                            row.spawn(k.text(format!("{} = {} {}", o.target, sim_lesson::units::significant(o.value, 3), o.unit), 11.5, TEXT, 0));
                        }
                    });
                }
            }
        }
        if !m.caption.is_empty() {
            c.spawn(k.text(&m.caption, 12., SUBTLE, 0));
        }
    });
}

/// A remedy, shown under the question whose wrong option pointed at it.
pub(super) fn remedy_card(c: &mut ChildSpawnerCommands, k: &Kit, l: &Learn, id: &str, theme: &crate::markdown::Theme) {
    let Some(r) = l.lesson.as_ref().and_then(|x| x.remedy(id)) else { return };
    c.spawn((Node { border_radius: BorderRadius::all(Val::Px(6.)), flex_direction: FlexDirection::Column, row_gap: Val::Px(8.), padding: UiRect::all(Val::Px(12.)), border: UiRect::left(Val::Px(3.)), flex_shrink: 0., ..default() }, BackgroundColor(SURFACE), BorderColor::all(WARN))).with_children(|c| {
        c.spawn(k.text(format!("A common idea that trips people up: {}", r.misconception), 12.5, WARN, 2));
        crate::markdown::render(c, &sim_markdown::parse(&r.body), theme, |_| None::<LessonAction>);
        c.spawn(wrap()).with_children(|row| {
            if let Some(scene) = &r.scene {
                row.spawn(k.button("See it in the scene", LessonAction::ShowAt { scene: scene.clone(), time: None, part: None }, Look::Ghost, true));
            }
            if let Some(q) = &r.then {
                row.spawn(k.button("Try the follow-up question", LessonAction::Goto(q.clone()), Look::Ghost, true));
            }
        });
    });
}

pub(super) fn task_card(col: &mut ChildSpawnerCommands, k: &Kit, l: &Learn, b: &sim_lesson::Block, t: &Task, theme: &crate::markdown::Theme, scene: &SpatialScene) {
    let scene_id = format!("task-{}", t.id);
    let live = l.scene.as_ref().is_some_and(|a| a.id == scene_id);
    let state = l.tasks.get(&t.id);
    let attempts = l.slug().and_then(|s| l.progress.lessons.get(s)).and_then(|p| p.tasks.get(&t.id)).cloned().unwrap_or_default();
    let solved = attempts.iter().any(|a| a.passed);
    // No background: the 3D view of the task's scene is drawn under the page.
    col.spawn((frame_on(Color::NONE), super::ui::BlockNode(b.id.clone()))).with_children(|c| {
        c.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, ..default() }).with_children(|r| {
            r.spawn(k.text(if t.title.is_empty() { &t.id } else { &t.title }, 15., TEXT, 2));
            r.spawn(badge(k, match (t.kind, solved) { (_, true) => "Solved", (TaskKind::Fault, _) => "Find the fault", (TaskKind::Design, _) => "Design task" }, if solved { OK } else { ACCENT }));
        });
        crate::markdown::render(c, &sim_markdown::parse(&t.goal), theme, |_| None::<LessonAction>);
        if !live {
            c.spawn(wrap()).with_children(|r| {
                r.spawn(k.button(if attempts.is_empty() { "Start the task" } else { "Continue the task" }, LessonAction::TaskStart(t.id.clone()), Look::Primary, true));
            });
            c.spawn(k.text("The task gets its own copy of the system. Change it in the builder; each time you come back, the simulation runs your copy and checks the goal.", 11.5, FAINT, 0));
        }
        if live {
            if let Some(task_scene) = l.lesson.as_ref().and_then(|lesson| lm::task_scene(lesson, t, &BTreeMap::new()).ok()) {
                let mut block = b.clone();
                block.id = scene_id.clone();
                super::ui::scene_card(c, k, l, &block, &task_scene, 0, scene);
            }
            c.spawn(wrap()).with_children(|r| {
                r.spawn(k.button("Open my copy in the builder", LessonAction::OpenBuilder, Look::Primary, l.scene.as_ref().is_some_and(|a| a.installed)));
                r.spawn(k.button("Check again", LessonAction::TaskCheck(t.id.clone()), Look::Ghost, true));
                r.spawn(k.button("Start over", LessonAction::ResetSandbox, Look::Ghost, true));
            });
        }
        // The latest judgement.
        if let Some(Some(Ok(res))) = state.map(|s| s.last.as_ref().map(|r| r.as_ref().map_err(|e| e.clone()))) {
            let modified = l.scene.as_ref().filter(|a| a.id == scene_id).and_then(|a| a.sandbox.as_ref()).is_some_and(|s| s.modified);
            c.spawn(k.text(match (res.passed, modified) { (true, _) => "The goal holds on this run.", (false, false) => "Not yet: this is the starting copy. Change it in the builder.", (false, true) => "Not yet: your change runs, but the goal does not hold." }, 13., if res.passed { OK } else { WARN }, 2));
            for check in &res.checks {
                c.spawn(k.text(format!("{} {}", if check.passed { "met:" } else { "not yet:" }, if check.why.is_empty() { &check.message } else { &check.why }), 11.5, if check.passed { OK } else { SUBTLE }, 0));
            }
        }
        if !attempts.is_empty() && !t.report.is_empty() {
            c.spawn(k.section("Your attempts"));
            let mut md = format!("| # | {} | goal |\n|---:|{}---|", t.report.iter().map(|m| format!("{} ({})", m.label, m.unit)).collect::<Vec<_>>().join(" | "), "---:|".repeat(t.report.len()));
            for (i, a) in attempts.iter().enumerate() {
                let values: Vec<String> = t.report.iter().map(|m| a.metrics.iter().find(|(l, _)| l == &m.label).map(|(_, v)| sim_lesson::units::significant(*v, 3)).unwrap_or_else(|| "—".into())).collect();
                md.push_str(&format!("\n| {} | {} | {} |", i + 1, values.join(" | "), if a.passed { "met" } else { "not yet" }));
            }
            crate::markdown::render(c, &sim_markdown::parse(&md), theme, |_| None::<LessonAction>);
        }
        let shown = state.map(|s| s.hints).unwrap_or(0).min(t.hints.len());
        for h in &t.hints[..shown] {
            c.spawn(k.text(format!("Hint: {h}"), 12.5, SUBTLE, 0));
        }
        if shown < t.hints.len() && !solved {
            c.spawn(wrap()).with_children(|r| {
                r.spawn(k.button(&format!("Show a hint ({} of {})", shown + 1, t.hints.len()), LessonAction::TaskHint(t.id.clone()), Look::Ghost, true));
            });
        }
    });
}

pub(super) fn lab_card(col: &mut ChildSpawnerCommands, k: &Kit, l: &Learn, b: &sim_lesson::Block, lab: &Lab, theme: &crate::markdown::Theme) {
    let state = l.labs.get(&lab.id);
    let configured = lesson_lab::bench_config();
    let bench = configured.as_ref().ok().and_then(|p| p.as_ref());
    col.spawn((frame(), super::ui::BlockNode(b.id.clone()))).with_children(|c| {
        c.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, ..default() }).with_children(|r| {
            r.spawn(k.text(if lab.title.is_empty() { &lab.id } else { &lab.title }, 15., TEXT, 2));
            r.spawn(badge(k, "Hardware lab", MEASURED));
        });
        c.spawn(k.text(format!("Joint: {} · drive {:.0} % for {} s", lab.joint, lab.test.duty * 100., num(lab.test.seconds)), 12., SUBTLE, 1));
        if !lab.notes.is_empty() {
            crate::markdown::render(c, &sim_markdown::parse(&lab.notes), theme, |_| None::<LessonAction>);
        }
        // 1. Predict.
        if let Some(p) = &lab.predict {
            c.spawn(k.section("1 · Your prediction"));
            crate::markdown::render(c, &sim_markdown::parse(p), theme, |_| None::<LessonAction>);
            let focused = l.input.as_ref().is_some_and(|i| i.purpose == Purpose::LabPrediction(lab.id.clone()));
            let shown = if focused { l.input.as_ref().map(|i| i.buffer.clone()).unwrap_or_default() } else { state.map(|s| s.prediction.clone()).unwrap_or_default() };
            c.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(8.), ..default() }).with_children(|r| {
                r.spawn(Node { width: Val::Px(180.), flex_direction: FlexDirection::Column, ..default() }).with_children(|w| {
                    w.spawn(k.input(&shown, "Type a number", LessonAction::LabPredict(lab.id.clone()), focused));
                });
                r.spawn(k.text(&lab.unit, 13., SUBTLE, 0));
            });
        }
        // 2. What the model and the recorded measurements say.
        c.spawn(k.section("2 · What we expect"));
        match l.model.labs.get(&lab.id) {
            None => {
                c.spawn(k.text("Simulating the test on the detailed model…", 12., FAINT, 0));
            }
            Some(Err(e)) => {
                c.spawn(k.text(format!("No model prediction: {e}"), 12., WARN, 0));
            }
            Some(Ok(p)) => {
                k.property(c, "Model", &format!("{:.3}", p.model), &format!("{} ({})", p.unit, p.fidelity), None::<LessonAction>, false);
                if let Some(r) = p.registry {
                    k.property(c, "Recorded measurement at this drive", &format!("{r:.3}"), &p.unit, None::<LessonAction>, false);
                }
            }
        }
        // 3. The bench.
        c.spawn(k.section("3 · On the bench"));
        for (i, item) in sim_lesson::blocks::LAB_CHECKLIST.iter().enumerate() {
            let ticked = state.is_some_and(|s| s.ticks[i]);
            c.spawn(k.button(&format!("{} {item}", if ticked { "[x]" } else { "[ ]" }), LessonAction::LabTick(lab.id.clone(), i), Look::Chip(ticked), true));
        }
        let ready = bench.is_some() && state.is_some_and(|s| s.ticks.iter().all(|t| *t)) && (lab.predict.is_none() || state.is_some_and(|s| sim_lesson::units::split_quantity(&s.prediction).is_some()));
        let running = state.is_some_and(|s| s.running);
        c.spawn(wrap()).with_children(|r| {
            r.spawn(k.button("STOP motors", LessonAction::LabStop, Look::Danger, true));
            r.spawn(k.button(if running { "Running on the bench…" } else { "Run on the bench" }, LessonAction::LabRun(lab.id.clone()), Look::Primary, ready && !running));
        });
        match &bench {
            None => {
                c.spawn(k.text(configured.as_ref().err().map(String::as_str).unwrap_or("Set SIM_BENCH_CONFIG to a local calibration JSON file. No hardware server is used. The operator, supported fixture, taught windows, independent supervisor and STOP remain required."), 11.5, FAINT, 0));
            }
            Some(path) => {
                c.spawn(k.text(format!("Local bench: {} · shared calibration acquisition under the campaign guards.", path.display()), 11.5, FAINT, 0));
            }
        }
        // 4. Compare.
        if let Some(result) = state.and_then(|s| s.result.as_ref()) {
            c.spawn(k.section("4 · Compare"));
            match result {
                Err(e) => {
                    c.spawn(k.text(format!("The bench stopped: {e}"), 12.5, WARN, 1));
                }
                Ok(v) => {
                    let value = v["steady_rad_s"].as_f64();
                    let mut rows = vec![];
                    if let Some(p) = state.and_then(|s| sim_lesson::units::read(&s.prediction, &lab.unit).ok()) {
                        rows.push(("Your prediction", p.value));
                    }
                    if let Some(Ok(p)) = l.model.labs.get(&lab.id) {
                        rows.push(("Model", p.model));
                        if let Some(r) = p.registry {
                            rows.push(("Recorded measurement", r));
                        }
                    }
                    match value {
                        Some(v) => rows.push(("Your bench", v)),
                        None => {
                            c.spawn(k.text("The joint did not reach a steady speed before it had to stop.", 12.5, WARN, 0));
                        }
                    }
                    let md = format!("| | {} |\n|---|---:|\n{}", lab.unit, rows.iter().map(|(label, v)| format!("| {label} | {v:.3} |")).collect::<Vec<_>>().join("\n"));
                    crate::markdown::render(c, &sim_markdown::parse(&md), theme, |_| None::<LessonAction>);
                    if let Some(s) = v["stopped"].as_str() {
                        c.spawn(k.text(s, 11.5, SUBTLE, 0));
                    }
                }
            }
        }
    });
}

/// Seconds on screen per block, and visits; flushed to progress now and then.
#[derive(Default)]
pub(crate) struct Tracking {
    pending: BTreeMap<String, f64>,
    visible: BTreeSet<String>,
    flushed: f64,
}

pub(super) fn track_blocks(time: Res<Time>, mut learn: ResMut<Learn>, blocks: Query<(&super::ui::BlockNode, &ComputedNode, &UiGlobalTransform, &InheritedVisibility)>, window: Single<&Window>) {
    if !learn.active || learn.lesson.is_none() {
        return;
    }
    let dt = time.delta_secs_f64().min(0.5);
    let height = window.physical_height() as f32;
    let mut now_visible = BTreeSet::new();
    for (node, computed, gt, vis) in &blocks {
        if !vis.get() || computed.size().y <= 0. {
            continue;
        }
        let (top, bottom) = (gt.translation.y - computed.size().y * 0.5, gt.translation.y + computed.size().y * 0.5);
        // Counted when at least part of it is in the middle three-fifths of the window.
        if bottom > height * 0.2 && top < height * 0.8 {
            now_visible.insert(node.0.clone());
        }
    }
    let Some(slug) = learn.slug().map(String::from) else { return };
    let newly: Vec<String> = now_visible.difference(&learn.tracking.visible).cloned().collect();
    for id in &now_visible {
        *learn.tracking.pending.entry(id.clone()).or_default() += dt;
    }
    for id in newly {
        learn.progress.block(&slug, &id).visits += 1;
    }
    learn.tracking.visible = now_visible;
    let t = time.elapsed_secs_f64();
    if t - learn.tracking.flushed > 15. {
        learn.tracking.flushed = t;
        let pending = std::mem::take(&mut learn.tracking.pending);
        for (id, secs) in pending {
            learn.progress.block(&slug, &id).seconds += secs;
        }
        let path = learn.progress_path.clone();
        let _ = learn.progress.save(&path);
    }
}
