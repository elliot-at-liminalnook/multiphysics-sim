mod common;
use common::*;
use nalgebra::{Quaternion, UnitQuaternion, Vector3};
use sim_core::Behavior;
use sim_domain_robot::articulated::embedding::RigidEmbedding;
use sim_domain_robot::{Articulated, Generalized, Options};
use std::sync::Arc;

fn body(slider: bool, contact: bool) -> (Articulated, Generalized) {
    let mut m = empty_model();
    m.gravity = [0.0; 3];
    if slider {
        m.links
            .push(box_link("ground", [0.1; 3], 1.0, [0.0; 3], true));
    }
    m.links
        .push(box_link("body", [0.1; 3], 2.0, [0.0, 0.0, 1.0], false));
    if slider {
        m.joints.push(joint(
            "slide",
            "prismatic",
            Some("ground"),
            "body",
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
        ));
    }
    let art = Articulated::new(
        Arc::new(m),
        &Options {
            contact,
            flex: false,
            ..Options::default()
        },
    )
    .unwrap();
    let states = art.states().iter().map(|s| s.initial).collect();
    let g = art.generalized(
        states,
        vec![0.0; art.state_count],
        &vec![0.0; art.port_names.len() + 1],
        vec![],
    );
    (art, g)
}

#[test]
fn failed_cached_mechanics_restarts_fresh_before_subdivision() {
    use sim_domain_robot::articulated::embedding::{ImplicitSolverWorkspace, ImplicitStepConfig};
    use sim_dynamics::hybrid::HybridConfig;
    let (art, mut g)=body(true,false);g.q[0]=0.1;
    let map=RigidEmbedding::new(&art,&["slide.slide".into()],Default::default()).unwrap();
    let config=ImplicitStepConfig {reuse_step_jacobian:true,..Default::default()};
    let mut workspace=ImplicitSolverWorkspace::default();
    let load=|t:f64,g:&Generalized| Ok(vec![-40000.0*g.q[0].powi(3)-2.0*g.qd[0]+(20.0*t).sin()]);
    g=map.advance_implicit_mechanics_cached(&g,0.0,0.01,&config,&HybridConfig::default(),&mut workspace,load).unwrap().endpoint.generalized;
    let mut restart=config.clone();restart.restart_failed_reused_mechanics=true;restart.cached_mechanical_iteration_limit=Some(1);
    let recovered=map.advance_implicit_mechanics_cached(&g,0.01,0.01,&restart,&HybridConfig::default(),&mut workspace,load).unwrap();
    assert_eq!(recovered.segments.len(),1);
    assert_eq!(recovered.refinement.rejected_trials,0);
    assert!(recovered.segments[0].diagnostics.fresh_restart_reason.is_some());
    assert!(!recovered.segments[0].diagnostics.started_with_reused_jacobian);
    let fresh=map.advance_implicit_mechanics(&g,0.01,0.01,&config,&HybridConfig::default(),load).unwrap();
    assert_eq!(fresh.endpoint.generalized.q,recovered.endpoint.generalized.q);
    assert_eq!(fresh.endpoint.generalized.qd,recovered.endpoint.generalized.qd);
    restart.restart_failed_reused_mechanics=false;
    assert!(map.advance_implicit_mechanics(&g,0.01,0.01,&restart,&HybridConfig::default(),load).is_err());
    restart.restart_failed_reused_mechanics=true;restart.reuse_step_jacobian=false;
    assert!(map.advance_implicit_mechanics(&g,0.01,0.01,&restart,&HybridConfig::default(),load).is_err());
}

