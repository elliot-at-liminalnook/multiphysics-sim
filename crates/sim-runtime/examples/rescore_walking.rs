//! Evaluate the new task on immutable recorded physics, without changing inputs
//! or fitting weights to the tested trajectories.
use serde_json::{Value, json};
use sim_runtime::{
    embedded::Config,
    session::{Scene, Session},
    walking_task::{WalkingMonitor, WalkingTaskConfig},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        return Err("usage: rescore_walking capture.json walking-config.json".into());
    }
    let capture: Value = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    if capture["completed"] != true || !capture["error"].is_null() {
        return Err("complete capture required for task rescoring".into());
    }
    if !capture["task"]["walking"].is_null() {
        return Err("capture already has walking rewards; use the original task capture to avoid double-counting".into());
    }
    let config: Config = serde_json::from_value(capture["recording"]["config"].clone())?;
    let scene: Scene = serde_json::from_value(capture["recording"]["scene"].clone())?;
    let session = Session::new(scene, 0)?;
    let walking: WalkingTaskConfig = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let p = config.policy.as_ref().ok_or("policy required")?;
    let period = capture["task"]["period_s"]
        .as_f64()
        .ok_or("task period required")?;
    let mut monitor = WalkingMonitor::new(
        walking.clone(),
        period,
        p.body_feedback
            .as_ref()
            .ok_or("body reference required")?
            .reference_link
            .clone(),
        p.point_feedback
            .as_ref()
            .ok_or("foot references required")?
            .markers
            .iter()
            .map(|m| m.link.clone())
            .collect(),
        &session.robot.art,
    )?;
    let frames = capture["frames"].as_array().ok_or("frames required")?;
    let mut body = 0.;
    let mut steps = 0.;
    let mut heading = 0.;
    let mut outcomes = vec![];
    let mut final_state = None;
    for (i, f) in frames.iter().enumerate() {
        let r = monitor.observe(
            &session.robot.art,
            f,
            if i == 0 { 0. } else { period },
            i + 1 == frames.len(),
        )?;
        body += r.body_reward;
        steps += r.step_reward;
        heading += r.heading.as_ref().map_or(0., |h| h.reward);
        if let Some(o) = &r.outcome {
            outcomes.push(o.clone());
        }
        final_state = Some(r);
    }
    let old: f64 = capture["transitions"]
        .as_array()
        .ok_or("transitions required")?
        .iter()
        .map(|t| t["reward"].as_f64().unwrap())
        .sum();
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"version":1,"config":walking,"original_reward":old,"body_reward":body,"heading_reward":heading,"step_reward":steps,"total_reward":old+body+heading+steps,"outcomes":outcomes,"final":final_state,
        "scope":"Task-only re-evaluation of immutable saved physics. Same shared monitor as the environment. Development audit, not controller training or independent task acceptance."})
        )?
    );
    Ok(())
}
