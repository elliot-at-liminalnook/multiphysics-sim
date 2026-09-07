//! Compare a projected rigid mechanical block with an independent dense KKT
//! solve at one captured committed stage. This does NOT advance a timestep.
//! Usage: compare_transmission_block capture.json [attempt-index|all] [repeats]
use nalgebra::{DMatrix, DVector};
use serde_json::json;
use sim_domain_robot::articulated::transmission_coordinates::{
    AccelerationProblem, TransmissionCoordinateMap,
};
use sim_domain_robot::{Articulated, Generalized};
use sim_dynamics::ImplicitAttempt;
use sim_runtime::session::{Scene, Session};
use std::time::Instant;

fn mechanical_forces(art: &Articulated, g: &Generalized) -> DVector<f64> {
    let e = art.evaluate(g);
    forces_from_evaluation(art, &e)
}

fn forces_from_evaluation(
    art: &Articulated,
    e: &sim_domain_robot::articulated::Evaluation,
) -> DVector<f64> {
    DVector::from_vec(
        art.bases
            .iter()
            .enumerate()
            .filter(|(_, b)| !b.grounded)
            .flat_map(|(i, _)| e.base_wrench[i])
            .chain(
                e.joints
                    .iter()
                    .flat_map(|j| j.tau_needed.iter().zip(&j.tau_passive).map(|(a, b)| a - b)),
            )
            .collect(),
    )
}

fn set_accelerations(art: &Articulated, g: &mut Generalized, a: &[f64]) {
    let mut offset = 0;
    for b in art.bases.iter().filter(|b| !b.grounded) {
        g.rates[b.state + 7..b.state + 13].copy_from_slice(&a[offset..offset + 6]);
        offset += 6;
    }
    for (i, (_, d)) in art.dofs().enumerate() {
        g.qdd[i] = a[offset + i];
        g.rates[d.qd_state] = a[offset + i];
    }
}

fn full_solve(
    mass: &DMatrix<f64>,
    g: &DMatrix<f64>,
    cfm: &[f64],
    force: &DVector<f64>,
    b: &DVector<f64>,
) -> Result<DVector<f64>, String> {
    let n = mass.nrows();
    let m = g.nrows();
    let mut kkt = DMatrix::zeros(n + m, n + m);
    let mut rhs = DVector::zeros(n + m);
    kkt.view_mut((0, 0), (n, n)).copy_from(mass);
    kkt.view_mut((0, n), (n, m)).copy_from(&(-g.transpose()));
    kkt.view_mut((n, 0), (m, n)).copy_from(g);
    for i in 0..m {
        kkt[(n + i, n + i)] = cfm[i];
    }
    rhs.rows_mut(0, n).copy_from(force);
    rhs.rows_mut(n, m).copy_from(b);
    kkt.lu()
        .solve(&rhs)
        .ok_or("singular full acceleration block".into())
}

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || args.len() > 3 {
        return Err(
            "usage: compare_transmission_block capture.json [attempt-index|all] [repeats]".into(),
        );
    }
    let capture: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&args[0]).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let scene: Scene =
        serde_json::from_value(capture["recording"]["scene"].clone()).map_err(|e| e.to_string())?;
    let session = Session::new(
        scene,
        capture["recording"]["seed"]
            .as_u64()
            .ok_or("missing seed")?,
    )?;
    let islands = capture["attempt_audit"]["islands"]
        .as_array()
        .ok_or("capture needs attempt audit")?;
    if islands.len() != 1 || islands[0]["capacity_reached"] != false {
        return Err("requires one uncapped audited island".into());
    }
    let attempts = islands[0]["attempts"]
        .as_array()
        .ok_or("missing attempts")?;
    let repeats = args
        .get(2)
        .map(|s| s.parse::<usize>())
        .transpose()
        .map_err(|e| e.to_string())?
        .unwrap_or(500);
    if repeats == 0 || repeats > 100000 {
        return Err("repeats must be 1..100000".into());
    }
    let all = args.get(1).is_some_and(|s| s == "all");
    let indices: Vec<usize> = if all {
        attempts
            .iter()
            .enumerate()
            .filter(|(_, a)| a["solve"]["committed"] == true)
            .map(|(i, _)| i)
            .collect()
    } else if let Some(s) = args.get(1) {
        vec![s.parse::<usize>().map_err(|e| e.to_string())?]
    } else {
        vec![attempts
            .iter()
            .enumerate()
            .filter(|(_, a)| a["solve"]["committed"] == true)
            .max_by_key(|(_, a)| {
                a["solve"]["newton"]["iterations"]
                    .as_array()
                    .map_or(0, Vec::len)
            })
            .map(|(i, _)| i)
            .ok_or("no committed stages")?]
    };
    if indices.is_empty() {
        return Err("no committed stages".into());
    }
    let reports: Vec<_> = indices
        .iter()
        .map(|&i| {
            compare_point(
                &session,
                attempts.get(i).ok_or("invalid attempt index")?,
                i,
                repeats,
                &args[0],
            )
        })
        .collect::<Result<_, String>>()?;
    let report = if all {
        json!({"capture":args[0],"stages":reports,"committed_stages":indices.len()})
    } else {
        reports.into_iter().next().unwrap()
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
    );
    Ok(())
}

