//! Live runs: the run session on its own thread (Run, Pause, Step, Reset),
//! its recorded history and graph pins, and the Alt-drag grab that pushes
//! on a running system.
use super::*;

impl LiveRun {
    /// The one way a run session starts: `run_thread` on a `RunThread`.
    pub(super) fn spawn(document: SystemDocument, registry: BehaviorRegistry, observed: Vec<String>, description_id: String, fidelity: Fidelity) -> Self {
        let initial = RunShared { snapshot: None, running: true, speed: 1.0, history: BTreeMap::new(), reset_pending: false };
        let (simulated, source_id) = (document.clone(), description_id.clone());
        let worker = crate::jobs::RunThread::spawn("builder-run", initial, move |commands, shared| run_thread(simulated, registry, observed, source_id, commands, shared));
        Self { worker, description_id, fidelity, document, edited: false }
    }
}

impl Builder {
    pub(super) fn stop_run(&mut self) {
        // Keep every run that got anywhere.
        if self.run.as_ref().is_some_and(|r| r.worker.shared().lock().ok().and_then(|s| s.snapshot.as_ref().and_then(|x| x.status.as_ref()).map(|x| x.time)).unwrap_or(0.) >= 0.1) {
            let _ = self.save_run("");
        }
        self.run = None;
    }

    /// Keep the current run: its document, seed, settings and recorded history.
    pub fn save_run(&mut self, note: &str) -> Result<PathBuf, String> {
        let run = self.run.as_ref().ok_or("nothing is running")?;
        let (duration, history) = {
            let s = run.worker.shared().lock().map_err(|_| "run state unavailable")?;
            if s.reset_pending {
                return Err("reset in progress; save again in a moment".into());
            }
            (s.snapshot.as_ref().and_then(|x| x.status.as_ref()).map(|x| x.time).unwrap_or(0.), s.history.clone())
        };
        // The model the run simulated last: every swap (edit or grab) goes
        // through `hot_swap`, which keeps `run.document` equal to what it sent.
        let document = run.document.clone();
        let description = self.last_description.clone();
        let series = history
            .into_iter()
            .map(|(id, points)| {
                let (label, unit) = description.as_ref().map(|d| (system_builder::observable_key(d, &id), d.observables.get(&id).map(|o| sim_inspect::plot::unit(d, o).to_string()).unwrap_or_default())).unwrap_or((id.clone(), String::new()));
                system_builder::Series { observable: id, label, unit, times: points.iter().map(|p| p[0]).collect(), values: points.iter().map(|p| p[1]).collect() }
            })
            .collect();
        let note = if run.edited { format!("{note}{}{}", if note.is_empty() { "" } else { "; " }, sim_runtime::run_history::EDITED_WHILE_RUNNING) } else { note.to_string() };
        let record = sim_runtime::run_history::RunRecord::new(&document, system_builder::config_for(&document), duration, series, &note).with_provenance(run.fidelity, run.edited);
        let path = sim_runtime::run_history::save(&sim_runtime::run_history::dir_for(&self.store.path), &record)?;
        self.runs = sim_runtime::run_history::list(&sim_runtime::run_history::dir_for(&self.store.path));
        self.status = format!("Saved run {} ({:.2} s)", record.id, duration);
        self.panel_dirty = true;
        Ok(path)
    }

    pub(super) fn start_run(&mut self, scene: &SpatialScene) {
        if let Some(run) = &self.run {
            if run.description_id == scene.description.id {
                let _ = run.worker.send(RunControl::Start);
                self.status = RUNNING_STATUS.into();
                return;
            }
        }
        let fidelity = if self.realtime { Fidelity::Realtime } else { Fidelity::Detailed };
        let document = match fidelity.document(&self.document, &self.registry) {
            Ok(d) => d,
            Err(e) => {
                self.status = format!("Realtime profile: {}", e.trim_start_matches("realtime profile: "));
                return;
            }
        };
        let registry = self.registry.clone();
        let mut observed: Vec<String> = scene
            .animation
            .as_ref()
            .map(|a| a.observables().into_iter().collect())
            .unwrap_or_default();
        observed.extend(graphs::recordable(scene, &self.graphs.pinned));
        observed.sort();
        observed.dedup();
        self.run = Some(LiveRun::spawn(document, registry, observed, scene.description.id.clone(), fidelity));
        self.status = RUNNING_STATUS.into();
    }

