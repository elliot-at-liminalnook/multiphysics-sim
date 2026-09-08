use sim_core::{Channel, Contract, Coupler, QuantityKind};
use sim_script::{RhaiController, Sources, parameter_map};
use sim_domain_control::angle_integral::{AngleIntegral, AngleIntegralConfig};
fn config() -> AngleIntegralConfig {
    AngleIntegralConfig { period_s:0.02, integral_gain_per_s:0.5, leak_rate_per_s:0.,
        maximum_bias_rad:0.04, maximum_rate_rad_s:0.01 }
}
fn controller() -> RhaiController {
    let source=Sources::single("integral.rhai",r#"
fn control(t,s,a,state) {
    let old=if state.contains("bias") { state.bias } else { 0.0 };
    // Mutate the local state first to check error rollback at the script boundary.
    state.bias=99.0;
    state.bias=angle_integral_update(old,s.error,s.enabled>=1.0,parameters());
    a.bias=state.bias;
    #{commands:a,state:state}
}"#);
    let mut c=RhaiController::new(source,parameter_map(&serde_json::to_value(config()).unwrap()).unwrap()).unwrap();
    c.open(&Contract {element:"test".into(),period:0.02,
        sensors:vec![Channel{name:"error".into(),kind:QuantityKind::Angle},Channel{name:"enabled".into(),kind:QuantityKind::Dimensionless}],
        actuators:vec![Channel{name:"bias".into(),kind:QuantityKind::Angle}]}).unwrap(); c
}
#[test]
fn rhai_matches_registry_kernel_and_fresh_replay() {
    let kernel=AngleIntegral::new(config()).unwrap(); let mut expected=0.;
    let mut c=controller(); let mut replay=controller(); let (mut out,mut again)=([0.],[0.]);
    for i in 0..50 {
        let input=[if i<20 {0.1}else{-0.1},if i<35 {1.}else{0.}];
        expected=kernel.update(expected,input[0],input[1]>=1.).unwrap();
        c.sample(i as f64*0.02,&input,&mut out).unwrap(); replay.sample(i as f64*0.02,&input,&mut again).unwrap();
        assert_eq!(out[0].to_bits(),expected.to_bits()); assert_eq!(out,again);
    }
}
#[test]
fn failed_native_update_does_not_commit_script_state_or_commands() {
    let mut c=controller(); let mut out=[0.]; c.sample(0.,&[0.1,1.],&mut out).unwrap(); let previous=out;
    assert!(c.sample(0.02,&[f64::NAN,1.],&mut out).is_err()); assert_eq!(out,previous);
    c.sample(0.02,&[0.1,1.],&mut out).unwrap();
    let expected=AngleIntegral::new(config()).unwrap().update(previous[0],0.1,true).unwrap();
    assert_eq!(out[0].to_bits(),expected.to_bits());
}
