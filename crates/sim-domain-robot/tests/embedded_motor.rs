mod common;
use common::*;
use nalgebra::{DMatrix, DVector};
use sim_core::Behavior;
use sim_domain_robot::articulated::embedding::{
    CoupledForces, EmbeddedMotorBank, EmbeddedMotorConfig, MotorBoundary, RigidEmbedding,
};
use sim_domain_robot::{Articulated, Generalized, Options};
use std::sync::Arc;

struct FixedDrive;
impl sim_domain_robot::articulated::embedding::SampledMotorControl for FixedDrive {
    type State = f64;
    fn boundaries(
        &self,
        _: f64,
        _: &Generalized,
        _: &[f64],
        voltage: &f64,
    ) -> Result<Vec<MotorBoundary>, String> {
        Ok(vec![MotorBoundary {
            voltage_v: *voltage,
            winding_temperature_k: 293.15,
        }])
    }
    fn jump(&mut self, _: usize, _: f64, _: &Generalized, _: &mut f64) -> Result<(), String> {
        Err("no fixed-drive event".into())
    }
}

fn fixture(inductance: f64) -> (Articulated, Generalized, EmbeddedMotorConfig) {
    let mut model = empty_model();
    model.gravity = [0.0; 3];
    model
        .links
        .push(box_link("ground", [0.1; 3], 1.0, [0.0, 0.0, -1.0], true));
    model
        .links
        .push(box_link("rotor", [0.1; 3], 2.0, [0.0; 3], false));
    model.joints.push(joint(
        "shaft",
        "revolute",
        Some("ground"),
        "rotor",
        [0.0; 3],
        [0.0, 0.0, 1.0],
    ));
    let art = Articulated::new(
        Arc::new(model),
        &Options {
            flex: false,
            contact: false,
            ..Default::default()
        },
    )
    .unwrap();
    let g = art.generalized(
        art.states().iter().map(|s| s.initial).collect(),
        vec![0.0; art.state_count],
        &vec![0.0; art.port_names.len() + 1],
        vec![],
    );
    let config = EmbeddedMotorConfig {
        dof: "joint.shaft".into(),
        parameters: [
            ("resistance", 2.0),
            ("inductance", inductance),
            ("torque_constant", 0.08),
            ("back_emf_constant", 0.08),
            ("ratio", 5.0),
            ("efficiency", 1.0),
            ("rotor_inertia", 1e-4),
            ("gear_inertia", 0.0003),
            ("gear_stiffness", 20.0),
            ("gear_damping", 0.04),
            ("temp_coeff", 0.0),
            ("derating", 0.0),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v))
        .collect(),
        residual_scales: [1.0; 3],
    };
    (art, g, config)
}

#[test]
fn registered_motor_and_mechanics_match_independent_linear_circuit() {
    for inductance in [0.003, 0.0] {
        let (art, mut g, config) = fixture(inductance);
        let map = RigidEmbedding::new(&art, &["joint.shaft".into()], Default::default()).unwrap();
        let mut bank = EmbeddedMotorBank::new(&art, &[config]).unwrap();
        let mut workspace =
            sim_domain_robot::articulated::embedding::ImplicitSolverWorkspace::default();
        let mut state = bank.initial_states();
        let mut reference = DVector::zeros(5); // i, rotor speed, gear angle, shaft speed, shaft angle
        let h = 0.002;
        let (ratio, jout, jload, kt, ke, resistance, k, c) = (
            5.0,
            0.0028,
            2.0 * 0.1 * 0.1 / 6.0,
            0.08,
            0.08,
            2.0,
            20.0,
            0.04,
        );
        let voltage = 2.0;
        // Independent coupled linear BE equations, including gearbox compliance,
        // reflected rotor inertia, electrical back-EMF and mechanical loading.
        let matrix = DMatrix::from_row_slice(
            5,
            5,
            &[
                inductance / h + resistance,
                ke,
                0.0,
                0.0,
                0.0,
                -ratio * kt,
                jout / (ratio * h) + c / ratio,
                k,
                -c,
                -k,
                0.0,
                -1.0 / ratio,
                1.0 / h,
                0.0,
                0.0,
                0.0,
                -c / ratio,
                -k,
                jload / h + c,
                k,
                0.0,
                0.0,
                0.0,
                -1.0,
                1.0 / h,
            ],
        )
        .lu();
        let boundary = [MotorBoundary {
            voltage_v: voltage,
            winding_temperature_k: 293.15,
        }];
        for i in 0..30 {
            let before_g = g.clone();
            let before = state.clone();
            let rhs = DVector::from_column_slice(&[
                voltage + inductance / h * reference[0],
                jout / (ratio * h) * reference[1],
                reference[2] / h,
                jload / h * reference[3],
                reference[4] / h,
            ]);
            reference = matrix.solve(&rhs).unwrap();
            let step = map
                .step_implicit_coupled(
                    &g,
                    &state,
                    i as f64 * h,
                    h,
                    &Default::default(),
                    |t, h, g, x| bank.evaluate(t, h, g, &state, x, &boundary).map(|r| r.0),
                )
                .unwrap();
            let cached = map
                .step_implicit_coupled(
                    &g,
                    &state,
                    i as f64 * h,
                    h,
                    &sim_domain_robot::articulated::embedding::ImplicitStepConfig {
                        reuse_mechanical_endpoint: true,
                        ..Default::default()
                    },
                    |t, h, g, x| bank.evaluate(t, h, g, &state, x, &boundary).map(|r| r.0),
                )
                .unwrap();
            assert_eq!(cached.auxiliary, step.auxiliary);
            assert_eq!(
                cached.endpoint.generalized.states,
                step.endpoint.generalized.states
            );
            assert_eq!(
                cached.endpoint.generalized.rates,
                step.endpoint.generalized.rates
            );
            assert_eq!(
                cached.endpoint.full_accelerations,
                step.endpoint.full_accelerations
            );
            assert!(cached.diagnostics.mechanical_cache_hits > 0);
            let prepared = map
                .step_implicit_coupled(
                    &g,
                    &state,
                    i as f64 * h,
                    h,
                    &sim_domain_robot::articulated::embedding::ImplicitStepConfig {
                        reuse_mechanical_endpoint: true,
                        reuse_mechanical_dynamics: true,
                        ..Default::default()
                    },
                    |t, h, g, x| bank.evaluate(t, h, g, &state, x, &boundary).map(|r| r.0),
                )
                .unwrap();
            assert_eq!(prepared.auxiliary, step.auxiliary);
            assert_eq!(
                prepared.endpoint.generalized.states,
                step.endpoint.generalized.states
            );
            assert_eq!(
                prepared.endpoint.generalized.rates,
                step.endpoint.generalized.rates
            );
            assert_eq!(
                prepared.endpoint.full_accelerations,
                step.endpoint.full_accelerations
            );
            assert!(prepared.diagnostics.dynamics_cache_hits > 0);
            assert!(
                prepared.diagnostics.dynamics_preparations
                    < cached.diagnostics.dynamics_preparations
            );
            assert_eq!(
                prepared.diagnostics.endpoint_evaluations,
                prepared.diagnostics.dynamics_preparations
                    + prepared.diagnostics.dynamics_cache_hits
            );
            assert!(
                cached.diagnostics.mechanical_preparations
                    < step.diagnostics.mechanical_preparations
            );
            assert_eq!(
                cached.diagnostics.endpoint_evaluations,
                cached.diagnostics.mechanical_preparations
                    + cached.diagnostics.mechanical_cache_hits
            );
            let reused = bank
                .advance_with_control_cached(
                    &map,
                    &g,
                    &state,
                    &voltage,
                    i as f64 * h,
                    h,
                    &sim_domain_robot::articulated::embedding::ImplicitStepConfig {
                        reuse_step_jacobian: true,
                        auxiliary_rate_unknowns: true,
                        ..Default::default()
                    },
                    &Default::default(),
                    &mut FixedDrive,
                    &mut workspace,
                    |_, _| Ok(vec![0.0]),
                )
                .unwrap()
                .motor;
            let actual_reused = [
                reused.motor_states[0],
                reused.motor_states[1],
                reused.motor_states[2],
                reused.endpoint.generalized.qd[0],
                reused.endpoint.generalized.q[0],
            ];
            assert!(
                actual_reused
                    .iter()
                    .zip(reference.iter())
                    .all(|(a, b)| (a - b).abs() < 1e-7)
            );
            if i > 0 && i < 10 {
                assert_eq!(reused.solves.successful_trials_with_reused_jacobian, 1);
                assert!(
                    reused.solves.successful_trial_endpoint_evaluations
                        < step.diagnostics.endpoint_evaluations
                );
            }
            assert!(step.diagnostics.maximum_auxiliary_residual < 1e-8);
            assert!(step.diagnostics.maximum_scaled_velocity_residual < 1e-8);
            for (auxiliary_rate_unknowns, auxiliary_endpoint_correction_scale) in [(false,false),(true,false),(true,true)] {
                let condensed = map.step_implicit_coupled_with_rates(
                    &g, &state, i as f64 * h, h,
                    &sim_domain_robot::articulated::embedding::ImplicitStepConfig {
                        condense_auxiliary: true,
                        auxiliary_rate_unknowns,
                        auxiliary_endpoint_correction_scale,
                        ..Default::default()
                    },
                    |t, _, g, x, rates| bank.evaluate_with_rates(t, g, x, rates, &boundary).map(|r| r.0),
                ).unwrap();
                let cg = &condensed.endpoint.generalized;
                let cx = &condensed.auxiliary;
                let values = [cx[0], cx[1], cx[2], cg.qd[0], cg.q[0]];
                assert!(values.iter().zip(reference.iter()).all(|(a,b)| (a-b).abs()<1e-8), "{values:?} vs {reference:?}");
                assert!(condensed.diagnostics.auxiliary_solves > 0);
                assert!(condensed.diagnostics.maximum_auxiliary_residual <= 1e-10);
            }
            state = step.auxiliary;
            g = step.endpoint.generalized;
            let actual = [state[0], state[1], state[2], g.qd[0], g.q[0]];
            assert!(
                actual
                    .iter()
                    .zip(reference.iter())
                    .all(|(a, b)| (a - b).abs() < 1e-7),
                "{actual:?} vs {reference:?}"
            );
            let (_, readings) = bank
                .evaluate((i + 1) as f64 * h, h, &g, &before, &state, &boundary)
                .unwrap();
            assert!(
                (readings[0].shaft_torque_nm - jload * (g.qd[0] - before_g.qd[0]) / h).abs() < 1e-8
            );
            assert!((readings[0].heating_w - resistance * state[0].powi(2)).abs() < 1e-10);
            assert!((readings[0].gear_speed_rad_s - state[1] / ratio).abs() < 1e-12);
        }
    }
}

