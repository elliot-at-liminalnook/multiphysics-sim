//! Polling: jobs, file watching, notes, the agent and the reader's settings;
//! the run's chart images.
use super::*;

/// Reader preferences that live outside the page: text size and reduced motion.
pub(super) fn apply_settings(learn: Res<Learn>, mut ui_scale: ResMut<UiScale>, mut scene: ResMut<SpatialScene>) {
    if !learn.is_changed() {
        return;
    }
    let s = learn.settings.text_scale.clamp(0.8, 1.6);
    if (ui_scale.0 - s).abs() > 1e-3 {
        ui_scale.0 = s;
    }
    if scene.reduced_motion != learn.settings.reduced_motion {
        scene.reduced_motion = learn.settings.reduced_motion;
    }
}

/// Jobs, file watching, notes and the agent.
#[allow(clippy::too_many_arguments)]
pub(super) fn poll(
    mut commands: Commands,
    time: Res<Time>,
    mut learn: ResMut<Learn>,
    builder: Option<ResMut<Builder>>,
    mut scene: ResMut<SpatialScene>,
    mut images: ResMut<Assets<Image>>,
) {
    let now = time.elapsed_secs_f64();
    // Lesson file and index.
    if now - learn.checked > 0.5 {
        learn.checked = now;
        if let Some(path) = learn.lesson.as_ref().map(|l| l.path.clone()) {
            if lesson_stamp(&path) != learn.stamp {
                let before = learn.lesson.as_ref().map(|l| l.source.clone());
                learn.load(&path);
                if learn.lesson.as_ref().map(|l| l.source.clone()) != before {
                    learn.status = "Reloaded: lesson.md changed on disk.".into();
                    // A changed scene block re-records.
                    let changed = learn.scene.as_ref().and_then(|a| learn.lesson.as_ref().map(|l| l.scene(&a.id) != Some(&a.scene)));
                    if changed == Some(true) {
                        if let Some(id) = learn.scene.as_ref().map(|a| a.id.clone()) {
                            learn.activate(&id);
                        }
                    }
                }
            }
        }
        learn.load_figures();
        if now - learn.scanned > 3.0 {
            learn.scanned = now;
            let entries = sim_lesson::index::scan(&learn.dir);
            if entries != learn.entries {
                learn.entries = entries;
                learn.dirty = true;
            }
            let categories = sim_lesson::categories::load(&learn.dir).unwrap_or_default();
            if categories != learn.categories {
                learn.categories = categories;
                learn.dirty = true;
            }
        }
    }
    // Notes.
    if let Some(notes) = &learn.notes {
        let doc = notes.document();
        let error = notes.error();
        if doc.revision != learn.notes_doc.revision || doc.subject != learn.notes_doc.subject {
            learn.notes_doc = doc;
            learn.dirty = true;
        }
        if let Some(e) = error.filter(|e| !learn.status.contains(e.as_str())) {
            learn.status = format!("Notes: {e}");
            learn.dirty = true;
        }
    }
    let pending = std::mem::take(&mut learn.pending);
    for (id, label) in pending {
        match learn.notes.as_mut().and_then(|n| n.result(id)) {
            None => learn.pending.push((id, label)),
            Some(Ok(doc)) => {
                learn.notes_doc = doc;
                learn.status = format!("{label} · saved");
                learn.dirty = true;
            }
            Some(Err(e)) => {
                learn.status = format!("{label}: {e}");
                learn.dirty = true;
            }
        }
    }
    agent::tick(&mut learn, now);
    if learn.poll_figures(&mut images) {
        learn.dirty = true;
    }
    // Comparisons.
    let mut finished = false;
    for state in learn.compares.values_mut() {
        let received = state.job.as_ref().and_then(crate::jobs::Job::poll);
        if let Some(result) = received {
            state.result = Some(result);
            state.job = None;
            finished = true;
        }
    }
    if finished {
        learn.dirty = true;
    }
    // Model values, bench results, and notes waiting to go to Codex.
    if learn.poll_model(&mut images) {
        learn.dirty = true;
        let ids: Vec<String> = learn.labs.iter().filter(|(_, s)| s.result.as_ref().is_some_and(|r| r.is_ok())).map(|(id, _)| id.clone()).collect();
        for id in ids {
            learn.record_lab(&id);
        }
    }
    if let Some(t) = learn.ask_when_saved.clone() {
        if learn.notes_doc.threads.contains_key(&t) {
            learn.ask_when_saved = None;
            match learn.agent_input(&t).and_then(|input| learn.agent.ask(input)) {
                Ok(_) => learn.status = "Asked Codex about this moment; the reply is posted to the note.".into(),
                Err(e) => learn.status = format!("Codex: {e}"),
            }
            learn.dirty = true;
        }
    }
    // The active scene's jobs.
    let label = learn.lesson.as_ref().map(|l| l.meta.title.clone());
    let mut builder = builder;
    let Some(a) = learn.scene.as_mut() else { return };
    let stage = match a.jobs.as_ref().and_then(crate::jobs::Job::next_update) {
        Some(stage) => Some(stage),
        // Every stage is queued before the job's result and the closures only
        // return Ok, so an error with nothing queued is a panic: show it as the
        // run's error instead of leaving the scene "recording" forever.
        None => match a.jobs.as_ref().and_then(crate::jobs::Job::poll) {
            Some(Err(e)) => Some(Stage::Run(Err(e))),
            _ => None,
        },
    };
    let mut parts = None;
    let mut judged: Option<Arc<SceneRun>> = None;
    let mut changed = stage.is_some();
    let received = matches!(stage, Some(Stage::Builder(Ok(_))));
    match stage {
        Some(Stage::Builder(Ok((sb, mut b)))) => {
            let _ = b.set_level(&a.scene.level);
            b.lesson = label;
            if let Ok(doc) = runtime::load_system(&sb.path, b.registry()) {
                parts = Some((a.id.clone(), runtime::instance_paths(&doc)));
            }
            a.sandbox = Some(sb);
            a.installed = false;
            a.framed = false;
            commands.insert_resource(*b);
        }
        Some(Stage::Builder(Err(e))) => {
            a.error = Some(e);
            a.recording = false;
            a.jobs = None;
        }
        Some(Stage::Run(Ok(run))) => {
            a.recording = false;
            a.error = None;
            a.charts = charts(&run, &a.scene, &a.timeline, &mut images);
            a.phase_charts = phase_charts(&run, &a.scene, &mut images);
            let rules = if a.explore { sim_script::pacing::PacingRules::exploring() } else { Default::default() };
            a.plan = run.pacing_with(&a.scene, &a.timeline, &rules).unwrap_or_default();
            a.challenge = a.scene.challenge.as_ref().map(|c| runtime::check_claims(&run, &c.win));
            if !a.overrides.is_empty() && a.challenge.as_ref().is_some_and(|r| r.iter().all(|x| x.passed)) {
                a.challenge_met = Some(serde_json::to_string(&a.overrides).unwrap_or_default());
            }
            a.run = Some(Arc::new(run));
            judged = a.run.clone();
            let t = a.time;
            a.seek(t);
            a.playing = a.scene.autoplay;
            // A companion run follows on the same channel (first recording only).
            if a.scene.companion.is_none() || !a.overrides.is_empty() || a.companion.is_some() {
                a.jobs = None;
            }
        }
        Some(Stage::Companion(result)) => {
            a.jobs = None;
            match result {
                Ok(run) => {
                    let run = Arc::new(run);
                    if let Some(main) = a.run.clone() {
                        a.charts = charts_with(&main, Some(&run), &a.scene, &a.timeline, &mut images);
                    }
                    a.companion = Some(run);
                }
                Err(e) if e != "cancelled" => a.error = Some(format!("companion run: {e}")),
                Err(_) => {}
            }
        }
        Some(Stage::Run(Err(e))) => {
            a.recording = false;
            a.jobs = None;
            if e != "cancelled" {
                a.error = Some(e);
            }
        }
        None => {}
    }
    // Installed once the builder on this sandbox has compiled the scene (the
    // builder received this frame is inserted only after this system).
    if !a.installed && !received {
        if let (Some(b), Some(sb)) = (builder.as_deref_mut(), a.sandbox.as_ref()) {
            if b.path() == sb.path && b.compile_settled() {
                a.installed = true;
                if let Some(e) = b.compile_error() {
                    a.error = Some(format!("Does not compile: {e}"));
                }
                scene.set_changed();
                changed = true;
            }
        }
    }
    if let (Some((id, parts)), Some(index)) = (parts, learn.index.as_mut()) {
        index.with_parts(&id, parts);
    }
    // Sketch questions: draw the reader's curve over the run once both exist.
    let pending: Vec<(sim_lesson::quiz::Quiz, String)> = learn.lesson.as_ref().zip(learn.slug()).map(|(lesson, slug)| {
        let run_key = learn.scene.as_ref().and_then(|a| a.run.as_ref().map(|r| r.key.clone()));
        lesson.quizzes().filter(|(_, q)| q.kind == sim_lesson::quiz::QuizKind::Sketch && learn.sketch_results.get(&q.id).map(|r| &r.2) != run_key.as_ref())
            .filter_map(|(_, q)| Some((q.clone(), learn.progress.quiz(slug, &q.id)?.prediction.clone()?))).collect()
    }).unwrap_or_default();
    for (q, prediction) in pending {
        let window = learn.sketch_window(&q);
        let Some(run) = learn.scene.as_ref().filter(|a| q.scene.as_ref() == Some(&a.id)).and_then(|a| a.run.clone()) else { continue };
        if let Some((image, gap)) = practice::sketch_result(&run, &q, &prediction, window) {
            learn.sketch_results.insert(q.id.clone(), (images.add(image), gap, run.key.clone()));
            learn.dirty = true;
        }
    }
    if changed {
        learn.dirty = true;
    }
    if let Some(run) = judged {
        learn.judge_task_run(&run);
    }
}

