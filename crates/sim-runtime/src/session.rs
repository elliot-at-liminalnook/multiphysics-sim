//! Host-independent interactive episodes. Hosts send inputs and render snapshots;
//! all integration, sensor sampling and Rhai execution remain in Rust.
use crate::{BuildOptions, PhysicalRobot, registry};
use serde::{Deserialize, Serialize};
use sim_core::{Channel, Contract, Coupler, CouplerError, QuantityKind};
use sim_domain_robot::PhysicalModel;
use sim_script::{RhaiController, Sources, parameter_map};
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputChannel {
    pub name: String,
    pub kind: QuantityKind,
    pub lower: f64,
    pub upper: f64,
    pub initial: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerProgram {
    pub sources: Sources,
    #[serde(default = "empty_parameters")]
    pub parameters: serde_json::Value,
    /// Additional named command inputs, appended to the controller observations.
    #[serde(default)]
    pub inputs: Vec<InputChannel>,
}
fn empty_parameters() -> serde_json::Value {
    serde_json::json!({})
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    pub version: u32,
    pub robot: PhysicalModel,
    pub options: BuildOptions,
    #[serde(default)]
    pub controller: Option<ControllerProgram>,
    pub period_s: f64,
    pub duration_s: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Telemetry {
    pub sample_time: f64,
    pub sensors: Vec<f64>,
    pub actuators: Vec<f64>,
}

struct EpisodeCoupler {
    policy: Option<Box<dyn Coupler>>,
    command_channels: Vec<Channel>,
    command_values: Arc<Mutex<Vec<f64>>>,
    telemetry: Arc<Mutex<Telemetry>>,
    bounds: Vec<(Option<f64>, Option<f64>)>,
}
impl Coupler for EpisodeCoupler {
    fn open(&mut self, contract: &Contract) -> Result<(), CouplerError> {
        if let Some(policy) = &mut self.policy {
            let mut augmented = contract.clone();
            augmented.sensors.extend(self.command_channels.clone());
            policy.open(&augmented)?;
        }
        Ok(())
    }
    fn sample(
        &mut self,
        t: f64,
        sensors: &[f64],
        actuators: &mut [f64],
    ) -> Result<(), CouplerError> {
        let values = self.command_values.lock().unwrap().clone();
        if let Some(policy) = &mut self.policy {
            let mut inputs = sensors.to_vec();
            inputs.extend(values);
            policy.sample(t, &inputs, actuators)?;
        } else {
            actuators.copy_from_slice(&values);
        }
        for (k, (&value, (lower, upper))) in actuators.iter().zip(&self.bounds).enumerate() {
            if !value.is_finite()
                || lower.is_some_and(|lo| value < lo)
                || upper.is_some_and(|hi| value > hi)
            {
                return Err(CouplerError::Other(format!(
                    "actuator {k} command {value} violates its CAD limits"
                )));
            }
        }
        *self.telemetry.lock().unwrap() = Telemetry {
            sample_time: t,
            sensors: sensors.to_vec(),
            actuators: actuators.to_vec(),
        };
        Ok(())
    }
    fn close(&mut self) {
        if let Some(policy) = &mut self.policy {
            policy.close();
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LinkPose {
    pub name: String,
    pub position_m: [f64; 3],
    /// Row-major local-to-world rotation matrix; model and world are Z-up.
    pub rotation: [[f64; 3]; 3],
}
impl LinkPose {
    pub(crate) fn valid_rigid_transform(&self) -> bool {
        if self.position_m.iter().any(|v| !v.is_finite()) { return false; }
        let r=self.rotation;
        let dot=|a:[f64;3],b:[f64;3]|->f64{(0..3).map(|i|a[i]*b[i]).sum()};
    if !r.iter().flatten().all(|v| v.is_finite()) {
        return false;
    }
    for i in 0..3 {
        for j in 0..3 {
            if (dot(r[i], r[j]) - if i == j { 1.0 } else { 0.0 }).abs() > 1e-8 {
                return false;
            }
        }
    }
    let cross = [
        r[1][1] * r[2][2] - r[1][2] * r[2][1],
        r[1][2] * r[2][0] - r[1][0] * r[2][2],
        r[1][0] * r[2][1] - r[1][1] * r[2][0],
    ];
    (dot(r[0], cross) - 1.0).abs() <= 1e-8
}
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Contact {
    pub link: usize,
    pub other: Option<usize>,
    pub point_m: [f64; 3],
    pub force_n: [f64; 3],
    pub penetration_m: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EpisodeFrame {
    pub time_s: f64,
    pub done: bool,
    pub poses: Vec<LinkPose>,
    pub joint_positions: Vec<f64>,
    pub telemetry: Telemetry,
    pub contacts: Vec<Contact>,
    pub error: Option<String>,
}

/// Portable recording contains the exact scene, seed, and one command per step.
/// Replaying reconstructs controller state and noise history as well as physics.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recording {
    pub version: u32,
    pub scene: Scene,
    pub seed: u64,
    pub actions: Vec<Vec<f64>>,
}

pub struct Session {
    pub robot: PhysicalRobot,
    pub scene: Scene,
    pub contract: Contract,
    pub inputs: Vec<InputChannel>,
    values: Arc<Mutex<Vec<f64>>>,
    telemetry: Arc<Mutex<Telemetry>>,
    seed: u64,
    actions: Vec<Vec<f64>>,
    error: Option<String>,
}
impl Session {
    /// Bounded diagnostic capture, disabled by default. Zero clears and disables
    /// capture; changing the limit clears old records without resetting physics.
    pub fn set_attempt_audit_limit(&mut self, limit: usize) -> Result<(), String> {
        if limit > 10_000 { return Err("attempt limit must be 0..10000".into()); }
        for island in &mut self.robot.runtime.islands { island.set_attempt_audit_limit(limit); }
        Ok(())
    }

    pub fn new(scene: Scene, seed: u64) -> Result<Self, String> {
        if scene.version != 1 {
            return Err("unsupported scene version".into());
        }
        if seed > (1_u64 << 53) - 1 {
            return Err("seed exceeds portable exact integer range".into());
        }
        for (name, value) in [
            ("period_s", scene.period_s),
            ("duration_s", scene.duration_s),
            ("step", scene.options.step),
            ("sample", scene.options.sample),
        ] {
            if !value.is_finite() || value <= 0. {
                return Err(format!("{name} must be finite and positive"));
            }
        }
        let steps = scene.period_s / scene.options.step;
        if (steps - steps.round()).abs() > 1e-8 {
            return Err("episode period must be an integer number of physics steps".into());
        }
        if scene.robot.control.mode == "trajectory" {
            return Err(
                "interactive scenes must use held targets or an explicit controller".into(),
            );
        }
        let mut robot = PhysicalRobot::build(scene.robot.clone(), &registry(), &scene.options)?;
        robot.runtime.seed(seed);
        let seam = robot
            .seam
            .ok_or("interactive robot must expose an actuator/controller seam")?;
        let contract = robot.runtime.contract(seam);
        let bounds: Vec<_> = contract
            .actuators
            .iter()
            .map(|a| {
                if a.name.ends_with(".duty") {
                    return (Some(-1.), Some(1.));
                }
                scene
                    .robot
                    .joint(a.name.trim_end_matches(".target"))
                    .and_then(|j| j.limits)
                    .map(|[lo, hi]| (lo.is_finite().then_some(lo), hi.is_finite().then_some(hi)))
                    .unwrap_or((None, None))
            })
            .collect();
        let inputs = if let Some(program) = &scene.controller {
            program.inputs.clone()
        } else {
            contract
                .actuators
                .iter()
                .zip(&bounds)
                .map(|(a, &(lo, hi))| InputChannel {
                    name: a.name.clone(),
                    kind: a.kind,
                    lower: lo.unwrap_or(-f64::MAX),
                    upper: hi.unwrap_or(f64::MAX),
                    initial: scene
                        .robot
                        .control
                        .targets
                        .get(a.name.trim_end_matches(".target"))
                        .copied()
                        .unwrap_or(0.),
                })
                .collect()
        };
        let mut names = BTreeSet::new();
        for input in &inputs {
            if input.name.is_empty()
                || !names.insert(&input.name)
                || (scene.controller.is_some()
                    && contract.sensors.iter().any(|s| s.name == input.name))
            {
                return Err(format!("duplicate or empty command input: {}", input.name));
            }
            if !input.lower.is_finite()
                || !input.upper.is_finite()
                || !input.initial.is_finite()
                || input.lower > input.upper
                || input.initial < input.lower
                || input.initial > input.upper
            {
                return Err(format!("invalid limits/initial value for {}", input.name));
            }
        }
        let policy: Option<Box<dyn Coupler>> = scene
            .controller
            .as_ref()
            .map(|program| {
                let parameters = parameter_map(&program.parameters).map_err(|e| e.to_string())?;
                RhaiController::with_seed(program.sources.clone(), parameters, seed)
                    .map(|c| Box::new(c) as Box<dyn Coupler>)
                    .map_err(|e| e.to_string())
            })
            .transpose()?;
        let values = Arc::new(Mutex::new(inputs.iter().map(|i| i.initial).collect()));
        let telemetry = Arc::new(Mutex::new(Telemetry::default()));
        robot
            .runtime
            .attach(
                seam,
                Box::new(EpisodeCoupler {
                    policy,
                    command_channels: inputs
                        .iter()
                        .map(|i| Channel {
                            name: i.name.clone(),
                            kind: i.kind,
                        })
                        .collect(),
                    command_values: values.clone(),
                    telemetry: telemetry.clone(),
                    bounds,
                }),
            )
            .map_err(|e| e.to_string())?;
        Ok(Self {
            robot,
            scene,
            contract,
            inputs,
            values,
            telemetry,
            seed,
            actions: Vec::new(),
            error: None,
        })
    }

    pub fn step(&mut self, action: &[f64]) -> Result<EpisodeFrame, String> {
        if let Some(error) = &self.error {
            return Err(format!("episode failed: {error}; reset before continuing"));
        }
        if self.robot.time() >= self.scene.duration_s - 1e-10 {
            return Err("episode finished; reset before continuing".into());
        }
        if action.len() != self.inputs.len() {
            return Err(format!(
                "expected {} inputs, received {}",
                self.inputs.len(),
                action.len()
            ));
        }
        for (input, &value) in self.inputs.iter().zip(action) {
            if !value.is_finite() || value < input.lower || value > input.upper {
                return Err(format!(
                    "{} input outside [{}, {}]",
                    input.name, input.lower, input.upper
                ));
            }
        }
        *self.values.lock().unwrap() = action.to_vec();
        self.actions.push(action.to_vec());
        if let Err(error) = self.robot.advance(self.scene.period_s) {
            self.error = Some(error.clone());
            return Err(error);
        }
        Ok(self.frame())
    }

    pub fn reset(&mut self, seed: u64) -> Result<EpisodeFrame, String> {
        let replacement = Self::new(self.scene.clone(), seed)?;
        *self = replacement;
        Ok(self.frame())
    }

    pub fn frame(&self) -> EpisodeFrame {
        let poses = self
            .robot
            .poses()
            .into_iter()
            .zip(&self.robot.model.links)
            .map(|((r, p), link)| LinkPose {
                name: link.name.clone(),
                position_m: p.into(),
                rotation: std::array::from_fn(|i| std::array::from_fn(|j| r[(i, j)])),
            })
            .collect();
        let contacts = self
            .robot
            .contacts()
            .into_iter()
            .map(|c| Contact {
                link: c.link,
                other: c.other,
                point_m: c.point.into(),
                force_n: c.force.into(),
                penetration_m: c.penetration,
            })
            .collect();
        EpisodeFrame {
            time_s: self.robot.time(),
            done: self.error.is_some() || self.robot.time() >= self.scene.duration_s - 1e-10,
            poses,
            joint_positions: self.robot.joint_angles(),
            telemetry: self.telemetry.lock().unwrap().clone(),
            contacts,
            error: self.error.clone(),
        }
    }

    pub fn recording(&self) -> Recording {
        Recording {
            version: 1,
            scene: self.scene.clone(),
            seed: self.seed,
            actions: self.actions.clone(),
        }
    }

    pub fn replay(recording: Recording) -> Result<Self, String> {
        if recording.version != 1 {
            return Err("unsupported recording version".into());
        }
        let mut session = Self::new(recording.scene, recording.seed)?;
        for action in recording.actions {
            session.step(&action)?;
        }
        Ok(session)
    }
}
