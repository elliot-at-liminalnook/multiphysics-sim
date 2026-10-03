//! A robot system's live run: a system file whose root hosts a robot
//! (`SystemDocument::links`: a `robot.articulated` linked to its
//! `.simrobot.json`, a `control.external` linked to its controller binding,
//! optionally a `control.drive_limiter` linked to the drive profile) runs on
//! the shared drive host (`sim_runtime::drive_host::DriveHost`: the shared
//! `Session`, so `PhysicalRobot::advance` with its retry and battery
//! sampling, the same code Robot mode runs), not on `SystemSession`, whose
//! `Runtime::advance` would be a second stepping path for the same robot.
//! Every other system keeps the system session (`live_run`).
//!
//! The "builder-run" `RunThread` resolves the system (`system_robot::resolve`:
//! the file reads) and builds the host (the controller program starts) on
//! the run thread, then keeps the generic run's Start/Pause/Step/Reset
//! semantics and wall-clock pacing, one `DriveHost::step` per seam period.
//! Twist requests come from the one apply, [`Builder::drive_request`]
//! (through [`Builder::drive`] for `BuildAction::Drive` and REST
//! `system_drive`; directly for the bound keys and gamepad, whose one
//! poller, `crate::drive_input`, Build mode drains in
//! `actions::drive_devices`), as `RunControl::Twist`;
//! the run thread applies every queued command in order before the next
//! period. The run thread is the one writer of `RunShared::drive`.
use super::*;
use sim_domain_control::drive::kinematics::BodyTwist;
use sim_runtime::drive_host::{DriveHost, DriveRequest, DriveStatus, twist_json};
use sim_runtime::system_robot::{self, RobotSystem};
use crate::drive_input::{DriveTarget, LiveTarget};

pub(super) const RUNNING_STATUS: &str = "Running the robot system on the shared drive host (its linked robot, controller binding and drive profile; background thread, paced to real time at most).";
/// Why a robot system's run is not kept as a run record.
pub(super) const NO_RUN_RECORD: &str = "a robot system's run is not kept as a run record yet: its drive session (the scene with the controller identity, the seed and one twist per seam period) is not written from Build mode; drive the robot in Robot mode (robot_save_recording) to record it";
/// The Build-mode drive rule (Robot mode's `run::DRIVE_RULE`, for a system run).
pub(super) const DRIVE_RULE: &str = "requests are normalized axes, a profile action or stop, interpreted against the linked drive profile (DriveRequest::interpret: kinematics::scale) by the one apply (Builder::drive_request, for the run-panel drive buttons, system_ui, REST system_drive and the bound keys and gamepad); once per seam period the run thread sends the limited twist (kinematics::step: acceleration limit, deadman on simulated time) and the heartbeat on the controller's four command channels, and the controller mixes it. A nonzero request moves the robot only while the run runs (the last Run, not Pause or Reset); a stop or halt is accepted until the run fails or ends (also while a reset is in progress: it waits in the channel behind the Reset), and one made while the system is still loading waits in the run thread's channel and is applied, in order, right after it loads. The run thread applies every queued request in order before each period. Pause (every pause path: the run panel's Pause and R key, system_ui, REST system_run pause, leaving Build, opening a lesson) invalidates a request live at Pause on the run thread (DriveHost::pause, sim_runtime::drive_host::PAUSE_RULE: the request becomes zero and the deadman counts as expired, the commanded twist is kept), so after Run the profile's on-loss rule applies until a fresh request arrives; the viewer also disarms held keys and the gamepad at an accepted stop, halt, Pause or Reset (drive_input::DISARM_RULE), so an input still held must be released and pressed again";
/// The status after stopping a robot run whose controller was still starting.
pub(super) const STOPPED_WHILE_STARTING: &str = "The robot system's run was stopped while its controller was still starting; the starting controller is still shutting down (its run thread closes it as soon as the start returns, within the controller reply timeout).";
/// Why a system run is not a robot system.
const NOT_ROBOT: &str = "system_drive drives a robot system: a root that hosts a robot.articulated, its control.external and optionally a control.drive_limiter, each linked to its file (system command link_file); this run is the system session";
/// Why a robot system has no seed source of its own: the document's run settings give it.
const SEED_RULE: &str = "the session seed is the system's run seed (system_builder::config_for(document).seed), as for every system run";