#[test]
fn cached_mechanical_intervals_match_linear_solution_and_rollback_failed_trials() {
    use sim_domain_robot::articulated::embedding::{ImplicitSolverWorkspace, ImplicitStepConfig};
    use sim_dynamics::hybrid::HybridConfig;
    for (broyden_updates, broyden_negligible_updates, linearized_jacobian_probes) in
        [(false, false, false), (true, false, false), (true, true, false), (true, false, true)] {
    let (art, mut g) = body(true, false);
    g.q[0] = 0.1;
    let map = RigidEmbedding::new(&art, &["slide.slide".into()], Default::default()).unwrap();
    let mut config = ImplicitStepConfig {reuse_step_jacobian: true, ..Default::default()};
    config.newton.broyden_updates = broyden_updates;
    config.newton.broyden_negligible_updates = broyden_negligible_updates;
    config.linearized_jacobian_probes = linearized_jacobian_probes;
    let refinement = HybridConfig {maximum_halvings: 1, ..Default::default()};
    let mut workspace = ImplicitSolverWorkspace::default();
    let load = |_: f64, g: &Generalized| Ok(vec![-30.0*g.q[0]-2.0*g.qd[0]]);
    let mut reused = 0;
    for i in 0..40 {
        let h = 0.01;
        let expected_v = (2.0*g.qd[0]-h*30.0*g.q[0])/(2.0+h*2.0+h*h*30.0);
        let expected_q = g.q[0]+h*expected_v;
        let step = map.advance_implicit_mechanics_cached(&g,i as f64*h,h,&config,&refinement,&mut workspace,load).unwrap();
        assert_eq!(step.segments.len(),1);
        reused += usize::from(step.segments[0].diagnostics.started_with_reused_jacobian);
        assert!((step.endpoint.generalized.q[0]-expected_q).abs()<1e-10);
        assert!((step.endpoint.generalized.qd[0]-expected_v).abs()<1e-10);
        g = step.endpoint.generalized;
    }
    assert!(reused>20);
    let mut reference_workspace = workspace.clone();
    assert!(map.advance_implicit_mechanics_cached(&g,0.4,0.01,&config,&refinement,&mut workspace,|_,_|Err("deliberate force failure".into())).is_err());
    let actual = map.advance_implicit_mechanics_cached(&g,0.4,0.01,&config,&refinement,&mut workspace,load).unwrap();
    let reference = map.advance_implicit_mechanics_cached(&g,0.4,0.01,&config,&refinement,&mut reference_workspace,load).unwrap();
    assert_eq!(actual.endpoint.generalized.q,reference.endpoint.generalized.q);
    assert_eq!(actual.endpoint.generalized.qd,reference.endpoint.generalized.qd);
    assert_eq!(serde_json::to_value(actual.segments).unwrap(),serde_json::to_value(reference.segments).unwrap());
    let smaller = map.advance_implicit_mechanics_cached(&actual.endpoint.generalized,0.41,0.005,&config,&refinement,&mut workspace,load).unwrap();
    assert!(!smaller.segments[0].diagnostics.started_with_reused_jacobian);
    let mut changed=config.clone(); changed.linearized_probe_relative_step*=4.0;
    let changed=map.advance_implicit_mechanics_cached(&smaller.endpoint.generalized,0.415,0.005,&changed,&refinement,&mut workspace,load).unwrap();
    assert!(!changed.segments[0].diagnostics.started_with_reused_jacobian);
    }
}

#[test]
fn derivative_probe_radius_rejects_invalid_configuration_without_changing_state() {
    use sim_domain_robot::articulated::embedding::ImplicitStepConfig;
    let (art,g)=body(true,false);
    let map=RigidEmbedding::new(&art,&["slide.slide".into()],Default::default()).unwrap();
    let before=(g.q.clone(),g.qd.clone(),g.states.clone());
    for radius in [0.0,-1e-6,f64::NAN,f64::INFINITY,1e-3] {
        let config=ImplicitStepConfig {linearized_jacobian_probes:true,linearized_probe_relative_step:radius,..Default::default()};
        assert!(map.step_implicit(&g,0.0,0.01,&config,|_,_|Ok(vec![0.0])).is_err());
        assert_eq!(before,(g.q.clone(),g.qd.clone(),g.states.clone()));
    }
}

