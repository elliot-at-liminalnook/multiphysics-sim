use sim_runtime::{
    posture::*,
    session::{EpisodeFrame, LinkPose},
    tracking::Marker,
};
fn fixture() -> (Vec<EpisodeFrame>, HoldConfig) {
    let config = HoldConfig {
        expected_cad_sha256: None,
        body_link: "body".into(),
        body_up_local: [0., 0., 1.],
        world_up: [0., 0., 1.],
        markers: vec![Marker {
            id: "tip".into(),
            link: "body".into(),
            local_point_m: [0., 0., 1.],
        }],
        start_s: 0.,
        end_s: 1.,
        maximum_sample_gap_s: 1.,
    };
    let frame = EpisodeFrame {
        time_s: 0.,
        done: false,
        poses: vec![LinkPose {
            name: "body".into(),
            position_m: [0.; 3],
            rotation: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        }],
        joint_positions: vec![],
        telemetry: Default::default(),
        contacts: vec![],
        error: None,
    };
    let mut end = frame.clone();
    end.time_s = 1.;
    end.poses[0].position_m = [3., 4., 2.];
    (vec![frame, end], config)
}
#[test]
fn hold_metrics_match_known_translation_and_tilt() {
    let (mut fs, c) = fixture();
    let r = summarize_hold(&fs, true, None, &c).unwrap();
    assert_eq!(r.maximum_body_horizontal_displacement_m, 5.);
    assert_eq!(r.maximum_body_height_change_m, 2.);
    assert_eq!(r.maximum_body_tilt_rad, 0.);
    assert!((r.maximum_marker_displacement_m["tip"] - 29f64.sqrt()).abs() < 1e-14);
    fs[1].poses[0].position_m = [0.; 3];
    fs[1].poses[0].rotation = [[1., 0., 0.], [0., 0., -1.], [0., 1., 0.]];
    let r = summarize_hold(&fs, true, None, &c).unwrap();
    assert!((r.maximum_body_tilt_rad - std::f64::consts::FRAC_PI_2).abs() < 1e-14);
    assert!((r.maximum_marker_displacement_m["tip"] - 2f64.sqrt()).abs() < 1e-14);
}
#[test]
fn hold_rejects_incomplete_sparse_ambiguous_and_invalid_evidence() {
    let (fs, c) = fixture();
    assert!(summarize_hold(&fs, false, None, &c).is_err());
    let mut wrong_source = c.clone();
    wrong_source.expected_cad_sha256 = Some("expected".into());
    assert!(summarize_hold(&fs, true, Some("different"), &wrong_source).is_err());
    let mut bad = c.clone();
    bad.maximum_sample_gap_s = 0.1;
    assert!(summarize_hold(&fs, true, None, &bad).is_err());
    let mut bad = c.clone();
    bad.world_up = [0.; 3];
    assert!(summarize_hold(&fs, true, None, &bad).is_err());
    let mut bad = fs.clone();
    bad[1].poses[0].rotation[2][2] = -1.;
    assert!(summarize_hold(&bad, true, None, &c).is_err());
    let mut bad = fs.clone();
    bad[1].poses[0].position_m[0] = f64::NAN;
    assert!(summarize_hold(&bad, true, None, &c).is_err());
    let mut bad = fs.clone();
    let duplicate = bad[0].poses[0].clone();
    bad[1].poses.push(duplicate);
    assert!(summarize_hold(&bad, true, None, &c).is_err());
    let mut bad = fs.clone();
    bad[1].time_s = 0.;
    assert!(summarize_hold(&bad, true, None, &c).is_err());
    let mut bad = fs.clone();
    bad[1].error = Some("failed".into());
    assert!(summarize_hold(&bad, true, None, &c).is_err());
}
#[test]
fn declared_up_directions_allow_other_frames() {
    let (fs, mut c) = fixture();
    c.world_up = [1., 0., 0.];
    c.body_up_local = [1., 0., 0.];
    let r = summarize_hold(&fs, true, None, &c).unwrap();
    assert_eq!(r.maximum_body_height_change_m, 3.);
    assert!((r.maximum_body_horizontal_displacement_m - 20f64.sqrt()).abs() < 1e-14);
    assert_eq!(r.maximum_body_tilt_rad, 0.);
}
