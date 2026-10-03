//! Browser worker bindings; physics and controller behavior live in sim-runtime.
use sim_runtime::embedded::{CaptureMode, Config, EmbeddedRecording, EmbeddedSession};
use sim_runtime::session::{Recording, Scene, Session};
use wasm_bindgen::prelude::*;

fn error(e: impl ToString) -> JsValue {
    JsValue::from_str(&e.to_string())
}

/// Pure preparation for the ordinary shared environment; call from a worker.
#[wasm_bindgen]
pub fn materialize_motion(scene_json: &str, actions_json: &str, recipe_json: &str, values_json: &str) -> Result<String, JsValue> {
    let recipe: sim_runtime::motion_parameters::MotionParameterization = serde_json::from_str(recipe_json).map_err(error)?;
    let variant = recipe.materialize(&serde_json::from_str(scene_json).map_err(error)?,
        &serde_json::from_str::<Vec<Vec<f64>>>(actions_json).map_err(error)?,
        &serde_json::from_str(values_json).map_err(error)?).map_err(error)?;
    serde_json::to_string(&serde_json::json!({"variant":variant,"metadata":recipe.metadata()})).map_err(error)
}

/// Validate and bind an immutable motion experiment to current library sources.
#[wasm_bindgen]
pub fn bind_motion_experiment(spec_json: &str) -> Result<String, JsValue> {
    let experiment=sim_runtime::experiment::Experiment::bind(serde_json::from_str(spec_json).map_err(error)?).map_err(error)?;
    serde_json::to_string(&experiment).map_err(error)
}

/// Host-driven motion trial, including incremental checkpoint reconstruction.
#[wasm_bindgen]
pub struct MotionEvaluation { evaluation: sim_runtime::experiment::Evaluation }
#[wasm_bindgen]
impl MotionEvaluation {
    #[wasm_bindgen(constructor)]
    pub fn new(experiment_json: &str, proposal_json: &str) -> Result<Self, JsValue> {
        let experiment:sim_runtime::experiment::Experiment=serde_json::from_str(experiment_json).map_err(error)?;
        Ok(Self {evaluation:experiment.start(serde_json::from_str(proposal_json).map_err(error)?).map_err(error)?})
    }
    pub fn resume(experiment_json: &str, checkpoint_json: &str) -> Result<Self, JsValue> {
        let experiment:sim_runtime::experiment::Experiment=serde_json::from_str(experiment_json).map_err(error)?;
        Ok(Self {evaluation:experiment.resume(serde_json::from_str(checkpoint_json).map_err(error)?).map_err(error)?})
    }
    pub fn advance(&mut self, maximum_actions: u32) -> Result<String, JsValue> {
        let status=self.evaluation.advance(maximum_actions as usize).map_err(error)?;
        serde_json::to_string(&serde_json::json!({"status":status})).map_err(error)
    }
    pub fn checkpoint(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.evaluation.checkpoint().map_err(error)?).map_err(error)
    }
    pub fn frame(&self) -> Result<String, JsValue> { serde_json::to_string(&self.evaluation.frame().map_err(error)?).map_err(error) }
    pub fn metadata(&self) -> Result<String, JsValue> { serde_json::to_string(&self.evaluation.metadata()).map_err(error) }
}

/// Inspect original CAD JSON before model parsing supplies legacy defaults.
/// Call from a worker; no simulation session or source geometry is modified.
#[wasm_bindgen]
pub fn inspect_robot_contract(document_json: &str) -> Result<String, JsValue> {
    let document = serde_json::from_str(document_json).map_err(error)?;
    let inspection = sim_runtime::robot_contract::inspect(document).map_err(error)?;
    serde_json::to_string(&inspection).map_err(error)
}

