//! Thin durable host for the reusable physical validation-case preparer.
use sim_runtime::{
    experiment::ExperimentSpec,
    experiment_variants::{self, Case},
};
use std::{
    fs::{self, OpenOptions},
    io::{BufWriter, Write},
    path::Path,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    if a.len() != 3 {
        return Err(
            "usage: prepare_validation_case detailed-spec.json case.json fresh-directory".into(),
        );
    }
    let spec: ExperimentSpec = serde_json::from_slice(&fs::read(&a[0])?)?;
    let case: Case = serde_json::from_slice(&fs::read(&a[1])?)?;
    let result = experiment_variants::prepare(&spec, &case)?;
    fs::create_dir(&a[2])?;
    for (name, value) in [
        ("spec.json", serde_json::to_value(&result.spec)?),
        (
            "preparation.json",
            serde_json::json!({"case":result.case,"source_spec_blake3":result.source_spec_blake3,"prepared_spec_blake3":result.prepared_spec_blake3,"preparer_runtime":result.preparer_runtime,"source_duration_s":result.source_duration_s,"added_action_intervals":result.added_action_intervals,"edits":result.edits,"scope":result.scope}),
        ),
    ] {
        let f = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(Path::new(&a[2]).join(name))?;
        let mut w = BufWriter::new(f);
        serde_json::to_writer(&mut w, &value)?;
        w.flush()?;
        w.get_ref().sync_all()?;
    }
    println!("Validation case prepared; not run");
    Ok(())
}