#[test]
fn motor_bindings_boundaries_and_combined_failure_fail_closed() {
    let (art, g, config) = fixture(0.003);
    let map = RigidEmbedding::new(&art, &["joint.shaft".into()], Default::default()).unwrap();
    assert!(EmbeddedMotorBank::new(&art, &[config.clone(), config.clone()]).is_err());
    let mut invalid = config.clone();
    invalid.dof = "missing".into();
    assert!(EmbeddedMotorBank::new(&art, &[invalid]).is_err());
    let mut invalid = config.clone();
    invalid.parameters.insert("backlash.events".into(), 1.0);
    assert!(EmbeddedMotorBank::new(&art, &[invalid]).is_err());
    let mut invalid = config.clone();
    invalid.residual_scales[0] = 0.0;
    assert!(EmbeddedMotorBank::new(&art, &[invalid]).is_err());
    let bank = EmbeddedMotorBank::new(&art, &[config]).unwrap();
    let old = bank.initial_states();
    let before = g.clone();
    let old_before = old.clone();
    assert!(
        map.step_implicit_coupled(&g, &old, 0.0, 0.01, &Default::default(), |_, _, _, _| Ok(
            CoupledForces {
                generalized_loads: vec![0.0],
                auxiliary_residuals: vec![],
            }
        ))
        .is_err()
    );
    assert!(
        map.step_implicit_coupled(&g, &old, 0.0, 0.01, &Default::default(), |t, h, g, x| {
            bank.evaluate(
                t,
                h,
                g,
                &old,
                x,
                &[MotorBoundary {
                    voltage_v: 2.0,
                    winding_temperature_k: 0.0,
                }],
            )
            .map(|r| r.0)
        })
        .is_err()
    );
    assert_eq!(old, old_before);
    assert_eq!(g.states, before.states);
    assert_eq!(g.q, before.q);
    assert_eq!(g.qd, before.qd);
}

