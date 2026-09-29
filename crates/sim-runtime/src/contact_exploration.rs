//! Offline contact-motion preparation for ordinary environment experiments.
//! Robot topology remains in the CAD scene and marker/planner configuration.
use crate::{
    contact_reference, experiment::ExperimentSpec, motion_parameters::MotionParameterization,
    session::Scene, tracking::CaptureConfig,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sim_domain_control::{
    motion_parameters::{ParameterSpace, Values},
    motion_primitives::ContactTemplate,
    trajectory::{Trajectory, TrajectoryConfig},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub version: u32,
    pub experiment: ExperimentSpec,
    pub planning_scene: Scene,
    pub markers: CaptureConfig,
    pub compiler: contact_reference::Recipe,
    pub template: ContactTemplate,
    /// Explicit operational screen, in independent-coordinate order. These
    /// bounds screen desired reference speed, not achieved speed or torque.
    pub maximum_reference_speed_rad_s: Vec<f64>,
    pub speed_limit_provenance: String,
    /// Which existing speed command follows the compiled nominal speed.
    pub forward_command: String,
    /// Search parameters that set controller policy values (JSON pointers into
    /// the runtime controller parameters), e.g. reference-governor limits.
    /// Their parameters are declared in the template space but not used by it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub policy_bindings: Vec<PolicyBinding>,
    /// Reject on reference speed from a coarse compile first (1/`factor` of the
    /// samples) when it already exceeds the limit by `margin`. A coarse compile
    /// that fails is ignored: only the full compile can reject on kinematics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coarse_speed_screen: Option<CoarseScreen>,
    /// Derive every motor-dependent value (speed screen, planner actuator
    /// model, governor search ranges) from the accepted actuator registry at
    /// load, instead of carrying copies. See [`Recipe::sync_actuators`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actuator_limits: Option<ActuatorLimitPolicy>,
    /// Reject gaits the physical leg cannot track, before any physics:
    /// measured leg tracking error ≈ effective delay × commanded speed (the
    /// gait through its own reference governor, as the simulation commands
    /// it). See [`LegTrackingScreen`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leg_tracking_screen: Option<LegTrackingScreen>,
}
/// Physical-leg tracking model from measured gait runs: per joint role
/// (joint-name suffix), the effective delay (s) such that tracking error ≈
/// delay × governed reference speed, in RMS and at the peak.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegTrackingScreen {
    /// Role suffix → (RMS delay s, peak delay s).
    pub delays_s: std::collections::BTreeMap<String, (f64, f64)>,
    pub maximum_rms_rad: f64,
    pub maximum_peak_rad: f64,
    /// Measured run(s) the delays came from.
    pub evidence: String,
}
impl LegTrackingScreen {
    /// Predicted per-joint (RMS, peak) leg tracking error for a compiled gait
    /// under `governor` (the candidate's own reference governor). Joints
    /// without a role delay are not predicted.
    pub fn predict(&self, compiled: &Value, governor: &Value) -> Result<Vec<(String, f64, f64)>, String> {
        let mut c = compiled.clone();
        c["playback_governor"] = governor.clone();
        let gait = crate::gait_playback::Gait::from_compiled(&c, "screen")?;
        let period = gait.info.period_s;
        let mut g = crate::gait_playback::GovernedGait::new(gait);
        let dt = 0.01;
        let n = (period / dt).ceil() as usize;
        let joints = g.gait.info.joints.clone();
        let (mut sum, mut peak) = (vec![0.; joints.len()], vec![0f64; joints.len()]);
        // One settling period, then one measured period.
        for k in 0..2 * n {
            let out = g.step(k as f64 * dt, dt, 1.)?;
            if k >= n {
                for (i, (_, v)) in out.iter().enumerate() {
                    sum[i] += v * v;
                    peak[i] = peak[i].max(v.abs());
                }
            }
        }
        Ok(joints.iter().enumerate().filter_map(|(i, j)| {
            let (rms_delay, peak_delay) = self.delays_s.iter().find(|(role, _)| j.ends_with(role.as_str()))?.1;
            Some((j.clone(), rms_delay * (sum[i] / n as f64).sqrt(), peak_delay * peak[i]))
        }).collect())
    }
}
/// How motor limits become search limits. Fractions apply to the limits
/// derived from each joint's resolved profile at the simulated supply.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActuatorLimitPolicy {
    /// Accepted actuator registry (path relative to the repository root).
    pub registry: String,
    /// Reference-speed screen = this × full-drive speed, per joint.
    pub screen_speed_fraction: f64,
    /// Governor speed search upper bound = this × the slowest joint's full-drive speed.
    pub governor_speed_fraction: f64,
    /// Governor acceleration search upper bound = this × the lowest measured
    /// acceleration (joints without a measured envelope do not constrain it).
    pub governor_acceleration_fraction: f64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyBinding {
    pub parameter: String,
    /// JSON pointer into `scene.controller.parameters`.
    pub pointer: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoarseScreen {
    pub factor: usize,
    pub margin: f64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub recipe: Recipe,
    pub values: Values,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Prepared {
    pub spec: ExperimentSpec,
    pub compiled: Value,
    pub values: Values,
    pub screen: Value,
}

impl Recipe {
    /// Apply the accepted actuator registry to both robot copies and derive
    /// the speed screen, planner actuator model and governor search bounds
    /// from the resolved profiles. Returns the provenance record (registry
    /// and family hashes, per-coordinate limits). Idempotent.
    pub fn sync_actuators(&mut self, root: &std::path::Path) -> Result<Value, String> {
        let Some(policy) = self.actuator_limits.clone() else { return Ok(Value::Null) };
        for (name, f) in [("screen", policy.screen_speed_fraction), ("governor speed", policy.governor_speed_fraction), ("governor acceleration", policy.governor_acceleration_fraction)] {
            if !(f > 0. && f <= 1.5) {
                return Err(format!("{name} fraction must be in (0, 1.5]"));
            }
        }
        let registry = crate::actuator_registry::Registry::load(&root.join(&policy.registry))?;
        registry.apply(&mut self.experiment.scene.robot)?;
        registry.apply(&mut self.planning_scene.robot)?;
        let limits = crate::actuator_registry::joint_limits(&self.experiment.scene.robot)?;
        let coordinates = self.compiler.robot.independent_coordinates.clone();
        let mut rows = Vec::new();
        let mut speeds = Vec::new();
        for c in &coordinates {
            let joint = c.strip_prefix("joint.").unwrap_or(c);
            let l = limits.get(joint).ok_or(format!("coordinate {c} has no profiled motor"))?;
            speeds.push(policy.screen_speed_fraction * l.full_drive_speed_rad_s);
            let actuator = self.compiler.robot.actuators.entry(c.clone()).or_default();
            actuator.insert("no_load_speed".into(), l.no_load_speed_rad_s);
            actuator.insert("stall_torque".into(), l.stall_torque_nm);
            rows.push(serde_json::to_value(l).unwrap());
        }
        self.maximum_reference_speed_rad_s = speeds;
        let slowest = limits.values().map(|l| l.full_drive_speed_rad_s).fold(f64::INFINITY, f64::min);
        let lowest_acceleration = limits.values().filter_map(|l| l.measured_acceleration_rad_s2).fold(f64::INFINITY, f64::min);
        let mut governor = json!({});
        for p in &mut self.template.space.parameters {
            let upper = match p.name.as_str() {
                "governor_speed_rad_s" => policy.governor_speed_fraction * slowest,
                "governor_acceleration_rad_s2" if lowest_acceleration.is_finite() => policy.governor_acceleration_fraction * lowest_acceleration,
                _ => continue,
            };
            if upper <= p.bounds[0] {
                return Err(format!("{}: derived upper bound {upper} is not above the search floor {}", p.name, p.bounds[0]));
            }
            p.bounds[1] = upper;
            governor[p.name.clone()] = json!(p.bounds);
        }
        let identity = registry.identity();
        self.speed_limit_provenance = format!(
            "Derived at load from the accepted actuator registry {} (blake3 {}): screen = {} × each joint's full-drive speed from its resolved profile at the simulated supply; governor speed ≤ {} × slowest joint, acceleration ≤ {} × lowest measured envelope. Families: {}.",
            policy.registry, registry.registry_hash, policy.screen_speed_fraction, policy.governor_speed_fraction, policy.governor_acceleration_fraction,
            registry.families.iter().map(|(n, f)| format!("{n} {}", &f.content_hash()[..12])).collect::<Vec<_>>().join(", ")
        );
        Ok(json!({"version": 1, "registry": identity, "policy": serde_json::to_value(&policy).unwrap(),
                  "coordinates": rows, "maximum_reference_speed_rad_s": self.maximum_reference_speed_rad_s, "governor_bounds": governor}))
    }
    /// Cheap, kinematics-free screen: the parameters materialize a contact
    /// schedule with an all-stance pause the stopping controller can use.
    /// Microseconds, versus seconds for `prepare`; the same check also runs
    /// first inside the full compile.
    pub fn schedule_screen(&self, values: &Values) -> Result<(), String> {
        let motion = self.template.materialize(values)?;
        contact_reference::pause_windows(&motion, self.compiler.minimum_pause_window_s)?;
        Ok(())
    }
    /// Coarse reference-speed estimate; `Some(reason)` rejects the candidate.
    fn coarse_speed_rejection(&self, motion: &sim_domain_control::contact_phase::ContactPhaseConfig) -> Option<String> {
        let c = self.coarse_speed_screen.as_ref()?;
        let samples = self.compiler.robot.uniform_samples / c.factor.max(1);
        if samples < 64 || samples >= self.compiler.robot.uniform_samples {
            return None;
        }
        let mut compiler = self.compiler.clone();
        compiler.motion = motion.clone();
        compiler.robot.uniform_samples = samples;
        compiler.maximum_reference_errors = [1e9; 3];
        let compiled = contact_reference::compile(self.planning_scene.clone(), self.markers.clone(), compiler).ok()?;
        let curve: TrajectoryConfig = serde_json::from_value(compiled["trajectory"].clone()).ok()?;
        let rates = Trajectory::new(curve).ok()?.maximum_absolute_rates().ok()?;
        let coordinates = &self.compiler.robot.independent_coordinates;
        rates.iter().zip(&self.maximum_reference_speed_rad_s).enumerate().find_map(|(i, (rate, limit))| {
            (*rate > limit * c.margin).then(|| format!(
                "coarse reference actuator-speed screen ({samples} samples): {} requests {rate} rad/s, limit {limit} × margin {}",
                coordinates[i], c.margin
            ))
        })
    }
    pub fn prepare(&self, values: &Values) -> Result<Prepared, String> {
        if self.version != 1 || self.speed_limit_provenance.trim().is_empty() {
            return Err(
                "version-one contact exploration and explicit screening provenance required".into(),
            );
        }
        let coordinates = &self.compiler.robot.independent_coordinates;
        let motor = self
            .experiment
            .config
            .motors
            .as_ref()
            .ok_or("explicit motor bank required")?;
        if motor.target_coordinates.as_ref() != Some(coordinates)
            || motor.effective.is_some()
            || self.planning_scene.robot.source["cad_sha256"]
                != self.experiment.scene.robot.source["cad_sha256"]
        {
            return Err(
                "contact compiler/runtime CAD and coordinate order must match detailed motors"
                    .into(),
            );
        }
        if self.maximum_reference_speed_rad_s.len() != coordinates.len()
            || self
                .maximum_reference_speed_rad_s
                .iter()
                .any(|x| !x.is_finite() || *x <= 0.)
        {
            return Err("positive finite speed limits for every coordinate required".into());
        }
        let motion = self.template.materialize(values)?;
        contact_reference::pause_windows(&motion, self.compiler.minimum_pause_window_s)?;
        if let Some(reason) = self.coarse_speed_rejection(&motion) {
            return Err(reason);
        }
        let mut compiler = self.compiler.clone();
        compiler.motion = motion;
        // Preserve reference audit policy/tolerances. Any inverse-load diagnostic
        // failure remains visible; it cannot become a dynamic acceptance result.
        let compiled = contact_reference::compile(
            self.planning_scene.clone(),
            self.markers.clone(),
            compiler,
        )?;
        let curve: TrajectoryConfig =
            serde_json::from_value(compiled["trajectory"].clone()).map_err(|e| e.to_string())?;
        let trajectory = Trajectory::new(curve)?;
        let variant = self.experiment.parameterization.materialize(
            &self.experiment.scene,
            &self.experiment.source_actions,
            &self.experiment.baseline,
        )?;
        let mut spec = self.experiment.clone();
        spec.scene = variant.scene;
        spec.source_actions = variant.actions;
        let controller = spec
            .scene
            .controller
            .as_mut()
            .ok_or("missing runtime controller")?;
        let params = &mut controller.parameters;
        let indices = params["motor_indices"]
            .as_object()
            .ok_or("missing motor index map")?;
        let bounds = indices
            .iter()
            .map(|(name, index)| {
                let index = index.as_u64().ok_or("invalid motor index")? as usize;
                let bound: &Value = &params["output_bounds"][name];
                Ok((
                    index,
                    (
                        Some(bound[0].as_f64().ok_or("missing lower command bound")?),
                        Some(bound[1].as_f64().ok_or("missing upper command bound")?),
                    ),
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut ordered = vec![(None, None); coordinates.len()];
        let mut seen = std::collections::BTreeSet::new();
        for (i, b) in bounds {
            if i >= ordered.len() || !seen.insert(i) {
                return Err("invalid motor ordering".into());
            }
            ordered[i] = b;
        }
        if seen.len() != ordered.len() {
            return Err("incomplete motor bounds".into());
        }
        trajectory.validate_value_bounds(&ordered)?;
        let rates = trajectory.maximum_absolute_rates()?;
        for (i, (rate, limit)) in rates
            .iter()
            .zip(&self.maximum_reference_speed_rad_s)
            .enumerate()
        {
            if rate > limit {
                return Err(format!(
                    "reference actuator-speed screen: {} requests {rate} rad/s, limit {limit}",
                    coordinates[i]
                ));
            }
        }
        let old_speed = params["nominal_speed_m_s"]
            .as_f64()
            .filter(|v| v.is_finite() && *v > 0.)
            .ok_or("missing positive source speed")?;
        let speed = compiled["nominal_speed_m_s"]
            .as_f64()
            .filter(|v| v.is_finite() && *v > 0.)
            .ok_or("missing positive compiled speed")?;
        let input = controller
            .inputs
            .iter()
            .position(|i| i.name == self.forward_command)
            .ok_or("missing forward speed input")?;
        for row in &mut spec.source_actions {
            row[input] *= speed / old_speed;
            crate::forecast_actions::validate_values(&controller.inputs, row)?;
        }
        for key in [
            "motion",
            "trajectory",
            "static_feedforward",
            "dynamic_feedforward",
            "velocity_feedforward",
            "phase_offset_s",
            "pause_windows_s",
            "initial_phase_s",
            "nominal_speed_m_s",
        ] {
            if params.get(key).is_none() {
                return Err(format!("controller contract missing {key}"));
            }
            params[key] = compiled[key].clone();
        }
        params["period_s"] = compiled["motion"]["period_s"].clone();
        for b in &self.policy_bindings {
            let value = *values.get(&b.parameter).ok_or_else(|| format!("missing policy parameter {}", b.parameter))?;
            let slot = params.pointer_mut(&b.pointer).ok_or_else(|| format!("controller has no policy value at {}", b.pointer))?;
            if !slot.is_number() {
                return Err(format!("policy value at {} is not numeric", b.pointer));
            }
            *slot = json!(value);
        }
        params["reference_load_audit"] = json!({"required_load_audits_passed":compiled["required_load_audits_passed"],"nominal":compiled["nominal_physical_summary"],"reverse":compiled["reverse_load_audit"],"scope":"Offline reference diagnostic only; runtime acceptance is separate"});
        spec.config.initial_coordinates = Some(
            serde_json::from_value(compiled["initial_coordinates"].clone())
                .map_err(|e| e.to_string())?,
        );
        spec.config.initial_base_translation_m = Some(
            serde_json::from_value(compiled["initial_base_translation_m"].clone())
                .map_err(|e| e.to_string())?,
        );
        spec.config.initial_base_rotation_vector_rad = Some(
            serde_json::from_value(compiled["initial_base_rotation_vector_rad"].clone())
                .map_err(|e| e.to_string())?,
        );
        let initial = spec.config.initial_coordinates.as_ref().unwrap();
        let servos = spec
            .config
            .motors
            .as_mut()
            .unwrap()
            .servos
            .as_mut()
            .ok_or("contact exploration requires explicit servo initial targets")?;
        if servos.len() != initial.len() {
            return Err("initial servo target ordering mismatch".into());
        }
        for (servo, angle) in servos.iter_mut().zip(initial) {
            servo.target_rad = *angle;
        }
        // The immutable generated spec contains the actual policy. Search-space
        // binding remains in this recipe; stale scalar transforms cannot apply twice.
        spec.parameterization = MotionParameterization {
            version: 1,
            space: ParameterSpace { parameters: vec![] },
            trajectories: vec![],
            commands: vec![],
            scalars: vec![],
            checks: vec![],
        };
        spec.baseline = Values::new();
        let mut screen = json!({"passed":true,"reference_maximum_speed_rad_s":rates,
            "speed_limit_provenance":self.speed_limit_provenance,"scope":"CAD inverse-kinematic closure, compiler interpolation/pause checks, full-curve command bounds and reference speed limits only. No dynamic stability, continuous collision, achieved tracking or loaded motor accuracy is certified."});
        if let Some(leg) = &self.leg_tracking_screen {
            let governor = spec.scene.controller.as_ref().map(|c| c.parameters["reference_governor"].clone()).unwrap_or(Value::Null);
            let predicted = leg.predict(&compiled, &governor)?;
            if let Some((j, rms, peak)) = predicted.iter().find(|(_, rms, peak)| *rms > leg.maximum_rms_rad || *peak > leg.maximum_peak_rad) {
                return Err(format!("physical-leg tracking screen: {j} predicted {:.1}° RMS / {:.1}° peak on the leg (limits {:.1}° / {:.1}°)",
                    rms.to_degrees(), peak.to_degrees(), leg.maximum_rms_rad.to_degrees(), leg.maximum_peak_rad.to_degrees()));
            }
            screen["leg_tracking_prediction_rad"] = json!(predicted.iter().map(|(j, r, p)| json!({"joint": j, "rms": r, "peak": p})).collect::<Vec<_>>());
            screen["leg_tracking_evidence"] = json!(leg.evidence);
        }
        Ok(Prepared {
            spec,
            values: values.clone(),
            screen,
            compiled,
        })
    }
}
pub fn register(registry: &mut sim_core::BehaviorRegistry) -> Result<(), String> {
    use sim_core::primitive::{Descriptor, Field};
    registry.register_primitive(
        Descriptor::new(
            "experiment.prepare_contact_motion",
            "Compile and screen named contact motion before a dynamic trial",
            vec![Field::structured(
                "$",
                "SI; explicit parameter units",
                "contact_exploration::Request",
            )],
            vec![Field::structured(
                "$",
                "SI",
                "contact_exploration::Prepared",
            )],
            &[
                "Uses shared CAD kinematics and trajectory compiler; no simulation step",
                "Screening does not certify dynamic stability or hardware accuracy",
            ],
        ),
        |r: Request| r.recipe.prepare(&r.values),
    )
}
