//! Fixed-motion affine relaxation: can this force basis balance the CAD model?
use nalgebra::{DMatrix, DVector};
use serde::Deserialize;
use sim_runtime::{
    contact_planning::{
        ContactPlanRecipe, ContactPlanner, JointContactDecision, JointContactMotion,
        JointContactReport, JointContactVariable,
    },
    session::{Scene, Session},
    tracking::CaptureConfig,
};
use sim_solve::affine_feasibility::affine_residual_lower_bound;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    robot: ContactPlanRecipe,
    candidate: JointContactMotion,
    variables: Vec<JointContactVariable>,
    #[serde(rename = "search")]
    _search: serde_json::Value,
}
fn set(c: &mut JointContactMotion, d: &JointContactDecision, value: f64) -> Result<(), String> {
    if let JointContactDecision::Force {
        clock,
        foot,
        node,
        axis,
    } = *d
    {
        let v = c
            .force_templates
            .get_mut(clock)
            .and_then(|v| v.get_mut(foot))
            .and_then(|v| v.keyframes.get_mut(node))
            .and_then(|v| v.values.get_mut(axis))
            .ok_or("invalid force decision")?;
        *v = value;
        Ok(())
    } else {
        Err("force decision required".into())
    }
}
fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 3
        && !(args.len() == 4
            && matches!(
                args[3].as_str(),
                "--analytic-basis" | "--analytic-box-audit"
            ))
    {
        return Err(
            "usage: audit_joint_force_basis scene.json markers.json recipe.json [--analytic-basis|--analytic-box-audit]"
                .into(),
        );
    }
    let read = |p: &str| std::fs::read(p).map_err(|e| e.to_string());
    let scene: Scene = serde_json::from_slice(&read(&args[0])?).map_err(|e| e.to_string())?;
    let markers: CaptureConfig =
        serde_json::from_slice(&read(&args[1])?).map_err(|e| e.to_string())?;
    let recipe: Recipe = serde_json::from_slice(&read(&args[2])?).map_err(|e| e.to_string())?;
    if recipe.candidate.motion.feet.iter().any(|f| !f.additional_steps.is_empty()) {
        return Err("this legacy audit/initialization CLI accepts one stance per foot; use the per-stance shared APIs and updated Jacobian audits".into());
    }
    let mut session = Session::new(scene, 0)?;
    session.robot.art.contact_on = false;
    let seed = session.robot.generalized();
    let planner = ContactPlanner::new(&session.robot.art, &seed, &markers, recipe.robot.clone())?;
    let variables = recipe
        .variables
        .iter()
        .filter(|v| matches!(v.decision, JointContactDecision::Force { .. }))
        .collect::<Vec<_>>();
    if variables.is_empty() {
        return Err("force variables required".into());
    }
    let residual = |r: &JointContactReport| {
        DVector::from_iterator(
            r.motion_report.frames.len() * 6,
            r.motion_report.frames.iter().flat_map(|f| {
                f.wrench_residual.iter().enumerate().map(|(i, r)| {
                    r / if i < 3 {
                        recipe.robot.force_tolerance_n
                    } else {
                        recipe.robot.moment_tolerance_nm
                    }
                })
            }),
        )
    };
    let mut zero = recipe.candidate.clone();
    for v in &variables {
        set(&mut zero, &v.decision, 0.0)?;
    }
    let analytic = args.len() == 4;
    let box_audit = args.get(3).is_some_and(|s| s == "--analytic-box-audit");
    let (zero_report, mut a) = if analytic {
        planner.joint_balance_jacobian(
            &zero,
            &variables.iter().map(|v| (*v).clone()).collect::<Vec<_>>(),
        )?
    } else {
        (planner.evaluate_joint(&zero)?, DMatrix::zeros(0, 0))
    };
    let b = residual(&zero_report);
    if !analytic {
        a = DMatrix::zeros(b.len(), variables.len());
    }
    for (j, v) in variables.iter().enumerate().filter(|_| !analytic) {
        let mut c = zero.clone();
        set(&mut c, &v.decision, 1.0)?;
        let r = residual(&planner.evaluate_joint(&c)?);
        a.set_column(j, &(r - &b));
    }
    let bound = affine_residual_lower_bound(
        &a,
        &b,
        &variables
            .iter()
            .map(|v| v.bound.clone())
            .collect::<Vec<_>>(),
    )?;
    let mut fitted = zero;
    for (v, x) in variables.iter().zip(&bound.least_squares_values) {
        set(&mut fitted, &v.decision, *x)?;
    }
    let fit_report = planner.evaluate_joint_uncached(&fitted)?;
    let measured = residual(&fit_report);
    let affine = DVector::from_column_slice(&bound.least_squares_residuals);
    let error = (&measured - affine).amax();
    let least_squares_inside_box = bound
        .least_squares_values
        .iter()
        .zip(&variables)
        .all(|(x, v)| *x >= v.bound.lower && *x <= v.bound.upper);
    if !error.is_finite() || (error > 1e-8 && (!box_audit || least_squares_inside_box)) {
        let largest_force = bound
            .least_squares_values
            .iter()
            .map(|x| x.abs())
            .fold(0.0_f64, f64::max);
        let largest_bound = variables
            .iter()
            .map(|v| v.bound.lower.abs().max(v.bound.upper.abs()))
            .fold(0.0_f64, f64::max);
        return Err(format!(
            "affine relaxation disagrees with independent CAD evaluation by {error}; maximum absolute LS force {largest_force} N versus largest box endpoint {largest_bound} N"
        ));
    }
    let mut contract_check = serde_json::Value::Null;
    let mut box_probes = Vec::new();
    if box_audit {
        // Partial, reordered decisions must retain every unselected force in
        // the affine offset. This also checks both clock blocks independently.
        let selected = variables
            .iter()
            .rev()
            .step_by(2)
            .map(|v| (*v).clone())
            .collect::<Vec<_>>();
        let (initial, selected_a) = planner.joint_balance_jacobian(&recipe.candidate, &selected)?;
        let mut changed = recipe.candidate.clone();
        let mut delta = DVector::zeros(selected.len());
        for (j, v) in selected.iter().enumerate() {
            let JointContactDecision::Force {
                clock,
                foot,
                node,
                axis,
            } = v.decision
            else {
                unreachable!()
            };
            let old = changed.force_templates[clock][foot].keyframes[node].values[axis];
            let new = if j % 2 == 0 {
                v.bound.lower
            } else {
                v.bound.upper
            };
            delta[j] = new - old;
            set(&mut changed, &v.decision, new)?;
        }
        let actual = residual(&planner.evaluate_joint_uncached(&changed)?);
        let error = (&actual - (residual(&initial) + selected_a * delta)).amax();
        if !error.is_finite() || error > 1e-8 {
            return Err(format!("partial reordered balance map failed: {error}"));
        }
        let mut invalid = selected[0].clone();
        invalid.decision = JointContactDecision::Force {
            clock: usize::MAX,
            foot: 0,
            node: 1,
            axis: 0,
        };
        for invalid_variables in [
            vec![],
            vec![selected[0].clone(), selected[0].clone()],
            vec![invalid],
        ] {
            if planner
                .joint_balance_jacobian(&recipe.candidate, &invalid_variables)
                .is_ok()
            {
                return Err("balance map accepted invalid decisions".into());
            }
        }
        if let Some(motion) = recipe
            .variables
            .iter()
            .find(|v| matches!(v.decision, JointContactDecision::Motion { .. }))
        {
            if planner
                .joint_balance_jacobian(&recipe.candidate, &[motion.clone()])
                .is_ok()
            {
                return Err("balance map accepted motion decision".into());
            }
        }
        contract_check = serde_json::json!({"selected_reordered_columns":selected.len(), "maximum_normalized_affine_error":error, "empty_duplicate_out_of_range_rejected":true, "motion_decision_present_and_rejected":recipe.variables.iter().any(|v| matches!(v.decision, JointContactDecision::Motion {..}))});
        for kind in [
            "lower",
            "upper",
            "alternating",
            "reverse_alternating",
            "midpoint",
        ] {
            let values = DVector::from_iterator(
                variables.len(),
                variables.iter().enumerate().map(|(j, v)| match kind {
                    "lower" => v.bound.lower,
                    "upper" => v.bound.upper,
                    "alternating" => {
                        if j % 2 == 0 {
                            v.bound.lower
                        } else {
                            v.bound.upper
                        }
                    }
                    "reverse_alternating" => {
                        if j % 2 == 0 {
                            v.bound.upper
                        } else {
                            v.bound.lower
                        }
                    }
                    _ => v.bound.lower + 0.5 * (v.bound.upper - v.bound.lower),
                }),
            );
            let mut probe = recipe.candidate.clone();
            for (v, x) in variables.iter().zip(values.iter()) {
                set(&mut probe, &v.decision, *x)?;
            }
            let actual = residual(&planner.evaluate_joint_uncached(&probe)?);
            let predicted = &a * &values + &b;
            let error = (&actual - &predicted).amax();
            if !error.is_finite() || error > 1e-8 {
                return Err(format!(
                    "bounded affine validation failed at {kind}: {error}"
                ));
            }
            if actual.amax() + 1e-8 < bound.maximum_residual_lower_bound {
                return Err("dual lower bound exceeds bounded physical probe".into());
            }
            box_probes.push(serde_json::json!({"kind":kind,"maximum_normalized_affine_error":error,"maximum_measured_residual":actual.amax()}));
        }
    }
    let mut worst = (0..b.len()).collect::<Vec<_>>();
    worst.sort_by(|i, j| {
        bound.dual_weights[*j]
            .abs()
            .total_cmp(&bound.dual_weights[*i].abs())
    });
    let rows=worst.into_iter().take(12).map(|i|{
        let f=&zero_report.motion_report.frames[i/6];
        serde_json::json!({"row":i,"phase":f.time_s/recipe.candidate.motion.period_s,"clock":f.clock,
            "axis":i%6,"normalized_ls_residual":bound.least_squares_residuals[i],"dual_weight":bound.dual_weights[i]})
    }).collect::<Vec<_>>();
    println!(
        "{}",
        serde_json::json!({"rows":b.len(),"columns":variables.len(),
        "speed_m_s":zero_report.motion_report.speed_m_s,"basis_method":if analytic {"analytic CAD balance map"} else {"unit-force report subtraction"},"bound":bound,"largest_dual_rows":rows,
        "independent_affine_error":error,"fitted_candidate":fitted,
        "least_squares_inside_variable_box":least_squares_inside_box,
        "maximum_absolute_least_squares_force_n":bound.least_squares_values.iter().map(|v|v.abs()).fold(0.0_f64,f64::max),
        "bounded_validation_tolerance":if box_audit {Some(1e-8)} else {None},"box_probes":box_probes,"contract_check":contract_check,
        "fitted_physical_report":{"force_n":fit_report.motion_report.maximum_force_error_n,
            "moment_nm":fit_report.motion_report.maximum_moment_error_nm,
            "torque_margin_nm":fit_report.motion_report.minimum_torque_margin_nm,
            "cone_violation_n":fit_report.maximum_cone_violation_n,"sampled_feasible":fit_report.sampled_feasible},
        "cache_statistics":planner.joint_cache_statistics(),
        "scope":"Fixed motion and force basis only. Normalized balance rows must all have magnitude <=1. Dual bound applies to the affine model within the declared force-variable box, even before friction/actuator constraints. Analytic box-audit mode independently verifies five bounded probes at 1e-8 tolerance and retains any failed out-of-box LS extrapolation separately; these probes and floating-point linear algebra are not interval arithmetic or a global robot speed limit. Least-squares forces may violate their bounds or cones and are not a qualified controller."})
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
