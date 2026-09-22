//! Prepare a timestep-sensitivity experiment without changing physical inputs.
use sim_runtime::{experiment::ExperimentSpec, experiment_variants};
use std::{
    fs,
    io::{BufWriter, Write},
    path::Path,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    if a.len() != 3 {
        return Err("usage: prepare_timestep_case spec.json divisor fresh-directory".into());
    }
    let source: ExperimentSpec = serde_json::from_slice(&fs::read(&a[0])?)?;
    let result = experiment_variants::refine_timestep(&source, a[1].parse()?)?;
    fs::create_dir(&a[2])?;
    for (name, value) in [
        ("spec.json", serde_json::to_value(&result.spec)?),
        (
            "preparation.json",
            serde_json::json!({"divisor":result.divisor,"source_step_s":result.source_step_s,
        "refined_step_s":result.refined_step_s,"source_spec_blake3":result.source_spec_blake3,
        "prepared_spec_blake3":result.prepared_spec_blake3,"preparer_runtime":result.preparer_runtime,
        "scope":"Numerical clock refinement only; no simulation result"}),
        ),
    ] {
        let f = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(Path::new(&a[2]).join(name))?;
        let mut w = BufWriter::new(f);
        serde_json::to_writer(&mut w, &value)?;
        w.flush()?;
        w.get_ref().sync_all()?;
    }
    println!("Timestep case prepared; not run");
    Ok(())
}
