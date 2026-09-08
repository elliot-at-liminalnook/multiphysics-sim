//! Reduced integration diagnostic with optional shared motor components at
//! declared voltage/temperature boundaries and optional sampled servo firmware.
//! Winding temperature is imposed; no thermal network is integrated here.
use crate::embedded_policy::{InputEvent, PolicyConfig, SampledPolicy};
use crate::session::{Scene, Session};
use serde::Deserialize;
use serde_json::json;
use sim_domain_robot::Generalized;
use sim_domain_control::motion_clock::{MotionClock, MotionClockConfig, MotionClockState};
use sim_domain_robot::articulated::embedding::{
    DriverBoundary, EmbeddedDriverBank, EmbeddedDriverConfig, EmbeddedMotorBank,
    EmbeddedMotorConfig, EmbeddingConfig, ImplicitSolverWorkspace, ImplicitStepConfig,
    MotorBoundary, RigidEmbedding,
};
use sim_domain_robot::articulated::embedding::{
    EmbeddedServoBank, EmbeddedServoConfig, ServoBoundary,
};
use web_time::Instant;

#[derive(Clone, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct MotionGate {
    pub clock: MotionClockConfig,
    pub support_links: Vec<String>,
    pub minimum_upward_force_n: f64,
    /// Must explicitly acknowledge privileged observations absent from CAD sensors.
    pub observation_source: String,
}

#[derive(Clone, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Opt in to the scene's Rhai program over explicitly ideal observations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<PolicyConfig>,
    /// Process-global optional timers, for a standalone diagnostic process.
    /// Nested solver/phase buckets must not be summed as disjoint costs.
    #[serde(default)]
    pub profile_solver: bool,
    #[serde(default)]
    pub motion_gate: Option<MotionGate>,
    /// Optional at-rest pose in CAD motor order, radians. Original closure is
    /// solved first; registered motor gear angles start aligned without preload.
    #[serde(default)]
    pub initial_coordinates: Option<Vec<f64>>,
    /// Explicit rigid translation of the floating initial base in world metres.
    /// This changes the initial condition, not source geometry or floor height.
    #[serde(default)]
    pub initial_base_translation_m: Option<[f64; 3]>,
    #[serde(default)]
    pub embedding: EmbeddingConfig,
    /// Absent retains the previous explicit midpoint diagnostic.
    #[serde(default)]
    pub implicit: Option<ImplicitStepConfig>,
    /// Opt-in bounded recovery for pure mechanics/effective servos. Controller
    /// samples remain outside retries. This is not timestep error estimation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mechanical_subdivision: Option<sim_dynamics::hybrid::HybridConfig>,
    #[serde(default)]
    pub motors: Option<MotorExperiment>,
    /// Optional bounded tail of trial endpoints for failure diagnosis. Adds
    /// allocation/serialization work inside the solve; not a timing baseline.
    #[serde(default)]
    pub trace_trials: usize,
    /// Accepted continuous contact endpoints, excluding event-search trials.
    #[serde(default)]
    pub audit_contact_steps: bool,
    pub step_s: f64,
    pub steps: usize,
    pub report_every: usize,
    /// Base world force/moment then all joint force/torque coordinates.
    pub applied_generalized_loads: Vec<f64>,
    /// Explicit environment disturbance, additional to fixed loads and actuators.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub world_loads: Option<sim_domain_robot::world_load::WorldLoadSchedule>,
}
#[derive(Clone, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct MotorExperiment {
    /// Explicit browser/training reduction. Bypasses electronics, internal
    /// inertia, gearbox compliance/backlash, firmware, latency and thermal
    /// behavior. All parameters and their assumption reference are required.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective: Option<EffectiveServoProfile>,
    /// CAD motor order; these are imposed boundaries, not battery/driver models.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundaries: Option<Vec<MotorBoundary>>,
    /// Mutually exclusive with direct winding boundaries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drivers: Option<Vec<DriverBoundary>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub servos: Option<Vec<ServoBoundary>>,
    /// Explicit motor-order angle references in radians, sampled by the original
    /// firmware clocks. No alteration of motor/driver/firmware parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_trajectory: Option<sim_domain_control::trajectory::TrajectoryConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_coordinates: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_cad_sha256: Option<String>,
    /// Winding V, rotor torque N m, gearbox angle evolution rad/s.
    pub residual_scales: [f64; 3],
    #[serde(default)]
    pub events: Option<sim_dynamics::hybrid::HybridConfig>,
}

#[derive(Clone, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct EffectiveServoProfile {
    pub version: u32,
    pub assumption_reference: String,
    pub components: Vec<EffectiveServoBinding>,
}
#[derive(Clone, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct EffectiveServoBinding {
    pub dof: String,
    pub parameters: std::collections::BTreeMap<String,f64>,
}

/// Full diagnostics for reproducible headless experiments, or bounded latest
/// state for interactive hosts. Physics and event processing are identical.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CaptureMode {
    Full,
    Latest,
}

#[derive(Clone, serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddedRecording {
    pub version: u32,
    pub kind: String,
    pub scene: Scene,
    pub config: Config,
    pub seed: u64,
    pub completed_steps: usize,
    /// A failed attempt can occur after the last committed physics step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input_events: Vec<InputEvent>,
}

/// Incremental execution of a CAD-derived rigid mechanism and registered motor,
/// driver and servo components. No robot-specific topology lives here.
/// The constructor's scene and experiment are immutable for the session.
pub struct EmbeddedSession {
    world_loads: Option<sim_domain_robot::world_load::BoundWorldLoads>,
    effective_servos: Vec<sim_domain_robot::effective_servo::EffectiveServo>,
    policy: Option<SampledPolicy>,
    input_events: Vec<InputEvent>,
    replay_inputs: std::collections::VecDeque<InputEvent>,
    replay_expected: Option<(usize, Option<String>)>,
    session: Session,
    config: Config,
    seed: u64,
    capture: CaptureMode,
    retain_solver_diagnostics: bool,
    names: Vec<String>,
    independent_joint_indices: Vec<usize>,
    g: sim_domain_robot::Generalized,
    trajectory: Option<sim_domain_control::trajectory::Trajectory>,
    motion_clock: Option<MotionClock>,
    policy_stride: Option<usize>,
    clock_state: MotionClockState,
    motion_gate_trace: Vec<serde_json::Value>,
    motor_configs: Vec<EmbeddedMotorConfig>,
    driver_configs: Vec<EmbeddedDriverConfig>,
    servo_configs: Vec<EmbeddedServoConfig>,
    bank: Option<EmbeddedMotorBank>,
    driver_bank: Option<EmbeddedDriverBank>,
    servo_bank: Option<EmbeddedServoBank>,
    motor_states: Vec<f64>,
    servo_states: Vec<f64>,
    completed_steps: usize,
    error: Option<String>,
    step_wall_s: f64,
    solver_steps: Vec<sim_domain_robot::articulated::embedding::ImplicitStepDiagnostics>,
    hybrid_steps: Vec<sim_dynamics::hybrid::HybridDiagnostics>,
    hybrid_solves: Vec<sim_domain_robot::articulated::embedding::MotorSolveStatistics>,
    contact_steps: Option<Vec<sim_domain_robot::articulated::embedding::EmbeddedContactStep>>,
    trials: std::cell::RefCell<std::collections::VecDeque<serde_json::Value>>,
    frames: Vec<serde_json::Value>,
    workspace: ImplicitSolverWorkspace,
}

