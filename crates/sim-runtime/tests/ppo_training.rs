use sim_runtime::{embedded::Config,environment::Task,session::Scene,ppo_training::*};
use sim_domain_control::{neural::*,ppo::GaussianExploration};
use sim_core::QuantityKind as Q;
use serde_json::json;

#[test]
fn runtime_rollouts_feed_ppo_and_retain_replayable_motion_targets(){
    let mut scene:Scene=serde_json::from_str(include_str!("../../../examples/interactive/pendulum.scene.json")).unwrap();
    scene.robot.source["cad_sha256"]=json!("analytic-ppo");
    scene.options.contact=true;scene.robot.world.floor_z=-10.;
    let mut config:Config=serde_json::from_str(include_str!("../../../examples/interactive/pendulum.policy.json")).unwrap();config.steps=240;
    let task:Task=serde_json::from_value(json!({"version":1,"observation_source":"ideal_runtime_teacher_only","period_s":scene.period_s,
        "observations":[{"name":"angle","source":{"kind":"coordinate_position","coordinate":"joint.pivot"}}],"rewards":[],"termination_bounds":[],"speed":{"body_link":"pendulum"}})).unwrap();
    let actor=Network{version:1,features:vec![Feature{source:"pivot.angle".into(),subtract:None,kind:Q::Angle,center:0.,scale:1.,clip:10.}],
        outputs:vec![Output{target:"pivot.target".into(),kind:Q::Angle,scale:0.1}],layers:vec![Layer{weights:vec![vec![0.]],biases:vec![0.]}]};
    let mut state=PpoState::new(actor).unwrap();let exploration=GaussianExploration{standard_deviation:vec![0.1]};
    let count=(config.step_s*config.steps as f64/task.period_s).round() as usize;let actions=vec![vec![0.4];count];
    let mut rollouts=vec![];
    for seed in [1,2]{
        let r=collect_speed_episode(scene.clone(),config.clone(),task.clone(),&actions,&state.actor,&state.critic,&exploration,seed).unwrap();
        assert!(r.error.is_none()&&r.final_transition.truncated&&!r.final_transition.terminated);
        assert_eq!(r.steps.len(),count);
        for s in &r.steps {
            assert_eq!(s.motion_before.time_s,s.time_s);
            assert!((s.motion_after.time_s-s.time_s-task.period_s).abs()<1e-12);
            assert!(s.motion_after.acceleration_since(&s.motion_before).unwrap().generalized_acceleration.iter().all(|x|x.is_finite()));
        }
        let saved=serde_json::to_value(&r).unwrap();assert!(saved["motion_contract"]["frame_coordinates"].is_array());
        use sim_runtime::motion_forecast::*;
        let recipe=ForecastRecipe{expected_cad_sha256:"analytic-ppo".into(), imu_observations:vec![], terrain_relative_links:vec![],reference_link:"ground".into(),axes:vec![MotionAxis::Joint{name:"joint.pivot".into(),index:0,position_kind:Q::Angle}],
            physics_context:None, controller_context:None, controller_inputs:vec![], actuator_targets:vec!["pivot.target".into()],horizons_steps:vec![1],period_s:0.02,reference:KinematicReference::ConstantVelocity};
        let forecasts=samples_from_speed_rollout(&r,&recipe,0.,0.06).unwrap();assert_eq!(forecasts.len(),2);
        assert_eq!(forecasts[0].inputs[9],r.steps[0].applied_targets_rad[0]);
        assert_eq!(forecasts[0].inputs[10],r.steps[1].applied_targets_rad[0]);
        assert_eq!(forecasts[0].targets[0],r.steps[1].motion_after.joint_positions[0]);
        assert_eq!(forecasts[0].targets[1],r.steps[1].motion_after.joint_velocities[0]);
        let mut bound_recipe=recipe.clone();bound_recipe.physics_context=Some(sim_runtime::physics_context::PhysicsContext::from_recording(&r.recording.runtime).unwrap());
        let bound_samples=samples_from_speed_rollout(&r,&bound_recipe,0.,0.06).unwrap();
        assert_eq!(bound_samples[0].inputs,forecasts[0].inputs);assert_eq!(bound_samples[0].targets,forecasts[0].targets);
        let mut controller_recipe=recipe.clone();controller_recipe.actuator_targets.clear();
        controller_recipe.controller_inputs=scene.controller.as_ref().unwrap().inputs.clone();
        controller_recipe.controller_context=Some(sim_runtime::forecast_actions::ControllerContext::from_runtime(&r.recording.runtime.scene,&r.recording.runtime.config).unwrap());
        controller_recipe.physics_context=Some(sim_runtime::physics_context::PhysicsContext::from_recording(&r.recording.runtime).unwrap());
        let controller_samples=samples_from_speed_rollout(&r,&controller_recipe,0.,0.06).unwrap();
        assert_eq!(controller_samples[0].inputs[9],0.4);assert_eq!(controller_samples[0].inputs[10],0.4);
        assert_eq!(controller_samples[0].targets,forecasts[0].targets);
        let mut terrain_recipe=recipe.clone();terrain_recipe.terrain_relative_links=vec!["pendulum".into()];
        let terrain_forecasts=samples_from_speed_rollout(&r,&terrain_recipe,0.,0.06).unwrap();
        let terrain_offset=terrain_recipe.future_action_offset()-2;
        let first=&r.steps[1].motion_before.poses.iter().find(|p|p.name=="pendulum").unwrap();
        assert_eq!(terrain_forecasts[0].inputs[terrain_offset],first.position_m[2]-scene.robot.world.floor_z);
        assert_eq!(terrain_forecasts[0].inputs[terrain_recipe.future_action_offset()],r.steps[1].applied_targets_rad[0]);
        assert_eq!(terrain_forecasts[0].targets,forecasts[0].targets);
        let mut broken=r.clone();broken.steps[1].motion_before.joint_positions[0]+=0.1;
        assert!(samples_from_speed_rollout(&broken,&recipe,0.,0.06).unwrap_err().contains("discontinuous"));
        broken=r.clone();broken.error=Some("numerical failure".into());
        assert!(samples_from_speed_rollout(&broken,&recipe,0.,0.06).unwrap_err().contains("numerical errors"));
        let mut replay=sim_runtime::environment::EmbeddedEnvironment::new(r.recording.runtime.scene.clone(),r.recording.runtime.config.clone(),r.recording.task.clone(),r.recording.runtime.seed).unwrap();
        for a in &actions{replay.step(a).unwrap();}
        assert_eq!(replay.transition(),&r.final_transition);
        rollouts.push(r);
    }
    let settings=PpoSettings{epochs:2,batch_size:2,actor_learning_rate:0.001,critic_learning_rate:0.01,clip_ratio:0.2,maximum_gradient_norm:1.,fall_multiplier_learning_rate:10.,shuffle_seed:Some(73)};
    let original=state.clone();let mut repeated=state.clone();
    let mut stale=state.clone();stale.critic.layers.last_mut().unwrap().biases[0]=1.;
    assert!(update_speed_policy(&mut stale,&rollouts,&exploration,&settings).unwrap_err().contains("critic state"));
    let before=state.actor.parameters();let report=update_speed_policy(&mut state,&rollouts,&exploration,&settings).unwrap();
    update_speed_policy(&mut repeated,&rollouts,&exploration,&settings).unwrap();assert_eq!(state.actor.parameters(),repeated.actor.parameters());
    let mut other=original;let mut changed=settings.clone();changed.shuffle_seed=Some(74);
    update_speed_policy(&mut other,&rollouts,&exploration,&changed).unwrap();assert_ne!(state.actor.parameters(),other.actor.parameters());
    assert!(report.exact_policy_kl.iter().all(|k|k.is_finite()&&*k>=0.));
    assert_eq!(report.episodes,2);assert_eq!(report.fall_rate,0.);assert_eq!(report.fall_multiplier,0.);
    assert_ne!(before,state.actor.parameters());assert!(report.actor_losses.iter().all(|l|l.loss.is_finite()));
    let updated=state.actor.parameters();assert!(update_speed_policy(&mut state,&rollouts,&exploration,&settings).unwrap_err().contains("current frozen"));
    assert_eq!(updated,state.actor.parameters());
    let mut restored:PpoState=serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();restored.validate().unwrap();
    let followup=collect_speed_episode(scene.clone(),config.clone(),task.clone(),&actions,&state.actor,&state.critic,&exploration,3).unwrap();
    update_speed_policy(&mut state,&[followup.clone()],&exploration,&settings).unwrap();
    update_speed_policy(&mut restored,&[followup],&exploration,&settings).unwrap();
    assert_eq!(serde_json::to_value(&state).unwrap(),serde_json::to_value(&restored).unwrap());
    assert_eq!(state.updates,2);
    let mut broken=state.clone();broken.critic.outputs[0].scale=2.;assert!(broken.validate().is_err());
    broken=state.clone();broken.fall_multiplier=f64::NAN;assert!(broken.validate().is_err());
    rollouts[0].error=Some("solver failure".into());
    assert!(update_speed_policy(&mut state,&rollouts,&exploration,&settings).unwrap_err().contains("numerical failures"));
}

#[test]
fn acceleration_estimates_use_real_sample_intervals_and_reject_ambiguous_history(){
    use sim_runtime::motion_data::MotionSnapshot;
    let f=|t:f64,v:f64| json!({"time_s":t,"joint_positions":[0.],"joint_velocities":[v],"poses":[{
        "name":"body","position_m":[0.,0.,1.],"rotation":[[1.,0.,0.],[0.,1.,0.],[0.,0.,1.]],"velocity_m_s":[v,0.,0.],"angular_velocity_rad_s":[0.,v,0.]}]});
    let a=MotionSnapshot::from_frame(&f(1.,2.)).unwrap();let b=MotionSnapshot::from_frame(&f(1.25,3.)).unwrap();
    let x=b.acceleration_since(&a).unwrap();assert_eq!(x.generalized_acceleration,vec![4.]);
    assert_eq!(x.link_linear_m_s2,vec![[4.,0.,0.]]);assert_eq!(x.link_angular_rad_s2,vec![[0.,4.,0.]]);
    assert!(a.acceleration_since(&a).is_err());assert!(a.acceleration_since(&b).is_err());
}
