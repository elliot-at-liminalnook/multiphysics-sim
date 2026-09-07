use sim_runtime::{
    session::{EpisodeFrame, LinkPose, Recording, Telemetry},
    tracking::*,
};

fn requirements() -> TrackingRequirements {
    TrackingRequirements {
        version: 1,
        experiment_id: "bench-sweep-1".into(),
        coordinate_frame: "bench-Z-up-m".into(),
        start_s: 0.0,
        end_s: 0.1,
        max_sample_gap_s: 0.05,
        points: vec![PointBudget {
            id: "foot".into(),
            placement_margin_m: 0.02,
            other_error_reserve_m: 0.008,
        }],
    }
}
fn evidence(offset: [f64; 3], uncertainty_m: f64) -> TrackingEvidence {
    TrackingEvidence {
        version: 1,
        experiment_id: "bench-sweep-1".into(),
        coordinate_frame: "bench-Z-up-m".into(),
        source: EvidenceSource::Synthetic {
            description: "Known straight-line motion; not hardware evidence".into(),
        },
        completed: true,
        samples: (0..3)
            .map(|i| TrackingSample {
                time_s: i as f64 * 0.05,
                points: [(
                    "foot".into(),
                    PointMeasurement {
                        position_m: [i as f64 * 0.01 + offset[0], offset[1], offset[2]],
                        uncertainty_m,
                    },
                )]
                .into(),
            })
            .collect(),
    }
}

#[test]
fn budget_uses_vector_distance_both_uncertainties_and_other_errors() {
    let candidate = evidence([0.003, 0.004, 0.0], 0.001);
    let reference = evidence([0.0; 3], 0.002);
    let r = compare_tracking(&candidate, &reference, &requirements()).unwrap();
    assert!(r.passed);
    assert!((r.points[0].max_error_m - 0.005).abs() < 1e-14);
    assert!((r.points[0].rms_error_m - 0.005).abs() < 1e-14);
    assert!((r.points[0].minimum_remaining_margin_m - 0.004).abs() < 1e-14);
    let mut uncertain = reference.clone();
    uncertain.samples[1]
        .points
        .get_mut("foot")
        .unwrap()
        .uncertainty_m = 0.007;
    let r = compare_tracking(&candidate, &uncertain, &requirements()).unwrap();
    assert!(!r.passed);
    assert_eq!(r.points[0].violations, 1);
    assert_eq!(r.points[0].worst_margin_time_s, 0.05);
}

#[test]
fn missing_incomplete_unaligned_and_invalid_evidence_cannot_pass() {
    let reference = evidence([0.0; 3], 0.0);
    for mutate in [
        |e: &mut TrackingEvidence| {
            e.completed = false;
        },
        |e: &mut TrackingEvidence| {
            e.samples.clear();
        },
        |e: &mut TrackingEvidence| {
            e.samples[1].points.clear();
        },
        |e: &mut TrackingEvidence| {
            e.coordinate_frame = "camera-frame".into();
        },
        |e: &mut TrackingEvidence| {
            e.experiment_id = "different-motion".into();
        },
        |e: &mut TrackingEvidence| {
            e.samples[1].time_s = 0.0;
        },
        |e: &mut TrackingEvidence| {
            e.samples[1].time_s = 0.04;
        },
        |e: &mut TrackingEvidence| {
            e.samples[1].time_s = f64::NAN;
        },
        |e: &mut TrackingEvidence| {
            e.samples[0].time_s = 0.001;
        },
        |e: &mut TrackingEvidence| {
            e.samples.remove(1);
        },
        |e: &mut TrackingEvidence| {
            e.samples[1].points.get_mut("foot").unwrap().position_m[0] = f64::INFINITY;
        },
        |e: &mut TrackingEvidence| {
            e.samples[1].points.get_mut("foot").unwrap().uncertainty_m = -1.0;
        },
        |e: &mut TrackingEvidence| {
            e.source = EvidenceSource::Hardware {
                artifact: "".into(),
                procedure: "unregistered".into(),
            };
        },
    ] {
        let mut candidate = reference.clone();
        mutate(&mut candidate);
        assert!(
            compare_tracking(&candidate, &reference, &requirements()).is_err(),
            "{candidate:?}"
        );
    }
    // Even paired sparse evidence fails the requested sampling coverage.
    let mut sparse = reference.clone();
    sparse.samples.remove(1);
    assert!(compare_tracking(&sparse, &sparse, &requirements()).is_err());
    let mut r = requirements();
    r.points.push(r.points[0].clone());
    assert!(compare_tracking(&reference, &reference, &r).is_err());
    r = requirements();
    r.points[0].other_error_reserve_m = 0.02;
    assert!(compare_tracking(&reference, &reference, &r).is_err());
}