/// What a robot system's run thread publishes besides the live snapshot.
#[derive(Clone)]
pub(super) struct RobotDrive {
    /// The resolved system (None while loading, or when resolving failed).
    pub system: Option<Arc<RobotSystem>>,
    /// The drive host's status after the last period or request (None before the host is built).
    pub status: Option<DriveStatus>,
    /// The last request the run thread could not apply, verbatim.
    pub twist_error: Option<String>,
    /// loading | running | paused | failed | ended.
    pub phase: &'static str,
    pub seed: u64,
    /// The run's start number ([`ROBOT_RUNS`]): a new run of unchanged
    /// content (Stop and Run, the file reopened) is still a new drive target.
    pub run: u64,
}

/// Robot system runs started in this process: each [`LiveRun::spawn_robot`]
/// takes the next number, which its run thread publishes as `RobotDrive::run`
/// and [`Builder::drive_target`] puts into the target's run identity. The
/// content hash, revision and per-thread generation alone repeat when a run
/// of the same file is replaced by a new one.
static ROBOT_RUNS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

impl LiveRun {
    /// A robot system's run: `robot_thread` on the "builder-run" `RunThread`.
    pub(super) fn spawn_robot(document: SystemDocument, path: PathBuf, registry: BehaviorRegistry, description_id: String) -> Self {
        let initial = RunShared { snapshot: None, running: true, speed: 1.0, history: BTreeMap::new(), reset_pending: false, drive: None };
        let (simulated, source_id) = (document.clone(), description_id.clone());
        let run = ROBOT_RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        // Join bound zero: dropping the run never waits on the thread, which may be inside
        // `DriveHost::new` (starting python3, up to EXTERNAL_REPLY_TIMEOUT) or a period; it
        // exits at its next command check and its drop closes the controller process.
        let worker = crate::jobs::RunThread::spawn("builder-run", initial, move |commands, shared| robot_thread(simulated, path, registry, source_id, run, commands, shared)).join_bound(std::time::Duration::ZERO);
        Self { worker, description_id, fidelity: Fidelity::Detailed, document, edited: false, robot: true, requested_running: true, drive_requested: None, drive_refusal: None }
    }
}

impl Builder {
    /// The one drive apply, answering the drive block (`BuildAction::Drive`
    /// from the run-panel buttons and `system_ui`, REST `system_drive`):
    /// [`Builder::drive_request`], then `system_state.live_run.drive`.
    pub fn drive(&mut self, request: DriveRequest) -> Result<serde_json::Value, String> {
        self.drive_request(request)?;
        Ok(self.drive_json())
    }

    /// The one drive apply without the answer (the bound keys and gamepad
    /// in Build mode, `actions::drive_devices`, call it every frame while
    /// they drive): interprets `request` against the loaded system's profile
    /// (`DriveRequest::interpret`), checks the run rule ([`DRIVE_RULE`]) and
    /// sends `RunControl::Twist`. A refusal is kept for
    /// `system_state.live_run.drive.last_refusal` (and `DriveInput::last_error`,
    /// which mirrors it); an accepted request clears it. The panel is marked
    /// for a rebuild only when the refusal text changes, so held keys and
    /// sticks rebuild nothing (the 4 Hz live-run refresh shows the twist).
    pub fn drive_request(&mut self, request: DriveRequest) -> Result<(), String> {
        let checked = self.check_drive(&request);
        let run = self.run.as_mut().ok_or("nothing is running: start the robot system's run first (the Run button, or REST system_run start)")?;
        let refuse = |run: &mut LiveRun, panel_dirty: &mut bool, e: String| -> Result<(), String> {
            if run.drive_refusal.as_ref() != Some(&e) {
                run.drive_refusal = Some(e.clone());
                *panel_dirty = true;
            }
            Err(e)
        };
        match checked {
            Err(e) => refuse(run, &mut self.panel_dirty, e),
            Ok((request, halt)) => {
                if run.worker.send(RunControl::Twist { request, halt }).is_err() {
                    return refuse(run, &mut self.panel_dirty, "the run has ended; start a new run".to_string());
                }
                // Only a refusal's text changing rebuilds the panel at once; the
                // requested twist (an analog stick changes it every frame) reaches
                // the drive strip with the 4 Hz live-run refresh (`sync_run`).
                if run.drive_refusal.is_some() {
                    self.panel_dirty = true;
                }
                run.drive_requested = Some((request, halt));
                run.drive_refusal = None;
                Ok(())
            }
        }
    }

