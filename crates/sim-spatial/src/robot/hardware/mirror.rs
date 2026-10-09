//! The suspended simulated leg mirror: the port of
//! `web/viewer/calibration-mirror.mjs` (`LegMirror`). The real leg's
//! measured encoders pose the preset's CAD robot, held still and lifted
//! [`LIFT_M`], through the shared `sim_runtime::kinematic_mirror::KinematicMirror`
//! on a `jobs::RunThread` ("hardware-mirror", the page's worker). Geometry
//! only: no forces, contact or motor model, and nothing here commands motors.
//!
//! - **Display.** While shown, `mirror_panel` writes the solved poses to
//!   `RobotView::mirror` (drawn instead of the run's frame) with the mirrored
//!   leg's links tinted blue (the page's `robotViewer.begin/show/end`), and
//!   Robot mode refuses Run, Step and Reset by name ([`refuse_run`], [`MIRRORING`]).
//! - **Latest wins** (the page's `busy`/`queued`, :127-136): the worker drains
//!   its channel and solves only the newest pose request.
//! - **Gait sim sampling** (calibration-ui.mjs:241-270): a gait playing in Sim
//!   or Both mode is sampled on the worker with the shared
//!   `sim_runtime::gait_playback` (governed when the gait has a governor, as
//!   `sim-web`'s `GaitPlayer::governed`); in Both the bound leg's motors stay
//!   on the encoders. Sim samples at the link thread's [`GaitRun::t`]; Both
//!   at the leg's clock ([`super::link::LinkSnapshot::leg_clock`]), the one
//!   the server's leg runs on, so the simulated legs and the real one keep
//!   step. While the leg's data is not live the gait pose is held.
//! - **Live data only** ([`Mirror::set_leg_health`]): the encoders pose the
//!   leg only while the link is live. Stale or disconnected, the last solved
//!   pose stays and the status line says it is not live, with its age.
//! - **Preferences**: the page's `calibration-mirror-v1` is
//!   `settings::MirrorSettings`, saved on every change.
mod apply;
mod gait;
mod thread;

pub use apply::apply;

use super::actions::{Align, GaitMode};
use super::link::{GaitRun, LegClock, LinkHealth, POLL_ACTIVE};
use super::settings::{MirrorBinding, MirrorSettings, sign};
use super::view::fixed;
use crate::robot::preset::{PresetRun, RecordedRun};
use bevy::math::DQuat;
use serde_json::{Value, json};
use sim_runtime::hardware::protocol::calibration::{Axis, GaitBinding, Status};
use sim_runtime::session::Scene;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Weak};
use std::time::Instant;
use thread::worker;

/// Encoder counts per revolution (calibration-mirror.mjs:4).
pub const COUNTS: f64 = 4096.0;
/// Suspension lift (m): the page's `LIFT_M`, shared with the gait preview.
pub const LIFT_M: f64 = crate::robot::gait::LIFT_M;
/// CAD motor joints a motor can be bound to, with the panel's labels (:5).
pub const JOINTS: [(&str, &str); 3] = [("Hip servo output", "Hip swing (belt)"), ("Worm servo output", "Worm drive"), ("Foot servo output", "Foot slide")];
/// The simulated legs (:25).
pub const LEGS: [&str; 4] = ["+X", "-X", "+Y", "-Y"];
/// The section's explanatory paragraph (:28).
pub const NOTE: &str = "Align each motor once: move the real leg until it matches the simulated leg's alignment pose (CAD home, or mid-travel where the real part cannot reach home), then press Save sim alignment. The pose's joint angle is saved with the alignment. If the simulated part turns the wrong way, flip its sign. The mirrored leg is tinted blue. Geometry only: no simulated forces, contact or motor model.";
/// Why Run, Step and Reset are refused while the mirror is shown ([`refuse_run`],
/// called by `check` and `check_planar` in robot/actions/mod.rs).
pub const MIRRORING: &str = "the leg mirror is showing the real leg on the robot; turn the mirror off (Leg calibration › Simulated leg mirror) to run the simulation";