/// Read-only comparison using the same typed fidelity API as native experiments.
#[wasm_bindgen]
pub fn compare_environment_fidelity(reference_json: &str, candidate_json: &str, plan_json: &str) -> Result<String, JsValue> {
    let report = sim_runtime::fidelity::compare(
        &serde_json::from_str(reference_json).map_err(error)?,
        &serde_json::from_str(candidate_json).map_err(error)?,
        &serde_json::from_str(plan_json).map_err(error)?,
    ).map_err(error)?;
    serde_json::to_string(&report).map_err(error)
}

/// Teacher training transitions use exactly the native environment adapter.
/// Invoke from a worker: one action interval can take longer than a display frame.
#[wasm_bindgen]
pub struct EnvironmentSimulation {
    environment: sim_runtime::environment::EmbeddedEnvironment,
    replay_actions: std::collections::VecDeque<Vec<f64>>,
}

#[wasm_bindgen]
impl EnvironmentSimulation {
    #[wasm_bindgen(constructor)]
    pub fn new(scene_json: &str, config_json: &str, task_json: &str, seed: u32) -> Result<Self, JsValue> {
        Ok(Self { environment: sim_runtime::environment::EmbeddedEnvironment::new(
            serde_json::from_str(scene_json).map_err(error)?,
            serde_json::from_str(config_json).map_err(error)?,
            serde_json::from_str(task_json).map_err(error)?, seed as u64,
        ).map_err(error)?, replay_actions: Default::default() })
    }
    pub fn step(&mut self, action: &[f64]) -> Result<String, JsValue> {
        if !self.replay_actions.is_empty() { return Err(error("finish replay before changing actions")); }
        self.environment.step(action).map_err(error)?;
        self.frame()
    }
    pub fn reset(&mut self, seed: u32) -> Result<String, JsValue> {
        self.environment.reset(seed as u64).map_err(error)?;
        self.replay_actions.clear();
        self.frame()
    }
    pub fn frame(&self) -> Result<String, JsValue> {
        let mut frame=self.environment.frame().map_err(error)?;
        let t=self.environment.transition();
        frame["learning"]=serde_json::json!(t);
        frame["done"]=serde_json::json!(t.terminated || t.truncated || self.environment.error().is_some());
        frame["error"]=serde_json::json!(self.environment.error());
        serde_json::to_string(&frame).map_err(error)
    }
    pub fn contract(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.environment.contract()).map_err(error)
    }
    /// Same typed, read-only forecast query as the native environment.
    pub fn predict_controller_trajectory(&self, model_json:&str, previous_json:&str, actions_json:&str)->Result<String,JsValue>{
        let result=self.environment.predict_controller_trajectory(
            &serde_json::from_str(model_json).map_err(error)?,
            &serde_json::from_str(previous_json).map_err(error)?,
            &serde_json::from_str::<Vec<Vec<f64>>>(actions_json).map_err(error)?).map_err(error)?;
        serde_json::to_string(&result).map_err(error)
    }
    pub fn recording(&self) -> Result<String, JsValue> {
        if !self.replay_actions.is_empty() { return Err(error("finish replay before recording")); }
        serde_json::to_string(&self.environment.episode_recording()).map_err(error)
    }
    pub fn inputs(&self) -> Result<String,JsValue> {
        serde_json::to_string(self.environment.inputs()).map_err(error)
    }
    pub fn metadata(&self) -> Result<String,JsValue> {
        serde_json::to_string(&self.environment.metadata()).map_err(error)
    }
    pub fn prepare_replay(&mut self, json:&str) -> Result<u32,JsValue> {
        let (next,actions)=self.environment.prepare_replay(serde_json::from_str(json).map_err(error)?).map_err(error)?;
        let count=actions.len();self.environment=next;self.replay_actions=actions.into();Ok(count as u32)
    }
    pub fn advance_replay(&mut self) -> Result<String,JsValue> {
        let action=self.replay_actions.front().ok_or_else(||error("no pending replay action"))?.clone();
        self.environment.step(&action).map_err(error)?;
        self.replay_actions.pop_front();self.frame()
    }
}