    /// What Build mode offers the drive device poller (`crate::drive_input`):
    /// a robot system's run whose profile has loaded (the linked binding's
    /// supported axes), with Build's own editing keys
    /// (`actions::OWNED_KEYS`) never read for driving; nothing otherwise,
    /// and nothing while a text draft is open (`input`) or a placement drag
    /// runs (`drag`: its X/Y/Z axis constraints would also be drive keys,
    /// X the default stop). A target that goes away while the devices drive
    /// gets one stop from the poller (`drive_input::input::devices`).
    /// The run identity is the system file, the run thread's run id
    /// (content hash and revision), the run's start number
    /// (`RobotDrive::run`) and the run thread's generation (bumped by each
    /// build, so a Reset): a run rebuilt from an edited file, a new run of
    /// unchanged content and a Reset are each a new target, so the poller
    /// disarms inputs held across them.
    pub(super) fn drive_target(&self) -> DriveTarget {
        if self.input.is_some() || self.drag.is_some() {
            return DriveTarget::default();
        }
        let Some(run) = self.run.as_ref().filter(|r| r.robot) else { return DriveTarget::default() };
        let Ok(s) = run.worker.shared().lock() else { return DriveTarget::default() };
        let Some((drive, system)) = s.drive.as_ref().and_then(|d| Some((d, d.system.as_ref()?))) else { return DriveTarget::default() };
        let status = s.snapshot.as_ref().and_then(|x| x.status.as_ref());
        let (run_id, generation) = status.map_or(("", 0), |st| (st.run_id.as_str(), st.generation));
        let live = LiveTarget {
            mode: ViewerMode::Build,
            supported: system.controlled.resolved.limits.supported,
            run: format!("{} {run_id} (run {}, generation {generation})", self.path().display(), drive.run),
        };
        DriveTarget { live: Some(live), owned_keys: super::actions::OWNED_KEYS.to_vec() }
    }

    /// The twist `request` asks for now, or why it is refused.
    fn check_drive(&self, request: &DriveRequest) -> Result<(BodyTwist, bool), String> {
        let run = self.run.as_ref().ok_or("nothing is running: start the robot system's run first (the Run button, or REST system_run start)")?;
        if !run.robot {
            return Err(NOT_ROBOT.into());
        }
        let s = run.worker.shared().lock().map_err(|_| "run state unavailable")?;
        let drive = s.drive.as_ref();
        // During a reset only stop and halt pass: they wait in the channel behind the Reset
        // and reach the rebuilt host, which starts at zero anyway.
        if s.reset_pending {
            return match request {
                DriveRequest::Stop => Ok((BodyTwist::ZERO, false)),
                DriveRequest::Action { name } => match drive.and_then(|d| d.system.as_ref()) {
                    Some(system) => request.interpret(&system.controlled).and_then(|(t, halt)| if halt || t.is_zero() { Ok((t, halt)) } else { Err(format!("drive request refused: action `{name}` moves the robot and a reset is in progress")) }),
                    None => Err("drive request refused: a reset is in progress and the profile is reloading; only stop is accepted until it has loaded".into()),
                },
                DriveRequest::Axes { .. } => Err("drive request refused: a reset is in progress (the run is rebuilt paused at t = 0); a stop or halt is accepted, drive again once the system has reloaded and Run is pressed".into()),
            };
        }
        let phase = drive.map_or("loading", |d| d.phase);
        let error = s.snapshot.as_ref().and_then(|x| x.error.clone());
        match phase {
            "failed" => return Err(format!("drive request refused: the run failed ({}); Reset rebuilds it (system_run reset)", error.as_deref().unwrap_or("no error text"))),
            "ended" => return Err(format!("drive request refused: {}; Reset starts a new session", error.as_deref().unwrap_or("the run ended"))),
            _ => {}
        }
        let (twist, halt) = match drive.and_then(|d| d.system.as_ref()) {
            Some(system) => request.interpret(&system.controlled)?,
            // Before the profile is loaded only a stop has a meaning.
            None if *request == DriveRequest::Stop => (BodyTwist::ZERO, false),
            None => return Err("drive request refused: the robot system is still loading (its linked files are read and the controller started on the run thread); only stop is accepted until it has loaded".into()),
        };
        if !halt && !twist.is_zero() && !run.requested_running {
            return Err(format!("drive request refused (phase {phase}): drive requests move the robot only while it runs; press Run (or REST system_run start) first (a stop or halt is accepted while paused)"));
        }
        Ok((twist, halt))
    }

