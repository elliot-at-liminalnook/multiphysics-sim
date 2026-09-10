//! Serializable ask/tell interface; caller evaluates candidates in shared runtime.
use serde::Deserialize;
use sim_solve::mixed_cem::{Batch, Config, Observation, Problem, State, ask, initialize, tell};
use std::{fs::OpenOptions, io::Write};

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Initialize {
        problem: Problem,
        config: Config,
    },
    Ask {
        problem: Problem,
        config: Config,
        state: State,
    },
    Tell {
        problem: Problem,
        config: Config,
        batch: Batch,
        observations: Vec<Observation>,
    },
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        return Err("usage: mixed_cem request.json fresh-result.json".into());
    }
    let request: Request = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let result = match request {
        Request::Initialize { problem, config } => {
            serde_json::to_value(initialize(&problem, &config)?)?
        }
        Request::Ask {
            problem,
            config,
            state,
        } => serde_json::to_value(ask(&problem, &config, &state)?)?,
        Request::Tell {
            problem,
            config,
            batch,
            observations,
        } => serde_json::to_value(tell(&problem, &config, &batch, &observations)?)?,
    };
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[1])?;
    serde_json::to_writer(&mut file, &result)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}