/// The refusal of a run action while the mirror is shown, naming it
/// (`Run refused: …`): Run, Step and Reset would advance or rebuild the run
/// whose frame the mirror replaces. Pause is allowed (it only stops a run).
/// None: the action is allowed. Robot mode's one `check` (and its planar
/// twin) calls this, so a click, `system_ui` and REST `robot_run` share it.
pub fn refuse_run(action: crate::robot::run::RunAction, mirroring: bool) -> Option<String> {
    use crate::robot::run::RunAction;
    match action {
        RunAction::Start | RunAction::Step | RunAction::Reset if mirroring => Some(format!("{} refused: {MIRRORING}", action.label())),
        _ => None,
    }
}
/// `--robot FILE` has no scene for the mirror to pose.
pub const NO_SCENE: &str = "open a robot preset (the mirror poses a scene's robot)";
const RECORD_NOTE: &str = "Display-only encoder to CAD-joint binding; not promoted to CAD.";
/// The page's worker error (calibration-mirror.mjs:43), shown as "Mirror unavailable: …".
pub const WORKER_FAILED: &str = "Mirror worker failed";

/// The page's `DEFAULT_JOINT` (:7): role name → CAD joint (else the hip).
pub fn default_joint(role: &str) -> &'static str {
    match role {
        "knee" => "Foot servo output",
        "worm" => "Worm servo output",
        _ => "Hip servo output",
    }
}
/// `defaultAlign` (:13): the printed knee cannot reach CAD home, so it aligns mid-travel.
pub fn default_align(joint: &str) -> Align {
    if joint == "Foot servo output" { Align::Mid } else { Align::Home }
}
/// `ALIGN` (:12).
pub fn align_label(align: Align) -> &'static str {
    match align {
        Align::Home => "CAD home",
        Align::Mid => "Mid-travel",
    }
}

/// One motor coordinate of the scene (a `MirrorCoordinate`, owned).
#[derive(Clone, Debug, PartialEq)]
pub struct Coordinate {
    pub joint: String,
    pub home: f64,
    pub lower: Option<f64>,
    pub upper: Option<f64>,
}

/// Where the mirror's scene comes from (a preset's, or a recorded preset's).
#[derive(Clone)]
pub enum SceneSource {
    Preset(Arc<PresetRun>),
    Recorded(Arc<RecordedRun>),
}
impl SceneSource {
    fn scene(&self) -> &Scene {
        match self {
            SceneSource::Preset(p) => &p.scene,
            SceneSource::Recorded(r) => &r.scene,
        }
    }
    /// Identity of the loaded run (a preset switch or reload is a new one).
    pub fn id(&self) -> SceneId {
        match self {
            SceneSource::Preset(p) => SceneId::Preset(Arc::downgrade(p)),
            SceneSource::Recorded(r) => SceneId::Recorded(Arc::downgrade(r)),
        }
    }
}

/// A loaded run's identity: equal only for the same allocation (`Weak::ptr_eq`).
/// The `Weak` holds the allocation (not the scene), so no later run can reuse the address.
#[derive(Clone)]
pub enum SceneId {
    Preset(Weak<PresetRun>),
    Recorded(Weak<RecordedRun>),
}
impl PartialEq for SceneId {
    fn eq(&self, other: &Self) -> bool {
        matches!((self, other), (SceneId::Preset(a), SceneId::Preset(b)) if Weak::ptr_eq(a, b)) || matches!((self, other), (SceneId::Recorded(a), SceneId::Recorded(b)) if Weak::ptr_eq(a, b))
    }
}

