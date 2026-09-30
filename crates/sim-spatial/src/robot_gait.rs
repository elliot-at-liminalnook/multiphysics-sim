//! Robot-mode gait preview: a gait-lab gait posed on the loaded preset's
//! robot, geometry only. One worker thread owns everything expensive: it
//! reads the compiled gait through the shared
//! `sim_runtime::gait_playback::compiled_with_governor` and `Gait::from_compiled`,
//! samples it on the shared `Clock` (through `GovernedGait::step` while
//! playing, as the browser's calibration-mirror `sampleGait`), and solves the
//! pose with the shared `sim_runtime::kinematic_mirror::KinematicMirror`
//! built from the preset's scene. It also lists the tracked gait-lab reports
//! with `sim_runtime::gait_lab::scan_results`. The UI thread only sends
//! commands and takes the latest generation-stamped state. Nothing is
//! simulated, written or sent to hardware.
use bevy::math::{DMat3, DQuat};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sim_runtime::gait_lab::{LabReport, scan_results};
use sim_runtime::gait_playback::{Clock, Gait, GovernedGait, GovernorSource, compiled_with_governor};
use sim_runtime::kinematic_mirror::{KinematicMirror, MirrorCoordinate};
use sim_runtime::session::LinkPose;
use crate::robot_preset::PresetRun;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

/// Suspension lift of the mirrored robot (m): the browser's `LIFT_M` in
/// web/viewer/calibration-mirror.mjs:4, so both surfaces pose the same way.
pub const LIFT_M: f64 = 0.25;
pub const LABEL: &str = "kinematic preview (geometry only, suspended) — not a physics result";
/// Shortest wall time between played frames (s). The worker solves each pose
/// synchronously, so a slower solve lowers the rate by itself; this caps it at
/// 30 frames/s. Measured on the 29-link robot-measured-400hz scene in the dev
/// profile (opt-level with debuginfo): about 6 ms per played pose (small moves),
/// up to about 40 ms for a seek across the cycle (KinematicMirror::pose walks
/// large moves in 0.1 rad steps).
pub const FRAME_S: f64 = 1.0 / 30.0;
pub const SAMPLING_RULE: &str = "open and seek(t) reset the governor, so the pose is the raw sampler value Gait::sample(t) (drives = desired); play advances the shared Clock by wall time × speed_scale (0 < scale ≤ 1) and steps GovernedGait::step(t, dt, scale) as the browser's calibration-mirror sampleGait does, so with a governor the pose is the governed value (drives = commanded); pause keeps the governor state; coordinates the gait does not drive stay at their CAD home";
pub const LISTING_RULE: &str = "gait reports (no kind) under <workspace root>/examples/full-robot/measured-actuator-integration/gait-lab-2026-09-25/results read with sim_runtime::gait_lab::scan_results; offered only when report.compiled_gait (relative to the workspace root, or absolute) exists; status, fidelity and speed are the report's own, verbatim";

/// Where a preview's gait comes from.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum GaitSource {
    /// A tracked report by its results directory name (robot_state.gait_preview.reports[].name).
    Report(String),
    /// A compiled.json, relative to the workspace root or absolute.
    Path(String),
}

/// The one gait-preview handler's actions (inspector, `system_ui`, REST).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum GaitAction {
    Open { source: GaitSource },
    Play,
    Pause,
    Seek { t: f64 },
    Speed { scale: f64 },
    Stop,
    /// Scan the tracked results again (off the UI thread).
    List,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GaitPhase {
    Idle,
    Loading,
    Playing,
    Paused,
    Failed,
}

/// One offered tracked report.
#[derive(Clone, Debug, Serialize)]
pub struct Listed {
    pub name: String,
    pub gait: String,
    pub compiled: PathBuf,
    pub status: String,
    pub fidelity: String,
    pub speed_m_s: Option<f64>,
    pub summary: String,
}

/// The loaded gait's identity.
#[derive(Clone, Debug, Serialize)]
pub struct Loaded {
    pub name: String,
    pub compiled: PathBuf,
    /// The tracked report it was opened from (None for an explicit path).
    pub report: Option<Listed>,
    pub governor_source: GovernorSource,
    pub governor: Value,
    pub joints: Vec<String>,
    pub period_s: f64,
    pub nominal_speed_m_s: f64,
}

