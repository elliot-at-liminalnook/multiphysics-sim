//! The UI side of the run thread: `RunController` construction, run actions, polling, overlays, speed and state.
use serde_json::{Value, json};
use sim_domain_robot::PhysicalModel;
use std::sync::Arc;
use crate::robot::gait::GaitPreview;
use crate::robot::graphs;
use crate::robot::playback::{RecordedAction, RecordedPlayback};
use crate::robot::preset::{PresetRun, RecordedRun};
use crate::robot::recording::{Listed, Saved};
use super::protocol::{Command, Published, Status};
use super::worker::worker;
use super::{COMPUTE_LIMITED_FRACTION, COMPUTE_LIMITED_RULE, DEFLECTION_MAGNIFICATION, Drive, FORCE_SCALE_M_PER_N, Frame, JOINT_AXIS_HALF_M, OVERLAY_COST_RULE, OVERLAY_SAMPLE, OverlayFlags, PACING, Phase, ReplayPhase, ReplayState, RunAction, SPEED_SCALES, Servo, Source, SpeedRequest, accept, check_target, servo, speed_target};

/// The UI side of the run thread: sends commands, accepts frames.
pub struct RunController {
    /// The `robot-run` worker (idle, with no thread, for a recorded preset);
    /// dropping it (reload, preset switch) stops and joins it within a bound.
    pub(super) thread: crate::jobs::RunThread<Command, Published>,
    /// Generation the UI expects; frames from older generations are discarded.
    pub(super) generation: u64,
    /// Whether the UI last asked the robot to run (commands are ordered).
    pub(super) running: bool,
    pub(super) frame: Option<Frame>,
    pub(super) status: Status,
    /// The loaded model, for file-based jog validation (never written).
    pub(super) model: PhysicalModel,
    /// Targets requested by jogs in this generation (cleared by Reset).
    pub(super) jogged: std::collections::BTreeMap<String, f64>,
    pub(super) jog_error: Option<String>,
    /// The preset this controller runs (None for `--robot FILE`).
    pub(super) preset: Option<Arc<PresetRun>>,
    /// A recorded preset played back (no run thread, nothing simulated).
    pub(super) recorded: Option<Arc<RecordedRun>>,
    /// Its timeline worker (robot_playback): the clock and the frame lookup, off the UI thread.
    pub(super) playback: Option<RecordedPlayback>,
    pub(super) chunk_s: f64,
    pub(super) drive: Option<Arc<Drive>>,
    /// The motion values last sent in this generation (cleared by Reset).
    pub(super) requested: Option<[f64; 3]>,
    /// The keys behind `requested`, and whether they are physically held (press/release) or latched.
    pub(super) keys: Vec<char>,
    pub(super) keys_physical: bool,
    pub(super) motion_refusal: Option<String>,
    pub(super) motion_error: Option<String>,
    /// Saves requested and finished (request numbers), the pending target, the last pair written and the last save error.
    pub(super) save_requested: u64,
    pub(super) save_done: u64,
    pub(super) saving: Option<std::path::PathBuf>,
    pub(super) saved: Option<Saved>,
    pub(super) save_error: Option<String>,
    /// The latest replay state (local until the run thread publishes a newer one).
    pub(super) replay: ReplayState,
    /// The recording listing in flight (a newer one replaces it), and the last list or why not.
    pub(super) listing: crate::jobs::Latest<Vec<Listed>>,
    pub(super) recordings: Vec<Listed>,
    pub(super) list_error: Option<String>,
    /// Time histories of the applied frames of the current generation (robot_graphs).
    pub(super) graphs: graphs::History,
    /// The loaded model's chassis link (graphs::CHASSIS_RULE), or why none.
    pub(super) chassis: Result<usize, String>,
    /// Presets: the kinematic gait preview (robot_gait), on its own worker.
    pub(super) gait: Option<GaitPreview>,
    /// The overlays requested of the run thread (`--robot FILE`).
    pub(super) overlays: OverlayFlags,
    /// The requested run speed scale (SPEED_SCALES; pacing only).
    pub(super) speed_scale: f64,
}