/// The same incremental mechanism/motor runner used by integrate_embedding.
/// Hosts choose bounded work chunks, never a rendering-dependent physics dt.
#[wasm_bindgen]
pub struct EmbeddedSimulation {
    session: EmbeddedSession,
}

#[wasm_bindgen]
impl EmbeddedSimulation {
    #[wasm_bindgen(constructor)]
    pub fn new(scene_json: &str, config_json: &str, seed: u32) -> Result<Self, JsValue> {
        let scene: Scene = serde_json::from_str(scene_json).map_err(error)?;
        let config: Config = serde_json::from_str(config_json).map_err(error)?;
        if config.profile_solver {
            return Err(error(
                "process-global profiling is unavailable in the browser session",
            ));
        }
        Ok(Self {
            session: EmbeddedSession::new(scene, config, seed as u64, CaptureMode::Latest)
                .map_err(error)?,
        })
    }
    pub fn advance(&mut self, steps: u32) -> Result<String, JsValue> {
        if steps == 0 || steps > 1000 {
            return Err(error("browser work chunk must be 1..1000 nominal steps"));
        }
        // A solver failure is a visible terminal frame, preserving the last
        // committed physical state and failure reason for the UI.
        let _ = self.session.advance(steps as usize);
        self.frame()
    }
    pub fn frame(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.session.interactive_frame().map_err(error)?).map_err(error)
    }
    pub fn metadata(&self) -> Result<String, JsValue> {
        let c = self.session.config();
        serde_json::to_string(&serde_json::json!({"coordinate_names":self.session.coordinate_names(),"joint_indices":self.session.joint_indices(),"step_s":c.step_s,"steps":c.steps,"report_every":c.report_every,"policy_contract":self.session.policy_metadata()})).map_err(error)
    }
    pub fn inputs(&self) -> Result<String, JsValue> {
        serde_json::to_string(self.session.inputs()).map_err(error)
    }
    pub fn set_inputs(&mut self, values: &[f64]) -> Result<(), JsValue> {
        self.session.set_inputs(values).map_err(error)
    }
    pub fn recording(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.session.recording()).map_err(error)
    }
    /// Return the required replay step count. Transport advances it in bounded
    /// chunks so progress and cancellation remain available between calls.
    pub fn prepare_replay(&mut self, json: &str) -> Result<u32, JsValue> {
        let recording: EmbeddedRecording = serde_json::from_str(json).map_err(error)?;
        if serde_json::to_value(&recording.scene).map_err(error)?
            != serde_json::to_value(self.session.scene()).map_err(error)?
            || serde_json::to_value(&recording.config).map_err(error)?
                != serde_json::to_value(self.session.config()).map_err(error)?
        {
            return Err(error(
                "replay must match the loaded scene and controller recipe; load another preset to change them",
            ));
        }
        if recording.config.profile_solver {
            return Err(error(
                "process-global profiling is unavailable in the browser session",
            ));
        }
        let (session, steps) =
            EmbeddedSession::prepare_replay(recording, CaptureMode::Latest).map_err(error)?;
        self.session = session;
        Ok(steps as u32)
    }
}

/// The committed device bindings (`sim.drive-bindings/1`) with their
/// description and the W3C Standard Gamepad table (`drive_bindings::json`).
#[wasm_bindgen]
pub fn default_drive_bindings() -> Result<String, JsValue> {
    serde_json::to_string(&sim_runtime::drive_bindings::json(None)).map_err(error)
}

/// A stored bindings file checked by the shared parser; the answer is the
/// same shape as `default_drive_bindings` (`stored: true`). A refusal names
/// its field (`drive_bindings.keyboard.axes[2].key: …`).
#[wasm_bindgen]
pub fn validate_drive_bindings(json: &str) -> Result<String, JsValue> {
    let file = bindings_file(json)?;
    serde_json::to_string(&sim_runtime::drive_bindings::json(Some(&file))).map_err(error)
}

