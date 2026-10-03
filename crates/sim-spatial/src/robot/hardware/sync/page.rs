//! The page's pure rules for live sync: what counts as live input
//! (`snapshot()`, `currentSample()`), the `/live/open` source, the mapping
//! rows and the banner and readings text.
use super::{DISTINCT, NOT_LIVE};
use crate::robot::RobotView;
use crate::robot::hardware::settings::{SyncBinding, sign};
use crate::robot::hardware::view::fixed;
use crate::robot::run::ReplayPhase;
use serde_json::Value;
use sim_runtime::hardware::protocol::bench;

/// The page's `snapshot().live` (`!playback`): a preset's own physics run,
/// not a recorded preset, not a replay in progress or a run a replay
/// replaced (until Reset), and no gait preview loaded or loading.
pub fn live_run(recorded: bool, preset: bool, replay: ReplayPhase, replaced: bool, gait_holds: bool) -> bool {
    let replay_run = replay == ReplayPhase::Replaying || (replaced && replay != ReplayPhase::Idle);
    !recorded && preset && !replay_run && !gait_holds
}

/// The page's `snapshot()` (viewer.js:428), from the run's accepted frame.
pub struct LiveInput<'a> {
    /// A live run (not a recorded preset or a replay in progress).
    pub live: bool,
    pub coordinates: &'a [String],
    pub targets: &'a [f64],
    pub time_s: f64,
    /// The episode ended (`frame.done`).
    pub done: bool,
}
pub fn live_input(view: &RobotView) -> Option<LiveInput<'_>> {
    let run = view.run.as_ref()?;
    let frame = run.frame()?;
    let replay = run.replay_state();
    let gait_holds = run.gait_preview().is_some_and(|g| g.holds().is_some());
    let live = live_run(run.recorded().is_some(), run.preset().is_some(), replay.phase, replay.replaced, gait_holds);
    // `done` is the frame's (the page's `frame.done`), not the run's phase.
    let (coordinates, targets, done) = frame.motor_targets.as_ref().map_or((&[][..], &[][..], false), |t| (t.coordinates.as_slice(), t.targets_rad.as_slice(), t.done));
    Some(LiveInput { live, coordinates, targets, time_s: frame.time, done })
}

/// `currentSample()` (:17).
pub fn sample_from(input: Option<LiveInput>, sequence: u64) -> Result<bench::Sample, String> {
    match input {
        Some(s) if s.live && !s.targets.is_empty() && s.coordinates.len() == s.targets.len() && !s.done => {
            Ok(bench::Sample { sequence, time_s: s.time_s, targets: s.coordinates.iter().cloned().zip(s.targets.iter().copied()).collect() })
        }
        _ => Err(NOT_LIVE.into()),
    }
}

/// The `/live/open` source: `JSON.stringify({preset, cad})`, members in the
/// page's order (`cad` omitted when the scene's robot has no source).
pub fn source_text(view: &RobotView) -> String {
    let Some(p) = view.run.as_ref().and_then(|r| r.preset()) else { return String::new() };
    let mut s = format!("{{\"preset\":{}", Value::from(p.preset.id.as_str()));
    if !p.scene.robot.source.is_null() {
        s.push_str(&format!(",\"cad\":{}", p.scene.robot.source));
    }
    s.push('}');
    s
}

/// `new Set(ids).size !== 3` (:23).
pub fn distinct(rows: &[SyncBinding]) -> Result<(), String> {
    let ids: std::collections::BTreeSet<u8> = rows.iter().map(|b| b.motor_id).collect();
    if ids.len() == 3 { Ok(()) } else { Err(DISTINCT.into()) }
}

/// `stateText`'s banner (:16).
pub fn banner_text(active: bool, preparing: bool, s: &str) -> String {
    format!("{}{s}", if active { "MOTOR SYNC · " } else if preparing { "CONNECTING MOTORS · " } else { "SIMULATION ONLY · " })
}

/// The leg options (:30): distinct `c.replace('joint.','').split(' | ')[0]`, in order.
pub fn legs(coordinates: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in coordinates {
        let leg = c.replacen("joint.", "", 1).split(" | ").next().unwrap_or_default().to_string();
        if !out.contains(&leg) {
            out.push(leg);
        }
    }
    out
}

/// `mapping(saved)` (:20): the leg's coordinates, each with its saved (by
/// row) or default (`ids[i]`) motor and sign.
pub fn mapping(coordinates: &[String], ids: &[u8], leg: &str, saved: Option<&[SyncBinding]>) -> Vec<SyncBinding> {
    let prefix = format!("joint.{leg} | ");
    coordinates
        .iter()
        .filter(|c| c.starts_with(&prefix))
        .enumerate()
        .map(|(i, c)| {
            let s = saved.and_then(|s| s.get(i));
            let motor_id = s.map(|s| s.motor_id).filter(|id| ids.contains(id)).or_else(|| ids.get(i).copied()).or_else(|| ids.first().copied()).unwrap_or(0);
            let polarity = s.map_or(1, |s| sign(s.polarity));
            SyncBinding { coordinate: c.clone(), motor_id, polarity }
        })
        .collect()
}

/// The readings block (:26), one line per binding.
pub fn reading_lines(samples: &[bench::LiveSample], rows: &[SyncBinding]) -> String {
    rows.iter()
        .map(|b| match samples.iter().rev().find(|p| p.id == b.motor_id) {
            None => format!("ID {}: waiting", b.motor_id),
            Some(p) => {
                // `toFixed` (ties away from zero), not `format!` (ties to even).
                let age = p.live_source.as_ref().map(|_| format!(" · input age {} ms", p.input_age_s().map_or("NaN".into(), |a| fixed(a * 1000.0, 0)))).unwrap_or_default();
                format!("ID {}: {}° · {} V · {} °C{age}", b.motor_id, fixed(p.measured_deg(), 2), fixed(p.telemetry.voltage_v, 1), p.telemetry.temperature_c)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
