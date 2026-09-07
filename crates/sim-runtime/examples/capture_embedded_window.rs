//! Bounded, read-only reporting through the production incremental runtime.
use serde_json::json;
use sim_runtime::{
    embedded::{CaptureMode, Config, EmbeddedSession},
    session::Scene,
};

fn ticks(value: f64, step: f64, label: &str) -> Result<usize, String> {
    let ticks = value / step;
    if !ticks.is_finite()
        || ticks < 0.0
        || (ticks - ticks.round()).abs() > 1e-8
        || ticks > 1_000_000.0
    {
        return Err(format!("{label} must lie on the nominal step grid"));
    }
    Ok(ticks.round() as usize)
}
fn run() -> Result<bool, String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 5 {
        return Err(
            "usage: capture_embedded_window scene.json config.json start-s end-s sample-period-s"
                .into(),
        );
    }
    let scene: Scene = serde_json::from_slice(&std::fs::read(&args[0]).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let config: Config =
        serde_json::from_slice(&std::fs::read(&args[1]).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let number = |i: usize| args[i].parse::<f64>().map_err(|e| e.to_string());
    let (start, end, period) = (number(2)?, number(3)?, number(4)?);
    let (first, last, stride) = (
        ticks(start, config.step_s, "start")?,
        ticks(end, config.step_s, "end")?,
        ticks(period, config.step_s, "period")?,
    );
    if stride == 0
        || last < first
        || last > config.steps
        || (last - first) % stride != 0
        || (last - first) / stride > 10_000
    {
        return Err("window requires ordered in-horizon endpoints, positive dividing period and at most 10001 frames".into());
    }
    let mut session = EmbeddedSession::new(scene, config.clone(), 0, CaptureMode::Latest)?;
    let metadata = session.diagnostic_metadata();
    let initial_frame = session.frame()?;
    if first > 0 {
        let _ = session.advance(first); // Latched failure is recorded below.
    }
    let mut frames = vec![session.frame()?];
    while session.completed_steps() < last && session.error().is_none() {
        let _ = session.advance(stride);
        frames.push(session.frame()?);
        if frames.len() % 100 == 0 {
            eprintln!(
                "captured {} s",
                session.completed_steps() as f64 * config.step_s
            );
        }
    }
    let complete = session.completed_steps() == last && session.error().is_none();
    let report = json!({"kind":"embedded_window_v1","scene_path":args[0],"config_path":args[1],"config":config,
        "window_s":[start,end],"sample_period_s":period,"metadata":metadata,"seed":0,"initial_frame":initial_frame,
        "frames":frames,"completed_steps":session.completed_steps(),"window_complete":complete,
        "full_motion_complete":session.remaining_steps()==0&&session.error().is_none(),"error":session.error(),
        "scope":"Deliberate observation window in the unchanged incremental runtime. A successful window need not complete the full motion. Samples are endpoint snapshots; policy observations retain their own timestamp. No events or states are skipped in advancing to or through the window."});
    println!(
        "{}",
        serde_json::to_string(&report).map_err(|e| e.to_string())?
    );
    Ok(complete)
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sampling_requires_finite_aligned_times() {
        assert_eq!(ticks(1.2, 0.00025, "sample").unwrap(), 4800);
        for v in [f64::NAN, f64::INFINITY, -0.1, 0.0003] {
            assert!(ticks(v, 0.00025, "sample").is_err());
        }
    }
}
