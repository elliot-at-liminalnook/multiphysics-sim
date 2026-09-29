//! Diagnostic: precision of one refinement sweep's local optimum with exact
//! angles (any nonzero correction is estimator noise or bias).
use sim_vision::camera::CameraModel;
use sim_vision::mvs::MvsSettings;
use sim_vision::refine::{RefineSettings, refine_angles};
use sim_vision::render::{RenderSettings, render};
use sim_vision::rig::Rig;
use sim_vision::scene::Scene;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let scene: Scene = serde_json::from_str(&std::fs::read_to_string(&args[1]).unwrap()).unwrap();
    let width: usize = args[2].parse().unwrap();
    let rig = Rig { schema: Rig::SCHEMA.into(), axis_point: [0.0; 3], axis: [0.0, 0.0, 1.0], optical_center: [0.0, 0.156, 0.084], optical_axis: [0.0, 1.0, 0.0], up: [0.0, 0.0, 1.0], source: serde_json::Value::Null };
    let camera = CameraModel::camera_module_3_wide(width);
    let n = 24;
    let truth: Vec<f64> = (0..n).map(|k| (k as f64 * 15.0).to_radians()).collect();
    let images: Vec<_> = truth.iter().map(|&a| render(&scene, &camera, &|_| rig.pose(a), 0.0, RenderSettings { supersample: 2, time_samples: 1 }).gray()).collect();
    for (pixels, hw, depths) in [(300, 2, 64), (1000, 2, 64), (300, 3, 64), (300, 2, 160)] {
        let s = RefineSettings { sweeps: 1, pixels, global_solve: false, mvs: MvsSettings { half_window: hw, depths, min_score: -1.0, ..MvsSettings::default() }, ..RefineSettings::default() };
        let r = refine_angles(&images, &truth, &camera, &rig, s);
        let rms = (r.corrections.iter().map(|c| c.to_degrees().powi(2)).sum::<f64>() / n as f64).sqrt();
        println!("width {width} pixels {pixels} window {} depths {depths}: rms local optimum {rms:.4}° ({:.3} px)", 2 * hw + 1, rms / (102.0 / width as f64));
    }
}
