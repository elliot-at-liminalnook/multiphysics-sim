use sim_runtime::{
    planning::{plan_marker_motion_with_inspections, MarkerMotionConfig},
    session::{Scene, Session},
    tracking::CaptureConfig,
};
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if !(3..=4).contains(&args.len()) {
        return Err(
            "usage: plan_marker_motion scene.json markers.json config.json [inspection-times.json]"
                .into(),
        );
    }
    let read = |p: &str| std::fs::read(p).map_err(|e| e.to_string());
    let scene: Scene = serde_json::from_slice(&read(&args[0])?).map_err(|e| e.to_string())?;
    let markers: CaptureConfig =
        serde_json::from_slice(&read(&args[1])?).map_err(|e| e.to_string())?;
    let config: MarkerMotionConfig =
        serde_json::from_slice(&read(&args[2])?).map_err(|e| e.to_string())?;
    let session = Session::new(scene, 0)?;
    let extra: Vec<f64> = if args.len() == 4 {
        serde_json::from_slice(&read(&args[3])?).map_err(|e| e.to_string())?
    } else {
        vec![]
    };
    let plan = plan_marker_motion_with_inspections(
        &session.robot.art,
        &session.robot.generalized(),
        &markers,
        &config,
        &extra,
    )?;
    println!(
        "{}",
        serde_json::to_string_pretty(&plan).map_err(|e| e.to_string())?
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
