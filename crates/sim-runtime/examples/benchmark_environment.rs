//! Stream ordinary environment transitions for sustained and command-response
//! benchmarks. Physics, observation, command and replay semantics stay in Rust's
//! shared environment; this host only manages files, progress and cancellation.
use serde_json::json;
use sim_runtime::environment::{EmbeddedEnvironment, EnvironmentRecording};
use std::{
    fs,
    io::{BufWriter, Write},
    path::Path,
    time::Instant,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if !(2..=4).contains(&args.len()) {
        return Err("usage: benchmark_environment input-recording.json fresh-output-directory [prefix-seconds] [cancel-file]".into());
    }
    let bytes = fs::read(&args[0])?;
    let record: EnvironmentRecording = serde_json::from_slice(&bytes)?;
    let period = record.task.period_s;
    let horizon = record.runtime.config.steps as f64 * record.runtime.config.step_s;
    let requested = args
        .get(2)
        .map(|v| v.parse::<f64>())
        .transpose()?
        .unwrap_or(horizon);
    let intervals = requested / period;
    if !requested.is_finite()
        || requested <= 0.
        || requested > horizon
        || !intervals.is_finite()
        || (intervals - intervals.round()).abs() > 1e-8
    {
        return Err(
            "requested duration must be positive, within the episode and on the task grid".into(),
        );
    }
    let start = Instant::now();
    let loaded = EmbeddedEnvironment::new(
        record.runtime.scene.clone(),
        record.runtime.config.clone(),
        record.task.clone(),
        record.runtime.seed,
    )?;
    let (mut env, actions) = loaded.prepare_replay(record)?;
    drop(loaded);
    let count = intervals.round() as usize;
    if count > actions.len() {
        return Err("source recording does not cover the requested duration".into());
    }
    let output = Path::new(&args[1]);
    fs::create_dir(output)?;
    let write_json =
        |name: &str, value: &serde_json::Value| -> Result<(), Box<dyn std::error::Error>> {
            let file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(output.join(name))?;
            let mut writer = BufWriter::new(file);
            serde_json::to_writer(&mut writer, value)?;
            writer.write_all(b"\n")?;
            writer.flush()?;
            Ok(())
        };
    write_json(
        "metadata.json",
        &json!({"version":1,"input_path":args[0],
        "input_blake3":blake3::hash(&bytes).to_hex().to_string(),
        "requested_duration_s":requested,"episode_duration_s":horizon,
        "metadata":env.metadata(),"contract":env.contract(),"task":env.task(),
        "construction_wall_s":start.elapsed().as_secs_f64(),
        "scope":"Unchanged production environment. Transition observations retain declared units and timestamps; finite differences are not extra physics states."}),
    )?;
    let mut trace = BufWriter::new(
        fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(output.join("transitions.jsonl"))?,
    );
    serde_json::to_writer(
        &mut trace,
        &json!({"action":null,"transition":env.transition()}),
    )?;
    trace.write_all(b"\n")?;
    trace.flush()?;
    let stepping = Instant::now();
    let mut heartbeat = Instant::now();
    let mut stop = "requested_prefix";
    let mut error = None;
    let mut completed = 0;
    for action in actions.iter().take(count) {
        if args.get(3).is_some_and(|p| Path::new(p).exists()) {
            stop = "cancelled";
            break;
        }
        match env.step(action) {
            Ok(transition) => {
                completed += 1;
                serde_json::to_writer(
                    &mut trace,
                    &json!({"action":action,"transition":transition}),
                )?;
                trace.write_all(b"\n")?;
                if transition.terminated {
                    stop = "task_terminated";
                    break;
                }
                if transition.truncated {
                    stop = "episode_complete";
                    break;
                }
            }
            Err(e) => {
                stop = "simulation_error";
                error = Some(e);
                break;
            }
        }
        if heartbeat.elapsed().as_secs_f64() >= 5. {
            trace.flush()?;
            eprintln!(
                "{}",
                json!({"simulated_s":env.transition().time_s,"requested_s":requested,
                "wall_s":stepping.elapsed().as_secs_f64(),"completed_actions":completed})
            );
            heartbeat = Instant::now();
        }
    }
    trace.flush()?;
    let wall_s = stepping.elapsed().as_secs_f64();
    let transition = env.transition();
    let complete = stop == "episode_complete" && error.is_none() && !transition.terminated;
    let speed = if complete {
        transition
            .speed
            .as_ref()
            .map(|s| s.net_speed_m_s)
            .or_else(|| transition.progress.as_ref().map(|p| p.net_speed_m_s))
    } else {
        None
    };
    write_json(
        "summary.json",
        &json!({"version":1,"stop":stop,"error":error,
        "requested_steps_completed":completed==count,"episode_complete":complete,
        "eligible_net_speed_m_s":speed,"completed_actions":completed,"final_transition":transition,
        "stepping_and_streaming_wall_s":wall_s,"simulated_seconds_per_wall_second":transition.time_s/wall_s,
        "scope":"Eligibility requires the complete original horizon without task termination or numerical failure. Prefixes and cancellations remain diagnostics. Streaming cost is included; no hardware or realtime qualification."}),
    )?;
    write_json(
        "recording.json",
        &serde_json::to_value(env.episode_recording())?,
    )?;
    if error.is_some() || stop == "task_terminated" {
        return Err(format!("benchmark ended with {stop}; saved diagnostic evidence").into());
    }
    eprintln!(
        "{}",
        json!({"stop":stop,"completed_actions":completed,"eligible_net_speed_m_s":speed,"wall_s":wall_s})
    );
    Ok(())
}
