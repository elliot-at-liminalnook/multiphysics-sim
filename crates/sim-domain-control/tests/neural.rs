use sim_core::{Channel, QuantityKind as Q};
use sim_domain_control::{neural::*, policy_search::*};

fn network() -> Network {
    Network {version:1,features:vec![Feature {source:"angle".into(),subtract:Some("target".into()),kind:Q::Angle,center:0.1,scale:0.2,clip:2.0}],
        outputs:vec![Output {target:"correction".into(),kind:Q::Angle,scale:0.01}],
        layers:vec![Layer {weights:vec![vec![0.5]],biases:vec![0.2]}]}
}
fn channels() -> (Vec<Channel>,Vec<Channel>) {
    (vec![Channel{name:"target".into(),kind:Q::Angle},Channel{name:"angle".into(),kind:Q::Angle}],
     vec![Channel{name:"unchanged".into(),kind:Q::Angle},Channel{name:"correction".into(),kind:Q::Angle}])
}
#[test]
fn typed_named_normalization_and_bounded_inference() {
    let (s,a)=channels();let n=network().bind(&s,&a).unwrap();
    let v=n.sample(&[0.3,0.6]).unwrap();
    assert_eq!(v[0],0.0);assert!((v[1]-0.01*0.7f64.tanh()).abs()<1e-15);
    assert!((n.sample(&[0.0,100.0]).unwrap()[1]-0.01*1.2f64.tanh()).abs()<1e-15);
    assert!(n.sample(&[f64::NAN,0.0]).is_err());assert!(n.sample(&[0.0]).is_err());
    let mut wrong=s;wrong[0].kind=Q::Torque;assert!(network().bind(&wrong,&a).is_err());
    let mut wrong=network();wrong.layers[0].weights[0].push(0.0);assert!(wrong.validate().is_err());
    let mut wrong=network();wrong.layers[0].weights[0][0]=f64::MAX;wrong.layers[0].biases[0]=f64::MAX;
    assert!(wrong.bind(&channels().0,&a).unwrap().sample(&[0.0,100.0]).is_err());
}
#[test]
fn paired_search_improves_a_known_objective_and_rejects_failed_trials() {
    let objective=|n:&Network| Ok(-n.parameters().iter().map(|p|(p-0.8).powi(2)).sum::<f64>());
    let c=SearchConfig{seed:42,iterations:40,perturbation:0.1};
    let a=search(network(),&c,objective).unwrap();let b=search(network(),&c,objective).unwrap();
    assert!(a.best_score>a.initial_score);assert_eq!(a.policy.parameters(),b.policy.parameters());
    assert_eq!(a.trials.len(),80);
    let mut calls=0;let r=search(network(),&c,|_| {calls+=1;if calls==1 {Ok(0.0)} else {Err("numerical failure".into())}}).unwrap();
    assert_eq!(r.policy.parameters(),network().parameters());assert!(r.trials.iter().all(|t|t.error.is_some()&&!t.accepted&&t.score.is_none()));
}