impl RunController {
    /// Spawns the (idle) run thread with its own clone of the loaded model.
    pub fn spawn(model: PhysicalModel) -> Self {
        Self::spawn_source(Source::Robot(model.clone()), model, None, 0)
    }
    /// A fresh run context for a reloaded file (`robot_source`): the same idle
    /// run thread as [`Self::spawn`], starting at `generation` so it continues
    /// strictly after the replaced controller's and no older frame is accepted.
    /// FILE mode never records or replays, so this generation is never stored.
    pub fn spawn_at(model: PhysicalModel, generation: u64) -> Self {
        Self::spawn_source(Source::Robot(model.clone()), model, None, generation)
    }
    /// Spawns the (idle) run thread for a preset; `scene.robot` is the model
    /// the links and inspector show. Nothing is built until Run or Step.
    pub fn spawn_preset(run: Arc<PresetRun>) -> Self {
        let model = run.scene.robot.clone();
        let links: Vec<String> = model.links.iter().map(|l| l.name.clone()).collect();
        let mut c = Self::spawn_source(Source::Preset(run.clone()), model, Some(run.clone()), 0);
        c.gait = Some(GaitPreview::spawn(run, links));
        c.refresh_recordings();
        c
    }
    /// A recorded preset: no run thread is spawned and nothing is built; the
    /// capture's frame 0 (mapped on the loader thread) is shown. Every live-only
    /// action is refused naming the preset ([`Self::recorded_refusal`]).
    pub fn spawn_recorded(run: Arc<RecordedRun>) -> Self {
        let model = run.scene.robot.clone();
        // No run thread: nothing is ever sent (all sends are refused first).
        let generation = 0;
        let status = Status { phase: Phase::Idle, generation, rtf: None, error: None, end: None };
        let thread = crate::jobs::RunThread::idle("robot-run", Published { status: status.clone(), frame: None, jog_error: None, drive: None, motion_error: None, save: None, replay: None });
        let mut graphs = graphs::History::default();
        graphs.clear(generation);
        let frame = run.frames.first().cloned();
        Self { thread, generation, running: false, frame, status, jogged: Default::default(), jog_error: None, preset: None, recorded: Some(run.clone()), playback: Some(RecordedPlayback::spawn(run)), chunk_s: 0.0,
            drive: None, requested: None, keys: Vec::new(), keys_physical: false, motion_refusal: None, motion_error: None,
            save_requested: 0, save_done: 0, saving: None, saved: None, save_error: None,
            replay: ReplayState::new(0, generation, None, ReplayPhase::Idle), listing: Default::default(), recordings: Vec::new(), list_error: None,
            graphs, chassis: graphs::chassis(&model), model, gait: None, overlays: OverlayFlags::default(), speed_scale: 1.0 }
    }
    pub fn recorded(&self) -> Option<&Arc<RecordedRun>> {
        self.recorded.as_ref()
    }
    /// The recorded timeline worker, when a recorded preset is loaded.
    pub fn playback(&self) -> Option<&RecordedPlayback> {
        self.playback.as_ref()
    }
    /// Why a recorded timeline action is refused: not a recorded preset (named), else the timeline's rules.
    pub fn check_recorded(&self, action: &RecordedAction) -> Result<(), String> {
        let Some(p) = &self.playback else {
            let what = match &self.preset {
                Some(p) => format!("preset `{}` is an embedded preset run live by {}", p.preset.id, p.kind()),
                None => "this is a --robot FILE view (a live PhysicalRobot run)".to_string(),
            };
            return Err(format!("recorded timeline `{}` is refused: {what}; the timeline is for recorded presets (mode `recorded`) only", action.name()));
        };
        p.check(action)
    }
    /// The one recorded timeline handler behind the inspector Recorded buttons,
    /// `system_ui` recorded:* and REST `robot_recorded`. Speed goes through
    /// [`Self::speed`], so the header −/×/+ buttons, keys and `robot_speed` set the same scale.
    pub fn recorded_act(&mut self, action: RecordedAction) -> Result<(), String> {
        self.check_recorded(&action)?;
        if let RecordedAction::Speed { scale } = action {
            return self.speed(SpeedRequest::Set { scale });
        }
        self.playback.as_mut().expect("checked above").act(action)
    }
    /// `robot_state.recorded` (None unless a recorded preset is loaded).
    pub fn recorded_json(&self) -> Option<Value> {
        Some(crate::robot::playback::state_json(self.recorded.as_ref()?, self.playback.as_ref()?))
    }
    /// The refusal of a live-only action (`what`) when a recorded preset is loaded.
    pub(super) fn recorded_refusal(&self, what: &str) -> Result<(), String> {
        match &self.recorded {
            Some(r) => Err(r.refusal(what)),
            None => Ok(()),
        }
    }
    fn spawn_source(source: Source, model: PhysicalModel, preset: Option<Arc<PresetRun>>, generation: u64) -> Self {
        let status = Status { phase: Phase::Idle, generation, rtf: None, error: None, end: None };
        let published = Published { status: status.clone(), frame: None, jog_error: None, drive: None, motion_error: None, save: None, replay: None };
        let chunk_s = source.chunk_s();
        let links: Vec<String> = model.links.iter().map(|l| l.name.clone()).collect();
        let thread = crate::jobs::RunThread::spawn("robot-run", published, move |rx, out| worker(source, links, rx, out, generation));
        let mut graphs = graphs::History::default();
        graphs.clear(generation);
        Self { thread, generation, running: false, frame: None, status, jogged: Default::default(), jog_error: None, preset, recorded: None, playback: None, chunk_s,
            drive: None, requested: None, keys: Vec::new(), keys_physical: false, motion_refusal: None, motion_error: None,
            save_requested: 0, save_done: 0, saving: None, saved: None, save_error: None,
            replay: ReplayState::new(0, generation, None, ReplayPhase::Idle), listing: Default::default(), recordings: Vec::new(), list_error: None,
            graphs, chassis: graphs::chassis(&model), model, gait: None, overlays: OverlayFlags::default(), speed_scale: 1.0 }
    }
    pub fn preset(&self) -> Option<&Arc<PresetRun>> {
        self.preset.as_ref()
    }
    pub fn chunk_s(&self) -> f64 {
        self.chunk_s
    }
    pub fn end(&self) -> Option<&Value> {
        self.status.end.as_ref()
    }

