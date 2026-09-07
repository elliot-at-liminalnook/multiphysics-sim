use sim_domain_control::support_preload::{SupportPreload, SupportPreloadConfig};
fn controller(period_s: f64) -> SupportPreload {
    SupportPreload::new(SupportPreloadConfig {
        period_s,
        integral_gain_m_per_ns: 0.005,
        maximum_extension_m: 0.004,
    })
    .unwrap()
}
#[test]
fn load_deficit_extends_excess_retracts_and_saturation_does_not_wind_up() {
    let c = controller(0.02);
    let x = c.update(0., 10., 0., true).unwrap();
    assert!((x - 0.001).abs() < 1e-14);
    let mut saturated = x;
    for _ in 0..100 {
        saturated = c.update(saturated, 100., 0., true).unwrap();
    }
    assert_eq!(saturated, 0.004);
    assert!((c.update(saturated, 0., 10., true).unwrap() - 0.003).abs() < 1e-14);
    assert_eq!(c.update(x, 0., 100., true).unwrap(), 0.);
    assert_eq!(c.update(x, 10., 0., false).unwrap(), 0.);
}
#[test]
fn constant_force_error_integrates_on_simulation_time() {
    let integrate = |period| {
        let c = controller(period);
        let mut x = 0.;
        for _ in 0..(0.2 / period) as usize {
            x = c.update(x, 2., 1., true).unwrap();
        }
        x
    };
    assert!((integrate(0.02) - 0.001).abs() < 1e-14);
    assert!((integrate(0.01) - integrate(0.02)).abs() < 1e-14);
}
#[test]
fn invalid_loads_and_scales_are_rejected() {
    let c = controller(0.02);
    for x in [f64::NAN, f64::INFINITY, -1., 0.005] {
        assert!(c.update(x, 1., 0., true).is_err());
    }
    for load in [f64::NAN, f64::INFINITY, -1.] {
        assert!(c.update(0., load, 0., true).is_err());
        assert!(c.update(0., 1., load, true).is_err());
    }
    assert!(
        SupportPreload::new(SupportPreloadConfig {
            period_s: 0.,
            integral_gain_m_per_ns: 1.,
            maximum_extension_m: 1.
        })
        .is_err()
    );
}

#[test]
fn registered_component_matches_direct_feedback_and_replays_from_state() {
    use sim_core::{BehaviorRegistry, Context, View};
    use sim_domain_control::support_preload::SUPPORT_PRELOAD;
    let mut registry = BehaviorRegistry::default();
    sim_domain_control::elements::register(&mut registry).unwrap();
    let descriptor = registry.get(&SUPPORT_PRELOAD.into()).unwrap();
    let parameters = [
        ("period_s", 0.02),
        ("integral_gain_m_per_ns", 0.005),
        ("maximum_extension_m", 0.004),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v))
    .collect();
    descriptor.validate_parameters(&parameters).unwrap();
    let mut component = descriptor.equations.unwrap()(&parameters).unwrap();
    let c = controller(0.02);
    let mut state = vec![0., 0.];
    let mut expected = 0.;
    for i in 0..30 {
        let old = state.clone();
        let measured = if i < 15 { 0. } else { 20. };
        let enabled = i != 10;
        let input = [10., measured, if enabled { 1. } else { 0. }];
        let time = i as f64 * 0.02;
        let view = View {
            time,
            states: &old,
            offsets: &[0],
            rate_map: &[],
            across: &[],
            across_rates: &[],
            signals_in: &input,
        };
        let mut events = vec![];
        component.scheduled_events(&view, &mut events);
        assert_eq!(events, vec![(0, time)]);
        component.jump(0, &view, &mut state);
        let mut replay = old.clone();
        component.jump(0, &view, &mut replay);
        assert_eq!(state, replay);
        expected = c.update(expected, 10., measured, enabled).unwrap();
        let mut residual = [0.; 2];
        let mut signals = [0.];
        component.residual(&mut Context::new(
            time,
            &state,
            &[0.; 2],
            &[0],
            &[],
            &[],
            &[],
            &input,
            &mut residual,
            &mut [],
            &mut signals,
        ));
        assert_eq!(signals[0], expected);
        assert_eq!(residual, [0.; 2]);
    }
}
