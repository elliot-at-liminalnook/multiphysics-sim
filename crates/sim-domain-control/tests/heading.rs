use sim_domain_control::heading::{HeadingFeedback, HeadingFeedbackConfig, HEADING_FEEDBACK,
    shortest_angle_error, world_z_heading};

#[test]
fn shortest_angle_rate_damping_and_caps_have_independent_analytic_oracles() {
    let c = HeadingFeedback::new(HeadingFeedbackConfig {position_gain: 2., velocity_damping_s: 0.1,
        maximum_correction_rad: 0.5}).unwrap();
    let pi = std::f64::consts::PI;
    assert!((shortest_angle_error(-pi + 0.1, pi - 0.1).unwrap() - 0.2).abs() < 1e-14);
    assert!((c.correction(-pi + 0.1, pi - 0.1, 0., 1.).unwrap() - 0.3).abs() < 1e-14);
    assert_eq!(c.correction(0., 0., 0., 1.).unwrap(), -0.1);
    assert_eq!(c.correction(1., 0., 0., 0.).unwrap(), 0.5);
    assert_eq!(c.correction(-1., 0., 0., 0.).unwrap(), -0.5);
    assert_eq!(c.correction(0.4, 0.4, 2., 2.).unwrap(), 0.);
    assert!(c.correction(f64::NAN, 0., 0., 0.).is_err());
    assert!(c.correction(0., 0., 0., f64::INFINITY).is_err());
    for (gain, damping, cap) in [(-1., 0., 1.), (0., -1., 1.), (1., 0., 0.), (1., 0., f64::INFINITY)] {
        assert!(HeadingFeedback::new(HeadingFeedbackConfig {position_gain:gain,velocity_damping_s:damping,maximum_correction_rad:cap}).is_err());
    }
}

#[test]
fn tilted_heading_rate_uses_the_projected_forward_direction() {
    let [angle, rate] = world_z_heading([0.6, 0., 0.8], [0.3, 0., 2.]).unwrap();
    assert_eq!(angle, 0.);
    assert!((rate - 1.6).abs() < 1e-14); // wz - wx*tan(pitch), not simply wz.
    let a: f64 = 0.7;
    let [angle, rotated_rate] = world_z_heading([0.6*a.cos(),0.6*a.sin(),0.8],
        [0.3*a.cos(),0.3*a.sin(),2.]).unwrap();
    assert!((angle - a).abs() < 1e-14 && (rotated_rate - rate).abs() < 1e-14);
    assert!(world_z_heading([0.,0.,1.], [0.;3]).is_err());
    assert!(world_z_heading([1.,0.,0.], [f64::NAN,0.,0.]).is_err());
}

#[test]
fn registry_exposes_angle_ports_units_and_the_shared_capped_result() {
    use sim_core::{BehaviorRegistry, Context, QuantityKind, signal_in, signal_out};
    let mut registry = BehaviorRegistry::default();
    sim_domain_control::elements::register(&mut registry).unwrap();
    let d = registry.get(&HEADING_FEEDBACK.into()).unwrap();
    assert_eq!(d.ports[0].schema, signal_in("target", QuantityKind::Angle).schema);
    assert_eq!(d.ports[3].schema, signal_in("actual_rate", QuantityKind::AngularVelocity).schema);
    assert_eq!(d.ports[4].schema, signal_out("correction", QuantityKind::Angle).schema);
    let p = [("position_gain".into(), 2.), ("velocity_damping_s".into(),0.1),
        ("maximum_correction_rad".into(),0.5)].into_iter().collect();
    d.validate_parameters(&p).unwrap();
    let c = d.equations.unwrap()(&p).unwrap(); assert!(c.states().is_empty());
    let mut out = [0.];
    c.residual(&mut Context::new(0., &[], &[], &[], &[], &[], &[], &[0.1,0.,0.,1.], &mut [], &mut [], &mut out));
    assert!((out[0] - 0.1).abs() < 1e-14);
    let units = d.parameters.as_ref().unwrap().iter().map(|p| (p.name.as_str(),p.unit.as_str()))
        .collect::<std::collections::BTreeMap<_,_>>();
    assert_eq!(units["position_gain"],"1"); assert_eq!(units["velocity_damping_s"],"s");
    assert_eq!(units["maximum_correction_rad"],"rad");
}
