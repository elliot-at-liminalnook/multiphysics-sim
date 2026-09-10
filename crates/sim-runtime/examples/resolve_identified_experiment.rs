//! Freeze an experiment's existing CAD identification into explicit values.
//! Uses the same library resolution as assembly, retaining the original fit as
//! experimental source evidence. This is not a new fit or a CAD calibration.
use serde_json::json;
use sim_runtime::experiment::ExperimentSpec;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err(
            "usage: resolve_identified_experiment input-spec.json fresh-explicit-spec.json".into(),
        );
    }
    let mut spec: ExperimentSpec = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let identification = spec.scene.robot.identification.clone();
    if identification.is_empty() {
        return Err("input has no identification to resolve".into());
    }
    spec.scene.robot.apply_identification();
    spec.scene.robot.identification.clear();
    let source = &mut spec.scene.robot.source;
    if !source.is_object() {
        *source = json!({"original_source":source.take()});
    }
    if source.get("resolved_identification").is_some() {
        return Err("source already contains a resolved identification receipt".into());
    }
    source["resolved_identification"] = json!({
        "version":1,"input_experiment":args[0],"identification":identification,
        "operation":"PhysicalModel::apply_identification, once; clear active identification to avoid applying it again",
        "scope":"Experimental explicit values, with original fit evidence retained. Calibration quality is unchanged. Promote accepted fitted values through CAD identification authoring."});
    sim_runtime::experiment::Experiment::bind(spec.clone())?;
    let file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&args[1])?;
    let mut writer = std::io::BufWriter::new(file);
    serde_json::to_writer(&mut writer, &spec)?;
    std::io::Write::flush(&mut writer)?;
    Ok(())
}
