//! The camera turntable's Rhai model scripts build their parts on an empty
//! archive, record the run, and re-running replaces what the last run made.
use serde_json::json;
use sim_cad::ArchiveDocument;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

fn empty() -> ArchiveDocument {
    ArchiveDocument::from_bytes(Path::new("/tmp/scripts-test.rcad"), sim_cad::edit::empty_archive(None).unwrap(), &|| false, &|_| {}).unwrap()
}

#[test]
fn the_turntable_model_builds_and_rebuilds_in_place() {
    let doc = Arc::new(empty());
    let stop = Arc::new(AtomicBool::new(false));
    let (edit, summary) = sim_cad::scripts::stage(doc.clone(), "examples/camera-turntable/cad/turntable_model.rhai", &json!({}), true, stop.clone()).unwrap();
    assert_eq!(summary.script, "examples/camera-turntable/cad/turntable_model.rhai");
    // 17 bodies, two groups and the joint, plus the fixed connections.
    assert!(summary.created.len() >= 21, "{} nodes", summary.created.len());
    let layout = &summary.result["layout"];
    assert!((layout["belt_length_mm"].as_f64().unwrap() - 610.0).abs() < 1e-6, "{layout}");
    assert!(layout["teeth_in_mesh"].as_f64().unwrap() > 6.0);
    let built = doc.apply(edit).unwrap();
    assert_eq!(built.manifest["robot_settings"]["model_scripts"]["examples/camera-turntable/cad/turntable_model.rhai"]["nodes"].as_array().unwrap().len(), summary.created.len());
    // Every body has geometry the kernel reads.
    let g = sim_cad::geometry::load_geometry(&built, &|| false, &|_| {}).unwrap();
    assert_eq!(g.len(), 17, "the 17 bodies");
    // Running it again with another servo angle replaces the previous parts.
    let (again, second) = sim_cad::scripts::stage(Arc::new(built), "examples/camera-turntable/cad/turntable_model.rhai", &json!({"servo_angle_deg": 30.0}), true, stop).unwrap();
    assert!(!second.removed.is_empty());
    let _ = again;
}

#[test]
fn the_object_scan_kit_builds_its_platform_stand_and_cameras() {
    let doc = Arc::new(empty());
    let (edit, summary) = sim_cad::scripts::stage(doc.clone(), "examples/camera-turntable/cad/object_scan_kit.rhai", &json!({"stand_angle_deg": 20.0}), true, Arc::new(AtomicBool::new(false))).unwrap();
    let nodes = &summary.result["nodes"];
    for key in ["platform", "stand", "camera_low", "camera_high"] {
        assert!(nodes[key].is_string(), "{key} in {nodes}");
    }
    assert!(summary.result["cameras"]["high"]["tilt_deg"].as_f64().unwrap() > summary.result["cameras"]["low"]["tilt_deg"].as_f64().unwrap());
    doc.apply(edit).unwrap();
}

#[test]
fn a_script_outside_the_repository_or_not_rhai_is_refused() {
    let doc = Arc::new(empty());
    let stop = Arc::new(AtomicBool::new(false));
    let e = sim_cad::scripts::stage(doc.clone(), "/etc/hosts", &json!({}), true, stop.clone()).err().unwrap();
    assert!(e.contains("not a Rhai file") || e.contains("inside the repository"), "{e}");
    let e = sim_cad::scripts::stage(doc, "README.md", &json!({}), true, stop).err().unwrap();
    assert!(e.contains("not a Rhai file"), "{e}");
}