/// One solved pose.
#[derive(Clone, Debug)]
pub struct Sample {
    pub generation: u64,
    pub seq: u64,
    pub gait_time_s: f64,
    /// Raw sampler (Gait::sample) and governed (GovernedGait::step) angles, in `Loaded::joints` order.
    pub desired: Vec<f64>,
    pub commanded: Vec<f64>,
    pub drives: &'static str,
    /// Per loaded link (by index), mapped by name from the mirror's poses.
    pub poses: Vec<Option<([f64; 3], DQuat)>>,
    pub unmatched: Vec<String>,
    pub authored_limit_violations: Vec<String>,
    pub maximum_scaled_closure_error: f64,
    pub solve_s: f64,
}

/// What the worker publishes (stamped with the generation of the last command it applied).
#[derive(Clone, Debug)]
struct State {
    generation: u64,
    phase: GaitPhase,
    error: Option<String>,
    loaded: Option<Arc<Loaded>>,
    clock: Clock,
    sample: Option<Sample>,
    mirror: Option<Result<MirrorInfo, String>>,
}
#[derive(Clone, Debug, Serialize)]
struct MirrorInfo {
    coordinates: usize,
    build_s: f64,
}

struct Shared {
    state: State,
    listing: Option<(u64, Result<(Vec<Listed>, Vec<String>), String>)>,
}

enum Command {
    Open { generation: u64, source: GaitSource },
    Play { generation: u64 },
    Pause { generation: u64 },
    Seek { generation: u64, t: f64 },
    Speed { generation: u64, scale: f64 },
    Stop { generation: u64 },
    List { seq: u64 },
}

/// The UI side of the gait worker.
pub struct GaitPreview {
    tx: mpsc::Sender<Command>,
    shared: Arc<Mutex<Shared>>,
    preset: String,
    root: PathBuf,
    /// Generation the UI expects; every command bumps it and older states are stale.
    generation: u64,
    state: State,
    /// The source of an open not yet applied (phase loading).
    opening: Option<GaitSource>,
    list_requested: u64,
    list_done: u64,
    reports: Vec<Listed>,
    skipped: Vec<String>,
    list_error: Option<String>,
}

impl GaitPreview {
    /// Spawns the idle worker for a preset and lists the tracked reports.
    pub fn spawn(run: Arc<PresetRun>, links: Vec<String>) -> Self {
        let (tx, rx) = mpsc::channel();
        let state = State { generation: 0, phase: GaitPhase::Idle, error: None, loaded: None, clock: idle_clock(), sample: None, mirror: None };
        let shared = Arc::new(Mutex::new(Shared { state: state.clone(), listing: None }));
        let out = shared.clone();
        let (preset, root) = (run.preset.id.clone(), run.root.clone());
        std::thread::Builder::new().name("robot-gait".into()).spawn(move || worker(run, links, rx, out)).expect("spawn gait preview thread");
        let mut g = Self { tx, shared, preset, root, generation: 0, state, opening: None, list_requested: 0, list_done: 0, reports: Vec::new(), skipped: Vec::new(), list_error: None };
        let _ = g.act(GaitAction::List);
        g
    }

    /// Whether a gait is loaded or loading (physics runs and replays are refused meanwhile).
    pub fn holds(&self) -> Option<String> {
        if self.opening.is_some() {
            return Some("a gait preview is loading".into());
        }
        let l = self.state.loaded.as_ref()?;
        Some(format!("a gait preview of `{}` is {} ({LABEL})", l.name, if self.state.phase == GaitPhase::Playing { "playing" } else { "loaded" }))
    }
    pub fn phase(&self) -> GaitPhase {
        if self.opening.is_some() { GaitPhase::Loading } else { self.state.phase }
    }

