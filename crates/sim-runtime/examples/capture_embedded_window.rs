//! Bounded, read-only reporting through the production incremental runtime.
use serde_json::json;
use sim_runtime::{
    embedded::{CaptureMode, Config, EmbeddedRecording, EmbeddedSession},
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
            "usage: capture_embedded_window scene.json config.json start-s end-s sample-period-s, or capture_embedded_window --replay recording.json start-s end-s sample-period-s"
                .into(),
        );
    }
    let replay = args[0] == "--replay";
    let (scene, config, recording) = if replay {
        let recording: EmbeddedRecording =
            serde_json::from_slice(&std::fs::read(&args[1]).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        if recording.failure.is_some() {
            return Err("dense replay requires a recording without a failed attempt".into());
        }
        (
            recording.scene.clone(),
            recording.config.clone(),
            Some(recording),
        )
    } else {
        let scene: Scene =
            serde_json::from_slice(&std::fs::read(&args[0]).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        let config: Config =
            serde_json::from_slice(&std::fs::read(&args[1]).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        (scene, config, None)
    };
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
        || recording.as_ref().is_some_and(|r| last > r.completed_steps)
        || (last - first) % stride != 0
        || (last - first) / stride > 10_000
    {
        return Err("window requires ordered in-horizon endpoints, positive dividing period and at most 10001 frames".into());
    }
    let seed = recording.as_ref().map_or(0, |r| r.seed);
    let mut session = if let Some(recording) = recording {
        EmbeddedSession::prepare_replay(recording, CaptureMode::Latest)?.0
    } else {
        EmbeddedSession::new(scene, config.clone(), seed, CaptureMode::Latest)?
    };
    let snapshot = |session: &EmbeddedSession| {
        if replay {
            session.interactive_frame()
        } else {
            session.frame()
        }
    };
    let metadata = session.diagnostic_metadata();
    let initial_frame = snapshot(&session)?;
    if first > 0 {
        let _ = session.advance(first); // Latched failure is recorded below.
    }
    let mut frames = vec![snapshot(&session)?];
    while session.completed_steps() < last && session.error().is_none() {
        let _ = session.advance(stride);
        frames.push(snapshot(&session)?);
        if frames.len() % 100 == 0 {
            eprintln!(
                "captured {} s",
                session.completed_steps() as f64 * config.step_s
            );
        }
    }
    let complete = session.completed_steps() == last && session.error().is_none();
    let report = json!({"kind":"embedded_window_v1","scene_path":if replay {None}else{Some(&args[0])},"config_path":if replay {None}else{Some(&args[1])},"recording_path":if replay {Some(&args[1])}else{None},"config":config,
        "recording":if replay {Some(session.recording())}else{None},
        "window_s":[start,end],"sample_period_s":period,"metadata":metadata,"seed":seed,"initial_frame":initial_frame,
        "frames":frames,"completed_steps":session.completed_steps(),"window_complete":complete,
        "full_motion_complete":session.remaining_steps()==0&&session.error().is_none(),"error":session.error(),
        "scope":"Deliberate observation window in the unchanged incremental runtime. Replay preserves the recorded recipe/seed/input event schedule; host sample spacing changes observation only. At a command boundary replay exposes the newly scheduled held inputs, while an environment endpoint precedes the next host input submission; completed physics/policy samples must still agree. A successful window need not complete the full motion. Samples are endpoint snapshots; policy observations retain their own timestamp. No events or states are skipped in advancing to or through the window."});
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