fn compare_point(
    session: &Session,
    value: &serde_json::Value,
    index: usize,
    repeats: usize,
    capture_path: &str,
) -> Result<serde_json::Value, String> {
    let point: ImplicitAttempt =
        serde_json::from_value(value["solve"].clone()).map_err(|e| e.to_string())?;
    if point.committed != Some(true) || !point.solve_succeeded {
        return Err("requires a committed successful stage".into());
    }
    let base = session
        .robot
        .generalized_at_solver_point(0, &point)?
        .ok_or("stage has no robot")?;
    let art = &session.robot.art;
    if art.links.iter().any(|l| l.flex.is_some()) {
        return Err(
            "this rigid-block experiment does not discard modal flexibility; supply a rigid model"
                .into(),
        );
    }
    let nb = art.bases.iter().filter(|b| !b.grounded).count() * 6;
    let n = nb + base.q.len();
    let assembly_start = Instant::now();
    let external = mechanical_forces(art, &base);
    let mut zero = base.clone();
    set_accelerations(art, &mut zero, &vec![0.0; n]);
    for lp in &art.loops {
        zero.states[lp.lambda_state..lp.lambda_state + lp.rows].fill(0.0);
    }
    for t in &art.transmissions {
        zero.states[t.lambda_state] = 0.0;
    }
    let evaluate = art.prepare_evaluation(zero.clone());
    let bias = forces_from_evaluation(art, &evaluate(&zero));
    // Unit acceleration probes exploit affine inverse dynamics. This diagnostic
    // assembly reuses contact geometry/forces with unchanged dependencies. It is
    // still an explicit basis construction, not a recursive mass kernel.
    let probe_mass = || {
        let mut mass = DMatrix::zeros(n, n);
        for col in 0..n {
            let mut probe = zero.clone();
            let mut a = vec![0.0; n];
            a[col] = 1.0;
            set_accelerations(art, &mut probe, &a);
            mass.set_column(
                col,
                &(forces_from_evaluation(art, &evaluate(&probe)) - &bias),
            );
        }
        mass
    };
    let mass = probe_mass();
    let direct_mass = art.rigid_mass_matrix(&zero)?;
    let mass_difference = (&mass - &direct_mass).amax();
    let mass_tolerance = 1e-10 * (1.0 + mass.amax());
    if mass_difference > mass_tolerance {
        return Err(format!("direct mass disagrees with loaded probe reference: {mass_difference} > {mass_tolerance}"));
    }
    let audit = art.audit_constraints(&zero, &Default::default())?;
    if audit.coordinates.len() != n {
        return Err("mechanical coordinate ordering mismatch".into());
    }
    let g = DMatrix::from_fn(audit.rows.len(), n, |i, j| {
        audit.scaled_velocity_matrix[i][j] * audit.row_scales[i] / audit.column_scales[j]
    });
    let b = DVector::from_iterator(audit.rows.len(), audit.rows.iter().map(|r| -r.stabilized));
    let force = external - &bias;
    let remaining: usize = art.loops.iter().map(|lp| lp.rows).sum();
    let other_cfm: Vec<_> = art
        .loops
        .iter()
        .flat_map(|lp| {
            (0..lp.rows).map(|i| {
                if i < 3 {
                    art.loop_cfm
                } else {
                    art.loop_angular_cfm
                }
            })
        })
        .collect();
    let mut ideal_cfm = other_cfm.clone();
    ideal_cfm.resize(audit.rows.len(), 0.0);
    let relations: Vec<_> = art
        .transmissions
        .iter()
        .map(|t| (nb + t.driver, nb + t.driven, t.ratio))
        .collect();
    let map = TransmissionCoordinateMap::new(n, &relations)?;
    let other_g = g.rows(0, remaining).into_owned();
    let problem = AccelerationProblem {
        mass: &direct_mass,
        force: force.as_slice(),
        transmission_rhs: &b.as_slice()[remaining..],
        other_jacobian: &other_g,
        other_rhs: &b.as_slice()[..remaining],
        other_cfm: &other_cfm,
    };
    let assembly_s = assembly_start.elapsed().as_secs_f64();
    let projected = map.solve_accelerations(&problem)?;
    let reference = full_solve(&mass, &g, &ideal_cfm, &force, &b)?;
    let error = |a: &[f64], b: &[f64]| {
        a.iter()
            .zip(b)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f64::max)
    };
    let mut regularized_cfm = other_cfm.clone();
    regularized_cfm.resize(audit.rows.len(), art.loop_angular_cfm);
    let regularized = full_solve(&mass, &g, &regularized_cfm, &force, &b)?;
    let saved: Vec<_> = art
        .bases
        .iter()
        .filter(|b| !b.grounded)
        .flat_map(|b| base.rates[b.state + 7..b.state + 13].iter().copied())
        .chain(base.qdd.iter().copied())
        .collect();
    let mut full_times = Vec::new();
    let mut projected_times = Vec::new();
    let mut probe_mass_times = Vec::new();
    let mut direct_mass_times = Vec::new();
    let mass_repeats = repeats.min(10);
    for i in 0..mass_repeats {
        for direct in if i % 2 == 0 {
            [false, true]
        } else {
            [true, false]
        } {
            let start = Instant::now();
            if direct {
                std::hint::black_box(art.rigid_mass_matrix(&zero)?);
                direct_mass_times.push(start.elapsed().as_secs_f64());
            } else {
                std::hint::black_box(probe_mass());
                probe_mass_times.push(start.elapsed().as_secs_f64());
            }
        }
    }
    // Alternate order to reduce drift bias; include allocation, factorization,
    // and reaction reconstruction. The full reference has no diagnostic arrays.
    for i in 0..repeats {
        for projected_first in if i % 2 == 0 {
            [false, true]
        } else {
            [true, false]
        } {
            let start = Instant::now();
            if projected_first {
                std::hint::black_box(map.solve_accelerations(&problem)?);
                projected_times.push(start.elapsed().as_secs_f64());
            } else {
                std::hint::black_box(full_solve(&mass, &g, &ideal_cfm, &force, &b)?);
                full_times.push(start.elapsed().as_secs_f64());
            }
        }
    }
    let mean = |times: &[f64]| times.iter().sum::<f64>() / (times.len() as f64);
    Ok(json!({
        "capture":capture_path,"attempt_index":index,"stage_time_s":point.stage_time,"repeats":repeats,
        "mechanical_coordinates":audit.coordinates,"full_block_dimension":n+g.nrows(),
        "mechanical_velocity_units":audit.velocity_units,
        "transmissions":art.transmissions.iter().map(|t|&t.name).collect::<Vec<_>>(),
        "mass_max_asymmetry":(&mass-mass.transpose()).amax(),"block_assembly_s":assembly_s,
        "direct_mass_max_asymmetry":(&direct_mass-direct_mass.transpose()).amax(),
        "mass_max_difference":mass_difference,"mass_comparison_tolerance":mass_tolerance,
        "mass_build_repeats":mass_repeats,"probe_mass_mean_s":mean(&probe_mass_times),
        "direct_mass_mean_s":mean(&direct_mass_times),
        "full_solve_mean_s":mean(&full_times),"projected_solve_mean_s":mean(&projected_times),
        "acceleration_max_difference":error(&projected.accelerations,reference.rows(0,n).as_slice()),
        "other_reaction_max_difference":error(&projected.other_reactions,reference.rows(n,remaining).as_slice()),
        "transmission_reaction_max_difference":error(&projected.transmission_reactions,reference.rows(n+remaining,relations.len()).as_slice()),
        "regularized_vs_saved_acceleration_max_difference":error(regularized.rows(0,n).as_slice(),&saved),
        "ideal_vs_regularized_acceleration_max_difference":error(reference.rows(0,n).as_slice(),regularized.rows(0,n).as_slice()),
        "projected":projected,
        "notes":["Fixed-stage rigid mechanical block only; no coupled timestep or trajectory performance claim.",
          "Both compared blocks use ZERO transmission CFM. Original remaining-loop CFM, pose, velocities and contact loads are retained.",
          "Finite unit-acceleration differences assemble mass; compare asymmetry and regularized reconstruction error before trusting the fixture.",
          "The full solve uses loaded-probe mass; the projected solve uses independently assembled link-motion inertia. This is not a recursive ABA implementation.",
          "Mass timings alternate order. The probe reference reuses its prepared evaluator and bias; direct construction includes its own kinematics.",
          "Mixed coordinate residuals need declared physical scales; this report does not decide promotion."]
    }))
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
