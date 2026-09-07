use nalgebra::{DMatrix, DVector};
use sim_domain_robot::articulated::transmission_coordinates::{
    AccelerationProblem, TransmissionCoordinateMap,
};

fn close(a: &[f64], b: &[f64], tol: f64) {
    assert_eq!(a.len(), b.len());
    for (i, (a, b)) in a.iter().zip(b).enumerate() {
        assert!(
            (a - b).abs() <= tol * (1.0 + a.abs().max(b.abs())),
            "{i}: {a} != {b}"
        );
    }
}

#[test]
fn chained_signed_ratios_preserve_affine_constraints_virtual_work_and_reactions() {
    let map =
        TransmissionCoordinateMap::new(5, &[(2, 0, 5.0), (1, 2, -2.0), (4, 3, 0.25)]).unwrap();
    assert_eq!(map.independent_coordinates(), &[0, 3]);
    let z = [0.3, -0.7];
    let rhs = [0.02, -0.03, 0.04];
    let x = map.expand(&z, &rhs).unwrap();
    let g = map.constraint_matrix();
    close(
        (g.clone() * DVector::from_column_slice(&x)).as_slice(),
        &rhs,
        1e-14,
    );
    let offset = map.expand(&[0.0; 2], &rhs).unwrap();
    close(
        (&map.matrix() * DVector::from_column_slice(&z) + DVector::from_vec(offset)).as_slice(),
        &x,
        1e-14,
    );
    let force = [1.1, -2.3, 0.7, 4.0, 0.9];
    let velocity = map.expand(&z, &[0.0; 3]).unwrap();
    let projected = map.project_forces(&force).unwrap();
    let work: f64 = force.iter().zip(velocity).map(|(f, v)| f * v).sum();
    let reduced_work: f64 = projected.iter().zip(z).map(|(f, v)| f * v).sum();
    close(&[work], &[reduced_work], 1e-14);
    let lambda = DVector::from_vec(vec![3.0, -1.0, 0.5]);
    let reaction = g.transpose() * &lambda;
    let (recovered, remainder) = map.recover_reactions(reaction.as_slice()).unwrap();
    close(&recovered, lambda.as_slice(), 1e-14);
    close(&remainder, &[0.0; 2], 1e-14);
    let (_, remainder) = map.recover_reactions(&force).unwrap();
    close(&remainder, &projected, 1e-14);
}

#[test]
fn geared_inertia_and_output_load_match_analytic_acceleration() {
    let map = TransmissionCoordinateMap::new(2, &[(0, 1, 5.0)]).unwrap();
    let mass = DMatrix::from_diagonal(&DVector::from_vec(vec![2.0, 3.0]));
    let solution = map
        .solve_accelerations(&AccelerationProblem {
            mass: &mass,
            force: &[10.0, -3.0],
            transmission_rhs: &[0.0],
            other_jacobian: &DMatrix::zeros(0, 2),
            other_rhs: &[],
            other_cfm: &[],
        })
        .unwrap();
    let driver_acceleration = (10.0 - 3.0 / 5.0) / (2.0 + 3.0 / 25.0);
    close(
        &solution.accelerations,
        &[driver_acceleration, driver_acceleration / 5.0],
        1e-14,
    );
    close(
        &solution.transmission_reactions,
        &[2.0 * driver_acceleration - 10.0],
        1e-14,
    );
    close(&solution.dynamics_residual, &[0.0; 2], 1e-14);
    assert_eq!(solution.linear_system_dimension, 1);
}

#[test]
fn projected_solve_matches_independent_full_kkt_with_remaining_regularized_loops() {
    let relations = [(0, 1, 5.0), (2, 1, -0.5)];
    let map = TransmissionCoordinateMap::new(4, &relations).unwrap();
    let seed = DMatrix::from_row_slice(
        4,
        4,
        &[
            2.0, 0.2, -0.1, 0.7, 0.0, 3.0, 0.4, -0.3, 0.0, 0.0, 1.0, 0.2, 0.0, 0.0, 0.0, 2.0,
        ],
    );
    let mass = seed.transpose() * seed;
    let other = DMatrix::from_row_slice(2, 4, &[0.3, 0.0, 0.7, -1.0, 0.6, 0.0, 1.4, -2.0]);
    let force = [1.3, -0.4, 2.0, -0.2];
    let trans_rhs = [0.01, -0.03];
    let other_rhs = [0.02, 0.04];
    let cfm = [1e-4, 2e-4];
    let result = map
        .solve_accelerations(&AccelerationProblem {
            mass: &mass,
            force: &force,
            transmission_rhs: &trans_rhs,
            other_jacobian: &other,
            other_rhs: &other_rhs,
            other_cfm: &cfm,
        })
        .unwrap();
    // Assemble G independently from relation coefficients, not map.matrix().
    let mut g = DMatrix::zeros(4, 4);
    for (i, &(driver, driven, ratio)) in relations.iter().enumerate() {
        g[(i, driver)] = 1.0;
        g[(i, driven)] = -ratio;
    }
    g.view_mut((2, 0), (2, 4)).copy_from(&other);
    let mut kkt = DMatrix::zeros(8, 8);
    kkt.view_mut((0, 0), (4, 4)).copy_from(&mass);
    kkt.view_mut((0, 4), (4, 4)).copy_from(&(-g.transpose()));
    kkt.view_mut((4, 0), (4, 4)).copy_from(&g);
    kkt[(6, 6)] = cfm[0];
    kkt[(7, 7)] = cfm[1];
    let rhs = DVector::from_vec([force.to_vec(), trans_rhs.to_vec(), other_rhs.to_vec()].concat());
    let reference = kkt.clone().lu().solve(&rhs).unwrap();
    close(
        &result.accelerations,
        reference.rows(0, 4).as_slice(),
        1e-11,
    );
    close(
        &result.transmission_reactions,
        reference.rows(4, 2).as_slice(),
        1e-11,
    );
    close(
        &result.other_reactions,
        reference.rows(6, 2).as_slice(),
        1e-11,
    );
    close(&result.dynamics_residual, &[0.0; 4], 1e-11);
    close(&result.transmission_residual, &[0.0; 2], 1e-11);
    close(&result.other_constraint_residual, &[0.0; 2], 1e-11);
    assert_eq!(result.linear_system_dimension, 4);
    // Finite transmission regularization is a DIFFERENT mechanical problem.
    kkt[(4, 4)] = 0.01;
    kkt[(5, 5)] = 0.01;
    let regularized = kkt.lu().solve(&rhs).unwrap();
    assert!((regularized.rows(0, 4) - reference.rows(0, 4)).norm() > 1e-5);
}

#[test]
fn invalid_cycles_dimensions_and_unrepresentable_scales_are_rejected() {
    for relations in [
        vec![(0, 1, 0.0)],
        vec![(0, 1, f64::NAN)],
        vec![(0, 3, 1.0)],
        vec![(0, 0, 1.0)],
        vec![(0, 1, 2.0), (1, 0, 0.5)],
        vec![(0, 1, 1e-300), (1, 2, 1e-300)],
    ] {
        assert!(TransmissionCoordinateMap::new(3, &relations).is_err());
    }
    let map = TransmissionCoordinateMap::new(2, &[(0, 1, 5.0)]).unwrap();
    assert!(map.expand(&[1.0, 2.0], &[0.0]).is_err());
    assert!(map.expand(&[1.0], &[f64::INFINITY]).is_err());
    assert!(map.project_forces(&[1.0]).is_err());
    let empty = TransmissionCoordinateMap::new(0, &[]).unwrap();
    assert!(empty.expand(&[], &[]).unwrap().is_empty());
}