/// Operating-point images: y against x over the whole run.
fn phase_charts(run: &SceneRun, scene: &Scene, images: &mut Assets<Image>) -> Vec<PhaseChart> {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    scene
        .phase
        .iter()
        .filter_map(|p| {
            let (x, y) = (run.series(&p.x)?, run.series(&p.y)?);
            // Pair y with x at x's sample times (both recorded per step).
            let points: Vec<[f64; 2]> = x.times.iter().zip(&x.values).filter_map(|(t, xv)| {
                let i = y.times.partition_point(|u| u < t);
                y.values.get(i).filter(|_| y.times.get(i).is_some_and(|u| (u - t).abs() < 1e-9)).map(|yv| [*xv, *yv])
            }).collect();
            let (pixels, range, window) = crate::chart::rasterize_span(&[(&points, [120, 150, 190])], None);
            let (w, h) = crate::chart::RASTER;
            let image = Image::new(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, pixels, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default());
            Some(PhaseChart {
                title: if p.title.is_empty() { format!("{} against {}", y.label, x.label) } else { p.title.clone() },
                x: (x.label.clone(), x.unit.clone()),
                y: (y.label.clone(), y.unit.clone()),
                ids: (x.observable.clone(), y.observable.clone()),
                image: images.add(image),
                range,
                window,
            })
        })
        .collect()
}

