//! Conditional lower bounds for max-norm affine residuals on a finite box.
use crate::least_squares::VariableBound;
use nalgebra::{DMatrix, DVector};
use serde::Serialize;
#[derive(Debug, Serialize)]
pub struct AffineResidualBound {
    pub least_squares_values: Vec<f64>,
    pub least_squares_residuals: Vec<f64>,
    pub least_squares_maximum_residual: f64,
    pub dual_weights: Vec<f64>,
    pub dual_column_residual_maximum: f64,
    pub maximum_residual_lower_bound: f64,
}
/// For r=A*x+b, any dual w gives ||r||inf >=
/// (w^T*b + min_{lo<=x<=hi} w^T*A*x)/||w||1. A least-squares residual supplies
/// a useful w; the box correction accounts for imperfect orthogonality/rank
/// truncation. This is floating-point linear algebra, not interval arithmetic.
/// Bounds concern the supplied affine model only. A bound below tolerance does
/// not prove feasibility; the unconstrained LS candidate may leave the box.
pub fn affine_residual_lower_bound(
    a: &DMatrix<f64>,
    b: &DVector<f64>,
    bounds: &[VariableBound],
) -> Result<AffineResidualBound, String> {
    if a.nrows() == 0
        || a.ncols() == 0
        || a.nrows() != b.len()
        || a.ncols() != bounds.len()
        || a.iter().chain(b.iter()).any(|v| !v.is_finite())
        || bounds
            .iter()
            .any(|v| !v.lower.is_finite() || !v.upper.is_finite() || v.lower > v.upper)
    {
        return Err(
            "finite nonempty affine system and matched finite variable box required".into(),
        );
    }
    let svd = a.clone().svd(true, true);
    let threshold = (svd.singular_values.amax() * 1e-12).max(f64::MIN_POSITIVE);
    let x = svd.solve(&(-b), threshold).map_err(str::to_owned)?;
    let r = a * &x + b;
    let mut w = r.clone();
    if w.dot(b) < 0.0 {
        w = -w;
    }
    let column = a.transpose() * &w;
    let norm = w.iter().map(|v| v.abs()).sum::<f64>();
    let numerator = w.dot(b)
        + column
            .iter()
            .zip(bounds)
            .map(|(c, v)| c * if *c >= 0.0 { v.lower } else { v.upper })
            .sum::<f64>();
    let lower = if norm > 0.0 {
        (numerator / norm).max(0.0)
    } else {
        0.0
    };
    if x.iter()
        .chain(r.iter())
        .chain(w.iter())
        .any(|v| !v.is_finite())
        || !lower.is_finite()
    {
        return Err("nonfinite affine certificate".into());
    }
    Ok(AffineResidualBound {
        least_squares_values: x.as_slice().to_vec(),
        least_squares_residuals: r.as_slice().to_vec(),
        least_squares_maximum_residual: r.amax(),
        dual_weights: w.as_slice().to_vec(),
        dual_column_residual_maximum: column.amax(),
        maximum_residual_lower_bound: lower,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn separates_unrepresentable_balance_from_rank_deficiency_and_box_limits() {
        let a = DMatrix::from_row_slice(2, 1, &[1., 1.]);
        let b = DVector::from_vec(vec![-2., 2.]);
        let bounds = [VariableBound {
            lower: -10.,
            upper: 10.,
        }];
        let r = affine_residual_lower_bound(&a, &b, &bounds).unwrap();
        assert!((r.maximum_residual_lower_bound - 2.).abs() < 1e-12);
        for i in 0..=100 {
            let x = -10. + i as f64 / 5.;
            assert!(
                (a.clone() * DVector::from_vec(vec![x]) + &b).amax() + 1e-12
                    >= r.maximum_residual_lower_bound
            );
        }
        let a = DMatrix::from_row_slice(2, 2, &[1., 1., 1., 1.]);
        let r = affine_residual_lower_bound(
            &a,
            &DVector::from_vec(vec![-2., -2.]),
            &[bounds[0].clone(), bounds[0].clone()],
        )
        .unwrap();
        assert!(r.least_squares_maximum_residual < 1e-12);
        assert!(r.maximum_residual_lower_bound < 1e-12);
        assert!(affine_residual_lower_bound(&a, &b, &bounds).is_err());
        // A truncated tiny singular direction can still matter over a wide
        // variable box. Its dual-column correction must prevent a false bound.
        let a = DMatrix::from_diagonal(&DVector::from_vec(vec![1., 1e-14]));
        let r = affine_residual_lower_bound(
            &a,
            &DVector::from_vec(vec![0., 2.]),
            &[
                bounds[0].clone(),
                VariableBound {
                    lower: -3e14,
                    upper: 3e14,
                },
            ],
        )
        .unwrap();
        assert!(r.least_squares_maximum_residual > 1.9);
        assert_eq!(r.maximum_residual_lower_bound, 0.0);
    }
}
