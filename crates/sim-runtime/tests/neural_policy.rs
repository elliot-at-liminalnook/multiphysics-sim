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
fn gaussian_policy_records_raw_likelihood_and_replays_saturated_actions() {
    use sim_domain_control::ppo::{GaussianDecision,GaussianExploration};
    let(s,mut c,t)=recipe();
    let exploration=GaussianExploration{standard_deviation:vec![10.]};
    let policy=c.policy.as_mut().unwrap();
    policy.neural_residual=Some(network(0.,1.));policy.neural_command_saturation=true;
    policy.neural_exploration=Some(exploration.clone());
    let mut e=EmbeddedEnvironment::new(s.clone(),c.clone(),t.clone(),71).unwrap();
    let transition=e.step(&[0.3]).unwrap();let frame=e.frame().unwrap();
    let d:GaussianDecision=serde_json::from_value(frame["policy"]["neural_decision"].clone()).unwrap();
    assert_eq!(d.log_probability,exploration.log_probability(&d.means,&d.raw_actions).unwrap());
    let requested=frame["policy"]["neural_command_saturation"]["requested_targets_rad"][0].as_f64().unwrap();
    assert!((requested-0.3-d.raw_actions[0]).abs()<1e-12);
    assert_eq!(frame["policy"]["neural_command_saturation"]["saturated_commands"],1);
    let record=e.episode_recording();let(mut replay,actions)=e.prepare_replay(record).unwrap();
    assert_eq!(replay.step(&actions[0]).unwrap(),transition);
    assert_eq!(replay.frame().unwrap()["policy"]["neural_decision"],frame["policy"]["neural_decision"]);
    e.reset(72).unwrap();e.step(&[0.3]).unwrap();assert_ne!(e.frame().unwrap()["policy"]["neural_decision"],frame["policy"]["neural_decision"]);
    c.policy.as_mut().unwrap().neural_command_saturation=false;
    assert!(EmbeddedEnvironment::new(s,c,t,0).err().unwrap().contains("saturation"));
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

#[test]
fn explicit_neural_saturation_preserves_bounds_and_zero_policy_physics() {
    let (s, mut c, t) = recipe();
    c.steps *= 2;
    let mut baseline = EmbeddedEnvironment::new(s.clone(), c.clone(), t.clone(), 7).unwrap();
    c.policy.as_mut().unwrap().neural_residual = Some(network(0.0, 100.0));
    c.policy.as_mut().unwrap().neural_command_saturation = true;
    let mut zero = EmbeddedEnvironment::new(s.clone(), c.clone(), t.clone(), 7).unwrap();
    for action in [0.3, -0.2, 0.4] {
        assert_eq!(baseline.step(&[action]).unwrap(), zero.step(&[action]).unwrap());
        let a = baseline.frame().unwrap();
        let b = zero.frame().unwrap();
        for key in ["poses", "joint_positions", "joint_velocities", "servo_targets_rad", "contacts"] {
            assert_eq!(a[key], b[key], "zero residual changed physical field {key}");
        }
    }
    c.policy.as_mut().unwrap().neural_residual = Some(network(10.0, 100.0));
    let upper = c.policy.as_ref().unwrap().target_bounds_rad["joint.pivot"][1];
    let mut saturated = EmbeddedEnvironment::new(s, c, t, 7).unwrap();
    saturated.step(&[0.3]).unwrap();
    let frame = saturated.frame().unwrap();
    assert_eq!(frame["policy"]["targets"]["pivot.target"].as_f64().unwrap(), upper);
    assert_eq!(frame["policy"]["neural_command_saturation"]["saturated_commands"], 1);
    assert!(frame["policy"]["neural_command_saturation"]["requested_targets_rad"][0].as_f64().unwrap() > upper);
}

#[test]
fn neural_saturation_requires_a_network_and_cannot_hide_nonfinite_commands() {
    let (s, mut c, t) = recipe();
    c.policy.as_mut().unwrap().neural_command_saturation = true;
    assert!(EmbeddedEnvironment::new(s.clone(), c.clone(), t.clone(), 7).err().unwrap().contains("explicit neural policy"));
    c.policy.as_mut().unwrap().neural_residual = Some(network(10.0, 1e308));
    let mut invalid_scene = s;
    invalid_scene.controller.as_mut().unwrap().sources = serde_json::from_value(serde_json::json!({
        "entry":"nonfinite.rhai", "files":{"nonfinite.rhai":"fn control(t, sensors, commands, state) { commands[\"pivot.target\"] = 1.0e308; #{commands:commands,state:state} }"}
    })).unwrap();
    let mut invalid = EmbeddedEnvironment::new(invalid_scene, c, t, 7).unwrap();
    assert!(invalid.step(&[0.3]).unwrap_err().contains("nonfinite combined neural command"));
}
