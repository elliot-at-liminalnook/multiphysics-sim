use sim_core::{Channel, Contract, Coupler, QuantityKind};
use sim_script::{RhaiController, Sources, parameter_map};

#[test]
fn rhai_heading_uses_shared_bounds_and_preserves_commands_after_rejection() {
    let source = Sources::single("heading.rhai", r#"
fn control(t,s,a,state) {
    a.delta=heading_correction(s.target,s.actual,0.0,s.rate,parameters());
    #{commands:a,state:state}
}"#);
    let mut c = RhaiController::new(source, parameter_map(&serde_json::json!({
        "position_gain":2,"velocity_damping_s":0.1,"maximum_correction_rad":0.5})).unwrap()).unwrap();
    c.open(&Contract {element:"test".into(),period:0.02,sensors:vec![
        Channel{name:"target".into(),kind:QuantityKind::Angle},
        Channel{name:"actual".into(),kind:QuantityKind::Angle},
        Channel{name:"rate".into(),kind:QuantityKind::AngularVelocity}],
        actuators:vec![Channel{name:"delta".into(),kind:QuantityKind::Angle}]}).unwrap();
    let mut out=[0.]; c.sample(0.,&[0.1,0.,1.],&mut out).unwrap();
    assert!((out[0]-0.1).abs()<1e-14); let before=out;
    assert!(c.sample(0.02,&[0.,0.,f64::INFINITY],&mut out).is_err()); assert_eq!(out,before);
    c.sample(0.02,&[-1.,0.,0.],&mut out).unwrap(); assert_eq!(out,[-0.5]);
}