#[test]
fn rotated_marker_uses_local_offset_and_rejects_ambiguous_links() {
    let markers = vec![Marker {
        id: "foot".into(),
        link: "calf".into(),
        local_point_m: [0.1, 0.0, 0.0],
    }];
    let mut frame = EpisodeFrame {
        time_s: 0.0,
        done: false,
        poses: vec![LinkPose {
            name: "calf".into(),
            position_m: [1.0, 2.0, 3.0],
            rotation: [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
        }],
        joint_positions: vec![],
        telemetry: Telemetry::default(),
        contacts: vec![],
        error: None,
    };
    assert_eq!(
        sample_markers(&frame, &markers).unwrap().points["foot"].position_m,
        [1.0, 2.1, 3.0]
    );
    frame.poses.push(frame.poses[0].clone());
    assert!(sample_markers(&frame, &markers).is_err());
    frame.poses.clear();
    assert!(sample_markers(&frame, &markers).is_err());
}

#[test]
fn real_runtime_capture_matches_replay_and_refinement_on_moving_marker() {
    let mut recording = Recording {
        version: 1,
        scene: serde_json::from_str(include_str!(
            "../../../examples/interactive/pendulum.scene.json"
        ))
        .unwrap(),
        seed: 3,
        actions: vec![vec![0.2]; 4],
    };
    let config = CaptureConfig {
        experiment_id: "pendulum-refinement".into(),
        coordinate_frame: "world-Z-up".into(),
        expected_cad_sha256: None,
        markers: vec![Marker {
            id: "tip".into(),
            link: recording.scene.robot.links.last().unwrap().name.clone(),
            local_point_m: [0.1, 0.0, 0.0],
        }],
    };
    let candidate = capture_tracking(&recording, &config).unwrap();
    let mut wrong_source = config.clone();
    wrong_source.expected_cad_sha256 = Some("0".repeat(64));
    assert!(
        capture_tracking(&recording, &wrong_source)
            .unwrap_err()
            .contains("CAD hash")
    );
    let replay = capture_tracking(&recording, &config).unwrap();
    assert_eq!(
        serde_json::to_value(&candidate).unwrap(),
        serde_json::to_value(&replay).unwrap()
    );
    let first = candidate.samples[0].points["tip"].position_m;
    let last = candidate.samples.last().unwrap().points["tip"].position_m;
    assert!(
        first.iter().zip(last).any(|(a, b)| (a - b).abs() > 1e-6),
        "must test actual motion"
    );
    recording.scene.options.step *= 0.5;
    let reference = capture_tracking(&recording, &config).unwrap();
    let r = TrackingRequirements {
        version: 1,
        experiment_id: config.experiment_id,
        coordinate_frame: config.coordinate_frame,
        start_s: 0.0,
        end_s: candidate.samples.last().unwrap().time_s,
        max_sample_gap_s: recording.scene.period_s,
        points: vec![PointBudget {
            id: "tip".into(),
            placement_margin_m: 0.001,
            other_error_reserve_m: 0.0,
        }],
    };
    let report = compare_tracking(&candidate, &reference, &r).unwrap();
    assert!(report.passed, "{report:?}");
    assert_eq!(report.candidate_source, "simulation");
    assert_eq!(report.reference_source, "simulation");
}

#[test]
fn marker_tracking_in_a_moving_link_frame_preserves_relative_motion() {
    let mut frame = EpisodeFrame {
        time_s: 0.,
        done: false,
        joint_positions: vec![],
        telemetry: Default::default(),
        contacts: vec![],
        error: None,
        poses: vec![
            LinkPose {
                name: "body".into(),
                position_m: [10., 20., 30.],
                rotation: [[0., -1., 0.], [1., 0., 0.], [0., 0., 1.]],
            },
            LinkPose {
                name: "foot".into(),
                position_m: [10., 22., 30.],
                rotation: [[0., -1., 0.], [1., 0., 0.], [0., 0., 1.]],
            },
        ],
    };
    let markers = [Marker {
        id: "tip".into(),
        link: "foot".into(),
        local_point_m: [1., 0., 0.],
    }];
    let sample = sample_markers_in_link_frame(&frame, &markers, "body").unwrap();
    assert_eq!(sample.points["tip"].position_m, [3., 0., 0.]);
    for p in &mut frame.poses {
        for x in &mut p.position_m {
            *x += 100.;
        }
    }
    assert_eq!(
        sample_markers_in_link_frame(&frame, &markers, "body")
            .unwrap()
            .points["tip"]
            .position_m,
        [3., 0., 0.]
    );
    assert!(sample_markers_in_link_frame(&frame, &markers, "missing").is_err());
    frame.poses[0].rotation[0][0] = 1.;
    assert!(sample_markers_in_link_frame(&frame, &markers, "body").is_err());
}
