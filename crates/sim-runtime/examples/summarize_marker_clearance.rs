use serde_json::{Value, json};
use sim_runtime::{
    contact_audit::sampled_floor_clearances,
    session::{LinkPose, Scene, Session},
    support::static_support_geometry,
    tracking::CaptureConfig,
};
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if !(3..=5).contains(&args.len()) {
        return Err(
            "usage: summarize_marker_clearance scene.json capture.json markers.json [excluded-swing-marker-id] [minimum-support-force-n]".into(),
        );
    }
    let read = |p: &str| std::fs::read(p).map_err(|e| e.to_string());
    let scene: Scene = serde_json::from_slice(&read(&args[0])?).map_err(|e| e.to_string())?;
    let capture: Value = serde_json::from_slice(&read(&args[1])?).map_err(|e| e.to_string())?;
    let markers: CaptureConfig =
        serde_json::from_slice(&read(&args[2])?).map_err(|e| e.to_string())?;
    if capture["completed"] != true
        || !capture["error"].is_null()
        || capture["source"] != scene.robot.source
        || markers.expected_cad_sha256.as_deref() != scene.robot.source["cad_sha256"].as_str()
    {
        return Err("complete capture and matching CAD provenance required".into());
    }
    let session = Session::new(scene, 0)?;
    let minimum_force = args
        .get(4)
        .map(|s| s.parse::<f64>().map_err(|e| e.to_string()))
        .transpose()?;
    if minimum_force.is_some_and(|f| !f.is_finite() || f < 0.0) {
        return Err("finite nonnegative minimum support force required".into());
    }
    let links: Vec<_> = markers.markers.iter().map(|m| m.link.clone()).collect();
    let supports = if let Some(excluded) = args.get(3) {
        if markers.markers.iter().filter(|m| &m.id == excluded).count() != 1 {
            return Err("swing marker must be unique and present".into());
        }
        Some(
            markers
                .markers
                .iter()
                .filter(|m| &m.id != excluded)
                .cloned()
                .collect::<Vec<_>>(),
        )
    } else {
        None
    };
    let mut frames = vec![];
    let mut previous = None;
    for f in capture["frames"]
        .as_array()
        .ok_or("missing capture frames")?
    {
        let t = f["time_s"]
            .as_f64()
            .filter(|t| t.is_finite() && *t >= 0.0)
            .ok_or("invalid frame time")?;
        if previous.is_some_and(|p| t <= p) {
            return Err("nonmonotonic frame times".into());
        }
        previous = Some(t);
        let poses: Vec<LinkPose> =
            serde_json::from_value(f["poses"].clone()).map_err(|e| e.to_string())?;
        let support = supports
            .as_ref()
            .map(|m| static_support_geometry(&session.robot.art, &poses, m))
            .transpose()?;
        let shift = minimum_force
            .map(|minimum| {
                let s = support
                    .as_ref()
                    .ok_or("explicit tripod supports required for a load target")?;
                let p: [[f64; 2]; 3] = s
                    .support_points_world_xy_m
                    .clone()
                    .try_into()
                    .map_err(|_| "exactly three support markers required")?;
                sim_runtime::support::minimum_support_shift(
                    [s.center_of_mass_world_m[0], s.center_of_mass_world_m[1]],
                    s.total_mass_kg * (-session.robot.art.model.gravity[2]),
                    &p,
                    [minimum; 3],
                )
            })
            .transpose()?;
        frames.push(json!({"time_s":t,"clearances":sampled_floor_clearances(&session.robot.art,&poses,&links)?,"assumed_static_support":support,
            "minimum_support_force_n":minimum_force,"minimum_load_com_shift":shift}));
    }
    if frames.is_empty() {
        return Err("missing capture frames".into());
    }
    println!("{}",serde_json::to_string_pretty(&json!({"source":session.scene.robot.source,"world":session.scene.robot.world,"frames":frames,"scope":"Sampled compiled foot-surface clearance over the supplied world floor; caller must supply the original experiment world. Not continuous or full-CAD clearance, force support, stability or a walking gate."})).map_err(|e|e.to_string())?);
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
