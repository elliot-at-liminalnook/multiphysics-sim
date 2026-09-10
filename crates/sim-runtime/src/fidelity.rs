//! Matched-input comparisons of production environment captures. This does not
//! integrate physics, infer acceptable error, or certify a model against hardware.
use crate::{
    embedded::EmbeddedRecording,
    environment::{Task, Transition},
    motion_data::MotionSnapshot,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// Complete parsed episode settings, including robot, world, controller, solver,
/// reductions, initial conditions, task and seed. This is a resolved serialized
/// recipe, not the original CAD document or a claim about omitted CAD properties.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionContext {
    pub version: u32,
    pub scene: crate::session::Scene,
    pub config: crate::embedded::Config,
    pub task: Task,
    pub seed: u64,
    #[serde(default, skip_serializing_if="Option::is_none")]
    pub runtime_identity: Option<crate::physics_context::RuntimeIdentity>,
}
impl ExecutionContext {
    pub fn new(record: &EmbeddedRecording, task: &Task) -> Self {
        Self {
            version: 1,
            scene: record.scene.clone(),
            config: record.config.clone(),
            task: task.clone(),
            seed: record.seed,
            runtime_identity: record.runtime_identity.clone(),
        }
    }
    pub fn differences(&self, candidate: &Self) -> Result<Vec<ContextDifference>, String> {
        if self.version != 1 || candidate.version != 1 {
            return Err("unsupported execution context version".into());
        }
        for context in [self,candidate] {if let Some(identity)=&context.runtime_identity {identity.validate()?;}}
        let mut out = vec![];
        differences("", Some(&json!(self)), Some(&json!(candidate)), &mut out);
        Ok(out)
    }
}

