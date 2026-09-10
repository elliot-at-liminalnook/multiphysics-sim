//! Shared fixed-motion whole-force-trajectory optimization.
use super::*;
use nalgebra::{DMatrix, DVector};
use sim_solve::conic::{Cone, ConicConfig, ConicResult, solve_linear_conic};
#[derive(Debug, Serialize)]
pub struct JointForceConicOptimization {
    pub search: ConicResult,
    pub config: ConicConfig,
    pub force_variables: usize,
    pub balance_rows: usize,
    pub cone_count: usize,
    pub friction: f64,
    pub candidate: Option<JointContactMotion>,
    pub report: Option<JointContactReport>,
    pub independent_affine_error: Option<f64>,
    pub force_box_violation_n: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub servo_command_rows: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub independent_servo_command_affine_error: Option<f64>,
    pub scope: &'static str,
}
impl ContactPlanner<'_> {
    /// Optimize selected force coefficients with fixed motion and exact circular friction cones.
    /// Motion decisions are ignored; every original force bound and nonselected force is retained.
    /// Original-coordinate physical gates are audited separately from native convergence.
    pub fn optimize_joint_forces_conic(
        &self,
        initial: &JointContactMotion,
        variables: &[JointContactVariable],
        config: &ConicConfig,
    ) -> Result<JointForceConicOptimization, String> {
        let mu = self.art.model.world.floor_friction;
        if !mu.is_finite() || mu < 0. {
            return Err("finite nonnegative model friction required".into());
        }
        let variables = variables
            .iter()
            .filter(|v| matches!(v.decision, JointContactDecision::Force { .. } | JointContactDecision::AdditionalForce { .. }))
            .cloned()
            .collect::<Vec<_>>();
        let mut zero = initial.clone();
        let mut columns = BTreeMap::new();
        for (j, v) in variables.iter().enumerate() {
            let Some(ForceNode { clock, slot, node, axis, .. }) = v.decision.force_node(&initial.motion)?
            else {
                unreachable!()
            };
            if !v.bound.lower.is_finite()
                || !v.bound.upper.is_finite()
                || v.bound.lower > v.bound.upper
            {
                return Err("invalid force bounds".into());
            }
            if columns.insert((clock, slot, node, axis), j).is_some() {
                return Err("duplicate force decision".into());
            }
            *zero
                .force_templates
                .get_mut(clock)
                .and_then(|c| c.get_mut(slot))
                .and_then(|t| t.keyframes.get_mut(node))
                .and_then(|k| k.values.get_mut(axis))
                .ok_or("invalid force decision")? = 0.;
        }
        let residual = |r: &JointContactReport| {
            DVector::from_iterator(
                r.motion_report.frames.len() * 6,
                r.motion_report.frames.iter().flat_map(|f| {
                    f.wrench_residual.iter().enumerate().map(|(i, r)| {
                        r / if i < 3 {
                            self.recipe.force_tolerance_n
                        } else {
                            self.recipe.moment_tolerance_nm
                        }
                    })
                }),
            )
        };
        let (baseline, balance) = self.joint_balance_jacobian(&zero, &variables)?;
        let offset = residual(&baseline);
        let commands = if self.recipe.servo_command_limits.is_some() {
            let (report, matrix) = self.joint_servo_command_jacobian(&zero, &variables)?;
            Some((matrix, command_residual(&report)?))
        } else {
            None
        };
        let n = variables.len();
        let mut node_cones = Vec::new();
        for (ci, clock) in zero.force_templates.iter().enumerate() {
            for (fi, foot) in clock.iter().enumerate() {
                for (ki, key) in foot.keyframes.iter().enumerate() {
                    let indices = std::array::from_fn::<_, 3, _>(|axis| {
                        columns.get(&(ci, fi, ki, axis)).copied()
                    });
                    if indices.iter().all(Option::is_none) && key.values.iter().all(|x| *x == 0.) {
                        continue;
                    }
                    node_cones.push((indices, [key.values[0], key.values[1], key.values[2]]));
                }
            }
        }
        let (a, b, cones) = force_conic_constraints(
            &balance,
            &offset,
            &variables,
            &node_cones,
            mu,
            commands.as_ref().map(|(a, b)| (a, b)),
        );
        let mut q = DVector::zeros(n + 1);
        q[n] = 1.;
        let search = solve_linear_conic(&q, &a, &b, &cones, config)?;
        // A failed native status is retained. Never reinterpret an infeasibility ray as forces.
        let (candidate, report, affine_error, box_violation) = if search.has_primal_candidate {
            let mut fitted = zero.clone();
            let mut box_violation = 0.0_f64;
            for (j, v) in variables.iter().enumerate() {
                let Some(ForceNode { clock, slot, node, axis, .. }) = v.decision.force_node(&initial.motion)?
                else {
                    unreachable!()
                };
                let x = search.values[j];
                fitted.force_templates[clock][slot].keyframes[node].values[axis] = x;
                box_violation = box_violation.max(v.bound.lower - x).max(x - v.bound.upper);
            }
            let report = self.evaluate_joint_uncached(&fitted)?;
            let predicted = &balance * DVector::from_column_slice(&search.values[..n]) + &offset;
            let error = (residual(&report) - predicted).amax();
            if !error.is_finite() || error > 1e-8 {
                return Err(format!(
                    "conic forces disagree with independent CAD map: {error}"
                ));
            }
            (Some(fitted), Some(report), Some(error), Some(box_violation))
        } else {
            (None, None, None, None)
        };

        let command_error = if let (Some(report), Some((matrix, offset))) = (&report, &commands) {
            let predicted = matrix * DVector::from_column_slice(&search.values[..n]) + offset;
            let error = (command_residual(report)? - predicted).amax();
            if !error.is_finite() || error > 1e-8 {
                return Err(format!(
                    "conic command map disagrees with independent CAD audit: {error}"
                ));
            }
            Some(error)
        } else {
            None
        };
        Ok(JointForceConicOptimization {
            search,
            config: config.clone(),
            force_variables: n,
            balance_rows: offset.len(),
            cone_count: node_cones.len(),
            friction: mu,
            candidate,
            report,
            independent_affine_error: affine_error,
            force_box_violation_n: box_violation,
            servo_command_rows: commands.as_ref().map(|(_, b)| b.len()),
            independent_servo_command_affine_error: command_error,
            scope: if commands.is_some() {
                "Convex fixed-motion force minimax balance with original force boxes, circular friction cones and hard explicit nominal servo-command limits. Torque-speed and geometry constraints are independently audited, not enforced by this conic subproblem. Floating-point primal/dual evidence is not a global speed or runtime certificate. No projection or physical tolerance change."
            } else {
                "Convex whole-force-curve minimax balance at fixed CAD motion with original force boxes and circular node friction cones. Motor and geometry constraints are omitted from optimization but independently audited on a returned primal candidate. Native primal/dual diagnostics are floating-point evidence, not interval certificates, global robot speed limits or runtime validation. No force projection or physical tolerance changes."
            },
        })
    }
}

