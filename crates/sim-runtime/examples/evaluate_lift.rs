//! Apply a shared sampled lift check to a complete embedded motor capture.
use serde_json::{Value, json};
use sim_runtime::{
    contact_audit::sampled_floor_clearances,
    lift::{LiftRequirements, LiftSample, evaluate_lift},
    session::{LinkPose, Scene, Session},
};
use std::collections::{BTreeMap, BTreeSet};

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let explicit_time = args.get(3).is_some_and(|x| x == "--simulation-time");
    if args.len() != 3 && !(args.len() == 4 && explicit_time) {
        return Err(
            "usage: evaluate_lift scene.json capture.json requirements.json [--simulation-time]"
                .into(),
        );
    }
    let read = |p: &str| -> Result<Value, String> {
        serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())
    };
    let scene: Scene = serde_json::from_value(read(&args[0])?).map_err(|e| e.to_string())?;
    let mut capture = read(&args[1])?;
    // Environment captures preserve the entire typed scene in their recording.
    // Accept that native/WASM path directly instead of requiring metadata to be
    // copied by a robot-specific conversion script.
    if capture["kind"] == "sampled_environment_capture" {
        let recorded: Scene = serde_json::from_value(capture["recording"]["scene"].clone())
            .map_err(|e| format!("environment scene: {e}"))?;
        if serde_json::to_value(&recorded).map_err(|e| e.to_string())?
            != serde_json::to_value(&scene).map_err(|e| e.to_string())?
        {
            return Err("environment capture and supplied scene differ".into());
        }
        capture["source"] = recorded.robot.source.clone();
        capture["scene_options"] =
            serde_json::to_value(&recorded.options).map_err(|e| e.to_string())?;
        capture["world"] =
            serde_json::to_value(&recorded.robot.world).map_err(|e| e.to_string())?;
        capture["motion_gate"] = capture["recording"]["config"]["motion_gate"].clone();
    }
    // An array batches independent windows over one immutable recording.
    let requested = read(&args[2])?;
    let batch = requested.is_array();
    let requirements: Vec<LiftRequirements> = if batch {
        serde_json::from_value(requested).map_err(|e| e.to_string())?
    } else {
        vec![serde_json::from_value(requested).map_err(|e| e.to_string())?]
    };
    if requirements.is_empty() {
        return Err("at least one lift requirement required".into());
    }
    if capture["completed"] != true
        || !capture["error"].is_null()
        || capture["source"] != scene.robot.source
        || capture["scene_options"]
            != serde_json::to_value(&scene.options).map_err(|e| e.to_string())?
        || (!capture["motion_gate"].is_null() && !explicit_time)
        || !scene.options.contact
        || scene.robot.gravity[0] != 0.0
        || scene.robot.gravity[1] != 0.0
        || scene.robot.gravity[2] >= 0.0
    {
        return Err("complete contact-enabled capture with matching source/options and world -Z gravity required; gated captures require explicit --simulation-time for phase boundaries".into());
    }
    let session = Session::new(scene, 0)?;
    let recorded_world = !capture["world"].is_null();
    if recorded_world
        && capture["world"]
            != serde_json::to_value(&session.scene.robot.world).map_err(|e| e.to_string())?
    {
        return Err("capture and supplied scene worlds differ".into());
    }
    let art = &session.robot.art;
    let names: Vec<_> = requirements
        .iter()
        .flat_map(|r| {
            r.minimum_support_forces_n
                .keys()
                .chain(std::iter::once(&r.swing_link))
        })
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let swings: Vec<_> = requirements
        .iter()
        .map(|r| r.swing_link.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    for name in &names {
        if art.links.iter().filter(|l| &l.name == name).count() != 1 {
            return Err(format!("unique link required: {name}"));
        }
    }
    let mut series: BTreeMap<String, Vec<LiftSample>> =
        swings.iter().map(|n| (n.clone(), vec![])).collect();
    for frame in capture["frames"]
        .as_array()
        .ok_or("missing captured frames")?
    {
        let poses: Vec<LinkPose> =
            serde_json::from_value(frame["poses"].clone()).map_err(|e| e.to_string())?;
        let clearance = sampled_floor_clearances(art, &poses, &swings)?;
        let mut forces: BTreeMap<String, f64> = names.iter().map(|n| (n.clone(), 0.0)).collect();
        for c in frame["contacts"]
            .as_array()
            .ok_or("missing explicit contact array")?
        {
            let index = c["link"]
                .as_u64()
                .and_then(|i| usize::try_from(i).ok())
                .filter(|i| *i < art.links.len())
                .ok_or("invalid contact link")?;
            if !c["other"].is_null() {
                continue;
            }
            let f: [f64; 3] =
                serde_json::from_value(c["force_n"].clone()).map_err(|e| e.to_string())?;
            if f.iter().any(|x| !x.is_finite()) {
                return Err("nonfinite floor force".into());
            }
            if let Some(total) = forces.get_mut(&art.links[index].name) {
                *total += f[2];
            }
        }
        let time_s = frame["time_s"].as_f64().ok_or("missing time")?;
        for c in clearance {
            series.get_mut(&c.link).unwrap().push(LiftSample {
                time_s,
                swing_clearance_m: c.minimum_clearance_m,
                floor_forces_n: forces.clone(),
            });
        }
    }
    let reports = requirements
        .iter()
        .map(|r| {
            let report = evaluate_lift(&series[&r.swing_link], r)?;
            Ok(json!({"requirements":r,"report":report}))
        })
        .collect::<Result<Vec<Value>, String>>()?;
    let mut result = json!({"time_basis":"simulation_time","motion_gate":capture["motion_gate"],
        "source":capture["source"],"world":session.scene.robot.world,"world_recorded_in_capture":recorded_world,
        "provenance_scope":"Caller must supply the original experiment scene/world; historical embedded captures do not independently record their world. Preserve exact input hashes with the result. Forces are privileged simulated observations, not deployed sensors."});
    if batch {
        result["reports"] = json!(reports);
        result["sample_series"] = json!(series);
    } else {
        result["requirements"] = reports[0]["requirements"].clone();
        result["report"] = reports[0]["report"].clone();
        result["samples"] = json!(series[&requirements[0].swing_link]);
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