fn bindings_file(json: &str) -> Result<sim_runtime::drive_bindings::BindingsFile, JsValue> {
    let value: serde_json::Value = serde_json::from_str(json).map_err(|e| error(format!("drive_bindings: {e}")))?;
    sim_runtime::drive_bindings::BindingsFile::from_value(&value).map_err(error)
}

/// The browser's held keys and pads through the bindings
/// (`drive_bindings::browser_axes`): `devices_json` is `BrowserDevices`
/// (`{"keys": ["KeyW"], "gamepads": [{"mapping", "id", "axes", "buttons", "pressed"}]}`),
/// `supported_json` the drive's supported axes (`[true, false, true]`).
/// Answers `DeviceAxes` (normalized axes; scale them with `drive_request`).
#[wasm_bindgen]
pub fn drive_device_axes(bindings_json: &str, devices_json: &str, supported_json: &str) -> Result<String, JsValue> {
    let resolved = bindings_file(bindings_json)?.resolve().map_err(error)?;
    let devices: sim_runtime::drive_bindings::BrowserDevices = serde_json::from_str(devices_json).map_err(|e| error(format!("devices: {e}")))?;
    let supported: [bool; 3] = serde_json::from_str(supported_json).map_err(|e| error(format!("supported axes: {e}")))?;
    serde_json::to_string(&sim_runtime::drive_bindings::browser_axes(&resolved, &devices, supported)).map_err(error)
}

/// The files a controller binding's embedded program needs, relative to the
/// binding, as a JSON array (`embedded_drive::files_to_read`): fetch each and
/// pass them to `build_drive_scene`.
#[wasm_bindgen]
pub fn drive_binding_files(binding_path: &str, binding_json: &str) -> Result<String, JsValue> {
    let path = std::path::Path::new(binding_path);
    let binding = sim_runtime::controller_binding::ControllerBinding::from_json(binding_json, path).map_err(error)?;
    serde_json::to_string(&sim_runtime::embedded_drive::files_to_read(&binding, path).map_err(error)?).map_err(error)
}

/// The embedded drive (`embedded_drive::build`, the one scene builder) from
/// the model, its binding and `files_json` (`{"<path as the binding writes
/// it>": "<text>"}`), for `DriveSimulation`'s constructor. Call from a worker:
/// it parses the model and resolves its geometry.
#[wasm_bindgen]
pub fn build_drive_scene(model_path: &str, model_json: &str, binding_path: &str, binding_json: &str, files_json: &str) -> Result<String, JsValue> {
    let files: std::collections::BTreeMap<String, String> = serde_json::from_str(files_json).map_err(|e| error(format!("files: {e}")))?;
    let drive = sim_runtime::embedded_drive::build(
        model_json,
        model_path,
        std::path::Path::new(binding_path),
        binding_json,
        &files,
        sim_runtime::controller_binding::DRIVE_DURATION_S,
    )
    .map_err(error)?;
    serde_json::to_string(&drive).map_err(error)
}

/// A drive request interpreted against the drive's profile, without a
/// session (`embedded_drive::interpret_json`): `{"twist": {forward_m_s,
/// lateral_m_s, yaw_rad_s}, "halt": bool}`. `request_json` is
/// `{"axes": {...}}`, `{"action": {"name": ...}}` or `"stop"`.
#[wasm_bindgen]
pub fn drive_request(drive_json: &str, request_json: &str) -> Result<String, JsValue> {
    serde_json::to_string(&sim_runtime::embedded_drive::interpret_json(drive_json, request_json).map_err(error)?).map_err(error)
}

/// A bound robot driven in the browser: the binding's embedded Rhai adapter
/// in the shared embedded session, with the shared limiter and deadman on
/// simulation time (`embedded_drive::DriveSession`). The page sends requests
/// only. Run it in a worker and advance it in bounded chunks of control periods.
#[wasm_bindgen]
pub struct DriveSimulation {
    session: sim_runtime::embedded_drive::DriveSession,
}