impl JointContactMotion {
    /// Enumerate every interior force coefficient using explicitly supplied
    /// per-clock/per-foot xyz bounds in N. No robot properties are inferred.
    pub fn force_variables_for_bounds(
        &self,
        bounds: &[Vec<[sim_solve::least_squares::VariableBound; 3]>],
    ) -> Result<Vec<JointContactVariable>, String> {
        let resolved = self.resolved_force_timing()?;
        if bounds.len() != resolved.force_templates.len() {
            return Err("force-bound clock count mismatch".into());
        }
        let mut variables = Vec::new();
        let slots = force_slots(&resolved.motion);
        for (clock, (templates, limits)) in resolved.force_templates.iter().zip(bounds).enumerate()
        {
            PreparedForces::new(&resolved.motion, templates)?;
            if limits.len() != templates.len() {
                return Err("force-bound foot count mismatch".into());
            }
            for (foot, (template, axes)) in templates.iter().zip(limits).enumerate() {
                for bound in axes {
                    if !bound.lower.is_finite()
                        || !bound.upper.is_finite()
                        || bound.lower > bound.upper
                    {
                        return Err("finite ordered force bounds required".into());
                    }
                }
                for node in 1..template.keyframes.len() - 1 {
                    for (axis, bound) in axes.iter().enumerate() {
                        variables.push(JointContactVariable {
                            decision: slots[foot].decision(clock, node, axis),
                            bound: bound.clone(),
                        });
                    }
                }
            }
        }
        Ok(variables)
    }
}

fn command_residual(report: &JointContactReport) -> Result<DVector<f64>, String> {
    let mut values = Vec::new();
    for frame in &report.motion_report.frames {
        values.extend_from_slice(
            &frame
                .servo_command
                .as_ref()
                .ok_or("missing servo-command audit")?
                .inequalities,
        );
    }
    Ok(DVector::from_vec(values))
}

