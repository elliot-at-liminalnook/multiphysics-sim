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