/// The loaded run's scene (Ok(None) while Robot mode loads; Err for
/// `--robot FILE`, which has no scene).
pub fn scene_of(view: &crate::robot::RobotView) -> Result<Option<SceneSource>, String> {
    // A planar v2 file has no run here (it runs on robot_planar's thread) and no scene to pose.
    if view.is_planar() {
        return Err(crate::robot::planar::NO_MIRROR.into());
    }
    let Some(run) = view.run.as_ref() else { return Ok(None) };
    if let Some(p) = run.preset() {
        return Ok(Some(SceneSource::Preset(p.clone())));
    }
    if let Some(r) = run.recorded() {
        return Ok(Some(SceneSource::Recorded(r.clone())));
    }
    Err(NO_SCENE.into())
}
/// The loaded model's link names, by index (the order `MirrorDisplay::poses` uses).
pub fn link_names(view: &crate::robot::RobotView) -> Vec<String> {
    view.model.as_ref().map(|m| m.links.iter().map(|l| l.name.clone()).collect()).unwrap_or_default()
}

/// A solved pose, mapped to the loaded links by name.
#[derive(Clone, Debug)]
pub struct Solved {
    pub poses: Vec<Option<([f64; 3], DQuat)>>,
}

pub enum MirrorCommand {
    Load { scene: SceneSource, links: Vec<String> },
    Pose { seq: u64, values: Vec<f64> },
    Gait { number: u64, compiled: Arc<Value>, name: String },
    Sample { seq: u64, t: f64, dt: f64, scale: f64, reset: bool },
}

/// What the worker hands back; the UI takes each result once.
#[derive(Default)]
pub struct MirrorShared {
    coordinates: Option<Result<Vec<Coordinate>, String>>,
    pose: Option<(u64, Result<(Solved, Vec<String>), String>)>,
    gait: Option<(u64, Result<(), String>)>,
    sample: Option<(u64, Result<Vec<(String, f64)>, String>)>,
}

/// The page's `LegMirror` state.
pub struct Mirror {
    /// The preferences as read (bindings of motors not yet listed are kept until roles are known).
    saved: MirrorSettings,
    /// The page's `this.settings`: bindings for the listed motors only.
    pub settings: MirrorSettings,
    /// Motor ID → role, from `state.calibration.axes` (None until the server lists them).
    roles: Option<BTreeMap<u8, String>>,
    worker: Option<crate::jobs::RunThread<MirrorCommand, MirrorShared>>,
    scene_id: Option<SceneId>,
    coordinates: Option<Vec<Coordinate>>,
    loading: bool,
    error: Option<String>,
    /// The status line's text (without the error override and the leg's data note).
    line: String,
    /// `line` describes encoder readings (not a loading or simulated-gait line).
    line_is_reading: bool,
    /// The text the next solve's status is built from (`this.text`).
    text: String,
    /// `text` describes encoder readings.
    text_is_reading: bool,
    /// The leg's data as of this frame (`mirror_sync`, [`Mirror::set_leg_health`]):
    /// the encoders pose the leg only while it is `Live`.
    leg_health: LinkHealth,
    /// The values last requested (`this.pending`).
    pending: Option<Vec<f64>>,
    pose_sent: u64,
    pose_done: u64,
    /// The robot shows the mirror (`mirrorActive`).
    shown: bool,
    /// Run the begin procedure again (the page's `begin()` was called).
    restart: bool,
    /// Set by `end()`: the panel clears `RobotView::mirror`.
    ended: bool,
    /// The last server state given to `update` (`this.last`).
    last: Option<Status>,
    /// Gait pose by joint (`this.gait`) and whether the bound leg stays on the encoders.
    gait: Option<BTreeMap<String, f64>>,
    gait_real_leg: bool,
    /// The compiled gait number loaded (or loading) on the worker, and whether it loaded.
    gait_number: Option<u64>,
    gait_ready: bool,
    sampled: bool,
    sample_sent: u64,
    sample_done: u64,
    sample_both: bool,
    last_sample: Option<Instant>,
    /// The leg clock time (s) last sampled in this play, and the scale it was
    /// interpolated at (Both only; see [`Mirror::follow_gait`]).
    last_t: Option<(f64, f64)>,
    /// A gait load or sample error (the page shows it in the Gait playback status).
    gait_notice: Option<String>,
    /// Bumped when roles or settings change (the panel rebuilds its rows).
    pub revision: u64,
}

