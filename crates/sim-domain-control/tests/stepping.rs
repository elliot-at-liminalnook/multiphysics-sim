use sim_domain_control::stepping::*;
fn sequence() -> StepSequence {
    StepSequence::new(
        StepSequenceConfig {
            period_s: 0.02,
            initial_hold_s: 0.2,
            phase_durations_s: [0.5, 0.4, 0.4, 0.5, 0.2],
            maximum_wait_s: 1.,
            qualification_s: 0.04,
            lift_m: 0.005,
            order: vec![0, 2, 1, 3],
            support_offsets_m: vec![
                [0., 0.016, 0.],
                [-0.018, 0., -0.003],
                [0., -0.016, 0.],
                [0.009, 0., 0.],
            ],
            stance_offsets_m: vec![[0.01, 0.], [0., 0.], [0.01, 0.], [0., 0.]],
            command_postures: vec![],
            restart_order_on_translation_reversal: false,
            update_command_before_lift: false,
            swing_body_advance_fraction: 0.,
            whole_swing_horizontal_motion: false,
            horizontal_swing_finish_fraction: 1.,
            maximum_speed_m_s: 0.01,
            maximum_yaw_rate_rad_s: 0.1,
        },
        [0.; 3],
        0.,
        vec![
            [0., -0.3, -0.4],
            [0.3, 0., -0.4],
            [0., 0.3, -0.4],
            [-0.3, 0., -0.4],
        ],
    )
    .unwrap()
}
#[test]
fn constant_command_reproduces_crawl_geometry_and_keeps_stance_feet_planted() {
    let mut s = sequence();
    let mut previous = None;
    for i in 0..=810 {
        let r = s
            .sample(i as f64 * 0.02, [0.00125, 0., 0.], true, true)
            .unwrap();
        if let Some(p) = &previous {
            let p: &StepReference = p;
            for foot in 0..4 {
                if r.foot != Some(foot) && p.foot != Some(foot) {
                    assert_eq!(r.feet_world_m[foot], p.feet_world_m[foot]);
                }
            }
            assert!(
                r.body_world_m
                    .iter()
                    .zip(p.body_world_m)
                    .all(|(a, b)| (a - b).abs() < 0.002)
            );
        }
        if i == 410 {
            assert!((r.body_world_m[0] - 0.01).abs() < 1e-12);
            assert!((r.feet_world_m[0][0] - 0.02).abs() < 1e-12);
            assert!((r.feet_world_m[1][0] - 0.31).abs() < 1e-12);
        }
        if i == 810 {
            assert!((r.body_world_m[0] - 0.02).abs() < 1e-12);
        }
        previous = Some(r);
    }
}
#[test]
fn stop_finishes_transfer_then_holds_and_resume_is_continuous() {
    let mut s = sequence();
    let mut last: Option<StepReference> = None;
    for i in 0..=150 {
        let command = if i < 45 || i >= 140 {
            [0.00125, 0., 0.]
        } else {
            [0.; 3]
        };
        let r = s.sample(i as f64 * 0.02, command, true, true).unwrap();
        if i == 109 {
            assert_eq!(r.phase, StepPhase::Settle)
        }
        if (110..140).contains(&i) {
            assert_eq!(r.phase, StepPhase::Idle);
            assert!((r.body_world_m[0] - 0.0025).abs() < 1e-12);
            assert_eq!(r.latched_twist, [0.; 3]);
        }
        if i == 140 {
            assert_eq!(r.phase, StepPhase::Shift);
            assert_eq!(r.body_world_m, last.as_ref().unwrap().body_world_m);
            assert_eq!(r.feet_world_m, last.as_ref().unwrap().feet_world_m);
        }
        last = Some(r);
    }
}
#[test]
fn support_and_landing_guards_wait_and_timeout_without_advancing_state() {
    let mut s = sequence();
    let mut held = None;
    for i in 0..=85 {
        let before = format!("{s:?}");
        let result = s.sample(i as f64 * 0.02, [0.00125, 0., 0.], false, false);
        if i == 85 {
            assert!(result.is_err());
            assert_eq!(format!("{s:?}"), before);
            break;
        }
        let r = result.unwrap();
        if i >= 35 {
            assert_eq!(r.phase, StepPhase::Shift);
            assert!(r.waiting);
            if let Some(p) = &held {
                assert_eq!(&r.body_world_m, p)
            }
            held = Some(r.body_world_m);
        }
    }
    let before = format!("{s:?}");
    assert!(s.sample(99., [0.; 3], true, true).is_err());
    assert_eq!(format!("{s:?}"), before);
}
#[test]
fn planar_twist_matches_circular_arc_and_small_angle_limit() {
    let r = advance_planar([0.; 3], [1., 0., 1.], std::f64::consts::FRAC_PI_2);
    assert!((r[0] - 1.).abs() < 1e-12 && (r[1] - 1.).abs() < 1e-12);
    let r = advance_planar([0., 0., std::f64::consts::FRAC_PI_2], [1., 0., 0.], 2.);
    assert!(r[0].abs() < 1e-12 && (r[1] - 2.).abs() < 1e-12);
    let r = advance_planar([0.; 3], [1., 0., 1e-10], 2.);
    assert!((r[0] - 2.).abs() < 1e-12 && (r[1] - 2e-10).abs() < 1e-20);
}

