use sim_core::{Channel,Contract,Coupler,QuantityKind};
use sim_script::{RhaiController,Sources,parameter_map};
#[test]
fn script_packet_loss_and_replay_match_rust_and_invalid_input_rolls_back() {
    let source=Sources::single("lease.rhai",r#"
fn control(t,s,a,state) {
 if !state.contains("lease") { state.lease=#{sequence:-1.0,age_s:0.25}; }
 state.lease=command_lease_update(state.lease.sequence,state.lease.age_s,s.sequence,parameters());
 a.fresh=if state.lease.fresh {1.0}else{0.0};
 #{commands:a,state:state}
}"#);
    let make=||{
        let mut c=RhaiController::new(source.clone(),parameter_map(&serde_json::json!({"period_s":0.02,"timeout_s":0.25})).unwrap()).unwrap();
        c.open(&Contract {element:"lease-test".into(),period:0.02,sensors:vec![Channel{name:"sequence".into(),kind:QuantityKind::Dimensionless}],actuators:vec![Channel{name:"fresh".into(),kind:QuantityKind::Dimensionless}]}).unwrap();c
    };
    let (mut c,mut replay)=(make(),make());let(mut out,mut again)=([0.],[0.]);
    for i in 0..40 {
        let input=[if i<20 {1.}else{2.}];c.sample(i as f64*0.02,&input,&mut out).unwrap();
        replay.sample(i as f64*0.02,&input,&mut again).unwrap();assert_eq!(out,again);
        assert_eq!(out[0],if i%20<13 {1.}else{0.});
    }
    assert!(c.sample(0.8,&[1.5],&mut out).is_err());
    c.sample(0.8,&[3.],&mut out).unwrap();replay.sample(0.8,&[3.],&mut again).unwrap();assert_eq!(out,again);
}