#[test]
fn mechanical_subdivision_preserves_direct_steps_and_recovers_nonlinear_drag() {
    use sim_domain_robot::articulated::embedding::ImplicitStepConfig;
    use sim_dynamics::hybrid::HybridConfig;
    let (art, mut g)=body(true,false);
    let map=RigidEmbedding::new(&art,&["slide.slide".into()],Default::default()).unwrap();
    let config=ImplicitStepConfig::default();
    let load=|_:f64,_:&Generalized| Ok(vec![2.0]);
    let direct=map.step_implicit(&g,0.0,0.02,&config,load).unwrap();
    let recovered=map.advance_implicit_mechanics(&g,0.0,0.02,&config,&HybridConfig::default(),load).unwrap();
    assert_eq!(direct.endpoint.generalized.q,recovered.endpoint.generalized.q);
    assert_eq!(direct.endpoint.generalized.qd,recovered.endpoint.generalized.qd);
    assert_eq!(recovered.refinement.continuous_attempts,1);
    assert_eq!(recovered.segments.len(),1);
    assert!((recovered.endpoint.generalized.qd[0]-0.02).abs()<1e-12);

    // A deliberately small nonlinear iteration allowance fails the large step
    // for cubic drag. Smaller steps must meet exactly the same solver tolerances.
    g.qd[0]=10.0;
    let mut config=config;config.newton.max_iterations=5;
    let drag=|_:f64,g:&Generalized| Ok(vec![-2.0*g.qd[0].powi(3)]);
    assert!(map.step_implicit(&g,0.0,0.1,&config,drag).is_err());
    let recovered=map.advance_implicit_mechanics(&g,0.0,0.1,&config,&HybridConfig {maximum_halvings:10,maximum_segments:256,..Default::default()},drag).unwrap();
    assert!(recovered.refinement.rejected_trials>0);
    assert!(recovered.segments.len()>1);
    let mut replay=g.clone();let mut time=0.0;
    for segment in &recovered.segments {
        assert!((segment.start_time_s-time).abs()<1e-12);
        replay=map.step_implicit(&replay,time,segment.step_s,&config,drag).unwrap().endpoint.generalized;
        time+=segment.step_s;
    }
    assert!((time-0.1).abs()<1e-12);
    assert_eq!(replay.q,recovered.endpoint.generalized.q);
    assert_eq!(replay.qd,recovered.endpoint.generalized.qd);
    assert!(replay.qd[0]>0.0&&replay.qd[0]<g.qd[0]);
    let before=(g.q.clone(),g.qd.clone(),g.states.clone());
    assert!(map.advance_implicit_mechanics(&g,0.0,0.1,&config,&HybridConfig {maximum_halvings:0,..Default::default()},drag).is_err());
    assert_eq!(before,(g.q.clone(),g.qd.clone(),g.states.clone()));
}

#[test]
fn world_load_pulse_matches_linear_and_angular_impulse() {
    use sim_domain_robot::world_load::{WorldLoadPulse, WorldLoadSchedule};
    let (art, mut g)=body(false,false);
    let map=RigidEmbedding::new(&art,&[],Default::default()).unwrap();
    let loads=WorldLoadSchedule {version:1,base_link:"body".into(),provenance:"free cube impulse test".into(),
        maximum_force_n:2.0,maximum_moment_nm:0.1,
        pulses:vec![WorldLoadPulse {name:"push and twist".into(),start_s:0.02,duration_s:0.04,
            force_world_n:[2.0,0.0,0.0],moment_world_nm:[0.0,0.0,0.1]}],
    }.bind(&["body".into()],0.01,10).unwrap();
    for i in 0..10 {
        let w=loads.wrench(i);
        g=map.step_midpoint(&g,i as f64*0.01,0.01,|_,_|Ok(w.to_vec())).unwrap().endpoint.generalized;
    }
    let s=art.bases[0].state;
    assert!((2.0*g.states[s+7]-2.0*0.04).abs()<1e-12);
    assert!((art.links[0].inertia[(2,2)]*g.states[s+12]-0.1*0.04).abs()<1e-12);
}

