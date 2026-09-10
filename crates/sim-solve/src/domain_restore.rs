//! Local restoration along a parameter segment; no global domain or path certificate.
use serde::Serialize;
pub enum DomainPoint<T> {
    Accepted(T),
    Rejected(String),
}
#[derive(Debug, Serialize)]
pub struct DomainAttempt {
    pub fraction: f64,
    pub rejection: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct DomainRestoration<T> {
    pub values: Vec<f64>,
    pub fraction: f64,
    pub rejected_upper_fraction: Option<f64>,
    pub evaluation: T,
    pub attempts: Vec<DomainAttempt>,
    pub scope: &'static str,
}
/// Evaluate both endpoints, then bisect a known accepted/rejected bracket.
/// `Err` is fatal; `Rejected` denotes a model-domain rejection to bracket.
/// Domains need not be monotone: this finds one accepted point near a local
/// boundary, not the furthest accepted point or a valid path between endpoints.
pub fn bisect_evaluation_domain<T>(
    reference: &[f64],
    target: &[f64],
    iterations: usize,
    mut evaluate: impl FnMut(&[f64]) -> Result<DomainPoint<T>, String>,
) -> Result<DomainRestoration<T>, String> {
    if reference.is_empty()
        || reference.len() != target.len()
        || iterations == 0
        || iterations > 64
        || reference.iter().chain(target).any(|v| !v.is_finite())
    {
        return Err(
            "matched finite nonempty endpoints and 1..64 bisection iterations required".into(),
        );
    }
    let mut attempts = Vec::new();
    let mut test = |fraction: f64, values: &[f64]| -> Result<DomainPoint<T>, String> {
        let result = evaluate(values)?;
        attempts.push(DomainAttempt {
            fraction,
            rejection: match &result {
                DomainPoint::Accepted(_) => None,
                DomainPoint::Rejected(e) => Some(e.clone()),
            },
        });
        Ok(result)
    };
    let mut best = match test(0., reference)? {
        DomainPoint::Accepted(v) => v,
        DomainPoint::Rejected(e) => {
            return Err(format!("reference outside evaluation domain: {e}"));
        }
    };
    let scope = "Bisection of an accepted/rejected parameter bracket only. Retains an independently evaluated accepted point; does not prove a globally maximal fraction, physical feasibility, a valid continuous path or maximum robot speed.";
    match test(1., target)? {
        DomainPoint::Accepted(evaluation) => {
            return Ok(DomainRestoration {
                values: target.to_vec(),
                fraction: 1.,
                rejected_upper_fraction: None,
                evaluation,
                attempts,
                scope,
            });
        }
        DomainPoint::Rejected(_) => {}
    }
    let (mut lo, mut hi) = (0., 1.);
    let mut best_values = reference.to_vec();
    for _ in 0..iterations {
        let fraction = lo + 0.5 * (hi - lo);
        if fraction == lo || fraction == hi {
            break;
        }
        // Convex form avoids overflowing the difference of finite endpoints.
        let values = reference
            .iter()
            .zip(target)
            .map(|(a, b)| (1. - fraction) * a + fraction * b)
            .collect::<Vec<_>>();
        if values.iter().any(|v| !v.is_finite()) {
            return Err("nonfinite segment interpolation".into());
        }
        match test(fraction, &values)? {
            DomainPoint::Accepted(value) => {
                lo = fraction;
                best = value;
                best_values = values;
            }
            DomainPoint::Rejected(_) => hi = fraction,
        }
    }
    Ok(DomainRestoration {
        values: best_values,
        fraction: lo,
        rejected_upper_fraction: Some(hi),
        evaluation: best,
        attempts,
        scope,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restores_a_local_bracket_without_claiming_other_valid_intervals_are_absent() {
        let result = bisect_evaluation_domain(&[0.], &[1.], 24, |x| {
            Ok(if x[0] <= 0.3 || (0.7..=0.8).contains(&x[0]) {
                DomainPoint::Accepted(x[0])
            } else {
                DomainPoint::Rejected("outside".into())
            })
        })
        .unwrap();
        assert!(result.fraction <= 0.3);
        assert!(result.rejected_upper_fraction.unwrap() > 0.3);
        assert!(0.3 - result.fraction < 2f64.powi(-24));
        assert_eq!(result.values[0], result.evaluation);
        assert_eq!(result.attempts.len(), 26);
        let direct =
            bisect_evaluation_domain(&[0.], &[0.75], 24, |x| Ok(DomainPoint::Accepted(x[0])))
                .unwrap();
        assert_eq!(direct.fraction, 1.);
        assert_eq!(direct.attempts.len(), 2);
    }
    #[test]
    fn invalid_references_inputs_and_fatal_errors_are_not_hidden() {
        assert!(
            bisect_evaluation_domain(&[0.], &[1.], 8, |_| Ok(DomainPoint::<()>::Rejected(
                "bad".into()
            )))
            .unwrap_err()
            .contains("reference outside")
        );
        assert!(
            bisect_evaluation_domain(&[0.], &[1.], 8, |x| if x[0] == 0. {
                Ok(DomainPoint::Accepted(()))
            } else {
                Err("fatal".into())
            })
            .unwrap_err()
            .contains("fatal")
        );
        assert!(
            bisect_evaluation_domain(&[f64::NAN], &[1.], 8, |_| Ok(DomainPoint::Accepted(())))
                .is_err()
        );
    }
}