#[wasm_bindgen]
impl DriveSimulation {
    /// `drive_json` is `build_drive_scene`'s answer.
    #[wasm_bindgen(constructor)]
    pub fn new(drive_json: &str, seed: u32) -> Result<DriveSimulation, JsValue> {
        let drive: sim_runtime::embedded_drive::EmbeddedDrive = serde_json::from_str(drive_json).map_err(|e| error(format!("embedded drive: {e}")))?;
        if drive.config.profile_solver {
            return Err(error("process-global profiling is unavailable in the browser session"));
        }
        Ok(Self { session: sim_runtime::embedded_drive::DriveSession::new(drive, seed as u64).map_err(error)? })
    }
    /// A fresh request (`DriveRequest`'s JSON) at the current simulation
    /// time; answers the drive status. Refused while replaying.
    pub fn request(&mut self, request_json: &str) -> Result<String, JsValue> {
        let request: sim_runtime::drive_host::DriveRequest = serde_json::from_str(request_json).map_err(|e| error(format!("drive request: {e}")))?;
        let status = self.session.request(&request).map_err(error)?;
        serde_json::to_string(&status.json()).map_err(error)
    }
    /// The page paused (`DriveSession::pause`): a request live at Pause does
    /// not drive after resume until a fresh one arrives. A no-op while
    /// replaying. Answers the drive status.
    pub fn pause(&mut self) -> Result<String, JsValue> {
        serde_json::to_string(&self.session.pause().json()).map_err(error)
    }
    /// Up to `periods` (1..1000) control periods, then the frame. A solver
    /// failure is a visible terminal frame (its `error`), as in
    /// `EmbeddedSimulation::advance`, and so is the horizon (`done: true`,
    /// `remaining_steps() == 0`); any other refusal is an error.
    pub fn advance(&mut self, periods: u32) -> Result<String, JsValue> {
        if periods == 0 || periods > 1000 {
            return Err(error("browser work chunk must be 1..1000 control periods"));
        }
        if let Err(e) = self.session.advance(periods as usize) {
            let session = self.session.session();
            if session.error().is_none() && session.remaining_steps() > 0 {
                return Err(error(e));
            }
        }
        self.frame()
    }
    pub fn frame(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.session.frame().map_err(error)?).map_err(error)
    }
    pub fn metadata(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.session.metadata()).map_err(error)
    }
    pub fn recording(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.session.recording()).map_err(error)
    }
    /// Check a recording against the loaded drive (identity, scene, config)
    /// and start its replay; answers the control periods to advance.
    pub fn prepare_replay(&mut self, json: &str) -> Result<u32, JsValue> {
        let recording: EmbeddedRecording = serde_json::from_str(json).map_err(|e| error(format!("recording: {e}")))?;
        if recording.config.profile_solver {
            return Err(error("process-global profiling is unavailable in the browser session"));
        }
        let periods = self.session.prepare_replay(recording).map_err(error)?;
        u32::try_from(periods).map_err(|_| error(format!("replay of {periods} control periods is too long")))
    }
}

#[wasm_bindgen]
pub struct Simulation {
    session: Session,
}

