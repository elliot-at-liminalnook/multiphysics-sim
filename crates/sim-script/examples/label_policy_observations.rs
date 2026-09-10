//! Query a stateful Rhai teacher on recorded learner observations.
//! Labels are counterfactual advice, never substituted for measured actions.
use serde_json::{json, Value};
use sim_core::{Channel, Contract, Coupler};
use sim_script::{parameter_map, RhaiController};
use std::{error::Error, fs};

fn finite(value: &Value) -> Result<f64, Box<dyn Error>> {
    value.as_f64().filter(|x| x.is_finite()).ok_or("finite value required".into())
}
fn channels(value: &Value) -> Result<Vec<Channel>, Box<dyn Error>> {
    value.as_array().ok_or("channel array required")?.iter().map(|v| {
        let c: Channel = serde_json::from_value(v.clone())?;
        if v["unit"].as_str() != Some(c.unit()) { return Err("channel unit mismatch".into()); }
        Ok(c)
    }).collect()
}
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 { return Err("usage: label_policy_observations capture.json teacher-controller.json > labels.json".into()); }
    let capture: Value = serde_json::from_slice(&fs::read(&args[0])?)?;
    if capture["completed"] != true || !capture["error"].is_null() { return Err("complete capture required".into()); }
    let program: Value = serde_json::from_slice(&fs::read(&args[1])?)?;
    let meta = &capture["metadata"]["policy_contract"];
    let contract = Contract {
        element: "embedded.policy".into(), period: finite(&meta["period_s"])?,
        sensors: channels(&meta["observations"])?, actuators: channels(&meta["actuators"])?,
    };
    if contract.period <= 0.0 { return Err("positive period required".into()); }
    let bounds: Vec<[f64; 2]> = serde_json::from_value(meta["software_target_bounds_rad"].clone())?;
    if bounds.len() != contract.actuators.len() || bounds.iter().any(|b| !b[0].is_finite() || !b[1].is_finite() || b[0] >= b[1]) {
        return Err("finite actuator command bounds required".into());
    }
    let mut teacher = RhaiController::with_seed(
        serde_json::from_value(program["sources"].clone())?,
        parameter_map(&program["parameters"]).map_err(|e| e.to_string())?,
        capture["recording"]["seed"].as_u64().ok_or("seed required")?,
    ).map_err(|e| e.to_string())?;
    teacher.open(&contract)?;
    let frames = capture["frames"].as_array().ok_or("frames required")?;
    let initial = frames.first().ok_or("initial frame required")?;
    if finite(&initial["time_s"])? != 0.0 { return Err("initial frame must be at zero".into()); }
    let mut commands: Vec<f64> = serde_json::from_value(initial["servo_targets_rad"].clone())?;
    if commands.len() != contract.actuators.len() { return Err("command dimensions differ".into()); }
    let mut samples = Vec::new();
    let mut saturated = 0usize;
    let mut maximum_difference = 0.0_f64;
    for frame in frames {
        let p = &frame["policy"];
        if p.is_null() { continue; }
        let time = finite(&p["time_s"])?;
        if (time - samples.len() as f64 * contract.period).abs() > 1e-9 || time >= finite(&frame["time_s"])? {
            return Err("missing, repeated or misaligned policy observation".into());
        }
        let observations = contract.sensors.iter().map(|c| finite(&p["observations"][&c.name])).collect::<Result<Vec<_>, _>>()?;
        teacher.sample(time, &observations, &mut commands)?;
        let requested = commands.clone();
        for (i, command) in commands.iter_mut().enumerate() {
            if !command.is_finite() { return Err("nonfinite teacher output".into()); }
            let bounded = command.clamp(bounds[i][0], bounds[i][1]);
            saturated += usize::from(bounded != *command);
            *command = bounded;
            maximum_difference = maximum_difference.max((*command - finite(&p["targets"][&contract.actuators[i].name])?).abs());
        }
        samples.push(json!({"policy_time_s":time,"requested_targets_rad":requested,"targets_rad":commands}));
    }
    if samples.is_empty() || (samples.len() as f64 * contract.period - finite(&frames.last().unwrap()["time_s"])?).abs() > 1e-9 {
        return Err("incomplete policy observation sequence".into());
    }
    println!("{}", serde_json::to_string(&json!({"version":1,"source_capture":args[0],"teacher_controller":args[1],
        "actuators":contract.actuators,"samples":samples,"saturated_commands":saturated,
        "maximum_difference_from_applied_commands_rad":maximum_difference,
        "scope":"Stateful Rhai teacher queried on the learner's recorded observations, with explicit saturation at recorded actuator command bounds. Both raw and bounded advice retained. These labels were not physically executed and do not establish recovery or speed."}))?);
    Ok(())
}