#[test]
fn condensed_cross_coupled_states_match_independent_linear_system() {
    use sim_domain_robot::articulated::embedding::{CoupledForces, ImplicitStepConfig};
    use nalgebra::{Matrix3, Vector3};
    let (art, seed) = body(true, false);
    let map = RigidEmbedding::new(&art, &["slide.slide".into()], Default::default()).unwrap();
    // Two mutually coupled internal states drive a 2 kg slider. Condensation
    // must retain both off-diagonal terms and feedback from mechanical speed.
    for rates in [false, true] {
        for h in [0.01, 0.001] {
            let matrix = Matrix3::new(2.0/h, -1.0, -2.0, 2.0, 1.0/h+3.0, -0.7, -1.0, -0.2, 1.0/h+5.0).lu();
            let mut g = seed.clone();
            let mut x = vec![0.3, -0.2];
            for i in 0..20 {
                let drive = if i<10 {4.0} else {-4.0};
                let expected = matrix.solve(&Vector3::new(2.0*g.qd[0]/h, x[0]/h+drive, x[1]/h-1.0)).unwrap();
                let q = g.q[0]+h*expected[0];
                let step = map.step_implicit_coupled_with_rates(
                    &g, &x, i as f64*h, h,
                    &ImplicitStepConfig {condense_auxiliary:true, auxiliary_rate_unknowns:rates, ..Default::default()},
                    |_,_,g,x,r| Ok(CoupledForces {
                        generalized_loads:vec![x[0]+2.0*x[1]],
                        auxiliary_residuals:vec![r[0]+3.0*x[0]-0.7*x[1]+2.0*g.qd[0]-drive, r[1]-0.2*x[0]+5.0*x[1]-g.qd[0]+1.0],
                    }),
                ).unwrap();
                assert!((step.endpoint.generalized.qd[0]-expected[0]).abs()<1e-10);
                assert!((step.endpoint.generalized.q[0]-q).abs()<1e-10);
                assert!((step.auxiliary[0]-expected[1]).abs()<1e-10);
                assert!((step.auxiliary[1]-expected[2]).abs()<1e-10);
                assert!(step.diagnostics.maximum_auxiliary_residual<=1e-10);
                assert!(step.diagnostics.auxiliary_evaluations>step.diagnostics.endpoint_evaluations);
                g=step.endpoint.generalized;
                x=step.auxiliary;
            }
        }
    }
    let before = seed.states.clone();
    let failed = map.step_implicit_coupled(&seed, &[0.0], 0.0, 0.01,
        &ImplicitStepConfig {condense_auxiliary:true,..Default::default()},
        |_,_,_,_| Ok(CoupledForces {generalized_loads:vec![1.0], auxiliary_residuals:vec![1.0]}));
    assert!(failed.unwrap_err().contains("local auxiliary solve"));
    assert_eq!(seed.states,before);
}

#[test]
fn prepared_dynamics_owns_contact_state_and_applies_fresh_forces() {
    let (art, mut seed) = body(false, true);
    let base = art.bases[0].state;
    seed.states[base + 2] = 0.0499;
    let map = RigidEmbedding::new(&art, &[], Default::default()).unwrap();
    let mut motion = map
        .solve(&seed, &[], &[0.2, 0.0, 0.0, 0.0, 0.0, 0.0])
        .unwrap();
    let prepared = map.prepare_dynamics(&motion).unwrap();
    let original = prepared.accelerations(&[0.0; 6]).unwrap();
    assert!(!original.contacts.is_empty());
    let forced = prepared
        .accelerations(&[6.0, 0.0, 0.0, 0.0, 0.0, 0.0])
        .unwrap();
    // Independent F=ma: 6 N changes a 2 kg COM's acceleration by 3 m/s^2.
    assert!((forced.full_accelerations[0] - original.full_accelerations[0] - 3.0).abs() < 1e-12);
    assert_eq!(forced.bristle_rates, original.bristle_rates);
    assert_eq!(forced.contacts.len(), original.contacts.len());
    // Mutating the input after preparation cannot change the owned snapshot.
    motion.generalized.states[base + 2] = 1.0;
    motion.generalized.states[art.links[0].bristle_state] = 0.01;
    let unchanged = prepared.accelerations(&[0.0; 6]).unwrap();
    assert_eq!(unchanged.full_accelerations, original.full_accelerations);
    assert_eq!(unchanged.bristle_rates, original.bristle_rates);
    let fresh = map
        .prepare_dynamics(&motion)
        .unwrap()
        .accelerations(&[0.0; 6])
        .unwrap();
    assert!(fresh.contacts.is_empty());
    assert!(fresh.full_accelerations.amax() < 1e-12);
    assert!(prepared.accelerations(&[f64::NAN; 6]).is_err());
    assert!(prepared.accelerations(&[0.0; 5]).is_err());
    let invalid = sim_domain_robot::articulated::embedding::ImplicitStepConfig {
        reuse_mechanical_dynamics: true,
        ..Default::default()
    };
    assert!(
        map.step_implicit(&seed, 0.0, 0.001, &invalid, |_, _| Ok(vec![0.0; 6]))
            .is_err()
    );
}

