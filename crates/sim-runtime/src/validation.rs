//! Reproducible derivative and trajectory comparisons for every session host.
use crate::session::{Recording, Session};
use serde::{Deserialize, Serialize};
use sim_dynamics::jacobian_check::{check_jacobian, CheckConfig, CheckReport};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use web_time::Instant;

/// Unique diagnostic labels for all stored states, including repeated node names.
pub fn state_labels(runtime: &sim_compile::Runtime) -> HashMap<sim_core::StateId, String> {
    let mut ports: HashMap<_, BTreeSet<String>> = HashMap::new();
    for island in &runtime.islands {
        for (port_id, lanes) in &island.system.port_lanes {
            let port = &runtime.model.ports[*port_id];
            for full in lanes {
                let id = island.system.state_ids[*full];
                if runtime
                    .model
                    .state
                    .entry(id)
                    .unwrap()
                    .name
                    .starts_with("node.")
                {
                    ports.entry(id).or_default().insert(format!(
                        "{}.{}",
                        runtime.model.objects[runtime.model.behaviors[port.owner].object].name,
                        port.name
                    ));
                }
            }
        }
    }
    runtime
        .model
        .state
        .iter()
        .enumerate()
        .map(|(ordinal, (id, state))| {
            let context = ports
                .get(&id)
                .map(|names| {
                    format!(
                        " ({})",
                        names.iter().cloned().collect::<Vec<_>>().join(" | ")
                    )
                })
                .unwrap_or_default();
            // State names such as node.angle repeat. An ordinal keeps every
            // channel distinct in comparisons, including unconnected metadata.
            (id, format!("{}{} [state {ordinal}]", state.name, context))
        })
        .collect()
}

