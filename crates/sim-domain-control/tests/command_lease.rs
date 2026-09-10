use sim_domain_control::command_lease::{CommandLease, CommandLeaseConfig};
fn lease() -> CommandLease { CommandLease::new(CommandLeaseConfig { period_s:0.02, timeout_s:0.25 }).unwrap() }
#[test]
fn duplicate_and_reordered_packets_expire_and_only_a_new_packet_recovers() {
    let c=lease();let mut s=c.initial();assert!(!s.fresh);
    s=c.update(s.sequence,s.age_s,7.).unwrap();assert!(s.fresh);
    for i in 1..=13 { s=c.update(s.sequence,s.age_s,if i%2==0 {6.}else{7.}).unwrap();assert_eq!(s.fresh,i<13); }
    assert_eq!(s.sequence,7.);assert_eq!(s.age_s,0.25);
    for _ in 0..1000 { s=c.update(s.sequence,s.age_s,7.).unwrap();assert!(!s.fresh); }
    s=c.update(s.sequence,s.age_s,8.).unwrap();assert!(s.fresh);assert_eq!(s.age_s,0.);
    for bad in [f64::NAN,f64::INFINITY,-1.,1.5,9_007_199_254_740_992.] { assert!(c.update(s.sequence,s.age_s,bad).is_err()); }
}
#[test]
fn registry_schedule_and_units_use_the_same_freshness_kernel() {
    use sim_core::{BehaviorRegistry,Context,View,QuantityKind,signal_out};
    let mut registry=BehaviorRegistry::default();sim_domain_control::elements::register(&mut registry).unwrap();
    let d=registry.get(&"control.command_lease".into()).unwrap();
    assert_eq!(d.ports[2].schema,signal_out("age",QuantityKind::Time).schema);
    let parameters=[("period_s".into(),0.02),("timeout_s".into(),0.25)].into_iter().collect();
    d.validate_parameters(&parameters).unwrap();let mut b=d.equations.unwrap()(&parameters).unwrap();
    let mut states=vec![0.,-1.,0.25];let mut expected=lease().initial();
    for i in 0..40 {
        let old=states.clone();let input=[if i<20 {1.}else{2.}];let t=i as f64*0.02;
        let v=View {time:t,states:&old,offsets:&[0],rate_map:&[],across:&[],across_rates:&[],signals_in:&input};
        let mut events=vec![];b.scheduled_events(&v,&mut events);assert_eq!(events,vec![(0,t)]);
        b.jump(0,&v,&mut states);expected=lease().update(expected.sequence,expected.age_s,input[0]).unwrap();
        let(mut residual,mut signals)=([0.;3],[0.;2]);
        b.residual(&mut Context::new(t,&states,&[0.;3],&[0],&[],&[],&[],&input,&mut residual,&mut [],&mut signals));
        assert_eq!(signals,[if expected.fresh {1.}else{0.},expected.age_s]);
    }
    assert!(CommandLease::new(CommandLeaseConfig {period_s:0.02,timeout_s:0.01}).is_err());
}