    /// Why `action` is refused by the preview itself (run/replay exclusion is the caller's).
    pub fn check(&self, action: &GaitAction) -> Result<(), String> {
        let loaded = || self.state.loaded.as_ref().ok_or_else(|| "no gait preview is open; open a tracked report or a compiled.json path first".to_string());
        match action {
            GaitAction::Open { source } => {
                if let Some(s) = &self.opening {
                    return Err(format!("a gait preview is still loading ({s:?}); wait for it before opening another"));
                }
                match source {
                    GaitSource::Report(name) if self.list_done >= self.list_requested && !self.reports.iter().any(|r| &r.name == name) => Err(format!(
                        "no tracked gait report `{name}`; offered: {}",
                        if self.reports.is_empty() { "none".into() } else { self.reports.iter().map(|r| r.name.as_str()).collect::<Vec<_>>().join(", ") }
                    )),
                    GaitSource::Path(p) => {
                        let path = self.resolve(p);
                        if path.is_file() { Ok(()) } else { Err(format!("gait file `{}`: not found (a compiled.json, relative to the workspace root {} or absolute)", path.display(), self.root.display())) }
                    }
                    _ => Ok(()),
                }
            }
            GaitAction::Play => {
                loaded()?;
                if self.state.phase == GaitPhase::Playing { Err("the gait preview is already playing".into()) } else { Ok(()) }
            }
            GaitAction::Pause => {
                loaded()?;
                if self.state.phase == GaitPhase::Playing { Ok(()) } else { Err("the gait preview is not playing".into()) }
            }
            GaitAction::Seek { t } => {
                loaded()?;
                if t.is_finite() { Ok(()) } else { Err(format!("gait time {t} is not finite")) }
            }
            GaitAction::Speed { scale } => {
                if scale.is_finite() && *scale > 0.0 && *scale <= 1.0 { Ok(()) } else { Err(format!("gait speed scale {scale} is outside (0, 1] (the shared gait_playback::Clock clamps to [0, 1]; 0 is Pause)")) }
            }
            GaitAction::Stop => loaded().map(|_| ()).or_else(|e| if self.opening.is_some() { Ok(()) } else { Err(e) }),
            GaitAction::List => if self.list_done < self.list_requested { Err("the tracked gait reports are already being listed".into()) } else { Ok(()) },
        }
    }

    fn resolve(&self, p: &str) -> PathBuf {
        let p = Path::new(p);
        if p.is_absolute() { p.to_path_buf() } else { self.root.join(p) }
    }

    /// The one handler (after the caller's run/replay exclusion check).
    pub fn act(&mut self, action: GaitAction) -> Result<(), String> {
        self.check(&action)?;
        let send = |tx: &mpsc::Sender<Command>, c| tx.send(c).map_err(|_| "the gait preview thread has stopped".to_string());
        if let GaitAction::List = action {
            self.list_requested += 1;
            return send(&self.tx, Command::List { seq: self.list_requested });
        }
        self.generation += 1;
        let generation = self.generation;
        let command = match action {
            GaitAction::Open { source } => {
                let source = match source {
                    GaitSource::Path(p) => GaitSource::Path(self.resolve(&p).display().to_string()),
                    s => s,
                };
                self.opening = Some(source.clone());
                Command::Open { generation, source }
            }
            GaitAction::Play => Command::Play { generation },
            GaitAction::Pause => Command::Pause { generation },
            GaitAction::Seek { t } => Command::Seek { generation, t },
            GaitAction::Speed { scale } => Command::Speed { generation, scale },
            GaitAction::Stop => {
                // The live frame shows again at once; the worker's older states are stale.
                self.opening = None;
                self.state = State { generation, phase: GaitPhase::Idle, error: None, loaded: None, clock: Clock { speed_scale: self.state.clock.speed_scale, ..idle_clock() }, sample: None, mirror: self.state.mirror.clone() };
                Command::Stop { generation }
            }
            GaitAction::List => unreachable!("handled above"),
        };
        send(&self.tx, command)
    }

    /// Takes the worker's latest state; true when the displayed pose changed.
    pub fn poll(&mut self) -> bool {
        let shared = self.shared.clone();
        let s = shared.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((seq, result)) = s.listing.as_ref().filter(|(seq, _)| *seq > self.list_done) {
            self.list_done = *seq;
            match result {
                Ok((reports, skipped)) => {
                    self.reports = reports.clone();
                    self.skipped = skipped.clone();
                    self.list_error = None;
                }
                Err(e) => self.list_error = Some(e.clone()),
            }
        }
        if s.state.generation < self.generation {
            return false;
        }
        let before = self.state.sample.as_ref().map(|x| (x.generation, x.seq));
        self.state = s.state.clone();
        self.opening = None;
        before != self.state.sample.as_ref().map(|x| (x.generation, x.seq))
    }
    /// The pose to draw instead of the run's frame, while a gait is loaded.
    pub fn poses(&self) -> Option<&[Option<([f64; 3], DQuat)>]> {
        self.state.loaded.as_ref()?;
        self.state.sample.as_ref().map(|s| s.poses.as_slice())
    }
    pub fn active(&self) -> bool {
        self.opening.is_some() || self.state.phase == GaitPhase::Playing || self.list_done < self.list_requested
    }
    pub fn reports(&self) -> &[Listed] {
        &self.reports
    }
    /// The loaded gait (None while idle, loading its first gait or after Stop).
    pub fn loaded(&self) -> Option<&Loaded> {
        self.state.loaded.as_deref()
    }
    /// The latest solved pose.
    pub fn sample(&self) -> Option<&Sample> {
        self.state.sample.as_ref()
    }
    /// The last load or solve error, as in `robot_state.gait_preview.error`.
    pub fn error(&self) -> Option<&str> {
        self.state.error.as_deref()
    }
    pub fn speed_scale(&self) -> f64 {
        self.state.clock.speed_scale
    }
    /// Why the listing failed, if it did.
    pub fn list_error(&self) -> Option<&str> {
        self.list_error.as_deref()
    }

