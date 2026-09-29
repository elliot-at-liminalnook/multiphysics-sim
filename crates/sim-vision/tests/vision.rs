use sim_vision::camera::CameraModel;
use sim_vision::mvs::{MvsSettings, View, depth_map};
use sim_vision::refine::{RefineSettings, refine_angles};
use sim_vision::render::{RenderSettings, render};
use sim_vision::rig::Rig;
use sim_vision::scene::Scene;
use sim_vision::{Gray, dot, norm, sub};

fn rig() -> Rig {
    Rig { schema: Rig::SCHEMA.into(), axis_point: [0.0; 3], axis: [0.0, 0.0, 1.0], optical_center: [0.0, 0.156, 0.084], optical_axis: [0.0, 1.0, 0.0], up: [0.0, 0.0, 1.0], source: serde_json::Value::Null }
}

fn room() -> Scene {
    serde_json::from_value(serde_json::json!({
        "schema": "sim.vision-scene/1",
        "lighting": {"ambient": 0.45, "sun_direction": [0.3, 0.5, -1.0], "sun": 0.6},
        "objects": [
            {"name": "room", "shape": {"kind": "room", "min": [-2.0, -1.6, -0.8], "max": [2.4, 1.9, 1.7]},
             "texture": {"kind": "noise", "scale": 6.0, "a": [0.15, 0.2, 0.3], "b": [0.9, 0.85, 0.7], "seed": 3}},
            {"name": "crate", "shape": {"kind": "box", "center": [0.9, 0.9, -0.3], "size": [0.4, 0.4, 0.4], "yaw_deg": 25.0},
             "texture": {"kind": "noise", "scale": 20.0, "a": [0.5, 0.2, 0.1], "b": [0.95, 0.7, 0.4], "seed": 9}},
            {"name": "ball", "shape": {"kind": "sphere", "center": [-0.8, 0.6, 0.1], "radius": 0.25},
             "texture": {"kind": "noise", "scale": 25.0, "a": [0.1, 0.4, 0.2], "b": [0.8, 0.95, 0.6], "seed": 4}}
        ]
    }))
    .unwrap()
}

fn shots(n: usize, errors_deg: &[f64], width: usize) -> (CameraModel, Vec<Gray>, Vec<f64>, Vec<f64>) {
    let camera = CameraModel::camera_module_3_wide(width);
    let (scene, rig) = (room(), rig());
    let truth: Vec<f64> = (0..n).map(|k| (k as f64 * 360.0 / n as f64).to_radians()).collect();
    let images = truth.iter().map(|&a| render(&scene, &camera, &|_| rig.pose(a), 0.0, RenderSettings { supersample: 2, time_samples: 1 }).gray()).collect();
    let reported = truth.iter().zip(errors_deg.iter().cycle()).map(|(a, e)| a + e.to_radians()).collect();
    (camera, images, truth, reported)
}

#[test]
fn rig_pose_turns_the_camera_about_the_axis() {
    let r = rig();
    let p = r.pose(90f64.to_radians());
    assert!(norm(sub(p.center, [-0.156, 0.0, 0.084])) < 1e-12);
    // Optical axis points radially outward; image "down" is world −Z.
    assert!(norm(sub(p.direction_to_world([0.0, 0.0, 1.0]), [-1.0, 0.0, 0.0])) < 1e-12);
    assert!(norm(sub(p.direction_to_world([0.0, 1.0, 0.0]), [0.0, 0.0, -1.0])) < 1e-12);
    // Projection round trip.
    let cam = CameraModel::camera_module_3_wide(640);
    let world = [-1.2, 0.3, 0.5];
    let (u, v) = cam.project(p.to_camera(world)).unwrap();
    let back = p.to_world(sim_vision::scale(cam.ray(u, v), p.to_camera(world)[2]));
    assert!(norm(sub(back, world)) < 1e-9);
    assert!((cam.horizontal_fov_deg() - 102.0).abs() < 1e-9);
    assert!((r.radius() - 0.156).abs() < 1e-12);
    assert!(dot(p.direction_to_world([1.0, 0.0, 0.0]), [0.0, 0.0, 1.0]).abs() < 1e-12);
}