#[test]
fn eventful_motor_locates_engagement_and_matches_piecewise_linear_motion() {
    for (direction, auxiliary_rate_unknowns, condense_auxiliary) in [-1.0, 1.0]
        .into_iter()
        .flat_map(|d| [false, true].map(|rates| (d, rates)))
        .flat_map(|(d,rates)| [false,true].map(|condense| (d,rates,condense)))
    {
        let (art, g, mut config) = fixture(0.003);
        config.parameters.insert("backlash.events".into(), 1.0);
        config.parameters.insert("backlash".into(), 0.02);
        config.parameters.insert("gear_damping".into(), 0.0);
        let map = RigidEmbedding::new(&art, &["joint.shaft".into()], Default::default()).unwrap();
        let mut bank = EmbeddedMotorBank::new_with_events(&art, &[config]).unwrap();
        assert_eq!(bank.state_layout(), vec![(0, 4, 0)]);
        let mut initial = bank.initial_states();
        initial[1] = 5.0 * direction;
        let before = initial.clone();
        let boundary = [MotorBoundary {
            voltage_v: 0.4 * direction,
            winding_temperature_k: 293.15,
        }];
        let end = bank
            .advance_with_events(
                &map,
                &g,
                &initial,
                0.0,
                0.02,
                &sim_domain_robot::articulated::embedding::ImplicitStepConfig {
                    auxiliary_rate_unknowns,
                    condense_auxiliary,
                    ..Default::default()
                },
                &sim_dynamics::hybrid::HybridConfig {
                    maximum_halvings: 0,
                    ..Default::default()
                },
                &boundary,
                |_, _| Ok(vec![0.0]),
            )
            .unwrap();
        assert_eq!(
            end.hybrid
                .events
                .iter()
                .map(|e| e.guard)
                .collect::<Vec<_>>(),
            vec![2, if direction > 0.0 { 0 } else { 1 }]
        );
        assert_eq!(end.hybrid.events[0].time, 0.0);
        let event_time = end.hybrid.events[1].time;
        assert!((event_time - 0.01).abs() < 3e-9, "{event_time}");
        assert_eq!(end.motor_states[3], direction);
        assert_eq!(initial, before);
        // No load or current before engagement: gearbox rotates at exactly 1 rad/s.
        // After engagement the constant backlash offset enters the linear equations.
        let h = 0.02 - event_time;
        let (ratio, jout, jload, k) = (5.0, 0.0028, 2.0 * 0.1 * 0.1 / 6.0, 20.0);
        let matrix = DMatrix::from_row_slice(
            5,
            5,
            &[
                0.003 / h + 2.0,
                0.08,
                0.0,
                0.0,
                0.0,
                -ratio * 0.08,
                jout / (ratio * h),
                k,
                0.0,
                -k,
                0.0,
                -1.0 / ratio,
                1.0 / h,
                0.0,
                0.0,
                0.0,
                0.0,
                -k,
                jload / h,
                k,
                0.0,
                0.0,
                0.0,
                -1.0,
                1.0 / h,
            ],
        );
        let rhs = DVector::from_column_slice(&[
            0.4,
            jout / (ratio * h) * 5.0 + k * 0.01,
            event_time / h,
            -k * 0.01,
            0.0,
        ]);
        let expected = matrix.lu().solve(&(rhs * direction)).unwrap();
        let actual = [
            end.motor_states[0],
            end.motor_states[1],
            end.motor_states[2],
            end.endpoint.generalized.qd[0],
            end.endpoint.generalized.q[0],
        ];
        assert!(
            actual
                .iter()
                .zip(expected.iter())
                .all(|(a, b)| (a - b).abs() < 1e-7),
            "{actual:?} vs {expected:?}"
        );
        assert_eq!(end.hybrid.accepted_segments, 2);
        assert!(end.solves.maximum_auxiliary_residual < 1e-8);
        assert!(end.contact_steps.is_none());
        bank.set_contact_step_audit(true);
        // Even after startup/event processing, a later failed interval must not
        // poison the input or the bank's next attempt from that same input.
        assert!(
            bank.advance_with_events(
                &map,
                &g,
                &initial,
                0.0,
                0.02,
                &sim_domain_robot::articulated::embedding::ImplicitStepConfig {
                    auxiliary_rate_unknowns,
                    condense_auxiliary,
                    ..Default::default()
                },
                &sim_dynamics::hybrid::HybridConfig {
                    maximum_halvings: 2,
                    ..Default::default()
                },
                &boundary,
                |t, _| if t > 0.015 {
                    Err("fixture failure".into())
                } else {
                    Ok(vec![0.0])
                }
            )
            .is_err()
        );
        assert_eq!(initial, before);
        let repeated = bank
            .advance_with_events(
                &map,
                &g,
                &initial,
                0.0,
                0.02,
                &sim_domain_robot::articulated::embedding::ImplicitStepConfig {
                    reuse_mechanical_endpoint: true,
                    reuse_mechanical_dynamics: true,
                    reuse_step_jacobian: true,
                    auxiliary_rate_unknowns,
                    condense_auxiliary,
                    ..Default::default()
                },
                &sim_dynamics::hybrid::HybridConfig {
                    maximum_halvings: 0,
                    ..Default::default()
                },
                &boundary,
                |_, _| Ok(vec![0.0]),
            )
            .unwrap();
        let stages = repeated.contact_steps.as_ref().unwrap();
        assert_eq!(stages.len(), 2);
        assert_eq!(stages.len(), repeated.hybrid.accepted_segments);
        assert!(repeated.hybrid.continuous_attempts > stages.len());
        assert_eq!(stages[0].start_time_s, 0.0);
        assert_eq!(stages[0].step_s, event_time);
        assert_eq!(stages[1].start_time_s, event_time);
        assert_eq!(stages[1].start_time_s + stages[1].step_s, 0.02);
        assert!(stages.iter().all(|s| s.contacts.is_empty()));
        assert_eq!(repeated.motor_states, end.motor_states);
        assert!(repeated.solves.successful_trial_mechanical_cache_hits > 0);
        assert!(repeated.solves.successful_trial_dynamics_cache_hits > 0);
        assert_eq!(
            serde_json::to_value(&repeated.hybrid).unwrap(),
            serde_json::to_value(&end.hybrid).unwrap()
        );
        assert_eq!(
            repeated.endpoint.generalized.rates,
            end.endpoint.generalized.rates
        );
        assert_eq!(
            repeated.endpoint.full_accelerations,
            end.endpoint.full_accelerations
        );

        assert_eq!(repeated.endpoint.generalized.q, end.endpoint.generalized.q);
    }
}

#[test]
fn registered_event_modes_initialize_and_release_without_velocity_impulses() {
    let (art, g, mut config) = fixture(0.003);
    config.parameters.insert("backlash.events".into(), 1.0);
    config.parameters.insert("backlash".into(), 0.02);
    let mut bank = EmbeddedMotorBank::new_with_events(&art, &[config]).unwrap();
    let mut state = bank.initial_states();
    state[2] = 0.02;
    let boundaries = [MotorBoundary {
        voltage_v: 0.0,
        winding_temperature_k: 293.15,
    }];
    assert_eq!(
        bank.event_data(0.0, &g, &state, &boundaries).unwrap().1,
        vec![(2, 0.0)]
    );
    for (theta, guard, mode) in [
        (0.02, 2, 1.0),
        (0.009, 0, 0.0),
        (-0.011, 1, -1.0),
        (-0.009, 1, 0.0),
    ] {
        state[2] = theta;
        let continuous = state[..3].to_vec();
        bank.jump(guard, 0.0, &g, &mut state, &boundaries).unwrap();
        assert_eq!(state[3], mode);
        assert_eq!(state[..3], continuous);
        let reading = bank
            .evaluate(0.0, 0.01, &g, &state, &state, &boundaries)
            .unwrap()
            .1;
        let expected = if mode == 0.0 {
            0.0
        } else {
            20.0 * (theta - mode * 0.01)
        };
        assert!((reading[0].shaft_torque_nm - expected).abs() < 1e-12);
        assert!(
            bank.event_data(0.0, &g, &state, &boundaries)
                .unwrap()
                .1
                .is_empty()
        );
    }
}