    /// `robot_state.gait_preview`.
    pub fn json(&self, block: Option<String>) -> Value {
        let s = &self.state;
        let l = s.loaded.as_deref();
        let by_joint = |v: &[f64]| -> serde_json::Map<String, Value> { l.map(|l| l.joints.iter().zip(v).map(|(j, q)| (j.clone(), json!(q))).collect()).unwrap_or_default() };
        let x = s.sample.as_ref();
        let mut v = json!({
            "label": LABEL, "phase": self.phase(), "error": s.error, "generation": self.generation, "applied_generation": s.generation, "frame_generation": x.map(|x| x.generation), "frame_seq": x.map(|x| x.seq),
            "opening": self.opening, "preset": self.preset,
            "report": l.and_then(|l| l.report.as_ref()).map(|r| &r.name), "name": l.map(|l| &l.name), "compiled": l.map(|l| &l.compiled),
            "governor_source": l.map(|l| l.governor_source), "governor": l.map(|l| &l.governor),
            "period_s": l.map(|l| l.period_s), "nominal_speed_m_s": l.map(|l| l.nominal_speed_m_s),
            "report_speed_m_s": l.and_then(|l| l.report.as_ref()).and_then(|r| r.speed_m_s),
            "status": l.and_then(|l| l.report.as_ref()).map(|r| &r.status), "fidelity": l.and_then(|l| l.report.as_ref()).map(|r| &r.fidelity),
            "gait_time_s": x.map(|x| x.gait_time_s), "clock_gait_time_s": l.map(|_| s.clock.gait_time_s), "speed_scale": s.clock.speed_scale,
            "joints": l.map(|l| &l.joints), "desired_rad": x.map(|x| by_joint(&x.desired)), "commanded_rad": x.map(|x| by_joint(&x.commanded)), "drives": x.map(|x| x.drives),
        });
        let more = json!({
            "lift_m": LIFT_M, "lift_source": "web/viewer/calibration-mirror.mjs:4 LIFT_M",
            "authored_limit_violations": x.map(|x| &x.authored_limit_violations), "maximum_scaled_closure_error": x.map(|x| x.maximum_scaled_closure_error),
            "unmatched_pose_links": x.map(|x| &x.unmatched), "solve_ms": x.map(|x| x.solve_s * 1e3), "frame_interval_min_ms": FRAME_S * 1e3,
            "mirror": s.mirror.as_ref().map(|m| match m { Ok(m) => json!(m), Err(e) => json!({"error": e}) }),
            "available": block.is_none(), "unavailable_reason": block,
            "reports": self.reports, "reports_skipped": self.skipped, "reports_error": self.list_error, "reports_pending": self.list_done < self.list_requested,
            "listing_rule": LISTING_RULE, "sampling_rule": SAMPLING_RULE,
            "exclusion_rule": "a gait preview is refused while a physics run is running or a replay is in progress; Run, Step and Replay are refused while a gait is loaded or loading; Stop ends the preview and the run's live frame (or the assembly pose) shows again",
        });
        if let (Value::Object(v), Value::Object(more)) = (&mut v, more) {
            v.extend(more);
        }
        v
    }
}

fn idle_clock() -> Clock {
    Clock { gait_time_s: 0.0, speed_scale: 1.0, playing: false }
}