#[test]
fn lowering_waits_for_landing_before_body_transfer() {
    let mut s = sequence();
    for i in 0..=100 {
        let r = s
            .sample(i as f64 * 0.02, [0.00125, 0., 0.], true, i >= 99)
            .unwrap();
        if (75..100).contains(&i) {
            assert_eq!(r.phase, StepPhase::Lower);
            assert!(r.waiting);
            assert_eq!(r.progress, 1.);
        }
        if i == 100 {
            assert_eq!(r.phase, StepPhase::Return);
            assert!(!r.waiting);
            assert_eq!(r.progress, 0.);
        }
    }
}

#[test]
fn velocity_posture_interpolates_and_latches_without_moving_planted_feet() {
    let base = sequence();
    let mut config = base.config().clone();
    let mut reverse = CommandPosture {
        forward_speed_m_s: -0.01,
        support_offsets_m: config.support_offsets_m.clone(),
        stance_offsets_m: config.stance_offsets_m.clone(),
    };
    reverse.support_offsets_m[0] = [0.02, 0.03, 0.];
    reverse.stance_offsets_m[0] = [0.02, 0.];
    config.command_postures = vec![
        reverse,
        CommandPosture {
            forward_speed_m_s: 0.,
            support_offsets_m: config.support_offsets_m.clone(),
            stance_offsets_m: config.stance_offsets_m.clone(),
        },
    ];
    let feet = vec![
        [0., -0.3, -0.4],
        [0.3, 0., -0.4],
        [0., 0.3, -0.4],
        [-0.3, 0., -0.4],
    ];
    let mut seq = StepSequence::new(config.clone(), [0.; 3], 0., feet.clone()).unwrap();
    let mut saw_landing = false;
    for i in 0..105 {
        // Reverse is latched at t=.2. Changing the request at .4 must not
        // change this transfer's landing or its support displacement.
        let command = if i < 20 { -0.005 } else { 0.005 };
        let r = seq
            .sample(i as f64 * 0.02, [command, 0., 0.], true, true)
            .unwrap();
        for foot in 1..4 {
            assert_eq!(r.feet_world_m[foot], feet[foot]);
        }
        if r.phase == StepPhase::Raise {
            assert!((r.body_world_m[0] - 0.01).abs() < 1e-12);
            assert!((r.body_world_m[1] - 0.023).abs() < 1e-12);
            assert_eq!(r.latched_twist[0], -0.005);
        }
        if r.phase == StepPhase::Return {
            assert!((r.feet_world_m[0][0] - (-0.025)).abs() < 1e-12);
            saw_landing = true;
        }
    }
    assert!(saw_landing);
    config.command_postures.reverse();
    assert!(
        StepSequence::new(config, [0.; 3], 0., feet)
            .unwrap_err()
            .contains("sorted")
    );
}

