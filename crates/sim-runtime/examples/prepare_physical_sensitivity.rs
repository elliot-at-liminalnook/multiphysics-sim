//! Prepare explicit parameter perturbations; execute them with the ordinary
//! benchmark host. Robot edits retain the shared Scene's original-input receipt.
use serde::Deserialize;
use serde_json::{Value, json};
use sim_runtime::environment::EnvironmentRecording;
use std::{fs, path::Path};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    version: u32,
    cases: Vec<Case>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    name: String,
    rationale: String,
    edits: Vec<Edit>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Edit {
    target: Target,
    pointer: String,
    factor: f64,
}
#[derive(Clone, Copy, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum Target {
    Robot,
    Config,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 {
        return Err(
            "usage: prepare_physical_sensitivity input-recording.json plan.json fresh-directory"
                .into(),
        );
    }
    let bytes = fs::read(&args[0])?;
    let baseline: EnvironmentRecording = serde_json::from_slice(&bytes)?;
    let plan_bytes = fs::read(&args[1])?;
    let plan: Plan = serde_json::from_slice(&plan_bytes)?;
    if plan.version != 1 || plan.cases.is_empty() {
        return Err("invalid sensitivity plan".into());
    }
    let mut names = std::collections::BTreeSet::new();
    let mut prepared = vec![];
    for case in plan.cases {
        if case.name.is_empty()
            || !case
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            || !names.insert(case.name.clone())
            || case.rationale.trim().is_empty()
            || case.edits.is_empty()
        {
            return Err("case requires a unique safe name, rationale and edits".into());
        }
        let mut record = baseline.clone();
        let mut robot = json!(record.runtime.scene.robot);
        let mut config = json!(record.runtime.config);
        let mut receipts = vec![];
        let mut pointers = std::collections::BTreeSet::new();
        for edit in case.edits {
            if !edit.factor.is_finite()
                || edit.factor <= 0.
                || edit.factor == 1.
                || !edit.pointer.starts_with('/')
                || !pointers.insert(format!("{}:{}", json!(edit.target), edit.pointer))
            {
                return Err("invalid or repeated perturbation".into());
            }
            let document = match edit.target {
                Target::Robot => &mut robot,
                Target::Config => &mut config,
            };
            let slot = document
                .pointer_mut(&edit.pointer)
                .ok_or("missing perturbation field")?;
            let original = slot
                .as_f64()
                .ok_or("perturbation requires a numeric field")?;
            let value = original * edit.factor;
            if !value.is_finite() || value == original {
                return Err("ineffective or overflowing perturbation".into());
            }
            *slot = json!(value);
            receipts.push(json!({"target":edit.target,"pointer":edit.pointer,"original":original,"factor":edit.factor,"value":value}));
        }
        // Assign the parsed edited robot to the existing Scene: serializing that
        // Scene uses RobotInput to retain source values and mark each override.
        record.runtime.scene.robot = serde_json::from_value(robot)?;
        record.runtime.config = serde_json::from_value(config)?;
        let output = serde_json::to_vec(&record)?;
        // The shared boundary validates its generated receipt on readback.
        let _: EnvironmentRecording = serde_json::from_slice(&output)?;
        prepared.push((
            case.name,
            output,
            json!({"rationale":case.rationale,"edits":receipts}),
        ));
    }
    let root = Path::new(&args[2]);
    fs::create_dir(root)?;
    fs::write(root.join("plan.json"), &plan_bytes)?;
    let mut cases = vec![];
    for (name, output, receipt) in prepared {
        let path = root.join(format!("{name}.input.json"));
        fs::write(&path, &output)?;
        cases.push(json!({"name":name,"input":path,"input_blake3":blake3::hash(&output).to_hex().to_string(),"receipt":receipt}));
    }
    let manifest: Value = json!({"version":1,"source":args[0],"source_blake3":blake3::hash(&bytes).to_hex().to_string(),
        "plan_blake3":blake3::hash(&plan_bytes).to_hex().to_string(),"cases":cases,
        "scope":"Planned parameter perturbations, not measured results or accepted CAD edits. Original horizon, actions and seed retained unless explicitly listed. Execute through the shared runtime; rank matched-duration responses and failures before hardware measurement."});
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(())
}
