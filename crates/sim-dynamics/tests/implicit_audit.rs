use sim_dynamics::{Integrator,Simulation,System};

struct Decay;
impl System for Decay {
    fn dimension(&self)->usize { 2 }
    fn algebraic(&self)->Option<Vec<bool>> { Some(vec![false,true]) }
    fn residual(&self,t:f64,x:&[f64],rate:&[f64],out:&mut[f64]) {
        out[0]=rate[0]+x[0];
        out[1]=x[1]-x[0]*x[0]-t;
    }
}
#[test]
fn audit_captures_the_actual_mixed_dae_stage_without_changing_trajectory() {
    let run=|limit| {
        let mut s=Simulation::new(Decay,Integrator::implicit_midpoint(),vec![1.0,1.0]);
        s.set_attempt_audit_limit(limit);
        for _ in 0..4 { s.step(0.2).unwrap(); }
        s
    };
    let ordinary=run(0); let audited=run(2);
    assert_eq!(ordinary.state,audited.state);
    assert_eq!(ordinary.events,audited.events);
    assert_eq!(ordinary.stats.subdivided_steps,audited.stats.subdivided_steps);
    assert!(ordinary.implicit_attempts.is_empty());
    assert_eq!(audited.implicit_attempts.len(),2);
    let a=&audited.implicit_attempts[0];
    assert!(a.solve_succeeded);
    assert_eq!(a.committed, Some(true));
    assert_eq!(a.stage_time,0.1);
    let end=1.0+a.step*a.stage_rate[0];
    assert!((end-0.9/1.1).abs()<1e-10);
    assert!((a.stage_state[0]-(1.0+end)/2.0).abs()<1e-14);
    assert!((a.stage_state[1]-a.stage_state[0].powi(2)-a.stage_time).abs()<1e-10);
    let mut r=vec![0.0;2];
    Decay.residual(a.stage_time,&a.stage_state,&a.stage_rate,&mut r);
    assert_eq!(r,a.residual);
    assert!(!a.newton.iterations.is_empty());
    assert_eq!(a.newton.row_scale.len(),2);
    assert_eq!(a.newton.residual_limits.len(),2);
    assert!(a.residual.iter().zip(&a.newton.residual_limits).all(|(r, limit)| r.abs() <= *limit));
}
struct Impossible;
impl System for Impossible {
    fn dimension(&self)->usize { 1 }
    fn algebraic(&self)->Option<Vec<bool>> { Some(vec![true]) }
    fn residual(&self,_:f64,_:&[f64],_:&[f64],out:&mut[f64]) { out[0]=1.0; }
}

struct AnalyticDecay { wrong_algebraic_derivative:bool, offset:f64 }
impl System for AnalyticDecay {
    fn dimension(&self)->usize {2}
    fn algebraic(&self)->Option<Vec<bool>> {Some(vec![false,true])}
    fn residual(&self,t:f64,x:&[f64],rate:&[f64],out:&mut[f64]) {
        Decay.residual(t,x,rate,out);
        out[0]+=self.offset;
    }
    fn jacobian(&self,_:f64,x:&[f64],_:&[f64],out:&mut sim_dynamics::JacobianParts)->bool {
        out.d_dx.extend([(0,0,1.0),(1,0,-2.0*x[0]),
            (1,1,if self.wrong_algebraic_derivative {0.5} else {1.0})]);
        out.d_drate.push((0,0,1.0));
        true
    }
}

