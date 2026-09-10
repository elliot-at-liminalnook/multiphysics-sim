use sim_runtime::{motion_forecast::*,motion_data::MotionSnapshot};
use sim_core::QuantityKind as Q;
use serde_json::json;
fn recipe()->ForecastRecipe{ForecastRecipe{expected_cad_sha256:"analytic".into(), imu_observations:vec![], terrain_relative_links:vec![],reference_link:"body".into(),axes:vec![MotionAxis::Joint{name:"joint".into(),index:0,position_kind:Q::Angle},MotionAxis::Link{name:"body.x".into(),link:"body".into(),axis:0}],physics_context:None, controller_context:None, controller_inputs:vec![], actuator_targets:vec!["motor".into()],horizons_steps:vec![1,5,10],period_s:0.02,reference:KinematicReference::ConstantAcceleration}}
fn snapshot(t:f64,p:f64,v:f64)->MotionSnapshot{MotionSnapshot::from_frame(&json!({"time_s":t,"joint_positions":[p],"joint_velocities":[v],"poses":[{"name":"body","position_m":[p,0.,1.],"velocity_m_s":[v,0.,0.],"angular_velocity_rad_s":[0.,0.,0.],"rotation":[[1.,0.,0.],[0.,1.,0.],[0.,0.,1.]]}]})).unwrap()}
#[test]
fn constant_acceleration_reference_is_exact_and_translation_invariant(){
    let r=recipe();let previous=snapshot(0.,10.,2.);let current=snapshot(0.02,10.0406,2.06);
    let (input,prior)=forecast_input(&r,&previous,&current,&[0.2],&vec![vec![0.3];10]).unwrap();
    assert!((input[2]-3.).abs()<1e-12);assert!((input[5]-3.).abs()<1e-12);
    for (i,h) in r.horizons_steps.iter().enumerate(){let t=*h as f64*0.02;
        assert!((prior[i*6]-10.0406-2.06*t-1.5*t*t).abs()<1e-12);
        assert!((prior[i*6+3]-2.06*t-1.5*t*t).abs()<1e-12);}
    let training=vec![ForecastSample{time_s:0.02,inputs:input.clone(),prior:prior.clone(),targets:prior.clone()}];
    let model=TrajectoryForecaster::initialize(r,&training,4,7).unwrap();assert_eq!(model.predict(&input,&prior).unwrap(),prior);
    let mut shifted_previous=previous.clone();let mut shifted_current=current.clone();
    shifted_previous.poses[0].position_m[0]+=100.;shifted_current.poses[0].position_m[0]+=100.;
    assert_eq!(forecast_input(&recipe(),&shifted_previous,&shifted_current,&[0.2],&vec![vec![0.3];10]).unwrap(),(input,prior));
    let mut cv=recipe();cv.reference=KinematicReference::ConstantVelocity;
    let (inputs,prior)=forecast_input(&cv,&previous,&current,&[0.2],&vec![vec![0.3];10]).unwrap();
    assert!((inputs[2]-3.).abs()<1e-12); // acceleration remains a model input
    assert_eq!(prior[2],0.);assert!((prior[0]-current.joint_positions[0]-2.06*0.02).abs()<1e-12);
}

#[test]
fn multi_step_labels_use_applied_future_actions_and_stay_inside_split_windows(){
    let r=recipe();let mut frames=vec![];
    for i in 0..=12{let t=i as f64*0.02;let motion=snapshot(t,1.5*t*t,3.*t);
        let mut frame=serde_json::to_value(motion).unwrap();frame["policy"]=json!({"time_s":if i==0 {0.} else {(i-1) as f64*0.02},"targets":{"motor":i as f64*0.01}});frames.push(frame);}
    let mut capture=json!({"error":null,"frames":frames,"metadata":{"frame_coordinates":[{"name":"joint","position_unit":"rad"}]},
        "recording":{"scene":{"robot":{"source":{"cad_sha256":"analytic"},"gravity":[0.,0.,-9.81]}}}});
    let samples=samples_from_capture(&capture,&r,0.,0.24).unwrap();assert_eq!(samples.len(),2);
    assert!((samples[0].inputs[12]-0.01).abs()<1e-12); // previous target
    assert!((samples[0].inputs[13]-0.02).abs()<1e-12); // first future target
    for s in &samples{for (a,b) in s.targets.iter().zip(&s.prior){assert!((a-b).abs()<1e-12);}}
    assert_eq!(samples_from_capture(&capture,&r,0.,0.22).unwrap().len(),1);
    capture["frames"][2]["policy"]["time_s"]=serde_json::Value::Null;
    assert!(samples_from_capture(&capture,&r,0.,0.24).unwrap_err().contains("timing"));
}
#[test]
fn forecast_residual_learns_action_dependence_on_held_out_actions(){
    let r=recipe();let previous=snapshot(0.,0.,0.);let current=snapshot(0.02,0.,0.);
    let sample=|action:f64|{
        let(inputs,prior)=forecast_input(&r,&previous,&current,&[0.],&vec![vec![action];10]).unwrap();let mut targets=prior.clone();
        for (i,h) in r.horizons_steps.iter().enumerate(){let t=*h as f64*0.02;for axis in 0..2{targets[i*6+axis*3]+=0.5*action*t*t;targets[i*6+axis*3+1]+=action*t;targets[i*6+axis*3+2]+=action;}}
        ForecastSample{time_s:0.02,inputs,prior,targets}
    };
    let training=(-10..=10).map(|i|sample(i as f64/10.)).collect::<Vec<_>>();let validation=vec![sample(0.345),sample(-0.625)];
    let mut model=TrajectoryForecaster::initialize(r.clone(),&training,8,7).unwrap();let initial=model.normalized_loss(&validation).unwrap();
    model.fit(&training,100,7,0.01).unwrap();assert!(model.normalized_loss(&validation).unwrap()<initial*0.02);
    assert!(forecast_input(&r,&current,&current,&[0.],&vec![vec![0.];10]).is_err());
}

#[test]
fn physical_forecast_action_gradient_includes_both_normalization_scales() {
    let r=recipe();let (inputs,prior)=forecast_input(&r,&snapshot(0.,0.,0.),&snapshot(0.02,0.,0.),&[0.2],&vec![vec![0.3];10]).unwrap();
    let sample=ForecastSample{time_s:0.02,inputs:inputs.clone(),prior:prior.clone(),targets:prior.clone()};
    let mut m=TrajectoryForecaster::initialize(r,&[sample],4,7).unwrap();
    for f in &mut m.network.features{f.scale=0.4;}
    for o in &mut m.network.outputs{o.scale=0.07;}
    for (i,row) in m.network.layers.last_mut().unwrap().weights.iter_mut().enumerate(){for (j,v)in row.iter_mut().enumerate(){*v=(i+j+1)as f64*0.03;}}
    let mut derivative=vec![0.;prior.len()];derivative[prior.len()-3]=1.;derivative[prior.len()-2]=-0.4;
    let g=m.input_gradient(&inputs,&derivative).unwrap();
    let objective=|x:&[f64]|m.predict(x,&prior).unwrap().iter().zip(&derivative).map(|(a,b)|a*b).sum::<f64>();
    for i in inputs.len()-10..inputs.len(){let mut plus=inputs.clone();let mut minus=inputs.clone();plus[i]+=1e-6;minus[i]-=1e-6;
        assert!((g[i]-(objective(&plus)-objective(&minus))/2e-6).abs()<1e-9);}
    assert!(m.input_gradient(&inputs,&[]).is_err());
}