#[test]
fn plane_sweep_recovers_ground_truth_depth() {
    let (camera, images, truth, _) = shots(24, &[0.0], 320);
    let rig = rig();
    let scene = room();
    let gt = render(&scene, &camera, &|_| rig.pose(truth[0]), 0.0, RenderSettings { supersample: 1, time_samples: 1 });
    let reference = View { image: &images[0], camera: &camera, pose: rig.pose(truth[0]) };
    let neighbors: Vec<View> = [1usize, 2, 22, 23].iter().map(|&j| View { image: &images[j], camera: &camera, pose: rig.pose(truth[j]) }).collect();
    let map = depth_map(&reference, &neighbors, MvsSettings { depths: 160, ..MvsSettings::default() });
    let mut errors: Vec<f64> = Vec::new();
    for j in 0..map.rows {
        for i in 0..map.columns {
            let z = map.depth[j * map.columns + i];
            if z.is_finite() {
                let (x, y) = map.pixel(i, j);
                let truth = gt.depth[y * camera.width + x] as f64;
                errors.push((z as f64 - truth).abs() / truth);
            }
        }
    }
    errors.sort_by(|a, b| a.total_cmp(b));
    let completeness = errors.len() as f64 / (map.rows * map.columns) as f64;
    let median = errors[errors.len() / 2];
    let within_5 = errors.iter().filter(|e| **e < 0.05).count() as f64 / errors.len() as f64;
    eprintln!("completeness {completeness:.2}, median relative error {median:.4}, within 5 % {within_5:.2}");
    assert!(completeness > 0.5, "completeness {completeness}");
    assert!(median < 0.02, "median relative depth error {median}");
    assert!(within_5 > 0.85, "share within 5 %: {within_5}");
}

#[test]
fn angle_refinement_recovers_injected_servo_errors() {
    let injected = [0.0, 0.35, -0.2, 0.1, -0.3, 0.25, 0.0, -0.15, 0.3, -0.25, 0.2, -0.1];
    let (camera, images, truth, reported) = shots(24, &injected, 480);
    let refined = refine_angles(&images, &reported, &camera, &rig(), RefineSettings { sweeps: 5, ..RefineSettings::default() });
    // Errors after removing the unobservable common offset, in degrees and pixels.
    let pixel_deg = camera.horizontal_fov_deg() / camera.width as f64;
    let errors = |c: &[f64]| -> Vec<f64> {
        let e: Vec<f64> = reported.iter().zip(c).zip(&truth).map(|((r, c), t)| (r + c - t).to_degrees()).collect();
        let mean = e.iter().sum::<f64>() / e.len() as f64;
        e.iter().map(|x| x - mean).collect()
    };
    // Split each error pattern into its smooth part (Fourier terms up to
    // once per 90°, which neighbours cannot see: it trades exactly against an
    // inverse-depth offset) and the shot-to-shot remainder, which stitching needs.
    let split = |e: &[f64]| -> (Vec<f64>, Vec<f64>) {
        let n = e.len();
        let mut smooth = vec![e.iter().sum::<f64>() / n as f64; n];
        for k in 1..=4usize {
            let (mut a, mut b) = (0.0, 0.0);
            for (i, x) in e.iter().enumerate() {
                let t = 2.0 * std::f64::consts::PI * (k * i) as f64 / n as f64;
                a += 2.0 / n as f64 * x * t.cos();
                b += 2.0 / n as f64 * x * t.sin();
            }
            for (i, s) in smooth.iter_mut().enumerate() {
                let t = 2.0 * std::f64::consts::PI * (k * i) as f64 / n as f64;
                *s += a * t.cos() + b * t.sin();
            }
        }
        let rest = e.iter().zip(&smooth).map(|(x, s)| x - s).collect();
        (smooth, rest)
    };
    let rms = |e: &[f64]| (e.iter().map(|x| x * x).sum::<f64>() / e.len() as f64).sqrt();
    let (before, after) = (errors(&vec![0.0; truth.len()]), errors(&refined.corrections));
    let ((sb, rb), (sa, ra)) = (split(&before), split(&after));
    eprintln!("1 px = {pixel_deg:.3}°; shot-to-shot rms {:.4}° → {:.4}°, smooth rms {:.4}° → {:.4}°; sweeps {:?}",
        rms(&rb), rms(&ra), rms(&sb), rms(&sa), refined.sweep_change.iter().map(|c| (c.to_degrees() * 1000.0).round() / 1000.0).collect::<Vec<_>>());
    eprintln!("errors after (deg): {:?}", after.iter().map(|x| (x * 1000.0).round() / 1000.0).collect::<Vec<_>>());
    assert!(rms(&rb) > 0.15);
    // Shot-to-shot errors (what stitching seams depend on): under 1/5 pixel rms, 5× smaller.
    assert!(rms(&ra) < pixel_deg / 5.0 && rms(&ra) < rms(&rb) / 5.0, "shot-to-shot rms after refinement {}°", rms(&ra));
}