#[test]
fn free_body_constant_force_and_world_rotation_match_exact_motion() {
    let (art, mut g) = body(false, false);
    let map = RigidEmbedding::new(&art, &[], Default::default()).unwrap();
    let s = art.bases[0].state;
    let q0 = UnitQuaternion::from_axis_angle(&Vector3::x_axis(), 0.4);
    g.states[s + 3..s + 7].copy_from_slice(&[q0.w, q0.i, q0.j, q0.k]);
    g.states[s + 7..s + 13].copy_from_slice(&[0.3, -0.2, 0.1, 0.0, 0.0, 1.0]);
    for i in 0..20 {
        g = map
            .step_midpoint(&g, i as f64 * 0.01, 0.01, |_, _| {
                Ok(vec![2.0, 0.0, 0.0, 0.0, 0.0, 0.0])
            })
            .unwrap()
            .endpoint
            .generalized;
    }
    let t = 0.2;
    for (actual, expected) in
        g.states[s..s + 3]
            .iter()
            .zip([0.3 * t + 0.5 * t * t, -0.2 * t, 1.0 + 0.1 * t])
    {
        assert!((actual - expected).abs() < 1e-12);
    }
    let expected = UnitQuaternion::from_axis_angle(&Vector3::z_axis(), t) * q0;
    let actual = UnitQuaternion::from_quaternion(Quaternion::new(
        g.states[s + 3],
        g.states[s + 4],
        g.states[s + 5],
        g.states[s + 6],
    ));
    assert!(actual.angle_to(&expected) < 1e-12);
    assert!((g.states[s + 7] - 0.5).abs() < 1e-12);
    assert!((g.rates[s + 7] - 1.0).abs() < 1e-12);
    assert_eq!(&g.rates[s..s + 3], &g.states[s + 7..s + 10]);
}

#[test]
fn oscillator_error_decreases_quadratically_with_step_refinement() {
    let (art, mut seed) = body(true, false);
    seed.q[0] = 0.2;
    let map = RigidEmbedding::new(&art, &["slide.slide".into()], Default::default()).unwrap();
    let mut errors = Vec::new();
    for steps in [50, 100, 200] {
        let h = 1.0 / steps as f64;
        let mut g = seed.clone();
        for i in 0..steps {
            g = map
                .step_midpoint(&g, i as f64 * h, h, |_, g| Ok(vec![-8.0 * g.q[0]]))
                .unwrap()
                .endpoint
                .generalized;
        }
        let expected_q = 0.2 * 2.0_f64.cos();
        let expected_v = -0.4 * 2.0_f64.sin();
        errors.push((g.q[0] - expected_q).hypot((g.qd[0] - expected_v) / 2.0));
    }
    assert!(errors[2] < 1e-5, "{errors:?}");
    assert!(
        errors
            .windows(2)
            .all(|e| e[0] / e[1] > 3.8 && e[0] / e[1] < 4.2),
        "{errors:?}"
    );
}

