use sim_domain_control::optimization::*;
#[test]
fn solves_scaled_box_quadratic_with_boundary_optimum_and_monotone_steps(){
    let settings=ProjectedAscentSettings{iterations:100,step_fraction:0.4,backtracks:30};
    let result=projected_ascent(&[0.,0.],&[[-1.,1.],[-10.,10.]],&settings,|x|Ok((-(x[0]-2.).powi(2)-(x[1]-3.).powi(2),vec![-2.*(x[0]-2.),-2.*(x[1]-3.)]))).unwrap();
    assert!((result.parameters[0]-1.).abs()<1e-8);assert!((result.parameters[1]-3.).abs()<1e-6);
    assert!(result.objectives.windows(2).all(|w|w[1]>w[0]));
    assert!(projected_ascent(&[2.],&[[-1.,1.]],&settings,|_|Ok((0.,vec![0.]))).is_err());
    assert!(projected_ascent(&[0.],&[[-1.,1.]],&settings,|_|Err("physics failed".into())).unwrap_err().contains("physics failed"));
    assert!(projected_ascent(&[0.],&[[-1.,1.]],&settings,|_|Ok((f64::NAN,vec![0.]))).is_err());
}