#[test]
fn captured_implicit_jacobian_checks_stage_weights_and_rejects_changed_context() {
    use sim_dynamics::jacobian_check::{check_implicit_jacobian,CheckConfig};
    let mut s=Simulation::new(AnalyticDecay {wrong_algebraic_derivative:false,offset:0.0},
        Integrator::implicit_midpoint(),vec![1.0,1.0]);
    s.set_attempt_audit_limit(2);
    s.step(0.2).unwrap();
    let point=&s.implicit_attempts[0];
    let at=point.last_linearization.as_ref().unwrap();
    // Verify the stored base was evaluated at its increment, not at the
    // terminal converged state. Differential stage weight is theta=1/2,
    // algebraic stage weight is one, and the rate weight is 1/h=5.
    let x=[1.0+0.5*at.increment[0],1.0+at.increment[1]];
    let rate=[at.increment[0]/0.2,at.increment[1]/0.2];
    let mut residual=[0.0;2];
    s.system.residual(0.1,&x,&rate,&mut residual);
    assert_eq!(residual.as_slice(),at.residual);
    let report=check_implicit_jacobian(&s.system,point,&CheckConfig::default()).unwrap();
    assert!(report.passed,"{report:?}");
    let wrong=AnalyticDecay {wrong_algebraic_derivative:true,offset:0.0};
    let report=check_implicit_jacobian(&wrong,point,&CheckConfig::default()).unwrap();
    assert!(!report.passed && report.mismatches>0,"{report:?}");
    let changed=AnalyticDecay {wrong_algebraic_derivative:false,offset:1.0};
    assert!(check_implicit_jacobian(&changed,point,&CheckConfig::default()).unwrap_err().contains("context"));
    let mut legacy=point.clone();
    legacy.last_linearization=None;
    assert!(check_implicit_jacobian(&s.system,&legacy,&CheckConfig::default()).is_err());
}
#[test]
fn failed_attempts_record_retry_method_and_depth_with_bounded_storage() {
    let mut s=Simulation::new(Impossible,Integrator::implicit_midpoint(),vec![0.0]);
    s.set_attempt_audit_limit(2);
    assert!(s.step(0.2).is_err());
    assert_eq!(s.implicit_attempts.len(),2);
    let (a,b)=(&s.implicit_attempts[0],&s.implicit_attempts[1]);
    assert_eq!((a.theta,a.subdivision_depth,a.step),(0.5,0,0.2));
    assert_eq!((b.theta,b.subdivision_depth,b.step),(1.0,1,0.1));
    assert!(!a.solve_succeeded && !b.solve_succeeded);
    assert_eq!((a.committed,b.committed),(Some(false),Some(false)));
    assert!(a.error.as_ref().unwrap().contains("singular"));
    s.set_attempt_audit_limit(0);
    assert!(s.implicit_attempts.is_empty());
}

#[test]
fn captured_resolve_uses_common_initial_state_and_mixed_stage_weights() {
    use sim_dynamics::attempt_check::{resolve_implicit_attempt,refine_implicit_attempt};
    let mut s=Simulation::new(Decay,Integrator::implicit_midpoint(),vec![1.0,1.0]);
    s.set_attempt_audit_limit(2);
    s.step(0.2).unwrap();
    let original=&s.implicit_attempts[0];
    let initial=[1.01,1.01_f64.powi(2)];
    let resolved=resolve_implicit_attempt(&Decay,original,&initial,Default::default()).unwrap();
    assert!(resolved.solve_succeeded,"{resolved:?}");
    assert_eq!(resolved.initial_state,initial);
    assert_eq!(resolved.theta,0.5);
    assert!((resolved.stage_state[0]-1.01/1.1).abs()<1e-10);
    assert!((resolved.stage_rate[0]+resolved.stage_state[0]).abs()<1e-10);
    assert!((resolved.stage_state[1]-resolved.stage_state[0].powi(2)-0.1).abs()<1e-10);
    let changed=AnalyticDecay {wrong_algebraic_derivative:false,offset:1.0};
    assert!(resolve_implicit_attempt(&changed,original,&initial,Default::default()).unwrap_err().contains("context"));
    let mut invalid=original.clone(); invalid.step=0.0;
    assert!(resolve_implicit_attempt(&Decay,&invalid,&initial,Default::default()).is_err());
    let mut inconsistent=original.clone(); inconsistent.initial_state[0]+=0.1;
    assert!(resolve_implicit_attempt(&Decay,&inconsistent,&initial,Default::default()).unwrap_err().contains("stage mapping"));
    assert!(resolve_implicit_attempt(&Decay,original,&[1.0],Default::default()).is_err());
    let refined=refine_implicit_attempt(&Decay,original,&initial,Default::default(),4).unwrap();
    assert_eq!(refined.len(),4);
    assert!(refined.iter().all(|p|p.solve_succeeded && p.theta==0.5));
    for (k,p) in refined.iter().enumerate() {
        let expected_old=1.01*(0.975_f64/1.025).powi(k as i32);
        assert!((p.initial_state[0]-expected_old).abs()<1e-10);
        assert!((p.stage_state[0]-expected_old/1.025).abs()<1e-10);
        assert!((p.stage_state[1]-p.stage_state[0].powi(2)-p.stage_time).abs()<1e-10);
    }
}

