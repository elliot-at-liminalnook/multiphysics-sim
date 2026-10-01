//! Recorded-preset timeline: one worker thread owns a recorded preset's
//! pre-mapped frames (`robot_preset::RecordedRun`), runs the playback clock and
//! looks up the frame at or before the clock time with the shared
//! `sim_runtime::embedded_capture::frame_at` (web/viewer/viewer.js `replayAt`).
//! It publishes generation-stamped states; the UI thread only sends commands
//! and takes the latest state of its current generation (as `robot_gait`'s
//! GaitPreview). Nothing is simulated: the frames are recorded physics.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sim_runtime::embedded_capture::{LOOKUP_RULE, TIME_RULE, frame_at};
use crate::robot::preset::{RECORDED_LABEL, RecordedRun};
use crate::robot::run::{Frame, SPEED_SCALES, SpeedRequest, speed_target};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

/// Worker tick while playing (s).
pub const TICK_S: f64 = 1.0 / 60.0;
pub const CLOCK_RULE: &str = "play advances the recorded clock by wall time × speed (the exact run speed scales, × real time) on the playback worker, ticking every 1/60 s; the shown frame is the one at or before the clock time. Reaching the last frame's time stops the clock there (phase ended). Play at the last frame restarts from the first frame, as the browser's Play does (web/viewer/viewer.js). Pause keeps the clock time; a speed change applies from the moment it is received";
pub const SEEK_RULE: &str = "seek t (s, recorded time) must be finite and within [first, last] frame time; outside that range it is refused naming the range, never clamped. Seek pauses (as the browser's timeline slider) and shows the frame at or before t";
pub const STEP_RULE: &str = "step moves exactly one frame index (+1 or −1), pauses and sets the clock to that frame's time_s; refused at the first frame (−1) or the last frame (+1), naming the end";
pub const START_RULE: &str = "start seeks to the first frame and pauses";