impl Mirror {
    pub fn new(saved: &MirrorSettings) -> Self {
        Self {
            saved: saved.clone(),
            settings: MirrorSettings { enabled: saved.enabled, leg: saved.leg.clone(), bindings: BTreeMap::new() },
            roles: None,
            worker: None,
            scene_id: None,
            coordinates: None,
            loading: false,
            error: None,
            line: String::new(),
            line_is_reading: false,
            text: String::new(),
            text_is_reading: false,
            leg_health: LinkHealth::Waiting,
            pending: None,
            pose_sent: 0,
            pose_done: 0,
            shown: false,
            restart: false,
            ended: false,
            last: None,
            gait: None,
            gait_real_leg: true,
            gait_number: None,
            gait_ready: false,
            sampled: false,
            sample_sent: 0,
            sample_done: 0,
            sample_both: false,
            last_sample: None,
            last_t: None,
            gait_notice: None,
            revision: 1,
        }
    }

    /// The motors' roles, once (the page constructs its mirror from the
    /// first status that lists axes, calibration-ui.mjs:121; :19-22).
    pub fn set_roles(&mut self, roles: BTreeMap<u8, String>) {
        if self.roles.is_some() {
            return;
        }
        for (id, role) in &roles {
            let saved = self.saved.bindings.get(id);
            let joint = saved.map_or_else(|| default_joint(role).to_string(), |b| b.joint.clone());
            let binding = MirrorBinding { polarity: saved.map_or(1, |b| sign(b.polarity)), align: saved.map_or_else(|| default_align(&joint), |b| b.align), joint };
            self.settings.bindings.insert(*id, binding);
        }
        self.roles = Some(roles);
        self.revision += 1;
    }
    pub fn roles(&self) -> Option<&BTreeMap<u8, String>> {
        self.roles.as_ref()
    }
    /// Publish remembered display choices without commanding hardware or
    /// creating a worker. Existing roles are reapplied to the saved bindings.
    pub(super) fn load_preferences(&mut self, saved: &MirrorSettings) {
        self.saved = saved.clone();
        self.settings = MirrorSettings { enabled: saved.enabled, leg: saved.leg.clone(), bindings: BTreeMap::new() };
        if let Some(roles) = self.roles.take() {
            self.set_roles(roles);
        }
        self.restart = true;
        if !saved.enabled { self.end(); }
        self.revision += 1;
    }

    /// The preferences to persist (before roles are known, the saved bindings are kept).
    pub fn to_save(&self) -> MirrorSettings {
        let mut s = self.settings.clone();
        if self.roles.is_none() {
            s.bindings = self.saved.bindings.clone();
        }
        s
    }
    pub fn shown(&self) -> bool {
        self.shown
    }
    pub fn busy(&self) -> bool {
        self.pose_sent > self.pose_done
    }
    /// Work in flight or a gait playing: the panel keeps frames coming until it lands.
    pub fn working(&self) -> bool {
        self.busy() || self.loading || self.sample_sent > self.sample_done || self.gait_number.is_some()
    }
    pub fn gait_notice(&self) -> Option<&str> {
        self.gait_notice.as_deref()
    }

