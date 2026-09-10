//! Export the shared runtime's actual compiled surface samples for explicit links.
use sim_runtime::{
    session::{Scene, Session},
    tracking::{CaptureConfig, compiled_surface_markers},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        return Err("usage: export_surface_markers scene.json selection.json".into());
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Selection {
        links: Vec<String>,
        expected_cad_sha256: String,
        coordinate_frame: String,
        experiment_id: String,
    }
    let scene: Scene = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let selection: Selection = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    if selection.coordinate_frame.is_empty() || selection.experiment_id.is_empty() {
        return Err("explicit frame and experiment ID required".into());
    }
    let session = Session::new(scene, 0)?;
    let markers = compiled_surface_markers(
        &session.robot.art,
        &selection.links,
        &selection.expected_cad_sha256,
    )?;
    println!(
        "{}",
        serde_json::to_string(&CaptureConfig {
            experiment_id: selection.experiment_id,
            coordinate_frame: selection.coordinate_frame,
            expected_cad_sha256: Some(selection.expected_cad_sha256),
            markers,
        })?
    );
    Ok(())
}