/// One timeline request, shared by the inspector buttons, `system_ui` recorded:* and REST `robot_recorded`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum RecordedAction {
    Play,
    Pause,
    Seek { t: f64 },
    Step { delta: i64 },
    Speed { scale: f64 },
    Start,
}
impl RecordedAction {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Play => "play",
            Self::Pause => "pause",
            Self::Seek { .. } => "seek",
            Self::Step { .. } => "step",
            Self::Speed { .. } => "speed",
            Self::Start => "start",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackPhase {
    Playing,
    Paused,
    /// Play reached the last frame and stopped there.
    Ended,
}

/// The playback clock over time-sorted frame times (pure; the worker drives it).
#[derive(Clone, Debug, PartialEq)]
pub struct Timeline {
    pub times: Vec<f64>,
    pub t: f64,
    pub index: usize,
    pub speed: f64,
    pub phase: PlaybackPhase,
}
impl Timeline {
    pub fn new(times: Vec<f64>) -> Self {
        let t = times.first().copied().unwrap_or(0.0);
        Self { times, t, index: 0, speed: 1.0, phase: PlaybackPhase::Paused }
    }
    fn first(&self) -> f64 {
        self.times.first().copied().unwrap_or(0.0)
    }
    fn last(&self) -> f64 {
        self.times.last().copied().unwrap_or(0.0)
    }
    /// The at-or-before index for `t` (the shared `frame_at` rule over the frame times).
    fn lookup(&self, t: f64) -> usize {
        self.times.partition_point(|x| *x <= t).saturating_sub(1)
    }
    /// Why `action` is refused now ([`SEEK_RULE`], [`STEP_RULE`], exact speed scales).
    pub fn check(&self, action: &RecordedAction) -> Result<(), String> {
        match *action {
            RecordedAction::Play if self.phase == PlaybackPhase::Playing => Err("the recorded timeline is already playing".into()),
            RecordedAction::Pause if self.phase != PlaybackPhase::Playing => Err("the recorded timeline is not playing".into()),
            RecordedAction::Seek { t } if !t.is_finite() => Err(format!("recorded seek time t = {t} is not finite")),
            RecordedAction::Seek { t } if t < self.first() || t > self.last() => Err(format!("recorded seek time t = {t} s is outside the capture's [{}, {}] s (refused, not clamped)", self.first(), self.last())),
            RecordedAction::Step { delta } if delta != 1 && delta != -1 => Err(format!("recorded step delta {delta} is not +1 or −1 (one frame)")),
            RecordedAction::Step { delta: -1 } if self.index == 0 => Err("step −1 is refused: already at the first frame (index 0)".into()),
            RecordedAction::Step { delta: 1 } if self.index + 1 >= self.times.len() => Err(format!("step +1 is refused: already at the last frame (index {})", self.index)),
            RecordedAction::Speed { scale } => speed_target(self.speed, SpeedRequest::Set { scale }).map(|_| ()),
            _ => Ok(()),
        }
    }
    /// Applies a checked action.
    pub fn apply(&mut self, action: RecordedAction) {
        match action {
            RecordedAction::Play => {
                if self.t >= self.last() {
                    self.t = self.first();
                }
                self.phase = PlaybackPhase::Playing;
            }
            RecordedAction::Pause => self.phase = PlaybackPhase::Paused,
            RecordedAction::Seek { t } => {
                self.t = t;
                self.phase = PlaybackPhase::Paused;
            }
            RecordedAction::Step { delta } => {
                let i = (self.index as i64 + delta).clamp(0, self.times.len() as i64 - 1) as usize;
                self.t = self.times[i];
                self.phase = PlaybackPhase::Paused;
            }
            RecordedAction::Speed { scale } => self.speed = scale,
            RecordedAction::Start => {
                self.t = self.first();
                self.phase = PlaybackPhase::Paused;
            }
        }
        self.index = self.lookup(self.t);
    }
    /// Advances a playing clock by `wall_s` × speed; stops at the last frame.
    pub fn advance(&mut self, wall_s: f64) {
        if self.phase != PlaybackPhase::Playing {
            return;
        }
        self.t += wall_s * self.speed;
        if self.t >= self.last() {
            self.t = self.last();
            self.phase = PlaybackPhase::Ended;
        }
        self.index = self.lookup(self.t);
    }
}

/// A published playback state: the clock and the frame it shows, stamped with
/// the generation of the command it follows.
#[derive(Clone, Debug)]
pub struct PlaybackState {
    pub generation: u64,
    pub timeline: Timeline,
    pub frame: Frame,
}
impl crate::jobs::Stamped for PlaybackState {
    fn generation(&self) -> u64 {
        self.generation
    }
}

enum Command {
    Act { generation: u64, action: RecordedAction },
}

/// The UI side of the playback worker.
pub struct RecordedPlayback {
    /// The `robot-recorded` worker; dropping it stops and joins it (bounded).
    thread: crate::jobs::RunThread<Command, PlaybackState>,
    /// Generation the UI expects; every command bumps it and older states are stale.
    generation: u64,
    state: PlaybackState,
}
impl RecordedPlayback {
    /// Spawns the paused worker at the first frame.
    pub fn spawn(run: Arc<RecordedRun>) -> Self {
        let timeline = Timeline::new(run.capture.frames.iter().map(|f| f.time_s).collect());
        let state = PlaybackState { generation: 0, timeline, frame: stamped(&run, 0, 0) };
        let thread = crate::jobs::RunThread::spawn("robot-recorded", state.clone(), move |rx, out| worker(run, rx, out));
        Self { thread, generation: 0, state }
    }
    pub fn check(&self, action: &RecordedAction) -> Result<(), String> {
        self.state.timeline.check(action)
    }
    /// The one timeline handler: checked against the latest accepted state, then sent to the worker.
    pub fn act(&mut self, action: RecordedAction) -> Result<(), String> {
        self.check(&action)?;
        self.generation += 1;
        self.thread.send(Command::Act { generation: self.generation, action }).map_err(|_| "the recorded playback thread has stopped".to_string())?;
        // The UI's own view of what it asked for; the worker's state replaces it once published.
        self.state.timeline.apply(action);
        Ok(())
    }
    /// Takes the worker's latest state of the current generation; true when the shown frame changed.
    pub fn poll(&mut self) -> bool {
        // An older generation's state is stale and never applied.
        let Some(s) = self.thread.latest(self.generation) else { return false };
        let before = (self.state.frame.generation, self.state.frame.steps);
        self.state = s;
        before != (self.state.frame.generation, self.state.frame.steps)
    }
    pub fn frame(&self) -> &Frame {
        &self.state.frame
    }
    pub fn timeline(&self) -> &Timeline {
        &self.state.timeline
    }
    pub fn playing(&self) -> bool {
        self.state.timeline.phase == PlaybackPhase::Playing
    }
    /// Commands sent whose state the worker has not yet published.
    pub fn pending(&self) -> bool {
        self.state.generation < self.generation
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn state_generation(&self) -> u64 {
        self.state.generation
    }
}

/// Frame `i` of the run, stamped with `generation` (steps = frame index, time = its time_s).
fn stamped(run: &RecordedRun, i: usize, generation: u64) -> Frame {
    let mut f = run.frames[i].clone();
    f.generation = generation;
    f
}

fn worker(run: Arc<RecordedRun>, rx: mpsc::Receiver<Command>, out: Arc<Mutex<PlaybackState>>) {
    // The initial state is the one `spawn` published.
    let mut state = out.lock().unwrap_or_else(|p| p.into_inner()).clone();
    let mut last_tick = Instant::now();
    let publish = |state: &PlaybackState| *out.lock().unwrap_or_else(|p| p.into_inner()) = state.clone();
    loop {
        let command = if state.timeline.phase == PlaybackPhase::Playing {
            let wait = Duration::from_secs_f64(TICK_S).saturating_sub(last_tick.elapsed());
            match rx.recv_timeout(wait) {
                Ok(c) => Some(c),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        } else {
            match rx.recv() {
                Ok(c) => Some(c),
                Err(_) => return,
            }
        };
        // Wall time since the last tick advances a playing clock first, at the speed in force.
        let now = Instant::now();
        state.timeline.advance((now - last_tick).as_secs_f64());
        last_tick = now;
        if let Some(Command::Act { generation, action }) = command {
            state.generation = generation;
            // Re-checked against the worker's own clock (play may have moved it since the UI checked).
            if state.timeline.check(&action).is_ok() {
                state.timeline.apply(action);
            }
        }
        // The shared at-or-before lookup over the capture's frames.
        let i = frame_at(&run.capture.frames, state.timeline.t);
        debug_assert_eq!(i, state.timeline.index);
        state.frame = stamped(&run, i, state.generation);
        publish(&state);
    }
}

/// `robot_state.recorded`: identity and paths, the timeline, the verbatim
/// preset text, the capture's metadata as read (absent fields null and listed
/// in `absent`), unmatched names, the rules and the label.
pub fn state_json(run: &RecordedRun, p: &RecordedPlayback) -> Value {
    let pre = &run.preset;
    let c = &run.capture;
    let m = &c.meta;
    let tl = p.timeline();
    let f = p.frame();
    let src = m.source.as_ref();
    let fields: [(&str, bool); 12] = [
        ("source", src.is_some()),
        ("source.fidelity", src.and_then(|s| s.fidelity.as_ref()).is_some()),
        ("source.cad_sha256", src.and_then(|s| s.cad_sha256.as_ref()).is_some()),
        ("source.file", src.and_then(|s| s.file.as_ref()).is_some()),
        ("source.cad_revision", src.and_then(|s| s.cad_revision.as_ref()).is_some()),
        ("completed", m.completed.is_some()),
        ("error", m.error.is_some()),
        ("simulated_s", m.simulated_s.is_some()),
        ("stepping_wall_s", m.stepping_wall_s.is_some()),
        ("step_s", m.step_s.is_some()),
        ("completed_steps", m.completed_steps.is_some()),
        ("requested_steps", m.requested_steps.is_some()),
    ];
    let absent: Vec<&str> = fields.iter().filter(|(_, present)| !present).map(|(k, _)| *k).collect();
    json!({"label": RECORDED_LABEL, "preset": pre.id, "preset_label": pre.label, "scene": pre.scene, "capture": pre.capture,
        "scene_path": run.scene_path, "capture_path": run.capture_path,
        "frame_count": c.frames.len(), "first_time_s": tl.times.first(), "last_time_s": tl.times.last(), "duration_s": c.duration_s(),
        "time_s": tl.t, "frame_index": tl.index, "frame_time_s": f.time, "speed": tl.speed, "speed_scales": SPEED_SCALES, "phase": tl.phase,
        "generation": p.generation(), "state_generation": p.state_generation(), "frame_generation": f.generation, "pending": p.pending(),
        "description": pre.entry.get("description"), "readiness": pre.readiness(), "evidence": pre.evidence(),
        "meta": {"source": {"fidelity": src.and_then(|s| s.fidelity.as_ref()), "cad_sha256": src.and_then(|s| s.cad_sha256.as_ref()), "file": src.and_then(|s| s.file.as_ref()), "cad_revision": src.and_then(|s| s.cad_revision.as_ref())},
            "completed": m.completed, "error": m.error, "simulated_s": m.simulated_s, "stepping_wall_s": m.stepping_wall_s, "step_s": m.step_s,
            "completed_steps": m.completed_steps, "requested_steps": m.requested_steps},
        "absent": absent, "absent_rule": "metadata fields missing (or null) in the capture file: reported null here and never defaulted",
        "recorded_rate": m.recorded_rate(), "recorded_rate_rule": "simulated_s / stepping_wall_s from the file; null when either is absent",
        "unmatched_capture_links": run.unmatched,
        "lookup_rule": LOOKUP_RULE, "time_rule": TIME_RULE, "clock_rule": CLOCK_RULE, "seek_rule": SEEK_RULE, "step_rule": STEP_RULE, "start_rule": START_RULE,
        "thread": "robot-recorded playback worker (not the UI thread); nothing is simulated"})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeline_rules() {
        let mut tl = Timeline::new(vec![0.0, 0.01, 0.02, 0.03]);
        // Seek: at or before, refused outside the range and when not finite.
        tl.apply(RecordedAction::Seek { t: 0.015 });
        assert_eq!((tl.index, tl.phase), (1, PlaybackPhase::Paused));
        assert!(tl.check(&RecordedAction::Seek { t: 0.031 }).unwrap_err().contains("outside"));
        assert!(tl.check(&RecordedAction::Seek { t: f64::NAN }).unwrap_err().contains("not finite"));
        // Step: one index, refused at the ends.
        tl.apply(RecordedAction::Step { delta: 1 });
        assert_eq!((tl.index, tl.t), (2, 0.02));
        tl.apply(RecordedAction::Start);
        assert!(tl.check(&RecordedAction::Step { delta: -1 }).unwrap_err().contains("first frame"));
        // Speed: exact scales only.
        assert!(tl.check(&RecordedAction::Speed { scale: 0.3 }).is_err());
        tl.apply(RecordedAction::Speed { scale: 0.5 });
        // Play: wall × speed, stops at the end, restarts from the first frame.
        tl.apply(RecordedAction::Play);
        tl.advance(0.03);
        assert!((tl.t - 0.015).abs() < 1e-12 && tl.index == 1);
        tl.advance(1.0);
        assert_eq!((tl.t, tl.index, tl.phase), (0.03, 3, PlaybackPhase::Ended));
        assert!(tl.check(&RecordedAction::Step { delta: 1 }).unwrap_err().contains("last frame"));
        tl.apply(RecordedAction::Play);
        assert_eq!((tl.t, tl.index, tl.phase), (0.0, 0, PlaybackPhase::Playing));
    }
}