    pub(super) fn pause_run(&mut self) {
        if let Some(run) = &self.run {
            let _ = run.worker.send(RunControl::Pause);
            // No time: the last published snapshot may be a frame behind the pause.
            self.status = "Paused (Step advances one timestep; Run resumes).".into();
        }
    }

    /// Swap the live run onto `source` (a detailed document) through the run's
    /// fidelity profile, so a realtime run never receives the detailed model.
    /// `run.document` becomes exactly what was sent and the run counts as
    /// edited. If the profile fails the run is stopped (kept up to the swap)
    /// and the status says why; returns false then. Never touches
    /// `self.document` or the file.
    pub(super) fn hot_swap(&mut self, source: &SystemDocument, description_id: String) -> bool {
        let Some(fidelity) = self.run.as_ref().map(|r| r.fidelity) else { return false };
        match fidelity.document(source, &self.registry) {
            Ok(document) => {
                let run = self.run.as_mut().expect("checked above");
                let _ = run.worker.send(RunControl::Swap(Box::new(document.clone()), description_id.clone()));
                run.description_id = description_id;
                run.document = document;
                run.edited = true;
                true
            }
            Err(e) => {
                // Never continue a realtime run on the detailed model: stop it
                // (kept up to the edit, on the model it ran) and say why.
                self.stop_run();
                self.status = format!("The {} run was stopped (kept up to the edit): the edited system has no valid {e}", fidelity.label());
                false
            }
        }
    }

    /// Ask the running session to record `ids` as well (newly pinned plots).
    pub(crate) fn observe(&mut self, scene: &SpatialScene) {
        if let Some(run) = &self.run {
            let mut ids = graphs::recordable(scene, &self.graphs.pinned);
            if let Some(a) = &scene.animation {
                ids.extend(a.observables());
            }
            ids.sort();
            ids.dedup();
            let _ = run.worker.send(RunControl::Observe(ids));
        }
    }

    /// Recorded history of one observable (empty when not running).
    pub(crate) fn history(&self, id: &str) -> Vec<[f64; 2]> {
        self.run.as_ref().and_then(|r| r.worker.shared().lock().ok().and_then(|s| s.history.get(id).map(|h| h.iter().copied().collect()))).unwrap_or_default()
    }

    /// Pin observables (IDs or readable keys) to the graph dock.
    pub fn set_plots(&mut self, scene: &SpatialScene, pin: Option<Vec<String>>, visible: Option<bool>) -> Result<serde_json::Value, String> {
        if let Some(pin) = pin {
            let mut ids = Vec::new();
            for p in pin {
                let id = if scene.description.observables.contains_key(&p) {
                    p
                } else {
                    scene.description.observables.keys().find(|id| system_builder::observable_key(&scene.description, id) == p).cloned().ok_or_else(|| format!("no observable `{p}`"))?
                };
                ids.push(id);
            }
            ids.truncate(graphs::MAX_CHARTS);
            self.graphs.pinned = ids;
            self.graphs.visible = true;
            self.observe(scene);
        }
        if let Some(v) = visible {
            self.graphs.visible = v;
        }
        self.panel_dirty = true;
        Ok(serde_json::json!({"visible": self.graphs.visible, "pinned": self.graphs.pinned, "charts": self.graphs.charts.iter().map(|c| serde_json::json!({"id": c.id, "title": c.title, "unit": c.unit, "latest": c.latest, "range": [c.range.0, c.range.1], "window": [c.window.0, c.window.1]})).collect::<Vec<_>>()}))
    }

    pub fn run_start(&mut self, scene: &SpatialScene) {
        self.start_run(scene);
        self.panel_dirty = true;
    }
    pub fn run_pause(&mut self) {
        self.pause_run();
        self.panel_dirty = true;
    }