#[test]
fn accepted_contact_trace_uses_original_endpoint_forces_without_changing_motion() {
    let (base, _, config) = fixture(0.003);
    let mut model = (*base.model).clone();
    model.world.floor_z = -0.049;
    let art = Articulated::new(
        Arc::new(model),
        &Options {
            flex: false,
            contact: true,
            ..Default::default()
        },
    )
    .unwrap();
    let g = art.generalized(
        art.states().iter().map(|s| s.initial).collect(),
        vec![0.0; art.state_count],
        &vec![0.0; art.port_names.len() + 1],
        vec![],
    );
    let map = RigidEmbedding::new(&art, &["joint.shaft".into()], Default::default()).unwrap();
    let mut bank = EmbeddedMotorBank::new_with_events(&art, &[config]).unwrap();
    let state = bank.initial_states();
    let boundary = [MotorBoundary {
        voltage_v: 0.0,
        winding_temperature_k: 293.15,
    }];
    let reference = bank
        .advance_with_events(
            &map,
            &g,
            &state,
            0.0,
            0.0001,
            &Default::default(),
            &Default::default(),
            &boundary,
            |_, _| Ok(vec![0.0]),
        )
        .unwrap();
    bank.set_contact_step_audit(true);
    let audited = bank
        .advance_with_events(
            &map,
            &g,
            &state,
            0.0,
            0.0001,
            &Default::default(),
            &Default::default(),
            &boundary,
            |_, _| Ok(vec![0.0]),
        )
        .unwrap();
    assert_eq!(audited.motor_states, reference.motor_states);
    assert_eq!(
        audited.endpoint.generalized.states,
        reference.endpoint.generalized.states
    );
    assert_eq!(
        audited.endpoint.generalized.rates,
        reference.endpoint.generalized.rates
    );
    let steps = audited.contact_steps.unwrap();
    assert_eq!(steps.len(), 1);
    let original = art.evaluate(&audited.endpoint.generalized);
    assert!(!original.contacts.is_empty());
    assert_eq!(steps[0].contacts.len(), original.contacts.len());
    for (sample, contact) in steps[0].contacts.iter().zip(original.contacts) {
        assert_eq!(sample.link, contact.link);
        assert_eq!(sample.other, contact.other);
        assert_eq!(sample.force_n, std::array::from_fn(|i| contact.force[i]));
        assert_eq!(sample.point_m, std::array::from_fn(|i| contact.point[i]));
        assert_eq!(sample.penetration_m, contact.penetration);
    }
}

#[test]
fn driver_feedback_matches_independent_loaded_motor_with_series_drop_and_foldback() {
    use sim_domain_robot::articulated::embedding::{
        DriverBoundary, EmbeddedDriverBank, EmbeddedDriverConfig, ImplicitStepConfig,
    };
    for (limit, voltage, foldback, condense_auxiliary) in [(100.0, 2.0, 0.0), (0.1, 8.0, 20.0)]
        .into_iter().flat_map(|(l,v,f)| [false,true].map(|c| (l,v,f,c))) {
        for direction in [-1.0, 1.0] {
            let (art, mut g, config) = fixture(0.003);
            let map =
                RigidEmbedding::new(&art, &["joint.shaft".into()], Default::default()).unwrap();
            let mut bank = EmbeddedMotorBank::new_with_events(&art, &[config]).unwrap();
            let driver = EmbeddedDriverBank::new(
                &bank,
                &[EmbeddedDriverConfig {
                    dof: "joint.shaft".into(),
                    parameters: [
                        ("on_resistance".into(), 0.5),
                        ("current_limit".into(), limit),
                    ]
                    .into(),
                }],
            )
            .unwrap();
            driver.validate_binding(&bank).unwrap();
            let boundary = [DriverBoundary {
                supply_voltage_v: 10.0,
                duty: direction * voltage / 10.0,
                winding_temperature_k: 293.15,
            }];
            let mut state = bank.initial_states();
            let mut reference = DVector::zeros(5);
            let h = 0.002;
            let (ratio, jout, jload, k, c) = (5.0, 0.0028, 2.0 * 0.1 * 0.1 / 6.0, 20.0, 0.04);
            // Independent discrete equations. In the driven sign branch,
            // bridge drop adds R_on and active foldback adds 20 ohm and
            // 20*I_limit volts. No old-current voltage approximation is used.
            let matrix = DMatrix::from_row_slice(
                5,
                5,
                &[
                    0.003 / h + 2.0 + 0.5 + foldback,
                    0.08,
                    0.0,
                    0.0,
                    0.0,
                    -ratio * 0.08,
                    jout / (ratio * h) + c / ratio,
                    k,
                    -c,
                    -k,
                    0.0,
                    -1.0 / ratio,
                    1.0 / h,
                    0.0,
                    0.0,
                    0.0,
                    -c / ratio,
                    -k,
                    jload / h + c,
                    k,
                    0.0,
                    0.0,
                    0.0,
                    -1.0,
                    1.0 / h,
                ],
            )
            .lu();
            for i in 0..20 {
                let rhs = DVector::from_column_slice(&[
                    direction * (voltage + foldback * limit) + 0.003 / h * reference[0],
                    jout / (ratio * h) * reference[1],
                    reference[2] / h,
                    jload / h * reference[3],
                    reference[4] / h,
                ]);
                reference = matrix.solve(&rhs).unwrap();
                let step = bank
                    .advance_with_boundary_law(
                        &map,
                        &g,
                        &state,
                        i as f64 * h,
                        h,
                        &ImplicitStepConfig {
                            reuse_mechanical_endpoint: true,
                            condense_auxiliary,
                            ..Default::default()
                        },
                        &Default::default(),
                        |t, _, x| driver.evaluate(t, x, &boundary).map(|r| r.0),
                        |_, _| Ok(vec![0.0]),
                    )
                    .unwrap();
                state = step.motor_states;
                g = step.endpoint.generalized;
                let actual = [state[0], state[1], state[2], g.qd[0], g.q[0]];
                assert!(
                    actual
                        .iter()
                        .zip(reference.iter())
                        .all(|(a, b)| (a - b).abs() < 1e-7),
                    "{actual:?} vs {reference:?}"
                );
                let (b, r) = driver
                    .evaluate((i + 1) as f64 * h, &state, &boundary)
                    .unwrap();
                assert!((r[0].supply_current_a - boundary[0].duty * state[0]).abs() < 1e-14);
                let expected_drop = 0.5 * state[0] + foldback * (state[0] - direction * limit);
                assert!((b[0].voltage_v - (direction * voltage - expected_drop)).abs() < 1e-10);
                assert!(r[0].power_difference_w >= 0.0);
                if foldback > 0.0 {
                    assert!(state[0].abs() > limit, "foldback is not a hard current cap");
                }
            }
        }
    }
}

#[test]
fn driver_binding_and_boundary_validation_are_explicit() {
    use sim_domain_robot::articulated::embedding::{
        DriverBoundary, EmbeddedDriverBank, EmbeddedDriverConfig,
    };
    let (art, g, config) = fixture(0.003);
    let mut bank = EmbeddedMotorBank::new_with_events(&art, &[config]).unwrap();
    let cfg = EmbeddedDriverConfig {
        dof: "joint.shaft".into(),
        parameters: [("current_limit".into(), 1.0)].into(),
    };
    assert!(EmbeddedDriverBank::new(&bank, &[]).is_err());
    let mut bad = cfg.clone();
    bad.dof = "missing".into();
    assert!(EmbeddedDriverBank::new(&bank, &[bad]).is_err());
    let mut bad = cfg.clone();
    bad.parameters.insert("on_resistance".into(), -1.0);
    assert!(EmbeddedDriverBank::new(&bank, &[bad]).is_err());
    let driver = EmbeddedDriverBank::new(&bank, &[cfg]).unwrap();
    let initial = bank.initial_states();
    let good = DriverBoundary {
        supply_voltage_v: 10.0,
        duty: 0.5,
        winding_temperature_k: 293.15,
    };
    for bad in [
        DriverBoundary { duty: 1.01, ..good },
        DriverBoundary {
            supply_voltage_v: -1.0,
            ..good
        },
        DriverBoundary {
            winding_temperature_k: 0.0,
            ..good
        },
    ] {
        assert!(driver.evaluate(0.0, &initial, &[bad]).is_err());
    }
    assert!(driver.evaluate(0.0, &[], &[good]).is_err());
    let map = RigidEmbedding::new(&art, &["joint.shaft".into()], Default::default()).unwrap();
    assert!(
        bank.advance_with_boundary_law(
            &map,
            &g,
            &initial,
            0.0,
            0.001,
            &Default::default(),
            &Default::default(),
            |_, _, _| Err("boundary failure".into()),
            |_, _| Ok(vec![0.0])
        )
        .is_err()
    );
    assert_eq!(initial, bank.initial_states());
    // A subsequent valid interval is independent of the failed boundary call.
    assert!(
        bank.advance_with_boundary_law(
            &map,
            &g,
            &initial,
            0.0,
            0.001,
            &Default::default(),
            &Default::default(),
            |t, _, x| driver.evaluate(t, x, &[good]).map(|r| r.0),
            |_, _| Ok(vec![0.0])
        )
        .is_ok()
    );
}

