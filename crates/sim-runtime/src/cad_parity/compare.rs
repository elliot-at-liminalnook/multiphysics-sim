//! Path-specific comparisons. Numeric acceptance is inclusive:
//! |native-reference| <= absolute + relative*|reference|.
//! At zero reference only the absolute allowance applies. Undefined or overflowed
//! relative errors remain absent, never encoded as JSON infinity or NaN.
use super::contract::*;
use serde_json::Value;

fn diagnostic(
    p: &ComparisonPolicy,
    path: String,
    status: ExecutionStatus,
    message: String,
    absolute_error: Option<f64>,
    relative_error: Option<f64>,
) -> Diagnostic {
    Diagnostic {
        step_id: p.step_id.clone(),
        path,
        family: p.family.clone(),
        status,
        message,
        unit: p.unit.clone(),
        absolute_error,
        relative_error,
        tolerance: match &p.metric {
            Metric::Exact => None,
            Metric::Numeric { tolerance } | Metric::PointSet { tolerance, .. } => {
                Some(tolerance.clone())
            }
        },
    }
}
fn failure(p: &ComparisonPolicy, path: &str, message: impl Into<String>) -> Diagnostic {
    diagnostic(
        p,
        path.into(),
        ExecutionStatus::Failed,
        message.into(),
        None,
        None,
    )
}
fn valid_tolerance(t: &Tolerance) -> bool {
    t.absolute.is_finite()
        && t.relative.is_finite()
        && t.absolute >= 0.0
        && t.relative >= 0.0
        && !t.justification.trim().is_empty()
}
/// Returns (accepted, representable absolute error, representable relative error).
/// Scaling prevents overflow in both subtraction and the tolerance sum.
fn numeric(a: f64, b: f64, t: &Tolerance) -> (bool, Option<f64>, Option<f64>) {
    let difference = (b - a).abs();
    let absolute = difference.is_finite().then_some(difference);
    let relative = if a == b {
        Some(0.0)
    } else if a == 0.0 {
        None
    } else {
        let r = if difference.is_finite() {
            difference / a.abs()
        } else {
            (b / a.abs() - a / a.abs()).abs()
        };
        r.is_finite().then_some(r)
    };
    if a == b {
        return (true, absolute, relative);
    }
    if t.absolute == 0.0 && t.relative == 0.0 {
        return (false, absolute, relative);
    }
    let direct_allowance = t.absolute + t.relative * a.abs();
    if difference.is_finite() && direct_allowance.is_finite() {
        return (difference <= direct_allowance, absolute, relative);
    }
    let scale = a.abs().max(b.abs()).max(t.absolute);
    let error = if difference.is_finite() {
        difference / scale
    } else {
        (b / scale - a / scale).abs()
    };
    let allowance = t.absolute / scale + t.relative * (a.abs() / scale);
    (error <= allowance, absolute, relative)
}
fn state(p: &ComparisonPolicy, side: &str, o: &Observation) -> Option<Diagnostic> {
    let (status, why) = match &o.value {
        ObservedValue::Present(_) => return None,
        ObservedValue::Missing(why) => (ExecutionStatus::Incomplete, format!("missing: {why}")),
        ObservedValue::Unsupported(why) => {
            (ExecutionStatus::Unsupported, format!("unsupported: {why}"))
        }
        ObservedValue::Invalid(why) => (
            ExecutionStatus::Failed,
            format!("invalid/non-finite: {why}"),
        ),
    };
    Some(diagnostic(
        p,
        p.path.clone(),
        status,
        format!("{side} {why}"),
        None,
        None,
    ))
}
fn escaped(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}
fn lossy_integer(n: &serde_json::Number) -> bool {
    n.as_u64().is_some_and(|x| x > (1u64 << 53))
        || n.as_i64().is_some_and(|x| x.unsigned_abs() > (1u64 << 53))
}
fn values(p: &ComparisonPolicy, path: &str, a: &Value, b: &Value, out: &mut Vec<Diagnostic>) {
    if matches!(p.metric, Metric::Numeric { .. })
        && [a, b].iter().any(|v| {
            v.is_null()
                || v.as_array().is_some_and(Vec::is_empty)
                || v.as_object().is_some_and(|o| o.is_empty())
        })
    {
        out.push(failure(
            p,
            path,
            "numeric observation is null or an empty container; missing is not zero",
        ));
        return;
    }
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => {
            if matches!(p.metric, Metric::Exact) {
                if a != b {
                    out.push(failure(p, path, "exact number mismatch"));
                }
                return;
            }
            // Numeric policies must never collapse distinct JSON integers through
            // binary64 conversion. Above 2^53 not every integer is representable;
            // require an exact policy there rather than inventing zero error.
            if lossy_integer(a) || lossy_integer(b) {
                out.push(failure(
                    p,
                    path,
                    "numeric integer exceeds lossless binary64 range; use exact comparison",
                ));
                return;
            }
            let (Some(a), Some(b)) = (a.as_f64(), b.as_f64()) else {
                out.push(failure(
                    p,
                    path,
                    "number is not representable as finite f64",
                ));
                return;
            };
            if !a.is_finite() || !b.is_finite() {
                out.push(failure(p, path, "invalid/non-finite number"));
                return;
            }
            let Metric::Numeric { tolerance } = &p.metric else {
                if a != b {
                    out.push(failure(p, path, "exact number mismatch"));
                }
                return;
            };
            let (ok, abs, rel) = numeric(a, b, tolerance);
            out.push(diagnostic(p, path.into(), if ok { ExecutionStatus::Passed }
                else { ExecutionStatus::Failed }, format!("reference={a}, native={b}; inclusive absolute + relative*|reference| allowance; relative error at zero is undefined unless equal"), abs, rel));
        }
        (Value::Array(a), Value::Array(b)) => {
            if a.len() != b.len() {
                out.push(failure(
                    p,
                    path,
                    format!("array lengths {} != {}", a.len(), b.len()),
                ));
            }
            for (index, (a, b)) in a.iter().zip(b).enumerate() {
                values(p, &format!("{path}/{index}"), a, b, out);
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            for (key, a) in a {
                let next = format!("{path}/{}", escaped(key));
                match b.get(key) {
                    Some(b) => values(p, &next, a, b, out),
                    None => out.push(failure(p, &next, "native object field missing")),
                }
            }
            for key in b.keys().filter(|key| !a.contains_key(*key)) {
                out.push(failure(
                    p,
                    &format!("{path}/{}", escaped(key)),
                    "reference object field missing",
                ));
            }
        }
        _ if a == b => {}
        _ => out.push(failure(p, path, "value/type mismatch")),
    }
}
fn has_number(v: &Value) -> bool {
    match v {
        Value::Number(_) => true,
        Value::Array(a) => a.iter().any(has_number),
        Value::Object(o) => o.values().any(has_number),
        _ => false,
    }
}
fn points(value: &Value, limit: usize) -> Result<Vec<[f64; 3]>, String> {
    let a = value.as_array().ok_or("point set is not an array")?;
    if a.is_empty() || a.len() > limit {
        return Err(format!("point count {} outside 1..={limit}", a.len()));
    }
    a.iter()
        .enumerate()
        .map(|(i, value)| {
            let row = value
                .as_array()
                .ok_or_else(|| format!("point /{i} is not an array"))?;
            if row.len() != 3 {
                return Err(format!("point /{i} requires three coordinates"));
            }
            let mut p = [0.0; 3];
            for (j, value) in row.iter().enumerate() {
                if value.as_number().is_some_and(lossy_integer) {
                    return Err(format!(
                        "point /{i}/{j} integer exceeds lossless binary64 range"
                    ));
                }
                p[j] = value
                    .as_f64()
                    .filter(|x| x.is_finite())
                    .ok_or_else(|| format!("point /{i}/{j} is invalid/non-finite"))?;
            }
            Ok(p)
        })
        .collect()
}
fn distance(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1]).hypot(a[2] - b[2])
}
fn directed(a: &[[f64; 3]], b: &[[f64; 3]]) -> f64 {
    a.iter()
        .map(|x| {
            b.iter()
                .map(|y| distance(x, y))
                .fold(f64::INFINITY, f64::min)
        })
        .fold(0.0, f64::max)
}
fn point_set(
    p: &ComparisonPolicy,
    a: &Value,
    b: &Value,
    t: &Tolerance,
    limit: usize,
) -> Diagnostic {
    // Hard cap keeps O(n*m) bounded even for a hostile manifest. It is a sample
    // metric, not a surface Hausdorff or topology equivalence claim.
    if limit == 0
        || limit > 4096
        || p.frame.as_deref().is_none_or(str::is_empty)
        || p.unit.as_deref().is_none_or(str::is_empty)
    {
        return failure(
            p,
            &p.path,
            "point-set policy requires frame, unit and max_points in 1..=4096",
        );
    }
    let (a, b) = match (points(a, limit), points(b, limit)) {
        (Ok(a), Ok(b)) => (a, b),
        (a, b) => {
            return failure(
                p,
                &p.path,
                format!(
                    "invalid point sets: reference={:?}, native={:?}",
                    a.err(),
                    b.err()
                ),
            );
        }
    };
    let error = directed(&a, &b).max(directed(&b, &a));
    let extent = a.iter().map(|x| distance(x, &[0.0; 3])).fold(0.0, f64::max);
    if !error.is_finite() || !extent.is_finite() {
        return failure(
            p,
            &p.path,
            "point metric/declared-frame radius overflowed; invalid observation",
        );
    }
    let relative = if extent > 0.0 {
        let ratio = error / extent;
        ratio.is_finite().then_some(ratio)
    } else if error == 0.0 {
        Some(0.0)
    } else {
        None
    };
    // Avoid lost displacement when extent dwarfs error: compare the actual error.
    let scale = extent.max(error).max(t.absolute);
    let allowance = t.absolute + t.relative * extent;
    let ok = if allowance.is_finite() {
        error <= allowance
    } else if scale == 0.0 {
        true
    } else {
        error / scale <= t.absolute / scale + t.relative * (extent / scale)
    };
    diagnostic(p, p.path.clone(), if ok { ExecutionStatus::Passed } else { ExecutionStatus::Failed },
        "symmetric sampled-vertex Hausdorff distance; relative scale=max reference radius about declared frame origin; topology compared separately".into(),
        Some(error), relative)
}
/// Observations are addressed by stable named fields; nested diagnostic paths use
/// JSON-pointer escaped segments. No observation or identity is normalized away.
pub fn compare(
    scenario: &Scenario,
    reference: &AdapterRun,
    native: &AdapterRun,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for p in &scenario.comparisons {
        let start = out.len();
        let a: Vec<_> = reference
            .receipts
            .iter()
            .filter(|r| r.step_id == p.step_id)
            .collect();
        let b: Vec<_> = native
            .receipts
            .iter()
            .filter(|r| r.step_id == p.step_id)
            .collect();
        if a.len() != 1 || b.len() != 1 {
            out.push(diagnostic(
                p,
                p.path.clone(),
                ExecutionStatus::Incomplete,
                "comparison requires exactly one receipt per adapter".into(),
                None,
                None,
            ));
            continue;
        }
        let (Some(a), Some(b)) = (
            a[0].observations.get(&p.path),
            b[0].observations.get(&p.path),
        ) else {
            out.push(diagnostic(
                p,
                p.path.clone(),
                ExecutionStatus::Incomplete,
                "named observation missing in one or both adapters".into(),
                None,
                None,
            ));
            continue;
        };
        if let Some(d) = state(p, "reference", a) {
            out.push(d);
        }
        if let Some(d) = state(p, "native", b) {
            out.push(d);
        }
        if out.len() != start {
            continue;
        }
        for (field, valid) in [
            (
                "owner",
                match &p.owners {
                    Some(owners) => {
                        !owners.reference.trim().is_empty()
                            && !owners.native.trim().is_empty()
                            && a.owner == owners.reference
                            && b.owner == owners.native
                    }
                    None => !a.owner.trim().is_empty() && a.owner == b.owner,
                },
            ),
            ("unit", a.unit == p.unit && b.unit == p.unit),
            ("frame", a.frame == p.frame && b.frame == p.frame),
            (
                "provenance",
                a.provenance == b.provenance
                    && (!p.require_provenance
                        || a.provenance.as_ref().is_some_and(|x| !x.trim().is_empty())),
            ),
            ("uncertainty", a.uncertainty == b.uncertainty),
        ] {
            if !valid {
                out.push(failure(
                    p,
                    &format!("{}/@{field}", p.path),
                    format!("{field} missing or mismatched"),
                ));
            }
        }
        let (ObservedValue::Present(a), ObservedValue::Present(b)) = (&a.value, &b.value) else {
            continue;
        };
        match &p.metric {
            Metric::Exact => {
                if a != b {
                    values(p, &p.path, a, b, &mut out);
                }
            }
            Metric::Numeric { tolerance } | Metric::PointSet { tolerance, .. }
                if !valid_tolerance(tolerance) =>
            {
                out.push(failure(p, &p.path, "invalid tolerance: finite nonnegative bounds and reference justification required"));
            }
            Metric::Numeric { .. } => {
                if !has_number(a) || !has_number(b) {
                    out.push(failure(
                        p,
                        &p.path,
                        "numeric policy has no numerical observation",
                    ));
                } else {
                    values(p, &p.path, a, b, &mut out);
                }
            }
            Metric::PointSet {
                tolerance,
                max_points,
            } => out.push(point_set(p, a, b, tolerance, *max_points)),
        }
        if out.len() == start {
            out.push(diagnostic(
                p,
                p.path.clone(),
                ExecutionStatus::Passed,
                "exact agreement".into(),
                None,
                None,
            ));
        }
        // A declared difference remains unresolved evidence even when values agree.
        if let Some(reason) = &p.deliberate_difference {
            out.push(diagnostic(
                p,
                p.path.clone(),
                ExecutionStatus::DeliberateDifference,
                format!("declared deliberate difference: {reason}"),
                None,
                None,
            ));
        }
    }
    out
}
#[cfg(test)]
#[path = "compare_fixtures.rs"]
mod fixtures;
