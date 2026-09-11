//! Sampled geometry audit of a complete or explicitly partial environment capture.
use sim_runtime::{
    contact_audit::{sampled_floor_clearances, sampled_inter_link_penetrations},
    motion_data::MotionSnapshot,
    session::{LinkPose, Scene, Session},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let include_pairs = args.len() == 2 && args[1] == "--pairs";
    if args.len() != 1 && !include_pairs {
        return Err(
            "usage: audit_capture_geometry environment-or-motion-capture.json [--pairs]".into(),
        );
    }
    let bytes = std::fs::read(&args[0])?;
    let capture: serde_json::Value = serde_json::from_slice(&bytes)?;
    let motion_capture = capture["benchmark_source"].is_object()
        && capture["metadata"]["frame_coordinates"].is_array();
    if capture["kind"] != "sampled_environment_capture" && !motion_capture {
        return Err("environment or benchmark motion capture required".into());
    }
    let scene: Scene = serde_json::from_value(capture["recording"]["scene"].clone())?;
    let seed = capture["recording"]["seed"]
        .as_u64()
        .ok_or("recorded seed required")?;
    let session = Session::new(scene, seed)?;
    let art = &session.robot.art;
    let links = art
        .links
        .iter()
        .filter(|l| !l.contact.is_empty())
        .map(|l| l.name.clone())
        .collect::<Vec<_>>();
    let frames = capture["frames"].as_array().ok_or("frames required")?;
    if frames.is_empty() {
        return Err("nonempty recorded frames required".into());
    }
    let mut out = vec![];
    let mut previous = -1.;
    for frame in frames {
        let time = frame["time_s"].as_f64().ok_or("frame time required")?;
        if !time.is_finite() || time < 0. || time <= previous {
            return Err("finite increasing frame times required".into());
        }
        previous = time;
        let poses: Vec<LinkPose> = if motion_capture {
            MotionSnapshot::from_frame(frame)?
                .poses
                .into_iter()
                .map(|p| LinkPose {
                    name: p.name,
                    position_m: p.position_m,
                    rotation: p.rotation,
                })
                .collect()
        } else {
            serde_json::from_value(frame["poses"].clone())?
        };
        let penetrations = sampled_inter_link_penetrations(art, &poses)?;
        let maximum = penetrations
            .iter()
            .map(|p| p.penetration_m)
            .fold(0., f64::max);
        let floor = sampled_floor_clearances(art, &poses, &links)?;
        let mut row = serde_json::json!({"time_s":time,"maximum_inter_link_penetration_m":maximum,"floor_clearances":floor});
        if include_pairs {
            row["inter_link_penetrations"] = serde_json::to_value(&penetrations)?;
        }
        out.push(row);
    }
    let mut report = serde_json::json!({"source":session.scene.robot.source,"seed":seed,
        "capture_blake3":blake3::hash(&bytes).to_hex().to_string(),
        "capture_format":if motion_capture {"benchmark_motion_capture"} else {"sampled_environment_capture"},
        "capture_completed":if motion_capture {&capture["benchmark_source"]["summary"]["episode_complete"]} else {&capture["completed"]},
        "capture_error":capture["error"],"frames":out,
        "scope":"Shared runtime compiled geometry checked at recorded poses only. Partial captures remain partial; no continuous-time collision or successful gait certificate."});
    if include_pairs {
        report["link_names"] =
            serde_json::to_value(art.links.iter().map(|link| &link.name).collect::<Vec<_>>())?;
    }
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}