#[test]
fn registered_servo_preserves_quantization_deadband_delay_and_saturation() {
    use sim_domain_robot::articulated::embedding::{
        EmbeddedServoBank, EmbeddedServoConfig, ServoBoundary,
    };
    let (art, mut g, cfg) = fixture(0.003);
    let motors = EmbeddedMotorBank::new_with_events(&art, &[cfg]).unwrap();
    let config = EmbeddedServoConfig {
        dof: "joint.shaft".into(),
        parameters: [
            ("rate".into(), 50.0),
            ("latency".into(), 0.02),
            ("resolution".into(), 0.1),
            ("deadband".into(), 0.15),
            ("kp".into(), 2.0),
            ("ki".into(), 3.0),
            ("kd".into(), 0.1),
            ("limit".into(), 0.5),
        ]
        .into(),
    };
    let mut bank = EmbeddedServoBank::new(&motors, &[config.clone()]).unwrap();
    let mut state = bank.initial_states();
    assert_eq!(bank.state_layout(), vec![(0, 6, 0)]);
    let mut input = [ServoBoundary {
        target_rad: 0.5,
        supply_voltage_v: 10.0,
        winding_temperature_k: 293.15,
    }];
    g.q[0] = 0.14;
    g.qd[0] = 2.0;
    assert_eq!(bank.commands(0.0, &g, &state, &input).unwrap(), vec![0.0]);
    assert_eq!(
        bank.event_data(0.0, &g, &state, &input).unwrap().1,
        vec![(0, 0.02)]
    );
    assert!(bank.jump(0, 0.01, &g, &mut state, &input).is_err());
    bank.jump(0, 0.02, &g, &mut state, &input).unwrap();
    assert_eq!(state[0], 0.0);
    assert_eq!(state[1], 0.0); // delayed, anti-windup
    assert_eq!(state[3], -1.0);
    assert_eq!(state[4], 0.5);
    input[0].target_rad = 0.0;
    g.qd[0] = 0.0;
    bank.jump(0, 0.04, &g, &mut state, &input).unwrap();
    assert_eq!(state[0], 0.5);
    assert_eq!(state[2], 0.0); // quantized error enters deadband
    assert_eq!(state[4], -0.05);
    input[0].target_rad = 0.3;
    bank.jump(0, 0.06, &g, &mut state, &input).unwrap();
    assert_eq!(state[0], -0.05);
    assert!((state[1] - 0.004).abs() < 1e-15);
    assert!((state[4] - 0.387).abs() < 1e-15);
    assert_eq!(state[5], 0.08);
    let mut bad = config;
    bad.parameters.insert("latency".into(), 2000.0);
    assert!(EmbeddedServoBank::new(&motors, &[bad]).is_err());
}

