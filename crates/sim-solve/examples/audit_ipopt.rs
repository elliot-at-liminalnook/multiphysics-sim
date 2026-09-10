//! Native constrained-solver checks, including the official HS071 example.
use sim_solve::{ipopt::*, least_squares::VariableBound};
struct Hs071;
impl NlpProblem for Hs071 {
    fn evaluate(&mut self, x: &[f64]) -> Result<NlpEvaluation, String> {
        Ok(NlpEvaluation {
            objective: x[0] * x[3] * (x[0] + x[1] + x[2]) + x[2],
            constraints: vec![x.iter().product(), x.iter().map(|v| v * v).sum()],
        })
    }
    fn derivatives(&mut self, x: &[f64]) -> Result<NlpDerivatives, String> {
        Ok(NlpDerivatives {
            objective_gradient: vec![
                x[3] * (2. * x[0] + x[1] + x[2]),
                x[0] * x[3],
                x[0] * x[3] + 1.,
                x[0] * (x[0] + x[1] + x[2]),
            ],
            constraint_jacobian: vec![
                x[1] * x[2] * x[3],
                x[0] * x[2] * x[3],
                x[0] * x[1] * x[3],
                x[0] * x[1] * x[2],
                2. * x[0],
                2. * x[1],
                2. * x[2],
                2. * x[3],
            ],
        })
    }
}
struct Simple {
    mode: &'static str,
}
// Analytic load-sharing oracle: fA+fB=1, fA<=1-v/4, fB<=1-v/2.
// Summing capacities proves v<=4/3, attained at fA=2/3, fB=1/3.
// These are explicit synthetic capacities, not this robot's physical limits.
struct SupportSpeed;
impl NlpProblem for SupportSpeed {
    fn evaluate(&mut self, x: &[f64]) -> Result<NlpEvaluation, String> {
        Ok(NlpEvaluation {
            objective: -x[0],
            constraints: vec![x[1] + x[2], x[1] + x[0] / 4., x[2] + x[0] / 2.],
        })
    }
    fn derivatives(&mut self, _: &[f64]) -> Result<NlpDerivatives, String> {
        Ok(NlpDerivatives {
            objective_gradient: vec![-1., 0., 0.],
            constraint_jacobian: vec![1., 1., 0.25, 1., 0.5, 1.],
        })
    }
}
impl NlpProblem for Simple {
    fn evaluate(&mut self, x: &[f64]) -> Result<NlpEvaluation, String> {
        Ok(NlpEvaluation {
            objective: (x[0] - 0.5).powi(2),
            constraints: vec![x[0] * x[0]],
        })
    }
    fn derivatives(&mut self, x: &[f64]) -> Result<NlpDerivatives, String> {
        if self.mode == "panic" {
            panic!("deliberate derivative callback panic");
        }
        Ok(NlpDerivatives {
            objective_gradient: vec![if self.mode == "nonfinite" {
                f64::NAN
            } else {
                2. * (x[0] - 0.5)
            }],
            constraint_jacobian: vec![2. * x[0]],
        })
    }
    fn intermediate(&mut self, _: &IpoptIteration) -> bool {
        self.mode != "cancel"
    }
}
fn run() -> Result<(), String> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: audit_ipopt /verified/path/to/libipopt.dylib")?;
    // Documented input: pinned CasADi 3.8.0 native Ipopt 3.14.19 distribution,
    // whose installed headers declare f64/i32/C bool. No Python is invoked.
    let library = unsafe { IpoptLibrary::load_f64_i32_c_bool(std::path::Path::new(&path)) }?;
    let config = IpoptConfig {
        maximum_iterations: 100,
        maximum_callback_evaluations: 1000,
        tolerance: 1e-9,
        constraint_tolerance: 1e-9,
        initial_bound_push: 0.01,
        initial_bound_fraction: 0.01,
    };
    let bounds = vec![
        VariableBound {
            lower: 1.,
            upper: 5.
        };
        4
    ];
    let constraints = vec![
        VariableBound {
            lower: 25.,
            upper: f64::INFINITY,
        },
        VariableBound {
            lower: 40.,
            upper: 40.,
        },
    ];
    let pattern = (0..2)
        .flat_map(|r| (0..4).map(move |c| (r, c)))
        .collect::<Vec<_>>();
    let hs = library.solve(
        &[1., 5., 5., 1.],
        &bounds,
        &constraints,
        &pattern,
        &config,
        &mut Hs071,
    )?;
    assert_eq!(hs.native_status, 0);
    assert!(hs.maximum_constraint_or_bound_violation.unwrap() < 1e-7);
    for (x, expected) in hs
        .values
        .iter()
        .zip([1., 4.74299963, 3.82114998, 1.37940829])
    {
        assert!((x - expected).abs() < 2e-6);
    }
    assert!((hs.final_evaluation.as_ref().unwrap().objective - 17.01401729).abs() < 1e-7);
    let small_bounds = [VariableBound {
        lower: -1.,
        upper: 1.,
    }];
    let impossible = [VariableBound {
        lower: 4.,
        upper: f64::INFINITY,
    }];
    let feasible = [VariableBound {
        lower: 0.04,
        upper: f64::INFINITY,
    }];
    let mut rows = vec![serde_json::json!({"case":"hs071","result":hs})];
    for push in [0.01, 1e-8] {
        let mut c = config.clone();
        c.initial_bound_push = push;
        c.initial_bound_fraction = push;
        let result = library.solve(
            &[0.],
            &[VariableBound {
                lower: 0.,
                upper: 1.,
            }],
            &[VariableBound {
                lower: 0.,
                upper: f64::INFINITY,
            }],
            &[(0, 0)],
            &c,
            &mut Simple { mode: "cancel" },
        )?;
        assert!(result.cancelled);
        assert!((result.values[0] - push).abs() < 1e-14);
        assert_eq!(result.maximum_constraint_or_bound_violation, Some(0.));
        rows.push(serde_json::json!({"case":"initial_bound_distance","push":push,"result":result}));
    }
    for (push, fraction) in [(0., 0.01), (f64::NAN, 0.01), (0.01, 0.), (0.01, 0.51)] {
        let mut c = config.clone();
        c.initial_bound_push = push;
        c.initial_bound_fraction = fraction;
        assert!(
            library
                .solve(
                    &[1., 5., 5., 1.],
                    &bounds,
                    &constraints,
                    &pattern,
                    &c,
                    &mut Hs071
                )
                .is_err()
        );
    }
    let support = library.solve(
        &[0.2, 0.5, 0.5],
        &[
            VariableBound {
                lower: 0.,
                upper: 4.,
            },
            VariableBound {
                lower: 0.,
                upper: 1.,
            },
            VariableBound {
                lower: 0.,
                upper: 1.,
            },
        ],
        &[
            VariableBound {
                lower: 1.,
                upper: 1.,
            },
            VariableBound {
                lower: f64::NEG_INFINITY,
                upper: 1.,
            },
            VariableBound {
                lower: f64::NEG_INFINITY,
                upper: 1.,
            },
        ],
        &[(0, 1), (0, 2), (1, 0), (1, 1), (2, 0), (2, 2)],
        &config,
        &mut SupportSpeed,
    )?;
    assert_eq!(support.native_status, 0);
    assert!(support.maximum_constraint_or_bound_violation.unwrap() < 1e-8);
    for (value, expected) in support.values.iter().zip([4. / 3., 2. / 3., 1. / 3.]) {
        assert!((value - expected).abs() < 1e-7);
    }
    rows.push(serde_json::json!({"case":"analytic_support_speed","result":support}));
    let bad = library.solve(
        &[0.5],
        &small_bounds,
        &impossible,
        &[(0, 0)],
        &config,
        &mut Simple { mode: "ordinary" },
    )?;
    assert_ne!(bad.native_status, 0);
    assert!(bad.maximum_constraint_or_bound_violation.unwrap() >= 3. - 1e-9);
    rows.push(serde_json::json!({"case":"infeasible","result":bad}));
    for mode in ["nonfinite", "panic", "cancel", "budget"] {
        let mut c = config.clone();
        if mode == "budget" {
            c.maximum_callback_evaluations = 3;
        }
        let r = library.solve(
            &[0.7],
            &small_bounds,
            &feasible,
            &[(0, 0)],
            &c,
            &mut Simple { mode },
        )?;
        assert_ne!(r.native_status, 0);
        if mode == "nonfinite" {
            assert!(r.rejected_callbacks > 0);
        }
        if mode == "panic" {
            assert!(r.callback_panicked);
        }
        if mode == "cancel" {
            assert!(r.cancelled);
        }
        if mode == "budget" {
            assert!(r.budget_exhausted);
            assert!(r.value_evaluations + r.derivative_evaluations <= 3);
        }
        rows.push(serde_json::json!({"case":mode,"result":r}));
    }
    assert!(
        library
            .solve(
                &[0.7],
                &small_bounds,
                &feasible,
                &[(0, 0), (0, 0)],
                &config,
                &mut Simple { mode: "ordinary" }
            )
            .is_err()
    );
    let recovered = library.solve(
        &[1., 5., 5., 1.],
        &bounds,
        &constraints,
        &pattern,
        &config,
        &mut Hs071,
    )?;
    assert_eq!(recovered.native_status, 0);
    assert!(recovered.maximum_constraint_or_bound_violation.unwrap() < 1e-7);
    println!(
        "{}",
        serde_json::json!({"ipopt_version":library.version,"cases":rows,"duplicate_sparsity_rejected":true,"successful_solve_after_callback_failures":true,"scope":"Native Rust-to-Ipopt interface audit, not robot gait feasibility or speed."})
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