    /// `this.joint(id)`: `"{leg} | {joint}"`.
    pub fn joint(&self, id: u8) -> Option<String> {
        self.settings.bindings.get(&id).map(|b| format!("{} | {}", self.settings.leg, b.joint))
    }
    fn coordinate(&self, id: u8) -> Option<(usize, &Coordinate)> {
        let joint = self.joint(id)?;
        self.coordinates.as_ref()?.iter().enumerate().find(|(_, c)| c.joint == joint)
    }
    /// The joint angle to align motor `id` at now (sent with Save sim
    /// alignment, :53-56); None until the robot model is loaded.
    pub fn alignment_angle(&self, id: u8) -> Option<f64> {
        let (_, c) = self.coordinate(id)?;
        let mid = self.settings.bindings.get(&id).is_some_and(|b| b.align == Align::Mid);
        Some(match (mid, c.lower, c.upper) {
            (true, Some(lo), Some(hi)) => (lo + hi) / 2.0,
            _ => c.home,
        })
    }
    /// `savedAngle` (:58): the joint angle an alignment was captured at (older saves: CAD home).
    pub fn saved_angle(axis: &Axis, c: &Coordinate) -> f64 {
        axis.reference_joint_rad.unwrap_or(c.home)
    }

    /// The status line (`status()`, :47): the error overrides the text.
    /// While the shown leg's data is not live the line says so first, with
    /// the last reading after it ("Leg data stale — last read 3 s ago; not
    /// live · last reading: hip: 2.1° from its alignment pose").
    pub fn status_text(&self) -> String {
        if let Some(e) = &self.error {
            return format!("Mirror unavailable: {e}");
        }
        match self.leg_note() {
            None => self.line.clone(),
            Some(note) if self.line.is_empty() => note,
            Some(note) if self.line_is_reading => format!("{note} · last reading: {}", self.line),
            Some(note) => format!("{note} · {}", self.line),
        }
    }

    /// The leg's health for this frame (`mirror_sync` sets it every frame:
    /// the link's [`super::link::LinkSnapshot::health`], `Waiting` with no
    /// link). Returns true when the data has just become live again: the
    /// caller then forces an [`Mirror::update`] with the current status so
    /// the live pose and line return. Compared before it is written.
    pub(crate) fn set_leg_health(&mut self, health: &LinkHealth) -> bool {
        let recovered = *health == LinkHealth::Live && self.leg_health != LinkHealth::Live;
        if self.leg_health != *health {
            self.leg_health = health.clone();
        }
        recovered
    }
    fn live(&self) -> bool {
        self.leg_health == LinkHealth::Live
    }
    /// Why the shown leg is not live (None when live, or when a simulated
    /// gait poses every leg and no encoder is shown).
    fn leg_note(&self) -> Option<String> {
        if self.gait.is_some() && !self.gait_real_leg {
            return None;
        }
        self.leg_health.leg_note()
    }

    /// `record()` (:49): the display binding, for exports.
    pub fn record(&self) -> Value {
        let s = self.to_save();
        json!({"enabled": s.enabled, "leg": s.leg, "bindings": s.bindings, "lift_m": LIFT_M, "counts_per_revolution": COUNTS as u32, "note": RECORD_NOTE})
    }

    /// For `hardware_status`.
    pub fn state_json(&self) -> Value {
        json!({"enabled": self.settings.enabled, "leg": self.settings.leg, "shown": self.shown, "status": self.status_text(), "coordinates": self.coordinates.as_ref().map(Vec::len),
            "busy": self.busy(), "gait": self.gait.is_some(), "gait_real_leg": self.gait_real_leg, "gait_notice": self.gait_notice, "record": self.record(),
            "leg_data": self.leg_health.name(), "leg_note": self.leg_health.leg_note()})
    }

    /// The page's `begin()`: run the begin procedure again at the next frame.
    pub fn begin(&mut self) {
        self.restart = true;
    }
    /// `robotViewer.end()`: stop showing (the panel clears `RobotView::mirror`).
    pub fn end(&mut self) {
        if self.shown {
            self.ended = true;
        }
        self.shown = false;
    }
    /// Whether `end()` asked for the display to be cleared (once).
    pub(crate) fn take_ended(&mut self) -> bool {
        std::mem::take(&mut self.ended)
    }
    /// The begin procedure is due: begun but not shown, or restarted.
    pub(crate) fn due(&self) -> bool {
        self.restart || (!self.shown && self.error.is_none())
    }