    /// The target a jog step starts from: this generation's last requested
    /// target, else the latest accepted frame's, else the file's control target.
    pub fn requested_target(&self, servo: &Servo) -> f64 {
        self.jogged.get(&servo.joint).copied().or_else(|| self.frame.as_ref().and_then(|f| f.servo(&servo.joint)).map(|(t, _)| t)).unwrap_or(servo.file_target)
    }

    /// Why a jog of `joint` to `target` is refused: file validation first
    /// (unknown joint, no servo target, non-finite, outside the file's
    /// limits), then run state. Works idle and after a failed build.
    pub fn check_jog(&self, joint: &str, target: f64) -> Result<Servo, String> {
        self.recorded_refusal(&format!("servo-target jog of `{joint}`"))?;
        if let Some(p) = &self.preset {
            return Err(format!("joint `{joint}`: servo-target jog is for `--robot FILE`; preset `{}` is driven by its declared controller recipe ({}), so the viewer sets no joint target", p.preset.id, p.kind()));
        }
        let servo = servo(&self.model, joint)?;
        check_target(&servo, target)?;
        if self.status.phase == Phase::Failed {
            return Err(format!("joint `{joint}`: the run failed; Reset rebuilds the robot before it can be jogged"));
        }
        Ok(servo)
    }

    /// The one jog handler behind the +/− buttons, `system_ui` jog:* and REST `robot_jog`.
    pub fn jog(&mut self, joint: &str, target: f64) -> Result<(), String> {
        self.check_jog(joint, target)?;
        self.jogged.insert(joint.to_string(), target);
        self.thread.send(Command::Jog { joint: joint.to_string(), target }).map_err(|_| "the run thread has stopped".to_string())
    }