/// Presence is explicit so serialization distinguishes an absent field from null.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "presence",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ContextValue {
    Missing,
    Present(Value),
}
impl From<Option<&Value>> for ContextValue {
    fn from(v: Option<&Value>) -> Self {
        match v {
            Some(v) => Self::Present(v.clone()),
            None => Self::Missing,
        }
    }
}
/// Exact JSON pointer and before/after values. Arrays retain order and are never
/// treated as sets.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextDifference {
    pub path: String,
    pub reference: ContextValue,
    pub candidate: ContextValue,
}
fn escape(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}
fn differences(path: &str, a: Option<&Value>, b: Option<&Value>, out: &mut Vec<ContextDifference>) {
    if a == b {
        return;
    }
    match (a, b) {
        (Some(Value::Object(a)), Some(Value::Object(b))) => {
            for k in a.keys().chain(b.keys()).collect::<BTreeSet<_>>() {
                differences(&format!("{path}/{}", escape(k)), a.get(k), b.get(k), out);
            }
        }
        (Some(Value::Array(a)), Some(Value::Array(b))) if a.len() == b.len() => {
            for (i, (a, b)) in a.iter().zip(b).enumerate() {
                differences(&format!("{path}/{i}"), Some(a), Some(b), out);
            }
        }
        _ => out.push(ContextDifference {
            path: path.into(),
            reference: a.into(),
            candidate: b.into(),
        }),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredChange {
    #[serde(flatten)]
    pub difference: ContextDifference,
    pub reason: String,
}

/// Host-supplied durable references. The comparator preserves these attestations;
/// it does not authenticate binaries, hardware, timestamps or capture contents.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub capture_reference: String,
    pub source_reference: String,
    pub executable_reference: String,
    pub host_reference: String,
    pub timing_scope: String,
}
impl Provenance {
    fn validate(&self) -> Result<(), String> {
        if [
            &self.capture_reference,
            &self.source_reference,
            &self.executable_reference,
            &self.host_reference,
            &self.timing_scope,
        ]
        .iter()
        .any(|s| s.trim().is_empty())
        {
            return Err("fidelity provenance requires explicit capture/source/executable/host/timing references".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComparisonPlan {
    pub version: u32,
    pub reference: Provenance,
    pub candidate: Provenance,
    /// Every differing resolved setting must match one exact declared edit.
    /// Unused declarations also fail; no wildcard or implicit timing exclusions.
    pub changes: Vec<DeclaredChange>,
    /// Absolute error limits in declared SI units, chosen by the experiment.
    /// Every measured unit needs a limit. No default physical accuracy budget.
    pub absolute_tolerances: BTreeMap<String, f64>,
}

/// Native run_environment capture format. Extra diagnostic fields are retained
/// by the caller's original artifact; only fields used here are deserialized.
#[derive(Clone, Serialize, Deserialize)]
pub struct EnvironmentCapture {
    pub version: u32,
    pub kind: String,
    pub completed: bool,
    pub error: Option<String>,
    pub recording: EmbeddedRecording,
    pub task: Task,
    pub metadata: Value,
    pub frames: Vec<Value>,
    pub transitions: Vec<Transition>,
    pub wall_s: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChannelError {
    pub unit: String,
    pub samples: usize,
    pub maximum_absolute: f64,
    pub rms: f64,
    pub worst_time_s: f64,
    pub absolute_tolerance: f64,
    pub within_tolerance: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Outcome {
    pub completed: bool,
    pub error: Option<String>,
    pub runtime_failure: Option<String>,
    pub final_transition: Transition,
    /// Undiscounted task reward only for complete nonfailed episodes.
    pub eligible_score: Option<f64>,
    pub eligible_reward_per_s: Option<f64>,
    pub wall_s: f64,
    /// Last captured task endpoint; a failed action can commit physics beyond it.
    pub observed_s: f64,
    pub unobserved_committed_steps: usize,
    pub simulated_s: f64,
    pub simulated_s_per_wall_s: f64,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ComparisonReport {
    pub version: u32,
    pub plan: ComparisonPlan,
    pub reference_context: ExecutionContext,
    pub candidate_context: ExecutionContext,
    pub matched_input_duration_s: f64,
    pub compared_frames: usize,
    pub channels: BTreeMap<String, ChannelError>,
    pub trajectory_within_tolerances: bool,
    pub categorical_outcomes_match: bool,
    pub reference: Outcome,
    pub candidate: Outcome,
    pub scope: String,
}

fn close_time(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-10 * a.abs().max(b.abs()).max(1.)
}

impl EnvironmentCapture {
    fn validate(&self) -> Result<(Vec<MotionSnapshot>, Vec<Vec<f64>>, Outcome), String> {
        let r = &self.recording;
        if self.version != 1
            || self.kind != "sampled_environment_capture"
            || self.frames.is_empty()
            || self.frames.len() != self.transitions.len()
            || !self.wall_s.is_finite()
            || self.wall_s <= 0.
            || !self.task.period_s.is_finite()
            || self.task.period_s <= 0.
            || r.version != 3
            || r.kind != "embedded_session"
            || !r.config.step_s.is_finite()
            || r.config.step_s <= 0.
        {
            return Err("invalid fidelity environment capture".into());
        }
        let motion = self
            .frames
            .iter()
            .map(MotionSnapshot::from_frame)
            .collect::<Result<Vec<_>, _>>()?;
        let last = self.transitions.last().unwrap();
        for (i, (m, t)) in motion.iter().zip(&self.transitions).enumerate() {
            if !close_time(m.time_s, i as f64 * self.task.period_s)
                || !close_time(m.time_s, t.time_s)
                || !close_time(m.time_s, t.completed_steps as f64 * r.config.step_s)
                || self.frames[i]["completed_steps"].as_u64() != Some(t.completed_steps as u64)
                || !t.reward.is_finite()
                || t.observations.iter().any(|v| !v.is_finite())
                || t.reward_terms.iter().any(|v| !v.value.is_finite())
                || (i + 1 < motion.len() && (t.terminated || t.truncated))
            {
                return Err(
                    "fidelity requires aligned committed task frames from time zero".into(),
                );
            }
        }
        let failed = self.error.is_some() || r.failure.is_some();
        if last.completed_steps > r.completed_steps
            || (last.completed_steps != r.completed_steps && !failed)
            || (r.completed_steps - last.completed_steps) as f64 * r.config.step_s
                > self.task.period_s + 1e-10
            || r.completed_steps > r.config.steps
            || self.completed != (self.error.is_none() && r.completed_steps == r.config.steps)
            || last.truncated != (last.completed_steps == r.config.steps)
        {
            return Err(
                "fidelity capture completion metadata disagrees with recorded horizon".into(),
            );
        }
        // Keep failure in the outcome. The existing shared command reconstruction
        // is applied only to committed intervals preceding that failure.
        let mut committed = r.clone();
        committed.failure = None;
        let channels = &r
            .scene
            .controller
            .as_ref()
            .ok_or("fidelity requires declared controller inputs")?
            .inputs;
        for (i, e) in r.input_events.iter().enumerate() {
            crate::forecast_actions::validate_values(channels, &e.values)?;
            if e.at_step > r.completed_steps
                || (e.at_step == r.completed_steps && r.failure.is_none() && self.error.is_none())
                || (i > 0 && r.input_events[i - 1].at_step >= e.at_step)
            {
                return Err(
                    "invalid fidelity action schedule or uncommitted event without failure".into(),
                );
            }
        }
        committed.completed_steps = last.completed_steps;
        committed
            .input_events
            .retain(|e| e.at_step < committed.completed_steps);
        crate::forecast_actions::validate(channels)?;
        let actions = if self.frames.len() == 1 {
            vec![]
        } else {
            crate::forecast_actions::from_recording(&committed, &self.frames, channels)?
        };
        let score: f64 = self.transitions.iter().skip(1).map(|t| t.reward).sum();
        if !score.is_finite() {
            return Err("nonfinite fidelity task score".into());
        }
        let eligible = self.completed
            && self.error.is_none()
            && r.failure.is_none()
            && !last.terminated
            && last.truncated;
        let outcome = Outcome {
            completed: self.completed,
            error: self.error.clone(),
            runtime_failure: r.failure.clone(),
            final_transition: last.clone(),
            eligible_score: eligible.then_some(score),
            eligible_reward_per_s: eligible.then_some(score / last.time_s),
            wall_s: self.wall_s,
            observed_s: last.time_s,
            unobserved_committed_steps: r.completed_steps - last.completed_steps,
            simulated_s: r.completed_steps as f64 * r.config.step_s,
            simulated_s_per_wall_s: r.completed_steps as f64 * r.config.step_s / self.wall_s,
        };
        if !outcome.simulated_s_per_wall_s.is_finite()
            || outcome
                .eligible_reward_per_s
                .is_some_and(|v| !v.is_finite())
        {
            return Err("nonfinite fidelity cost or reward rate".into());
        }
        Ok((motion, actions, outcome))
    }
}

/// Compare equal-time committed endpoints without interpolation. Different
/// physics timesteps are supported when their task observation clocks coincide.
/// Short/failed captures expose only their observed matched-input prefix.
pub fn compare(
    reference: &EnvironmentCapture,
    candidate: &EnvironmentCapture,
    plan: &ComparisonPlan,
) -> Result<ComparisonReport, String> {
    if plan.version != 1
        || plan.absolute_tolerances.is_empty()
        || plan
            .absolute_tolerances
            .values()
            .any(|v| !v.is_finite() || *v < 0.)
    {
        return Err("invalid explicit fidelity tolerance plan".into());
    }
    plan.reference.validate()?;
    plan.candidate.validate()?;
    let reference_context = ExecutionContext::new(&reference.recording, &reference.task);
    let candidate_context = ExecutionContext::new(&candidate.recording, &candidate.task);
    let actual = reference_context.differences(&candidate_context)?;
    let mut declared = BTreeMap::new();
    for c in &plan.changes {
        if c.reason.trim().is_empty()
            || declared
                .insert(c.difference.path.clone(), &c.difference)
                .is_some()
        {
            return Err("fidelity changes require unique exact paths and reasons".into());
        }
    }
    for d in &actual {
        if declared.remove(&d.path) != Some(d) {
            return Err(format!(
                "undeclared or mismatched fidelity context change at {}",
                d.path
            ));
        }
    }
    if !declared.is_empty() {
        return Err(format!(
            "unused fidelity change declaration at {}",
            declared.keys().next().unwrap()
        ));
    }
    // Task definitions, random seed, command interpretation and commanded episode
    // duration are controlled conditions, not approximation-profile knobs.
    if json!(reference.task) != json!(candidate.task)
        || reference.recording.seed != candidate.recording.seed
        || json!(reference.recording.scene.controller)
            != json!(candidate.recording.scene.controller)
        || reference.recording.scene.period_s != candidate.recording.scene.period_s
        || !close_time(
            reference.recording.config.step_s * reference.recording.config.steps as f64,
            candidate.recording.config.step_s * candidate.recording.config.steps as f64,
        )
    {
        return Err("fidelity comparison requires identical task, seed, controller, command clock and episode duration".into());
    }
    crate::forecast_actions::ControllerContext::from_runtime(
        &reference.recording.scene,
        &reference.recording.config,
    )?
    .matches(&crate::forecast_actions::ControllerContext::from_runtime(
        &candidate.recording.scene,
        &candidate.recording.config,
    )?)?;
    let (a, aa, reference_outcome) = reference.validate()?;
    let (b, ba, candidate_outcome) = candidate.validate()?;
    let n = a.len().min(b.len());
    if aa[..n - 1] != ba[..n - 1] {
        return Err(
            "fidelity comparison requires identical held actions on the compared interval".into(),
        );
    }
    let coordinates = reference.metadata["frame_coordinates"]
        .as_array()
        .ok_or("missing fidelity coordinate metadata")?;
    if candidate.metadata["frame_coordinates"] != reference.metadata["frame_coordinates"] {
        return Err("fidelity coordinate identities/units/order differ".into());
    }
    let mut channels = BTreeMap::new();
    for i in 0..n {
        if !close_time(a[i].time_s, b[i].time_s) {
            return Err("fidelity frame clocks differ".into());
        }
        let av = values(&a[i], &reference.frames[i], coordinates)?;
        let bv = values(&b[i], &candidate.frames[i], coordinates)?;
        if av.keys().ne(bv.keys()) {
            return Err("fidelity motion/sensor/contact topology or availability differs".into());
        }
        for (name, (unit, x)) in av {
            let (bu, y) = &bv[&name];
            if &unit != bu {
                return Err(format!("fidelity channel unit mismatch: {name}"));
            }
            let tolerance = *plan
                .absolute_tolerances
                .get(&unit)
                .ok_or_else(|| format!("missing fidelity tolerance for unit {unit}"))?;
            let d = (x - y).abs();
            if !d.is_finite() {
                return Err("nonfinite fidelity error".into());
            }
            let c = channels.entry(name).or_insert(ChannelError {
                unit,
                samples: 0,
                maximum_absolute: 0.,
                rms: 0.,
                worst_time_s: a[i].time_s,
                absolute_tolerance: tolerance,
                within_tolerance: true,
            });
            c.samples += 1;
            // Stable running RMS, avoiding squared overflow for finite inputs.
            c.rms = (c.rms * ((c.samples - 1) as f64 / c.samples as f64).sqrt())
                .hypot(d / (c.samples as f64).sqrt());
            if d > c.maximum_absolute {
                c.maximum_absolute = d;
                c.worst_time_s = a[i].time_s;
            }
            c.within_tolerance &= d <= tolerance;
        }
    }
    let categorical_outcomes_match = reference_outcome.completed == candidate_outcome.completed
        && reference_outcome.error == candidate_outcome.error
        && reference_outcome.runtime_failure == candidate_outcome.runtime_failure
        && reference_outcome.final_transition.terminated
            == candidate_outcome.final_transition.terminated
        && reference_outcome.final_transition.truncated
            == candidate_outcome.final_transition.truncated
        && reference_outcome.final_transition.termination_reasons
            == candidate_outcome.final_transition.termination_reasons;
    Ok(ComparisonReport { version: 1, plan: plan.clone(), reference_context, candidate_context,
        matched_input_duration_s: a[n-1].time_s, compared_frames: n,
        trajectory_within_tolerances: channels.values().all(|c|c.within_tolerance), channels, categorical_outcomes_match,
        reference: reference_outcome, candidate: candidate_outcome,
        scope: "Equal-time endpoint kinematics, held IMUs and per-link aggregate contact forces. Full final task transitions and eligible scores retained separately. No interpolation, hidden motor-state comparison, continuous-time fall guarantee, hardware calibration or automatic overall fidelity approval. Failed/short runs qualify only the compared prefix. Cost includes each host's declared capture overhead; provenance is host-attested.".into() })
}

fn values(
    m: &MotionSnapshot,
    frame: &Value,
    coordinates: &[Value],
) -> Result<BTreeMap<String, (String, f64)>, String> {
    let mut out = BTreeMap::new();
    let mut put = |name: String, unit: &str, value: f64| -> Result<(), String> {
        if !value.is_finite() || out.insert(name, (unit.into(), value)).is_some() {
            return Err("invalid or duplicate fidelity channel".into());
        }
        Ok(())
    };
    if coordinates.len() != m.joint_positions.len() {
        return Err("fidelity coordinate metadata dimension mismatch".into());
    }
    for (i, c) in coordinates.iter().enumerate() {
        let name = c["name"]
            .as_str()
            .ok_or("missing fidelity coordinate name")?;
        let (p, v) = match c["position_unit"].as_str() {
            Some("rad") => ("rad", "rad/s"),
            Some("m") => ("m", "m/s"),
            _ => return Err("unsupported fidelity coordinate unit".into()),
        };
        if c["index"].as_u64() != Some(i as u64) || c["velocity_unit"].as_str() != Some(v) {
            return Err("invalid fidelity coordinate index/rate unit".into());
        }
        put(
            format!("joint/{}/position", escape(name)),
            p,
            m.joint_positions[i],
        )?;
        put(
            format!("joint/{}/velocity", escape(name)),
            v,
            m.joint_velocities[i],
        )?;
    }
    for p in &m.poses {
        for i in 0..3 {
            put(
                format!("link/{}/position/{i}", escape(&p.name)),
                "m",
                p.position_m[i],
            )?;
            put(
                format!("link/{}/velocity/{i}", escape(&p.name)),
                "m/s",
                p.velocity_m_s[i],
            )?;
            put(
                format!("link/{}/angular_velocity/{i}", escape(&p.name)),
                "rad/s",
                p.angular_velocity_rad_s[i],
            )?;
            for j in 0..3 {
                put(
                    format!("link/{}/rotation/{i}/{j}", escape(&p.name)),
                    "1",
                    p.rotation[i][j],
                )?;
            }
        }
    }
    for s in &m.imu_samples {
        let name = format!("imu/{}/{}", escape(&s.link), escape(&s.name));
        put(
            format!("{name}/available"),
            "1",
            if s.sample_time_s.is_some() { 1. } else { 0. },
        )?;
        put(
            format!("{name}/next_sample_time"),
            "s",
            s.next_sample_time_s,
        )?;
        if let Some(t) = s.sample_time_s {
            put(format!("{name}/sample_time"), "s", t)?;
            for i in 0..3 {
                put(
                    format!("{name}/specific_force/{i}"),
                    "m/s²",
                    s.specific_force_m_s2[i],
                )?;
                put(
                    format!("{name}/angular_velocity/{i}"),
                    "rad/s",
                    s.angular_velocity_rad_s[i],
                )?;
            }
        }
    }
    let mut forces = vec![[0.; 3]; m.poses.len()];
    for c in frame["contacts"]
        .as_array()
        .ok_or("missing fidelity contacts")?
    {
        let link = c["link"].as_u64().ok_or("invalid fidelity contact link")? as usize;
        let f: [f64; 3] =
            serde_json::from_value(c["force_n"].clone()).map_err(|e| e.to_string())?;
        let sum = forces
            .get_mut(link)
            .ok_or("fidelity contact link outside pose topology")?;
        for i in 0..3 {
            sum[i] += f[i];
        }
    }
    for (p, f) in m.poses.iter().zip(forces) {
        for (i, x) in f.into_iter().enumerate() {
            put(format!("contact/{}/force/{i}", escape(&p.name)), "N", x)?;
        }
    }
    Ok(out)
}
