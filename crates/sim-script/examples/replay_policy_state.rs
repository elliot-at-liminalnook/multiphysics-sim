//! Recover diagnostic state by replaying every saved policy observation.
//! Refuses missing samples, inconsistent units, and changed actuator commands.
use serde_json::{Value, json};
use sim_core::{Channel, Contract, Coupler};
use sim_script::{RhaiController, parameter_map};
use std::{error::Error, fs};

fn number(v: &Value) -> Result<f64, Box<dyn Error>> {
    v.as_f64()
        .filter(|n| n.is_finite())
        .ok_or("finite number required".into())
}
fn channels(v: &Value) -> Result<Vec<Channel>, Box<dyn Error>> {
    v.as_array()
        .ok_or("channel array required")?
        .iter()
        .map(|v| {
            let c: Channel = serde_json::from_value(v.clone())?;
            if v["unit"].as_str() != Some(c.unit()) {
                return Err("channel unit mismatch".into());
            }
            Ok(c)
        })
        .collect()
}
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 2 {
        return Err("usage: replay_policy_state capture.json > states.json".into());
    }
    let c: Value = serde_json::from_slice(&fs::read(&args[1])?)?;
    let environment = c["kind"] == "sampled_environment_capture";
    let requested_complete = if environment { &c["requested_steps_completed"] } else { &c["completed"] };
    if requested_complete != true || !c["error"].is_null() {
        return Err("complete requested capture required".into());
    }
    let recording = c["recording"].get("runtime").unwrap_or(&c["recording"]);
    if !recording["config"]["policy"]["neural_residual"].is_null() {
        return Err("neural residual replay is not supported".into());
    }
    let meta = &c["metadata"]["policy_contract"];
    let contract = Contract {
        element: "embedded.policy".into(),
        period: number(&meta["period_s"])?,
        sensors: channels(&meta["observations"])?,
        actuators: channels(&meta["actuators"])?,
    };
    if contract.period <= 0.0 {
        return Err("positive policy period required".into());
    }
    let program = &recording["scene"]["controller"];
    let mut policy = RhaiController::with_seed(
        serde_json::from_value(program["sources"].clone())?,
        parameter_map(&program["parameters"]).map_err(|e| e.to_string())?,
        recording["seed"].as_u64().ok_or("seed required")?,
    )
    .map_err(|e| e.to_string())?;
    policy.open(&contract)?;
    let frames = c["frames"].as_array().ok_or("frames required")?;
    let initial = frames.first().ok_or("initial frame required")?;
    if number(&initial["time_s"])? != 0.0 {
        return Err("initial frame must be at zero".into());
    }
    let mut commands: Vec<f64> = serde_json::from_value(initial["servo_targets_rad"].clone())?;
    if commands.len() != contract.actuators.len() {
        return Err("initial command size mismatch".into());
    }
    let mut result = Vec::new();
    let mut max_error = 0.0_f64;
    let mut last_time = None;
    let mut last_policy = None;
    for f in frames {
        let p = &f["policy"];
        if p.is_null() {
            continue;
        }
        let t = number(&p["time_s"])?;
        if last_time == Some(t) {
            if last_policy != Some(p) {
                return Err("conflicting repeated policy sample".into());
            }
            continue;
        }
        let expected = result.len() as f64 * contract.period;
        if (t - expected).abs() > 1e-9 {
            return Err(format!(
                "missing/out-of-order policy sample: expected {expected}, got {t}"
            )
            .into());
        }
        let sensed = contract
            .sensors
            .iter()
            .map(|ch| number(&p["observations"][&ch.name]))
            .collect::<Result<Vec<_>, _>>()?;
        policy.sample(t, &sensed, &mut commands)?;
        for (ch, actual) in contract.actuators.iter().zip(&commands) {
            let recorded = number(&p["targets"][&ch.name])?;
            let error = (actual - recorded).abs();
            max_error = max_error.max(error);
            if error > 1e-12 {
                return Err(format!("command mismatch at {t}: {} error {error}", ch.name).into());
            }
        }
        result.push(json!({"policy_time_s":t,"frame_time_s":f["time_s"],"state":policy.state_json().map_err(|e| e.to_string())?}));
        last_time = Some(t);
        last_policy = Some(p);
    }
    if result.is_empty() {
        return Err("no policy samples".into());
    }
    if (result.len() as f64 * contract.period - number(&frames.last().unwrap()["time_s"])?).abs()
        > 1e-9
    {
        return Err("capture ends without a complete sequence of policy samples".into());
    }
    println!(
        "{}",
        serde_json::to_string(
            &json!({"version":1,"source_capture":args[1],"samples":result,
        "maximum_command_error_rad":max_error,"command_tolerance_rad":1e-12,
        "source_prefix_only":environment && c["completed"] != true,
        "scope":"Exact Rhai replay of every sampled observation; state corresponds to the command held until the next policy sample."})
        )?
    );
    Ok(())
}