#[test]
fn contact_memory_decays_and_failure_leaves_input_unchanged() {
    let (art, mut seed) = body(false, true);
    let s = art.links[0].bristle_state;
    seed.states[s] = 0.001;
    let map = RigidEmbedding::new(&art, &[], Default::default()).unwrap();
    let mut errors = Vec::new();
    for n in [20, 40, 80] {
        let h = 0.02 / n as f64;
        let mut g = seed.clone();
        for i in 0..n {
            g = map
                .step_midpoint(&g, i as f64 * h, h, |_, _| Ok(vec![0.0; 6]))
                .unwrap()
                .endpoint
                .generalized;
        }
        errors.push((g.states[s] - 0.001 * (-4.0_f64).exp()).abs());
        assert!((g.rates[s] + 200.0 * g.states[s]).abs() < 1e-14);
    }
    assert!(errors.windows(2).all(|e| e[0] / e[1] > 3.8), "{errors:?}");
    let before = seed.clone();
    assert!(
        map.step_midpoint(&seed, 0.0, 0.01, |t, _| if t > 0.0 {
            Err("load failure".into())
        } else {
            Ok(vec![0.0; 6])
        })
        .is_err()
    );
    assert_eq!(seed.states, before.states);
    assert_eq!(seed.q, before.q);
    assert_eq!(seed.rates, before.rates);
    for h in [0.0, -0.1, f64::NAN] {
        assert!(
            map.step_midpoint(&seed, 0.0, h, |_, _| Ok(vec![0.0; 6]))
                .is_err()
        );
    }
}

#[test]
fn implicit_stiff_viscous_load_matches_backward_euler_without_reversal() {
    let (art, _) = body(true, false);
    let mut model = (*art.model).clone();
    model.joints[0].physics.friction.viscous = 20_000.0;
    let art = Articulated::new(
        Arc::new(model),
        &Options {
            contact: false,
            flex: false,
            ..Options::default()
        },
    )
    .unwrap();
    let mut g = art.generalized(
        art.states().iter().map(|s| s.initial).collect(),
        vec![0.0; art.state_count],
        &vec![0.0; art.port_names.len() + 1],
        vec![],
    );
    g.qd[0] = 1.0;
    let map = RigidEmbedding::new(&art, &["slide.slide".into()], Default::default()).unwrap();
    let h = 0.01; // h*b/m=100: far outside explicit midpoint's stability region.
    let mut expected_v = 1.0;
    let mut expected_q = 0.0;
    for i in 0..4 {
        expected_v /= 101.0;
        expected_q += h * expected_v;
        let step = map
            .step_implicit(&g, i as f64 * h, h, &Default::default(), |_, _| {
                Ok(vec![0.0])
            })
            .unwrap();
        assert!(step.diagnostics.maximum_scaled_velocity_residual < 1e-9);
        g = step.endpoint.generalized;
        assert!(g.qd[0] >= 0.0);
        assert!((g.qd[0] - expected_v).abs() < 1e-12);
        assert!((g.q[0] - expected_q).abs() < 1e-12);
    }
}

#[test]
fn implicit_oscillator_refines_to_the_continuous_solution() {
    for linearized_jacobian_probes in [false,true] {
    let config=sim_domain_robot::articulated::embedding::ImplicitStepConfig {linearized_jacobian_probes,..Default::default()};
    let (art, mut seed) = body(true, false);
    seed.q[0] = 0.2;
    let map = RigidEmbedding::new(&art, &["slide.slide".into()], Default::default()).unwrap();
    let mut errors = Vec::new();
    for n in [40, 80, 160] {
        let h = 1.0 / n as f64;
        let mut g = seed.clone();
        for i in 0..n {
            g = map
                .step_implicit(&g, i as f64 * h, h, &config, |_, g| {
                    Ok(vec![-8.0 * g.q[0]])
                })
                .unwrap()
                .endpoint
                .generalized;
        }
        errors.push((g.q[0] - 0.2 * 2.0_f64.cos()).hypot((g.qd[0] + 0.4 * 2.0_f64.sin()) / 2.0));
    }
    assert!(errors[2] < 0.0025, "{errors:?}");
    assert!(
        errors
            .windows(2)
            .all(|e| e[0] / e[1] > 1.9 && e[0] / e[1] < 2.1),
        "{errors:?}"
    );
    }
}

