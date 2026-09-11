//! Offline measurement of a benchmark stream through shared typed response APIs.
use serde::Deserialize;
use serde_json::{Value, json};
use sim_domain_control::displacement::DisplacementAxes;
use sim_runtime::{
    environment::{Axis, Task, Transition},
    motion_response::{BodyBinding, summarize},
};
use std::{
    fs,
    io::{BufRead, BufReader},
    path::Path,
};

#[derive(Deserialize)]
struct Row {
    action: Option<Vec<f64>>,
    transition: Transition,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if !(3..=4).contains(&args.len()) || args.get(3).is_some_and(|a| a != "--prefix") {
        return Err("usage: analyze_command_response benchmark-directory command-spec.json fresh-report.json [--prefix]".into());
    }
    let root = Path::new(&args[0]);
    let meta: Value = serde_json::from_slice(&fs::read(root.join("metadata.json"))?)?;
    let task: Task = serde_json::from_value(meta["task"].clone())?;
    let link = task
        .speed
        .as_ref()
        .map(|s| s.body_link.as_str())
        .or_else(|| task.progress.as_ref().map(|p| p.link.as_str()))
        .ok_or("task has no progress body")?;
    let binding = BodyBinding::new(&task, link, Some(Axis::X))?;
    let axes = task
        .progress
        .as_ref()
        .map_or(DisplacementAxes::Xy, |p| p.axes);
    let spec_bytes = fs::read(&args[1])?;
    let spec: Value = serde_json::from_slice(&spec_bytes)?;
    let input_bytes = fs::read(meta["input_path"].as_str().ok_or("missing input path")?)?;
    if meta["input_blake3"] != blake3::hash(&input_bytes).to_hex().to_string() {
        return Err("capture input hash does not match preserved input".into());
    }
    let input: Value = serde_json::from_slice(&input_bytes)?;
    let input_task: Task = serde_json::from_value(input["task"].clone())?;
    if json!(input_task) != json!(task)
        || spec["duration_s"].as_f64() != meta["episode_duration_s"].as_f64()
    {
        return Err("task or protocol duration does not match capture".into());
    }
    let preset: Value = serde_json::from_slice(&fs::read(
        spec["preset"].as_str().ok_or("missing preset path")?,
    )?)?;
    let channels = input["runtime"]["scene"]["controller"]["inputs"]
        .as_array()
        .ok_or("missing inputs")?;
    let motion_indices: Vec<usize> = preset["motion_commands"]
        .as_array()
        .ok_or("missing motion channels")?
        .iter()
        .map(|name| {
            channels
                .iter()
                .position(|c| c["name"] == *name)
                .ok_or("unknown motion channel")
        })
        .collect::<Result<_, _>>()?;
    let events = input["runtime"]["input_events"]
        .as_array()
        .ok_or("missing input events")?;
    let dt = input["runtime"]["config"]["step_s"]
        .as_f64()
        .ok_or("missing physics step")?;
    let stride = (task.period_s / dt).round() as usize;
    let authored_stages = spec["stages"].as_array().ok_or("missing stages")?;
    let mut previous_end = 0.;
    for stage in authored_stages {
        let start = stage["start_s"].as_f64().ok_or("missing start")?;
        let end = stage["end_s"].as_f64().ok_or("missing end")?;
        if start != previous_end
            || end <= start
            || (start / task.period_s - (start / task.period_s).round()).abs() > 1e-8
            || (end / task.period_s - (end / task.period_s).round()).abs() > 1e-8
        {
            return Err("protocol stages must be contiguous and on the task grid".into());
        }
        previous_end = end;
        let requested = stage["requested_motion"]
            .as_array()
            .ok_or("missing requested motion")?;
        if requested.len() != motion_indices.len() {
            return Err("motion dimension mismatch".into());
        }
        for i in (start / task.period_s).round() as usize..(end / task.period_s).round() as usize {
            let event = events.get(i).ok_or("protocol exceeds scheduled input")?;
            if event["at_step"].as_u64() != Some((i * stride) as u64)
                || motion_indices
                    .iter()
                    .zip(requested)
                    .any(|(j, v)| event["values"][*j].as_f64() != v.as_f64())
            {
                return Err("protocol differs from captured input schedule".into());
            }
        }
    }
    if Some(previous_end) != spec["duration_s"].as_f64() {
        return Err("stages do not cover protocol".into());
    }
    let mut reader = BufReader::new(fs::File::open(root.join("transitions.jsonl"))?);
    let mut samples = vec![];
    let mut last = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        if !line.ends_with('\n') {
            if args.len() == 4 {
                break;
            } else {
                return Err(
                    "unfinished stream line; use explicit --prefix for an active run".into(),
                );
            }
        }
        let row: Row = serde_json::from_str(&line)?;
        let index = samples.len();
        if row.transition.completed_steps != index * stride
            || (row.transition.time_s - index as f64 * task.period_s).abs() > 1e-8
            || last
                .as_ref()
                .is_some_and(|t: &Transition| t.terminated || t.truncated)
        {
            return Err(
                "transition stream has a gap, reordered sample or post-terminal row".into(),
            );
        }
        if index == 0 {
            if row.action.is_some() {
                return Err("initial sample must have no action".into());
            }
        } else if row.action.as_ref()
            != Some(&serde_json::from_value::<Vec<f64>>(
                events.get(index - 1).ok_or("extra action")?["values"].clone(),
            )?)
        {
            return Err("observed action differs from preserved command input".into());
        }
        samples.push(binding.sample(&row.transition)?);
        last = Some(row.transition);
    }
    let last = last.ok_or("empty transition stream")?;
    let summary_path = root.join("summary.json");
    let terminal: Option<Value> = if summary_path.exists() {
        Some(serde_json::from_slice(&fs::read(summary_path)?)?)
    } else {
        None
    };
    if terminal
        .as_ref()
        .is_some_and(|s| s["final_transition"] != json!(last))
    {
        return Err("terminal summary differs from last stream sample".into());
    }
    let protocol_complete = last.truncated
        && !last.terminated
        && (last.time_s - previous_end).abs() < 1e-8
        && terminal
            .as_ref()
            .is_some_and(|s| s["episode_complete"] == true && s["error"].is_null());
    if !protocol_complete && args.len() != 4 {
        return Err(
            "protocol is not complete; explicit --prefix required for diagnostic analysis".into(),
        );
    }
    let mut stages = vec![];
    for stage in spec["stages"].as_array().ok_or("missing stages")? {
        let start = stage["start_s"].as_f64().ok_or("missing stage start")?;
        let end = stage["end_s"].as_f64().ok_or("missing stage end")?;
        if !start.is_finite() || !end.is_finite() || end <= start {
            return Err("invalid stage interval".into());
        }
        let selected: Vec<_> = samples
            .iter()
            .filter(|s| s.time_s >= start - 1e-9 && s.time_s <= end + 1e-9)
            .cloned()
            .collect();
        if selected.len() < 2 {
            continue;
        }
        if (selected[0].time_s - start).abs() > 1e-9 {
            return Err("stage start is missing from samples".into());
        }
        let measurements = summarize(&selected, axes)?;
        let tail_start = (measurements.end_s - 2.).max(start);
        let tail: Vec<_> = selected
            .iter()
            .filter(|s| s.time_s >= tail_start - 1e-9)
            .cloned()
            .collect();
        stages.push(json!({"name":stage["name"],"keys":stage["keys"],"requested_motion":stage["requested_motion"],
            "requested_end_s":end,"stage_complete":(measurements.end_s-end).abs()<1e-9,
            "measurements":measurements,"last_two_seconds":summarize(&tail,axes)?}));
    }
    let report = json!({"version":1,"input":meta["input_path"],"input_blake3":meta["input_blake3"],
        "physics_context":meta["metadata"]["physics_context"],"protocol_complete":protocol_complete,
        "command_spec_blake3":blake3::hash(&spec_bytes).to_hex().to_string(),
        "verified_action_count":samples.len()-1,
        "terminated":last.terminated,"last_observed_s":last.time_s,"stages":stages,
        "scope":"Read-only observed response; no reward or failure thresholds added. Acceleration is interval-average velocity difference. Orientation unwrapping assumes less than pi true rotation per sample; integrated heading rate is a separate quadrature diagnostic and can drift at the observation cadence. Tail averages use the last two observed seconds. Prefixes do not establish full-protocol success."});
    let file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&args[2])?;
    serde_json::to_writer_pretty(file, &report)?;
    Ok(())
}