#[wasm_bindgen]
impl Simulation {
    #[wasm_bindgen(constructor)]
    pub fn new(scene_json: &str, seed: u32) -> Result<Simulation, JsValue> {
        let scene: Scene = serde_json::from_str(scene_json).map_err(error)?;
        Ok(Self {
            session: Session::new(scene, seed as u64).map_err(error)?,
        })
    }
    pub fn step(&mut self, action: &[f64]) -> Result<String, JsValue> {
        serde_json::to_string(&self.session.step(action).map_err(error)?).map_err(error)
    }
    pub fn frame(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.session.frame()).map_err(error)
    }
    pub fn inputs(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.session.inputs).map_err(error)
    }
    /// Diagnostic only: capture the same implicit stages as native sim-validate.
    pub fn set_attempt_audit_limit(&mut self, limit: u32) -> Result<(), JsValue> {
        self.session
            .set_attempt_audit_limit(limit as usize)
            .map_err(error)
    }
    pub fn implicit_attempt_report(&self) -> Result<String, JsValue> {
        let report =
            sim_runtime::validation::implicit_attempt_report(&self.session).map_err(error)?;
        serde_json::to_string(&report).map_err(error)
    }
    /// Method-consistent linear contact impulses on a fully audited window.
    pub fn contact_impulse_report(&self, start: f64, end: f64) -> Result<String, JsValue> {
        let report =
            sim_runtime::contact_audit::committed_contact_impulses(&self.session, start, end)
                .map_err(error)?;
        serde_json::to_string(&report).map_err(error)
    }
    pub fn reset(&mut self, seed: u32) -> Result<String, JsValue> {
        serde_json::to_string(&self.session.reset(seed as u64).map_err(error)?).map_err(error)
    }
    pub fn recording(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.session.recording()).map_err(error)
    }
    pub fn replay(&mut self, recording_json: &str) -> Result<String, JsValue> {
        let recording: Recording = serde_json::from_str(recording_json).map_err(error)?;
        self.session = Session::replay(recording).map_err(error)?;
        self.frame()
    }
}

/// Shared gait playback (the same sampler the hardware host uses).
#[wasm_bindgen]
pub struct GaitPlayer {
    gait: sim_runtime::gait_playback::Gait,
    governed: sim_runtime::gait_playback::GovernedGait,
}

#[wasm_bindgen]
impl GaitPlayer {
    #[wasm_bindgen(constructor)]
    pub fn new(compiled_json: &str, name: &str) -> Result<GaitPlayer, JsValue> {
        let compiled: serde_json::Value = serde_json::from_str(compiled_json).map_err(error)?;
        let gait = sim_runtime::gait_playback::Gait::from_compiled(&compiled, name).map_err(error)?;
        Ok(Self { governed: sim_runtime::gait_playback::GovernedGait::new(gait.clone()), gait })
    }
    /// The gait as the simulation commands it: through its reference
    /// governor, advancing `dt` s of wall time at gait time `t`.
    pub fn governed(&mut self, t: f64, dt: f64, rate_scale: f64) -> Result<Vec<f64>, JsValue> {
        Ok(self.governed.step(t, dt, rate_scale).map_err(error)?.into_iter().map(|(q, _)| q).collect())
    }
    pub fn reset(&mut self) {
        self.governed = sim_runtime::gait_playback::GovernedGait::new(self.gait.clone());
    }
    pub fn info(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.gait.info).map_err(error)
    }
    /// Joint angles (rad) at gait time `t`, in `info().joints` order.
    pub fn sample(&self, t: f64) -> Result<Vec<f64>, JsValue> {
        self.gait.sample(t).map_err(error)
    }
}

/// Suspended display pose from measured motor coordinates (geometry only).
#[wasm_bindgen]
pub struct KinematicMirror {
    mirror: sim_runtime::kinematic_mirror::KinematicMirror,
}

#[wasm_bindgen]
impl KinematicMirror {
    #[wasm_bindgen(constructor)]
    pub fn new(scene_json: &str, lift_m: f64) -> Result<KinematicMirror, JsValue> {
        let scene: Scene = serde_json::from_str(scene_json).map_err(error)?;
        Ok(Self { mirror: sim_runtime::kinematic_mirror::KinematicMirror::new(scene, lift_m).map_err(error)? })
    }
    pub fn coordinates(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.mirror.coordinates()).map_err(error)
    }
    pub fn pose(&mut self, coordinates: &[f64]) -> Result<String, JsValue> {
        serde_json::to_string(&self.mirror.pose(coordinates).map_err(error)?).map_err(error)
    }
}