impl EmbeddedSession {
    pub fn new(
        scene: Scene,
        config: Config,
        seed: u64,
        capture: CaptureMode,
    ) -> Result<Self, String> {
        if !config.step_s.is_finite()
            || config.step_s <= 0.0
            || config.steps == 0
            || config.steps > 1000000
            || config.report_every == 0
            || config.steps % config.report_every != 0
            || config.trace_trials > 256
        {
            return Err(
                "positive finite step, 1..1000000 steps and a dividing report_every required"
                    .into(),
            );
        }
        if config.trace_trials > 0 && config.motors.as_ref().is_some_and(|m| m.events.is_some()) {
            return Err("trace_trials is available only for the unscheduled diagnostic; event runs report hybrid diagnostics".into());
        }
        if let Some(m) = &config.motors {
            if usize::from(m.boundaries.is_some())
                + usize::from(m.drivers.is_some())
                + usize::from(m.servos.is_some())
                != 1
            {
                return Err(
                    "choose exactly one of winding boundaries, driver inputs or servo targets"
                        .into(),
                );
            }
            if m.effective.is_some() && (m.servos.is_none() || m.events.is_some()) {
                return Err("effective servo profile requires targets and no detailed firmware event schedule".into());
            }
            if m.servos.is_some() && m.events.is_none() && m.effective.is_none() {
                return Err("servo targets require explicit event scheduling".into());
            }
        }
        let trajectory = config
            .motors
            .as_ref()
            .and_then(|m| m.target_trajectory.clone())
            .map(sim_domain_control::trajectory::Trajectory::new)
            .transpose()?;
        if config.policy.as_ref().is_some_and(|p|p.step_reference.is_some())
            && (trajectory.is_some()||config.motion_gate.is_some()) {
            return Err("online step references cannot be combined with an offline trajectory or motion gate".into());
        }
        if let Some(trajectory) = &trajectory {
            let servos = config
                .motors
                .as_ref()
                .and_then(|m| m.servos.as_ref())
                .ok_or("target trajectory requires servo firmware")?;
            if trajectory.dimension() != servos.len()
                || trajectory
                    .sample(0.0)?
                    .values
                    .iter()
                    .zip(servos)
                    .any(|(t, s)| *t != s.target_rad)
            {
                return Err(
                    "trajectory must match named motor order and initial servo targets".into(),
                );
            }
        }
        if trajectory.is_some() {
            let expected = config
                .motors
                .as_ref()
                .and_then(|m| m.expected_cad_sha256.as_deref())
                .ok_or("target trajectory requires CAD source hash")?;
            if expected.trim().is_empty()
                || Some(expected)
                    != scene
                        .robot
                        .source
                        .get("cad_sha256")
                        .and_then(|s| s.as_str())
            {
                return Err("target trajectory requires matching CAD source hash".into());
            }
        }
        let session = Session::new(scene, seed)?;
        let art = &session.robot.art;
        let motion_clock = config.motion_gate.as_ref().map(|gate| {
        let n=gate.clock.period_s/config.step_s;
        if trajectory.is_none() || !gate.minimum_upward_force_n.is_finite() || gate.minimum_upward_force_n<=0.0
            || gate.observation_source!="ideal_runtime_floor_force" || !n.is_finite() || n<1.0 || n>1e9
            || (n-n.round()).abs()>1e-8
            || config.motors.as_ref().and_then(|m|m.target_trajectory.as_ref()).unwrap().keyframes.last().unwrap().time_s!=gate.clock.duration_s {
            return Err("motion gate requires trajectory duration, positive force threshold, grid-aligned policy period and explicit ideal_runtime_floor_force observations".into());
        }
        MotionClock::new(gate.clock.clone())
    }).transpose()?;
        let policy_stride = config
            .motion_gate
            .as_ref()
            .map(|g| (g.clock.period_s / config.step_s).round() as usize);
        let clock_state = MotionClockState::default();
        let names: Vec<String> = session
            .scene
            .robot
            .motors
            .iter()
            .map(|m| {
                let joint = m.joint.as_ref().ok_or("motor has no declared joint")?;
                let found: Vec<_> = art
                    .dofs()
                    .filter(|(j, _)| &j.name == joint)
                    .map(|(_, d)| d.name.clone())
                    .collect();
                if found.len() != 1 {
                    return Err("motor does not select one independent coordinate".to_string());
                }
                Ok(found[0].clone())
            })
            .collect::<Result<_, String>>()?;
        if trajectory.is_some()
            && config
                .motors
                .as_ref()
                .and_then(|m| m.target_coordinates.as_ref())
                != Some(&names)
        {
            return Err("target trajectory requires exact named motor coordinates".into());
        }
        let map = RigidEmbedding::new(art, &names, config.embedding.clone())?;
        let effective_servos = config.motors.as_ref().and_then(|m|m.effective.as_ref()).map(|profile| {
            if profile.version!=1 || profile.assumption_reference.trim().is_empty()
                || names.is_empty() || profile.components.len()!=names.len() || profile.components.iter().zip(&names).any(|(c,n)|&c.dof!=n)
                || config.motors.as_ref().unwrap().servos.as_ref().unwrap().len()!=names.len()
                || config.motors.as_ref().unwrap().servos.as_ref().unwrap().iter().any(|s|!s.target_rad.is_finite()) {
                return Err("effective servo profile requires v1, an assumption reference and exact motor-coordinate bindings".into());
            }
            profile.components.iter().map(|c|sim_domain_robot::effective_servo::EffectiveServo::new(&c.parameters).map_err(|e|e.to_string())).collect::<Result<Vec<_>,String>>()
        }).transpose()?.unwrap_or_default();
        if config.applied_generalized_loads.len() != map.full_dimension()
            || config
                .applied_generalized_loads
                .iter()
                .any(|v| !v.is_finite())
        {
            return Err(format!(
                "expected {} finite full-coordinate applied loads",
                map.full_dimension()
            ));
        }
        if let Some(trajectory) = &trajectory {
            let dofs: Vec<_> = art.dofs().map(|(_, d)| d).collect();
            trajectory.validate_value_bounds(
                &map.independent_joint_indices()
                    .iter()
                    .map(|&i| (dofs[i].lower, dofs[i].upper))
                    .collect::<Vec<_>>(),
            )?;
        }
        let mut g = session.robot.generalized();
        if let Some(translation) = config.initial_base_translation_m {
            if art.bases.len() != 1
                || art.bases[0].grounded
                || translation.iter().any(|v| !v.is_finite())
            {
                return Err(
                    "initial translation requires one floating base and finite world metres".into(),
                );
            }
            for k in 0..3 {
                g.states[art.bases[0].state + k] += translation[k];
            }
        }
        if let Some(positions) = &config.initial_coordinates {
            g = map
                .solve(&g, positions, &vec![0.0; map.reduced_dimension()])?
                .generalized;
            if art.dofs().enumerate().any(|(i, (_, d))| {
                d.lower.is_some_and(|v| g.q[i] < v) || d.upper.is_some_and(|v| g.q[i] > v)
            }) {
                return Err("initial closed pose violates authored joint limits".into());
            }
        }
        if config.motors.is_some() && config.implicit.is_none() {
            return Err("coupled motor diagnostic requires implicit stepping".into());
        }
        let motor_configs: Vec<_> = config
            .motors
            .as_ref()
            .filter(|m|m.effective.is_none())
            .map(|experiment| {
                session
                    .scene
                    .robot
                    .motors
                    .iter()
                    .zip(&names)
                    .enumerate()
                    .map(|(i, (motor, dof))| {
                        let backlash = motor
                            .joint
                            .as_deref()
                            .and_then(|n| session.robot.model.joint(n))
                            .map(|j| j.physics.drive_backlash_rad(session.robot.model.version >= 4))
                            .transpose().map_err(|e| format!("{dof}: {e}"))?
                            .unwrap_or(0.0);
                        Ok::<_, String>(EmbeddedMotorConfig {
                            dof: dof.clone(),
                            residual_scales: experiment.residual_scales,
                            parameters: sim_domain_robot::motor::cad_motor_unit_parameters(
                                motor,
                                backlash,
                                session.scene.robot.world.ambient_c + 273.15,
                                false,
                                experiment.events.is_some(),
                            )
                            .into_iter()
                            .chain(session.scene.options.motor_dynamics.parameter_flags())
                            .map(|(k, v)| (k.into(), v))
                            .chain(
                                config
                                    .initial_coordinates
                                    .as_ref()
                                    .map(|q| ("initial.angle".into(), q[i])),
                            )
                            .collect(),
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()
            })
            .transpose()?
            .unwrap_or_default();
        let mut bank = config
            .motors
            .as_ref()
            .filter(|m|m.effective.is_none())
            .map(|m| {
                if m.events.is_some() {
                    EmbeddedMotorBank::new_with_events(art, &motor_configs)
                } else {
                    EmbeddedMotorBank::new(art, &motor_configs)
                }
            })
            .transpose()?;
        if config.audit_contact_steps {
            if config
                .motors
                .as_ref()
                .and_then(|m| m.events.as_ref())
                .is_none()
            {
                return Err(
                    "contact-step audit currently requires scheduled motor integration".into(),
                );
            }
            bank.as_mut().unwrap().set_contact_step_audit(true);
        }
        let driver_configs: Vec<_> = if config
            .motors
            .as_ref()
            .is_some_and(|m| m.effective.is_none() && (m.drivers.is_some() || m.servos.is_some()))
        {
            session
                .scene
                .robot
                .motors
                .iter()
                .zip(&names)
                .map(|(m, dof)| EmbeddedDriverConfig {
                    dof: dof.clone(),
                    parameters: sim_domain_robot::motor::cad_h_bridge_parameters(m),
                })
                .collect()
        } else {
            vec![]
        };
        let driver_bank = if driver_configs.is_empty() {
            None
        } else {
            Some(EmbeddedDriverBank::new(
                bank.as_ref().unwrap(),
                &driver_configs,
            )?)
        };
        let servo_configs: Vec<_> = if config.motors.as_ref().is_some_and(|m| m.effective.is_none() && m.servos.is_some()) {
            session
                .scene
                .robot
                .motors
                .iter()
                .zip(&names)
                .map(|(m, dof)| {
                    if m.firmware.kind == "none" {
                        return Err(format!("CAD motor {} declares no firmware", m.name));
                    }
                    Ok(EmbeddedServoConfig {
                        dof: dof.clone(),
                        parameters: sim_domain_robot::motor::cad_servo_firmware_parameters(m),
                    })
                })
                .collect::<Result<_, String>>()?
        } else {
            vec![]
        };
        let servo_bank = if servo_configs.is_empty() {
            None
        } else {
            Some(EmbeddedServoBank::new(
                bank.as_ref().unwrap(),
                &servo_configs,
            )?)
        };
        let servo_states = servo_bank
            .as_ref()
            .map(|s| s.initial_states())
            .unwrap_or_default();
        let motor_states = bank
            .as_ref()
            .map(|b| b.initial_states())
            .unwrap_or_default();

        if config.mechanical_subdivision.is_some()
            && (config.implicit.is_none() || bank.is_some() || servo_bank.is_some())
        {
            return Err("mechanical subdivision requires pure implicit mechanics or effective servos; coupled motor events use their own adapter".into());
        }
        if config.implicit.as_ref().is_some_and(|c|
            (c.restart_failed_reused_mechanics || c.cached_mechanical_iteration_limit.is_some())
                && config.mechanical_subdivision.is_none())
        {
            return Err("mechanical fresh-restart options require the mechanical subdivision adapter".into());
        }

        let independent_joint_indices = map.independent_joint_indices().to_vec();
        let policy = config
            .policy
            .as_ref()
            .map(|p| {
                let inputs = config
                    .motors
                    .as_ref()
                    .and_then(|m| m.servos.as_ref())
                    .ok_or("sampled policy requires servo integration")?;
                let program = session
                    .scene
                    .controller
                    .as_ref()
                    .ok_or("sampled policy requires a scene controller program")?;
                SampledPolicy::new(
                    art,
                    &names,
                    &independent_joint_indices,
                    &inputs.iter().map(|s| s.target_rad).collect::<Vec<_>>(),
                    p,
                    program,
                    session.scene.period_s,
                    config.step_s,
                    seed,
                )
            })
            .transpose()?;
        let contact_steps =
            (config.audit_contact_steps && capture == CaptureMode::Full).then(Vec::new);
        if config.profile_solver {
            sim_solve::profile::enable();
            sim_solve::profile::reset();
        }
        let world_loads = config.world_loads.as_ref().map(|loads| loads.bind(
            &art.bases.iter().filter(|b| !b.grounded).map(|b| art.links[b.link].name.clone()).collect::<Vec<_>>(),
            config.step_s, config.steps,
        )).transpose()?;
        let mut runner = Self {
            world_loads,
            effective_servos,
            policy,
            input_events: vec![],
            replay_inputs: Default::default(),
            replay_expected: None,
            session,
            config,
            seed,
            capture,
            names,
            independent_joint_indices,
            g,
            trajectory,
            motion_clock,
            policy_stride,
            clock_state,
            motion_gate_trace: vec![],
            motor_configs,
            driver_configs,
            servo_configs,
            bank,
            driver_bank,
            servo_bank,
            motor_states,
            servo_states,
            completed_steps: 0,
            error: None,
            step_wall_s: 0.0,
            solver_steps: vec![],
            retain_solver_diagnostics: false,
            hybrid_steps: vec![],
            hybrid_solves: vec![],
            contact_steps,
            trials: Default::default(),
            frames: vec![],
            workspace: Default::default(),
        };
        if capture == CaptureMode::Full {
            runner.frames.push(runner.frame()?);
        }
        Ok(runner)
    }

    pub fn completed_steps(&self) -> usize {
        self.completed_steps
    }
    /// Opt in to accumulating solver history even in Latest capture mode.
    /// This is host-side diagnostic memory, not physics or replay state. Enable
    /// only for bounded experiments; reset/replay starts with collection off.
    pub fn retain_solver_diagnostics(&mut self, enabled: bool) {
        self.retain_solver_diagnostics = enabled;
        if !enabled && self.capture == CaptureMode::Latest {
            self.solver_steps.clear();
            self.hybrid_steps.clear();
            self.hybrid_solves.clear();
        }
    }
    /// Diagnostics for accepted non-event implicit steps. Rejected solves and
    /// the separate hybrid motor/event path are not included in this history.
    pub fn implicit_step_diagnostics(
        &self,
    ) -> &[sim_domain_robot::articulated::embedding::ImplicitStepDiagnostics] {
        &self.solver_steps
    }
    /// Accepted outer intervals with event/subdivision attempt counts. Failed
    /// outer intervals are reported by the session error, not this history.
    pub fn interval_diagnostics(&self) -> &[sim_dynamics::hybrid::HybridDiagnostics] {
        &self.hybrid_steps
    }
    pub fn remaining_steps(&self) -> usize {
        self.config.steps - self.completed_steps
    }
    pub fn done(&self) -> bool {
        self.error.is_some() || self.remaining_steps() == 0
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    pub fn scene(&self) -> &Scene {
        &self.session.scene
    }
    pub(crate) fn articulated(&self) -> &sim_domain_robot::Articulated {
        &self.session.robot.art
    }
    pub fn config(&self) -> &Config {
        &self.config
    }
    pub fn coordinate_names(&self) -> &[String] {
        &self.names
    }
    pub fn joint_indices(&self) -> &[usize] {
        &self.independent_joint_indices
    }

    /// Layout and physical configuration for bounded diagnostic captures.
    /// Available in Latest mode without retaining an entire episode report.
    /// Values come from the same banks and recipe as the full report.
    pub fn diagnostic_metadata(&self) -> serde_json::Value {
        json!({"source":self.session.scene.robot.source,"scene_options":self.session.scene.options,
            "world":self.session.scene.robot.world,"embedding":self.config.embedding,"implicit":self.config.implicit,
            "independent_coordinates":self.names,"independent_joint_indices":self.independent_joint_indices,
            "motor_components":self.motor_configs,"driver_components":self.driver_configs,"servo_components":self.servo_configs,
            "motor_state_layout":self.bank.as_ref().map(|b|b.state_layout()),
            "servo_state_layout":self.servo_bank.as_ref().map(|b|b.state_layout()),
            "motor_experiment":self.config.motors})
    }

    pub fn inputs(&self) -> &[crate::session::InputChannel] {
        self.policy.as_ref().map(|p| p.inputs()).unwrap_or(&[])
    }
    /// Currently held validated policy inputs, including their reset values.
    pub fn input_values(&self) -> &[f64] {
        self.policy.as_ref().map(|p| p.values()).unwrap_or(&[])
    }
    pub fn neural_correction(&self, target: &str) -> Option<f64> {
        self.policy.as_ref()?.correction(target)
    }
    pub fn policy_metadata(&self) -> serde_json::Value {
        self.policy
            .as_ref()
            .map(|p| p.metadata())
            .unwrap_or(json!(null))
    }
    pub fn set_inputs(&mut self, values: &[f64]) -> Result<(), String> {
        if !self.replay_inputs.is_empty() {
            return Err("finish recorded inputs before changing commands".into());
        }
        if let Some(p) = self.policy.as_mut() {
            p.validate_inputs(values)?;
            if p.values() != values {
                p.set_inputs(values)?;
                let event = InputEvent {
                    at_step: self.completed_steps,
                    values: values.to_vec(),
                };
                if self
                    .input_events
                    .last()
                    .is_some_and(|e| e.at_step == self.completed_steps)
                {
                    *self.input_events.last_mut().unwrap() = event;
                } else {
                    self.input_events.push(event);
                }
            }
            Ok(())
        } else if values.is_empty() {
            Ok(())
        } else {
            Err("fixed-reference experiment declares no policy inputs".into())
        }
    }
    fn apply_replay_inputs(&mut self) -> Result<(), String> {
        while self
            .replay_inputs
            .front()
            .is_some_and(|e| e.at_step == self.completed_steps)
        {
            let event = self.replay_inputs.pop_front().unwrap();
            self.policy
                .as_mut()
                .ok_or("recorded inputs require a policy")?
                .set_inputs(&event.values)?;
        }
        Ok(())
    }

    /// Advance exactly the existing fixed nominal steps, preserving firmware,
    /// event modes, contact memory and solver workspace across host calls.
    /// A failure latches; callers must reset/replay instead of continuing a
    /// potentially failed component transaction. Rendering never chooses dt.
    pub fn advance(&mut self, steps: usize) -> Result<(), String> {
        if steps == 0 || steps > 1_000_000 {
            return Err("advance count must be 1..1000000".into());
        }
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        for _ in 0..steps.min(self.remaining_steps()) {
            self.apply_replay_inputs()?;
            if let Err(e) = self.advance_one() {
                return Err(self.latch_failure(e));
            }
            if let Some((expected_steps, failure)) = &self.replay_expected {
                if failure.is_some() && self.completed_steps > *expected_steps {
                    return Err(self.latch_failure("recorded failure did not reproduce".into()));
                }
                if failure.is_none()
                    && self.completed_steps == *expected_steps
                    && self.remaining_steps() > 0
                {
                    self.replay_expected = None;
                }
            }
            if self.capture == CaptureMode::Full
                && self.completed_steps % self.config.report_every == 0
            {
                self.frames.push(self.frame()?);
            }
            if self.capture == CaptureMode::Latest {
                if !self.retain_solver_diagnostics {
                    self.solver_steps.clear();
                    self.hybrid_steps.clear();
                    self.hybrid_solves.clear();
                }
                self.motion_gate_trace.clear();
            }
        }
        self.apply_replay_inputs()?;
        if self.remaining_steps() == 0 {
            if let Some(clock) = &self.motion_clock {
                let reference = clock.reference_at(
                    &self.clock_state,
                    self.completed_steps as f64 * self.config.step_s,
                )?;
                if reference < self.config.motion_gate.as_ref().unwrap().clock.duration_s - 1e-10 {
                    let e = format!(
                        "simulation horizon ended before motion completed: reference time {reference}s"
                    );
                    return Err(self.latch_failure(e));
                }
            }
        }
        if self.remaining_steps() == 0 {
            if self
                .replay_expected
                .as_ref()
                .is_some_and(|(_, failure)| failure.is_some())
            {
                return Err(self.latch_failure("recorded failure did not reproduce".into()));
            }
            self.replay_expected = None;
        }
        Ok(())
    }

    fn latch_failure(&mut self, actual: String) -> String {
        let error = match &self.replay_expected {
            Some((steps, expected))
                if *steps != self.completed_steps
                    || expected.as_deref() != Some(actual.as_str()) =>
            {
                format!("replay failure mismatch at step {}: expected {:?} at step {}; observed {actual}", self.completed_steps, expected, steps)
            }
            _ => actual,
        };
        self.error = Some(error.clone());
        error
    }

    fn advance_one(&mut self) -> Result<(), String> {
        let Self {
            world_loads,
            effective_servos,
            policy,
            session,
            config,
            names,
            g,
            trajectory,
            motion_clock,
            policy_stride,
            clock_state,
            motion_gate_trace,
            bank,
            driver_bank,
            servo_bank,
            motor_states,
            servo_states,
            completed_steps,
            step_wall_s,
            solver_steps,
            hybrid_steps,
            hybrid_solves,
            contact_steps,
            trials,
            workspace,
            ..
        } = self;
        let art = &session.robot.art;
        // Construction validates only immutable coordinate layout; it does not
        // solve dynamics or discard the persistent implicit workspace.
        let map = RigidEmbedding::new(art, names, config.embedding.clone())?;
        let boundaries_at = |time: f64, states: &[f64]| -> Result<Vec<MotorBoundary>, String> {
            match config.motors.as_ref() {
                Some(m) => {
                    if let Some(inputs) = &m.drivers {
                        driver_bank
                            .as_ref()
                            .ok_or("missing driver bank")?
                            .evaluate(time, states, inputs)
                            .map(|r| r.0)
                    } else {
                        Ok(m.boundaries
                            .as_ref()
                            .ok_or("servo boundaries require held control state")?
                            .clone())
                    }
                }
                None => Ok(vec![]),
            }
        };

        let i = *completed_steps;
        let mut applied_loads = config.applied_generalized_loads.clone();
        if let Some(loads) = world_loads { loads.add_to(i, &mut applied_loads)?; }
        trials.borrow_mut().clear();
        let start = Instant::now();
        let time = i as f64 * config.step_s;
        let mut pending_clock = clock_state.clone();
        let mut pending_gate = None;
        if let (Some(clock), Some(stride), Some(gate)) =
            (&motion_clock, *policy_stride, &config.motion_gate)
        {
            if i % stride == 0 {
                let forces =
                    crate::support::ideal_upward_floor_forces(art, &g, &gate.support_links)?;
                let ready = forces.iter().all(|f| *f >= gate.minimum_upward_force_n);
                clock.sample(&mut pending_clock, time, ready)?;
                pending_gate = Some(
                    json!({"time_s":time,"upward_forces_n":forces,"ready":ready,"clock":pending_clock}),
                );
                if pending_clock.timed_out {
                    *clock_state = pending_clock;
                    motion_gate_trace.push(pending_gate.take().unwrap());
                    return Err(format!(
                        "motion gate timed out at physics time {time}s, reference time {}s",
                        clock.reference_s(&clock_state)
                    ));
                }
            }
        }
        if let Some(p) = policy.as_mut() {
            let reference_time = motion_clock
                .as_ref()
                .map(|c| c.reference_at(&pending_clock, time))
                .transpose()?
                .unwrap_or(time);
            let reference = if let Some(traj) = trajectory {
                traj.sample(reference_time)?.values
            } else {
                config
                    .motors
                    .as_ref()
                    .unwrap()
                    .servos
                    .as_ref()
                    .unwrap()
                    .iter()
                    .map(|s| s.target_rad)
                    .collect()
            };
            p.sample(i, time, g, art, &reference, &map, reference_time,
                motion_clock.is_none() || pending_clock.advancing)?;
        }
        let result = if let Some(servos) = servo_bank.as_mut() {
            let inputs = config.motors.as_ref().unwrap().servos.as_ref().unwrap();
            let target_law = |t: f64| -> Result<Vec<f64>, String> {
                if let Some(p) = policy.as_ref() {
                    return Ok(p.targets.clone());
                }
                let reference = motion_clock
                    .as_ref()
                    .map(|c| c.reference_at(&pending_clock, t))
                    .transpose()?
                    .unwrap_or(t);
                match &trajectory {
                    Some(traj) => Ok(traj.sample(reference)?.values),
                    None => Ok(inputs.iter().map(|b| b.target_rad).collect()),
                }
            };
            let mut control = if trajectory.is_some() || policy.is_some() {
                servos.connect_target_law(
                    bank.as_ref().unwrap(),
                    driver_bank.as_ref().unwrap(),
                    inputs,
                    &target_law,
                )?
            } else {
                servos.connect(
                    bank.as_ref().unwrap(),
                    driver_bank.as_ref().unwrap(),
                    inputs,
                )?
            };
            bank.as_mut()
                .unwrap()
                .advance_with_control_cached(
                    &map,
                    &g,
                    &motor_states,
                    &*servo_states,
                    i as f64 * config.step_s,
                    config.step_s,
                    config.implicit.as_ref().unwrap(),
                    config.motors.as_ref().unwrap().events.as_ref().unwrap(),
                    &mut control,
                    workspace,
                    |_, _| Ok(applied_loads.clone()),
                )
                .map(|result| {
                    *servo_states = result.control_state;
                    let step = result.motor;
                    if let Some(all) = contact_steps.as_mut() {
                        all.extend(
                            step.contact_steps
                                .expect("enabled contact audit must be returned"),
                        );
                    }
                    hybrid_steps.push(step.hybrid);
                    hybrid_solves.push(step.solves);
                    *motor_states = step.motor_states;
                    step.endpoint
                })
        } else if let Some(events) = config.motors.as_ref().and_then(|m| m.events.as_ref()) {
            bank.as_mut()
                .unwrap()
                .advance_with_boundary_law(
                    &map,
                    &g,
                    &motor_states,
                    i as f64 * config.step_s,
                    config.step_s,
                    config.implicit.as_ref().unwrap(),
                    events,
                    |t, _, x| boundaries_at(t, x),
                    |_, _| Ok(applied_loads.clone()),
                )
                .map(|step| {
                    if let Some(all) = contact_steps.as_mut() {
                        all.extend(
                            step.contact_steps
                                .expect("enabled contact audit must be returned"),
                        );
                    }
                    hybrid_steps.push(step.hybrid);
                    hybrid_solves.push(step.solves);
                    *motor_states = step.motor_states;
                    step.endpoint
                })
        } else if let Some(implicit) = &config.implicit {
            let coupling = |t: f64, h: f64, g: &Generalized, x: &[f64]| {
                    let mut result = if let Some(bank) = &bank {
                        bank.evaluate(t, h, g, &motor_states, x, &boundaries_at(t, x)?)?
                            .0
                    } else {
                        sim_domain_robot::articulated::embedding::CoupledForces {
                            generalized_loads: vec![0.0; map.full_dimension()],
                            auxiliary_residuals: vec![],
                        }
                    };
                    if !effective_servos.is_empty() {
                        let targets=if let Some(p)=policy.as_ref() {p.targets.clone()}
                            else if let Some(traj)=trajectory.as_ref() {
                                let reference=motion_clock.as_ref()
                                    .map(|clock|clock.reference_at(&pending_clock,t)).transpose()?.unwrap_or(t);
                                traj.sample(reference)?.values
                            }
                            else {config.motors.as_ref().unwrap().servos.as_ref().unwrap().iter().map(|s|s.target_rad).collect()};
                        let base=map.full_dimension()-g.q.len();
                        for (i,servo) in effective_servos.iter().enumerate() {
                            let j=map.independent_joint_indices()[i];
                            result.generalized_loads[base+j]+=servo.torque(g.q[j],g.qd[j],targets[i]);
                        }
                    }
                    for (a, b) in result
                        .generalized_loads
                        .iter_mut()
                        .zip(&applied_loads)
                    {
                        *a += b;
                    }
                    if config.trace_trials > 0 {
                        let mut tail = trials.borrow_mut();
                        if tail.len() == config.trace_trials {
                            tail.pop_front();
                        }
                        tail.push_back(
                            json!({"time_s":t,"joint_positions":g.q,"joint_velocities":g.qd,
                            "motor_states":x,"motor_residuals":result.auxiliary_residuals,
                            "applied_generalized_loads":result.generalized_loads}),
                        );
                    }
                    Ok(result)
                };
            if let Some(refinement) = &config.mechanical_subdivision {
                // The opt-in mechanical adapter now owns a persistent numerical
                // workspace. Default to fresh derivatives at sampled commands;
                // the existing explicit reuse flag declares unchanged force-law
                // structure across those updates. Physical loads remain current.
                if policy_stride.is_some_and(|stride| i % stride == 0)
                    && !implicit.reuse_controller_sample_jacobian
                {
                    workspace.clear();
                }
                map.advance_implicit_mechanics_cached(
                    &g, time, config.step_s, implicit, refinement, workspace,
                    |t, g| coupling(t, 0.0, g, &[]).map(|f| f.generalized_loads),
                ).map(|step| {
                    for s in step.segments {
                        if let Some(first)=s.first_stage_diagnostics {solver_steps.push(first);}
                        solver_steps.push(s.diagnostics);
                    }
                    hybrid_steps.push(step.refinement);
                    step.endpoint
                })
            } else {
                map.step_implicit_coupled(
                    &g, &motor_states, time, config.step_s, implicit, coupling,
                ).map(|step| {
                    solver_steps.push(step.diagnostics);
                    *motor_states = step.auxiliary;
                    step.endpoint
                })
            }
        } else {
            map.step_midpoint(&g, i as f64 * config.step_s, config.step_s, |_, _| {
                Ok(applied_loads.clone())
            })
            .map(|step| step.endpoint)
        };
        *step_wall_s += start.elapsed().as_secs_f64();
        match result {
            Ok(step) => {
                *g = step.generalized;
                *clock_state = pending_clock;
                if let Some(sample) = pending_gate {
                    motion_gate_trace.push(sample);
                }
                *completed_steps += 1;
            }
            Err(e) => {
                return Err(format!("step {i}: {e}"));
            }
        }

        Ok(())
    }

    /// Physical frame at the current committed simulation time. Observations
    /// here are diagnostics, not a claim of deployed hardware sensor channels.
    pub fn frame(&self) -> Result<serde_json::Value, String> {
        let Self {
            effective_servos,
            session,
            config,
            g,
            motor_states,
            bank,
            driver_bank,
            servo_bank,
            servo_states,
            trajectory,
            motion_clock,
            clock_state,
            ..
        } = self;
        let art = &session.robot.art;
        let time_s = self.completed_steps as f64 * config.step_s;
        let bank = bank.as_ref();
        let servos = servo_bank.as_ref();
        let held = servo_states.as_slice();
        let boundaries_at = |time: f64, states: &[f64]| -> Result<Vec<MotorBoundary>, String> {
            match config.motors.as_ref() {
                Some(m) => {
                    if let Some(inputs) = &m.drivers {
                        driver_bank
                            .as_ref()
                            .ok_or("missing driver bank")?
                            .evaluate(time, states, inputs)
                            .map(|r| r.0)
                    } else {
                        Ok(m.boundaries
                            .as_ref()
                            .ok_or("servo boundaries require held control state")?
                            .clone())
                    }
                }
                None => Ok(vec![]),
            }
        };

        let eval = art.evaluate(g);
        let servo_commands = servos
            .map(|s| {
                s.commands(
                    time_s,
                    g,
                    held,
                    config.motors.as_ref().unwrap().servos.as_ref().unwrap(),
                )
            })
            .transpose()?;
        let driver_inputs = if let Some(commands) = &servo_commands {
            Some(
                config
                    .motors
                    .as_ref()
                    .unwrap()
                    .servos
                    .as_ref()
                    .unwrap()
                    .iter()
                    .zip(commands)
                    .map(|(input, duty)| DriverBoundary {
                        supply_voltage_v: input.supply_voltage_v,
                        duty: *duty,
                        winding_temperature_k: input.winding_temperature_k,
                    })
                    .collect::<Vec<_>>(),
            )
        } else {
            config.motors.as_ref().and_then(|m| m.drivers.clone())
        };
        let (boundaries, driver_readings) = if let Some(inputs) = driver_inputs {
            let (b, r) = driver_bank
                .as_ref()
                .unwrap()
                .evaluate(time_s, motor_states, &inputs)?;
            (b, Some(r))
        } else if !effective_servos.is_empty() {
            (vec![],None)
        } else {
            (boundaries_at(time_s, motor_states)?, None)
        };
        let readings = bank
            .map(|b| {
                b.evaluate(
                    time_s,
                    config.step_s,
                    g,
                    motor_states,
                    motor_states,
                    &boundaries,
                )
                .map(|r| r.1)
            })
            .transpose()?;
        let mut frame = json!({"driver_readings":driver_readings,"time_s":time_s,"joint_positions":g.q,"joint_velocities":g.qd,
            "motor_states":motor_states,"motor_readings":readings,
            "original_rows":art.original_closure(g),"contacts":eval.contacts.iter().map(|c|
                json!({"link":c.link,"other":c.other,"force_n":c.force.as_slice(),"point_m":c.point.as_slice(),"penetration_m":c.penetration})).collect::<Vec<_>>(),
            "contact_history":art.links.iter().filter(|l|!l.grounded).map(|l|json!({"link":l.name,
                "bristle_state":&g.states[l.bristle_state..l.bristle_state+3]})).collect::<Vec<_>>(),
            "poses":eval.links.iter().zip(&art.model.links).map(|(k,l)|json!({"name":l.name,"position_m":k.p.as_slice(),
                "velocity_m_s":k.vel.as_slice(),"angular_velocity_rad_s":k.w.as_slice(),
                "rotation":(0..3).map(|i|(0..3).map(|j|k.r[(i,j)]).collect::<Vec<_>>()).collect::<Vec<_>>()})).collect::<Vec<_>>()});
        if driver_bank.is_none() {
            frame.as_object_mut().unwrap().remove("driver_readings");
        }
        if let Some(loads) = &self.world_loads {
            let w = loads.wrench(self.completed_steps);
            frame["environment_load"] = json!({
                "base_link":config.world_loads.as_ref().unwrap().base_link,
                "force_world_n":&w[..3], "moment_world_nm":&w[3..],
                "sampling":"held over next nominal step; moment about base COM"
            });
        }
        if servo_commands.is_some() || !effective_servos.is_empty() {
            if let Some(commands)=servo_commands {frame["servo_commands"] = json!(commands);}
            if let Some(trajectory) = &trajectory {
                let reference = if let Some(clock) = &motion_clock {
                    if clock_state.samples == 0 {
                        0.0
                    } else {
                        clock.reference_at(clock_state, time_s)?
                    }
                } else {
                    time_s
                };
                frame["servo_targets_rad"] = json!(trajectory.sample(reference)?.values);
                if motion_clock.is_some() {
                    frame["reference_time_s"] = json!(reference);
                    frame["motion_clock"] = json!(clock_state);
                    frame["motion_progress"] = json!(motion_clock
                        .as_ref()
                        .unwrap()
                        .progress(clock_state, time_s)?);
                }
            }
            if effective_servos.is_empty() {frame["servo_states"] = json!(held);}
        }
        if let Some(p) = &self.policy {
            frame["policy"] = p.telemetry().clone();
            if let Some(reference) = frame.get("servo_targets_rad").cloned() {
                frame["reference_targets_rad"] = reference;
            }
            if let Some(reference)=p.telemetry().get("step_reference").and_then(|s|s.get("coordinates")) {
                frame["reference_targets_rad"]=reference.clone();
            }
            if frame.get("reference_targets_rad").is_none()
                && config.policy.as_ref().is_some_and(|p| p.step_reference.is_some()) {
                frame["reference_targets_rad"]=json!(config.motors.as_ref().unwrap().servos.as_ref().unwrap().iter().map(|s|s.target_rad).collect::<Vec<_>>());
            }
            frame["servo_targets_rad"] = json!(p.targets);
        }
        if !effective_servos.is_empty() {
            let targets:Vec<f64>=if let Some(p)=&self.policy {p.targets.clone()}
                else if trajectory.is_some() {serde_json::from_value(frame["servo_targets_rad"].clone()).map_err(|e|format!("effective targets: {e}"))?}
                else {config.motors.as_ref().unwrap().servos.as_ref().unwrap().iter().map(|s|s.target_rad).collect()};
            frame["servo_targets_rad"]=json!(targets);
            frame["motor_readings"]=json!(effective_servos.iter().enumerate().map(|(i,s)| {
                let j=self.independent_joint_indices[i];
                json!({"shaft_torque_nm":s.torque(g.q[j],g.qd[j],targets[i]),"gear_speed_rad_s":g.qd[j]})
            }).collect::<Vec<_>>());
            frame["actuator_profile"]=json!({"kind":"effective_servo","calibrated":false,
                "omitted":"winding/rotor dynamics, gearbox compliance/backlash and identified friction/efficiency, firmware, latency, quantization and heat"});
        }
        Ok(frame)
    }

    pub fn interactive_frame(&self) -> Result<serde_json::Value, String> {
        let mut f = self.frame()?;
        if f.get("servo_targets_rad").is_none() {
            if let Some(inputs) = self.config.motors.as_ref().and_then(|m| m.servos.as_ref()) {
                f["servo_targets_rad"] =
                    json!(inputs.iter().map(|s| s.target_rad).collect::<Vec<_>>());
            }
        }
        f["done"] = json!(self.done());
        f["error"] = json!(self.error);
        f["completed_steps"] = json!(self.completed_steps);
        f["requested_steps"] = json!(self.config.steps);
        f["stepping_wall_s"] = json!(self.step_wall_s);
        if let Some(p) = &self.policy {
            f["policy_inputs"] = json!(p.values());
        }
        Ok(f)
    }

    /// Retain the seeded input recipe, committed steps, and any failed attempt.
    /// Input changes are retained at their committed step boundaries.
    pub fn recording(&self) -> EmbeddedRecording {
        EmbeddedRecording {
            version: 3,
            kind: "embedded_session".into(),
            scene: self.session.scene.clone(),
            config: self.config.clone(),
            seed: self.seed,
            completed_steps: self.completed_steps,
            failure: self.error.clone(),
            input_events: self
                .input_events
                .iter()
                .filter(|e| e.at_step <= self.completed_steps)
                .cloned()
                .collect(),
        }
    }

    /// Validate and initialize a replay without blocking for its entire duration.
    /// The host advances the returned count in chunks through `advance`.
    pub fn prepare_replay(
        recording: EmbeddedRecording,
        capture: CaptureMode,
    ) -> Result<(Self, usize), String> {
        if ![1, 2, 3].contains(&recording.version)
            || recording.kind != "embedded_session"
            || recording.completed_steps > recording.config.steps
            || (recording.version == 1 && !recording.input_events.is_empty())
            || (recording.version < 3 && recording.failure.is_some())
            || recording.failure.as_ref().is_some_and(|e| e.is_empty())
            || recording
                .input_events
                .windows(2)
                .any(|w| w[1].at_step <= w[0].at_step)
            || recording
                .input_events
                .iter()
                .any(|e| e.at_step > recording.completed_steps)
        {
            return Err("invalid embedded recording version, kind or step count".into());
        }
        let steps = recording.completed_steps
            + usize::from(
                recording.failure.is_some() && recording.completed_steps < recording.config.steps,
            );
        let mut run = Self::new(recording.scene, recording.config, recording.seed, capture)?;
        if recording.version == 3 && (steps > 0 || recording.failure.is_some()) {
            run.replay_expected = Some((recording.completed_steps, recording.failure));
        }
        for event in &recording.input_events {
            run.policy
                .as_ref()
                .ok_or("recorded inputs require a policy")?
                .validate_inputs(&event.values)?;
        }
        run.input_events = recording.input_events.clone();
        run.replay_inputs = recording.input_events.into();
        run.apply_replay_inputs()?;
        Ok((run, steps))
    }

    /// Compatibility diagnostic for the headless example. Partial execution is
    /// never reported completed. Full history is unavailable in Latest mode.
    pub fn report(&self) -> Result<serde_json::Value, String> {
        if self.capture != CaptureMode::Full {
            return Err("full report requires Full capture mode".into());
        }
        let Self {
            session,
            config,
            names,
            error,
            step_wall_s,
            solver_steps,
            hybrid_steps,
            hybrid_solves,
            contact_steps,
            trials,
            motion_gate_trace,
            bank,
            driver_bank,
            servo_bank,
            motor_configs,
            driver_configs,
            servo_configs,
            frames,
            ..
        } = self;
        let completed_steps = self.completed_steps;
        let passed = error.is_none() && completed_steps == config.steps;
        let solver_profile = config.profile_solver.then(|| {
            sim_solve::profile::all()
                .iter()
                .map(|b| json!({"name":b.name,"seconds":b.seconds(),"calls":b.calls()}))
                .collect::<Vec<_>>()
        });
        let contact_impulses = contact_steps
            .as_ref()
            .filter(|_| completed_steps > 0)
            .map(|steps| {
                crate::contact_audit::embedded_contact_impulses(
                    0.0,
                    completed_steps as f64 * config.step_s,
                    steps,
                )
            })
            .transpose()?;
        let mut report = json!({"completed":passed,"error":error,"source":session.scene.robot.source,
        "scene_options":session.scene.options,"world":session.scene.robot.world,"embedding":config.embedding,"independent_coordinates":names,"step_s":config.step_s,"requested_steps":config.steps,
        "initial_coordinates":config.initial_coordinates,
        "initial_base_translation_m":config.initial_base_translation_m,
        "completed_steps":completed_steps,"simulated_s":completed_steps as f64*config.step_s,"stepping_wall_s":step_wall_s,
        "solver_profile":solver_profile,
        "motion_gate":config.motion_gate,"motion_gate_trace":motion_gate_trace,
        "implicit":config.implicit,"audit_contact_steps":config.audit_contact_steps,"contact_steps":contact_steps,"contact_impulses":contact_impulses,"solver_steps":solver_steps,"hybrid_steps":hybrid_steps,"hybrid_solves":hybrid_solves,
        "motor_state_layout":bank.as_ref().map(|b|b.state_layout()),
        "independent_joint_indices":self.independent_joint_indices,
        "trace_trials":config.trace_trials,"last_step_trials":trials.borrow().clone(),
        "motor_experiment":config.motors,"motor_components":motor_configs,"driver_components":driver_configs,"servo_components":servo_configs,"servo_state_layout":servo_bank.as_ref().map(|s|s.state_layout()),"control_guard_offset":servo_bank.as_ref().map(|_|bank.as_ref().unwrap().guard_count()),
        "applied_generalized_loads":config.applied_generalized_loads,"frames":frames,
        "terminal_frame":self.frame()?,
        "notes":["Motor components are integrated only when motor_experiment is present. Then they use the shared registered winding/rotor/gearbox equations with the recorded voltage and temperature boundaries.",
            if servo_bank.is_some() {"Registered servo firmware is ticked at declared deadlines with held outputs, internal quantization, saturation and sample-rounded latency. Supply and winding temperature remain imposed; no battery, thermal network or deployed external sensor model is added."} else if driver_bank.is_some() {"Driver inputs use the registered averaged H-bridge at each trial motor current. Supply voltage and winding temperature are imposed. No battery, thermal network, firmware or sampled sensors are integrated."} else {"No driver, battery, thermal-network, firmware or sampled sensor states are integrated. Zero motor voltage is a short-circuit boundary, not an open-circuit or servo-hold model."},
            "Declared external loads are additional to any motor outputs. This reduced diagnostic is not the detailed runtime's powered-hold comparison or a validated walking controller.",
            "The recorded implicit config selects backward Euler; null selects explicit midpoint. Both integrate shared rigid mechanics and contact memory; neither provides adaptive error or impact timing control. Explicit motor event configuration adds backlash guard/deadline processing and records retries.",
            "Stepping wall time includes closure and endpoint evaluation but excludes build, snapshot serialization and rendering."]});
        if self.policy.is_some() {
            report["policy_experiment"] = json!({"config": config.policy, "controller": session.scene.controller, "contract": self.policy_metadata(), "seed": self.seed, "input_events": self.recording().input_events});
        }
        Ok(report)
    }
}
