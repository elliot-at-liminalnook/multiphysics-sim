//! Audit an archived implicit linearization without repeating its warmup.
//! Usage: check_captured_stage SCENE POINT [CHECK_CONFIG]
//! POINT is {island, solve: ImplicitAttempt}. Scene initial external inputs
//! must reproduce the recorded residual exactly or the shared checker refuses.
use serde::Deserialize;
use sim_dynamics::{
    jacobian_check::{check_implicit_jacobian, CheckConfig},
    ImplicitAttempt,
};
use sim_runtime::{
    session::{Scene, Session},
    validation::state_labels,
};

#[derive(Deserialize)]
struct Point {
    #[serde(default)]
    seed: u64,
    island: usize,
    solve: ImplicitAttempt,
}
fn read<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, String> {
    serde_json::from_str(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
fn run() -> Result<bool, String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() < 2 || args.len() > 3 {
        return Err("usage: check_captured_stage SCENE POINT [CHECK_CONFIG]".into());
    }
    let point: Point = read(&args[1])?;
    let session = Session::new(read::<Scene>(&args[0])?, point.seed)?;
    let config: CheckConfig = match args.get(2) {
        Some(path) => read(path)?,
        None => Default::default(),
    };
    let island = session
        .robot
        .runtime
        .islands
        .get(point.island)
        .ok_or("invalid island")?;
    let report = check_implicit_jacobian(&island.system, &point.solve, &config)?;
    let names = state_labels(&session.robot.runtime);
    let coordinates: Vec<_> = island
        .system
        .full_of
        .iter()
        .map(|f| &names[&island.system.state_ids[*f]])
        .collect();
    println!("{}",serde_json::to_string_pretty(&serde_json::json!({"island":point.island,
        "stage_time":point.solve.stage_time,"step_s":point.solve.step,"coordinates":coordinates,
        "config":config,"report":report,"notes":[
            "Recorded base residual reproduced bit for bit using scene initial external inputs.",
            "Checks the actual increment residual at the recorded last fresh matrix point, not the terminal iterate.",
            "The scene must retain the capture's coordinate contract; this does not replay hidden external context.",
            "Inconclusive derivatives do not pass the gate. No solver policy or physics configuration is changed."
        ]})).map_err(|e|e.to_string())?);
    Ok(report.passed)
}
fn main() {
    match run() {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2)
        }
    }
}