    /// Advance a paused run by exactly one timestep on its run thread
    /// (`Command::Step`). Refused, never ignored, without a paused run.
    pub fn run_step(&mut self) -> Result<(), String> {
        let run = self.run.as_ref().ok_or("nothing is running: start a run, pause it, then step")?;
        {
            let s = run.worker.shared().lock().map_err(|_| "run state unavailable")?;
            if s.reset_pending {
                return Err("reset in progress; step again in a moment".into());
            }
            if s.running {
                return Err("pause the run before stepping".into());
            }
        }
        run.worker.send(RunControl::Step).map_err(|_| "the run has ended; start a new run".to_string())?;
        self.panel_dirty = true;
        Ok(())
    }

    /// Return the live run to t = 0, paused (`Command::Reset` on its run
    /// thread, which rebuilds the model it currently simulates).
    ///
    /// Recorded work is kept, not lost: a run that reached t >= 0.1 s is saved
    /// first (the `stop_run` rule) and its id is named in the status. After the
    /// reset the run simulates `run.document` (the last model swapped in) from
    /// t = 0 with no live edit, so `edited` becomes false; the fidelity is kept.
    /// The graphs are cleared at once and stay closed to pre-reset samples.
    pub fn run_reset(&mut self) -> Result<(), String> {
        let run = self.run.as_ref().ok_or("nothing is running")?;
        if run.worker.shared().lock().map_err(|_| "run state unavailable")?.reset_pending {
            return Err("reset in progress".into());
        }
        let reached = run.worker.shared().lock().ok().and_then(|s| s.snapshot.as_ref().and_then(|x| x.status.as_ref()).map(|x| x.time)).unwrap_or(0.);
        let kept = if reached >= 0.1 { Some(self.save_run("")?) } else { None };
        let run = self.run.as_mut().expect("checked above");
        {
            let mut s = run.worker.shared().lock().map_err(|_| "run state unavailable")?;
            s.reset_pending = true;
            s.history.clear();
        }
        if run.worker.send(RunControl::Reset).is_err() {
            if let Ok(mut s) = run.worker.shared().lock() {
                s.reset_pending = false;
            }
            return Err("the run has ended; start a new run".into());
        }
        run.edited = false;
        let kept = kept.and_then(|p| self.runs.iter().find(|(q, _)| *q == p).map(|(_, s)| s.id.clone()));
        self.status = match kept {
            Some(id) => format!("Reset to t = 0 (paused); kept the previous run as {id}."),
            None => "Reset to t = 0 (paused); the previous run was under 0.1 s and was not kept.".into(),
        };
        self.panel_dirty = true;
        Ok(())
    }

    /// Latest run-thread status: null without a run.
    pub(super) fn live_run_json(&self) -> serde_json::Value {
        let Some(run) = &self.run else { return serde_json::Value::Null };
        let s = run.worker.shared().lock().ok();
        let status = s.as_ref().and_then(|s| s.snapshot.as_ref().and_then(|x| x.status.clone()));
        serde_json::json!({
            "time": status.as_ref().map(|x| x.time),
            "phase": status.as_ref().map(|x| x.phase),
            "step": status.as_ref().map(|x| x.step),
            "generation": status.as_ref().map(|x| x.generation),
            "interval": system_builder::config_for(&run.document).interval,
            "reset_pending": s.as_ref().is_some_and(|s| s.reset_pending),
            "error": s.as_ref().and_then(|s| s.snapshot.as_ref().and_then(|x| x.error.clone())),
            "fidelity": run.fidelity.label(),
            "edited": run.edited,
        })
    }

    pub(super) fn running(&self) -> bool {
        self.run.as_ref().is_some_and(|r| r.worker.shared().lock().map(|s| s.running).unwrap_or(false))
    }
}