/// Chart images for the run's plotted observables.
fn charts(run: &SceneRun, scene: &Scene, timeline: &Timeline, images: &mut Assets<Image>) -> Vec<Chart> {
    charts_with(run, None, scene, timeline, images)
}

/// Charts with a companion run's curve drawn faintly behind each one.
fn charts_with(run: &SceneRun, companion: Option<&SceneRun>, scene: &Scene, timeline: &Timeline, images: &mut Assets<Image>) -> Vec<Chart> {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let keys = runtime::plot_keys(scene, timeline);
    let colors = [[77, 212, 191], [222, 148, 84], [140, 170, 255], [235, 110, 160]];
    keys.iter()
        .take(6)
        .enumerate()
        .filter_map(|(i, key)| {
            let s = run.series(key)?;
            let points: Vec<[f64; 2]> = s.times.iter().zip(&s.values).map(|(t, v)| [*t, *v]).collect();
            // The companion's curve first, in a faint purple, so this run's draws on top.
            let other: Vec<[f64; 2]> = companion.and_then(|c| c.series(key)).map(|c| c.times.iter().zip(&c.values).map(|(t, v)| [*t, *v]).collect()).unwrap_or_default();
            let (pixels, range, window) = crate::chart::rasterize_span(&[(&other, [120, 96, 170]), (&points, colors[i % colors.len()])], Some(crate::builder::HISTORY_SECONDS));
            let (w, h) = crate::chart::RASTER;
            let image = Image::new(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, pixels, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default());
            Some(Chart { key: key.clone(), unit: s.unit.clone(), image: images.add(image), range, window })
        })
        .collect()
}