#[test]
fn reversal_restarts_order_at_transfer_boundary_and_preserves_planted_positions() {
    let mut config = sequence().config().clone();
    config.restart_order_on_translation_reversal = true;
    let feet = vec![
        [0., -0.3, -0.4],
        [0.3, 0., -0.4],
        [0., 0.3, -0.4],
        [-0.3, 0., -0.4],
    ];
    let mut s = StepSequence::new(config, [0.; 3], 0., feet).unwrap();
    let mut previous: Option<StepReference> = None;
    let mut starts = vec![];
    for i in 0..900 {
        let command = if i < 420 { 0.00125 } else { -0.00125 };
        let r = s
            .sample(i as f64 * 0.02, [command, 0., 0.], true, true)
            .unwrap();
        if r.phase == StepPhase::Shift && r.progress == 0. {
            starts.push((r.step, r.foot.unwrap(), r.latched_twist[0]));
            if let Some(p) = &previous {
                assert_eq!(r.feet_world_m, p.feet_world_m);
                assert_eq!(r.body_world_m, p.body_world_m);
            }
        }
        previous = Some(r);
    }
    assert_eq!(
        starts.iter().take(9).map(|x| x.1).collect::<Vec<_>>(),
        vec![0, 2, 1, 3, 0, 0, 2, 1, 3]
    );
    assert_eq!(starts[4].2, 0.00125);
    assert_eq!(starts[5].2, -0.00125);
}

fn responsive_sequence() -> StepSequence {
    let mut config = sequence().config().clone();
    config.update_command_before_lift = true;
    config.restart_order_on_translation_reversal = true;
    StepSequence::new(
        config,
        [0.; 3],
        0.,
        vec![
            [0., -0.3, -0.4],
            [0.3, 0., -0.4],
            [0., 0.3, -0.4],
            [-0.3, 0., -0.4],
        ],
    )
    .unwrap()
}

#[test]
fn prelift_updates_preserve_constant_command_reference_exactly() {
    let mut old = sequence();
    let mut new = responsive_sequence();
    for i in 0..500 {
        let args = [0.00125, 0., 0.002];
        let a = old.sample(i as f64 * 0.02, args, true, true).unwrap();
        let b = new.sample(i as f64 * 0.02, args, true, true).unwrap();
        assert_eq!(a, b);
    }
}

#[test]
fn stop_before_lift_recenters_without_moving_feet_or_counting_a_transfer() {
    let mut s = responsive_sequence();
    let initial = s.sample(0., [0.00125, 0., 0.], true, true).unwrap();
    let mut previous = initial.clone();
    for i in 1..=90 {
        let command = if i < 20 || i >= 80 {
            [0.00125, 0., 0.]
        } else {
            [0.; 3]
        };
        let r = s.sample(i as f64 * 0.02, command, true, true).unwrap();
        assert_eq!(r.step, 0, "an unstarted swing is not a completed transfer");
        assert_eq!(r.feet_world_m, initial.feet_world_m);
        if i == 35 {
            assert_eq!(r.phase, StepPhase::Recenter);
            assert_eq!(r.foot, None);
            assert_eq!(r.latched_twist, [0.; 3]);
            assert_eq!(r.body_world_m, [0., 0.016, 0.]);
        }
        if (70..80).contains(&i) {
            assert_eq!(r.phase, StepPhase::Idle);
            assert_eq!(r.body_world_m, [0.; 3]);
        }
        if i == 80 {
            assert_eq!(r.phase, StepPhase::Shift);
            assert_eq!(r.foot, Some(0));
            assert_eq!(r.body_world_m, previous.body_world_m);
        }
        assert!((r.body_world_m[1] - previous.body_world_m[1]).abs() < 0.0013);
        previous = r;
    }
}

#[test]
fn airborne_stop_preserves_the_committed_landing() {
    let mut old = sequence();
    let mut new = responsive_sequence();
    for i in 0..150 {
        let command = if i < 45 { [0.00125, 0., 0.] } else { [0.; 3] };
        let a = old.sample(i as f64 * 0.02, command, true, true).unwrap();
        let b = new.sample(i as f64 * 0.02, command, true, true).unwrap();
        assert_eq!(a, b);
    }
}

#[test]
fn prelift_reversal_keeps_the_committed_transfer_and_then_restarts_order() {
    let mut s = responsive_sequence();
    let mut starts = vec![];
    for i in 0..250 {
        let command = if i < 120 {
            [0.00125, 0., 0.]
        } else {
            [-0.00125, 0., 0.]
        };
        let r = s.sample(i as f64 * 0.02, command, true, true).unwrap();
        if r.phase == StepPhase::Shift && r.progress == 0. {
            starts.push(r.foot);
        }
        if i == 135 {
            assert_eq!(r.phase, StepPhase::Raise);
            assert_eq!(r.foot, Some(2), "do not change which foot was unloaded");
            assert_eq!(r.latched_twist[0], 0.00125);
            assert_eq!(r.body_world_m, [0.0025, -0.016, 0.]);
        }
        if i == 175 {
            assert_eq!(r.phase, StepPhase::Return);
            assert!(
                r.feet_world_m[2][0] > 0.01,
                "finish the landing selected with the committed support shift"
            );
        }
    }
    assert_eq!(starts, vec![Some(0), Some(2), Some(0)]);
}