    /// Why an action is unavailable now (`Ok` when it can be sent).
    pub fn check(&self, action: RunAction) -> Result<(), String> {
        self.recorded_refusal(&format!("run {}", action.name()))?;
        if action != RunAction::Reset {
            if let Some(why) = self.replay_block() {
                return Err(why);
            }
        }
        if matches!(action, RunAction::Start | RunAction::Step) {
            if let Some(why) = self.gait.as_ref().and_then(GaitPreview::holds) {
                return Err(format!("{why}; Stop the gait preview before a physics {}", action.label()));
            }
        }
        let failed = self.status.phase == Phase::Failed;
        let ended = self.status.phase == Phase::Ended;
        let why = || self.status.end.as_ref().and_then(|e| e.get("message")).and_then(|m| m.as_str()).unwrap_or("the run ended").to_string();
        match action {
            RunAction::Start if failed => Err("the run failed; Reset rebuilds the robot before it can run again".into()),
            RunAction::Start if ended => Err(format!("{}; Reset rebuilds at t = 0 before it can run again", why())),
            RunAction::Start if self.running => Err("already running".into()),
            RunAction::Pause if !self.running || failed || ended => Err("not running".into()),
            RunAction::Step if failed => Err("the run failed; Reset rebuilds the robot before it can step".into()),
            RunAction::Step if ended => Err(format!("{}; Reset rebuilds at t = 0 before it can step", why())),
            RunAction::Step if self.running => Err(format!("step advances one {} s chunk only while paused; the robot is running — pause first", self.chunk_s)),
            _ => Ok(()),
        }
    }

    /// The one handler behind the buttons, `system_ui` and REST `robot_run`.
    pub fn act(&mut self, action: RunAction) -> Result<(), String> {
        self.check(action)?;
        let command = match action {
            RunAction::Start => {
                self.running = true;
                Command::Start
            }
            RunAction::Pause => {
                self.running = false;
                Command::Pause
            }
            RunAction::Step => Command::Step,
            RunAction::Reset => {
                // Reset leaves the robot paused at t = 0; every older frame is stale.
                self.running = false;
                self.generation += 1;
                self.frame = None;
                // The rebuild starts from the file's control targets again.
                self.jogged.clear();
                self.jog_error = None;
                // The rebuild starts from the session's initial inputs again.
                self.drive = None;
                self.requested = None;
                self.keys.clear();
                self.keys_physical = false;
                self.motion_error = None;
                // Reset ends any replay: a fresh run.
                self.replay = ReplayState::new(self.replay.seq, self.generation, None, ReplayPhase::Idle);
                self.graphs.clear(self.generation);
                self.status = Status { phase: Phase::Building, generation: self.generation, rtf: None, error: None, end: None };
                Command::Reset { generation: self.generation }
            }
        };
        self.thread.send(command).map_err(|_| "the run thread has stopped".to_string())
    }

    /// Takes the worker's latest status and frame; returns true when the
    /// displayed frame changed (a stale-generation frame is never accepted).
    pub fn poll(&mut self) -> bool {
        let shared = self.thread.shared().clone();
        let published = shared.lock().unwrap_or_else(|p| p.into_inner());
        let mut relist = false;
        // Saves are file results, kept across generations.
        if let Some((seq, result)) = published.save.as_ref().filter(|(seq, _)| *seq > self.save_done) {
            self.save_done = *seq;
            if self.save_done >= self.save_requested {
                self.saving = None;
            }
            match result {
                Ok(saved) => {
                    self.saved = Some(saved.clone());
                    self.save_error = None;
                    relist = true;
                }
                Err(e) => self.save_error = Some(e.clone()),
            }
        }
        if let Some((_, result)) = self.listing.poll() {
            match result {
                Ok(list) => {
                    self.recordings = list;
                    self.list_error = None;
                }
                Err(e) => self.list_error = Some(e),
            }
        }
        if let Some(r) = published.replay.as_ref().filter(|r| r.generation >= self.generation && r.seq >= self.replay.seq) {
            let cancel = self.replay.cancel_requested && self.replay.seq == r.seq;
            self.replay = r.clone();
            self.replay.cancel_requested |= cancel;
        }
        if published.status.generation >= self.generation {
            self.status = published.status.clone();
            self.jog_error = published.jog_error.clone();
            self.drive = published.drive.clone();
            self.motion_error = published.motion_error.clone();
            if matches!(self.status.phase, Phase::Failed | Phase::Ended) {
                self.running = false;
            }
        }
        let fresh = published.frame.as_ref().filter(|f| accept(self.generation, f) && self.frame.as_ref().is_none_or(|old| old.generation != f.generation || old.steps != f.steps || old.targets != f.targets || old.completed_steps != f.completed_steps || old.inputs != f.inputs || old.overlays.flags != f.overlays.flags));
        let changed = match fresh {
            Some(f) => {
                self.frame = Some(f.clone());
                // One graph sample per applied frame, only of the current generation.
                let motion = self.drive.as_ref().and_then(|d| d.motion.as_ref());
                self.graphs.sample(self.generation, f, motion, self.chassis.as_ref().ok().copied());
                true
            }
            None => false,
        };
        drop(published);
        if relist {
            self.refresh_recordings();
        }
        let preview = self.gait.as_mut().is_some_and(GaitPreview::poll);
        // A recorded preset's frame comes from its playback worker (current generation only).
        let recorded = match self.playback.as_mut() {
            Some(p) => {
                let fresh = p.poll();
                if fresh {
                    self.frame = Some(p.frame().clone());
                }
                fresh
            }
            None => false,
        };
        changed || preview || recorded
    }

