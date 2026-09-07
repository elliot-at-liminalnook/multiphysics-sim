//! Diagnose two scene configurations at identical supplied states and rates.
//! Usage: compare_matrices scene-a.json scene-b.json point.json
//! Point fields: island, time_s, states, rates. This is not trajectory validation.
//! Optional state_row_probe: {rows: [indices], relative_steps: [radii]} requires
//! a captured stage and compares selected central state derivatives at that point.
use nalgebra::{DMatrix, DVector};
use serde::Deserialize;
use sim_dynamics::{JacobianParts, System};
use sim_runtime::{
    session::{Scene, Session},
    validation::state_labels,
};

#[derive(Deserialize)]
struct Point {
    island: usize,
    time_s: f64,
    states: Vec<f64>,
    rates: Vec<f64>,
    #[serde(default)]
    stage: Option<Stage>,
    #[serde(default)]
    state_row_probe: Option<StateRowProbe>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StateRowProbe {
    rows: Vec<usize>,
    relative_steps: Vec<f64>,
}

// Diagnostic replacement of selected state-derivative rows only. The residual
// and rate matrix remain unchanged. Compare h and h/2 before trusting a stencil.
fn central_state_rows<S: System>(
    system: &S,
    p: &Point,
    original: &DMatrix<f64>,
    rows: &[usize],
    step: f64,
) -> Result<(DMatrix<f64>, f64), String> {
    let n = system.dimension();
    if original.shape() != (n, n)
        || p.states.len() != n
        || p.rates.len() != n
        || rows.is_empty()
        || rows.iter().any(|r| *r >= n)
        || !step.is_finite()
        || step <= 0.0
        || step > 0.1
    {
        return Err("invalid selected-row numerical probe".into());
    }
    let mut selected = rows.to_vec();
    selected.sort_unstable();
    selected.dedup();
    if selected.len() != rows.len() {
        return Err("duplicate selected derivative row".into());
    }
    let mut matrix = original.clone();
    let mut xp = p.states.clone();
    let mut plus = vec![0.0; n];
    let mut minus = plus.clone();
    let mut difference: f64 = 0.0;
    for col in 0..n {
        let h = step * (1.0 + p.states[col].abs());
        let mut coarse = vec![0.0; rows.len()];
        for (pass, radius) in [h, 0.5 * h].into_iter().enumerate() {
            xp[col] = p.states[col] + radius;
            system.residual(p.time_s, &xp, &p.rates, &mut plus);
            xp[col] = p.states[col] - radius;
            system.residual(p.time_s, &xp, &p.rates, &mut minus);
            xp[col] = p.states[col];
            for (i, &row) in rows.iter().enumerate() {
                let value = (plus[row] - minus[row]) / (2.0 * radius);
                if !value.is_finite() {
                    return Err("nonfinite selected-row derivative".into());
                }
                if pass == 0 {
                    coarse[i] = value;
                } else {
                    difference = difference.max((value - coarse[i]).abs());
                    matrix[(row, col)] = (4.0 * value - coarse[i]) / 3.0;
                }
            }
        }
    }
    Ok((matrix, difference))
}
#[derive(Deserialize)]
struct Stage {
    step_s: f64,
    theta: f64,
    expected_residual: Vec<f64>,
}

// A diagnostic dense solve with common scaling, not a replay of the production
// sparse factorization, cached matrix, acceptance policy, or line search.
fn stage_comparison<S: System>(
    system: &S,
    p: &Point,
    names: &[String],
    base: &[f64],
    a: (&DMatrix<f64>, &DMatrix<f64>),
    b: (&DMatrix<f64>, &DMatrix<f64>),
) -> Result<serde_json::Value, String> {
    let Some(stage) = &p.stage else {
        return Ok(serde_json::Value::Null);
    };
    let n = names.len();
    if !stage.step_s.is_finite()
        || stage.step_s <= 0.0
        || !stage.theta.is_finite()
        || stage.theta <= 0.0
        || stage.theta > 1.0
        || stage.expected_residual.len() != n
        || stage
            .expected_residual
            .iter()
            .zip(base)
            .any(|(a, b)| a.to_bits() != b.to_bits())
    {
        return Err("invalid stage or captured residual differs from current context".into());
    }
    let algebraic = system.algebraic().unwrap_or_else(|| vec![false; n]);
    if algebraic.len() != n {
        return Err("invalid algebraic contract".into());
    }
    let weights: Vec<_> = algebraic
        .iter()
        .map(|a| if *a { 1.0 } else { stage.theta })
        .collect();
    let combine = |(x, r): (&DMatrix<f64>, &DMatrix<f64>)| {
        DMatrix::from_fn(n, n, |i, j| {
            x[(i, j)] * weights[j] + r[(i, j)] / stage.step_s
        })
    };
    let ja = combine(a);
    let jb = combine(b);
    if ja.iter().chain(jb.iter()).any(|v| !v.is_finite()) {
        return Err("nonfinite combined stage matrix".into());
    }
    let row: Vec<f64> = (0..n)
        .map(|i| {
            let m = (0..n).map(|j| ja[(i, j)].abs()).fold(0.0, f64::max);
            if m > 0.0 {
                1.0 / m
            } else {
                1.0
            }
        })
        .collect();
    let col: Vec<f64> = (0..n)
        .map(|j| {
            let m = (0..n)
                .map(|i| (row[i] * ja[(i, j)]).abs())
                .fold(0.0, f64::max);
            if m > 0.0 {
                1.0 / m
            } else {
                1.0
            }
        })
        .collect();
    let norm = |r: &[f64]| {
        r.iter()
            .zip(&row)
            .map(|(r, s)| (r * s).abs())
            .fold(0.0, f64::max)
    };
    let rhs = DVector::from_iterator(n, base.iter().zip(&row).map(|(r, s)| -r * s));
    if row
        .iter()
        .chain(&col)
        .chain(rhs.iter())
        .any(|v| !v.is_finite())
    {
        return Err("nonfinite stage equilibration".into());
    }
    let mut reports = Vec::new();
    for (label, matrix) in [("a", &ja), ("b", &jb)] {
        let scaled = DMatrix::from_fn(n, n, |i, j| matrix[(i, j)] * row[i] * col[j]);
        if scaled.iter().any(|v| !v.is_finite()) {
            return Err("nonfinite scaled stage matrix".into());
        }
        let singular = scaled.clone().svd(false, false).singular_values;
        let maximum = singular.max();
        let minimum = singular.min();
        let scaled_delta = scaled
            .lu()
            .solve(&rhs)
            .ok_or("singular diagnostic stage matrix")?;
        let delta = DVector::from_iterator(n, (0..n).map(|i| scaled_delta[i] * col[i]));
        if delta.iter().any(|v| !v.is_finite()) {
            return Err("nonfinite diagnostic correction".into());
        }
        let linear = matrix * &delta + DVector::from_column_slice(base);
        let other = if label == "a" { &jb } else { &ja };
        let cross = other * &delta + DVector::from_column_slice(base);
        let mut probes = Vec::new();
        for k in 0..=12 {
            let alpha = 2.0_f64.powi(-k);
            let x: Vec<_> = (0..n)
                .map(|i| p.states[i] + weights[i] * alpha * delta[i])
                .collect();
            let rate: Vec<_> = (0..n)
                .map(|i| p.rates[i] + alpha * delta[i] / stage.step_s)
                .collect();
            let mut residual = vec![0.0; n];
            system.residual(p.time_s, &x, &rate, &mut residual);
            let finite = residual.iter().all(|v| v.is_finite());
            let mut rows: Vec<_> = (0..n).collect();
            rows.sort_by(|i, j| {
                (residual[*j] * row[*j])
                    .abs()
                    .total_cmp(&(residual[*i] * row[*i]).abs())
            });
            rows.truncate(4);
            probes.push(serde_json::json!({"fraction":alpha,"finite":finite,
                "common_scaled_residual_norm":if finite {Some(norm(&residual))} else {None},
                "largest_residual_rows":rows.iter().map(|i|serde_json::json!({"row":names[*i],
                    "residual":residual[*i],"initial_residual":base[*i],"row_scale":row[*i],
                    "predicted_residual":(1.0-alpha)*base[*i]+alpha*linear[*i]})).collect::<Vec<_>>()}));
        }
        let mut indices: Vec<_> = (0..n).collect();
        indices.sort_by(|i, j| scaled_delta[*j].abs().total_cmp(&scaled_delta[*i].abs()));
        indices.truncate(16);
        reports.push(serde_json::json!({"matrix":label,"common_scaled_singular_min":minimum,
            "common_scaled_singular_max":maximum,"common_scaled_condition_number":maximum/minimum,
            "linear_defect_norm":norm(linear.as_slice()),"other_matrix_linear_defect_norm":norm(cross.as_slice()),
            "largest_scaled_corrections":indices.iter().map(|i|serde_json::json!({"coordinate":names[*i],
                "increment":delta[*i],"scaled_increment":scaled_delta[*i]})).collect::<Vec<_>>(),"residual_probes":probes}));
    }
    Ok(
        serde_json::json!({"step_s":stage.step_s,"theta":stage.theta,"initial_common_scaled_residual_norm":norm(base),
        "reports":reports,"notes":["Both dense solves and residual probes use common row/column scales derived from matrix A.",
        "Condition numbers depend on scaling; these scales are numerical equilibration, not declared physical error budgets.",
        "Fresh diagnostic dense corrections do not reproduce cached sparse production Newton steps.",
        "Probe fractions are observations, not accepted timesteps or changed solver tolerances."]}),
    )
}
fn read<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, String> {
    serde_json::from_str(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 {
        return Err("usage: compare_matrices scene-a.json scene-b.json point.json".into());
    }
    let a = Session::new(read::<Scene>(&args[0])?, 0)?;
    let b = Session::new(read::<Scene>(&args[1])?, 0)?;
    let p: Point = read(&args[2])?;
    let evaluate = |s: &Session| -> Result<_, String> {
        let island = s
            .robot
            .runtime
            .islands
            .get(p.island)
            .ok_or("invalid island")?;
        let n = island.system.dimension();
        if !p.time_s.is_finite()
            || p.states.len() != n
            || p.rates.len() != n
            || p.states.iter().chain(&p.rates).any(|v| !v.is_finite())
        {
            return Err("invalid point".into());
        }
        let labels = state_labels(&s.robot.runtime);
        let names: Vec<_> = island
            .system
            .full_of
            .iter()
            .map(|f| labels[&island.system.state_ids[*f]].clone())
            .collect();
        let mut residual = vec![0.0; n];
        island
            .system
            .residual(p.time_s, &p.states, &p.rates, &mut residual);
        let mut parts = JacobianParts::default();
        if !island
            .system
            .jacobian(p.time_s, &p.states, &p.rates, &mut parts)
        {
            return Err("missing Jacobian".into());
        }
        Ok((names, residual, parts.dense(n)))
    };
    let (names, ra, (ax, ar)) = evaluate(&a)?;
    let (other, rb, (bx, br)) = evaluate(&b)?;
    if names != other {
        return Err("coordinate contracts differ".into());
    }
    if ra
        .iter()
        .zip(&rb)
        .any(|(a, b)| !a.is_finite() || a.to_bits() != b.to_bits())
    {
        return Err("residuals differ at the supplied point; verify physical configuration and external inputs".into());
    }
    let ia = &a.robot.runtime.islands[p.island].system;
    let ib = &b.robot.runtime.islands[p.island].system;
    if ia.algebraic() != ib.algebraic() {
        return Err("algebraic contracts differ".into());
    }
    let stage = stage_comparison(ia, &p, &names, &ra, (&ax, &ar), (&bx, &br))?;
    let mut selected_rows = Vec::new();
    if let Some(probe) = &p.state_row_probe {
        if p.stage.is_none() || probe.relative_steps.is_empty() || probe.relative_steps.len() > 8 {
            return Err(
                "selected-row probes require a captured stage and 1..8 stencil sizes".into(),
            );
        }
        for &step in &probe.relative_steps {
            let (matrix, difference) = central_state_rows(ia, &p, &ax, &probe.rows, step)?;
            selected_rows.push(serde_json::json!({"relative_step":step,
                "rows":probe.rows.iter().map(|i| &names[*i]).collect::<Vec<_>>(),
                "maximum_coarse_fine_state_derivative_difference":difference,
                "stage":stage_comparison(ia,&p,&names,&ra,(&ax,&ar),(&matrix,&ar))?,
                "notes":["Matrix B replaces only the selected state rows with Richardson-extrapolated central differences; matrix A is unchanged.",
                    "The rate matrix, residual equations, loads and tolerances are unchanged.",
                    "Coarse/fine agreement is diagnostic; this does not establish trajectory accuracy or promote a derivative."]}));
        }
    }
    let mut reports = Vec::new();
    for (kind, x, y) in [("state", ax, bx), ("rate", ar, br)] {
        let mut differences = Vec::new();
        let mut count = 0;
        let mut maximum: f64 = 0.0;
        for row in 0..names.len() {
            for col in 0..names.len() {
                let (u, v) = (x[(row, col)], y[(row, col)]);
                if !u.is_finite() || !v.is_finite() {
                    return Err("non-finite Jacobian".into());
                }
                let error = (u - v).abs();
                if error > 0.0 {
                    count += 1;
                    maximum = maximum.max(error);
                    differences.push((error, row, col, u, v));
                }
            }
        }
        differences.sort_by(|a, b| b.0.total_cmp(&a.0));
        differences.truncate(16);
        reports.push(serde_json::json!({"kind":kind,"differing_entries":count,"maximum_absolute_difference":maximum,
            "largest":differences.iter().map(|(error,row,col,u,v)|serde_json::json!({
                "row":names[*row],"column":names[*col],"a":u,"b":v,"error":error})).collect::<Vec<_>>()}));
    }
    println!("{}",serde_json::to_string_pretty(&serde_json::json!({"time_s":p.time_s,"dimension":names.len(),
        "residuals_match_bitwise":true,"matrices":reports,"stage":stage,"selected_state_row_probes":selected_rows,"notes":[
            "Both scenes start with their initial external inputs; callers must supply compatible captured states.",
            "Matrix equality is diagnostic, not a numerical derivative or trajectory accuracy gate."
        ]})).unwrap());
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_central_rows_preserve_other_rows_and_hold_rates_fixed() {
        struct Cubic;
        impl System for Cubic {
            fn dimension(&self) -> usize {
                2
            }
            fn residual(&self, _: f64, x: &[f64], r: &[f64], out: &mut [f64]) {
                out[0] = x[0].powi(3) + x[0] * x[1] + 3.0 * r[0];
                out[1] = x[0] - 4.0 * x[1];
            }
        }
        let p = Point {
            island: 0,
            time_s: 0.2,
            states: vec![0.7, -0.3],
            rates: vec![2.0, 0.0],
            stage: None,
            state_row_probe: None,
        };
        let original = DMatrix::from_element(2, 2, 37.0);
        let (matrix, difference) = central_state_rows(&Cubic, &p, &original, &[0], 1e-3).unwrap();
        assert!((matrix[(0, 0)] - (3.0 * 0.7 * 0.7 - 0.3)).abs() < 1e-11);
        assert!((matrix[(0, 1)] - 0.7).abs() < 1e-11);
        assert_eq!(matrix.row(1), original.row(1));
        assert!(
            difference > 1e-7,
            "cubic must exercise coarse/fine truncation"
        );
        for rows in [vec![], vec![2], vec![0, 0]] {
            assert!(central_state_rows(&Cubic, &p, &original, &rows, 1e-3).is_err());
        }
        assert!(central_state_rows(&Cubic, &p, &original, &[0], f64::NAN).is_err());
    }
    struct Mixed;
    impl System for Mixed {
        fn dimension(&self) -> usize {
            2
        }
        fn algebraic(&self) -> Option<Vec<bool>> {
            Some(vec![false, true])
        }
        fn residual(&self, _: f64, x: &[f64], r: &[f64], out: &mut [f64]) {
            out[0] = 2.0 * x[0] + x[1] + 3.0 * r[0];
            out[1] = x[0] - 4.0 * x[1];
        }
    }
    #[test]
    fn stage_weights_and_correction_probes_detect_a_wrong_rate_block() {
        let x = DMatrix::from_row_slice(2, 2, &[2.0, 1.0, 1.0, -4.0]);
        let r = DMatrix::from_row_slice(2, 2, &[3.0, 0.0, 0.0, 0.0]);
        let wrong = &r * 2.0;
        let mut p = Point {
            state_row_probe: None,
            island: 0,
            time_s: 0.5,
            states: vec![0.25, -0.5],
            rates: vec![2.0, 0.0],
            stage: Some(Stage {
                step_s: 0.125,
                theta: 0.5,
                expected_residual: vec![6.0, 2.25],
            }),
        };
        let names = vec!["differential".into(), "algebraic".into()];
        let base = vec![6.0, 2.25];
        let result = stage_comparison(&Mixed, &p, &names, &base, (&x, &r), (&x, &wrong)).unwrap();
        let reports = result["reports"].as_array().unwrap();
        let corrected = reports[0]["residual_probes"][0]["common_scaled_residual_norm"]
            .as_f64()
            .unwrap();
        assert!(corrected < 1e-14, "{result}");
        assert!(
            reports[1]["residual_probes"][0]["common_scaled_residual_norm"]
                .as_f64()
                .unwrap()
                > 0.01
        );
        assert!(
            reports[0]["other_matrix_linear_defect_norm"]
                .as_f64()
                .unwrap()
                > 0.01
        );
        p.stage.as_mut().unwrap().expected_residual[0] += 0.1;
        assert!(stage_comparison(&Mixed, &p, &names, &base, (&x, &r), (&x, &wrong)).is_err());
        p.stage
            .as_mut()
            .unwrap()
            .expected_residual
            .clone_from(&base);
        p.stage.as_mut().unwrap().step_s = 1e-320;
        assert!(stage_comparison(&Mixed, &p, &names, &base, (&x, &r), (&x, &wrong)).is_err());
    }
}
