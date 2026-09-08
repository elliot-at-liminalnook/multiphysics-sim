use sim_domain_control::angle_integral::{AngleIntegral, AngleIntegralConfig, ANGLE_INTEGRAL};
fn config() -> AngleIntegralConfig {
    AngleIntegralConfig { period_s: 0.02, integral_gain_per_s: 0.5, leak_rate_per_s: 0.,
        maximum_bias_rad: 0.04, maximum_rate_rad_s: 0.01 }
}

#[test]
fn bias_saturates_without_windup_and_releases_at_bounded_rate() {
    let c = AngleIntegral::new(config()).unwrap();
    let mut bias = 0.;
    for _ in 0..250 { let next = c.update(bias, 100., true).unwrap(); assert!((next-bias).abs() <= 0.0002+1e-15); bias=next; }
    assert_eq!(bias, 0.04);
    assert!((c.update(bias, -100., true).unwrap() - 0.0398).abs() < 1e-15);
    assert_eq!(c.update(bias, 0., true).unwrap(), bias);
    for _ in 0..250 { bias = c.update(bias, 100., false).unwrap(); }
    assert_eq!(bias, 0.);
    for _ in 0..250 { bias = c.update(bias, -100., true).unwrap(); }
    assert_eq!(bias, -0.04);
    let mut leaky = config(); leaky.leak_rate_per_s = 5.; leaky.maximum_rate_rad_s = 1.;
    assert!((AngleIntegral::new(leaky).unwrap().update(0.01, 0., true).unwrap() - 0.01/1.1).abs() < 1e-15);
}

#[test]
fn invalid_updates_leave_caller_state_unchanged_and_bad_scales_fail() {
    let c = AngleIntegral::new(config()).unwrap(); let bias: f64 = 0.02;
    for error in [f64::NAN, f64::INFINITY] {
        assert!(c.update(bias, error, true).is_err()); assert_eq!(bias.to_bits(), 0.02_f64.to_bits());
    }
    for prior in [-0.041, 0.041, f64::NAN, f64::INFINITY] { assert!(c.update(prior, 0., false).is_err()); }
    for change in [|c: &mut AngleIntegralConfig| c.period_s=0.,
        |c: &mut AngleIntegralConfig| c.integral_gain_per_s=-1.,
        |c: &mut AngleIntegralConfig| c.leak_rate_per_s=-1.,
        |c: &mut AngleIntegralConfig| c.maximum_bias_rad=f64::NAN,
        |c: &mut AngleIntegralConfig| c.maximum_rate_rad_s=0.,
        |c: &mut AngleIntegralConfig| { c.period_s=1e308; c.maximum_rate_rad_s=1e308; }] {
        let mut p=config(); change(&mut p); assert!(AngleIntegral::new(p).is_err());
    }
    let mut zero=config(); zero.integral_gain_per_s=0.;
    assert_eq!(AngleIntegral::new(zero).unwrap().update(0., 1., true).unwrap(), 0.);
}

#[test]
fn sampled_bias_rejects_constant_disturbance_in_an_analytic_first_order_plant() {
    // Synthetic angular lag with exact held-input propagation, not a robot or
    // actuator calibration. The proportional-only steady error is 0.01/2.5.
    let simulate=|period: f64, gain: f64| {
        let mut p=config(); p.period_s=period; p.integral_gain_per_s=gain;
        let c=AngleIntegral::new(p).unwrap(); let a=(-period/0.02).exp();
        let (mut y,mut bias)=(0.,0.);
        for _ in 0..(30./period).round() as usize {
            bias=c.update(bias,-y,true).unwrap();
            let command=-1.5*y+bias;
            y=a*y+(1.-a)*(command+0.01);
            assert!(y.abs()<0.01);
        }
        (y,bias)
    };
    assert!((simulate(0.02,0.).0-0.004).abs()<1e-12);
    let coarse=simulate(0.02,0.5); let fine=simulate(0.01,0.5);
    assert!(coarse.0.abs()<2e-5 && (coarse.1+0.01).abs()<5e-5);
    assert!((coarse.0-fine.0).abs()<2e-6);
}

#[test]
fn registry_ports_units_events_and_state_match_direct_updates() {
    use sim_core::{BehaviorRegistry, Context, View, signal_in, signal_out, QuantityKind};
    let mut registry=BehaviorRegistry::default(); sim_domain_control::elements::register(&mut registry).unwrap();
    let d=registry.get(&ANGLE_INTEGRAL.into()).unwrap();
    assert_eq!(d.ports[0].schema, signal_in("error",QuantityKind::Angle).schema);
    assert_eq!(d.ports[2].schema, signal_out("bias",QuantityKind::Angle).schema);
    let units=d.parameters.as_ref().unwrap().iter().map(|p|(p.name.as_str(),p.unit.as_str())).collect::<std::collections::BTreeMap<_,_>>();
    assert_eq!(units["maximum_rate_rad_s"],"rad/s"); assert_eq!(units["integral_gain_per_s"],"1/s");
    let parameters=[("period_s",0.02),("integral_gain_per_s",0.5),("leak_rate_per_s",0.),
        ("maximum_bias_rad",0.04),("maximum_rate_rad_s",0.01)].into_iter().map(|(k,v)|(k.into(),v)).collect();
    d.validate_parameters(&parameters).unwrap(); let mut component=d.equations.unwrap()(&parameters).unwrap();
    let c=AngleIntegral::new(config()).unwrap(); let mut state=vec![0.,0.]; let mut expected=0.;
    for i in 0..40 {
        let old=state.clone(); let enabled=i<30; let input=[if i<20 {0.1}else{-0.1},if enabled {1.}else{0.}];
        let t=i as f64*0.02;
        let v=View{time:t,states:&old,offsets:&[0],rate_map:&[],across:&[],across_rates:&[],signals_in:&input};
        let mut events=vec![]; component.scheduled_events(&v,&mut events); assert_eq!(events,vec![(0,t)]);
        component.jump(0,&v,&mut state); let mut replay=old.clone(); component.jump(0,&v,&mut replay); assert_eq!(state,replay);
        expected=c.update(expected,input[0],enabled).unwrap();
        let (mut residual,mut signals)=([0.;2],[0.]);
        component.residual(&mut Context::new(t,&state,&[0.;2],&[0],&[],&[],&[],&input,&mut residual,&mut [],&mut signals));
        assert_eq!(signals[0],expected); assert_eq!(residual,[0.;2]);
    }
    // A nonfinite registry gate must surface as invalid state, not silently
    // become a disabled update. The equation runtime rejects nonfinite jumps.
    let old=state.clone();
    let v=View{time:0.8,states:&old,offsets:&[0],rate_map:&[],across:&[],across_rates:&[],signals_in:&[0.1,f64::NAN]};
    component.jump(0,&v,&mut state); assert!(state[1].is_nan());
}
