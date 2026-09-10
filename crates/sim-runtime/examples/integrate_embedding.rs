//! Headless host for the shared incremental CAD-derived motor session.
use sim_runtime::{
    embedded::{CaptureMode, Config, EmbeddedSession, MAX_ADVANCE_STEPS},
    session::Scene,
};
fn run() -> Result<bool, String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: integrate_embedding scene.json config.json".into());
    }
    let scene: Scene = serde_json::from_slice(&std::fs::read(&args[0]).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let config: Config =
        serde_json::from_slice(&std::fs::read(&args[1]).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let mut session = EmbeddedSession::new(scene, config, 0, CaptureMode::Full)?;
    let completed = (|| -> Result<(), String> {
        while session.remaining_steps() > 0 {
            session.advance(session.remaining_steps().min(MAX_ADVANCE_STEPS))?;
        }
        Ok(())
    })()
    .is_ok();
    println!(
        "{}",
        serde_json::to_string_pretty(&session.report()?).map_err(|e| e.to_string())?
    );
    Ok(completed)
}
fn main() {
    match run() {
        Ok(true) => (),
        Ok(false) => std::process::exit(1),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    }
}
