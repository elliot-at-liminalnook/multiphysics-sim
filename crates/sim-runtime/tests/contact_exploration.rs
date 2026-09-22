use sim_domain_control::{motion_primitives::ContactTemplate,motion_parameters::Values};
#[test]
fn six_physical_motion_controls_are_independently_materialized(){
    let template:ContactTemplate=serde_json::from_str(include_str!("../../../examples/full-robot/measured-actuator-integration/gait-search-comparison-2026-09-19/contact-template.json")).unwrap();
    let values:Values=serde_json::from_str(include_str!("../../../examples/full-robot/measured-actuator-integration/gait-search-comparison-2026-09-19/baseline-values.json")).unwrap();
    let baseline=template.materialize(&values).unwrap();
    for p in &template.space.parameters {
        let mut candidate=values.clone();candidate.insert(p.name.clone(),(p.bounds[0]+p.bounds[1])*0.5);
        if candidate[&p.name]==values[&p.name]{candidate.insert(p.name.clone(),p.bounds[0]+0.4*(p.bounds[1]-p.bounds[0]));}
        let motion=template.materialize(&candidate).unwrap();
        match p.name.as_str(){
            "stride_m"=>assert!((motion.displacement_world_m.iter().map(|v|v*v).sum::<f64>().sqrt()-candidate[&p.name]).abs()<1e-12),
            "cadence_scale"=>{assert!((motion.period_s-0.9/candidate[&p.name]).abs()<1e-12);assert_eq!(motion.body.keyframes.last().unwrap().time_s,motion.period_s);},
            "clearance_m"=>assert!(motion.feet.iter().all(|f|f.swing_offset_world_m[2]==candidate[&p.name])),
            "stance_fraction"=>assert!(motion.feet.iter().all(|f|f.stance_fraction==candidate[&p.name])),
            "body_height_offset_m"=>assert!(motion.body.keyframes.iter().all(|k|k.values[2]==candidate[&p.name])),
            phase=>{let i:usize=phase.trim_start_matches("phase_").parse().unwrap();assert_eq!(motion.feet[i].phase_offset,candidate[phase]);},
        }
        assert_ne!(serde_json::to_value(&baseline).unwrap(),serde_json::to_value(&motion).unwrap());
    }
}
