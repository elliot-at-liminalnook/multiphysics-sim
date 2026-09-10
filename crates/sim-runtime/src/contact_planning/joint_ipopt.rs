//! The existing joint CAD problem presented to the shared native NLP driver.
use super::*;
use sim_solve::ipopt::{
    IpoptConfig, IpoptLibrary, IpoptResult, NlpDerivatives, NlpEvaluation, NlpProblem,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JointIpoptConfig {
    pub solver: IpoptConfig,
    /// Includes invalid motion probes, force derivative calls and final audit.
    pub maximum_model_evaluations: usize,
    /// Difference step in normalized variable coordinates.
    pub difference_step: f64,
    /// Eliminate validated zero endpoint rows and require collision-free
    /// sampled geometry as an evaluation domain. Default preserves old solves.
    #[serde(default)]
    pub collision_free_domain: bool,
    /// Combine body-value derivative probes with disjoint fixed-time support.
    /// Invalid combined probes fall back to the individual-column path.
    #[serde(default)]
    pub group_body_derivatives: bool,
}
#[derive(Debug, Serialize)]
pub struct JointIpoptOptimization {
    pub candidate: Option<JointContactMotion>,
    pub report: Option<JointContactReport>,
    pub returned_candidate_error: Option<String>,
    pub search: IpoptResult,
    pub best_sampled_feasible: Option<JointContactMotion>,
    pub model_evaluations: usize,
    pub model_budget_exhausted: bool,
    pub jacobian_nonzeros: usize,
    pub dense_jacobian_entries: usize,
    pub initial_report: JointContactReport,
    pub constraint_projection: ConstraintProjection,
    pub grouped_body_probes: usize,
    pub body_group_fallbacks: usize,
    pub scope: &'static str,
}

/// Original report row identities omitted from the native constraint vector.
/// These are derived from topology, never from numerical zeros at a seed.
#[derive(Debug, Serialize)]
pub struct ConstraintProjection {
    pub full_count: usize,
    pub fixed_zero_rows: Vec<usize>,
    pub collision_domain_rows: Vec<usize>,
    #[serde(skip)]
    native_rows: Vec<Option<usize>>,
}
impl ConstraintProjection {
    fn new(count: usize, fixed: Vec<usize>, collisions: Vec<usize>) -> Result<Self, String> {
        let mut omitted = vec![false; count];
        for &row in fixed.iter().chain(&collisions) {
            if row >= count || omitted[row] {
                return Err("invalid or duplicate projected constraint row".into());
            }
            omitted[row] = true;
        }
        let mut next = 0;
        let native_rows = omitted
            .into_iter()
            .map(|skip| {
                if skip {
                    None
                } else {
                    let row = next;
                    next += 1;
                    Some(row)
                }
            })
            .collect();
        Ok(Self {
            full_count: count,
            fixed_zero_rows: fixed,
            collision_domain_rows: collisions,
            native_rows,
        })
    }
    fn check(&self, full: &[f64]) -> Result<(), String> {
        if full.len() != self.full_count || full.iter().any(|v| !v.is_finite()) {
            return Err("invalid full constraint vector before projection".into());
        }
        if self.fixed_zero_rows.iter().any(|&r| full[r] != 0.) {
            return Err("fixed endpoint cone identity violated".into());
        }
        // The source quantity is nonnegative penetration. Any nonzero value,
        // even below the old normalization scale, is outside this domain.
        if self.collision_domain_rows.iter().any(|&r| full[r] != 0.) {
            return Err("joint trial outside sampled collision-free domain".into());
        }
        Ok(())
    }
    fn project(&self, full: &[f64]) -> Result<Vec<f64>, String> {
        self.check(full)?;
        Ok(full
            .iter()
            .zip(&self.native_rows)
            .filter_map(|(v, row)| row.map(|_| *v))
            .collect())
    }
    fn count(&self) -> usize {
        self.native_rows.iter().flatten().count()
    }
}

// Fixed structural rows, including entries that can become nonzero as motion
// changes. Force coefficients cannot affect geometry or other clocks' loads;
// motion decisions cannot affect the force-node friction cones.
fn structural_rows(
    planner: &ContactPlanner<'_>,
    candidate: &JointContactMotion,
    variables: &[JointContactVariable],
    count: usize,
    collision_free_domain: bool,
) -> Result<(Vec<Vec<usize>>, ConstraintProjection), String> {
    let cache = planner.joint_cache.borrow();
    let entry = cache
        .entry
        .as_ref()
        .ok_or("missing initial joint geometry")?;
    let mut row = 0;
    let mut motion_rows = Vec::new();
    let mut force_rows = Vec::new();
    let mut cone_rows = Vec::new();
    let mut fixed_zero_rows = Vec::new();
    let mut collision_domain_rows = Vec::new();
    for (report, templates) in entry.reports.iter().zip(&candidate.force_templates) {
        let mut clock_rows = Vec::new();
        for frame in &report.frames {
            let loads = planner.joint_frame_load_rows(frame);
            clock_rows.extend(row..row + loads);
            motion_rows.extend(row..row + loads + 2);
            if collision_free_domain {
                collision_domain_rows.push(row + loads);
            }
            row += loads + 2;
        }
        force_rows.push(clock_rows);
        let mut clock_cones = Vec::new();
        for template in templates {
            let mut foot_cones = Vec::new();
            for (node, _) in template.keyframes.iter().enumerate() {
                foot_cones.push([row, row + 1]);
                if collision_free_domain && (node == 0 || node + 1 == template.keyframes.len()) {
                    fixed_zero_rows.extend([row, row + 1]);
                }
                row += 2;
            }
            clock_cones.push(foot_cones);
        }
        cone_rows.push(clock_cones);
    }
    if row != count {
        return Err("joint NLP structural row mismatch".into());
    }
    let projection = ConstraintProjection::new(count, fixed_zero_rows, collision_domain_rows)?;
    let rows: Result<Vec<Vec<usize>>, String> = variables
        .iter()
        .map(|v| match v.decision.force_node(&candidate.motion)? {
            None => Ok(motion_rows.clone()),
            Some(ForceNode { clock, slot, node, .. }) => {
                let mut rows = force_rows.get(clock).ok_or("invalid force clock")?.clone();
                rows.extend(
                    cone_rows
                        .get(clock)
                        .and_then(|c| c.get(slot))
                        .and_then(|f| f.get(node))
                        .ok_or("invalid force cone node")?,
                );
                Ok(rows)
            }
        })
        .collect();
    let mut rows = rows?;
    for column in &mut rows {
        column.retain(|r| projection.native_rows[*r].is_some());
    }
    Ok((rows, projection))
}
struct JointNlp<'a, 'model> {
    planner: &'a ContactPlanner<'model>,
    initial: &'a JointContactMotion,
    variables: &'a [JointContactVariable],
    config: &'a JointIpoptConfig,
    rows: &'a [Vec<usize>],
    constraint_count: usize,
    projection: &'a ConstraintProjection,
    model_evaluations: usize,
    budget_exhausted: bool,
    grouped_body_probes: usize,
    body_group_fallbacks: usize,
    best: Option<(f64, JointContactMotion)>,
    progress: &'a mut dyn FnMut(&JointContactMotion, &JointContactReport),
}
impl JointNlp<'_, '_> {
    fn consume(&mut self) -> Result<(), String> {
        if self.model_evaluations >= self.config.maximum_model_evaluations - 1 {
            self.budget_exhausted = true;
            return Err("joint CAD evaluation budget exhausted".into());
        }
        self.model_evaluations += 1;
        Ok(())
    }
    fn decode(&self, x: &[f64]) -> Result<JointContactMotion, String> {
        if x.len() != self.variables.len() {
            return Err("normalized joint variable dimension mismatch".into());
        }
        let values = x
            .iter()
            .zip(self.variables)
            .map(|(x, v)| {
                if !x.is_finite() || *x < 0. || *x > 1. {
                    return Err("normalized joint variable outside bounds".into());
                }
                Ok(v.bound.lower + x * (v.bound.upper - v.bound.lower))
            })
            .collect::<Result<Vec<_>, String>>()?;
        decode_joint_values(
            self.initial,
            self.variables,
            self.planner.recipe.direction_world,
            &values,
        )
    }
    fn observe(
        &mut self,
        candidate: &JointContactMotion,
        report: &JointContactReport,
    ) -> Result<(), String> {
        if report.constraints.objective.len() != 1
            || report.constraints.inequalities.len() != self.constraint_count
        {
            return Err("joint NLP objective/constraint dimensions changed".into());
        }
        if report.sampled_feasible
            && self
                .best
                .as_ref()
                .is_none_or(|(v, _)| report.motion_report.speed_m_s > *v)
        {
            self.best = Some((report.motion_report.speed_m_s, candidate.clone()));
        }
        (self.progress)(candidate, report);
        Ok(())
    }
    fn objective(report: &JointContactReport) -> f64 {
        0.5 * report
            .constraints
            .objective
            .iter()
            .map(|r| r * r)
            .sum::<f64>()
    }
    fn evaluate_full(&mut self, x: &[f64]) -> Result<NlpEvaluation, String> {
        self.consume()?;
        let candidate = self.decode(x)?;
        let report = self.planner.evaluate_joint(&candidate)?;
        self.observe(&candidate, &report)?;
        self.projection.check(&report.constraints.inequalities)?;
        Ok(NlpEvaluation {
            objective: Self::objective(&report),
            constraints: report.constraints.inequalities,
        })
    }

    fn body_columns(
        &mut self,
        x: &[f64],
        candidate: &JointContactMotion,
        report: &JointContactReport,
    ) -> Result<std::collections::BTreeMap<usize, Vec<f64>>, String> {
        let mut output = std::collections::BTreeMap::new();
        if !self.config.group_body_derivatives {
            return Ok(output);
        }
        if !matches!(
            candidate.motion.body.interpolation,
            Interpolation::PeriodicCubicBSpline
        ) {
            return Ok(output);
        }
        let body = Trajectory::new(candidate.motion.body.clone())?;
        let variables = self
            .variables
            .iter()
            .enumerate()
            .filter_map(|(j, v)| match v.decision {
                JointContactDecision::Motion {
                    decision: ContactDecision::BodyControl { control, .. },
                } if v.bound.upper > v.bound.lower => Some((j, control)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let mut supports = vec![Vec::new(); variables.len()];
        {
            // This entry is the geometry just used by linearize_joint_forces.
            // Copy only support metadata before any new probe replaces it.
            let cache = self.planner.joint_cache.borrow();
            let entry = cache
                .entry
                .as_ref()
                .ok_or("missing body derivative geometry")?;
            let mut row = 0;
            for (clock, templates) in entry.reports.iter().zip(&candidate.force_templates) {
                for frame in &clock.frames {
                    let active = body.periodic_support_controls(
                        frame.time_s.rem_euclid(candidate.motion.period_s),
                    )?;
                    let count = self.planner.joint_frame_load_rows(frame) + 2;
                    for (i, (_, control)) in variables.iter().enumerate() {
                        if active.contains(control) {
                            supports[i].extend(row..row + count);
                        }
                    }
                    row += count;
                }
                row += templates
                    .iter()
                    .map(|t| 2 * t.keyframes.len())
                    .sum::<usize>();
            }
            if row != self.constraint_count {
                return Err("body support row layout changed".into());
            }
        }
        let groups = sim_solve::group_disjoint_columns(&supports, self.constraint_count)?;
        for group in groups {
            // A singleton offers no compression. Preserve ordinary probing.
            if group.len() < 2 {
                continue;
            }
            let mut sides: [Option<NlpEvaluation>; 2] = [None, None];
            let mut steps = [vec![0.; group.len()], vec![0.; group.len()]];
            let mut failed = false;
            for (slot, sign) in [-1., 1.].into_iter().enumerate() {
                let mut probe = x.to_vec();
                for (k, &i) in group.iter().enumerate() {
                    let j = variables[i].0;
                    probe[j] = (x[j] + sign * self.config.difference_step).clamp(0., 1.);
                    let actual = probe[j] - x[j];
                    if actual.abs() >= 1e-14 {
                        steps[slot][k] = actual;
                    } else {
                        probe[j] = x[j];
                    }
                }
                if steps[slot].iter().all(|h| *h == 0.) {
                    continue;
                }
                self.grouped_body_probes += 1;
                match self.evaluate_full(&probe) {
                    Ok(value) => sides[slot] = Some(value),
                    Err(_) => {
                        failed = true;
                        break;
                    }
                }
            }
            if self.budget_exhausted {
                return Err(
                    "joint CAD evaluation budget exhausted during grouped derivatives".into(),
                );
            }
            if failed {
                self.body_group_fallbacks += 1;
                continue;
            }
            for (k, &i) in group.iter().enumerate() {
                let mut column = vec![0.; 1 + self.constraint_count];
                let a = steps[0][k];
                let b = steps[1][k];
                if a == 0. && b == 0. {
                    self.body_group_fallbacks += 1;
                    continue;
                }
                for &row in &supports[i] {
                    column[1 + row] = match (a != 0., b != 0.) {
                        (true, true) => {
                            (sides[1].as_ref().unwrap().constraints[row]
                                - sides[0].as_ref().unwrap().constraints[row])
                                / (b - a)
                        }
                        (true, false) => {
                            (sides[0].as_ref().unwrap().constraints[row]
                                - report.constraints.inequalities[row])
                                / a
                        }
                        (false, true) => {
                            (sides[1].as_ref().unwrap().constraints[row]
                                - report.constraints.inequalities[row])
                                / b
                        }
                        _ => unreachable!(),
                    };
                }
                // Body values cannot change the speed-only objective or cone
                // coefficients. Full native sparsity stays unchanged as timing moves.
                output.insert(variables[i].0, column);
            }
        }
        Ok(output)
    }
}
impl NlpProblem for JointNlp<'_, '_> {
    fn evaluate(&mut self, x: &[f64]) -> Result<NlpEvaluation, String> {
        let full = self.evaluate_full(x)?;
        Ok(NlpEvaluation {
            objective: full.objective,
            constraints: self.projection.project(&full.constraints)?,
        })
    }
    fn derivatives(&mut self, x: &[f64]) -> Result<NlpDerivatives, String> {
        self.consume()?;
        let candidate = self.decode(x)?;
        let (report, columns) = self
            .planner
            .linearize_joint_forces(&candidate, self.variables)?;
        self.observe(&candidate, &report)?;
        self.projection.check(&report.constraints.inequalities)?;
        let body_columns = self.body_columns(x, &candidate, &report)?;
        let base_objective = Self::objective(&report);
        let mut gradient = vec![0.; x.len()];
        let mut jacobian = Vec::with_capacity(self.rows.iter().map(Vec::len).sum());
        for (j, (variable, column)) in self.variables.iter().zip(columns).enumerate() {
            let width = variable.bound.upper - variable.bound.lower;
            if width == 0. {
                jacobian.extend(self.rows[j].iter().map(|_| 0.));
                continue;
            }
            if let Some(column) = body_columns.get(&j) {
                gradient[j] = column[0];
                jacobian.extend(self.rows[j].iter().map(|row| column[1 + row]));
                continue;
            }
            if let Some(column) = column {
                if column.len() != 1 + self.constraint_count {
                    return Err("analytic joint derivative dimension mismatch".into());
                }
                gradient[j] = report.constraints.objective[0] * column[0] * width;
                jacobian.extend(self.rows[j].iter().map(|row| column[1 + row] * width));
                // Check the analytic implementation against the structural
                // declaration without dropping numerically zero entries.
                let declared = self.rows[j]
                    .iter()
                    .copied()
                    .collect::<std::collections::BTreeSet<_>>();
                if column[1..]
                    .iter()
                    .enumerate()
                    .any(|(r, v)| !declared.contains(&r) && *v != 0.)
                {
                    return Err("analytic force derivative outside declared sparsity".into());
                }
                continue;
            }
            let mut sides = [None, None];
            for (slot, sign) in [-1., 1.].into_iter().enumerate() {
                let mut h = self.config.difference_step;
                for _ in 0..12 {
                    if self.budget_exhausted {
                        return Err(
                            "joint CAD evaluation budget exhausted during derivatives".into()
                        );
                    }
                    let mut probe = x.to_vec();
                    probe[j] = (x[j] + sign * h).clamp(0., 1.);
                    let actual = probe[j] - x[j];
                    if actual.abs() < 1e-14 {
                        break;
                    }
                    match self.evaluate_full(&probe) {
                        Ok(value) => {
                            sides[slot] = Some((actual, value));
                            break;
                        }
                        Err(_) => h *= 0.5,
                    }
                }
            }
            let difference = |row: Option<usize>| -> Result<f64, String> {
                let value = |v: &NlpEvaluation| row.map_or(v.objective, |i| v.constraints[i]);
                match (&sides[0], &sides[1]) {
                    (Some((a, ra)), Some((b, rb))) => Ok((value(rb) - value(ra)) / (b - a)),
                    (Some((h, r)), None) | (None, Some((h, r))) => Ok((value(r)
                        - row.map_or(base_objective, |i| report.constraints.inequalities[i]))
                        / h),
                    _ => Err(format!(
                        "no valid numerical derivative for joint variable {j}"
                    )),
                }
            };
            gradient[j] = difference(None)?;
            for &row in &self.rows[j] {
                jacobian.push(difference(Some(row))?);
            }
        }
        Ok(NlpDerivatives {
            objective_gradient: gradient,
            constraint_jacobian: jacobian,
        })
    }
    fn intermediate(&mut self, _: &sim_solve::ipopt::IpoptIteration) -> bool {
        !self.budget_exhausted
    }
}
impl ContactPlanner<'_> {
    /// Opt-in sparse NLP for the same physical joint problem and speed cost.
    /// Requires contact-relative knots so event ordering remains a fixed local
    /// domain. Optional row projection retains all original physical checks,
    /// with collision-free geometry enforced as an evaluation domain.
    pub fn optimize_joint_ipopt(
        &self,
        initial: &JointContactMotion,
        variables: &[JointContactVariable],
        config: &JointIpoptConfig,
        library: &IpoptLibrary,
        mut progress: impl FnMut(&JointContactReport),
    ) -> Result<JointIpoptOptimization, String> {
        self.optimize_joint_ipopt_with_observer(initial, variables, config, library, |_, report| {
            progress(report)
        })
    }

    /// Same numerical solve with an immutable candidate/report observer.
    /// Observed points can include derivative probes and rejected NLP trials;
    /// their full sampled physical report, not solver acceptance, defines scope.
    /// The observer receives immutable references and returns no solver inputs.
    /// Observers must not re-enter this planner or mutate its shared caches.
    pub fn optimize_joint_ipopt_with_observer(
        &self,
        initial: &JointContactMotion,
        variables: &[JointContactVariable],
        config: &JointIpoptConfig,
        library: &IpoptLibrary,
        mut progress: impl FnMut(&JointContactMotion, &JointContactReport),
    ) -> Result<JointIpoptOptimization, String> {
        if initial.force_timing.is_none() {
            return Err("native joint NLP requires explicit contact-relative force timing".into());
        }
        if config.maximum_model_evaluations < 4
            || config.maximum_model_evaluations > 10_000_000
            || !config.difference_step.is_finite()
            || config.difference_step <= 0.
            || config.difference_step > 0.1
        {
            return Err("invalid joint NLP model budget or difference step".into());
        }
        let initial_report = self.evaluate_joint(initial)?;
        let values = joint_values(initial, variables, self.recipe.direction_world)?;
        if variables.is_empty()
            || values.iter().zip(variables).any(|(x, v)| {
                !v.bound.lower.is_finite()
                    || !v.bound.upper.is_finite()
                    || v.bound.lower > v.bound.upper
                    || !(v.bound.upper - v.bound.lower).is_finite()
                    || *x < v.bound.lower
                    || *x > v.bound.upper
            })
        {
            return Err("invalid joint NLP variable bounds or initial values".into());
        }
        let (rows, projection) = structural_rows(
            self,
            initial,
            variables,
            initial_report.constraints.inequalities.len(),
            config.collision_free_domain,
        )?;
        projection.check(&initial_report.constraints.inequalities)?;
        let pattern = rows
            .iter()
            .enumerate()
            .flat_map(|(c, rs)| rs.iter().map(move |r| (*r, c)))
            .map(|(r, c)| (projection.native_rows[r].expect("retained row"), c))
            .collect::<Vec<_>>();
        let normalized = values
            .iter()
            .zip(variables)
            .map(|(v, b)| {
                if b.bound.upper == b.bound.lower {
                    0.
                } else {
                    (v - b.bound.lower) / (b.bound.upper - b.bound.lower)
                }
            })
            .collect::<Vec<_>>();
        let normalized_bounds = variables
            .iter()
            .map(|v| VariableBound {
                lower: 0.,
                upper: if v.bound.upper == v.bound.lower {
                    0.
                } else {
                    1.
                },
            })
            .collect::<Vec<_>>();
        let constraints = vec![
            VariableBound {
                lower: f64::NEG_INFINITY,
                upper: 0.
            };
            projection.count()
        ];
        let mut model = JointNlp {
            planner: self,
            initial,
            variables,
            config,
            rows: &rows,
            constraint_count: projection.full_count,
            projection: &projection,
            model_evaluations: 1,
            budget_exhausted: false,
            grouped_body_probes: 0,
            body_group_fallbacks: 0,
            best: None,
            progress: &mut progress,
        };
        model.observe(initial, &initial_report)?;
        let search = library.solve(
            &normalized,
            &normalized_bounds,
            &constraints,
            &pattern,
            &config.solver,
            &mut model,
        )?;
        let (candidate, report, returned_candidate_error) = match model.decode(&search.values) {
            Ok(candidate) => {
                model.model_evaluations += 1;
                match self.evaluate_joint_uncached(&candidate) {
                    Ok(report) => {
                        model.observe(&candidate, &report)?;
                        let error = projection.check(&report.constraints.inequalities).err();
                        (Some(candidate), Some(report), error)
                    }
                    Err(e) => (Some(candidate), None, Some(e)),
                }
            }
            Err(e) => (None, None, Some(e)),
        };
        Ok(JointIpoptOptimization {
            candidate,
            report,
            returned_candidate_error,
            search,
            best_sampled_feasible: model.best.map(|(_, c)| c),
            model_evaluations: model.model_evaluations,
            model_budget_exhausted: model.budget_exhausted,
            jacobian_nonzeros: pattern.len(),
            dense_jacobian_entries: constraints.len() * variables.len(),
            initial_report,
            grouped_body_probes: model.grouped_body_probes,
            body_group_fallbacks: model.body_group_fallbacks,
            constraint_projection: projection,
            scope: "Same joint CAD motion/forces, target-speed cost and every physical acceptance check; optional topology-derived zero endpoint elimination and strict sampled collision-free evaluation domain. Normalized variables, structural sparse Jacobian, analytic force columns and numerical motion columns; native Ipopt limited-memory Hessian. Fixed contact-event ordering. Returned candidate independently audited in full CAD. Local sampled optimization, not global speed, continuous collision, runtime, browser or sim-to-real certification.",
        })
    }
}

#[cfg(test)]
mod projection_tests {
    use super::*;

    #[test]
    fn projection_preserves_physical_acceptance_and_row_identity() {
        let projection = ConstraintProjection::new(6, vec![1, 5], vec![3]).unwrap();
        assert_eq!(
            projection.native_rows,
            vec![Some(0), None, Some(1), None, Some(2), None]
        );
        // Every retained inequality keeps its value, including a zero that
        // happens to occur only at this candidate.
        for x in [-2., 0., 2.] {
            let full = [x, -0., 0., 0., -1., 0.];
            let native = projection.project(&full).unwrap();
            assert_eq!(native, vec![x, 0., -1.]);
            assert_eq!(
                full.iter().all(|r| *r <= 0.),
                native.iter().all(|r| *r <= 0.)
            );
        }
        // A collision must fail even when far below any normalized tolerance.
        assert!(projection.project(&[-1., 0., 0., 1e-20, -1., 0.]).is_err());
        assert!(projection.project(&[-1., 1e-20, 0., 0., -1., 0.]).is_err());
        assert!(
            projection
                .project(&[-1., 0., f64::NAN, 0., -1., 0.])
                .is_err()
        );
        assert!(projection.project(&[0.; 5]).is_err());
        assert!(ConstraintProjection::new(6, vec![1], vec![1]).is_err());
        assert!(ConstraintProjection::new(6, vec![6], vec![]).is_err());
    }

    #[test]
    fn unprojected_mode_preserves_collision_residual_for_legacy_solver() {
        let projection = ConstraintProjection::new(3, vec![], vec![]).unwrap();
        let full = [-1., 0., 0.2];
        assert_eq!(projection.project(&full).unwrap(), full);
        assert_eq!(projection.count(), 3);
    }
}