    /// The body of the page's `begin()` (:60-74) once the panel is open and
    /// the mirror enabled: `scene` is the loaded run's scene (Ok(None) while it
    /// loads; Err for `--robot FILE`). Returns the links to tint when it shows.
    pub(crate) fn prepare(&mut self, scene: Result<Option<SceneSource>, String>, links: &[String]) -> Option<BTreeSet<usize>> {
        let restart = std::mem::take(&mut self.restart);
        let source = match scene {
            Err(e) => {
                self.error = Some(e);
                self.shown = false;
                return None;
            }
            Ok(None) => {
                self.line = "Waiting for the robot model to load…".into();
                self.line_is_reading = false;
                return None;
            }
            Ok(Some(s)) => s,
        };
        let id = source.id();
        if self.scene_id.as_ref() != Some(&id) || self.worker.as_ref().is_none_or(|w| w.finished()) {
            // A new scene, or no live worker: a new worker. The old one may be mid-solve, so it is
            // released off the UI thread (RunThread's drop waits up to JOIN_BOUND).
            self.release_worker();
            self.scene_id = Some(id);
            self.shown = false;
            self.coordinates = None;
            self.loading = false;
            self.gait_number = None;
            self.gait_ready = false;
            self.last_t = None;
            self.pending = None;
            self.pose_done = self.pose_sent;
            self.sample_done = self.sample_sent;
            self.worker = Some(crate::jobs::RunThread::spawn("hardware-mirror", MirrorShared::default(), worker));
        }
        if self.coordinates.is_none() {
            if !self.loading || restart {
                self.line = "Preparing the suspended robot…".into();
                self.line_is_reading = false;
                self.loading = true;
                self.error = None;
                self.send(MirrorCommand::Load { scene: source, links: links.to_vec() });
            }
            return None;
        }
        // Each motor on its own CAD joint, and every joint in the model (:65-67).
        let joints: BTreeSet<String> = self.settings.bindings.keys().filter_map(|id| self.joint(*id)).collect();
        if joints.len() != self.settings.bindings.len() {
            self.error = Some("Bind each motor to a different CAD joint".into());
            return None;
        }
        let coordinates = self.coordinates.as_deref().unwrap_or_default();
        if let Some(j) = joints.iter().find(|j| !coordinates.iter().any(|c| &c.joint == *j)) {
            self.error = Some(format!("CAD model has no motor joint {j}"));
            return None;
        }
        self.error = None;
        if !self.settings.enabled {
            return None;
        }
        let prefix = format!("{} |", self.settings.leg);
        let tinted = links.iter().enumerate().filter(|(_, n)| n.starts_with(&prefix)).map(|(i, _)| i).collect();
        self.shown = true;
        let last = self.last.take().unwrap_or_default();
        self.update(&last, true);
        Some(tinted)
    }

    fn send(&mut self, command: MirrorCommand) -> bool {
        match self.worker.as_ref().map(|w| w.send(command)) {
            Some(Ok(())) => true,
            _ => {
                self.worker_failed();
                false
            }
        }
    }

    /// The worker is gone (a panic: the UI holds its channel open): settle what was in
    /// flight, end the display, show the page's worker error; the next begin respawns it.
    fn worker_failed(&mut self) {
        self.release_worker();
        self.loading = false;
        self.coordinates = None;
        self.pending = None;
        (self.pose_done, self.sample_done, self.gait_number, self.gait_ready, self.last_t) = (self.pose_sent, self.sample_sent, None, false, None);
        self.error = Some(WORKER_FAILED.into());
        self.end();
    }

    /// Drops the worker (if any) off the UI thread.
    fn release_worker(&mut self) {
        if let Some(old) = self.worker.take() {
            crate::jobs::drop_off_thread(old, "hardware-mirror");
        }
    }

