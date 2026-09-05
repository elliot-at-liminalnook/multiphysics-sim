mod common;
use common::*;
use sim_domain_robot::{Articulated, Options};
use sim_domain_robot::model::*;
use std::sync::Arc;

fn geared_pair() -> PhysicalModel {
    let mut m=empty_model();m.gravity=[0.0;3];
    m.links.push(box_link("base",[0.1;3],1.0,[0.0;3],true));
    m.links.push(box_link("input",[0.02;3],0.03,[0.2,0.0,0.0],false));
    m.links.push(box_link("output",[0.04;3],0.1,[-0.2,0.0,0.0],false));
    m.joints.push(joint("input","revolute",Some("base"),"input",[0.2,0.0,0.0],[0.0,0.0,1.0]));
    m.joints.push(joint("output","revolute",Some("base"),"output",[-0.2,0.0,0.0],[0.0,1.0,0.0]));
    for j in &mut m.joints {j.physics.friction=Friction::default();}
    m.transmissions.push(Transmission{name:"worm".into(),driver_joint:"input".into(),driven_joint:"output".into(),ratio:5.0});m
}

#[test]
fn different_axis_gears_conserve_virtual_power_and_enforce_ratio() {
    let a=Articulated::new(Arc::new(geared_pair()),&Options{flex:false,contact:false,..Options::default()}).unwrap();
    let t=&a.transmissions[0];
    let mut g=a.generalized(vec![0.0;a.state_count],vec![0.0;a.state_count],&[],vec![]);
    g.q[t.driver]=0.5;g.q[t.driven]=0.1;g.qd[t.driver]=2.0;g.qd[t.driven]=0.4;
    g.states[t.lambda_state]=3.0;
    let e=a.evaluate_with(&g,false);
    let passive:Vec<_>=e.joints.iter().flat_map(|j|j.tau_passive.iter().copied()).collect();
    assert!((passive[t.driver]-3.0).abs()<1e-10);
    assert!((passive[t.driven]+15.0).abs()<1e-10);
    assert!((passive[t.driver]*g.qd[t.driver]+passive[t.driven]*g.qd[t.driven]).abs()<1e-10);
    assert!((e.loop_rows[0]-a.loop_angular_cfm*3.0).abs()<1e-10);
    g.q[t.driven]+=0.01;
    assert!((a.evaluate_with(&g,false).loop_rows[0]+0.05*a.loop_alpha.powi(2)-a.loop_angular_cfm*3.0).abs()<1e-8);
}

#[test]
fn coupled_rotation_integrates_without_ratio_drift() {
    let mut rig=Rig::new(geared_pair(),&[("contact",0.0),("initial.joint.input.speed",1.0),("initial.joint.output.speed",0.2)],midpoint());
    for _ in 0..50 {rig.runtime.advance(1e-3,1e-3).unwrap();}
    let g=rig.generalized();let t=&rig.art.transmissions[0];
    assert!((g.q[t.driver]-5.0*g.q[t.driven]).abs()<1e-7);
    assert!(g.q[t.driver].abs()>0.04);
}

#[test]
fn invalid_and_redundant_couplings_are_rejected() {
    let mut m=geared_pair();m.transmissions[0].ratio=0.0;
    assert!(Articulated::new(Arc::new(m),&Options::default()).is_err());
    let mut m=geared_pair();m.transmissions.push(m.transmissions[0].clone());
    assert!(Articulated::new(Arc::new(m),&Options::default()).is_err());
}