    /// `system_state.bindings` and `system_state.drive_input`, added to every
    /// answer that carries `system_state` (`system_actions::apply`) by the
    /// one serializer Robot mode's `robot_state` uses
    /// (`crate::drive_input::insert_state`): set only while a robot system's
    /// run is live (the run the devices may drive), null otherwise.
    pub(super) fn with_drive_input(&self, state: &mut serde_json::Value, bindings: Option<&crate::drive_input::DriveBindings>, input: Option<&crate::drive_input::DriveInput>) {
        let drivable = self.run.as_ref().is_some_and(|r| r.robot);
        crate::drive_input::insert_state(state, drivable, bindings, input);
    }

    /// `system_state.live_run.drive`: null for a run that is not a robot
    /// system; else the phase, the system (instances, files, limits with
    /// units, deadman, period, channels, wiring), the drive status, the last
    /// request sent and refused, and the run's error.
    pub(super) fn drive_json(&self) -> serde_json::Value {
        let Some(run) = self.run.as_ref().filter(|r| r.robot) else { return serde_json::Value::Null };
        let s = run.worker.shared().lock().ok();
        let drive = s.as_ref().and_then(|s| s.drive.clone());
        let error = s.as_ref().and_then(|s| s.snapshot.as_ref().and_then(|x| x.error.clone()));
        let phase = drive.as_ref().map_or("loading", |d| d.phase);
        let accepts_motion = run.requested_running && matches!(phase, "running" | "paused");
        serde_json::json!({
            "phase": phase,
            "system": drive.as_ref().and_then(|d| d.system.as_ref()).map(|sys| sys.json()),
            "status": drive.as_ref().and_then(|d| d.status.as_ref()).map(DriveStatus::json),
            "requested": run.drive_requested.map(|(t, halt)| serde_json::json!({"twist": twist_json(t), "halt": halt})),
            "accepts_motion": accepts_motion,
            "last_refusal": run.drive_refusal,
            "last_apply_error": drive.as_ref().and_then(|d| d.twist_error.clone()),
            "error": error,
            "seed": drive.as_ref().map(|d| d.seed),
            "seed_rule": SEED_RULE,
            "rule": DRIVE_RULE,
            "pause_rule": sim_runtime::drive_host::PAUSE_RULE,
            "host": "sim_runtime::drive_host::DriveHost (the shared Session: PhysicalRobot with the external controller attached on the model's control.external seam)",
        })
    }
}

/// Input, before `InputSet::Window` (Build mode): Build mode's one
/// [`DriveTarget`] writer ([`Builder::drive_target`], `set_if_neq`), read by
/// the one drive device poller in `InputSet::Window`.
pub(super) fn drive_target(builder: Option<Res<Builder>>, target: Option<ResMut<DriveTarget>>) {
    let Some(mut target) = target else { return };
    target.set_if_neq(builder.as_deref().map_or_else(DriveTarget::default, Builder::drive_target));
}