    /// `update(state, force)` (:76-100): called on every new server state.
    ///
    /// While the leg's data is not live ([`Mirror::set_leg_health`]) the
    /// encoders pose nothing: the last solved pose and its text are held
    /// (also when `prepare` or a gait sample calls this with the cached
    /// state), and [`Mirror::status_text`] says why. Only a simulated gait
    /// that poses every leg (Sim, no encoder shown) is still solved.
    pub fn update(&mut self, state: &Status, force: bool) {
        self.last = Some(state.clone());
        if !self.settings.enabled || self.error.is_some() {
            return;
        }
        let Some(coordinates) = self.coordinates.as_ref() else { return };
        // A playing gait poses every motor joint; the bound leg shows the real
        // encoders instead when the real leg is part of the session.
        let mut values: Vec<f64> = coordinates.iter().map(|c| self.gait.as_ref().and_then(|g| g.get(&c.joint)).copied().unwrap_or(c.home)).collect();
        if self.gait.is_some() && !self.gait_real_leg {
            self.text = "Simulated gait".into();
            self.text_is_reading = false;
            if force || self.pending.as_ref() != Some(&values) {
                self.solve(values);
            }
            return;
        }
        if !self.live() {
            return;
        }
        let none = Axis::default();
        let mut lines = Vec::new();
        for (id, role) in self.roles.iter().flatten() {
            let axis = state.calibration.as_ref().and_then(|c| c.axes.get(id)).unwrap_or(&none);
            let raw = state.samples.get(id).map(|t| t.position());
            let (Some((i, c)), Some(binding)) = (self.coordinate(*id), self.settings.bindings.get(id)) else { continue };
            let pose = align_label(binding.align).to_lowercase();
            let Some(reference) = axis.reference else {
                values[i] = self.alignment_angle(*id).unwrap_or(c.home);
                lines.push(format!("{role}: not aligned — shown at {pose}"));
                continue;
            };
            // A multi-turn alignment only holds in the encoder tracking session it was saved in.
            let multi_turn = reference < 0 || reference > COUNTS as i64 - 1;
            if multi_turn && axis.reference_session != state.coordinate_session {
                values[i] = self.alignment_angle(*id).unwrap_or(c.home);
                lines.push(format!("{role}: alignment is from an earlier session — re-align (shown at {pose})"));
                continue;
            }
            let Some(raw) = raw else {
                lines.push(format!("{role}: no reading"));
                continue;
            };
            let delta = sign(binding.polarity) as f64 * (raw - reference as f64) * std::f64::consts::TAU / COUNTS;
            values[i] = Self::saved_angle(axis, c) + delta;
            lines.push(format!("{role}: {}° from its alignment pose", degrees_text(delta)));
        }
        // Unchanged values with a changed line still re-solve (the line is set
        // from the solve, with its limit check): a first alignment saved
        // exactly at the pose the motor was shown at changes no value, only
        // "not aligned" to "0.0° from its alignment pose". (The page skips
        // on equal values alone and keeps the old line.)
        let text = lines.join(" · ");
        if !force && self.pending.as_ref() == Some(&values) && self.text == text {
            return;
        }
        self.text = text;
        self.text_is_reading = true;
        self.solve(values);
    }

    /// `solve(values)`: one pose request; the worker solves only the newest.
    fn solve(&mut self, values: Vec<f64>) {
        self.pending = Some(values.clone());
        if !self.shown {
            // Nothing shows it (the page's `show()` ignores it); begin forces a solve.
            return;
        }
        self.pose_sent += 1;
        let seq = self.pose_sent;
        self.send(MirrorCommand::Pose { seq, values });
    }

