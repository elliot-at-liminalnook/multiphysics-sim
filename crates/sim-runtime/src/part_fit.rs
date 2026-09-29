//! Fit a library part's parameters to measured data through the shared
//! runtime, and promote the result from estimated to measured.
//!
//! The model is an ordinary system document (the part on a bench that
//! reproduces the measurement). Each measured condition sets some parameters
//! (supply voltage, load) and reads one observable over a window. Unknowns
//! may bind several parameters to one value (k = k_t = k_e). Levenberg–
//! Marquardt on relative finite differences minimizes the squared residuals;
//! the standard uncertainty of each unknown is from σ²(JᵀJ)⁻¹ with
//! σ² = SSR / (n − p).
use crate::system_builder;
use serde::{Deserialize, Serialize};
use sim_core::BehaviorRegistry;
use sim_system::{Command, ParameterBinding, SystemDocument};

/// One parameter of one instance: (level path, instance, parameter).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Target {
    pub at: String,
    pub instance: String,
    pub parameter: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Unknown {
    pub name: String,
    pub unit: String,
    pub initial: f64,
    pub targets: Vec<Target>,
    /// Keep the value at or above this (e.g. 0 for friction).
    #[serde(default)]
    pub minimum: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Condition {
    pub label: String,
    pub set: Vec<(Target, f64)>,
    pub observable: String,
    pub window: [f64; 2],
    pub measured: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FitResult {
    pub values: Vec<(String, f64)>,
    /// One standard deviation per unknown.
    pub uncertainties: Vec<(String, f64)>,
    pub correlation: Vec<Vec<f64>>,
    /// (condition label, measured, predicted).
    pub residuals: Vec<(String, f64, f64)>,
    pub rms: f64,
    pub iterations: usize,
}

fn bind(doc: &mut SystemDocument, registry: &BehaviorRegistry, target: &Target, value: f64) -> Result<(), String> {
    sim_system::apply(doc, registry, &[Command::SetParameter { at: target.at.clone(), name: target.instance.clone(), parameter: target.parameter.clone(), binding: Some(ParameterBinding::value(value)) }]).map_err(|e| e.to_string())?;
    Ok(())
}

/// Predicted observable for every condition at these unknown values.
pub fn predict(model: &SystemDocument, registry: &BehaviorRegistry, unknowns: &[Unknown], values: &[f64], conditions: &[Condition], duration: f64) -> Result<Vec<f64>, String> {
    let mut base = model.clone();
    for (u, v) in unknowns.iter().zip(values) {
        for t in &u.targets {
            bind(&mut base, registry, t, *v)?;
        }
    }
    let run = |c: &Condition| -> Result<f64, String> {
        let mut doc = base.clone();
        for (t, v) in &c.set {
            bind(&mut doc, registry, t, *v)?;
        }
        let series = system_builder::simulate(&doc, registry, duration, system_builder::config_for(&doc), std::slice::from_ref(&c.observable))?;
        let s = series.iter().find(|s| s.label == c.observable).ok_or_else(|| format!("no observable {}", c.observable))?;
        let tail: Vec<f64> = s.times.iter().zip(&s.values).filter(|(t, _)| **t >= c.window[0] && **t <= c.window[1]).map(|(_, v)| *v).collect();
        Ok(tail.iter().sum::<f64>() / tail.len().max(1) as f64)
    };
    // Conditions are independent runs: spread them over threads.
    let out = std::sync::Mutex::new(vec![Ok(0.0); conditions.len()]);
    std::thread::scope(|scope| {
        for (i, c) in conditions.iter().enumerate() {
            let out = &out;
            let run = &run;
            scope.spawn(move || {
                let r = run(c);
                out.lock().unwrap()[i] = r;
            });
        }
    });
    out.into_inner().unwrap().into_iter().collect()
}

fn solve(a: &[Vec<f64>], b: &[f64]) -> Option<Vec<f64>> {
    let n = b.len();
    let mut m: Vec<Vec<f64>> = a.iter().zip(b).map(|(r, v)| r.iter().copied().chain(std::iter::once(*v)).collect()).collect();
    for c in 0..n {
        let p = (c..n).max_by(|i, j| m[*i][c].abs().total_cmp(&m[*j][c].abs()))?;
        m.swap(c, p);
        if m[c][c].abs() < 1e-300 {
            return None;
        }
        for r in 0..n {
            if r != c {
                let f = m[r][c] / m[c][c];
                for k in c..=n {
                    m[r][k] -= f * m[c][k];
                }
            }
        }
    }
    Some((0..n).map(|i| m[i][n] / m[i][i]).collect())
}

fn inverse(a: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
    let n = a.len();
    let cols: Option<Vec<Vec<f64>>> = (0..n).map(|j| solve(a, &(0..n).map(|i| if i == j { 1. } else { 0. }).collect::<Vec<_>>())).collect();
    let cols = cols?;
    Some((0..n).map(|i| (0..n).map(|j| cols[j][i]).collect()).collect())
}

pub fn fit(model: &SystemDocument, registry: &BehaviorRegistry, unknowns: &[Unknown], conditions: &[Condition], duration: f64) -> Result<FitResult, String> {
    let (n, p) = (conditions.len(), unknowns.len());
    if n <= p {
        return Err(format!("{n} measurements cannot determine {p} unknowns with an uncertainty"));
    }
    let measured: Vec<f64> = conditions.iter().map(|c| c.measured).collect();
    let clamp = |x: &mut Vec<f64>| {
        for (v, u) in x.iter_mut().zip(unknowns) {
            if let Some(m) = u.minimum {
                *v = v.max(m);
            }
        }
    };
    let mut x: Vec<f64> = unknowns.iter().map(|u| u.initial).collect();
    let residual = |x: &[f64]| -> Result<(Vec<f64>, f64), String> {
        let y = predict(model, registry, unknowns, x, conditions, duration)?;
        let r: Vec<f64> = y.iter().zip(&measured).map(|(a, b)| a - b).collect();
        let ssr = r.iter().map(|v| v * v).sum();
        Ok((r, ssr))
    };
    let jacobian = |x: &[f64]| -> Result<Vec<Vec<f64>>, String> {
        let (r0, _) = residual(x)?;
        let mut j = vec![vec![0.; p]; n];
        for k in 0..p {
            let h = 1e-4 * x[k].abs().max(1e-6);
            let mut xp = x.to_vec();
            xp[k] += h;
            let (rp, _) = residual(&xp)?;
            for i in 0..n {
                j[i][k] = (rp[i] - r0[i]) / h;
            }
        }
        Ok(j)
    };
    let (mut r, mut ssr) = residual(&x)?;
    let mut lambda = 1e-3;
    let mut iterations = 0;
    for _ in 0..40 {
        iterations += 1;
        let j = jacobian(&x)?;
        let jtj: Vec<Vec<f64>> = (0..p).map(|a| (0..p).map(|b| (0..n).map(|i| j[i][a] * j[i][b]).sum()).collect()).collect();
        let jtr: Vec<f64> = (0..p).map(|a| -(0..n).map(|i| j[i][a] * r[i]).sum::<f64>()).collect();
        let mut improved = false;
        for _ in 0..12 {
            let damped: Vec<Vec<f64>> = (0..p).map(|a| (0..p).map(|b| jtj[a][b] + if a == b { lambda * jtj[a][a].max(1e-30) } else { 0. }).collect()).collect();
            let Some(step) = solve(&damped, &jtr) else { lambda *= 10.; continue };
            let mut trial: Vec<f64> = x.iter().zip(&step).map(|(a, b)| a + b).collect();
            clamp(&mut trial);
            let (rt, st) = residual(&trial)?;
            if st < ssr {
                let small = step.iter().zip(&x).all(|(d, v)| d.abs() <= 1e-7 * v.abs().max(1e-9));
                x = trial;
                r = rt;
                let gain = ssr - st;
                ssr = st;
                lambda = (lambda / 3.).max(1e-9);
                improved = true;
                if small || gain < 1e-12 * ssr.max(1e-30) {
                    return finish(model, registry, unknowns, conditions, duration, x, ssr, iterations, &jacobian);
                }
                break;
            }
            lambda *= 10.;
        }
        if !improved {
            break;
        }
    }
    finish(model, registry, unknowns, conditions, duration, x, ssr, iterations, &jacobian)
}

#[allow(clippy::too_many_arguments)]
fn finish(model: &SystemDocument, registry: &BehaviorRegistry, unknowns: &[Unknown], conditions: &[Condition], duration: f64, x: Vec<f64>, ssr: f64, iterations: usize, jacobian: &dyn Fn(&[f64]) -> Result<Vec<Vec<f64>>, String>) -> Result<FitResult, String> {
    let (n, p) = (conditions.len(), unknowns.len());
    let j = jacobian(&x)?;
    let jtj: Vec<Vec<f64>> = (0..p).map(|a| (0..p).map(|b| (0..n).map(|i| j[i][a] * j[i][b]).sum()).collect()).collect();
    let sigma2 = ssr / (n - p) as f64;
    let cov = inverse(&jtj).ok_or("the measurements do not determine every unknown (singular normal matrix)")?;
    let sd: Vec<f64> = (0..p).map(|k| (sigma2 * cov[k][k]).max(0.).sqrt()).collect();
    let correlation = (0..p).map(|a| (0..p).map(|b| cov[a][b] / (cov[a][a] * cov[b][b]).sqrt()).collect()).collect();
    let predicted = predict(model, registry, unknowns, &x, conditions, duration)?;
    Ok(FitResult {
        values: unknowns.iter().zip(&x).map(|(u, v)| (u.name.clone(), *v)).collect(),
        uncertainties: unknowns.iter().zip(&sd).map(|(u, s)| (u.name.clone(), *s)).collect(),
        correlation,
        residuals: conditions.iter().zip(&predicted).map(|(c, y)| (c.label.clone(), c.measured, *y)).collect(),
        rms: (ssr / n as f64).sqrt(),
        iterations,
    })
}

/// Commands that write fitted values into their targets as measured values
/// with uncertainty, citing the data they came from.
pub fn promote(unknowns: &[Unknown], result: &FitResult, data_path: &str, data_hash: &str) -> Vec<Command> {
    let mut out = Vec::new();
    for (u, ((_, v), (_, s))) in unknowns.iter().zip(result.values.iter().zip(&result.uncertainties)) {
        for t in &u.targets {
            out.push(Command::SetParameter {
                at: t.at.clone(),
                name: t.instance.clone(),
                parameter: t.parameter.clone(),
                binding: Some(ParameterBinding::Value {
                    value: *v,
                    unit: None,
                    provenance: Some(sim_inspect::Provenance::Measured { source: sim_inspect::SourceReference { artifact_hash: data_hash.into(), path: data_path.into(), line: None } }),
                    uncertainty: Some(*s),
                }),
            });
        }
    }
    out
}