/// The run thread's state: the resolved system, its host, and what it publishes.
struct Worker {
    shared: Arc<Mutex<RunShared>>,
    path: PathBuf,
    registry: BehaviorRegistry,
    source_id: String,
    /// The run's start number (`RobotDrive::run`).
    run: u64,
    run_id: String,
    generation: u64,
    seed: u64,
    system: Option<Arc<RobotSystem>>,
    host: Option<DriveHost>,
    /// Building has not finished (files read, controller started). Commands
    /// wait in the channel meanwhile (the build runs on this thread), so they
    /// are applied in order after it.
    loading: bool,
    /// Why the run cannot continue until Reset: a resolve, build or step error, named.
    failed: Option<String>,
    /// The horizon was reached (`DriveHost::ended`).
    ended: Option<String>,
    /// A refused command (a Step while running, Run after a failure, an edit while running), shown until the next control command.
    note: Option<String>,
    twist_error: Option<String>,
    running: bool,
    steps: u64,
    step_wall: f64,
}

impl Worker {
    fn time(&self) -> f64 {
        self.host.as_ref().map_or(0.0, |h| h.time())
    }

    fn phase(&self) -> &'static str {
        if self.failed.is_some() {
            "failed"
        } else if self.ended.is_some() {
            "ended"
        } else if self.loading {
            "loading"
        } else if self.running {
            "running"
        } else {
            "paused"
        }
    }

    /// Resolve `document` and build its host (both on this thread). The old
    /// host is dropped first, which closes its controller process.
    fn build(&mut self, document: &SystemDocument) {
        self.host = None;
        self.system = None;
        (self.failed, self.ended, self.note, self.twist_error) = (None, None, None, None);
        self.steps = 0;
        self.generation += 1;
        self.seed = system_builder::config_for(document).seed;
        self.run_id = format!("robot-system-{}-r{}", &document.content_hash()[..12], document.revision);
        self.loading = true;
        self.publish(None);
        let resolved = sim_system::flatten(document, &self.registry)
            .map_err(|e| e.to_string())
            .and_then(|flat| system_robot::resolve(document, &self.path, &flat))
            .and_then(|s| s.ok_or_else(|| "this system hosts no robot (no linked instances); Stop and Run it again to run it on the system session".to_string()));
        match resolved {
            Ok(system) => {
                let system = Arc::new(system);
                self.system = Some(system.clone());
                match system.host(self.seed) {
                    Ok(host) => self.host = Some(host),
                    Err(e) => self.failed = Some(e),
                }
            }
            Err(e) => self.failed = Some(e),
        }
        self.loading = false;
        if self.failed.is_some() {
            self.running = false;
        }
    }

    /// A request from the one apply, under the run rule.
    fn twist(&mut self, request: BodyTwist, halt: bool) {
        if let Some(why) = self.failed.as_ref().or(self.ended.as_ref()) {
            self.twist_error = Some(format!("drive request not applied: {why}; Reset rebuilds the run"));
            return;
        }
        if !self.running && !halt && !request.is_zero() {
            self.twist_error = Some("drive request not applied: the run was not running when it arrived (Pause or Reset was sent first); press Run (or REST system_run start) first".into());
            return;
        }
        let Some(host) = self.host.as_mut() else {
            self.twist_error = Some("drive request not applied: the robot system did not load".into());
            return;
        };
        self.twist_error = host.request(request, halt).err();
    }

    /// One seam period on the drive host; a failure ends the run with a named error.
    fn period(&mut self) {
        let Some(host) = self.host.as_mut() else { return };
        let started = std::time::Instant::now();
        match host.step() {
            Ok(_) => {
                self.steps += 1;
                self.step_wall = started.elapsed().as_secs_f64();
            }
            Err(e) => {
                let named = self.system.as_ref().map_or(e.clone(), |s| s.name_error(&e));
                self.failed = Some(named);
                self.running = false;
            }
        }
        if self.failed.is_none() && host.ended() {
            self.ended = Some(format!("drive session horizon reached at t = {:.3} s ({} periods of {} s; a drive session lasts controller_binding::DRIVE_DURATION_S = {} s of simulated time)", host.time(), host.twist.periods, host.session.scene.period_s, host.session.scene.duration_s));
            self.running = false;
        }
    }

    fn publish(&self, speed: Option<f64>) {
        use sim_inspect::live::{LiveSnapshot, Phase, SessionStatus};
        let phase = match self.phase() {
            "failed" => Phase::Failed,
            "running" => Phase::Running,
            _ => Phase::Paused,
        };
        let message = match self.phase() {
            "loading" => Some("loading: reading the linked files and starting the controller".to_string()),
            "ended" => self.ended.clone(),
            _ => None,
        };
        let status = SessionStatus { phase, run_id: self.run_id.clone(), generation: self.generation, step: self.steps, time: self.time(), sequence: self.steps, step_wall_seconds: self.step_wall, events: 0, message };
        let error = self.failed.clone().or_else(|| self.ended.clone()).or_else(|| self.note.clone());
        if let Ok(mut s) = self.shared.lock() {
            s.snapshot = Some(LiveSnapshot { version: 1, source_description_id: self.source_id.clone(), description: None, status: Some(status), frame: None, error });
            s.running = self.running;
            if let Some(speed) = speed {
                s.speed = speed;
            }
            s.drive = Some(RobotDrive { system: self.system.clone(), status: self.host.as_ref().map(|h| h.status()), twist_error: self.twist_error.clone(), phase: self.phase(), seed: self.seed, run: self.run });
        }
    }
}