fn force_conic_constraints(
    balance: &DMatrix<f64>,
    offset: &DVector<f64>,
    variables: &[JointContactVariable],
    node_cones: &[([Option<usize>; 3], [f64; 3])],
    mu: f64,
    hard_linear: Option<(&DMatrix<f64>, &DVector<f64>)>,
) -> (DMatrix<f64>, DVector<f64>, Vec<Cone>) {
    let n = variables.len();
    let base_rows = 2 * offset.len() + 2 * n + 1;
    // At zero friction the Lorentz cone no longer constrains the normal
    // sign. Retain the independent unilateral law explicitly.
    let hard_start = base_rows + if mu == 0. { node_cones.len() } else { 0 };
    let linear_rows = hard_start + hard_linear.map_or(0, |(_, b)| b.len());
    let mut a = DMatrix::zeros(linear_rows + 3 * node_cones.len(), n + 1);
    let mut b = DVector::zeros(a.nrows());
    for i in 0..offset.len() {
        for j in 0..n {
            a[(2 * i, j)] = balance[(i, j)];
            a[(2 * i + 1, j)] = -balance[(i, j)];
        }
        a[(2 * i, n)] = -1.;
        a[(2 * i + 1, n)] = -1.;
        b[2 * i] = -offset[i];
        b[2 * i + 1] = offset[i];
    }
    for (j, v) in variables.iter().enumerate() {
        let r = 2 * offset.len() + 2 * j;
        a[(r, j)] = 1.;
        b[r] = v.bound.upper;
        a[(r + 1, j)] = -1.;
        b[r + 1] = -v.bound.lower;
    }
    a[(base_rows - 1, n)] = -1.;
    if mu == 0. {
        for (k, (indices, constant)) in node_cones.iter().enumerate() {
            b[base_rows + k] = constant[2];
            if let Some(j) = indices[2] {
                a[(base_rows + k, j)] = -1.;
            }
        }
    }
    if let Some((matrix, offset)) = hard_linear {
        for i in 0..offset.len() {
            b[hard_start + i] = -offset[i];
            for j in 0..n {
                a[(hard_start + i, j)] = matrix[(i, j)];
            }
        }
    }
    let mut cones = vec![Cone::Nonnegative(linear_rows)];
    for (k, (indices, constant)) in node_cones.iter().enumerate() {
        let r = linear_rows + 3 * k;
        for (row, axis, scale) in [(r, 2, mu), (r + 1, 0, 1.), (r + 2, 1, 1.)] {
            b[row] = scale * constant[axis];
            if let Some(j) = indices[axis] {
                a[(row, j)] = -scale;
            }
        }
        cones.push(Cone::SecondOrder(3));
    }
    (a, b, cones)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hard_command_rows_limit_the_force_solution_and_cannot_be_paid_for_by_minimax_slack() {
        let variables = vec![JointContactVariable {
            decision: JointContactDecision::Force {
                clock: 0,
                foot: 0,
                node: 1,
                axis: 2,
            },
            bound: sim_solve::least_squares::VariableBound {
                lower: 0.,
                upper: 2.,
            },
        }];
        let balance = DMatrix::from_element(1, 1, 1.);
        let offset = DVector::from_vec(vec![-1.]);
        // A normalized nominal-command constraint f - 0.5 <= 0.
        let command = DMatrix::from_element(1, 1, 1.);
        let command_offset = DVector::from_vec(vec![-0.5]);
        let solve = |matrix: &DMatrix<f64>, constant: &DVector<f64>| {
            let (a, b, cones) = force_conic_constraints(
                &balance,
                &offset,
                &variables,
                &[([None, None, Some(0)], [0.; 3])],
                0.,
                Some((matrix, constant)),
            );
            solve_linear_conic(
                &DVector::from_vec(vec![0., 1.]),
                &a,
                &b,
                &cones,
                &ConicConfig::default(),
            )
            .unwrap()
        };
        let fit = solve(&command, &command_offset);
        assert!(fit.solved);
        assert!((fit.values[0] - 0.5).abs() < 1e-8);
        assert!((fit.values[1] - 0.5).abs() < 1e-8);
        // No force choice can repair a positive constant command violation;
        // even arbitrarily large balance slack must not make this feasible.
        let rejected = solve(&DMatrix::zeros(1, 1), &DVector::from_vec(vec![0.1]));
        assert_eq!(rejected.status, "PrimalInfeasible");
        assert!(!rejected.has_primal_candidate);
    }

    #[test]
    fn frictionless_support_cannot_pull_even_when_the_search_box_allows_negative_normals() {
        let variables = vec![JointContactVariable {
            decision: JointContactDecision::Force {
                clock: 0,
                foot: 0,
                node: 1,
                axis: 2,
            },
            bound: sim_solve::least_squares::VariableBound {
                lower: -1.,
                upper: 1.,
            },
        }];
        // A requested -1 N normal load is incompatible with unilateral support.
        // Minimax residual is 1, rather than zero obtained by pulling at -1 N.
        let (a, b, cones) = force_conic_constraints(
            &DMatrix::from_element(1, 1, 1.),
            &DVector::from_vec(vec![1.]),
            &variables,
            &[([None, None, Some(0)], [0.; 3])],
            0.,
            None,
        );
        let result = solve_linear_conic(
            &DVector::from_vec(vec![0., 1.]),
            &a,
            &b,
            &cones,
            &ConicConfig::default(),
        )
        .unwrap();
        assert!(result.solved);
        assert!(result.values[0].abs() < 1e-8);
        assert!((result.values[1] - 1.).abs() < 1e-8);
        assert!(
            point_force_cone_inequalities([0., 0., result.values[0]], 0.)
                .unwrap()
                .iter()
                .all(|v| *v < 1e-8)
        );
    }
}
