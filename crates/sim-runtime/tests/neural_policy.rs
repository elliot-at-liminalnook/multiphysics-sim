use sim_domain_control::neural::*;
use sim_core::QuantityKind as Q;
use sim_runtime::{embedded::Config, environment::{EmbeddedEnvironment, Task}, session::Scene};

fn recipe() -> (Scene,Config,Task) {
    let s=serde_json::from_str(include_str!("../../../examples/interactive/pendulum.scene.json")).unwrap();
    let mut c:Config=serde_json::from_str(include_str!("../../../examples/interactive/pendulum.policy.json")).unwrap();
    c.steps=160;
    let t=serde_json::from_str(include_str!("../../../examples/interactive/pendulum.environment.json")).unwrap();
    (s,c,t)
}
fn network(bias:f64,scale:f64)->Network {
    Network {version:1,features:vec![Feature{source:"pivot.angle".into(),subtract:None,kind:Q::Angle,center:0.0,scale:1.0,clip:5.0}],outputs:vec![Output{target:"pivot.target".into(),kind:Q::Angle,scale}],layers:vec![Layer{weights:vec![vec![0.0]],biases:vec![bias]}]}
}
#[test]
fn neural_corrections_are_held_replayed_and_still_subject_to_motor_bounds() {
    let(s,mut c,mut t)=recipe();
    c.policy.as_mut().unwrap().neural_residual=Some(network(0.2,0.01));
    t.observations.push(sim_runtime::environment::Observation{name:"correction".into(),source:sim_runtime::environment::ObservationSource::NeuralCorrection{actuator:"pivot.target".into()}});
    let mut e=EmbeddedEnvironment::new(s.clone(),c.clone(),t.clone(),7).unwrap();
    assert_eq!(*e.transition().observations.last().unwrap(),0.0);
    let first=e.step(&[0.3]).unwrap();
    assert!((first.observations.last().unwrap()-0.01*0.2f64.tanh()).abs()<1e-15);
    let frame=e.frame().unwrap();
    assert!((frame["policy"]["targets"]["pivot.target"].as_f64().unwrap()-0.3-0.01*0.2f64.tanh()).abs()<1e-15);
    let record=e.episode_recording();
    let(mut replay,actions)=e.prepare_replay(record.clone()).unwrap();
    assert_eq!(replay.step(&actions[0]).unwrap(),first);
    let mut changed=record;changed.runtime.config.policy.as_mut().unwrap().neural_residual=Some(network(0.3,0.01));
    assert!(e.prepare_replay(changed).is_err());
    assert_eq!(*e.reset(7).unwrap().observations.last().unwrap(),0.0);
    c.policy.as_mut().unwrap().neural_residual=Some(network(10.0,100.0));
    let mut invalid=EmbeddedEnvironment::new(s,c,t,7).unwrap();
    assert!(invalid.step(&[0.3]).unwrap_err().contains("command bounds"));
    assert!(invalid.error().is_some());
}