/// Link poses from a mirror solve, mapped to the loaded links by name (the
/// same row-major rotation as a session frame's poses).
pub fn map_poses(poses: &[LinkPose], links: &[String]) -> (Vec<Option<([f64; 3], DQuat)>>, Vec<String>) {
    let mut out = vec![None; links.len()];
    let mut unmatched = Vec::new();
    for p in poses {
        let m = p.rotation;
        let r = DMat3::from_cols([m[0][0], m[1][0], m[2][0]].into(), [m[0][1], m[1][1], m[2][1]].into(), [m[0][2], m[1][2], m[2][2]].into());
        match links.iter().position(|l| *l == p.name) {
            Some(i) => out[i] = Some((p.position_m, DQuat::from_mat3(&r).normalize())),
            None => unmatched.push(p.name.clone()),
        }
    }
    (out, unmatched)
}

/// The tracked gait reports with an existing compiled gait (and why others are skipped).
pub fn list_reports(root: &Path) -> Result<(Vec<Listed>, Vec<String>), String> {
    let listing = scan_results(&root.join(crate::builder::gait_lab::DEFAULT_RESULTS))?;
    let (mut offered, mut skipped) = (Vec::new(), Vec::new());
    for e in listing.entries {
        match e.report {
            Ok(LabReport::Gait(r)) => match r.compiled_gait.as_deref() {
                None => skipped.push(format!("{}: report has no compiled_gait", e.name)),
                Some(rel) => {
                    let path = if Path::new(rel).is_absolute() { PathBuf::from(rel) } else { root.join(rel) };
                    if path.is_file() {
                        offered.push(Listed { name: e.name, gait: r.gait, compiled: path, status: r.status, fidelity: r.fidelity, speed_m_s: r.speed_m_s, summary: r.summary });
                    } else {
                        skipped.push(format!("{}: compiled gait {} not found", e.name, path.display()));
                    }
                }
            },
            Ok(LabReport::Pose(_)) => skipped.push(format!("{}: a pose_sequence report, not a gait", e.name)),
            Ok(LabReport::Maneuver(_)) => skipped.push(format!("{}: a maneuver report, not a gait", e.name)),
            Err(err) => skipped.push(err),
        }
    }
    Ok((offered, skipped))
}

/// A loaded gait on the worker.
struct Current {
    loaded: Arc<Loaded>,
    gait: Gait,
    governed: GovernedGait,
    /// Per mirror coordinate, the gait joint driving it.
    index: Vec<Option<usize>>,
    homes: Vec<f64>,
}

/// Read, parse and validate a gait against the mirror's coordinates (errors name the path or joint).
fn open(source: &GaitSource, reports: &[Listed], coordinates: &[MirrorCoordinate], preset: &str) -> Result<Current, String> {
    let (path, report) = match source {
        GaitSource::Report(name) => {
            let r = reports.iter().find(|r| &r.name == name).ok_or_else(|| format!("no tracked gait report `{name}`"))?;
            (r.compiled.clone(), Some(r.clone()))
        }
        GaitSource::Path(p) => (PathBuf::from(p), None),
    };
    let (compiled, governor_source) = compiled_with_governor(&path)?;
    let name = report.as_ref().map_or_else(|| path.parent().and_then(|d| d.file_name()).map_or("gait".into(), |n| n.to_string_lossy().into_owned()), |r| r.name.clone());
    let gait = Gait::from_compiled(&compiled, &name).map_err(|e| format!("{}: {e}", path.display()))?;
    let missing: Vec<&String> = gait.info.joints.iter().filter(|j| !coordinates.iter().any(|c| &c.joint == *j)).collect();
    if !missing.is_empty() {
        return Err(format!("{}: gait joint{} {} {} not a coordinate of preset `{preset}`'s scene (its motor joints: {})", path.display(), if missing.len() > 1 { "s" } else { "" },
            missing.iter().map(|j| format!("`{j}`")).collect::<Vec<_>>().join(", "), if missing.len() > 1 { "are" } else { "is" },
            coordinates.iter().map(|c| c.joint.as_str()).collect::<Vec<_>>().join(", ")));
    }
    let index = coordinates.iter().map(|c| gait.index(&c.joint)).collect();
    let loaded = Arc::new(Loaded {
        name, compiled: path, report, governor_source, governor: compiled["playback_governor"].clone(),
        joints: gait.info.joints.clone(), period_s: gait.info.period_s, nominal_speed_m_s: gait.info.nominal_speed_m_s,
    });
    Ok(Current { loaded, governed: GovernedGait::new(gait.clone()), gait, index, homes: coordinates.iter().map(|c| c.home).collect() })
}

