//! Convert a completed compact stream into the existing forecast-training format.
//! Original capture provenance is retained, never replaced by this tool's runtime.
use serde_json::{Value, json};
use sim_runtime::{
    environment::EnvironmentRecording, motion_data::MotionSnapshot, physics_context::PhysicsContext,
};
use std::{
    fs,
    io::{BufRead, BufReader, BufWriter, Write},
    path::Path,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: pack_motion_capture benchmark-directory fresh-capture.json".into());
    }
    let root = Path::new(&args[0]);
    let read = |name: &str| -> Result<Value, Box<dyn std::error::Error>> {
        Ok(serde_json::from_slice(&fs::read(root.join(name))?)?)
    };
    let meta = read("metadata.json")?;
    let summary = read("summary.json")?;
    let record: EnvironmentRecording = serde_json::from_value(read("recording.json")?)?;
    if meta["motion_stream"] != true
        || !summary["error"].is_null()
        || record.error.is_some()
        || !matches!(
            summary["stop"].as_str(),
            Some("episode_complete" | "requested_prefix" | "task_terminated")
        )
    {
        return Err(
            "requires completed motion capture without numerical failure or cancellation".into(),
        );
    }
    let recorded_context = PhysicsContext::from_recording(&record.runtime)?;
    recorded_context.matches(&serde_json::from_value(
        meta["metadata"]["physics_context"].clone(),
    )?)?;
    let mut frames = vec![];
    let mut reader = BufReader::new(fs::File::open(root.join("motion.jsonl"))?);
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        if !line.ends_with('\n') {
            return Err("unfinished motion line".into());
        }
        let frame: Value = serde_json::from_str(&line)?;
        let motion = MotionSnapshot::from_frame(&frame)?;
        if (motion.time_s - frames.len() as f64 * record.task.period_s).abs() > 1e-8 {
            return Err("motion stream clock gap or reordering".into());
        }
        frames.push(frame);
    }
    let final_time = frames.last().ok_or("empty motion stream")?["time_s"]
        .as_f64()
        .ok_or("missing time")?;
    if frames.len() < 2
        || summary["final_transition"]["time_s"].as_f64() != Some(final_time)
        || (record.runtime.completed_steps as f64 * record.runtime.config.step_s - final_time).abs()
            > 1e-8
        || summary["completed_actions"].as_u64() != Some((frames.len() - 1) as u64)
    {
        return Err("motion stream does not cover terminal recording".into());
    }
    sim_runtime::forecast_actions::from_recording(
        &record.runtime,
        &frames,
        &record
            .runtime
            .scene
            .controller
            .as_ref()
            .ok_or("missing controller")?
            .inputs,
    )?;
    let capture = json!({"error":null,"completed":summary["episode_complete"],"task":record.task,
        "frames":frames,"metadata":meta["metadata"],"recording":record.runtime,
        "benchmark_source":{"directory":args[0],"input_blake3":meta["input_blake3"],"summary":summary},
        "scope":"Lossless motion/held-command/actuator-target fields from completed shared-runtime capture. Original physics identity retained. Prefixes and physical terminations can supply dynamics labels but do not establish sustained success."});
    let file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&args[1])?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer(&mut writer, &capture)?;
    writer.flush()?;
    Ok(())
}
