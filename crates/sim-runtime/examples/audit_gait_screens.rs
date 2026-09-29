//! Re-prepare recorded gait-search attempts with the current preparation
//! screens and report which stage rejects each one and how long it takes,
//! against the recorded outcome. No dynamic trial is run.
//!
//!     cargo run --release -p sim-runtime --example audit_gait_screens -- \
//!         comparison-config.json study-dir [coarse-factor coarse-margin] [threads] > audit.json
use serde_json::{Value, json};
use sim_runtime::contact_exploration::{self, CoarseScreen};
use std::{path::Path, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        return Err("usage: audit_gait_screens config.json study-dir [coarse-factor coarse-margin] [threads]".into());
    }
    let config: Value = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let mut recipe: contact_exploration::Recipe = serde_json::from_value(config["recipe"].clone())?;
    recipe.sync_actuators(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))?;
    if let (Some(f), Some(m)) = (args.get(2), args.get(3)) {
        recipe.coarse_speed_screen = Some(CoarseScreen { factor: f.parse()?, margin: m.parse()? });
    }
    let threads: usize = args.get(4).map(|t| t.parse()).transpose()?.unwrap_or(8);
    let mut cases = Vec::new();
    for dir in ["comparison", "comparison-independent-seed"] {
        let root = Path::new(&args[1]).join(dir);
        if !root.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(root)? {
            let path = entry?.path().join("trial.json");
            if path.is_file() {
                let trial: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
                cases.push((path.parent().unwrap().display().to_string(), trial));
            }
        }
    }
    cases.sort_by(|a, b| a.0.cmp(&b.0));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let Some((dir, trial)) = cases.get(i) else { break };
                let values: Vec<f64> = serde_json::from_value(trial["observation"]["values"].clone()).unwrap();
                let named = recipe.template.space.named_values(&values).unwrap();
                let t = Instant::now();
                let schedule = recipe.schedule_screen(&named);
                let schedule_s = t.elapsed().as_secs_f64();
                let t = Instant::now();
                let prepared = if schedule.is_ok() { Some(recipe.prepare(&named)) } else { None };
                let prepare_s = t.elapsed().as_secs_f64();
                let now = match (&schedule, &prepared) {
                    (Err(e), _) => json!({"stage":"schedule","reason":e}),
                    (Ok(()), Some(Err(e))) => json!({"stage":if e.starts_with("coarse") {"coarse"} else {"preparation"},"reason":e}),
                    _ => json!({"stage":"passed"}),
                };
                results.lock().unwrap().push(json!({
                    "attempt": dir,
                    "recorded": {"status": trial["observation"]["outcome"]["status"], "reason": trial["observation"]["outcome"]["reason"], "preparation_wall_s": trial["preparation_wall_s"]},
                    "now": now, "schedule_s": schedule_s, "preparation_s": prepare_s,
                }));
            });
        }
    });
    let mut results = results.into_inner()?;
    results.sort_by(|a, b| a["attempt"].as_str().cmp(&b["attempt"].as_str()));
    println!("{}", serde_json::to_string_pretty(&results)?);
    Ok(())
}