#[test]
fn captured_resolve_distinguishes_two_discrete_roots_with_identical_initial_state() {
    use sim_dynamics::{ImplicitAttempt,attempt_check::{resolve_implicit_attempt,refine_implicit_attempt}};
    struct Quadratic;
    impl System for Quadratic {
        fn dimension(&self)->usize {1}
        fn residual(&self,_:f64,x:&[f64],rate:&[f64],out:&mut[f64]) {
            out[0]=rate[0]-x[0]*x[0];
        }
    }
    // Backward Euler for x'=x^2: x_new - x_old = x_new^2 at h=1.
    // Both roots satisfy the discrete equations; only the smaller root
    // approaches the continuous solution as h tends to zero.
    let mut answers=Vec::new();
    let mut refined_answers=Vec::new();
    for sign in [-1.0,1.0] {
        let x=(1.0+sign*0.6_f64.sqrt())/2.0;
        let rate=x-0.1;
        let mut residual=vec![0.0];
        Quadratic.residual(1.0,&[x],&[rate],&mut residual);
        let point=ImplicitAttempt {start_time:0.0,step:1.0,theta:1.0,stage_time:1.0,
            subdivision_depth:0,branch:false,solve_succeeded:true,committed:None,error:None,
            initial_state:vec![0.1],stage_state:vec![x],stage_rate:vec![rate],residual,
            newton:Default::default(),last_linearization:None};
        let result=resolve_implicit_attempt(&Quadratic,&point,&[0.11],Default::default()).unwrap();
        assert!(result.solve_succeeded);
        assert_eq!(result.initial_state,[0.11]);
        assert!(result.residual[0].abs()<1e-10);
        let expected=(1.0+sign*0.56_f64.sqrt())/2.0;
        assert!((result.stage_state[0]-expected).abs()<1e-9);
        answers.push(result.stage_state[0]);
        let refined=refine_implicit_attempt(&Quadratic,&point,&[0.11],Default::default(),64).unwrap();
        assert_eq!(refined.len(),64);
        assert!(refined.iter().all(|p|p.solve_succeeded));
        assert_eq!(refined[0].initial_state,[0.11]);
        for pair in refined.windows(2) {
            assert_eq!(pair[0].stage_state,pair[1].initial_state);
            assert_eq!(pair[0].start_time+pair[0].step,pair[1].start_time);
        }
        let end=refined.last().unwrap();
        assert_eq!(end.start_time+end.step,1.0);
        assert!((end.stage_state[0]-0.11/0.89).abs()<1e-4);
        refined_answers.push(end.stage_state[0]);
        assert!(refine_implicit_attempt(&Quadratic,&point,&[0.11],Default::default(),0).is_err());
    }
    assert!(answers[1]-answers[0]>0.7);
    assert!((refined_answers[1]-refined_answers[0]).abs()<1e-10);
}

#[test]
fn captured_resolve_keeps_failed_newton_attempt() {
    use sim_dynamics::attempt_check::{resolve_implicit_attempt,refine_implicit_attempt};
    let mut s=Simulation::new(Impossible,Integrator::implicit_midpoint(),vec![0.0]);
    s.set_attempt_audit_limit(1);
    assert!(s.step(0.1).is_err());
    let a=resolve_implicit_attempt(&Impossible,&s.implicit_attempts[0],&[0.0],Default::default()).unwrap();
    assert!(!a.solve_succeeded);
    assert!(a.error.is_some());
    assert_eq!(a.residual,[1.0]);
    let refined=refine_implicit_attempt(&Impossible,&s.implicit_attempts[0],&[0.0],Default::default(),4).unwrap();
    assert_eq!(refined.len(),1);
    assert!(!refined[0].solve_succeeded);
}

