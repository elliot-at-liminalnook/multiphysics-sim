//! Structural audit plus independent iterative-chart comparison; no simulation.
use serde_json::json;
use sim_domain_robot::articulated::embedding::{DependentSolve, EmbeddingConfig, RigidEmbedding};
use sim_runtime::session::{Scene, Session};
use std::collections::BTreeSet;

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 1 {
        return Err("usage: audit_analytic_mechanisms scene.json".into());
    }
    let scene: Scene = serde_json::from_slice(&std::fs::read(&args[0]).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let session = Session::new(scene, 0)?;
    let art = &session.robot.art;
    let dofs: Vec<_> = art.dofs().collect();
    let independent: Vec<_> = session
        .scene
        .robot
        .motors
        .iter()
        .map(|m| {
            let found: Vec<_> = dofs
                .iter()
                .enumerate()
                .filter(|(_, (j, _))| Some(&j.name) == m.joint.as_ref())
                .map(|(i, _)| i)
                .collect();
            if found.len() != 1 {
                Err(format!("motor {} must name one joint coordinate", m.name))
            } else {
                Ok(found[0])
            }
        })
        .collect::<Result<_, _>>()?;
    let selected: BTreeSet<_> = independent.iter().copied().collect();
    if selected.len() != independent.len() {
        return Err("duplicate motor coordinates".into());
    }
    let audit = art.audit_slider_cranks();
    let mut issues = Vec::new();
    let mut covered = BTreeSet::new();
    for a in &audit {
        if let Some(c) = &a.candidate {
            if !selected.contains(&c.dof_indices[0]) {
                issues.push(format!("{} crank is not independent", a.loop_name));
            }
            for i in &c.dof_indices[1..] {
                if selected.contains(i) || !covered.insert(*i) {
                    issues.push(format!(
                        "{} has overlapping/nondependent coordinate {i}",
                        a.loop_name
                    ));
                }
            }
        } else {
            issues.push(format!("{}: {:?}", a.loop_name, a.rejection));
        }
    }
    for t in &art.transmissions {
        if !selected.contains(&t.driver)
            || selected.contains(&t.driven)
            || !covered.insert(t.driven)
        {
            issues.push(format!(
                "{} is not a disjoint independent-to-dependent transmission",
                t.name
            ));
        }
    }
    let expected: BTreeSet<_> = (0..dofs.len()).filter(|i| !selected.contains(i)).collect();
    if covered != expected {
        issues.push(
            "analytic mechanisms do not cover every dependent coordinate exactly once".into(),
        );
    }
    let mut report = json!({"source":session.scene.robot.source,"loops":audit,"transmissions":art.transmissions.iter().map(|t|json!({"name":t.name,"driver":dofs[t.driver].1.name,"driven":dofs[t.driven].1.name,"ratio":t.ratio})).collect::<Vec<_>>(),
        "independent_coordinates":independent.iter().map(|i|&dofs[*i].1.name).collect::<Vec<_>>(),"dependent_count":expected.len(),"covered_count":covered.len(),"issues":issues,
        "scope":"Structural geometry and kinematics only. Sweep is not a physical motor travel limit, collision check, controller, dynamic simulation or performance promotion."});
    if issues.is_empty() {
        let names: Vec<_> = independent
            .iter()
            .map(|i| dofs[*i].1.name.clone())
            .collect();
        let map = RigidEmbedding::new(
            art,
            &names,
            EmbeddingConfig {
                direct_closure_jacobian: true,
                dependent_solve: DependentSolve::PivotedQr,
                ..Default::default()
            },
        )?;
        let mut seed = session.robot.generalized();
        let nb = map.reduced_dimension() - independent.len();
        let mut max_q = 0.0_f64;
        let mut max_v = 0.0_f64;
        let mut max_a = 0.0_f64;
        let mut closure = [0.0_f64; 3];
        let mut minimum_projection = f64::INFINITY;
        let charts: Vec<_> = audit
            .iter()
            .map(|a| a.candidate.as_ref().unwrap())
            .collect();
        let mut samples = Vec::new();
        for k in 0..81 {
            let theta = -1.2 + 2.4 * k as f64 / 80.0;
            let mut candidate = session.robot.generalized();
            candidate.q.fill(0.0);
            candidate.qd.fill(0.0);
            candidate.qdd.fill(0.0);
            let mut velocity = vec![0.0; map.reduced_dimension()];
            let positions: Vec<_> = independent
                .iter()
                .enumerate()
                .map(|(j, &i)| {
                    let crank = charts.iter().any(|c| c.dof_indices[0] == i);
                    let q = if crank { theta } else { 0.1 * theta };
                    let v = if crank { 0.7 } else { 0.11 };
                    candidate.q[i] = q;
                    candidate.qd[i] = v;
                    velocity[nb + j] = v;
                    q
                })
                .collect();
            for t in &art.transmissions {
                candidate.q[t.driven] = candidate.q[t.driver] / t.ratio;
                candidate.qd[t.driven] = candidate.qd[t.driver] / t.ratio;
            }
            for c in &charts {
                let [i, j, s] = c.dof_indices;
                let x = c.coordinates(candidate.q[i], c.reference_branch, seed.q[j])?;
                candidate.q[j] = x.coupler_rad;
                candidate.q[s] = x.slider_m;
                for (offset, idx) in [j, s].into_iter().enumerate() {
                    candidate.qd[idx] = x.first_derivative[offset] * candidate.qd[i];
                    candidate.qdd[idx] = x.second_derivative[offset] * candidate.qd[i].powi(2);
                }
                minimum_projection = minimum_projection.min(x.rod_projection_m.abs());
            }
            let reference = map.solve(&seed, &positions, &velocity)?;
            for i in 0..dofs.len() {
                max_q = max_q.max((candidate.q[i] - reference.generalized.q[i]).abs());
                max_v = max_v.max((candidate.qd[i] - reference.generalized.qd[i]).abs());
                max_a = max_a.max((candidate.qdd[i] - reference.acceleration_bias[nb + i]).abs());
            }
            for row in art.original_closure(&candidate) {
                for (i, v) in [row.position, row.velocity, row.acceleration]
                    .into_iter()
                    .enumerate()
                {
                    closure[i] = closure[i].max(v.abs());
                }
            }
            samples.push(json!({"crank_rad":theta,"joint_positions":candidate.q,"minimum_reference_singular_value":reference.minimum_scaled_singular_value}));
            seed = reference.generalized;
        }
        report["comparison"] = json!({"samples":samples,"maximum_joint_coordinate_difference":max_q,"maximum_joint_velocity_difference":max_v,"maximum_joint_acceleration_bias_difference":max_a,"maximum_original_closure_by_level":closure,"minimum_absolute_rod_projection_m":minimum_projection,
            "passed":max_q<1e-9&&max_v<1e-9&&max_a<1e-8&&closure.iter().all(|e|*e<1e-10),
            "tolerance_scope":"Mixed rad/m joint diagnostics and original m/dimensionless closure values; fixed absolute numerical audit limits, not hardware accuracy budgets."});
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
    );
    if !issues.is_empty() || report["comparison"]["passed"] != true {
        return Err("analytic mechanism audit did not pass".into());
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
