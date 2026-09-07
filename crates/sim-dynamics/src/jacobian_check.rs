//! Independent residual-based checks of provided Jacobians. No sparsity pattern
//! or derivative implementation is reused by the numerical reference.
use crate::{JacobianParts, System};
use serde::{Deserialize, Serialize};

/// Check the Newton increment Jacobian at a recorded fresh linearization point.
/// The caller must retain the same system and external inputs. A fresh residual
/// must exactly reproduce the captured residual before any derivative probes.
/// This reassembles provided derivatives; it does not inspect cached LU factors.
pub fn check_implicit_jacobian<S: System>(
    system: &S, point: &crate::ImplicitAttempt, config: &CheckConfig,
) -> Result<CheckReport, String> {
    let n = system.dimension();
    let linearization = point.last_linearization.as_ref().ok_or("trial has no recorded fresh linearization")?;
    if point.initial_state.len()!=n || linearization.increment.len()!=n || linearization.residual.len()!=n
        || !point.step.is_finite() || point.step<=0.0 || !point.theta.is_finite()
        || point.theta<=0.0 || point.theta>1.0 || !point.stage_time.is_finite()
        || point.initial_state.iter().chain(&linearization.increment).chain(&linearization.residual).any(|v| !v.is_finite()) {
        return Err("invalid implicit linearization record".into());
    }
    let algebraic = system.algebraic().unwrap_or_else(|| vec![false;n]);
    if algebraic.len()!=n { return Err("implicit linearization algebraic dimension mismatch".into()); }
    struct Stage<'a,S> { system:&'a S, point:&'a crate::ImplicitAttempt, algebraic:Vec<bool> }
    impl<S:System> Stage<'_,S> {
        fn coordinates(&self,u:&[f64])->(Vec<f64>,Vec<f64>) {
            let x = u.iter().enumerate().map(|(i,v)| self.point.initial_state[i]
                + if self.algebraic[i] {*v} else {self.point.theta*v}).collect();
            let rate = u.iter().map(|v| v/self.point.step).collect();
            (x,rate)
        }
    }
    impl<S:System> System for Stage<'_,S> {
        fn dimension(&self)->usize { self.system.dimension() }
        fn residual(&self,t:f64,u:&[f64],_:&[f64],out:&mut[f64]) {
            let (x,rate)=self.coordinates(u);
            self.system.residual(t,&x,&rate,out);
        }
        fn jacobian(&self,t:f64,u:&[f64],_:&[f64],out:&mut JacobianParts)->bool {
            let (x,rate)=self.coordinates(u);
            let mut parts=JacobianParts::default();
            if !self.system.jacobian(t,&x,&rate,&mut parts) { return false; }
            for (r,c,v) in parts.d_dx {
                // Preserve invalid indices for the checker's validation.
                let weight=if self.algebraic.get(c).copied().unwrap_or(false) {1.0} else {self.point.theta};
                out.d_dx.push((r,c,weight*v));
            }
            for (r,c,v) in parts.d_drate { out.d_dx.push((r,c,v/self.point.step)); }
            true
        }
    }
    let stage=Stage {system,point,algebraic};
    let zeros=vec![0.0;n];
    let mut residual=vec![0.0;n];
    stage.residual(point.stage_time,&linearization.increment,&zeros,&mut residual);
    if residual.iter().zip(&linearization.residual).any(|(a,b)| a.to_bits()!=b.to_bits()) {
        return Err("recorded linearization residual does not match current system context".into());
    }
    check_jacobian(&stage,point.stage_time,&linearization.increment,&zeros,config)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CheckConfig {
    pub absolute_tolerance: f64,
    pub relative_tolerance: f64,
    /// Relative stencil radius; checks use both h and h/2.
    pub step: f64,
    /// Characteristic magnitudes in each coordinate's declared units.
    /// Empty means max(1, |value|); explicit scales must have dimension n.
    pub state_scales: Vec<f64>,
    pub rate_scales: Vec<f64>,
    pub columns: bool,
    pub directions: usize,
}
impl Default for CheckConfig {
    fn default() -> Self {
        Self {
            absolute_tolerance: 1e-7,
            relative_tolerance: 1e-5,
            step: 1e-5,
            state_scales: vec![],
            rate_scales: vec![],
            columns: true,
            directions: 4,
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Difference {
    pub inconclusive: bool,
    pub probe: String,
    pub row: usize,
    pub provided: f64,
    pub numerical: f64,
    pub error: f64,
    pub tolerance: f64,
    /// Coarse stencil parameter h: a coordinate displacement for column probes,
    /// or the multiplier applied to the scaled direction for directional probes.
    pub stencil_step: f64,
    pub numerical_coarse: f64,
    pub numerical_fine: f64,
    /// Richardson estimate, not a bound at contact/branch transitions.
    pub reference_truncation_estimate: f64,
    /// Roundoff estimate used by the independent reference acceptance check.
    pub reference_roundoff_estimate: f64,
    pub reason: String,
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct CheckReport {
    pub passed: bool,
    pub comparisons: usize,
    pub mismatches: usize,
    pub inconclusive: usize,
    /// Counts cover all unresolved comparisons, including entries omitted from
    /// the bounded differences list. Each comparison has one primary reason.
    pub inconclusive_by_reason: std::collections::BTreeMap<String, usize>,
    pub max_error_ratio: f64,
    /// Up to 32 mismatches and 32 inconclusive entries; counts cover all checks.
    pub differences: Vec<Difference>,
}

pub fn check_jacobian<S: System>(
    system: &S,
    time: f64,
    x: &[f64],
    rate: &[f64],
    cfg: &CheckConfig,
) -> Result<CheckReport, String> {
    let n = system.dimension();
    if n == 0 || x.len() != n || rate.len() != n {
        return Err("Jacobian check dimension mismatch".into());
    }
    if !time.is_finite() || x.iter().chain(rate).any(|v| !v.is_finite()) {
        return Err("Jacobian check requires finite time, states and rates".into());
    }
    if !cfg.step.is_finite()
        || cfg.step <= 0.0
        || !cfg.absolute_tolerance.is_finite()
        || cfg.absolute_tolerance <= 0.0
        || !cfg.relative_tolerance.is_finite()
        || cfg.relative_tolerance < 0.0
        || (!cfg.columns && cfg.directions == 0)
    {
        return Err("invalid Jacobian check tolerances or probes".into());
    }
    let scales = |specified: &[f64], values: &[f64]| -> Result<Vec<f64>, String> {
        if specified.is_empty() {
            return Ok(values.iter().map(|v| v.abs().max(1.0)).collect());
        }
        if specified.len() != n || specified.iter().any(|v| !v.is_finite() || *v <= 0.0) {
            return Err("Jacobian check scales must be positive, finite and dimension n".into());
        }
        Ok(specified.to_vec())
    };
    let xs = scales(&cfg.state_scales, x)?;
    let rs = scales(&cfg.rate_scales, rate)?;
    let mut parts = JacobianParts::default();
    if !system.jacobian(time, x, rate, &mut parts) {
        return Err("system provides no Jacobian to check".into());
    }
    if parts
        .d_dx
        .iter()
        .chain(&parts.d_drate)
        .any(|(r, c, v)| *r >= n || *c >= n || !v.is_finite())
    {
        return Err("provided Jacobian contains invalid indices or non-finite entries".into());
    }
    let (jx, jr) = parts.dense(n);
    let mut base = vec![0.0; n];
    system.residual(time, x, rate, &mut base);
    if base.iter().any(|v| !v.is_finite()) {
        return Err("base residual is non-finite".into());
    }
    let mut report = CheckReport::default();
    let mut probe = |label: String,
                     dx: &[f64],
                     dr: &[f64],
                     provided_values: &[f64],
                     linear_reference: Option<&[f64]>,
                     h: f64|
     -> Result<Vec<f64>, String> {
        if !h.is_finite() || h <= 0.0 {
            return Err(format!("invalid stencil radius in {label}"));
        }
        let eval = |amount: f64| -> Result<Vec<f64>, String> {
            let xp: Vec<_> = x.iter().zip(dx).map(|(x, d)| x + amount * d).collect();
            let rp: Vec<_> = rate.iter().zip(dr).map(|(r, d)| r + amount * d).collect();
            if xp.iter().chain(&rp).any(|v| !v.is_finite()) {
                return Err(format!("non-finite perturbed coordinates in {label}"));
            }
            if x.iter().zip(&xp).all(|(a, b)| a == b) && rate.iter().zip(&rp).all(|(a, b)| a == b) {
                return Err(format!(
                    "stencil cannot resolve a coordinate change in {label}"
                ));
            }
            let mut out = vec![0.0; n];
            system.residual(time, &xp, &rp, &mut out);
            if out.iter().any(|v| !v.is_finite()) {
                return Err(format!("non-finite numerical residual in {label}; reduce stencil or choose an interior point"));
            }
            Ok(out)
        };
        let plus = eval(h)?;
        let minus = eval(-h)?;
        let plus_half = eval(h / 2.0)?;
        let minus_half = eval(-h / 2.0)?;
        let mut reference_values = Vec::with_capacity(n);
        for row in 0..n {
            let provided = provided_values[row];
            let coarse = (plus[row] - minus[row]) / (2.0 * h);
            let fine = (plus_half[row] - minus_half[row]) / h;
            let numerical = (4.0 * fine - coarse) / 3.0;
            reference_values.push(numerical);
            let tolerance = cfg.absolute_tolerance
                + cfg.relative_tolerance * provided.abs().max(numerical.abs());
            let error = (provided - numerical).abs();
            if !provided.is_finite()
                || !numerical.is_finite()
                || !tolerance.is_finite()
                || !error.is_finite()
            {
                return Err(format!(
                    "non-finite derivative comparison in {label}, row {row}"
                ));
            }
            let jump = (plus[row] + minus[row] - 2.0 * base[row]) / h;
            let jump_half = (plus_half[row] + minus_half[row] - 2.0 * base[row]) / (h / 2.0);
            let roundoff = 16.0
                * f64::EPSILON
                * (plus[row].abs()
                    + minus[row].abs()
                    + plus_half[row].abs()
                    + minus_half[row].abs()
                    + base[row].abs())
                / h;
            let reason = if jump_half.abs() > 0.75 * jump.abs()
                && jump_half.abs() > 10.0 * tolerance.max(roundoff)
            {
                Some("one-sided slopes disagree across both stencil sizes (possible branch boundary)")
            } else if roundoff > tolerance {
                Some("numerical reference is limited by roundoff at this stencil/scale")
            } else if (fine - coarse).abs() / 3.0 > tolerance {
                Some("numerical reference is not resolved across stencil sizes")
            } else if linear_reference
                .is_some_and(|values| (values[row] - numerical).abs() > tolerance)
            {
                Some("directional derivative disagrees with numerical column superposition (possible branch intersection)")
            } else {
                None
            };
            report.comparisons += 1;
            report.max_error_ratio = report.max_error_ratio.max(error / tolerance);
            if let Some(reason) = reason {
                report.inconclusive += 1;
                *report.inconclusive_by_reason.entry(reason.into()).or_default() += 1;
            } else if error > tolerance {
                report.mismatches += 1;
            }
            if reason.is_some() || error > tolerance {
                report.differences.push(Difference {
                    inconclusive: reason.is_some(),
                    probe: label.clone(),
                    row,
                    provided,
                    numerical,
                    error,
                    tolerance,
                    stencil_step: h,
                    numerical_coarse: coarse,
                    numerical_fine: fine,
                    reference_truncation_estimate: (fine - coarse).abs() / 3.0,
                    reference_roundoff_estimate: roundoff,
                    reason: reason
                        .unwrap_or("provided derivative disagrees with numerical reference")
                        .into(),
                });
                report
                    .differences
                    .sort_by(|a, b| (b.error / b.tolerance).total_cmp(&(a.error / a.tolerance)));
                let mut kept = [0usize; 2];
                report.differences.retain(|d| {
                    let count = &mut kept[usize::from(d.inconclusive)];
                    *count += 1;
                    *count <= 32
                });
            }
        }
        Ok(reference_values)
    };
    let mut reference_x = nalgebra::DMatrix::zeros(n, n);
    let mut reference_rate = nalgebra::DMatrix::zeros(n, n);
    if cfg.columns {
        for (kind, scale) in [("state", &xs), ("rate", &rs)] {
            for col in 0..n {
                let mut dx = vec![0.0; n];
                let mut dr = vec![0.0; n];
                if kind == "state" {
                    dx[col] = 1.0;
                } else {
                    dr[col] = 1.0;
                }
                let provided: Vec<_> = (0..n)
                    .map(|row| {
                        if kind == "state" {
                            jx[(row, col)]
                        } else {
                            jr[(row, col)]
                        }
                    })
                    .collect();
                let reference = probe(
                    format!("{kind}[{col}]"),
                    &dx,
                    &dr,
                    &provided,
                    None,
                    cfg.step * scale[col],
                )?;
                for row in 0..n {
                    if kind == "state" {
                        reference_x[(row, col)] = reference[row];
                    } else {
                        reference_rate[(row, col)] = reference[row];
                    }
                }
            }
        }
    }
    // Deterministic directions independent of simulation RNG/noise streams.
    let mut rng = 0x3141592653589793u64;
    for k in 0..cfg.directions {
        let mut direction = |scales: &[f64]| {
            scales
                .iter()
                .map(|scale| {
                    rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
                    let unit = ((rng >> 11) as f64) / ((1u64 << 53) as f64);
                    (2.0 * unit - 1.0) * scale
                })
                .collect::<Vec<_>>()
        };
        let dx = direction(&xs);
        let dr = direction(&rs);
        let provided: Vec<f64> = (0..n)
            .map(|row| {
                (0..n)
                    .map(|col| jx[(row, col)] * dx[col] + jr[(row, col)] * dr[col])
                    .sum()
            })
            .collect();
        let linear_reference: Vec<f64> = (0..n)
            .map(|row| {
                (0..n)
                    .map(|col| {
                        reference_x[(row, col)] * dx[col] + reference_rate[(row, col)] * dr[col]
                    })
                    .sum()
            })
            .collect();
        probe(
            format!("direction[{k}]"),
            &dx,
            &dr,
            &provided,
            cfg.columns.then_some(linear_reference.as_slice()),
            cfg.step,
        )?;
    }
    report.passed = report.mismatches == 0 && report.inconclusive == 0;
    Ok(report)
}