#[test]
fn a_turn_request_updates_before_lift_without_jumping_planted_positions() {
    let mut s = responsive_sequence();
    let mut before = None;
    for i in 0..=35 {
        let command = if i < 20 {
            [0.00125, 0., 0.]
        } else {
            [0., 0., 0.002]
        };
        let r = s.sample(i as f64 * 0.02, command, true, true).unwrap();
        if i == 35 {
            let previous: StepReference = before.unwrap();
            assert_eq!(r.phase, StepPhase::Raise);
            assert_eq!(r.latched_twist, [0., 0., 0.002]);
            assert_eq!(r.feet_world_m, previous.feet_world_m);
            assert_eq!(r.body_world_m, [0., 0.016, 0.]);
        }
        before = Some(r);
    }
}

#[test]
fn recenter_waits_for_four_foot_support_and_timeout_is_transactional() {
    let mut s = responsive_sequence();
    for i in 0..=120 {
        let before = format!("{s:?}");
        let command = if i < 20 { [0.00125, 0., 0.] } else { [0.; 3] };
        let result = s.sample(i as f64 * 0.02, command, true, false);
        if i == 120 {
            assert!(result.unwrap_err().contains("Recenter readiness timed out"));
            assert_eq!(format!("{s:?}"), before);
        } else {
            let r = result.unwrap();
            if i >= 70 {
                assert_eq!(r.phase, StepPhase::Recenter);
                assert!(r.waiting);
                assert_eq!(r.body_world_m, [0.; 3]);
                assert_eq!(r.step, 0);
            }
        }
    }
}

#[test]
fn swing_body_advance_preserves_feet_endpoints_and_landing_waits() {
    let feet = vec![[0., -0.3, -0.4], [0.3, 0., -0.4],
        [0., 0.3, -0.4], [-0.3, 0., -0.4]];
    for fraction in [0.5, 1.] {
        let mut config = sequence().config().clone();
        config.swing_body_advance_fraction = fraction;
        let mut moving = StepSequence::new(config, [0.; 3], 0., feet.clone()).unwrap();
        let mut baseline = sequence();
        let mut previous: Option<StepReference> = None;
        for i in 0..=120 {
            // Delay first landing after the scheduled end of lowering, then
            // command a stop while airborne. Both planners must finish safely.
            let landed = !(70..80).contains(&i);
            let command = if i < 60 { [0.005, 0., 0.01] } else { [0.; 3] };
            let a = baseline.sample(i as f64 * 0.02, command, true, landed).unwrap();
            let b = moving.sample(i as f64 * 0.02, command, true, landed).unwrap();
            assert_eq!(a.feet_world_m, b.feet_world_m);
            assert_eq!(a.phase, b.phase);
            assert_eq!(a.waiting, b.waiting);
            assert_eq!(a.step, b.step);
            if i == 55 { // Midpoint of the combined raise/lower trajectory.
                let next = advance_planar([0.; 3], [0.005, 0., 0.01], 2.);
                assert!((b.body_world_m[0] - a.body_world_m[0] - 0.5 * fraction * next[0]).abs() < 1e-14);
                assert!((b.yaw_rad - a.yaw_rad - 0.5 * fraction * next[2]).abs() < 1e-14);
            }
            if matches!(b.phase, StepPhase::Hold | StepPhase::Shift | StepPhase::Idle | StepPhase::Settle) {
                assert_eq!(a.body_world_m, b.body_world_m);
                assert_eq!(a.yaw_rad, b.yaw_rad);
            }
            if let Some(p) = previous {
                assert!((b.body_world_m[0] - p.body_world_m[0]).abs() < 0.002);
                assert!((b.yaw_rad - p.yaw_rad).abs() < 0.002);
                if b.waiting && p.waiting { assert_eq!(b.body_world_m, p.body_world_m); }
            }
            previous = Some(b);
        }
    }
    for invalid in [-0.1, 1.01, f64::NAN] {
        let mut config = sequence().config().clone();
        config.swing_body_advance_fraction = invalid;
        assert!(StepSequence::new(config, [0.; 3], 0., feet.clone()).is_err());
    }
}