    pub fn frame(&self) -> Option<&Frame> {
        self.frame.as_ref()
    }
    pub fn overlays(&self) -> OverlayFlags {
        self.overlays
    }
    /// Why the overlays cannot be set (`Ok` for `--robot FILE`).
    pub fn check_overlays(&self) -> Result<(), String> {
        self.recorded_refusal("overlays (contacts, joint frames, deflections)")?;
        match &self.preset {
            Some(p) => Err(format!("overlays are not available for presets: preset `{}` runs {}, whose frames publish link poses but no contacts, joint frames or deflections (those are PhysicalRobot accessors, `--robot FILE` only)", p.preset.id, p.kind())),
            None => Ok(()),
        }
    }
    /// The one overlay handler behind keys C/J/F, the inspector buttons, `system_ui` overlay:* and REST `robot_overlay`.
    pub fn set_overlays(&mut self, flags: OverlayFlags) -> Result<(), String> {
        self.check_overlays()?;
        self.thread.send(Command::Overlays(flags)).map_err(|_| "the run thread has stopped".to_string())?;
        self.overlays = flags;
        Ok(())
    }
    /// `robot_state.overlays`: flags, the latest accepted frame's generation and time, counts, sample values and scales.
    pub fn overlays_json(&self) -> Value {
        let scales = json!({"force_m_per_n": FORCE_SCALE_M_PER_N, "deflection_magnification": DEFLECTION_MAGNIFICATION, "joint_axis_half_m": JOINT_AXIS_HALF_M});
        if let Err(e) = self.check_overlays() {
            return json!({"available": false, "reason": e, "flags": null, "scales": scales});
        }
        let f = self.frame.as_ref();
        let o = f.map(|f| &f.overlays);
        let contacts = o.and_then(|o| o.contacts.as_ref());
        let deflections = o.and_then(|o| o.deflections.as_ref());
        let max = deflections.map(|d| d.iter().map(|d| d.displacement.iter().map(|x| x * x).sum::<f64>().sqrt()).fold(0.0, f64::max));
        json!({"available": true, "flags": self.overlays, "frame_flags": o.and_then(|o| o.flags),
            "frame_generation": f.map(|f| f.generation), "frame_time": f.map(|f| f.time),
            "contacts": {"count": contacts.map(Vec::len), "sample": contacts.map(|c| &c[..c.len().min(OVERLAY_SAMPLE)])},
            "joints": {"count": o.and_then(|o| o.joints.as_ref()).map(Vec::len), "sample": o.and_then(|o| o.joints.as_ref()).and_then(|j| j.first())},
            "deflections": {"count": deflections.map(Vec::len), "max_displacement_m": max},
            "scales": scales, "frame": "model frame (Z up), SI: m, N; drawn through RobotRoot's transform like the link meshes",
            "source": "PhysicalRobot::contacts / joint_frames / deflections, copied on the run thread into the published frame (other: the other link's name, or ground)",
            "null_rule": "a count is null when there is no accepted frame, or when the frame was built with that overlay off (not computed)", "cost": OVERLAY_COST_RULE})
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    /// The fresh run context for a reloaded file: `previous` (if any) is
    /// dropped, which closes its channel and stops its run thread (joined
    /// within `jobs::JOIN_BOUND`) with any run, jog or replay state; its
    /// frames can never be applied: the new idle controller starts at the next
    /// generation. Returns it and whether run or jog state was discarded.
    pub fn replace(previous: Option<RunController>, model: PhysicalModel) -> (Self, bool) {
        let reset = previous.as_ref().is_some_and(Self::has_run_state);
        let generation = previous.as_ref().map_or(0, |r| r.generation + 1);
        // The user's overlay choice survives a reload.
        let overlays = previous.as_ref().map(|r| r.overlays);
        // So does the run speed scale.
        let speed = previous.as_ref().map(|r| r.speed_scale);
        drop(previous);
        let mut next = Self::spawn_at(model, generation);
        if let Some(flags) = overlays.filter(|f| *f != next.overlays) {
            let _ = next.set_overlays(flags);
        }
        if let Some(scale) = speed.filter(|s| *s != next.speed_scale) {
            let _ = next.speed(SpeedRequest::Set { scale });
        }
        (next, reset)
    }
    /// Whether discarding this controller loses run or jog state: anything
    /// built, running, failed or ended, or a jog requested this generation.
    pub fn has_run_state(&self) -> bool {
        self.running || self.frame.is_some() || !self.jogged.is_empty() || self.status.phase != Phase::Idle
    }
    pub fn phase(&self) -> Phase {
        self.status.phase
    }
    pub fn rtf(&self) -> Option<f64> {
        self.status.rtf
    }
    pub fn speed_scale(&self) -> f64 {
        self.speed_scale
    }
    /// The scale a speed request resolves to, or why it is refused.
    pub fn check_speed(&self, request: SpeedRequest) -> Result<f64, String> {
        speed_target(self.speed_scale, request)
    }
    /// The one handler behind the speed keys, buttons, `system_ui` run:speed_* and REST
    /// `robot_speed`: sets the requested scale in every phase (it applies on the next Run).
    pub fn speed(&mut self, request: SpeedRequest) -> Result<(), String> {
        let scale = self.check_speed(request)?;
        // A recorded preset has no run thread: the scale paces its playback worker.
        match self.playback.as_mut() {
            Some(p) => p.act(RecordedAction::Speed { scale })?,
            None => self.thread.send(Command::Speed(scale)).map_err(|_| "the run thread has stopped".to_string())?,
        }
        self.speed_scale = scale;
        Ok(())
    }
    /// compute_limited (COMPUTE_LIMITED_RULE): None unless running with a measured rtf.
    pub fn compute_limited(&self) -> Option<bool> {
        let running = self.status.phase == Phase::Running && self.running;
        self.status.rtf.filter(|_| running).map(|rtf| rtf < COMPUTE_LIMITED_FRACTION * self.speed_scale)
    }
    pub fn error(&self) -> Option<&str> {
        self.status.error.as_deref()
    }
    pub fn jog_error(&self) -> Option<&str> {
        self.jog_error.as_deref()
    }
    pub fn model(&self) -> &PhysicalModel {
        &self.model
    }

    /// One joint's jog state: file limit, requested target, and the target and
    /// measured value from the latest accepted frame (null before a build).
    pub fn jog_json(&self, joint: &str) -> Value {
        match servo(&self.model, joint) {
            Err(e) => json!({"joint": joint, "servo": false, "reason": e}),
            Ok(s) => {
                let latest = self.frame.as_ref().and_then(|f| f.servo(joint));
                json!({"joint": joint, "servo": true, "unit": s.unit, "limit": s.limit, "limit_text": s.limit_text(), "step": s.step(),
                    "file_target": s.file_target, "requested_target": self.jogged.get(joint), "target": latest.map(|l| l.0), "measured": latest.map(|l| l.1),
                    "frame_generation": self.frame.as_ref().map(|f| f.generation), "frame_time": self.frame.as_ref().map(|f| f.time)})
            }
        }
    }
    /// `live`, or `replay` while the current generation is a replay's run.
    pub fn graphs_mode(&self) -> &'static str {
        let r = &self.replay;
        if r.generation == self.generation && (r.phase == ReplayPhase::Replaying || r.replaced) { "replay" } else { "live" }
    }
    /// The fixed chart set for the selected link (by index into the loaded model).
    pub fn graph_charts(&self, selected: Option<usize>) -> Vec<graphs::ChartView> {
        let links: Vec<String> = self.model.links.iter().map(|l| l.name.clone()).collect();
        let selected = selected.and_then(|i| self.model.links.get(i)).map(|l| {
            let joints: Vec<(String, &'static str)> = self.model.joints.iter().filter(|j| j.child == l.name || j.parent.as_deref() == Some(l.name.as_str())).filter_map(|j| servo(&self.model, &j.name).ok()).map(|s| (s.joint, s.unit)).collect();
            (l.name.as_str(), if joints.is_empty() { Err("no servo joint on selected link".to_string()) } else { Ok(joints) })
        });
        let cx = graphs::Context { preset: self.preset.is_some(), motion: self.drive.as_ref().map(|d| d.motion.as_ref()), chassis: &self.chassis, links: &links, selected };
        graphs::charts(&self.graphs, &cx)
    }
    /// `robot_state.graphs`: visible, mode, generation, window and the charts with their traces.
    pub fn graphs_json(&self, selected: Option<usize>, visible: bool) -> Value {
        graphs::json(&self.graphs, &self.graph_charts(selected), visible, self.graphs_mode())
    }
    pub fn graphs(&self) -> &graphs::History {
        &self.graphs
    }

    /// Frames need drawing while the worker is building or running.
    pub fn active(&self) -> bool {
        self.playback.as_ref().is_some_and(|p| p.playing() || p.pending()) || self.gait.as_ref().is_some_and(GaitPreview::active) || matches!(self.status.phase, Phase::Building | Phase::Running) || self.running || self.replay.phase == ReplayPhase::Replaying || self.saving.is_some() || self.listing.pending().is_some()
    }

    /// `robot_state.run`: phase, time, steps, chunk, rtf, generation, error
    /// and the latest accepted frame. time/steps are null until a frame of
    /// the current generation exists (idle before any run).
    pub fn state_json(&self, links: &[String]) -> Value {
        let f = self.frame.as_ref();
        let poses: Option<Vec<Value>> = f.map(|f| {
            f.poses.iter().enumerate().map(|(i, pose)| {
                let (v, w) = f.velocities.get(i).copied().flatten().unzip();
                match pose {
                    Some((p, q)) => json!({"link": links.get(i), "position": p, "quat_xyzw": [q.x, q.y, q.z, q.w], "velocity_m_s": v, "angular_velocity_rad_s": w}),
                    None => json!({"link": links.get(i), "position": null, "quat_xyzw": null, "velocity_m_s": v, "angular_velocity_rad_s": w}),
                }
            }).collect()
        });
        let build = match &self.preset {
            None if self.recorded.is_some() => format!("nothing is built: {}", crate::robot::preset::RECORDED_RUNS_AS),
            None => "sim_runtime::physical::PhysicalRobot::build(model clone, sim_runtime::registry(), BuildOptions::default()) on the run thread".to_string(),
            Some(p) if p.task.is_some() => format!("sim_runtime::environment::EmbeddedEnvironment::new(scene, config, task, seed {}) from the preset's files unchanged, on the run thread", p.seed),
            Some(p) => format!("sim_runtime::embedded::EmbeddedSession::new(scene, config, seed {}, CaptureMode::Latest) from the preset's files unchanged, on the run thread", p.seed),
        };
        let without: Option<Vec<&String>> = f.map(|f| f.poses.iter().enumerate().filter(|(_, p)| p.is_none()).filter_map(|(i, _)| links.get(i)).collect());
        json!({"phase": self.status.phase, "time": f.map(|f| f.time), "steps": f.map(|f| f.steps), "chunk_s": self.chunk_s,
            "rtf": self.status.rtf, "speed_scale": self.speed_scale, "speed_scales": SPEED_SCALES, "compute_limited": self.compute_limited(), "compute_limited_rule": COMPUTE_LIMITED_RULE, "generation": self.generation, "error": self.status.error, "end": self.status.end, "frame_generation": f.map(|f| f.generation),
            "completed_steps": f.and_then(|f| f.completed_steps),
            "joints": f.map(|f| &f.joint_names), "joint_angles": f.map(|f| &f.joint_angles), "targets": f.map(|f| &f.targets), "poses": poses,
            "unmatched_frame_links": f.map(|f| &f.unmatched), "links_without_pose": without,
            "build": build,
            "steps_unit": "chunks of chunk_s since the last build", "pacing": PACING, "poses_frame": "link frame at its com, model frame (Z up); velocity_m_s / angular_velocity_rad_s are the session frame's published world-frame velocities (null for --robot FILE, which publishes none)"})
    }
}
