//! `RobotView`'s open/switch methods and its `robot_state` JSON.
use super::*;

impl RobotView {
    /// Starts the worker load; the window opens without waiting for it.
    /// The first load goes through the same worker check as a reload (`robot_source`).
    pub fn open(path: PathBuf) -> Self {
        let mut view = Self::new(path.clone(), None, None);
        view.source = Some(SourceWatch::open(path));
        view
    }
    /// Opens preset `id` from `presets` (its paths resolved against the
    /// workspace root, `crate::workspace`): refused now, naming the id, when it
    /// is unknown, not embedded or missing inputs, or when no root was found;
    /// its files are parsed on a worker thread.
    pub fn open_preset(presets: &Path, id: &str) -> Result<Self, String> {
        let root = crate::workspace::root().map_err(|e| format!("robot preset `{id}` resolves its inputs against the workspace root: {e}"))?.to_path_buf();
        let preset = crate::robot::preset::select(presets, &root, id)?;
        let (worker, dir) = (preset.clone(), root.clone());
        let path = root.join(preset.scene.as_deref().unwrap_or_default());
        // Parse and triangulate: CPU work.
        let load = crate::jobs::Job::spawn(crate::jobs::Pool::Compute, 0, format!("{}: the loader", path.display()), move |_| {
            if worker.is_recorded() {
                load_recorded(worker, &dir).map(|(l, r)| (l, Some(Opened::Recorded(r))))
            } else {
                load_preset(worker, &dir).map(|(l, r)| (l, Some(Opened::Preset(r))))
            }
        });
        let mut view = Self::new(path, Some(load), Some(preset));
        view.presets = Ok(presets.to_path_buf());
        Ok(view)
    }
    /// The preset list REST robot_presets/robot_preset read (None: the
    /// default `<root>/web/viewer/presets.json`).
    pub fn with_presets(mut self, presets: Option<PathBuf>) -> Self {
        if let Some(p) = presets {
            self.presets = Ok(p);
        }
        self
    }
    /// A switch to robot mode waits for this before entering it, so a file
    /// or preset that fails to load leaves the current mode: None while
    /// loading, else whether the first load succeeded. A success stays
    /// queued for `receive`, which installs it as at launch.
    pub(crate) fn opened(&mut self) -> Option<Result<(), String>> {
        if let Some(load) = &self.load {
            let generation = load.generation();
            let result = load.poll()?;
            return Some(match result {
                Ok(loaded) => {
                    self.load = Some(crate::jobs::Job::finished(generation, Ok(loaded)));
                    Ok(())
                }
                Err(e) => Err(e),
            });
        }
        match self.source.as_mut() {
            Some(source) => source.opened(),
            None => Some(Ok(())),
        }
    }
    /// What leaving robot mode would lose: a recording still being written
    /// (its result would never be shown) or a replay in progress.
    pub(crate) fn switch_blockers(&self) -> Vec<String> {
        let mut blockers = Vec::new();
        if let Some(run) = &self.run {
            if let Some(path) = run.save_pending() {
                blockers.push(format!("recording {} is being written: wait until the Save recording line (the Motion block for a preset, the Recording block for a controlled run) shows it saved", path.display()));
            }
            let replay = run.replay_state();
            if replay.phase == ReplayPhase::Replaying {
                blockers.push(format!("{} is replaying: wait, or press Cancel replay in the Replay block", replay.path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "a recording".into())));
            }
        }
        blockers
    }
    /// What robot mode reopens after a switch away: the preset or the file.
    pub(crate) fn document(&self) -> crate::app::switch::Document {
        match &self.preset {
            Some(p) => crate::app::switch::Document::Preset(p.id.clone()),
            None => crate::app::switch::Document::Path(self.path.clone()),
        }
    }
    pub(super) fn new(path: PathBuf, load: Option<crate::jobs::Job<(Loaded, Option<Opened>)>>, preset: Option<Preset>) -> Self {
        Self {
            preset,
            presets: crate::robot::preset::default_file(),
            root: crate::workspace::root().map(Path::to_path_buf),
            path,
            status: Status::Loading(std::time::Instant::now()),
            model: None,
            triangles: Vec::new(),
            notes: FileNotes::default(),
            cad_link: None,
            section: Section::Link,
            scroll: 0.0,
            scroll_max: 0.0,
            scroll_to: None,
            load,
            run: None,
            run_message: None,
            pose_dirty: false,
            ui_revision: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_micros() as u64,
            panels_ready: false,
            graphs_visible: false,
            source: None,
            notice: None,
            stress: StressOverlay::default(),
            mirror: None,
            planar: None,
        }
    }
    /// A planar (v2) file is displayed (`robot_planar`).
    pub(crate) fn is_planar(&self) -> bool {
        self.planar.is_some()
    }
    /// The selected link's name (a body's for a planar file).
    pub(super) fn link_name(&self, i: usize) -> Option<&str> {
        if let Some(p) = &self.planar {
            return p.loaded.model.bodies.get(i).map(|b| b.name.as_str());
        }
        self.model.as_ref()?.links.get(i).map(|l| l.name.as_str())
    }
    /// `robot_state` as REST and the snapshot answer it: [`Self::state_json`]
    /// with `cad_threads`, RoboCAD's comment threads on the CAD source as
    /// robot mode shows them (`threads::state_json`).
    pub(crate) fn state_with_threads(&self, link: Option<usize>, cad_threads: Value) -> Value {
        let mut out = self.state_json(link);
        out["cad_threads"] = cad_threads;
        out
    }
    /// `robot_state.bindings` and `robot_state.drive_input` (the device
    /// layer, `drive_input::DriveBindings` / `DriveInput`, which are
    /// resources rather than view state) added to a `robot_state` answer:
    /// set only while the run is controlled, null otherwise. Every caller
    /// that answers or publishes `robot_state` passes them.
    pub(crate) fn with_drive_input(&self, mut state: Value, bindings: Option<&crate::robot::drive_input::DriveBindings>, input: Option<&crate::robot::drive_input::DriveInput>) -> Value {
        let controlled = self.run.as_ref().is_some_and(|r| r.controlled().is_some());
        let bindings = bindings.filter(|_| controlled).map_or(Value::Null, |b| Value::Array(b.describe().into_iter().map(|(input, action)| json!({"input": input, "action": action})).collect()));
        let input = input.filter(|_| controlled).map_or(Value::Null, |i| {
            json!({"axes": {"forward": i.axes.forward, "lateral": i.axes.lateral, "yaw": i.axes.yaw}, "axes_unit": "normalized, -1..1",
                "source": i.source, "ignored_axes": i.ignored, "ignored_rule": "device axes the robot's drive profile does not support are zeroed before sending and listed here; REST robot_drive refuses them by name",
                "last_action": i.last_action, "last_error": i.last_error})
        });
        if let Some(o) = state.as_object_mut() {
            o.insert("bindings".into(), bindings);
            o.insert("drive_input".into(), input);
        }
        state
    }
    /// `robot_state`; `link` is the selected link (`picked::link`).
    pub fn state_json(&self, link: Option<usize>) -> Value {
        let (status, error, seconds) = match &self.status {
            Status::Loading(_) => ("loading", None, None),
            Status::Loaded { seconds } => ("loaded", None, Some(*seconds)),
            Status::Error(e) => ("error", Some(e.clone()), None),
        };
        if let Some(p) = &self.planar {
            return self.planar_state_json(p, status, error, seconds, link);
        }
        let links: Vec<Value> = self
            .model
            .iter()
            .flat_map(|m| m.links.iter().enumerate())
            .map(|(i, l)| json!({"index": i, "name": l.name, "has_mesh": self.triangles.get(i).is_some_and(|t| *t > 0), "triangles": self.triangles.get(i).copied().unwrap_or(0)}))
            .collect();
        let selected = link.and_then(|i| {
            let m = self.model.as_ref()?;
            let l = m.links.get(i)?;
            let material = m.materials.get(&l.material);
            let joints: Vec<usize> = touching(m, &l.name).map(|(j, _)| j).collect();
            Some(json!({"index": i, "name": l.name, "mass": l.mass, "com": l.com, "inertia": l.inertia, "material": l.material,
                "material_in_file": material.is_some(), "density": material.map(|x| x.density), "material_entry": material,
                "ground": l.ground, "members": l.members, "triangles": self.triangles.get(i).copied().unwrap_or(0), "joints": joints,
                "provenance": Value::Null, "file_notes": self.notes.links.get(i)}))
        });
        let m = self.model.as_ref();
        let joints: Vec<Value> = m.iter().flat_map(|m| m.joints.iter().enumerate()).map(|(i, j)| joint_json(i, j)).collect();
        let motors: Vec<Value> = m.iter().flat_map(|m| m.motors.iter().enumerate()).map(|(i, x)| json!({"motor": x, "provenance": Value::Null, "file_notes": {"notes": self.notes.motors.get(i)}})).collect();
        let profiles = m.and_then(|m| m.actuator_profiles.as_ref()).map(|p| {
            let hashes: serde_json::Map<String, Value> = p.families.iter().map(|(k, f)| (k.clone(), json!(f.content_hash()))).collect();
            json!({"content_hashes": hashes, "profiles": p, "provenance": "per parameter, typed in the file (measured | derived | estimated)"})
        });
        let names: Vec<String> = m.iter().flat_map(|m| m.links.iter().map(|l| l.name.clone())).collect();
        let run = self.run.as_ref().map(|r| r.state_json(&names));
        // The preset block: the parsed run once loaded, else the declared entry while loading.
        let recorded = self.run.as_ref().and_then(|r| r.recorded());
        let preset = match (self.run.as_ref().and_then(|r| r.preset()), &self.preset) {
            _ if recorded.is_some() => recorded.map(|r| r.state_json(names.len())),
            (Some(p), _) => Some(p.state_json(self.run.as_ref().and_then(|r| r.frame()).and_then(|f| f.completed_steps))),
            (None, Some(p)) => Some(json!({"id": p.id, "label": p.label, "mode": p.mode, "scene": p.scene, "config": p.config, "task": p.task,
                "readiness": p.readiness(), "evidence": p.evidence(), "loaded": false})),
            (None, None) => None,
        };
        let stepped = self.run.as_ref().and_then(|r| r.frame()).is_some();
        let previewing = self.run.as_ref().and_then(|r| r.gait_preview()).and_then(|g| g.poses()).is_some();
        let cad = self.cad_link.as_ref().map(|c| json!({"link": c, "rule": cad_link::RESOLUTION_RULE}));
        let jog = self.run.as_ref().filter(|r| r.preset().is_none() && r.recorded().is_none()).map(|r| {
            let m = r.model();
            let joints: Vec<Value> = m.joints.iter().filter(|j| j.kind != "fixed" && !j.is_loop()).map(|j| r.jog_json(&j.name)).collect();
            let selected: Vec<String> = jog_joints(self, link).into_iter().map(|(j, _)| j).collect();
            json!({"label": JOG_LABEL, "semantics": JOG_SEMANTICS, "control_mode": m.control.mode, "trajectory_keyframes": m.control.trajectory.len(),
                "step_rad": JOG_STEP_RAD, "step_m": JOG_STEP_M, "selected_link_joints": selected, "joints": joints, "last_apply_error": r.jog_error()})
        });
        let format = m.map(|m| json!({"version": m.version, "name": physical_format_name(m.version), "model": "sim_domain_robot::PhysicalModel (run by sim_runtime::physical::PhysicalRobot for --robot FILE)"}));
        let mut out = json!({"file": self.path, "workspace": crate::workspace::json(), "status": status, "error": error, "load_seconds": seconds,
            "link_count": m.map(|m| m.links.len()), "links": links, "selected": selected,
            "joints": joints, "motors": motors, "transmissions": m.map(|m| &m.transmissions), "battery": m.and_then(|m| m.battery.as_ref()),
            "actuator_profiles": profiles, "uncertainty": m.map(|_| &self.notes.uncertainty), "uncertainty_parsed": m.map(|m| &m.uncertainty), "identification": m.map(|m| &m.identification),
            "materials": m.map(|m| &m.materials), "source": m.map(|m| &m.source), "cad_link": cad,
            "source_file": self.source.as_ref().map_or_else(|| json!({"watching": false, "reason": "a preset is not watched (--robot FILE only)"}), |s| s.json(true)), "notice": self.notice,
            "provenance_rule": PROVENANCE_RULE, "unlabelled_values": UNLABELLED, "numbers": "JSON numbers as parsed by PhysicalModel (f64, shortest round-trip); SI units; a null in place of a number is non-finite",
            "section": self.section, "inspector_scroll": {"offset_px": self.scroll, "max_px": self.scroll_max},
            "pose": if recorded.is_some() { RECORDED_POSE } else if previewing { GAIT_POSE } else if stepped { SIMULATED_POSE } else { POSE }, "read_only": true, "stepped": stepped, "run": run, "jog": jog, "preset": preset, "motion": self.run.as_ref().map(|r| r.motion_json()), "recording": self.run.as_ref().filter(|r| r.preset().is_some() || r.controlled().is_some()).map(|r| r.recording_json()),
            "recordings": self.run.as_ref().map(|r| r.recordings_json()), "replay": self.run.as_ref().map(|r| r.replay_json()), "gait_preview": self.run.as_ref().map(|r| r.gait_json()),
            "ui_revision": self.ui_revision, "controls_ready": self.panels_ready});
        // Kept out of the literal above: serde_json's json! hits the default
        // recursion limit with every key in one macro call.
        out["format"] = json!(format);
        out["graphs"] = self.run.as_ref().map_or_else(|| json!({"visible": self.graphs_visible, "charts": []}), |r| r.graphs_json(link, self.graphs_visible));
        // Run-thread overlays (robot_overlay); null until loaded.
        out["overlays"] = self.run.as_ref().map_or(Value::Null, RunController::overlays_json);
        // A controlled run's drive state (RunController::drive_json: controller, profile,
        // limits with units, geometry with provenance, twists, deadman), or {bound: false,
        // binding_error} when the binding beside the model failed to load; null for any other run.
        // The device layer's `bindings` and `drive_input` are resources: `with_drive_input` adds them.
        out["drive"] = self.run.as_ref().filter(|r| r.controlled().is_some() || r.binding_error().is_some()).map_or(Value::Null, RunController::drive_json);
        // The recorded timeline (robot_recorded); absent unless a recorded preset is loaded.
        if let Some(r) = self.run.as_ref().and_then(RunController::recorded_json) {
            out["recorded"] = r;
        }
        if let Some(o) = out["overlays"].as_object_mut() {
            let stress = match &self.source {
                Some(_) => self.stress.json(self.model.as_ref()),
                None => json!({"available": false, "reason": STRESS_PRESET}),
            };
            o.insert("stress".into(), stress);
        }
        out
    }
    /// `robot_state` for a planar (v2) file: the shared fields, `format`
    /// (version, name, fidelity, build warnings), the planar summary and run,
    /// and every v3-only block null with `unavailable` naming why.
    fn planar_state_json(&self, p: &PlanarView, status: &str, error: Option<String>, seconds: Option<f64>, link: Option<usize>) -> Value {
        let planar = p.json(link);
        let selected = planar["selected_body"].clone();
        let f = p.run.frame().filter(|f| f.built);
        let jog: Vec<Value> = f.map(|f| f.joint_names.iter().enumerate().map(|(i, n)| json!({"index": i, "name": n, "angle_rad": f.joint_angles.get(i), "target_rad": f.targets.get(i)})).collect::<Vec<Value>>()).unwrap_or_default();
        let refusals: serde_json::Map<String, Value> = planar::UNAVAILABLE.iter().map(|(k, why)| (k.to_string(), json!(why))).collect();
        let mut out = json!({"file": self.path, "workspace": crate::workspace::json(), "status": status, "error": error, "load_seconds": seconds,
            "format": p.format_json(), "planar": planar, "links": [], "selected": selected,
            "source_file": self.source.as_ref().map(|s| s.json(true)), "notice": self.notice,
            "provenance_rule": "a planar v2 summary carries no per-value provenance: masses, centres, planar inertias and outlines are RoboCAD's geometry-derived export values, shown as stored",
            "section": self.section, "inspector_scroll": {"offset_px": self.scroll, "max_px": self.scroll_max},
            "pose": PLANAR_POSE, "read_only": true, "stepped": f.is_some_and(|f| f.steps > 0), "run": p.run.json(),
            "jog": {"label": "planar v2 joint target", "rule": planar::JOG_RULE, "selected_joint": p.selected_joint_name(), "joints": jog},
            "graphs": {"visible": false, "available": false, "reason": planar::GRAPHS}, "overlays": p.overlays_json(), "unavailable": refusals,
            "ui_revision": self.ui_revision, "controls_ready": self.panels_ready});
        // The v3-only blocks, null (kept out of the literal: json! hits the default recursion limit with every key in one call).
        for key in ["link_count", "joints", "motors", "transmissions", "battery", "actuator_profiles", "uncertainty", "uncertainty_parsed", "identification", "materials", "source", "cad_link",
            "preset", "motion", "recording", "recordings", "replay", "gait_preview", "drive"] {
            out[key] = Value::Null;
        }
        out
    }
}

/// Joints whose parent or child is the named link.
pub(super) fn touching<'a>(m: &'a PhysicalModel, link: &'a str) -> impl Iterator<Item = (usize, &'a sim_domain_robot::model::Joint)> + 'a {
    m.joints.iter().enumerate().filter(move |(_, j)| j.child == link || j.parent.as_deref() == Some(link))
}
fn joint_json(i: usize, j: &sim_domain_robot::model::Joint) -> Value {
    json!({"index": i, "name": j.name, "id": j.id, "type": j.kind, "parent": j.parent, "child": j.child, "origin": j.origin, "axis": j.axis,
        "limits": j.limits, "home": j.home, "motor": j.motor, "physics": j.physics, "fastened": j.fastened,
        "typed_provenance": {"drive_backlash": j.physics.drive_backlash.as_ref().map(|b| b.provenance)},
        "provenance": Value::Null, "file_notes": {"physics.source": j.physics.source}})
}
