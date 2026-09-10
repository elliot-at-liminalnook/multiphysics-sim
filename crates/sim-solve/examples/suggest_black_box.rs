//! A stateless experiment selector; no robot physics or execution lives here.
use serde::{Deserialize, Serialize};
use sim_solve::bayesian::{Config, Observation, Problem, initial_design, suggest};
use std::{fs::OpenOptions, io::Write};
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Request {
    problem: Problem,
    observations: Vec<Observation>,
    config: Config,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() == 3 && args[0] == "--design" {
        #[derive(Deserialize, Serialize)]
        #[serde(deny_unknown_fields)]
        struct Design {
            problem: Problem,
            count: usize,
            seed: u64,
        }
        let request: Design = serde_json::from_slice(&std::fs::read(&args[1])?)?;
        let values = initial_design(&request.problem, request.count, request.seed)?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[2])?;
        serde_json::to_writer(
            &mut output,
            &serde_json::json!({"version":1,"request":request,"values":values,"method":"egobox-doe-0.35.1 classic seeded Latin hypercube"}),
        )?;
        output.write_all(b"\n")?;
        output.sync_all()?;
        return Ok(());
    }
    if args.len() != 2 {
        return Err("usage: suggest_black_box request.json fresh-proposal.json".into());
    }
    let bytes = std::fs::read(&args[0])?;
    let request: Request = serde_json::from_slice(&bytes)?;
    if std::path::Path::new(&args[1]).exists() {
        return Err("fresh proposal path required".into());
    }
    let result = suggest(&request.problem, &request.observations, &request.config)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[1])?;
    serde_json::to_writer(
        &mut output,
        &serde_json::json!({"version":1,"request":request,"proposal":result}),
    )?;
    output.write_all(b"\n")?;
    output.sync_all()?;
    Ok(())
}