#[derive(Serialize)]
pub struct NamedJacobianCheck {
    pub island: usize,
    pub time_s: f64,
    /// Residual rows follow these state/port equations. Probe column indices
    /// refer to the same list. A compiled Jacobian may mix analytic and FD slots.
    pub coordinates: Vec<(String, String)>,
    pub states: Vec<f64>,
    pub rates: Vec<f64>,
    pub report: CheckReport,
}
pub fn audit_jacobians(
    session: &Session,
    config: &CheckConfig,
) -> Result<Vec<NamedJacobianCheck>, String> {
    let runtime = &session.robot.runtime;
    let labels = state_labels(runtime);
    runtime
        .islands
        .iter()
        .enumerate()
        .map(|(index, island)| {
            let coordinates = island
                .system
                .full_of
                .iter()
                .map(|full| {
                    let entry = runtime
                        .model
                        .state
                        .entry(island.system.state_ids[*full])
                        .unwrap();
                    (
                        labels[&island.system.state_ids[*full]].clone(),
                        entry.quantity.unit().into(),
                    )
                })
                .collect();
            Ok(NamedJacobianCheck {
                island: index,
                time_s: island.time,
                coordinates,
                states: island.state.clone(),
                rates: island.last_rate().to_vec(),
                report: check_jacobian(
                    &island.system,
                    island.time,
                    &island.state,
                    island.last_rate(),
                    config,
                )?,
            })
        })
        .collect()
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CompareConfig {
    /// Absolute tolerance in each value's declared unit; specific units override.
    pub absolute_tolerance: f64,
    pub absolute_by_unit: BTreeMap<String, f64>,
    pub relative_tolerance: f64,
    /// 1 compares derivatives at the same timestep. 2 also tests h versus h/2.
    pub reference_substeps: usize,
    /// Optional absolute step boundaries applied to both runs. They change the
    /// integration grid, not physical events or controller sampling deadlines.
    /// This diagnostic does not prevent either solver from subdividing further.
    pub shared_step_breakpoints_s: Vec<f64>,
    /// Reproduce this many actions using the candidate settings in both runs,
    /// verify identical states, then begin the independent comparison. Zero
    /// retains the full-trajectory comparison from the authored initial state.
    pub shared_prefix_frames: usize,
    /// Capture both suffixes' implicit attempts, up to this count per island.
    /// Zero disables capture; warmup is excluded. This does not change gates.
    pub attempt_audit_limit: usize,
}
impl Default for CompareConfig {
    fn default() -> Self {
        Self {
            absolute_tolerance: 1e-6,
            absolute_by_unit: BTreeMap::new(),
            relative_tolerance: 1e-5,
            reference_substeps: 1,
            shared_step_breakpoints_s: Vec::new(),
            shared_prefix_frames: 0,
            attempt_audit_limit: 0,
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct TrajectoryDifference {
    pub frame: usize,
    pub time_s: f64,
    pub name: String,
    pub unit: String,
    pub provided: f64,
    pub numerical: f64,
    pub error: f64,
    pub tolerance: f64,
}
/// Physical error magnitudes, independent of the pass/fail tolerances. Samples
/// are channel/report-frame pairs, not a continuous-time or spatial integral.
#[derive(Clone, Debug, Default, Serialize)]
pub struct UnitErrorSummary {
    pub comparisons: usize,
    pub mismatches: usize,
    pub rms_error: f64,
    pub maximum_absolute_error: f64,
    pub maximum_error_sample: Option<TrajectoryDifference>,
}
impl UnitErrorSummary {
    fn observe(&mut self, sample: &TrajectoryDifference) {
        self.comparisons += 1;
        self.mismatches += usize::from(sample.error > sample.tolerance);
        let n = self.comparisons as f64;
        // Weighted hypot avoids squaring large finite values or accumulating
        // a sum whose magnitude grows with the number of trajectory samples.
        self.rms_error = (self.rms_error * ((n - 1.0) / n).sqrt())
            .hypot(sample.error / n.sqrt());
        if self.maximum_error_sample.is_none() || sample.error > self.maximum_absolute_error {
            self.maximum_absolute_error = sample.error;
            self.maximum_error_sample = Some(sample.clone());
        }
    }
}
#[derive(Serialize)]
pub struct Comparison {
    pub passed: bool,
    pub seed: u64,
    pub compared_frames: usize,
    pub comparisons: usize,
    pub mismatches: usize,
    pub max_error_ratio: f64,
    pub candidate_wall_s: f64,
    pub reference_wall_s: f64,
    pub candidate_step_s: f64,
    pub reference_step_s: f64,
    pub candidate_options: crate::BuildOptions,
    pub reference_options: crate::BuildOptions,
    pub config: CompareConfig,
    pub source: serde_json::Value,
    pub differences: Vec<TrajectoryDifference>,
    /// Earliest reporting-frame failure; ties use stable channel-name order.
    /// The bounded worst-error list can otherwise hide the onset of divergence.
    pub first_difference: Option<TrajectoryDifference>,
    pub errors_by_unit: BTreeMap<String, UnitErrorSummary>,
    /// Per-frame accuracy and cumulative solver work. Different subdivision
    /// histories can explain integration differences even at equal nominal h.
    pub frames: Vec<FrameComparison>,
    /// Prefix work is excluded from accuracy counts and comparison timers.
    pub shared_prefix: Option<SharedPrefixReport>,
    /// Optional paired solve-point evidence. Capacity exhaustion is reported
    /// explicitly and is not evidence that every attempted solve was captured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt_audit: Option<ComparisonAttemptAudit>,
}
#[derive(Serialize)]
pub struct ComparisonAttemptAudit {
    pub limit_per_island: usize,
    pub capacity_reached: bool,
    pub candidate: serde_json::Value,
    pub reference: serde_json::Value,
}
#[derive(Serialize)]
pub struct SharedPrefixReport {
    pub frames: usize,
    pub time_s: f64,
    pub candidate_wall_s: f64,
    pub reference_wall_s: f64,
    pub exact_state_match: bool,
}
#[derive(Serialize)]
pub struct FrameComparison {
    pub frame: usize,
    pub time_s: f64,
    pub mismatches: usize,
    pub max_error_ratio: f64,
    pub errors_by_unit: BTreeMap<String, UnitErrorSummary>,
    pub candidate_solver: Vec<sim_dynamics::RunStats>,
    pub reference_solver: Vec<sim_dynamics::RunStats>,
}
/// Stored state and physical measurements with declared units, using the same
/// definitions as the independent trajectory comparator. Contact loads are
/// aggregated by directed link pair, avoiding sample-index correspondence.
/// This snapshot does not include every controller/external hidden history;
/// replay the recording to reconstruct that history.
pub fn measurement_snapshot(session: &Session) -> BTreeMap<String, (String, f64)> {
    measurements(session, &state_labels(&session.robot.runtime))
}

fn measurements(
    session: &Session,
    labels: &HashMap<sim_core::StateId, String>,
) -> BTreeMap<String, (String, f64)> {
    let mut values: BTreeMap<_, _> = session
        .robot
        .runtime
        .model
        .state
        .iter()
        .map(|(id, s)| (labels[&id].clone(), (s.quantity.unit().into(), s.committed)))
        .collect();
    let frame = session.frame();
    values.insert("diagnostic.time".into(), ("s".into(), frame.time_s));
    values.insert(
        "diagnostic.done".into(),
        ("1".into(), if frame.done { 1.0 } else { 0.0 }),
    );
    values.insert(
        "diagnostic.energy".into(),
        ("J".into(), session.robot.runtime.energy()),
    );
    for pose in &frame.poses {
        for (axis, value) in pose.position_m.iter().enumerate() {
            values.insert(
                format!("pose.{}.position.{axis}", pose.name),
                ("m".into(), *value),
            );
        }
        for (axis, value) in pose.rotation.iter().flatten().enumerate() {
            values.insert(
                format!("pose.{}.rotation.{axis}", pose.name),
                ("1".into(), *value),
            );
        }
    }
    for contact in &frame.contacts {
        let pair = format!("contact.{}.{:?}", contact.link, contact.other);
        for (axis, value) in contact.force_n.iter().enumerate() {
            values
                .entry(format!("{pair}.force.{axis}"))
                .or_insert(("N".into(), 0.0))
                .1 += value;
        }
        let depth = &mut values
            .entry(format!("{pair}.penetration"))
            .or_insert(("m".into(), 0.0))
            .1;
        *depth = depth.max(contact.penetration_m);
    }
    values
}
/// Reconstruct both runs from the same scene/seed/actions. Reference integration
/// bypasses all supplied derivatives and sparsity; the physics equations and
/// integrator stay identical. This detects derivative regressions, not wrong
/// equations shared by both runs; analytic physical benchmarks remain required.
/// An optional shared prefix reproduces and verifies the same state and
/// controller history before switching derivatives; its warmup frames are not
/// counted as independent trajectory validation.
pub fn compare_recording(
    recording: &Recording,
    config: &CompareConfig,
) -> Result<Comparison, String> {
    if recording.version != 1 || recording.actions.is_empty() {
        return Err("comparison requires a version-1 recording with actions".into());
    }
    if !config.absolute_tolerance.is_finite()
        || config.absolute_tolerance <= 0.0
        || !config.relative_tolerance.is_finite()
        || config.relative_tolerance < 0.0
        || config.reference_substeps == 0
        || config.reference_substeps > 64
        || config.shared_prefix_frames >= recording.actions.len()
        || config.attempt_audit_limit > 10000
        || config.shared_step_breakpoints_s.len() > 10000
        || config.shared_step_breakpoints_s.iter().any(|t| !t.is_finite() || *t < 0.0)
        || config.shared_step_breakpoints_s.windows(2).any(|p| p[0] >= p[1])
        || config
            .absolute_by_unit
            .values()
            .any(|v| !v.is_finite() || *v <= 0.0)
    {
        return Err("invalid trajectory comparison tolerances/substeps/breakpoints".into());
    }
    let mut candidate_scene = recording.scene.clone();
    candidate_scene.options.numerical_jacobian = false;
    let mut reference_scene = candidate_scene.clone();
    reference_scene.options.numerical_jacobian = true;
    // This reference must independently validate the experimental reuse path,
    // rather than sharing its cross-timestep approximation.
    reference_scene.options.event_jacobian_reuse = false;
    reference_scene.options.step /= config.reference_substeps as f64;
    let mut report = Comparison {
        passed: true,
        seed: recording.seed,
        compared_frames: 0,
        comparisons: 0,
        mismatches: 0,
        max_error_ratio: 0.0,
        candidate_wall_s: 0.0,
        reference_wall_s: 0.0,
        candidate_step_s: candidate_scene.options.step,
        reference_step_s: reference_scene.options.step,
        candidate_options: candidate_scene.options.clone(),
        reference_options: reference_scene.options.clone(),
        config: config.clone(),
        source: recording.scene.robot.source.clone(),
        differences: vec![],
        first_difference: None,
        errors_by_unit: BTreeMap::new(),
        frames: vec![],
        shared_prefix: None,
        attempt_audit: None,
    };
    let reference_start_scene = if config.shared_prefix_frames > 0 {
        candidate_scene.clone()
    } else {
        reference_scene.clone()
    };
    let mut candidate = Session::new(candidate_scene, recording.seed)?;
    let mut reference = Session::new(reference_start_scene, recording.seed)?;
    for session in [&mut candidate, &mut reference] {
        for island in &mut session.robot.runtime.islands {
            island.set_step_breakpoints(config.shared_step_breakpoints_s.clone())
                .map_err(|e| e.to_string())?;
        }
    }
    let candidate_labels = state_labels(&candidate.robot.runtime);
    let reference_labels = state_labels(&reference.robot.runtime);
    if config.shared_prefix_frames > 0 {
        let mut prefix = SharedPrefixReport { frames:config.shared_prefix_frames,
            time_s:0.0, candidate_wall_s:0.0, reference_wall_s:0.0, exact_state_match:false };
        for action in recording.actions.iter().take(config.shared_prefix_frames) {
            let started = Instant::now();
            candidate.step(action)?;
            prefix.candidate_wall_s += started.elapsed().as_secs_f64();
            let started = Instant::now();
            reference.step(action)?;
            prefix.reference_wall_s += started.elapsed().as_secs_f64();
        }
        if candidate.frame().error.is_some() || reference.frame().error.is_some() {
            return Err("shared comparison prefix failed".into());
        }
        if candidate.robot.runtime.snapshot() != reference.robot.runtime.snapshot()
            || measurements(&candidate, &candidate_labels) != measurements(&reference, &reference_labels)
            || serde_json::to_value(candidate.frame()).map_err(|e| e.to_string())?
                != serde_json::to_value(reference.frame()).map_err(|e| e.to_string())? {
            return Err("shared comparison prefix did not reproduce identical states".into());
        }
        // Both independent suffixes start without a cached matrix. Replaying
        // the prefix also reproduces controller history instead of copying
        // only mechanical coordinates into a newly initialized controller.
        for island in &mut candidate.robot.runtime.islands {
            island.set_numerical_jacobian(false);
        }
        for island in &mut reference.robot.runtime.islands {
            island.set_numerical_jacobian(true);
            island.event_jacobian_reuse = false;
        }
        reference.robot.step = reference_scene.options.step;
        reference.scene.options = reference_scene.options;
        prefix.time_s = candidate.robot.time();
        prefix.exact_state_match = true;
        report.shared_prefix = Some(prefix);
    }
    if config.attempt_audit_limit > 0 {
        candidate.set_attempt_audit_limit(config.attempt_audit_limit)?;
        reference.set_attempt_audit_limit(config.attempt_audit_limit)?;
    }
    for frame in config.shared_prefix_frames..=recording.actions.len() {
        if frame > config.shared_prefix_frames {
            let started = Instant::now();
            candidate.step(&recording.actions[frame - 1])?;
            report.candidate_wall_s += started.elapsed().as_secs_f64();
            let started = Instant::now();
            reference.step(&recording.actions[frame - 1])?;
            report.reference_wall_s += started.elapsed().as_secs_f64();
        }
        if let Some(error) = candidate.frame().error.or(reference.frame().error) {
            return Err(format!(
                "comparison solver failed at frame {frame}: {error}"
            ));
        }
        let a = measurements(&candidate, &candidate_labels);
        let b = measurements(&reference, &reference_labels);
        let mut frame_report = FrameComparison {
            frame, time_s: candidate.robot.time(), mismatches: 0, max_error_ratio: 0.0,
            errors_by_unit: BTreeMap::new(),
            candidate_solver: candidate.robot.runtime.islands.iter().map(|i| i.stats).collect(),
            reference_solver: reference.robot.runtime.islands.iter().map(|i| i.stats).collect(),
        };
        let names: std::collections::BTreeSet<_> = a.keys().chain(b.keys()).collect();
        for name in names {
            let unit = a.get(name).or_else(|| b.get(name)).unwrap().0.clone();
            // An absent contact contributes zero load/depth. Every other
            // channel must exist on both sides with the same declared unit.
            if !name.starts_with("contact.") && (a.get(name).is_none() || b.get(name).is_none()) {
                return Err(format!("channel contract differs: {name}"));
            }
            if let (Some(aa), Some(bb)) = (a.get(name), b.get(name)) {
                if aa.0 != bb.0 {
                    return Err(format!("unit mismatch: {name}"));
                }
            }
            let av = a.get(name).map(|v| v.1).unwrap_or(0.0);
            let bv = b.get(name).map(|v| v.1).unwrap_or(0.0);
            if !av.is_finite() || !bv.is_finite() {
                return Err(format!("non-finite comparison channel: {name}"));
            }
            let absolute = config
                .absolute_by_unit
                .get(&unit)
                .copied()
                .unwrap_or(config.absolute_tolerance);
            let tolerance = absolute + config.relative_tolerance * av.abs().max(bv.abs());
            let error = (av - bv).abs();
            if !tolerance.is_finite() || !error.is_finite() {
                return Err(format!("non-finite comparison tolerance/error: {name}"));
            }
            report.comparisons += 1;
            report.max_error_ratio = report.max_error_ratio.max(error / tolerance);
            frame_report.max_error_ratio = frame_report.max_error_ratio.max(error / tolerance);
            let sample = TrajectoryDifference {
                frame, time_s: candidate.robot.time(), name: name.clone(),
                unit: unit.clone(), provided: av, numerical: bv, error, tolerance,
            };
            report.errors_by_unit.entry(unit.clone()).or_default().observe(&sample);
            frame_report.errors_by_unit.entry(unit).or_default().observe(&sample);
            if error > tolerance {
                frame_report.mismatches += 1;
                report.mismatches += 1;
                if report.first_difference.is_none() {
                    report.first_difference = Some(sample.clone());
                }
                report.differences.push(sample);
                report
                    .differences
                    .sort_by(|a, b| (b.error / b.tolerance).total_cmp(&(a.error / a.tolerance)));
                report.differences.truncate(32);
            }
        }
        report.compared_frames += 1;
        report.frames.push(frame_report);
    }
    report.passed = report.mismatches == 0;
    if config.attempt_audit_limit > 0 {
        let capacity_reached = [&candidate, &reference].iter().any(|s|
            s.robot.runtime.islands.iter().any(|i| i.implicit_attempts.len() == config.attempt_audit_limit));
        report.attempt_audit = Some(ComparisonAttemptAudit {
            limit_per_island:config.attempt_audit_limit, capacity_reached,
            candidate:implicit_attempt_report(&candidate)?,
            reference:implicit_attempt_report(&reference)?,
        });
    }
    Ok(report)
}

/// Named solve-point diagnostics after explicitly enabling each island's audit.
/// Successful nonlinear trials may still belong to rejected outer steps.
pub fn implicit_attempt_report(session: &Session) -> Result<serde_json::Value, String> {
    let labels = state_labels(&session.robot.runtime);
    let mut islands = Vec::new();
    for (index,island) in session.robot.runtime.islands.iter().enumerate() {
        let names: Vec<_> = island.system.full_of.iter().map(|f| &labels[&island.system.state_ids[*f]]).collect();
        let mut attempts = Vec::new();
        for point in &island.implicit_attempts {
            let physical = match session.robot.generalized_at_solver_point(index,point)? {
                Some(g) => {
                    let audit = session.robot.art.audit_constraints(&g, &Default::default())?;
                    let evaluation = session.robot.art.evaluate(&g);
                    let contacts: Vec<_> = evaluation.contacts.iter().map(|c| serde_json::json!({
                        "link":c.link,"other":c.other,"point_m":c.point.as_slice(),
                        "force_n":c.force.as_slice(),"penetration_m":c.penetration})).collect();
                    Some(serde_json::json!({"constraints":audit,"contacts":contacts}))
                }
                None => None,
            };
            attempts.push(serde_json::json!({"solve":point,"physical_at_stage":physical}));
        }
        islands.push(serde_json::json!({"island":index,"coordinates":names,"attempts":attempts,
            "attempt_limit":island.attempt_audit_limit(),
            "capacity_reached":island.attempt_audit_limit()>0 && island.implicit_attempts.len()==island.attempt_audit_limit(),
            "solver_stats":island.stats}));
    }
    Ok(serde_json::json!({"source":session.scene.robot.source,"options":session.scene.options,
        "islands":islands,"notes":[
            "Opt-in diagnostic residual re-evaluations are excluded from ordinary solver profile counters.",
            "Stage configuration, rates, constraints and contact loads share the recorded implicit evaluation time.",
            "Use committed=true for substeps retained by the local Simulation; false includes discarded outer/event-search trials and superseded steps. Legacy missing status is unknown.",
            "Newton largest_rows are (row index, raw residual, absolute row-scaled residual) at iteration entry. The terminal solve-point residual is recomputed separately."
        ]}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_error_summary_includes_passing_samples_and_avoids_overflow() {
        let mut summary = UnitErrorSummary::default();
        for (frame, (error, tolerance)) in [(3.0, 1.0), (4.0, 1.0), (12.0, 20.0)].into_iter().enumerate() {
            summary.observe(&TrajectoryDifference {
                frame, time_s: frame as f64, name: "force".into(), unit: "N".into(),
                provided: error, numerical: 0.0, error, tolerance,
            });
        }
        assert_eq!(summary.comparisons, 3);
        assert_eq!(summary.mismatches, 2);
        assert!((summary.rms_error - 13.0 / 3.0_f64.sqrt()).abs() < 1e-14);
        assert_eq!(summary.maximum_absolute_error, 12.0);
        assert_eq!(summary.maximum_error_sample.as_ref().unwrap().frame, 2);
        let mut large = UnitErrorSummary::default();
        let mut sample = summary.maximum_error_sample.unwrap();
        sample.error = 1e200;
        for _ in 0..100 { large.observe(&sample); }
        assert!((large.rms_error / 1e200 - 1.0).abs() < 1e-14);
    }
}