#[test]
fn horizontal_swing_spans_apex_without_changing_clearance_support_or_endpoints() {
    let feet = vec![[0., -0.3, -0.4], [0.3, 0., -0.4],
        [0., 0.3, -0.4], [-0.3, 0., -0.4]];
    let mut config = sequence().config().clone();
    config.whole_swing_horizontal_motion = true;
    let mut moving = StepSequence::new(config, [0.; 3], 0., feet.clone()).unwrap();
    let mut original = sequence();
    let mut history = Vec::new();
    for i in 0..=120 {
        let landed = !(70..80).contains(&i);
        let command = if i < 60 { [0.005, 0., 0.01] } else { [0.; 3] };
        let a = original.sample(i as f64 * 0.02, command, true, landed).unwrap();
        let b = moving.sample(i as f64 * 0.02, command, true, landed).unwrap();
        assert_eq!(a.body_world_m, b.body_world_m);
        assert_eq!(a.yaw_rad, b.yaw_rad);
        assert_eq!(a.phase, b.phase);
        assert_eq!(a.waiting, b.waiting);
        assert_eq!(a.step, b.step);
        for foot in 0..4 {
            assert_eq!(a.feet_world_m[foot][2], b.feet_world_m[foot][2]);
            if b.foot != Some(foot) || !matches!(b.phase, StepPhase::Raise | StepPhase::Lower) {
                assert_eq!(a.feet_world_m[foot], b.feet_world_m[foot]);
            }
        }
        if i == 55 {
            for axis in 0..2 {
                assert!((b.feet_world_m[0][axis] -
                    0.5 * (feet[0][axis] + a.feet_world_m[0][axis])).abs() < 1e-14);
            }
        }
        if i >= 75 { assert_eq!(a.feet_world_m, b.feet_world_m); }
        history.push(b);
    }
    // Continuous horizontal velocity across the apex: the equal-duration
    // fixture's adjacent finite differences are symmetric around the midpoint.
    for axis in 0..2 {
        let before = history[55].feet_world_m[0][axis] - history[54].feet_world_m[0][axis];
        let after = history[56].feet_world_m[0][axis] - history[55].feet_world_m[0][axis];
        assert!((before - after).abs() < 1e-14);
    }
}

#[test]
fn early_horizontal_finish_holds_landing_xy_and_retains_vertical_clearance() {
    let feet = vec![[0., -0.3, -0.4], [0.3, 0., -0.4],
        [0., 0.3, -0.4], [-0.3, 0., -0.4]];
    for finish in [0.5, 0.75, 0.85, 1.] {
        let mut config = sequence().config().clone();
        config.whole_swing_horizontal_motion = true;
        config.horizontal_swing_finish_fraction = finish;
        let mut moving = StepSequence::new(config, [0.; 3], 0., feet.clone()).unwrap();
        let mut baseline = sequence();
        let mut previous = None;
        for i in 0..=100 {
            let a = baseline.sample(i as f64 * 0.02, [0.005, 0., 0.01], true, true).unwrap();
            let b = moving.sample(i as f64 * 0.02, [0.005, 0., 0.01], true, true).unwrap();
            assert_eq!(a.phase, b.phase); assert_eq!(a.body_world_m, b.body_world_m);
            for foot in 0..4 { assert_eq!(a.feet_world_m[foot][2], b.feet_world_m[foot][2]); }
            // Fixture starts raising at sample 35 and lands at sample 75.
            if i >= 35 + (40. * finish).ceil() as usize && i <= 75 {
                assert_eq!(a.feet_world_m[0], b.feet_world_m[0]);
                let xy = [b.feet_world_m[0][0], b.feet_world_m[0][1]];
                if let Some(p) = previous { assert_eq!(xy, p); }
                previous = Some(xy);
            }
        }
    }
    for invalid in [0., 0.49, 1.01, f64::NAN] {
        let mut config = sequence().config().clone(); config.horizontal_swing_finish_fraction = invalid;
        assert!(StepSequence::new(config, [0.; 3], 0., feet.clone()).is_err());
    }
}