#[test]
fn step_doubling_estimates_fine_endpoint_error_with_correct_order_and_dae_mask() {
    use sim_dynamics::attempt_check::check_implicit_step_doubling;
    for (integrator,order) in [(Integrator::BackwardEuler(Default::default()),1),
        (Integrator::implicit_midpoint(),2)] {
        let mut s=Simulation::new(Decay,integrator,vec![1.0,1.0]);
        s.set_attempt_audit_limit(1);
        s.step(0.02).unwrap();
        let p=&s.implicit_attempts[0];
        let report=check_implicit_step_doubling(&Decay,p,&p.initial_state,Default::default()).unwrap();
        assert!(report.coarse_reused);
        assert_eq!(report.coarse.stage_state,p.stage_state);
        assert_eq!(report.method_order,order);
        assert_eq!(report.coarse.initial_state,report.fine[0].initial_state);
        let fine=&report.fine[1];
        let endpoint=if order==1 {fine.stage_state[0]} else {
            fine.initial_state[0]+fine.step*fine.stage_rate[0]
        };
        let expected=if order==1 {1.0/1.01_f64.powi(2)} else {
            (0.995_f64/1.005).powi(2)
        };
        assert!((endpoint-expected).abs()<1e-11);
        let estimated=report.estimated_fine_error.as_ref().unwrap()[0].unwrap();
        let actual=(endpoint-(-0.02_f64).exp()).abs();
        assert!((estimated/actual-1.0).abs()<0.03,"{order}: {estimated} vs {actual}");
        assert!(report.estimated_fine_error.as_ref().unwrap()[1].is_none());
        assert!(report.endpoint_difference.as_ref().unwrap()[1].is_finite());
        let changed=check_implicit_step_doubling(&Decay,p,&[1.01,1.0201],Default::default()).unwrap();
        assert!(!changed.coarse_reused);
        assert_eq!(changed.coarse.initial_state,[1.01,1.0201]);
        assert_eq!(changed.fine[0].initial_state,changed.coarse.initial_state);
        assert!(changed.estimated_fine_error.is_some());
    }
}

#[test]
fn failed_step_doubling_has_no_error_estimate() {
    use sim_dynamics::attempt_check::check_implicit_step_doubling;
    let mut s=Simulation::new(Impossible,Integrator::implicit_midpoint(),vec![0.0]);
    s.set_attempt_audit_limit(1);
    assert!(s.step(0.1).is_err());
    let p=&s.implicit_attempts[0];
    let report=check_implicit_step_doubling(&Impossible,p,&[0.0],Default::default()).unwrap();
    assert!(!report.coarse.solve_succeeded);
    assert!(report.coarse_reused);
    assert_eq!(report.fine.len(),1);
    assert!(!report.fine[0].solve_succeeded);
    assert!(report.endpoint_difference.is_none());
    assert!(report.estimated_fine_error.is_none());
    let mut unsupported=p.clone();unsupported.theta=0.75;
    assert!(check_implicit_step_doubling(&Impossible,&unsupported,&[0.0],Default::default()).is_err());
}

struct LimitedStep {
    h: std::cell::Cell<f64>,
    fail_after: f64,
}
impl System for LimitedStep {
    fn dimension(&self) -> usize { 1 }
    fn begin_step(&self, h: f64) { self.h.set(h); }
    fn residual(&self, t: f64, _: &[f64], rate: &[f64], out: &mut [f64]) {
        out[0] = if self.h.get() > 0.06 || t > self.fail_after { 1.0 } else { rate[0] - 1.0 };
    }
}

#[test]
fn commit_audit_marks_subdivision_leaves_but_not_a_discarded_successful_prefix() {
    for fail_after in [f64::INFINITY, 0.075] {
        let run = |limit| {
            let mut s = Simulation::new(LimitedStep { h: std::cell::Cell::new(0.0), fail_after },
                Integrator::BackwardEuler(Default::default()), vec![0.0]);
            s.set_attempt_audit_limit(limit);
            let succeeded = s.step(0.2).is_ok();
            (s,succeeded)
        };
        let (ordinary,ok) = run(0);
        let (audited,audit_ok) = run(128);
        assert_eq!(ok,audit_ok);
        assert_eq!(ordinary.state,audited.state);
        assert_eq!(ordinary.time,audited.time);
        assert_eq!(ordinary.stats.subdivided_steps,audited.stats.subdivided_steps);
        assert!(audited.implicit_attempts.iter().any(|p| p.solve_succeeded));
        assert!(audited.implicit_attempts.iter().any(|p| !p.solve_succeeded));
        let committed: Vec<_> = audited.implicit_attempts.iter().filter(|p|p.committed==Some(true)).collect();
        if ok {
            assert_eq!(committed.len(),4);
            for (i,p) in committed.iter().enumerate() {
                assert!(p.solve_succeeded);
                assert_eq!(p.step,0.05);
                assert!((p.start_time-i as f64*0.05).abs()<1e-15);
            }
            assert!((audited.state[0]-0.2).abs()<1e-10);
        } else {
            assert!(committed.is_empty());
            assert_eq!(audited.time,0.0);
            assert_eq!(audited.state,[0.0]);
        }
        assert!(audited.implicit_attempts.iter().all(|p|p.committed.is_some()));
    }
}

