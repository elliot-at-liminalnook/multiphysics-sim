//! Qualified reduction recipes over the ordinary environment and experiment APIs.
//! No optimizer or alternate robot physics lives here.
use crate::{
    environment::{EmbeddedEnvironment, Transition},
    experiment::{Experiment, ExperimentSpec, Journal},
    fidelity::{
        self, ComparisonPlan, ComparisonReport, EnvironmentCapture, ExecutionContext, Provenance,
    },
    physics_context::RuntimeIdentity,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sim_domain_robot::motor::MotorDynamics;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub version: u32,
    pub name: String,
    /// Retain full motors or omit winding storage. Mechanical storage and
    /// controller events stay in either case; qualification decides eligibility.
    pub motor_dynamics: MotorDynamics,
    pub step_multiplier: usize,
    /// Use the existing local mechanism tangent for Newton probes, with exact
    /// closure/residual checks at accepted endpoints and exact-derivative retry.
    #[serde(default)]
    pub linearized_mechanism_probes: bool,
    /// Existing guarded temporal guess; exact residual checks and restart stay.
    #[serde(default)]
    pub predict_velocity_seed: bool,
    /// Guarded low-rank derivative updates in the shared Newton solver. Exact
    /// residual/correction acceptance and refresh limits are unchanged.
    #[serde(default)]
    pub broyden_updates: bool,
    /// Absolute matched-endpoint error budgets, keyed by canonical SI unit.
    pub absolute_tolerances: BTreeMap<String, f64>,
    pub minimum_speedup: f64,
    /// Task-level qualification instead of trajectory/electrical budgets: the
    /// reduced run only has to reach the same outcome (completion, falls,
    /// contacts) and a matching task score. For searches that rank gaits by
    /// distance over time and accept lower physical fidelity to run faster.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_level: Option<TaskLevel>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskLevel {
    /// |reduced − detailed| score must be within max(absolute, relative × |detailed|).
    pub relative_score_tolerance: f64,
    pub absolute_score_tolerance: f64,
}
impl Profile {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || self.name.trim().is_empty()
            || !matches!(
                self.motor_dynamics,
                MotorDynamics::Detailed | MotorDynamics::QuasistaticWinding
            )
            || (self.motor_dynamics.is_detailed()
                && self.step_multiplier == 1
                && !self.linearized_mechanism_probes
                && !self.predict_velocity_seed
                && !self.broyden_updates)
            || self.step_multiplier == 0
            || self.step_multiplier > 64
            || !self.minimum_speedup.is_finite()
            || self.minimum_speedup < 1.
            || self.absolute_tolerances.is_empty()
            || self
                .absolute_tolerances
                .values()
                .any(|v| !v.is_finite() || *v < 0.)
        {
            return Err("v1 exploration requires detailed or winding-only motors, an actual reduction, aligned 1..64 step multiplier, accuracy budgets and speedup >= 1".into());
        }
        if let Some(t) = &self.task_level {
            if [t.relative_score_tolerance, t.absolute_score_tolerance].iter().any(|v| !v.is_finite() || *v < 0.) {
                return Err("task-level score tolerances must be finite and nonnegative".into());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub version: u32,
    pub detailed: ExperimentSpec,
    pub profile: Profile,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Prepared {
    pub version: u32,
    pub recipe_id: String,
    pub runtime: RuntimeIdentity,
    pub detailed: ExperimentSpec,
    pub reduced: ExperimentSpec,
    pub changes: Vec<fidelity::DeclaredChange>,
}
fn digest(value: &impl Serialize) -> Result<String, String> {
    let bytes = crate::physics_context::fingerprint(
        &serde_json::to_value(value).map_err(|e| e.to_string())?,
    );
    Ok(blake3::hash(bytes.as_bytes()).to_hex().to_string())
}
fn context(spec: &ExperimentSpec) -> Result<ExecutionContext, String> {
    let variant =
        spec.parameterization
            .materialize(&spec.scene, &spec.source_actions, &spec.baseline)?;
    Ok(ExecutionContext {
        version: 1,
        scene: variant.scene,
        config: spec.config.clone(),
        task: spec.task.clone(),
        seed: spec.seed,
        runtime_identity: Some(RuntimeIdentity::current()),
    })
}
impl Recipe {
    /// Read-only preflight. Materializes references and validates declared command
    /// bounds, but does not advance physics, claim support feasibility or ask search.
    pub fn prepare(&self) -> Result<Prepared, String> {
        self.profile.validate()?;
        let s = &self.detailed;
        if self.version != 1 || s.version != 1 || !s.scene.options.motor_dynamics.is_detailed() {
            return Err("exploration needs a version-one detailed source experiment".into());
        }
        let motor = s
            .config
            .motors
            .as_ref()
            .ok_or("exploration requires explicit motor dynamics")?;
        if motor.effective.is_some() || s.scene.robot.motors.is_empty() {
            return Err(
                "exploration cannot use effective/ideal servos or an empty motor bank".into(),
            );
        }
        let factor = self.profile.step_multiplier;
        if s.config.steps == 0
            || s.config.report_every == 0
            || s.config.steps % factor != 0
            || s.config.report_every % factor != 0
            || !s.config.step_s.is_finite()
            || s.config.step_s <= 0.
        {
            return Err("reduced step must exactly divide the horizon and report clock".into());
        }
        let step = s.config.step_s * factor as f64;
        for period in [s.task.period_s, s.scene.period_s] {
            let ticks = period / step;
            if !ticks.is_finite() || ticks < 1. || (ticks - ticks.round()).abs() > 1e-9 {
                return Err("reduced step must divide task and controller-input clocks".into());
            }
        }
        let count = s.config.steps as f64 * s.config.step_s / s.task.period_s;
        if !count.is_finite()
            || count < 1.
            || (count - count.round()).abs() > 1e-9
            || s.source_actions.len() != count.round() as usize
        {
            return Err("complete explicit action schedule required".into());
        }
        let mut reduced = s.clone();
        reduced.scene.options.motor_dynamics = self.profile.motor_dynamics;
        reduced.config.step_s = step;
        reduced.config.steps /= factor;
        reduced.config.report_every /= factor;
        if self.profile.linearized_mechanism_probes {
            let solver = reduced
                .config
                .implicit
                .as_mut()
                .ok_or("mechanism probe reduction requires the implicit solver")?;
            solver.linearized_jacobian_probes = true;
            solver.reuse_mechanical_endpoint = true;
            solver.reuse_exact_probe_base = true;
        }
        if self.profile.predict_velocity_seed {
            let solver = reduced
                .config
                .implicit
                .as_mut()
                .ok_or("velocity prediction requires the implicit solver")?;
            solver.reuse_step_jacobian = true;
            solver.extrapolate_velocity_seed = true;
        }
        if self.profile.broyden_updates {
            let solver = reduced
                .config
                .implicit
                .as_mut()
                .ok_or("Broyden reduction requires the implicit solver")?;
            solver.newton.broyden_updates = true;
            // Condensed motor equations are small repeated inner solves. Keep
            // their original derivative policy while reducing outer geometry work.
            solver.auxiliary_broyden_updates = Some(false);
        }
        // No tolerances, clocks, limits, CAD fields or controller gains are edited.
        let a = context(s)?;
        let b = context(&reduced)?;
        let changes: Vec<_> = a
            .differences(&b)?
            .into_iter()
            .map(|difference| fidelity::DeclaredChange {
                difference,
                reason: "Explicit exploration motor-storage, timestep and mechanism-probe settings"
                    .into(),
            })
            .collect();
        if changes.is_empty() {
            return Err("profile makes no reduction relative to the source experiment".into());
        }
        Ok(Prepared {
            version: 1,
            recipe_id: digest(&(RuntimeIdentity::current(), self))?,
            runtime: RuntimeIdentity::current(),
            detailed: s.clone(),
            reduced,
            changes,
        })
    }
}

/// Incremental capture adapter around the same production environment. Hosts own
/// timing, persistence, progress and cancellation, one task interval at a time.
pub struct CaptureSession {
    env: EmbeddedEnvironment,
    actions: Vec<Vec<f64>>,
    frames: Vec<Value>,
    transitions: Vec<Transition>,
    error: Option<String>,
}
impl CaptureSession {
    pub fn new(spec: &ExperimentSpec) -> Result<Self, String> {
        let variant =
            spec.parameterization
                .materialize(&spec.scene, &spec.source_actions, &spec.baseline)?;
        let env = EmbeddedEnvironment::new(
            variant.scene,
            spec.config.clone(),
            spec.task.clone(),
            spec.seed,
        )?;
        if env.action_intervals() != variant.actions.len() {
            return Err("capture requires a complete action schedule".into());
        }
        Ok(Self {
            frames: vec![env.frame()?],
            transitions: vec![env.transition().clone()],
            env,
            actions: variant.actions,
            error: None,
        })
    }
    pub fn done(&self) -> bool {
        let t = self.env.transition();
        self.error.is_some() || t.terminated || t.truncated
    }
    pub fn advance(&mut self) -> Result<bool, String> {
        if self.done() {
            return Ok(false);
        }
        let action = self
            .actions
            .get(self.transitions.len() - 1)
            .ok_or("capture action schedule exhausted")?;
        match self.env.step(action) {
            Ok(t) => {
                self.transitions.push(t);
                self.frames.push(self.env.frame()?);
            }
            Err(e) => self.error = Some(e),
        }
        Ok(true)
    }
    pub fn time_s(&self) -> f64 {
        self.env.transition().time_s
    }
    /// A cancelled prefix stays incomplete and is ineligible for qualification.
    pub fn capture(&self, wall_s: f64) -> EnvironmentCapture {
        let recording = self.env.recording();
        EnvironmentCapture {
            version: 1,
            kind: "sampled_environment_capture".into(),
            completed: self.error.is_none() && recording.completed_steps == recording.config.steps,
            error: self.error.clone(),
            recording,
            task: self.env.task().clone(),
            metadata: self.env.metadata(),
            frames: self.frames.clone(),
            transitions: self.transitions.clone(),
            wall_s,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ElectricalError {
    pub unit: String,
    pub maximum_absolute: f64,
    pub rms: f64,
    pub samples: usize,
    pub absolute_tolerance: f64,
    pub within_tolerance: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Qualification {
    pub version: u32,
    pub recipe_id: String,
    pub report: ComparisonReport,
    pub electrical: BTreeMap<String, ElectricalError>,
    pub speedup: f64,
    pub qualified: bool,
    pub rejection_reasons: Vec<String>,
    pub capture_hashes: [String; 2],
}
fn electrical(frame: &Value) -> Result<BTreeMap<String, (String, f64)>, String> {
    let mut out = BTreeMap::new();
    for (key, fields) in [
        (
            "motor_readings",
            vec![
                ("current_a", "A"),
                ("shaft_torque_nm", "N·m"),
                ("gear_speed_rad_s", "rad/s"),
                ("heating_w", "W"),
            ],
        ),
        (
            "driver_readings",
            vec![
                ("motor_current_a", "A"),
                ("motor_voltage_v", "V"),
                ("supply_current_a", "A"),
                ("power_difference_w", "W"),
            ],
        ),
    ] {
        let rows = frame[key]
            .as_array()
            .ok_or_else(|| format!("missing {key} in qualification capture"))?;
        for (i, row) in rows.iter().enumerate() {
            for (field, unit) in &fields {
                let value = row[field]
                    .as_f64()
                    .filter(|v| v.is_finite())
                    .ok_or_else(|| format!("invalid {key}/{i}/{field}"))?;
                out.insert(format!("{key}/{i}/{field}"), (unit.to_string(), value));
            }
        }
    }
    if let Some(power) = frame.get("power") {
        let p: sim_domain_robot::articulated::embedding::EmbeddedPowerReading =
            serde_json::from_value(power.clone()).map_err(|e| e.to_string())?;
        for (name, unit, value) in [
            ("voltage", "V", p.voltage_v),
            ("current", "A", p.current_a),
            ("power", "W", p.power_w),
            ("soc", "1", p.state_of_charge),
            ("energy", "J", p.terminal_energy_j),
        ] {
            if !value.is_finite() {
                return Err("nonfinite power capture".into());
            }
            out.insert(format!("power/{name}"), (unit.into(), value));
        }
        for branch in p.branches {
            for (name, unit, value) in [
                ("voltage", "V", branch.voltage_v),
                ("current", "A", branch.current_a),
                ("loss", "W", branch.wiring_loss_w),
            ] {
                if !value.is_finite() {
                    return Err("nonfinite branch capture".into());
                }
                out.insert(
                    format!("power/branch/{}/{name}", branch.id),
                    (unit.into(), value),
                );
            }
        }
    }
    Ok(out)
}
pub fn qualify(
    recipe: &Recipe,
    a: &EnvironmentCapture,
    b: &EnvironmentCapture,
    reference: Provenance,
    candidate: Provenance,
) -> Result<Qualification, String> {
    let prepared = recipe.prepare()?;
    for (spec, capture) in [(&prepared.detailed, a), (&prepared.reduced, b)] {
        if !context(spec)?
            .differences(&ExecutionContext::new(&capture.recording, &capture.task))?
            .is_empty()
        {
            return Err("capture does not match the prepared exploration recipe/runtime".into());
        }
        // The base comparator matches the two captures to each other; also bind
        // their exact held actions to this recipe, not merely to each other.
        let v =
            spec.parameterization
                .materialize(&spec.scene, &spec.source_actions, &spec.baseline)?;
        validate_recorded_actions(capture, &v.actions)?;
    }
    let plan = ComparisonPlan {
        version: 1,
        reference,
        candidate,
        changes: prepared.changes,
        absolute_tolerances: recipe.profile.absolute_tolerances.clone(),
    };
    let report = fidelity::compare(a, b, &plan)?;
    let mut errors: BTreeMap<String, ElectricalError> = BTreeMap::new();
    for (af, bf) in a.frames.iter().zip(&b.frames) {
        for frame in [af, bf] {
            if frame["motor_readings"].as_array().map(Vec::len)
                != Some(prepared.detailed.scene.robot.motors.len())
            {
                return Err("qualification requires every motor reading".into());
            }
            if prepared
                .detailed
                .config
                .motors
                .as_ref()
                .is_some_and(|m| m.power.is_some())
                && frame.get("power").is_none()
            {
                return Err("missing authored power-system observations".into());
            }
        }
        let av = electrical(af)?;
        let bv = electrical(bf)?;
        if av.keys().ne(bv.keys()) {
            return Err("electrical channel topology differs".into());
        }
        for (name, (unit, x)) in av {
            let tolerance = *recipe
                .profile
                .absolute_tolerances
                .get(&unit)
                .ok_or_else(|| format!("missing electrical tolerance for {unit}"))?;
            let delta = (x - bv[&name].1).abs();
            if !delta.is_finite() {
                return Err("electrical comparison overflow".into());
            }
            let e = errors.entry(name).or_insert(ElectricalError {
                unit,
                maximum_absolute: 0.,
                rms: 0.,
                samples: 0,
                absolute_tolerance: tolerance,
                within_tolerance: true,
            });
            e.samples += 1;
            e.rms = (e.rms * ((e.samples - 1) as f64 / e.samples as f64).sqrt())
                .hypot(delta / (e.samples as f64).sqrt());
            e.maximum_absolute = e.maximum_absolute.max(delta);
            e.within_tolerance &= delta <= tolerance;
        }
    }
    let mut reasons = vec![];
    if report.reference.eligible_score.is_none() || report.candidate.eligible_score.is_none() {
        reasons.push("both full episodes must complete without failure/termination".into());
    }
    match &recipe.profile.task_level {
        Some(task) => {
            if !report.categorical_outcomes_match {
                reasons.push("task outcome mismatch".into());
            }
            if let (Some(a), Some(b)) = (report.reference.eligible_score, report.candidate.eligible_score) {
                if (a - b).abs() > task.absolute_score_tolerance.max(task.relative_score_tolerance * a.abs()) {
                    reasons.push(format!("task score {b} differs from detailed {a} beyond the task tolerance"));
                }
            }
        }
        None => {
            if !report.trajectory_within_tolerances || !report.categorical_outcomes_match {
                reasons.push("mechanical fidelity budget or outcome mismatch".into());
            }
            if errors.is_empty() || errors.values().any(|e| !e.within_tolerance) {
                reasons.push("electrical fidelity budget failed or no motor observations".into());
            }
        }
    }
    let speedup = a.wall_s / b.wall_s;
    if !speedup.is_finite() || speedup < recipe.profile.minimum_speedup {
        reasons.push("minimum measured speedup not achieved".into());
    }
    Ok(Qualification {
        version: 1,
        recipe_id: prepared.recipe_id,
        report,
        electrical: errors,
        speedup,
        qualified: reasons.is_empty(),
        rejection_reasons: reasons,
        capture_hashes: [digest(a)?, digest(b)?],
    })
}

fn validate_recorded_actions(
    capture: &EnvironmentCapture,
    actions: &[Vec<f64>],
) -> Result<(), String> {
    let channels = &capture
        .recording
        .scene
        .controller
        .as_ref()
        .ok_or("missing capture controller")?
        .inputs;
    let mut committed = capture.recording.clone();
    committed.failure = None;
    committed.completed_steps = capture
        .transitions
        .last()
        .ok_or("missing capture transition")?
        .completed_steps;
    committed
        .input_events
        .retain(|e| e.at_step < committed.completed_steps);
    let observed = if capture.frames.len() == 1 {
        vec![]
    } else {
        crate::forecast_actions::from_recording(&committed, &capture.frames, channels)?
    };
    if observed.len() > actions.len() || observed != actions[..observed.len()] {
        return Err("capture actions do not match the exploration recipe".into());
    }
    Ok(())
}

/// Recompute qualification from supplied captures before exporting a journal.
/// A receipt's boolean alone never unlocks exploration. The journal remains a
/// reduced-model result; finalists must return through `detailed_candidate`.
pub fn qualified_journal(
    recipe: &Recipe,
    a: &EnvironmentCapture,
    b: &EnvironmentCapture,
    reference: Provenance,
    candidate: Provenance,
) -> Result<Journal, String> {
    let q = qualify(recipe, a, b, reference, candidate)?;
    if !q.qualified {
        return Err(format!(
            "reduced profile not qualified: {}",
            q.rejection_reasons.join("; ")
        ));
    }
    Ok(Journal::new(Experiment::bind(recipe.prepare()?.reduced)?))
}
pub fn detailed_candidate(
    recipe: &Recipe,
    values: sim_domain_control::motion_parameters::Values,
) -> Result<ExperimentSpec, String> {
    let mut spec = recipe.prepare()?.detailed;
    spec.parameterization
        .materialize(&spec.scene, &spec.source_actions, &values)?;
    spec.baseline = values;
    Ok(spec)
}

pub fn register(registry: &mut sim_core::BehaviorRegistry) -> Result<(), String> {
    use sim_core::primitive::{Descriptor, Field};
    registry.register_primitive(
        Descriptor::new(
            "experiment.prepare_reduced",
            "Prepare an explicit reduced exploration pair without executing it",
            vec![Field::structured(
                "$",
                "SI; original CAD and controller definitions retained",
                "exploration::Recipe",
            )],
            vec![Field::structured(
                "$",
                "SI; exact changes and source identity",
                "exploration::Prepared",
            )],
            &[
                "No search or physics advances",
                "No fidelity qualification until complete matched captures pass explicit budgets",
                "Motor and contact limits remain in the production environment",
            ],
        ),
        |r: Recipe| r.prepare(),
    )
}