    /// Takes the worker's results; returns a solved pose to show.
    pub(crate) fn poll(&mut self) -> Option<Solved> {
        if self.worker.as_ref().is_some_and(|w| w.finished()) {
            self.worker_failed();
            return None;
        }
        let (coordinates, pose, gait, sample) = {
            let worker = self.worker.as_ref()?;
            let mut s = worker.lock();
            (s.coordinates.take(), s.pose.take(), s.gait.take(), s.sample.take())
        };
        if let Some(result) = coordinates {
            self.loading = false;
            match result {
                Ok(c) => {
                    self.coordinates = Some(c);
                    // Show it now (the page's begin continues after the load).
                    self.restart = true;
                }
                Err(e) => self.error = Some(e),
            }
        }
        if let Some((number, result)) = gait {
            if Some(number) == self.gait_number {
                match result {
                    Ok(()) => self.gait_ready = true,
                    Err(e) => self.gait_notice = Some(e),
                }
            }
        }
        if let Some((seq, result)) = sample {
            self.sample_done = self.sample_done.max(seq);
            match result {
                // Only while the run still plays (`if (gaitRun)`).
                Ok(pose) if self.gait_number.is_some() => self.set_gait(Some(pose.into_iter().collect()), self.sample_both),
                Ok(_) => {}
                Err(e) => self.gait_notice = Some(format!("Gait sample failed: {e}")),
            }
        }
        let (seq, result) = pose?;
        self.pose_done = self.pose_done.max(seq);
        match result {
            Ok((solved, violations)) => {
                // The mirrored leg's joints only ("{leg} | {joint}", as the tint).
                let prefix = format!("{} |", self.settings.leg);
                let limits: Vec<String> = violations.into_iter().filter(|n| n.starts_with(&prefix)).collect();
                self.line = if limits.is_empty() { self.text.clone() } else { format!("{} · Beyond CAD limit: {}", self.text, limits.join(", ")) };
                self.line_is_reading = self.text_is_reading;
                self.shown.then_some(solved)
            }
            Err(e) => {
                self.line = format!("{} · Pose not solved: {e}", self.text);
                self.line_is_reading = self.text_is_reading;
                None
            }
        }
    }

    /// `gaitBindings(state)` (:117-126): motor → CAD joint bindings for
    /// driving the real leg with a gait (aligned, taught, enabled motors),
    /// and the skipped motors with why.
    pub fn gait_bindings(&self, state: &Status) -> (Vec<GaitBinding>, Vec<String>) {
        let (mut out, mut skipped) = (Vec::new(), Vec::new());
        let none = Axis::default();
        for (id, role) in self.roles.iter().flatten() {
            let axis = state.calibration.as_ref().and_then(|c| c.axes.get(id)).unwrap_or(&none);
            let (joint, c) = (self.joint(*id), self.coordinate(*id).map(|(_, c)| c));
            let why = if axis.disabled {
                Some("disabled")
            } else if axis.lower.is_none() || axis.upper.is_none() {
                Some("poses not taught")
            } else if axis.reference.is_none() {
                Some("not aligned to the sim")
            } else if c.is_none() {
                Some("no CAD joint")
            } else {
                None
            };
            if let Some(why) = why {
                skipped.push(format!("{role} ({why})"));
                continue;
            }
            let (Some(joint), Some(c), Some(b)) = (joint, c, self.settings.bindings.get(id)) else { continue };
            out.push(GaitBinding { id: *id, joint, polarity: sign(b.polarity) as f64, home_rad: Self::saved_angle(axis, c) });
        }
        (out, skipped)
    }
}

impl Drop for Mirror {
    /// Leaving Robot mode drops the mirror with its worker, which may be mid-solve.
    fn drop(&mut self) {
        self.release_worker();
    }
}

/// The mirror line's angle (:94): `(delta * 180 / Math.PI).toFixed(1)`, in the
/// page's operation order and with `toFixed`'s ties away from zero
/// (128 counts = 11.25° shows "11.3").
pub fn degrees_text(delta_rad: f64) -> String {
    fixed(delta_rad * 180.0 / std::f64::consts::PI, 1)
}

#[cfg(test)]
mod tests;