#[test]
fn scheduled_servo_drives_motor_and_failed_interval_does_not_advance_controller() {
    use sim_domain_robot::articulated::embedding::{
        EmbeddedDriverBank, EmbeddedDriverConfig, EmbeddedServoBank, EmbeddedServoConfig,
        ImplicitStepConfig, ServoBoundary,
    };
    assert!(!sim_domain_robot::articulated::embedding::SampledMotorControl::permits_jacobian_reuse_after_sample(&FixedDrive, 0));
    let (art, g, cfg) = fixture(0.003);
    let map = RigidEmbedding::new(&art, &["joint.shaft".into()], Default::default()).unwrap();
    let mut motors = EmbeddedMotorBank::new_with_events(&art, &[cfg]).unwrap();
    let drivers = EmbeddedDriverBank::new(
        &motors,
        &[EmbeddedDriverConfig {
            dof: "joint.shaft".into(),
            parameters: [
                ("on_resistance".into(), 0.5),
                ("current_limit".into(), 100.0),
            ]
            .into(),
        }],
    )
    .unwrap();
    let cfg = EmbeddedServoConfig {
        dof: "joint.shaft".into(),
        parameters: [
            ("rate".into(), 50.0),
            ("kp".into(), 2.0),
            ("limit".into(), 1.0),
        ]
        .into(),
    };
    let mut servos = EmbeddedServoBank::new(&motors, &[cfg]).unwrap();
    let input = [ServoBoundary {
        target_rad: 0.1,
        supply_voltage_v: 10.0,
        winding_temperature_k: 293.15,
    }];
    let initial = servos.initial_states();
    let initial_motors = motors.initial_states();
    let implicit = ImplicitStepConfig {
        reuse_mechanical_endpoint: true,
        reuse_step_jacobian: true,
        ..Default::default()
    };
    let success = {
        let mut control = servos.connect(&motors, &drivers, &input).unwrap();
        motors
            .advance_with_control(
                &map,
                &g,
                &initial_motors,
                &initial,
                0.0,
                0.06,
                &implicit,
                &Default::default(),
                &mut control,
                |_, _| Ok(vec![0.0]),
            )
            .unwrap()
    };
    let reused = {
        let mut control = servos.connect(&motors, &drivers, &input).unwrap();
        motors.advance_with_control(
            &map, &g, &initial_motors, &initial, 0.0, 0.06,
            &ImplicitStepConfig { reuse_controller_sample_jacobian:true, ..implicit.clone() },
            &Default::default(), &mut control, |_,_| Ok(vec![0.0]),
        ).unwrap()
    };
    assert!(reused.motor.solves.successful_trials_with_reused_jacobian >= 2);
    println!("sample-reuse evaluations: {} vs {} original", reused.motor.solves.successful_trial_endpoint_evaluations, success.motor.solves.successful_trial_endpoint_evaluations);
    assert!((reused.motor.endpoint.generalized.q[0]-success.motor.endpoint.generalized.q[0]).abs()<1e-10);
    for (a,b) in reused.motor.motor_states.iter().zip(&success.motor.motor_states) {assert!((a-b).abs()<1e-9);}
    assert_eq!(reused.motor.hybrid.events.len(),success.motor.hybrid.events.len());
    let condensed = {
        let mut control = servos.connect(&motors, &drivers, &input).unwrap();
        motors.advance_with_control(
            &map, &g, &initial_motors, &initial, 0.0, 0.06,
            &ImplicitStepConfig { condense_auxiliary:true, auxiliary_rate_unknowns:true, ..implicit.clone() },
            &Default::default(), &mut control, |_,_| Ok(vec![0.0]),
        ).unwrap()
    };
    assert!((condensed.motor.endpoint.generalized.q[0]-success.motor.endpoint.generalized.q[0]).abs()<1e-10);
    for (a,b) in condensed.motor.motor_states.iter().zip(&success.motor.motor_states) { assert!((a-b).abs()<1e-9); }
    for (a,b) in condensed.control_state.iter().zip(&success.control_state) { assert!((a-b).abs()<1e-12); }
    assert_eq!(condensed.control_state.last(), success.control_state.last());
    assert_eq!(condensed.motor.hybrid.events.iter().map(|e|e.time).collect::<Vec<_>>(), success.motor.hybrid.events.iter().map(|e|e.time).collect::<Vec<_>>());
    assert_eq!(success.control_guard_offset, 0);
    // Every segment ends with a sampled controller jump, so none may reuse.
    assert_eq!(
        success.motor.solves.successful_trials_with_reused_jacobian,
        0
    );
    assert_eq!(
        success
            .motor
            .hybrid
            .events
            .iter()
            .map(|e| e.time)
            .collect::<Vec<_>>(),
        vec![0.02, 0.04, 0.06]
    );
    assert_eq!(success.motor.hybrid.accepted_segments, 3);
    assert!(success.motor.endpoint.generalized.q[0] > 0.0);
    assert!(success.motor.motor_states[0] > 0.0);
    assert_eq!(*success.control_state.last().unwrap(), 0.08);
    // Independent clocked proportional controller and linear circuit/load BE
    // reference. The new command affects the NEXT interval, not the one ending
    // at its sample time. This catches accidentally continuous feedback and
    // endpoint sampling before the mechanical state has been accepted.
    let h = 0.02;
    let (ratio, jout, jload, k, c) = (5.0, 0.0028, 2.0 * 0.1 * 0.1 / 6.0, 20.0, 0.04);
    let matrix = DMatrix::from_row_slice(
        5,
        5,
        &[
            0.003 / h + 2.5,
            0.08,
            0.0,
            0.0,
            0.0,
            -ratio * 0.08,
            jout / (ratio * h) + c / ratio,
            k,
            -c,
            -k,
            0.0,
            -1.0 / ratio,
            1.0 / h,
            0.0,
            0.0,
            0.0,
            -c / ratio,
            -k,
            jload / h + c,
            k,
            0.0,
            0.0,
            0.0,
            -1.0,
            1.0 / h,
        ],
    )
    .lu();
    let mut reference = DVector::<f64>::zeros(5);
    let mut duty = 0.0_f64;
    for _ in 0..3 {
        reference = matrix
            .solve(&DVector::from_column_slice(&[
                10.0 * duty + 0.003 / h * reference[0],
                jout / (ratio * h) * reference[1],
                reference[2] / h,
                jload / h * reference[3],
                reference[4] / h,
            ]))
            .unwrap();
        duty = (2.0 * (0.1 - reference[4])).clamp(-1.0, 1.0);
    }
    let actual = [
        success.motor.motor_states[0],
        success.motor.motor_states[1],
        success.motor.motor_states[2],
        success.motor.endpoint.generalized.qd[0],
        success.motor.endpoint.generalized.q[0],
    ];
    assert!(
        actual
            .iter()
            .zip(reference.iter())
            .all(|(a, b)| (a - b).abs() < 1e-7),
        "{actual:?} vs {reference:?}"
    );
    assert!((success.control_state[0] - duty).abs() < 1e-8);
    {
        let mut control = servos.connect(&motors, &drivers, &input).unwrap();
        assert!(
            motors
                .advance_with_control(
                    &map,
                    &g,
                    &initial_motors,
                    &initial,
                    0.0,
                    0.06,
                    &ImplicitStepConfig {reuse_controller_sample_jacobian:true,..implicit.clone()},
                    &Default::default(),
                    &mut control,
                    |t, _| if t > 0.045 {
                        Err("injected failure".into())
                    } else {
                        Ok(vec![0.0])
                    }
                )
                .is_err()
        );
    }
    assert_eq!(initial, servos.initial_states());
    assert_eq!(initial_motors, motors.initial_states());
    let repeat = {
        let mut control = servos.connect(&motors, &drivers, &input).unwrap();
        motors
            .advance_with_control(
                &map,
                &g,
                &initial_motors,
                &initial,
                0.0,
                0.06,
                &implicit,
                &Default::default(),
                &mut control,
                |_, _| Ok(vec![0.0]),
            )
            .unwrap()
    };
    assert_eq!(repeat.control_state, success.control_state);
    assert_eq!(repeat.motor.motor_states, success.motor.motor_states);
    assert_eq!(
        repeat.motor.endpoint.generalized.states,
        success.motor.endpoint.generalized.states
    );
    // A continuously changing reference is consumed at firmware ticks only.
    let law = |t: f64| Ok(vec![0.1 + 0.2 * t]);
    let moving = {
        let mut control = servos
            .connect_target_law(&motors, &drivers, &input, &law)
            .unwrap();
        motors
            .advance_with_control(
                &map,
                &g,
                &initial_motors,
                &initial,
                0.0,
                0.06,
                &implicit,
                &Default::default(),
                &mut control,
                |_, _| Ok(vec![0.0]),
            )
            .unwrap()
    };
    let mut reference = DVector::<f64>::zeros(5);
    let mut duty = 0.0;
    for tick in 1..=3 {
        reference = matrix
            .solve(&DVector::from_column_slice(&[
                10.0 * duty + 0.003 / h * reference[0],
                jout / (ratio * h) * reference[1],
                reference[2] / h,
                jload / h * reference[3],
                reference[4] / h,
            ]))
            .unwrap();
        duty = (2.0 * (0.1 + 0.2 * (tick as f64 * h) - reference[4])).clamp(-1.0, 1.0);
    }
    let actual = [
        moving.motor.motor_states[0],
        moving.motor.motor_states[1],
        moving.motor.motor_states[2],
        moving.motor.endpoint.generalized.qd[0],
        moving.motor.endpoint.generalized.q[0],
    ];
    assert!(
        actual
            .iter()
            .zip(reference.iter())
            .all(|(a, b)| (a - b).abs() < 1e-7)
    );
    assert!((moving.control_state[0] - duty).abs() < 1e-8);
    assert_eq!(
        moving
            .motor
            .hybrid
            .events
            .iter()
            .map(|e| e.time)
            .collect::<Vec<_>>(),
        vec![0.02, 0.04, 0.06]
    );
    let failed = |t: f64| {
        if t > 0.045 {
            Err("injected target-law failure".into())
        } else {
            law(t)
        }
    };
    let mut control = servos
        .connect_target_law(&motors, &drivers, &input, &failed)
        .unwrap();
    assert!(
        motors
            .advance_with_control(
                &map,
                &g,
                &initial_motors,
                &initial,
                0.0,
                0.06,
                &implicit,
                &Default::default(),
                &mut control,
                |_, _| Ok(vec![0.0])
            )
            .is_err()
    );
    assert_eq!(servos.initial_states(), initial);
}

