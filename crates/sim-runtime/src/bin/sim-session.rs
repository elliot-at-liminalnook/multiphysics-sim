//! Native headless host for the same session used by the browser worker.
use sim_runtime::session::{Recording, Scene, Session};
use std::path::Path;
fn write_json(path: &str, value: &impl serde::Serialize) -> Result<(), String> {
    let path = Path::new(path);
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path,serde_json::to_vec(value).map_err(|e|e.to_string())?).map_err(|e|e.to_string())
}
fn run() -> Result<(), String> {
    let profile = std::env::var_os("SIM_SESSION_PROFILE").is_some();
    if profile { sim_solve::profile::enable(); }
    let args: Vec<_> = std::env::args().skip(1).collect();
    let path = args.first().ok_or(
        "usage: sim-session scene.json [frames] [recording.json] [frame-trace.json], or --replay recording.json",
    )?;
    let session = if path == "--replay" {
        let text = std::fs::read_to_string(args.get(1).ok_or("missing recording path")?)
            .map_err(|e| e.to_string())?;
        let recording: Recording = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        Session::replay(recording)?
    } else {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let scene: Scene = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        let mut session = Session::new(scene, 0)?;
        if profile {
            eprintln!("Built {} links, {} actuators, {} unknowns", session.robot.model.links.len(),
                      session.contract.actuators.len(), session.robot.runtime.islands.iter().map(|i| i.state.len()).sum::<usize>());
        }
        let frames: usize = args
            .get(1)
            .map(|s| s.parse().map_err(|_| "invalid frame count".to_string()))
            .transpose()?
            .unwrap_or(1);
        let action: Vec<_> = session.inputs.iter().map(|i| i.initial).collect();
        let mut trace = args.get(3).map(|_|vec![session.frame()]);
        for frame in 0..frames {
            let started = std::time::Instant::now();
            let snapshot = session.step(&action)?;
            if let Some(trace) = &mut trace { trace.push(snapshot); }
            if profile {
                eprintln!("frame {frame}: t={:.6} s, wall={:.3} s, contacts={}", session.robot.time(),
                          started.elapsed().as_secs_f64(), session.frame().contacts.len());
            }
        }
        if let (Some(path),Some(trace)) = (args.get(3),trace) { write_json(path,&trace)?; }
        session
    };
    if path != "--replay" {
        if let Some(dest) = args.get(2) {
            write_json(dest,&session.recording())?;
        }
    }
    println!(
        "{}",
        serde_json::to_string(&session.frame()).map_err(|e| e.to_string())?
    );
    if profile { eprintln!("{}", sim_solve::profile::report()); }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
