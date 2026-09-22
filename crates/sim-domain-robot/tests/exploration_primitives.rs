//! Analytic calculations only: no robot/environment rollout or search.
use nalgebra::{DMatrix, DVector};
use sim_domain_robot::{
    actuator_envelope as drive, contact_feasibility as contact, reduction::LocalReduction,
};
fn near(a: f64, b: f64) {
    assert!(
        (a - b).abs() < 1e-9 * (1. + a.abs() + b.abs()),
        "{a} != {b}"
    );
}
fn drive_config() -> drive::Config {
    drive::Config {
        resistance_ohm: 2.,
        resistance_temperature_coefficient_per_k: 0.004,
        reference_temperature_k: 300.,
        motor_constant_nm_a: 0.1,
        gear_ratio: 10.,
        gear_efficiency: 0.8,
        current_limit_a: 4.,
        viscous_friction_nm_s_rad: 0.01,
        coulomb_friction_nm: 0.02,
        friction_speed_rad_s: 0.1,
    }
}
#[test]
fn reduction_preserves_virtual_work_and_kinetic_energy() {
    let j = DMatrix::from_row_slice(3, 2, &[1., 0., 0., 1., 2., -3.]);
    let m = DMatrix::from_diagonal(&DVector::from_vec(vec![2., 3., 5.]));
    let map = LocalReduction::new(j, DVector::from_vec(vec![0., 0., 4.])).unwrap();
    let u = [0.7, -0.2];
    let v = map.velocity(&u).unwrap();
    let f = [3., -5., 2.];
    let g = map.force(&f).unwrap();
    near(
        v.iter().zip(f).map(|(v, f)| v * f).sum(),
        u.iter().zip(g).map(|(u, g)| u * g).sum(),
    );
    let r = map.inertia(&m).unwrap();
    let vf = DVector::from_vec(v);
    let ur = DVector::from_column_slice(&u);
    near((vf.transpose() * m * vf)[0], (ur.transpose() * r * ur)[0]);
    assert_eq!(map.acceleration(&[1., 2.]).unwrap(), vec![1., 2., 0.]);
}
#[test]
fn reduction_rejects_singular_nonphysical_and_mismatched_inputs() {
    assert!(LocalReduction::new(DMatrix::zeros(2, 1), DVector::zeros(2)).is_err());
    let map = LocalReduction::new(DMatrix::identity(2, 2), DVector::zeros(2)).unwrap();
    assert!(map.force(&[1.]).is_err());
    assert!(
        map.inertia(&DMatrix::from_diagonal(&DVector::from_vec(vec![1., -1.])))
            .is_err()
    );
    assert!(map.velocity(&[f64::INFINITY, 0.]).is_err());
}
#[test]
fn dc_drive_power_balance_holds_during_drive_braking_and_regeneration() {
    let c = drive_config();
    for speed in [-8., -2., 0., 2., 8.] {
        for duty in [-1., -0.2, 0., 0.2, 1.] {
            let r = c
                .evaluate(drive::Input {
                    duty,
                    supply_voltage_v: 12.,
                    temperature_k: 300.,
                    output_speed_rad_s: speed,
                })
                .unwrap();
            near(r.electrical_power_w, r.mechanical_power_w + r.dissipation_w);
            near(r.electrical_power_w, 12. * r.bus_current_a);
            assert!(r.dissipation_w >= 0.);
        }
    }
    let r = c
        .evaluate(drive::Input {
            duty: 0.2,
            supply_voltage_v: 12.,
            temperature_k: 300.,
            output_speed_rad_s: 8.,
        })
        .unwrap();
    assert!(r.bus_current_a < 0.);
    assert!(r.output_torque_nm < 0.);
}
#[test]
fn voltage_temperature_current_and_overspeed_are_not_hidden() {
    let c = drive_config();
    let input = drive::Input {
        duty: 1.,
        supply_voltage_v: 6.,
        temperature_k: 300.,
        output_speed_rad_s: 0.,
    };
    let low = c.evaluate(input).unwrap();
    let hot = c
        .evaluate(drive::Input {
            temperature_k: 400.,
            ..input
        })
        .unwrap();
    assert!(hot.output_torque_nm < low.output_torque_nm);
    let high = c
        .evaluate(drive::Input {
            supply_voltage_v: 12.,
            ..input
        })
        .unwrap();
    assert!(!high.current_limit_satisfied);
    assert!(high.output_torque_nm > low.output_torque_nm);
    let overspeed = c
        .evaluate(drive::Input {
            output_speed_rad_s: 100.,
            ..input
        })
        .unwrap();
    assert!(overspeed.feasible_torque_interval_nm.is_none());
    near(
        drive::winding_error_retention(2., 0.004, 0.002).unwrap(),
        (-1_f64).exp(),
    );
    assert!(c.evaluate(drive::Input { duty: 1.1, ..input }).is_err());
    assert!(
        c.evaluate(drive::Input {
            temperature_k: 0.,
            ..input
        })
        .is_err()
    );
}
fn standing() -> contact::Request {
    contact::Request {
        reference_world_m: [0., 0., 0.],
        required_wrench_world: [0., 0., 20., 0., 0., 0.],
        contacts: vec![
            contact::Contact {
                point_world_m: [-1., 0., 0.],
                normal_world: [0., 0., 1.],
                force_world_n: [0., 0., 10.],
                friction_coefficient: 0.5,
                maximum_normal_force_n: 20.,
                enabled: true,
            },
            contact::Contact {
                point_world_m: [1., 0., 0.],
                normal_world: [0., 0., 1.],
                force_world_n: [0., 0., 10.],
                friction_coefficient: 0.5,
                maximum_normal_force_n: 20.,
                enabled: true,
            },
        ],
        motor_torques_nm: vec![1.],
        motor_torque_bounds_nm: vec![[-2., 2.]],
        force_tolerance_n: 1e-8,
        moment_tolerance_nm: 1e-8,
        torque_tolerance_nm: 1e-8,
    }
}
#[test]
fn support_rejects_tip_slip_disabled_contact_and_actuator_overload() {
    assert!(
        contact::evaluate(standing())
            .unwrap()
            .supplied_allocation_passes
    );
    let mut tip = standing();
    tip.contacts[0].force_world_n[2] = 0.;
    tip.contacts[1].force_world_n[2] = 20.;
    let r = contact::evaluate(tip).unwrap();
    assert!(r.force_balance_passes);
    assert!(!r.moment_balance_passes);
    let mut slip = standing();
    slip.contacts[0].force_world_n[0] = 6.;
    assert!(!contact::evaluate(slip).unwrap().contacts[0].passes);
    let mut inactive = standing();
    inactive.contacts[0].enabled = false;
    assert!(
        !contact::evaluate(inactive)
            .unwrap()
            .supplied_allocation_passes
    );
    let mut torque = standing();
    torque.motor_torques_nm[0] = 3.;
    assert!(
        !contact::evaluate(torque)
            .unwrap()
            .supplied_allocation_passes
    );
    let mut normal = standing();
    normal.contacts[0].normal_world = [0., 0., 2.];
    assert!(contact::evaluate(normal).is_err());
}
#[test]
fn centroidal_gravity_and_gyroscopic_moment_match_analytic_case() {
    let i = contact::CentroidalInput {
        mass_kg: 2.,
        inertia_world_kg_m2: [[1., 0., 0.], [0., 2., 0.], [0., 0., 3.]],
        acceleration_world_m_s2: [1., 0., 0.],
        gravity_world_m_s2: [0., 0., -10.],
        angular_velocity_world_rad_s: [1., 2., 3.],
        angular_acceleration_world_rad_s2: [0., 0., 0.],
        external_force_world_n: [0., 0., 0.],
        external_moment_about_com_world_nm: [0., 0., 0.],
    };
    assert_eq!(
        contact::required_wrench(i).unwrap(),
        [2., 0., 20., 6., -6., 2.]
    );
}
#[test]
fn motor_reduction_preserves_authored_mechanics_and_does_not_select_itself() {
    let p = std::collections::BTreeMap::from_iter(
        [
            ("resistance", 2.),
            ("inductance", 0.004),
            ("torque_constant", 0.1),
            ("rotor_inertia", 0.0001),
            ("gear_inertia", 0.001),
            ("ratio", 10.),
            ("backlash", 0.03),
            ("gear_friction", 0.02),
            ("no_load_current", 0.1),
            ("gear_stiffness", 100.),
            ("gear_damping", 0.1),
            ("efficiency", 0.8),
        ]
        .map(|(k, v)| (k.into(), v)),
    );
    let r = drive::prepare_reduction(drive::ReductionRequest {
        motor_parameters: p.clone(),
        dynamics: sim_domain_robot::motor::MotorDynamics::QuasistaticWinding,
        held_interval_s: 0.01,
    })
    .unwrap();
    for (k, v) in p {
        assert_eq!(r.motor_parameters[&k], v);
    }
    assert_eq!(r.motor_parameters["dynamics.quasistatic_winding"], 1.);
    assert_eq!(r.motor_parameters["dynamics.quasistatic_rotor"], 0.);
    assert_eq!(r.omitted_storage_terms.len(), 1);
}