#[test]
fn cross_step_workspace_rolls_back_and_refreshes_for_timestep_changes() {
    use sim_domain_robot::articulated::embedding::{ImplicitSolverWorkspace, ImplicitStepConfig};
    let (art, g, cfg) = fixture(0.003);
    let map = RigidEmbedding::new(&art, &["joint.shaft".into()], Default::default()).unwrap();
    let mut bank = EmbeddedMotorBank::new(&art, &[cfg]).unwrap();
    let mut workspace = ImplicitSolverWorkspace::default();
    let config = ImplicitStepConfig {
        reuse_step_jacobian: true,
        auxiliary_rate_unknowns: true,
        ..Default::default()
    };
    let initial = bank.initial_states();
    let first = bank
        .advance_with_control_cached(
            &map,
            &g,
            &initial,
            &2.0,
            0.0,
            0.002,
            &config,
            &Default::default(),
            &mut FixedDrive,
            &mut workspace,
            |_, _| Ok(vec![0.0]),
        )
        .unwrap()
        .motor;
    let mut snapshot = workspace.clone();
    let g = &first.endpoint.generalized;
    let x = &first.motor_states;
    assert!(
        bank.advance_with_control_cached(
            &map,
            g,
            x,
            &2.0,
            0.002,
            0.002,
            &config,
            &Default::default(),
            &mut FixedDrive,
            &mut workspace,
            |_, _| Err("injected external failure".into())
        )
        .is_err()
    );
    let repeat = bank
        .advance_with_control_cached(
            &map,
            g,
            x,
            &2.0,
            0.002,
            0.002,
            &config,
            &Default::default(),
            &mut FixedDrive,
            &mut workspace,
            |_, _| Ok(vec![0.0]),
        )
        .unwrap()
        .motor;
    let reference = bank
        .advance_with_control_cached(
            &map,
            g,
            x,
            &2.0,
            0.002,
            0.002,
            &config,
            &Default::default(),
            &mut FixedDrive,
            &mut snapshot,
            |_, _| Ok(vec![0.0]),
        )
        .unwrap()
        .motor;
    assert_eq!(repeat.motor_states, reference.motor_states);
    assert_eq!(
        repeat.endpoint.generalized.states,
        reference.endpoint.generalized.states
    );
    assert_eq!(
        repeat.solves.successful_trial_endpoint_evaluations,
        reference.solves.successful_trial_endpoint_evaluations
    );
    assert_eq!(repeat.solves.successful_trials_with_reused_jacobian, 1);
    // A rate-coordinate factorization cannot be reused for state coordinates,
    // even when model, dimension, timestep and physical state are identical.
    let mut changed_coordinates = workspace.clone();
    let switched = bank
        .advance_with_control_cached(
            &map,
            &repeat.endpoint.generalized,
            &repeat.motor_states,
            &2.0,
            0.004,
            0.002,
            &ImplicitStepConfig {
                auxiliary_rate_unknowns: false,
                ..config.clone()
            },
            &Default::default(),
            &mut FixedDrive,
            &mut changed_coordinates,
            |_, _| Ok(vec![0.0]),
        )
        .unwrap()
        .motor;
    assert_eq!(switched.solves.successful_trials_with_reused_jacobian, 0);
    let fresh = bank
        .advance_with_control_cached(
            &map,
            &repeat.endpoint.generalized,
            &repeat.motor_states,
            &2.0,
            0.004,
            0.002,
            &ImplicitStepConfig {
                auxiliary_rate_unknowns: false,
                ..config.clone()
            },
            &Default::default(),
            &mut FixedDrive,
            &mut ImplicitSolverWorkspace::default(),
            |_, _| Ok(vec![0.0]),
        )
        .unwrap()
        .motor;
    assert_eq!(switched.motor_states, fresh.motor_states);
    assert_eq!(
        switched.endpoint.generalized.states,
        fresh.endpoint.generalized.states
    );
    let smaller = bank
        .advance_with_control_cached(
            &map,
            &repeat.endpoint.generalized,
            &repeat.motor_states,
            &2.0,
            0.004,
            0.001,
            &config,
            &Default::default(),
            &mut FixedDrive,
            &mut workspace,
            |_, _| Ok(vec![0.0]),
        )
        .unwrap()
        .motor;
    assert_eq!(smaller.solves.successful_trials_with_reused_jacobian, 0);
}

#[test]
fn explicit_auxiliary_rates_resolve_small_changes_at_nonzero_angle() {
    use sim_domain_robot::articulated::embedding::ImplicitStepConfig;
    let (art, g, _) = fixture(0.003);
    let map = RigidEmbedding::new(&art, &["joint.shaft".into()], Default::default()).unwrap();
    // At this nonzero angle, subtracting rounded endpoint states cannot resolve
    // the prescribed speed to 1e-10 rad/s over a short event-location interval.
    let old: [f64; 1] = [-0.2761420885415331];
    let speed = -0.00001;
    for h in [1e-7, 1e-9, 1e-11] {
        let expected = old[0] + h * speed;
        assert!(((expected - old[0]) / h - speed).abs() > 1e-10);
        let step = map
            .step_implicit_coupled_with_rates(
                &g,
                &old,
                0.24,
                h,
                &ImplicitStepConfig {
                    auxiliary_rate_unknowns: true,
                    ..Default::default()
                },
                |_, _, _, _, rates| {
                    Ok(CoupledForces {
                        generalized_loads: vec![0.0],
                        auxiliary_residuals: vec![rates[0] - speed],
                    })
                },
            )
            .unwrap();
        assert_eq!(step.auxiliary, vec![expected]);
        assert!(step.diagnostics.maximum_auxiliary_residual < 1e-12);
        assert_eq!(step.endpoint.generalized.q, g.q);
    }
}

#[test]
fn controller_sample_reuse_requires_cross_step_reuse_and_preserves_legacy_serialization() {
    use sim_domain_robot::articulated::embedding::ImplicitStepConfig;
    assert!(serde_json::to_value(ImplicitStepConfig::default()).unwrap().get("reuse_controller_sample_jacobian").is_none());
    let (art,g,cfg)=fixture(0.003);
    let map=RigidEmbedding::new(&art,&["joint.shaft".into()],Default::default()).unwrap();
    let mut bank=EmbeddedMotorBank::new(&art,&[cfg]).unwrap();
    let initial=bank.initial_states();
    let result=bank.advance_with_control(&map,&g,&initial,&0.0,0.0,0.001,
        &ImplicitStepConfig{reuse_controller_sample_jacobian:true,..Default::default()},
        &Default::default(),&mut FixedDrive,|_,_|Ok(vec![0.0]));
    assert!(matches!(result,Err(e) if e.contains("controller-sample reuse requires")));
    assert_eq!(initial,bank.initial_states());
}

#[test]
fn newton_failure_audit_is_time_scoped_and_does_not_change_the_failure() {
    use sim_domain_robot::articulated::embedding::ImplicitStepConfig;
    let (art,g,_)=fixture(0.003);
    let map=RigidEmbedding::new(&art,&["joint.shaft".into()],Default::default()).unwrap();
    assert!(serde_json::to_value(ImplicitStepConfig::default()).unwrap().get("newton_audit_window_s").is_none());
    let mut errors=Vec::new();
    for window in [None,Some([0.0,0.001]),Some([1.0,2.0])] {
        let mut config=ImplicitStepConfig{newton_audit_window_s:window,..Default::default()};
        config.newton.max_iterations=1;
        let result=map.step_implicit_coupled_with_rates(&g,&[1.0],0.0,0.001,&config,
            |_,_,_,x,_|Ok(CoupledForces{generalized_loads:vec![0.0],auxiliary_residuals:vec![x[0]*x[0]-2.0]}));
        errors.push(result.unwrap_err());
    }
    assert_eq!(errors[0],errors[2]);
    assert!(errors[1].starts_with(&errors[0]));
    assert!(errors[1].contains("Newton tail"));
    assert!(!errors[0].contains("Newton tail"));
    for invalid in [[-1.0,0.0],[2.0,1.0],[0.0,f64::INFINITY]] {
        let result=map.step_implicit_coupled_with_rates(&g,&[1.0],0.0,0.001,
            &ImplicitStepConfig{newton_audit_window_s:Some(invalid),..Default::default()},
            |_,_,_,_,_|panic!("invalid audit window must be rejected before solving"));
        assert!(matches!(result,Err(e) if e.contains("invalid Newton audit time window")));
    }
}

