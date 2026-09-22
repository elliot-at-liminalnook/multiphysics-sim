//! Unpacked finite dense products for small matrices with many exact zeros.
//! These are numerical kernels, not sparsity approximations: no threshold drops
//! a coefficient. Different accumulation order from a platform GEMM can produce
//! roundoff differences. Callers must retain their physical residual checks.
use nalgebra::DMatrix;

fn finite_inputs(a: &DMatrix<f64>, b: &DMatrix<f64>) -> Result<(), &'static str> {
    if a.iter().chain(b.iter()).any(|x| !x.is_finite()) {
        Err("dense product requires finite inputs")
    } else { Ok(()) }
}

fn finite_output(out: DMatrix<f64>) -> Result<DMatrix<f64>, &'static str> {
    if out.iter().any(|x| !x.is_finite()) {
        Err("dense product overflow or nonfinite result")
    } else { Ok(out) }
}

/// Compute A B in column-major storage without packing either operand.
/// Exact zero entries of B avoid an entire column update. All inputs are checked
/// first, so zero skipping cannot hide a nonfinite value in A.
pub fn product(a: &DMatrix<f64>, b: &DMatrix<f64>) -> Result<DMatrix<f64>, &'static str> {
    if a.ncols() != b.nrows() { return Err("dense product dimension mismatch"); }
    finite_inputs(a,b)?;
    let (rows,inner,cols)=(a.nrows(),a.ncols(),b.ncols());
    let mut out=DMatrix::zeros(rows,cols);
    if rows==0 || inner==0 || cols==0 { return Ok(out); }
    for (j,dst) in out.as_mut_slice().chunks_exact_mut(rows).enumerate() {
        for k in 0..inner {
            let scale=b[(k,j)];
            if scale==0.0 { continue; }
            let column=&a.as_slice()[k*rows..(k+1)*rows];
            for (value,coefficient) in dst.iter_mut().zip(column) {
                *value += coefficient*scale;
            }
        }
    }
    finite_output(out)
}

/// Compute A^T B without constructing A^T or a packed GEMM buffer. Exact zeros
/// in A avoid a row update; no finite nonzero coefficient is omitted.
///
/// ```
/// use nalgebra::DMatrix;
/// use sim_solve::dense_products::transpose_product;
/// let a=DMatrix::from_row_slice(2,2,&[1.0,2.0,3.0,4.0]);
/// assert_eq!(transpose_product(&a,&a).unwrap(),
///     DMatrix::from_row_slice(2,2,&[10.0,14.0,14.0,20.0]));
/// ```
pub fn transpose_product(a: &DMatrix<f64>, b: &DMatrix<f64>) -> Result<DMatrix<f64>, &'static str> {
    if a.nrows() != b.nrows() { return Err("transpose product dimension mismatch"); }
    finite_inputs(a,b)?;
    let (rows,cols,inner)=(a.ncols(),b.ncols(),a.nrows());
    let mut out=DMatrix::zeros(rows,cols);
    if rows==0 || inner==0 || cols==0 { return Ok(out); }
    for (i,left) in a.as_slice().chunks_exact(inner).enumerate() {
        for (k,&scale) in left.iter().enumerate() {
            if scale==0.0 { continue; }
            for (dst,right) in out.as_mut_slice().chunks_exact_mut(rows)
                .zip(b.as_slice().chunks_exact(inner)) {
                dst[i] += scale*right[k];
            }
        }
    }
    finite_output(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangular_and_sparse_products_match_independent_gemm() {
        for (m,k,n) in [(0,0,0),(0,3,5),(5,0,4),(5,3,0),(1,1,1),(2,3,7),(7,5,2),(31,27,18),(54,54,18)] {
            let a=DMatrix::from_fn(m,k,|i,j| ((i*17+j*7)%19) as f64/13.0-0.6);
            let b=DMatrix::from_fn(k,n,|i,j| if (i+j)%3==0 {0.0} else {((i*11+j*3)%23) as f64/17.0-0.8});
            let expected=&a*&b;
            let actual=product(&a,&b).unwrap();
            assert_eq!(actual.shape(),(m,n));
            for (x,y) in actual.iter().zip(expected.iter()) {assert!((x-y).abs()<=2e-13*(1.0+y.abs()));}
            let transposed=transpose_product(&a.transpose(),&b).unwrap();
            for (x,y) in transposed.iter().zip(expected.iter()) {assert!((x-y).abs()<=2e-13*(1.0+y.abs()));}
        }
    }

    #[test]
    fn exact_zeros_do_not_drop_small_finite_coefficients() {
        let a=DMatrix::from_row_slice(2,3,&[1e-200,2.0,-3.0,4.0,-5.0,6.0]);
        let b=DMatrix::from_row_slice(3,2,&[1.0,-0.0,0.0,1e-200,0.0,0.0]);
        let expected=DMatrix::from_row_slice(2,2,&[1e-200,2e-200,4.0,-5e-200]);
        assert_eq!(product(&a,&b).unwrap(),expected);
        assert_eq!(transpose_product(&a.transpose(),&b).unwrap(),expected);
    }

    #[test]
    fn invalid_shapes_nonfinite_inputs_and_overflow_are_rejected() {
        assert!(product(&DMatrix::zeros(2,3),&DMatrix::zeros(2,4)).is_err());
        assert!(transpose_product(&DMatrix::zeros(2,3),&DMatrix::zeros(3,4)).is_err());
        for value in [f64::NAN,f64::INFINITY,f64::NEG_INFINITY] {
            let a=DMatrix::from_element(1,1,value);
            let zero=DMatrix::zeros(1,1);
            assert!(product(&a,&zero).is_err());
            assert!(product(&zero,&a).is_err());
            assert!(transpose_product(&a,&zero).is_err());
        }
        let huge=DMatrix::from_element(1,1,f64::MAX);
        assert!(product(&huge,&huge).is_err());
        assert!(transpose_product(&huge,&huge).is_err());
    }
}