struct TimedCrossing { fired: std::cell::Cell<bool> }
impl System for TimedCrossing {
    fn dimension(&self) -> usize {1}
    fn residual(&self, _: f64, _: &[f64], rate: &[f64], out: &mut [f64]) {out[0]=rate[0]-1.0;}
    fn guards(&self, t:f64, _: &[f64], out: &mut Vec<f64>) {
        out.push(if self.fired.get() {1.0} else {0.35-t});
    }
    fn jump(&mut self, _:usize, _:f64, _: &mut [f64]) {self.fired.set(true);}
}

#[test]
fn commit_audit_excludes_successful_outer_candidates_and_event_location_probes() {
    let run = |limit| {
        let mut s=Simulation::new(TimedCrossing {fired:std::cell::Cell::new(false)},
            Integrator::BackwardEuler(Default::default()),vec![0.0]);
        s.set_attempt_audit_limit(limit);
        s.step(1.0).unwrap();s
    };
    let ordinary=run(0);let audited=run(128);
    assert_eq!(ordinary.state,audited.state);
    assert_eq!(ordinary.events,audited.events);
    assert_eq!(audited.events.len(),1);
    assert!(audited.implicit_attempts[0].solve_succeeded);
    assert_eq!(audited.implicit_attempts[0].step,1.0);
    assert_eq!(audited.implicit_attempts[0].committed,Some(false));
    let discarded=audited.implicit_attempts.iter().filter(|p|p.solve_succeeded && p.committed==Some(false)).count();
    assert!(discarded>=2,"must exercise both the rejected outer candidate and event search");
    let committed:Vec<_>=audited.implicit_attempts.iter().filter(|p|p.committed==Some(true)).collect();
    assert_eq!(committed.len(),2);
    assert_eq!(committed[0].start_time,0.0);
    assert_eq!(committed[0].start_time+committed[0].step,committed[1].start_time);
    assert_eq!(committed[1].start_time+committed[1].step,1.0);
    assert!((committed.iter().map(|p|p.step*p.stage_rate[0]).sum::<f64>()-1.0).abs()<1e-10);
    let capped=run(1);
    assert_eq!(capped.state,audited.state);
    assert_eq!(capped.implicit_attempts[0].committed,Some(false));
}

#[test]
fn legacy_commit_status_is_unknown_and_diagnostic_resolves_do_not_commit() {
    let mut s=Simulation::new(Decay,Integrator::BackwardEuler(Default::default()),vec![1.0,1.0]);
    s.set_attempt_audit_limit(1);s.step(0.02).unwrap();
    let point=&s.implicit_attempts[0];
    assert_eq!(point.committed,Some(true));
    let mut legacy=serde_json::to_value(point).unwrap();
    legacy.as_object_mut().unwrap().remove("committed");
    let legacy:sim_dynamics::ImplicitAttempt=serde_json::from_value(legacy).unwrap();
    assert_eq!(legacy.committed,None);
    let result=sim_dynamics::attempt_check::resolve_implicit_attempt(&Decay,point,&point.initial_state,Default::default()).unwrap();
    assert!(result.solve_succeeded);
    assert_eq!(result.committed,Some(false));
}

#[test]
fn restoring_a_snapshot_uncommits_superseded_steps_without_erasing_attempts() {
    let mut s=Simulation::new(Decay,Integrator::BackwardEuler(Default::default()),vec![1.0,1.0]);
    s.set_attempt_audit_limit(128);s.step(0.02).unwrap();
    let snapshot=s.snapshot();let prefix=s.implicit_attempts.len();
    s.step(0.02).unwrap();let abandoned=s.implicit_attempts.len();
    assert!(s.implicit_attempts.iter().all(|p|p.committed==Some(true)));
    s.restore(&snapshot).unwrap();
    assert!(s.implicit_attempts[..prefix].iter().all(|p|p.committed==Some(true)));
    assert!(s.implicit_attempts[prefix..].iter().all(|p|p.committed==Some(false)));
    assert_eq!(s.implicit_attempts.len(),abandoned);
    s.step(0.01).unwrap();
    assert!(s.implicit_attempts[abandoned..].iter().all(|p|p.committed==Some(true)));
    let duration:f64=s.implicit_attempts.iter().filter(|p|p.committed==Some(true)).map(|p|p.step).sum();
    assert!((duration-s.time).abs()<1e-15);
}