fn worker(run: Arc<PresetRun>, links: Vec<String>, rx: mpsc::Receiver<Command>, out: Arc<Mutex<Shared>>) {
    let preset = run.preset.id.clone();
    let mut mirror: Option<Result<(KinematicMirror, Vec<MirrorCoordinate>), String>> = None;
    let mut mirror_info: Option<Result<MirrorInfo, String>> = None;
    let mut reports: Option<Vec<Listed>> = None;
    let mut current: Option<Current> = None;
    let mut state = State { generation: 0, phase: GaitPhase::Idle, error: None, loaded: None, clock: idle_clock(), sample: None, mirror: None };
    let mut seq = 0u64;
    let mut last_tick = Instant::now();
    let publish = |state: &State| out.lock().unwrap_or_else(|p| p.into_inner()).state = state.clone();

    // Solve and stamp one pose; `step` is Some(dt) while playing (governed), None at open/seek (raw sampler, governor reset).
    let solve = |cur: &mut Current, mirror: &mut KinematicMirror, clock: &Clock, step: Option<f64>, generation: u64, seq: &mut u64| -> Result<Sample, String> {
        let t = clock.gait_time_s;
        let desired = cur.gait.sample(t)?;
        let (commanded, drives) = match step {
            None => {
                cur.governed = GovernedGait::new(cur.gait.clone());
                (desired.clone(), "desired")
            }
            Some(dt) => {
                let q: Vec<f64> = cur.governed.step(t, dt, clock.speed_scale)?.into_iter().map(|(q, _)| q).collect();
                (q, if cur.gait.info.governor.is_some() { "commanded" } else { "desired" })
            }
        };
        let values: Vec<f64> = cur.index.iter().zip(&cur.homes).map(|(i, h)| i.map_or(*h, |i| commanded[i])).collect();
        let t0 = Instant::now();
        let pose = mirror.pose(&values)?;
        let solve_s = t0.elapsed().as_secs_f64();
        let (poses, unmatched) = map_poses(&pose.poses, &links);
        *seq += 1;
        Ok(Sample { generation, seq: *seq, gait_time_s: t, desired, commanded, drives, poses, unmatched, authored_limit_violations: pose.authored_limit_violations, maximum_scaled_closure_error: pose.maximum_scaled_closure_error, solve_s })
    };

    loop {
        let playing = state.phase == GaitPhase::Playing;
        let command = if playing {
            let wait = Duration::from_secs_f64(FRAME_S).saturating_sub(last_tick.elapsed());
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
        let Some(command) = command else {
            // A played frame.
            let (Some(cur), Some(Ok((m, _)))) = (current.as_mut(), mirror.as_mut()) else { continue };
            let dt = last_tick.elapsed().as_secs_f64();
            last_tick = Instant::now();
            state.clock.advance(dt);
            match solve(cur, m, &state.clock, Some(dt), state.generation, &mut seq) {
                Ok(s) => state.sample = Some(s),
                Err(e) => {
                    state.phase = GaitPhase::Paused;
                    state.clock.playing = false;
                    state.error = Some(format!("pose not solved at gait time {:.4} s: {e}; paused", state.clock.gait_time_s));
                }
            }
            publish(&state);
            continue;
        };
        match command {
            Command::List { seq: n } => {
                let result = list_reports(&run.root);
                if let Ok((r, _)) = &result {
                    reports = Some(r.clone());
                }
                out.lock().unwrap_or_else(|p| p.into_inner()).listing = Some((n, result));
                continue;
            }
            Command::Open { generation, source } => {
                state.generation = generation;
                if mirror.is_none() {
                    let t0 = Instant::now();
                    let built = KinematicMirror::new(run.scene.clone(), LIFT_M).map(|m| {
                        let c = m.coordinates();
                        (m, c)
                    }).map_err(|e| format!("preset `{preset}`: the kinematic mirror cannot serve its scene: {e}"));
                    mirror_info = Some(built.as_ref().map(|(_, c)| MirrorInfo { coordinates: c.len(), build_s: t0.elapsed().as_secs_f64() }).map_err(|e| e.clone()));
                    mirror = Some(built);
                    state.mirror = mirror_info.clone();
                }
                if reports.is_none() {
                    reports = list_reports(&run.root).ok().map(|r| r.0);
                }
                let opened = match mirror.as_mut().expect("built above") {
                    Err(e) => Err(e.clone()),
                    Ok((m, coordinates)) => open(&source, reports.as_deref().unwrap_or_default(), coordinates, &preset).and_then(|mut cur| {
                        let clock = Clock { gait_time_s: 0.0, speed_scale: state.clock.speed_scale, playing: false };
                        let s = solve(&mut cur, m, &clock, None, generation, &mut seq)?;
                        Ok((cur, clock, s))
                    }),
                };
                match opened {
                    Ok((cur, clock, s)) => {
                        state.loaded = Some(cur.loaded.clone());
                        current = Some(cur);
                        state.clock = clock;
                        state.sample = Some(s);
                        state.phase = GaitPhase::Paused;
                        state.error = None;
                    }
                    // Refused: any previous preview stays as it was.
                    Err(e) => {
                        state.error = Some(e);
                        if current.is_none() {
                            state.phase = GaitPhase::Failed;
                        }
                    }
                }
            }
            Command::Play { generation } => {
                state.generation = generation;
                if current.is_some() {
                    state.phase = GaitPhase::Playing;
                    state.clock.playing = true;
                    state.error = None;
                    last_tick = Instant::now();
                }
            }
            Command::Pause { generation } => {
                state.generation = generation;
                if current.is_some() {
                    state.phase = GaitPhase::Paused;
                    state.clock.playing = false;
                }
            }
            Command::Speed { generation, scale } => {
                state.generation = generation;
                state.clock.speed_scale = scale;
            }
            Command::Seek { generation, t } => {
                state.generation = generation;
                if let (Some(cur), Some(Ok((m, _)))) = (current.as_mut(), mirror.as_mut()) {
                    state.clock.gait_time_s = t;
                    match solve(cur, m, &state.clock, None, generation, &mut seq) {
                        Ok(s) => {
                            state.sample = Some(s);
                            state.error = None;
                        }
                        Err(e) => state.error = Some(format!("pose not solved at gait time {t} s: {e}")),
                    }
                    last_tick = Instant::now();
                }
            }
            Command::Stop { generation } => {
                current = None;
                state = State { generation, phase: GaitPhase::Idle, error: None, loaded: None, clock: Clock { speed_scale: state.clock.speed_scale, ..idle_clock() }, sample: None, mirror: mirror_info.clone() };
            }
        }
        publish(&state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::robot_run::{RunAction, RunController};

    fn wait(c: &mut RunController, what: &str, done: impl Fn(&Value) -> bool) -> Value {
        let start = Instant::now();
        loop {
            c.poll();
            let v = c.gait_json();
            if done(&v) {
                return v;
            }
            assert!(start.elapsed() < Duration::from_secs(120), "timed out waiting for {what}: {v}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn applied(v: &Value) -> bool {
        v["applied_generation"] == v["generation"] && v["phase"] != "loading"
    }

    /// robot-measured-400hz (29 links): the only preset family whose scene has
    /// the tracked gaits' twelve motor joints.
    #[test]
    fn gait_preview_seeks_to_the_shared_sampler_refuses_by_name_and_excludes_runs() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
        let p = crate::robot_preset::select(&root.join(crate::robot_preset::PRESETS), &root, "robot-measured-400hz").unwrap();
        let (loaded, run) = crate::robot::load_preset(p, &root).unwrap();
        assert_eq!(loaded.model.links.len(), 29);
        let mut c = RunController::spawn_preset(Arc::new(run));
        let v = wait(&mut c, "the listing", |v| v["reports_pending"] == false);
        let name = "6216-Bayesian-009-472d11d4";
        let listed: Vec<&str> = v["reports"].as_array().unwrap().iter().map(|r| r["name"].as_str().unwrap()).collect();
        assert!(listed.contains(&name), "{listed:?}");

        // Refused by name, before anything is loaded.
        let e = c.gait(GaitAction::Open { source: GaitSource::Path("no/such/compiled.json".into()) }).unwrap_err();
        assert!(e.contains("no/such/compiled.json") && e.contains("not found"), "{e}");
        let e = c.gait(GaitAction::Open { source: GaitSource::Report("no-such-report".into()) }).unwrap_err();
        assert!(e.contains("`no-such-report`"), "{e}");
        assert!(c.gait(GaitAction::Play).unwrap_err().contains("no gait preview is open"));

        c.gait(GaitAction::Open { source: GaitSource::Report(name.into()) }).unwrap();
        let v = wait(&mut c, "the open", |v| applied(v) && v["phase"] != "idle");
        assert_eq!(v["phase"], "paused", "{v}");
        println!("mirror {}", v["mirror"]);
        let dir = root.join(crate::builder::gait_lab::DEFAULT_RESULTS).join(name);
        assert_eq!(v["governor_source"], "spec_identity");
        assert_eq!(v["label"], LABEL);
        assert_eq!(v["lift_m"], 0.25);
        let report = std::fs::read_to_string(dir.join("report.yaml")).unwrap();
        assert!(report.contains(&format!("status: {}", v["status"].as_str().unwrap())));
        assert!(report.contains(v["fidelity"].as_str().unwrap()), "fidelity verbatim");

        // Physics Run/Step and Replay are refused while a gait is loaded.
        for a in [RunAction::Start, RunAction::Step] {
            let e = c.act(a).unwrap_err();
            assert!(e.contains("gait preview") && e.contains(name), "{e}");
        }
        assert!(c.check_replay().err().unwrap().contains("gait preview"));

        // Seek angles equal an independently loaded Gait::sample(t).
        let (compiled, source) = compiled_with_governor(&dir.join("compiled.json")).unwrap();
        assert_eq!(source, GovernorSource::SpecIdentity);
        let gait = Gait::from_compiled(&compiled, name).unwrap();
        assert!(gait.info.governor.is_some());
        let period = gait.info.period_s;
        assert_eq!(v["period_s"].as_f64(), Some(period));
        let mut solves = Vec::new();
        for k in 0..12 {
            let t = period * k as f64 / 12.0;
            c.gait(GaitAction::Seek { t }).unwrap();
            let v = wait(&mut c, "a seek", applied);
            assert_eq!(v["gait_time_s"].as_f64(), Some(t));
            assert_eq!(v["drives"], "desired");
            let expected = gait.sample(t).unwrap();
            for (i, j) in gait.info.joints.iter().enumerate() {
                assert_eq!(v["desired_rad"][j].as_f64(), Some(expected[i]), "{j} at t = {t}");
                assert_eq!(v["commanded_rad"][j].as_f64(), Some(expected[i]), "{j} at t = {t}");
            }
            assert!(c.display_poses().unwrap().iter().all(Option::is_some), "every link posed");
            solves.push(v["solve_ms"].as_f64().unwrap());
        }
        solves.sort_by(f64::total_cmp);
        println!("solve per pose (ms) over 12 seeks: min {:.2}, median {:.2}, max {:.2}", solves[0], solves[6], solves[11]);

        // Play advances the clock and poses the governed value.
        c.gait(GaitAction::Seek { t: 0.0 }).unwrap();
        c.gait(GaitAction::Speed { scale: 0.5 }).unwrap();
        c.gait(GaitAction::Play).unwrap();
        std::thread::sleep(Duration::from_millis(400));
        let v = wait(&mut c, "playing frames", |v| applied(v) && v["drives"] == "commanded");
        assert_eq!(v["phase"], "playing");
        let t = v["gait_time_s"].as_f64().unwrap();
        assert!(t > 0.1 && t < 0.5, "gait time {t} after ~0.4 s at scale 0.5");
        c.gait(GaitAction::Pause).unwrap();

        // A gait whose joint the scene lacks is refused by name; the preview stays.
        let mut bad = compiled.clone();
        bad["recipe"]["independent_coordinates"][0] = json!("joint.No such joint");
        let tmp = std::env::temp_dir().join(format!("gait-preview-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let bad_path = tmp.join("compiled.json");
        std::fs::write(&bad_path, bad.to_string()).unwrap();
        c.gait(GaitAction::Open { source: GaitSource::Path(bad_path.display().to_string()) }).unwrap();
        let v = wait(&mut c, "the refused open", |v| applied(v) && !v["error"].is_null());
        let e = v["error"].as_str().unwrap();
        assert!(e.contains("`No such joint`") && e.contains("robot-measured-400hz"), "{e}");
        assert_eq!(v["report"], name, "the previous preview stays");
        std::fs::remove_dir_all(&tmp).unwrap();

        // Stop restores the live frame (none built: the assembly pose) and frees Run.
        c.gait(GaitAction::Stop).unwrap();
        assert!(c.display_poses().is_none());
        assert_eq!(c.gait_json()["phase"], "idle");
        c.check(RunAction::Start).unwrap();

        // A running physics run refuses a preview, naming it.
        c.act(RunAction::Start).unwrap();
        let e = c.gait(GaitAction::Open { source: GaitSource::Report(name.into()) }).unwrap_err();
        assert!(e.contains("physics run is running"), "{e}");
        c.act(RunAction::Pause).unwrap();
    }
}
