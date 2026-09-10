use sim_runtime::{
    motion_data::LinkMotion, predictive_control::forecast_net_progress,
    speed_task::net_displacement,
};
fn pose() -> LinkMotion {
    LinkMotion {
        name: "body".into(),
        position_m: [3., 4., 1.],
        velocity_m_s: [0.; 3],
        angular_velocity_rad_s: [0.; 3],
        rotation: [[0., -1., 0.], [1., 0., 0.], [0., 0., 1.]],
    }
}
#[test]
fn predicted_net_progress_uses_world_endpoint_and_exact_rotated_derivatives() {
    let p = pose();
    let local = [1., 2., 0.5];
    let (score, g) = forecast_net_progress([0.; 2], &p, local).unwrap();
    assert!((score - (26f64.sqrt() - 5.)).abs() < 1e-14);
    assert!((g[0] - 5. / 26f64.sqrt()).abs() < 1e-14);
    assert!((g[1] + 1. / 26f64.sqrt()).abs() < 1e-14);
    assert_eq!(g[2], 0.);
    for i in 0..3 {
        let mut plus = local;
        let mut minus = local;
        plus[i] += 1e-6;
        minus[i] -= 1e-6;
        let central = (forecast_net_progress([0.; 2], &p, plus).unwrap().0
            - forecast_net_progress([0.; 2], &p, minus).unwrap().0)
            / 2e-6;
        assert!((g[i] - central).abs() < 1e-9);
    }
    let mut translated = p.clone();
    translated.position_m[0] += 100.;
    translated.position_m[1] -= 20.;
    let (s, d) = forecast_net_progress([100., -20.], &translated, local).unwrap();
    assert!((s - score).abs() < 1e-14);
    assert_eq!(d, g);
    assert!(forecast_net_progress([0.; 2], &p, [-1., 0., 0.]).unwrap().0 < 0.);
    assert_eq!(net_displacement([0.; 2], [0.; 2]).unwrap().distance_m, 0.);
    assert_eq!(
        net_displacement([0.; 2], [0.; 2])
            .unwrap()
            .position_gradient,
        [0.; 2]
    );
    assert!(net_displacement([f64::NAN, 0.], [0.; 2]).is_err());
    let mut invalid = p;
    invalid.rotation[0][0] = 2.;
    assert!(forecast_net_progress([0.; 2], &invalid, local).is_err());
}
