//! Run pacing and speed: the chunk grid, the pacing rule and run speed requests.
use serde::Serialize;
use std::time::Duration;

/// Sim time advanced per chunk (s): the 0.02 s grid (also the planar v2 run's, `planar::GRID_S`). Step advances
/// exactly one chunk; running advances whole chunks.
pub const CHUNK_S: f64 = 0.02;
/// Wall-clock window over which the real-time factor is measured.
pub(super) const RTF_WINDOW: Duration = Duration::from_secs(1);
pub const PACING: &str = "paced at most to speed_scale × real time (pace()): a chunk starts only when (wall time since the anchor) × speed_scale has caught up with sim time since the anchor; the anchor is (wall, sim) at Run, at a replay start and at every speed change, so a mid-run change causes no burst and no stall; lag beyond one chunk is dropped (re-anchored), never made up faster than speed_scale × real time. The scale changes pacing only: dt, chunk_s and the physics step are unchanged. FILE runs, preset runs and replays share this one rule. rtf = sim seconds / wall seconds over the last ~1 s of running (restarted at a speed change), including pacing sleeps; null when not running";
/// Run speed scales (× real time), powers of two (the planar v2 run uses the same scales).
pub const SPEED_SCALES: [f64; 7] = [0.125, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0];
/// Achieved rtf below this fraction of speed_scale while running is reported as compute-limited.
pub const COMPUTE_LIMITED_FRACTION: f64 = 0.9;
pub const COMPUTE_LIMITED_RULE: &str = "compute_limited = running and rtf (measured over ~1 s) is known and below 0.9 × speed_scale: the simulation could not keep up with the requested scale, so speed_scale was not reached; null when not running or rtf is not yet measured. speed_scale is the requested pace, never a claim that it was achieved";

/// The run thread's pacing decision for one loop turn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pace {
    /// Sim time is ahead of scaled wall time: sleep (capped so commands stay responsive).
    Sleep(Duration),
    /// A chunk is due.
    Advance,
    /// A chunk is due and the lag exceeds one chunk: re-anchor so the lag is dropped.
    AdvanceAndReanchor,
}
/// The pacing rule (PACING): `wall_s` and `sim_s` are measured since the anchor.
/// A chunk is due when `wall_s × scale ≥ sim_s`; lag beyond one chunk is dropped.
pub fn pace(wall_s: f64, sim_s: f64, scale: f64, chunk_s: f64) -> Pace {
    let due = wall_s * scale;
    if sim_s > due {
        Pace::Sleep(Duration::from_secs_f64(((sim_s - due) / scale).min(0.005)))
    } else if due - sim_s > chunk_s {
        Pace::AdvanceAndReanchor
    } else {
        Pace::Advance
    }
}

/// A run speed request, shared by keys, buttons, `system_ui` and REST `robot_speed`.
#[derive(Clone, Copy, Serialize, serde::Deserialize, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SpeedRequest {
    /// The next faster scale (refused at ×8).
    Up,
    /// The next slower scale (refused at ×0.125).
    Down,
    /// An exact scale from SPEED_SCALES (anything else is refused, never clamped).
    Set { scale: f64 },
}
fn allowed_scales() -> String {
    SPEED_SCALES.iter().map(|s| format!("{s}")).collect::<Vec<_>>().join(", ")
}
/// The scale a request resolves to from `current`, or why it is refused.
pub fn speed_target(current: f64, request: SpeedRequest) -> Result<f64, String> {
    let i = SPEED_SCALES.iter().position(|s| *s == current).unwrap_or(3);
    match request {
        SpeedRequest::Up => SPEED_SCALES.get(i + 1).copied().ok_or_else(|| format!("already at the fastest run speed ×{current} (allowed: {})", allowed_scales())),
        SpeedRequest::Down => i.checked_sub(1).map(|j| SPEED_SCALES[j]).ok_or_else(|| format!("already at the slowest run speed ×{current} (allowed: {})", allowed_scales())),
        SpeedRequest::Set { scale } if SPEED_SCALES.contains(&scale) => Ok(scale),
        SpeedRequest::Set { scale } => Err(format!("run speed scale {scale} is not allowed; allowed (× real time, powers of two): {}", allowed_scales())),
    }
}
