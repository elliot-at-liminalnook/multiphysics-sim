use sim_core::{Channel, Contract, Coupler, QuantityKind};
use sim_script::{RhaiController, Sources, parameter_map};

#[test]
fn rhai_uses_shared_damping_and_rejects_invalid_inputs_transactionally() {
    let source = Sources::single("damping.rhai", r#"
fn control(t,s,a,state) {
    a.delta=load_damping_displacement(s.velocity,s.load,parameters());
    #{commands:a,state:state}
}"#);
    let mut c = RhaiController::new(source, parameter_map(&serde_json::json!({
        "velocity_damping_s":0.2,"full_support_force_n":2})).unwrap()).unwrap();
    c.open(&Contract {element:"test".into(),period:0.02,sensors:vec![
        Channel{name:"velocity".into(),kind:QuantityKind::LinearVelocity},
        Channel{name:"load".into(),kind:QuantityKind::Force}],
        actuators:vec![Channel{name:"delta".into(),kind:QuantityKind::Length}]}).unwrap();
    let mut out=[0.]; c.sample(0.,&[0.03,1.],&mut out).unwrap(); assert_eq!(out,[-0.003]);
    assert!(c.sample(0.02,&[0.03,-1.],&mut out).is_err()); assert_eq!(out,[-0.003]);
    c.sample(0.02,&[-0.03,2.],&mut out).unwrap(); assert_eq!(out,[0.006]);
}