#[test]
fn servo_motor_independence_is_audited_and_coloring_preserves_the_complete_run() {
    use sim_domain_robot::articulated::embedding::{EmbeddedDriverBank,EmbeddedDriverConfig,EmbeddedServoBank,EmbeddedServoConfig,ServoBoundary,SampledMotorControl,ImplicitStepConfig};
    let (base,_,cfg)=fixture(0.003);
    let mut model=(*base.model).clone();
    model.links.push(box_link("rotor_b",[0.1;3],1.0,[0.3,0.0,0.0],false));
    model.joints.push(joint("shaft_b","revolute",Some("ground"),"rotor_b",[0.3,0.0,0.0],[0.0,1.0,0.0]));
    let art=Articulated::new(Arc::new(model),&Options {contact:false,flex:false,..Default::default()}).unwrap();
    let g=art.generalized(art.states().iter().map(|s|s.initial).collect(),vec![0.0;art.state_count],&vec![0.0;art.port_names.len()+1],vec![]);
    let mut a=cfg.clone();a.parameters.insert("backlash".into(),0.002);
    let mut b=a.clone();b.dof="joint.shaft_b".into();b.parameters.insert("ratio".into(),3.0);
    let mut motors=EmbeddedMotorBank::new_with_events(&art,&[a,b]).unwrap();
    let names=motors.dof_names();
    let drivers=EmbeddedDriverBank::new(&motors,&names.iter().map(|dof|EmbeddedDriverConfig {dof:dof.clone(),parameters:[("on_resistance".into(),0.5),("current_limit".into(),0.1)].into()}).collect::<Vec<_>>()).unwrap();
    let mut servos=EmbeddedServoBank::new(&motors,&names.iter().map(|dof|EmbeddedServoConfig {dof:dof.clone(),parameters:[("rate".into(),500.0),("kp".into(),3.0),("limit".into(),1.0)].into()}).collect::<Vec<_>>()).unwrap();
    let boundaries=[ServoBoundary {target_rad:0.1,supply_voltage_v:10.0,winding_temperature_k:293.15},ServoBoundary {target_rad:-0.2,supply_voltage_v:9.0,winding_temperature_k:305.0}];
    let held=servos.initial_states();let initial=motors.initial_states();
    let layout=motors.state_layout();let pattern=sim_solve::BlockDiagonalColoring::new(&layout.iter().map(|(_,n,_)|*n).collect::<Vec<_>>()).unwrap();
    {
        let control=servos.connect(&motors,&drivers,&boundaries).unwrap();
        assert!(control.independent_motor_boundaries());
        for mode in [-1.0,0.0,1.0] {for direction in [-1.0,1.0] {
            let mut x=initial.clone();
            for (i,(s,n,_)) in layout.iter().enumerate() {
                x[*s]=direction*(0.3+i as f64);x[s+1]=direction*0.4;x[s+2]=mode*0.004;
                if *n==4 {x[s+3]=mode;}
            }
            for epsilon in [1e-4,1e-6,1e-8] {
                assert_eq!(pattern.audit_off_block(&x,epsilon,|x,r|{
                    let boundaries=control.boundaries(0.01,&g,x,&held).unwrap();
                    let rates:Vec<_>=x.iter().zip(&initial).map(|(new,old)|(new-old)/0.001).collect();
                    r.copy_from_slice(&motors.evaluate_with_rates(0.01,&g,x,&rates,&boundaries).unwrap().0.auxiliary_residuals);
                }).unwrap(),0.0);
            }
        }}
    }
    let map=RigidEmbedding::new(&art,&names,Default::default()).unwrap();
    let mut run=|colored| {
        let mut control=servos.connect(&motors,&drivers,&boundaries).unwrap();
        motors.advance_with_control(&map,&g,&initial,&held,0.0,0.008,&ImplicitStepConfig {condense_auxiliary:true,color_auxiliary_jacobian:colored,auxiliary_rate_unknowns:true,..Default::default()},&Default::default(),&mut control,|_,_|Ok(vec![0.0;2])).unwrap()
    };
    let ordinary=run(false);let colored=run(true);
    assert_eq!(ordinary.motor.motor_states,colored.motor.motor_states);
    assert_eq!(ordinary.motor.endpoint.generalized.states,colored.motor.endpoint.generalized.states);
    assert_eq!(ordinary.control_state,colored.control_state);
    assert_eq!(serde_json::to_value(&ordinary.motor.hybrid).unwrap(),serde_json::to_value(&colored.motor.hybrid).unwrap());
    assert_eq!(colored.motor.solves.successful_trial_colored_auxiliary_solves,colored.motor.solves.successful_trial_auxiliary_solves);
    assert!(colored.motor.solves.successful_trial_auxiliary_evaluations<ordinary.motor.solves.successful_trial_auxiliary_evaluations);
    assert!(!FixedDrive.independent_motor_boundaries());
}

#[test]
fn unknown_control_keeps_ordinary_probes_and_coloring_requires_condensation() {
    use sim_domain_robot::articulated::embedding::ImplicitStepConfig;
    let (art,g,cfg)=fixture(0.003);
    let map=RigidEmbedding::new(&art,&["joint.shaft".into()],Default::default()).unwrap();
    let mut motors=EmbeddedMotorBank::new_with_events(&art,&[cfg]).unwrap();
    let initial=motors.initial_states();
    let mut run=|config:&ImplicitStepConfig|motors.advance_with_control(&map,&g,&initial,&2.0,0.0,0.002,config,&Default::default(),&mut FixedDrive,|_,_|Ok(vec![0.0]));
    let config=ImplicitStepConfig {condense_auxiliary:true,..Default::default()};
    let a=run(&config).unwrap();
    let b=run(&ImplicitStepConfig {color_auxiliary_jacobian:true,..config}).unwrap();
    assert_eq!(a.motor.motor_states,b.motor.motor_states);
    assert_eq!(a.motor.solves.successful_trial_auxiliary_evaluations,b.motor.solves.successful_trial_auxiliary_evaluations);
    assert_eq!(b.motor.solves.successful_trial_colored_auxiliary_solves,0);
    assert!(run(&ImplicitStepConfig {color_auxiliary_jacobian:true,..Default::default()}).is_err());
    assert!(serde_json::to_value(ImplicitStepConfig::default()).unwrap().get("color_auxiliary_jacobian").is_none());
    for (condense_auxiliary, auxiliary_rate_unknowns) in [(false,false),(false,true),(true,false)] {
        assert!(run(&ImplicitStepConfig {
            auxiliary_endpoint_correction_scale:true,condense_auxiliary,auxiliary_rate_unknowns,
            ..Default::default()
        }).is_err());
    }
    assert!(serde_json::to_value(ImplicitStepConfig::default()).unwrap().get("auxiliary_endpoint_correction_scale").is_none());
}
