//! Whole-state and trajectory replay acceptance using the shared evaluator.
use serde_json::{Value, json};
use sim_runtime::experiment::{Experiment, ExperimentSpec, Status};
fn stable(mut value: Value) -> Value {
    value.as_object_mut().unwrap().remove("stepping_wall_s");
    value
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: check_motion_experiment spec.json fresh-report.json".into());
    }
    let spec: ExperimentSpec = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let count = spec.source_actions.len();
    if count < 2 {
        return Err("replay acceptance needs at least two task intervals".into());
    }
    let experiment = Experiment::bind(spec)?;
    let proposal = experiment.propose(
        experiment.spec.baseline.clone(),
        "acceptance_baseline".into(),
    )?;
    let mut full = experiment.start(proposal.clone())?;
    let mut frames = vec![stable(full.frame()?)];
    for _ in 0..count {
        full.advance(1)?;
        frames.push(stable(full.frame()?));
    }
    if full.status() != Status::Complete {
        return Err("baseline did not complete the acceptance horizon".into());
    }
    let final_checkpoint = full.checkpoint()?;
    let robot_input = full.metadata()["environment"]["robot_input"].clone();
    let robot_inspection = if experiment.spec.scene.input_binding()?.input.is_some() {
        Some(sim_runtime::robot_contract::inspect(serde_json::to_value(
            &experiment.spec.scene,
        )?)?)
    } else {
        None
    };
    let mut partial = experiment.start(proposal.clone())?;
    let split = count / 2;
    partial.advance(split)?;
    let checkpoint = partial.checkpoint()?;
    let serialized = serde_json::to_vec(&checkpoint)?;
    let mut resumed = experiment.resume(serde_json::from_slice(&serialized)?)?;
    let mut replayed = 0;
    while resumed.status() == Status::Replaying {
        resumed.advance(1)?;
        replayed += 1;
        if stable(resumed.frame()?) != frames[replayed] {
            return Err("replayed prefix physical frame differs".into());
        }
    }
    if replayed != split {
        return Err("unexpected replay prefix length".into());
    }
    for frame in frames.iter().skip(split + 1) {
        resumed.advance(1)?;
        if stable(resumed.frame()?) != *frame {
            return Err("resumed trajectory differs".into());
        }
    }
    if serde_json::to_value(resumed.checkpoint()?)? != serde_json::to_value(&final_checkpoint)? {
        return Err("resumed final outcome differs".into());
    }
    let mut file = std::io::BufWriter::new(
        std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&args[1])?,
    );
    serde_json::to_writer(
        &mut file,
        &json!({"version":1,"passed":true,"experiment":experiment,"proposal":proposal,
        "partial":checkpoint,"full":final_checkpoint,"frames":frames,"split":split,"replayed_actions":replayed,
        "robot_input":robot_input,"robot_inspection":robot_inspection,
        "all_replayed_and_resumed_frames_exact":true,"scope":"Short shared evaluator/replay acceptance. Not a speed optimum or physical calibration."}),
    )?;
    std::io::Write::flush(&mut file)?;
    Ok(())
}
