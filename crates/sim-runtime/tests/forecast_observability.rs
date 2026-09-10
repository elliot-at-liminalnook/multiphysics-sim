use sim_runtime::{
    motion_data::{LinkMotion, MotionSnapshot},
    motion_forecast::{ForecastRecipe, KinematicReference, MotionAxis, forecast_input},
};

#[test]
fn legacy_body_relative_inputs_cannot_distinguish_height_above_fixed_ground() {
    let recipe = ForecastRecipe {
        expected_cad_sha256: "analytic-observability-case".into(),
        imu_observations:vec![], terrain_relative_links: vec![],
        reference_link: "body".into(),
        axes: ["body", "foot"]
            .into_iter()
            .flat_map(|link| {
                (0..3).map(move |axis| MotionAxis::Link {
                    name: format!("{link}.{axis}"),
                    link: link.into(),
                    axis,
                })
            })
            .collect(),
        physics_context:None, controller_context:None, controller_inputs:vec![], actuator_targets: vec!["motor".into()],
        horizons_steps: vec![10],
        period_s: 0.02,
        reference: KinematicReference::ConstantVelocity,
    };
    let current = MotionSnapshot {
        time_s: 0.02,
        floor_heights_m: Default::default(),
        imu_samples: vec![],
        joint_positions: vec![],
        joint_velocities: vec![],
        poses: [("body", 0.375), ("foot", 0.125)]
            .into_iter()
            .map(|(name, height)| LinkMotion {
                name: name.into(),
                position_m: [0., 0., height],
                velocity_m_s: [0., 0., -1.],
                angular_velocity_rad_s: [0.; 3],
                rotation: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            })
            .collect(),
    };
    let mut previous = current.clone();
    previous.time_s = 0.;
    for p in &mut previous.poses {
        // An airborne, freely falling history; neither case has hit the floor.
        p.position_m[2] += 0.02 - 0.5 * 9.81 * 0.02f64.powi(2);
        p.velocity_m_s[2] += 9.81 * 0.02;
    }
    let mut high_current = current.clone();
    let mut high_previous = previous.clone();
    for p in high_current
        .poses
        .iter_mut()
        .chain(&mut high_previous.poses)
    {
        p.position_m[2] += 0.5;
    }
    let actions = vec![vec![0.]; 10];
    let low = forecast_input(&recipe, &previous, &current, &[0.], &actions).unwrap();
    let high = forecast_input(&recipe, &high_previous, &high_current, &[0.], &actions).unwrap();
    assert_eq!(low, high);

    // Independent point-contact timing on the explicitly fixed z=0 plane.
    // This diagnoses missing information, not full-robot prediction error.
    let impact_time = |height: f64| ((1. + 2. * 9.81 * height).sqrt() - 1.) / 9.81;
    assert!(impact_time(current.poses[1].position_m[2]) < 0.2);
    assert!(impact_time(high_current.poses[1].position_m[2]) > 0.2);

    let mut aware = recipe.clone();
    aware.terrain_relative_links = vec!["body".into(), "foot".into()];
    assert!(
        forecast_input(&aware, &previous, &current, &[0.], &actions)
            .unwrap_err()
            .contains("ground observation")
    );
    let mut low_current = current.clone();
    low_current
        .observe_ground(&aware.terrain_relative_links, |_, _| 0.)
        .unwrap();
    high_current
        .observe_ground(&aware.terrain_relative_links, |_, _| 0.)
        .unwrap();
    let observed_low = forecast_input(&aware, &previous, &low_current, &[0.], &actions).unwrap();
    let observed_high =
        forecast_input(&aware, &high_previous, &high_current, &[0.], &actions).unwrap();
    let offset = aware.future_action_offset() - aware.actuator_targets.len() - 2;
    assert_eq!(&observed_low.0[offset..offset + 2], &[0.375, 0.125]);
    assert_eq!(&observed_high.0[offset..offset + 2], &[0.875, 0.625]);
    assert_eq!(observed_low.1, observed_high.1);
    // Translate the entire world as well: relative observations should agree.
    high_current
        .observe_ground(&aware.terrain_relative_links, |_, _| 0.5)
        .unwrap();
    assert_eq!(
        forecast_input(&aware, &high_previous, &high_current, &[0.], &actions).unwrap(),
        observed_low
    );
    aware.terrain_relative_links.push("foot".into());
    assert!(aware.validate().is_err());
}
