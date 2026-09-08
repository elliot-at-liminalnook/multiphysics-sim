use sim_domain_control::load_damping::{LoadDamping, LoadDampingConfig, LOAD_DAMPING};

#[test]
fn signed_velocity_is_opposed_only_to_the_extent_of_support() {
    let c = LoadDamping::new(LoadDampingConfig {velocity_damping_s: 0.2, full_support_force_n: 2.}).unwrap();
    assert_eq!(c.displacement(0.03, 0.).unwrap(), 0.);
    assert_eq!(c.displacement(0.03, 1.).unwrap(), -0.003);
    assert_eq!(c.displacement(-0.03, 2.).unwrap(), 0.006);
    assert_eq!(c.displacement(-0.03, 20.).unwrap(), 0.006);
    for (v, f) in [(f64::NAN, 1.), (0., f64::INFINITY), (0., -1.)] {
        assert!(c.displacement(v, f).is_err());
    }
    for (t, f) in [(-1., 1.), (0., 0.), (f64::NAN, 1.)] {
        assert!(LoadDamping::new(LoadDampingConfig {velocity_damping_s:t, full_support_force_n:f}).is_err());
    }
    let zero = LoadDamping::new(LoadDampingConfig {velocity_damping_s:0., full_support_force_n:1.}).unwrap();
    assert_eq!(zero.displacement(10., 2.).unwrap(), 0.);
}

#[test]
fn registry_exposes_the_same_validation_units_and_memoryless_correction() {
    use sim_core::{BehaviorRegistry, Context, QuantityKind, signal_in, signal_out};
    let mut registry = BehaviorRegistry::default();
    sim_domain_control::elements::register(&mut registry).unwrap();
    let d = registry.get(&LOAD_DAMPING.into()).unwrap();
    assert_eq!(d.ports[0].schema, signal_in("velocity", QuantityKind::LinearVelocity).schema);
    assert_eq!(d.ports[2].schema, signal_out("displacement", QuantityKind::Length).schema);
    let p = [("velocity_damping_s".into(), 0.2), ("full_support_force_n".into(), 2.)].into_iter().collect();
    d.validate_parameters(&p).unwrap();
    let c = d.equations.unwrap()(&p).unwrap();
    assert!(c.states().is_empty());
    let mut out = [0.];
    c.residual(&mut Context::new(0., &[], &[], &[], &[], &[], &[], &[0.03, 1.], &mut [], &mut [], &mut out));
    assert_eq!(out, [-0.003]);
    let units = d.parameters.as_ref().unwrap().iter().map(|p| (p.name.as_str(), p.unit.as_str())).collect::<std::collections::BTreeMap<_,_>>();
    assert_eq!(units["velocity_damping_s"], "s");
    assert_eq!(units["full_support_force_n"], "N");
}