fn robot_thread(document: SystemDocument, path: PathBuf, registry: BehaviorRegistry, source_id: String, run: u64, commands: mpsc::Receiver<RunControl>, shared: Arc<Mutex<RunShared>>) {
    // The document the next Reset rebuilds from: every edit while running arrives as a Swap.
    let mut document = document;
    let mut w = Worker {
        shared,
        path,
        registry,
        source_id,
        run,
        run_id: String::new(),
        generation: 0,
        seed: 0,
        system: None,
        host: None,
        loading: true,
        failed: None,
        ended: None,
        note: None,
        twist_error: None,
        // Run started this thread.
        running: true,
        steps: 0,
        step_wall: 0.0,
    };
    w.build(&document);
    let mut wall = std::time::Instant::now();
    let mut sim_at_wall = w.time();
    let mut last_publish = std::time::Instant::now() - std::time::Duration::from_secs(1);
    loop {
        // Every queued command, in order, before the next period.
        let mut changed = false;
        loop {
            let command = match commands.try_recv() {
                Ok(c) => c,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return,
            };
            changed = true;
            if matches!(command, RunControl::Start | RunControl::Pause | RunControl::Step | RunControl::Reset) {
                w.note = None;
            }
            match command {
                RunControl::Start => {
                    if let Some(why) = w.failed.as_ref().or(w.ended.as_ref()) {
                        w.note = Some(format!("Run refused: {why}; Reset rebuilds the run"));
                    } else {
                        w.running = true;
                        wall = std::time::Instant::now();
                        sim_at_wall = w.time();
                    }
                }
                RunControl::Pause => {
                    // Every Build pause reaches here (the run panel's Pause and R key, system_ui,
                    // REST system_run pause, leaving Build or opening a lesson: `Builder::pause_run`).
                    // PAUSE_RULE: a request live at Pause does not drive after Run without a fresh one.
                    w.running = false;
                    if let Some(host) = w.host.as_mut() {
                        host.pause();
                    }
                }
                RunControl::Step => {
                    if w.running {
                        w.note = Some("step refused: pause the run before stepping".into());
                    } else if let Some(why) = w.failed.as_ref().or(w.ended.as_ref()) {
                        w.note = Some(format!("step refused: {why}; Reset rebuilds the run"));
                    } else {
                        w.period();
                    }
                }
                RunControl::Reset => {
                    // Paused at t = 0, rebuilt from the latest document and its linked files.
                    w.running = false;
                    w.build(&document);
                    if let Ok(mut s) = w.shared.lock() {
                        s.history.clear();
                        s.reset_pending = false;
                    }
                    wall = std::time::Instant::now();
                    sim_at_wall = w.time();
                }
                RunControl::Swap(next, description_id) => {
                    document = *next;
                    w.source_id = description_id;
                    w.note = Some("the robot run keeps the system and linked files it loaded; edits apply at Reset (system_run reset)".into());
                }
                // Nothing is observed: the robot is not in the system description.
                RunControl::Observe(_) => {}
                RunControl::Twist { request, halt } => w.twist(request, halt),
            }
        }
        if changed {
            w.publish(None);
        }
        if w.running && w.host.is_some() {
            // Never faster than real time; a slower controller or plant runs as fast as it can.
            let ahead = w.time() - sim_at_wall - wall.elapsed().as_secs_f64();
            if ahead > 0. {
                std::thread::sleep(std::time::Duration::from_secs_f64(ahead.min(0.02)));
            } else {
                w.period();
            }
        } else {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        if last_publish.elapsed().as_millis() >= 33 {
            let elapsed = wall.elapsed().as_secs_f64();
            let speed = if elapsed > 0. && w.running { (w.time() - sim_at_wall) / elapsed } else { 0. };
            w.publish(Some(speed));
            last_publish = std::time::Instant::now();
        }
    }
}

#[cfg(test)]
mod device_tests {
    //! Written fixtures (not executed by their author): the drive device
    //! poller's requests in Build mode.
    use super::super::actions::{apply_device, device_action};
    use super::*;
    use crate::app::actions::{Act, Origin};
    use crate::drive_input::DriveDevice;

    /// A builder on a system without a run (the winch example, copied).
    fn builder() -> Builder {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dir = std::env::temp_dir().join(format!("builder-drive-devices-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("winch.system.json");
        std::fs::copy(root.join("examples/systems-builder/worm-drive/winch.system.json"), &path).unwrap();
        let registry = sim_runtime::registry_with_parts(&root.join("library/parts")).0;
        Builder::open(path, root.join("library/systems"), registry).unwrap()
    }

    /// A device request written for Build mode becomes `BuildAction::Drive`
    /// with its origin and reaches the one drive apply (`Builder::drive_request`):
    /// with nothing running it is refused naming why; a quiet request's
    /// refusal leaves the status line alone, a shown one's is the status
    /// line. A request made for Robot mode is not Build's, and a builder
    /// without a robot system's run offers no drive target.
    #[test]
    fn a_device_request_reaches_build_drive_through_the_one_apply() {
        let axes = DriveRequest::Axes { forward: 1.0, lateral: 0.0, yaw: 0.0 };
        let (action, origin) = device_action(&Act::quiet(DriveDevice { mode: ViewerMode::Build, request: axes.clone() })).unwrap();
        assert!(matches!(&action, BuildAction::Drive { request } if *request == axes));
        assert_eq!(origin, Origin::Quiet);
        assert!(device_action(&Act::quiet(DriveDevice { mode: ViewerMode::Robot, request: axes.clone() })).is_none());
        let mut b = builder();
        let status = b.status.clone();
        let e = apply_device(&mut b, action, origin).unwrap_err();
        assert!(e.starts_with("nothing is running"), "{e}");
        assert_eq!(b.status, status, "a quiet device request's refusal is not the status line");
        let (stop, shown) = device_action(&Act::ui(DriveDevice { mode: ViewerMode::Build, request: DriveRequest::Stop })).unwrap();
        assert_eq!(shown, Origin::Ui);
        let e = apply_device(&mut b, stop, shown).unwrap_err();
        assert_eq!(b.status, e);
        assert_eq!(b.drive_target(), DriveTarget::default());
    }
}