#[test]
fn implicit_contact_memory_and_atomic_failure() {
    let (art, mut seed) = body(false, true);
    let s = art.links[0].bristle_state;
    seed.states[s..s + 3].copy_from_slice(&[0.001, -0.002, 0.003]);
    let map = RigidEmbedding::new(&art, &[], Default::default()).unwrap();
    let h = 0.1; // 20 decay time constants in one step.
    let step = map
        .step_implicit(&seed, 0.0, h, &Default::default(), |_, _| Ok(vec![0.0; 6]))
        .unwrap();
    assert!(step.diagnostics.maximum_contact_history_residual < 1e-15);
    for k in 0..3 {
        assert!(
            (step.endpoint.generalized.states[s + k] - seed.states[s + k] / 21.0).abs() < 1e-15
        );
    }
    let before = seed.clone();
    assert!(
        map.step_implicit(&seed, 0.0, h, &Default::default(), |_, _| Err(
            "load failure".into()
        ))
        .is_err()
    );
    assert_eq!(seed.states, before.states);
    assert_eq!(seed.q, before.q);
    assert_eq!(seed.qd, before.qd);
    for h in [0.0, -0.1, f64::NAN] {
        assert!(
            map.step_implicit(&seed, 0.0, h, &Default::default(), |_, _| Ok(vec![0.0; 6]))
                .is_err()
        );
    }
}

#[test]
fn implicit_sliding_contact_matches_independent_scalar_force_balance() {
    let mut m = empty_model();
    m.gravity = [0.0; 3];
    m.links
        .push(box_link("ground", [0.1; 3], 1.0, [0.0, 0.0, -10.0], true));
    m.links
        .push(box_link("body", [0.1; 3], 2.0, [0.0, 0.0, 0.049], false));
    m.joints.push(joint(
        "slide",
        "prismatic",
        Some("ground"),
        "body",
        [0.0, 0.0, 0.049],
        [1.0, 0.0, 0.0],
    ));
    let art = Articulated::new(
        Arc::new(m),
        &Options {
            contact: true,
            flex: false,
            ..Options::default()
        },
    )
    .unwrap();
    let mut g = art.generalized(
        art.states().iter().map(|s| s.initial).collect(),
        vec![0.0; art.state_count],
        &vec![0.0; art.port_names.len() + 1],
        vec![],
    );
    let map = RigidEmbedding::new(&art, &["slide.slide".into()], Default::default()).unwrap();
    g.qd[0] = 0.2;
    let s = art.links[1].bristle_state;
    g.states[s] = 1e-5;
    let h = 0.005;
    // Five bottom fixture vertices, each penetrating by 1 mm. A prismatic
    // guide holds height/orientation; only horizontal velocity is unknown.
    let normal = 5.0 * art.floor_k * 0.001;
    let residual = |v: f64| {
        let mu = 0.3 + 0.1 * (-(v / 0.01).powi(2)).exp();
        let decay = art.floor_k * v.abs() / (mu * normal + 1e-3);
        let z = (g.states[s] + h * v) / (1.0 + h * decay);
        let zd = v - decay * z;
        let force = -art.floor_k * z - 2.0 * (art.floor_k * 2.0).sqrt() * zd;
        (v - 0.2 - h * force / 2.0, z)
    };
    let (mut lo, mut hi) = (0.0, 0.2);
    assert!(residual(lo).0 < 0.0 && residual(hi).0 > 0.0);
    for _ in 0..80 {
        let mid = 0.5 * (lo + hi);
        if residual(mid).0 < 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let expected = 0.5 * (lo + hi);
    for linearized_jacobian_probes in [false,true] {
    let config=sim_domain_robot::articulated::embedding::ImplicitStepConfig {linearized_jacobian_probes,..Default::default()};
    let step = map
        .step_implicit(&g, 0.0, h, &config, |_, _| Ok(vec![0.0]))
        .unwrap();
    let end = &step.endpoint.generalized;
    assert!(
        (end.qd[0] - expected).abs() < 1e-9,
        "{} vs {expected}",
        end.qd[0]
    );
    assert!((end.states[s] - residual(expected).1).abs() < 1e-11);
    assert!((end.q[0] - h * expected).abs() < 1e-11);
    let measured_normal: f64 = step
        .endpoint
        .contacts
        .iter()
        .filter(|c| c.link == 1)
        .map(|c| c.force.z)
        .sum();
    assert!((measured_normal - normal).abs() < 1e-8);
    assert!(step.diagnostics.maximum_contact_history_residual < 1e-11);
    }
}