/// A system file (`sim.system/1`) running in the browser on the shared
/// runtime, in its detailed or realtime profile. Authored parts arrive as
/// sources (`{"coreless_motor.part": "…"}`) and compile here exactly as they
/// do natively. Hosts advance it in bounded chunks of simulated time.
#[wasm_bindgen]
pub struct SystemRun {
    session: sim_runtime::system_session::SystemSession,
    ids: Vec<String>,
    labels: Vec<String>,
    interval: f64,
}

#[wasm_bindgen]
impl SystemRun {
    #[wasm_bindgen(constructor)]
    pub fn new(document_json: &str, parts_json: &str, profile: &str, observe_json: &str) -> Result<SystemRun, JsValue> {
        let mut registry = sim_runtime::registry();
        let parts: std::collections::BTreeMap<String, String> = serde_json::from_str(parts_json).map_err(error)?;
        for (file, source) in &parts {
            let def = sim_runtime::parts::parse(file, source).map_err(error)?;
            sim_runtime::parts::register(&mut registry, def).map_err(error)?;
        }
        let mut document: sim_runtime::system::SystemDocument = serde_json::from_str(document_json).map_err(error)?;
        match profile {
            "detailed" => {}
            "realtime" => document = sim_runtime::system::profile::realtime(&document, &registry).map_err(error)?,
            other => return Err(error(format!("unknown profile `{other}` (detailed or realtime)"))),
        }
        let config = sim_runtime::system_builder::config_for(&document);
        let compiled = sim_runtime::system_builder::compile(&document, &registry, config.clone()).map_err(error)?;
        // Hosted instances (linked files) need the native drive host; never run a partial model.
        sim_runtime::system_builder::check_hosted(&compiled.flat).map_err(error)?;
        let source = sim_runtime::system_session::ModelSource {
            model: compiled.flat.model.clone(),
            registry: registry.clone(),
            identities: compiled.flat.identities.clone(),
            source_hash: compiled.flat.source_hash.clone(),
            revision: document.revision.max(1),
        };
        let interval = config.interval;
        let mut session = sim_runtime::system_session::SystemSession::new("browser".into(), config, move |c| source.build(c)).map_err(error)?;
        let observe: Vec<String> = serde_json::from_str(observe_json).map_err(error)?;
        let description = session.description().clone();
        let mut ids = Vec::new();
        for key in &observe {
            let id = description.observables.keys().find(|id| sim_runtime::system_builder::observable_key(&description, id) == *key).ok_or_else(|| error(format!("no observable `{key}`")))?;
            ids.push(id.clone());
        }
        session.subscribe(ids.clone()).map_err(error)?;
        session.execute(sim_runtime::system_session::Command::Start).map_err(error)?;
        Ok(SystemRun { session, ids, labels: observe, interval })
    }
    /// Advance by `seconds` of simulated time (at most 10 s per call).
    pub fn advance(&mut self, seconds: f64) -> Result<f64, JsValue> {
        if !(seconds > 0.0 && seconds <= 10.0) {
            return Err(error("advance 0 < seconds ≤ 10"));
        }
        let target = self.session.status().time + seconds;
        while self.session.status().time + 0.5 * self.interval < target {
            // A session that is no longer running would never reach the target.
            if self.session.tick().map_err(error)?.is_none() {
                return Err(error("system session is not running"));
            }
        }
        Ok(self.session.status().time)
    }
    pub fn time(&self) -> f64 {
        self.session.status().time
    }
    pub fn step(&self) -> f64 {
        self.interval
    }
    /// Current values of the observed quantities, in `labels` order.
    pub fn values(&self) -> Vec<f64> {
        self.ids.iter().map(|id| sim_inspect_scalar(self.session.latest(), id)).collect()
    }
    pub fn labels(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.labels).map_err(error)
    }
}

fn sim_inspect_scalar(frame: &sim_runtime::inspect::SampleFrame, id: &str) -> f64 {
    sim_runtime::inspect::animation::scalar(Some(frame), id).map(|v| v.value).unwrap_or(f64::NAN)
}