fn run_thread(document: SystemDocument, registry: BehaviorRegistry, observed: Vec<String>, source_id: String, commands: mpsc::Receiver<RunControl>, shared: Arc<Mutex<RunShared>>) {
    let source_id = std::cell::RefCell::new(source_id);
    let publish = |status: Option<sim_inspect::live::SessionStatus>, frame: Option<sim_inspect::SampleFrame>, error: Option<String>, running: bool| {
        if let Ok(mut s) = shared.lock() {
            s.snapshot = Some(sim_inspect::live::LiveSnapshot { version: 1, source_description_id: source_id.borrow().clone(), description: None, status, frame, error });
            s.running = running;
        }
    };
    let config = system_builder::config_for(&document);
    let compiled = match system_builder::compile(&document, &registry, config.clone()) {
        Ok(c) => c,
        Err(e) => return publish(None, None, Some(e), false),
    };
    let source = sim_runtime::system_session::ModelSource {
        model: compiled.flat.model.clone(),
        registry: registry.clone(),
        identities: compiled.flat.identities.clone(),
        source_hash: compiled.flat.source_hash.clone(),
        revision: document.revision.max(1),
    };
    let mut session = match sim_runtime::system_session::SystemSession::new(compiled.launch.run_id.clone(), config.clone(), move |c| source.build(c)) {
        Ok(s) => s,
        Err(e) => return publish(None, None, Some(system_builder::locate(&compiled.flat, e)), false),
    };
    if let Err(e) = session.subscribe(observed) {
        return publish(None, None, Some(e), false);
    }
    let _ = session.execute(sim_runtime::system_session::Command::Start);
    let mut running = true;
    let mut pending: Vec<(f64, Vec<(String, f64)>)> = Vec::new();
    let mut last_sample = f64::NEG_INFINITY;
    let mut wall = std::time::Instant::now();
    let mut sim_at_wall = 0.0;
    let mut last_publish = std::time::Instant::now() - std::time::Duration::from_secs(1);
    // A refused step stays visible until the next command.
    let mut step_error: Option<String> = None;
    let sample = |frame: &sim_inspect::SampleFrame| -> (f64, Vec<(String, f64)>) {
        (frame.time, frame.values.keys().filter_map(|id| sim_inspect::animation::scalar(Some(frame), id).map(|v| (id.clone(), v.value))).collect())
    };
    let flush = |pending: &mut Vec<(f64, Vec<(String, f64)>)>| {
        if let Ok(mut s) = shared.lock() {
            if s.reset_pending {
                // Samples from before a requested reset never reach the graphs.
                pending.clear();
                return;
            }
            for (t, values) in pending.drain(..) {
                for (id, v) in values {
                    let h = s.history.entry(id).or_default();
                    h.push_back([t, v]);
                    while h.len() > HISTORY_POINTS || h.front().is_some_and(|f| f[0] < t - HISTORY_SECONDS) {
                        h.pop_front();
                    }
                }
            }
        }
    };
    loop {
        loop {
            let command = commands.try_recv();
            if matches!(command, Ok(RunControl::Start | RunControl::Pause | RunControl::Step | RunControl::Reset)) {
                step_error = None;
            }
            match command {
                Ok(RunControl::Start) => {
                    running = true;
                    let _ = session.execute(sim_runtime::system_session::Command::Start);
                    wall = std::time::Instant::now();
                    sim_at_wall = session.status().time;
                }
                Ok(RunControl::Pause) => {
                    running = false;
                    let _ = session.execute(sim_runtime::system_session::Command::Pause);
                }
                Ok(RunControl::Swap(next, description_id)) => {
                    let config = system_builder::config_for(&next);
                    match system_builder::compile(&next, &registry, config) {
                        Ok(compiled) => {
                            let source = sim_runtime::system_session::ModelSource {
                                model: compiled.flat.model.clone(),
                                registry: registry.clone(),
                                identities: compiled.flat.identities.clone(),
                                source_hash: compiled.flat.source_hash.clone(),
                                revision: next.revision.max(1),
                            };
                            match session.hot_swap(move |c| source.build(c)) {
                                Ok(preserved) => {
                                    *source_id.borrow_mut() = description_id;
                                    if !preserved {
                                        pending.clear();
                                        last_sample = f64::NEG_INFINITY;
                                        if let Ok(mut s) = shared.lock() {
                                            s.history.clear();
                                        }
                                    }
                                    wall = std::time::Instant::now();
                                    sim_at_wall = session.status().time;
                                    if running {
                                        let _ = session.execute(sim_runtime::system_session::Command::Start);
                                    }
                                    publish(Some(session.status().clone()), Some(session.latest().clone()), None, running);
                                }
                                Err(e) => publish(Some(session.status().clone()), Some(session.latest().clone()), Some(e), false),
                            }
                        }
                        Err(e) => publish(Some(session.status().clone()), Some(session.latest().clone()), Some(e), running),
                    }
                }
                Ok(RunControl::Step) => match session.execute(sim_runtime::system_session::Command::Step) {
                    Ok(_) => {
                        // The stepped point joins the trace like a tick sample.
                        let frame = session.latest();
                        last_sample = frame.time;
                        pending.push(sample(frame));
                        flush(&mut pending);
                        publish(Some(session.status().clone()), Some(session.latest().clone()), None, running);
                    }
                    Err(e) => {
                        step_error = Some(format!("step refused: {e}"));
                        publish(Some(session.status().clone()), Some(session.latest().clone()), step_error.clone(), running);
                    }
                },
                Ok(RunControl::Reset) => {
                    let result = session.execute(sim_runtime::system_session::Command::Reset);
                    // Paused at t = 0 (the runtime's reset); later Resume paces from here.
                    running = false;
                    pending.clear();
                    last_sample = f64::NEG_INFINITY;
                    wall = std::time::Instant::now();
                    sim_at_wall = session.status().time;
                    if let Ok(mut s) = shared.lock() {
                        s.history.clear();
                        s.reset_pending = false;
                    }
                    let error = result.err().map(|e| format!("reset failed: {e}"));
                    step_error = error.clone();
                    publish(Some(session.status().clone()), Some(session.latest().clone()), error, false);
                }
                Ok(RunControl::Observe(ids)) => {
                    if let Err(e) = session.subscribe(ids) {
                        publish(Some(session.status().clone()), Some(session.latest().clone()), Some(e), running);
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return,
            }
        }
        if running {
            // Never faster than real time; slower systems run as fast as they can.
            let ahead = session.status().time - sim_at_wall - wall.elapsed().as_secs_f64();
            if ahead > 0. {
                std::thread::sleep(std::time::Duration::from_secs_f64(ahead.min(0.02)));
            } else if let Err(e) = session.tick() {
                publish(Some(session.status().clone()), Some(session.latest().clone()), Some(system_builder::locate(&compiled.flat, e)), false);
                running = false;
            } else {
                let frame = session.latest();
                // About 1500 points across the kept window.
                if frame.time - last_sample >= HISTORY_SECONDS / 1500. {
                    last_sample = frame.time;
                    pending.push(sample(frame));
                }
            }
        } else {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        if last_publish.elapsed().as_millis() >= 33 {
            let speed = if wall.elapsed().as_secs_f64() > 0. { (session.status().time - sim_at_wall) / wall.elapsed().as_secs_f64() } else { 0. };
            publish(Some(session.status().clone()), Some(session.latest().clone()), step_error.clone(), running);
            if let Ok(mut s) = shared.lock() {
                s.speed = speed;
            }
            flush(&mut pending);
            last_publish = std::time::Instant::now();
        }
    }
}

pub(super) fn sync_run(time: Res<Time>, mut builder: ResMut<Builder>, mut scene: ResMut<SpatialScene>, mode: Res<State<ViewerMode>>) {
    builder.poll_study();
    builder.poll_replay();
    if (builder.study.job.is_some() || builder.replay.job.is_some()) && time.elapsed_secs_f64() - builder.live_refresh > 0.25 {
        builder.live_refresh = time.elapsed_secs_f64();
        builder.panel_dirty = true;
    }
    if builder.run.is_some() && time.elapsed_secs_f64() - builder.live_refresh > 0.25 {
        builder.live_refresh = time.elapsed_secs_f64();
        builder.panel_dirty = true;
    }
    // Under the lesson screen the scene shows the lesson's recorded runs; a
    // kept (paused) builder run shows again back in build mode.
    if *mode.get() == ViewerMode::Lessons {
        return;
    }
    let Some(run) = &builder.run else { return };
    let Ok(shared) = run.worker.shared().lock() else { return };
    if let Some(snapshot) = &shared.snapshot {
        if snapshot.source_description_id == scene.description.id {
            let changed = scene.live.snapshot.as_ref().is_none_or(|s| s.frame != snapshot.frame || s.status != snapshot.status || s.error != snapshot.error);
            if changed {
                scene.live.snapshot = Some(Arc::new(snapshot.clone()));
            }
        }
    }
}

/// Grab and push: with a run going, Alt-drag on a part changes the load
/// acting on it (a load torque on its shaft, a load force on its slide) in
/// the running model only; letting go restores it. The file is untouched.
pub(super) fn grab_push(buttons: Res<ButtonInput<MouseButton>>, keys: Res<ButtonInput<KeyCode>>, mut motion: MessageReader<bevy::input::mouse::MouseMotion>, pointed: Res<crate::view::PartHover>, scene: Res<SpatialScene>, mut builder: ResMut<Builder>) {
    let drag: f32 = motion.read().map(|e| e.delta.x).sum();
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    let running = builder.run.is_some();
    if builder.grab.is_none() {
        if !(running && alt && buttons.just_pressed(MouseButton::Left)) {
            return;
        }
        let Some(component) = pointed.0.clone() else { return };
        let Some((parameter, original)) = load_for(&scene, &component) else {
            builder.status = "Nothing to push on here: this part has no load torque or load force on its shaft or slide.".into();
            builder.panel_dirty = true;
            return;
        };
        let per_pixel = original.abs().max(0.01) / 150.;
        builder.grab = Some(Grab { parameter, original, per_pixel, dragged: 0., sent: std::time::Instant::now(), applied: original });
        return;
    }
    let mut grab = builder.grab.clone().unwrap();
    if !buttons.pressed(MouseButton::Left) {
        // Let go: the load returns to the file's value.
        builder.grab = None;
        if swap_with(&mut builder, &grab.parameter, grab.original) {
            builder.status = "Released: the load is back to its value in the file.".into();
        }
        builder.panel_dirty = true;
        return;
    }
    grab.dragged += drag;
    let value = grab.original + grab.dragged as f64 * grab.per_pixel;
    if (value - grab.applied).abs() > 1e-12 && grab.sent.elapsed() > std::time::Duration::from_millis(120) {
        grab.applied = value;
        grab.sent = std::time::Instant::now();
        builder.panel_dirty = true;
        if !swap_with(&mut builder, &grab.parameter, value) {
            // The run was stopped and the status says why; the gesture ends.
            builder.grab = None;
            return;
        }
        builder.status = format!("Pushing: {} = {} (was {}). Let go to release.", grab.parameter, crate::builder::ui::num(value), crate::builder::ui::num(grab.original));
    }
    builder.grab = Some(grab);
}

/// The load element acting on a part's shaft or slide, and its value now.
fn load_for(scene: &SpatialScene, component: &str) -> Option<(String, f64)> {
    let d = &scene.description;
    let nets: Vec<&sim_inspect::NetDescription> = d.nets.values().filter(|n| n.ports.iter().any(|p| d.ports.get(p).is_some_and(|q| q.component == component))).collect();
    for net in nets {
        for p in &net.ports {
            let c = &d.components.get(&d.ports.get(p)?.component)?;
            let parameter = match c.component_type.as_str() {
                "rotational.load_torque" => "torque",
                "translational.load_force" => "force",
                _ => continue,
            };
            let value = c.parameters.get(parameter).map(|v| v.value)?;
            return Some((format!("{}.{parameter}", c.id), value));
        }
    }
    None
}

/// Swap the running model for the document with one parameter changed,
/// through the run's fidelity (`Builder::hot_swap`); the file's document is
/// untouched. False only when the swap stopped the run (status explains).
pub(super) fn swap_with(builder: &mut Builder, parameter: &str, value: f64) -> bool {
    let Some(id) = builder.run.as_ref().map(|r| r.description_id.clone()) else { return true };
    let mut doc = builder.document.clone();
    let Ok(command) = sim_runtime::lesson::set_command(parameter, value) else { return true };
    if sim_system::apply(&mut doc, &builder.registry, &[command]).is_ok() {
        return builder.hot_swap(&doc, id);
    }
    true
}
