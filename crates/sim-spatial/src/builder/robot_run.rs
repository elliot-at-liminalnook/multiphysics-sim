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
//! Twist requests come from the one apply, [`Builder::drive`]
//! (`BuildAction::Drive`, REST `system_drive`), as `RunControl::Twist`;
//! the run thread applies every queued command in order before the next
//! period. The run thread is the one writer of `RunShared::drive`.
use super::*;
use sim_domain_control::drive::kinematics::BodyTwist;
use sim_runtime::drive_host::{DriveHost, DriveRequest, DriveStatus, twist_json};
use sim_runtime::system_robot::{self, RobotSystem};

pub(super) const RUNNING_STATUS: &str = "Running the robot system on the shared drive host (its linked robot, controller binding and drive profile; background thread, paced to real time at most).";
/// Why a robot system's run is not kept as a run record.
pub(super) const NO_RUN_RECORD: &str = "a robot system's run is not kept as a run record yet: its drive session (the scene with the controller identity, the seed and one twist per seam period) is not written from Build mode; drive the robot in Robot mode (robot_save_recording) to record it";
/// The Build-mode drive rule (Robot mode's `run::DRIVE_RULE`, for a system run).
pub(super) const DRIVE_RULE: &str = "requests are normalized axes, a profile action or stop, interpreted against the linked drive profile (DriveRequest::interpret: kinematics::scale) by the one apply (Builder::drive, for the run-panel drive buttons, system_ui and REST system_drive); once per seam period the run thread sends the limited twist (kinematics::step: acceleration limit, deadman on simulated time) and the heartbeat on the controller's four command channels, and the controller mixes it. A nonzero request moves the robot only while the run runs (the last Run, not Pause or Reset); a stop or halt is accepted until the run fails or ends (also while a reset is in progress: it waits in the channel behind the Reset), and one made while the system is still loading waits in the run thread's channel and is applied, in order, right after it loads. The run thread applies every queued request in order before each period";
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
}

impl LiveRun {
    /// A robot system's run: `robot_thread` on the "builder-run" `RunThread`.
    pub(super) fn spawn_robot(document: SystemDocument, path: PathBuf, registry: BehaviorRegistry, description_id: String) -> Self {
        let initial = RunShared { snapshot: None, running: true, speed: 1.0, history: BTreeMap::new(), reset_pending: false, drive: None };
        let (simulated, source_id) = (document.clone(), description_id.clone());
        // Join bound zero: dropping the run never waits on the thread, which may be inside
        // `DriveHost::new` (starting python3, up to EXTERNAL_REPLY_TIMEOUT) or a period; it
        // exits at its next command check and its drop closes the controller process.
        let worker = crate::jobs::RunThread::spawn("builder-run", initial, move |commands, shared| robot_thread(simulated, path, registry, source_id, commands, shared)).join_bound(std::time::Duration::ZERO);
        Self { worker, description_id, fidelity: Fidelity::Detailed, document, edited: false, robot: true, requested_running: true, drive_requested: None, drive_refusal: None }
    }
}

impl Builder {
    /// The one drive apply (`BuildAction::Drive` from the run-panel buttons
    /// and `system_ui`, REST `system_drive`): interprets `request` against the
    /// loaded system's profile (`DriveRequest::interpret`), checks the run
    /// rule ([`DRIVE_RULE`]) and sends `RunControl::Twist`. A refusal is kept
    /// for `system_state.live_run.drive.last_refusal`. Answers that drive block.
    pub fn drive(&mut self, request: DriveRequest) -> Result<serde_json::Value, String> {
        let checked = self.check_drive(&request);
        let run = self.run.as_mut().ok_or("nothing is running: start the robot system's run first (the Run button, or REST system_run start)")?;
        match checked {
            Err(e) => {
                run.drive_refusal = Some(e.clone());
                self.panel_dirty = true;
                Err(e)
            }
            Ok((request, halt)) => {
                run.worker.send(RunControl::Twist { request, halt }).map_err(|_| "the run has ended; start a new run".to_string())?;
                run.drive_requested = Some((request, halt));
                run.drive_refusal = None;
                self.panel_dirty = true;
                Ok(self.drive_json())
            }
        }
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
            "host": "sim_runtime::drive_host::DriveHost (the shared Session: PhysicalRobot with the external controller attached on the model's control.external seam)",
        })
    }
}

/// The run thread's state: the resolved system, its host, and what it publishes.
struct Worker {
    shared: Arc<Mutex<RunShared>>,
    path: PathBuf,
    registry: BehaviorRegistry,
    source_id: String,
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
            s.drive = Some(RobotDrive { system: self.system.clone(), status: self.host.as_ref().map(|h| h.status()), twist_error: self.twist_error.clone(), phase: self.phase(), seed: self.seed });
        }
    }
}

fn robot_thread(document: SystemDocument, path: PathBuf, registry: BehaviorRegistry, source_id: String, commands: mpsc::Receiver<RunControl>, shared: Arc<Mutex<RunShared>>) {
    // The document the next Reset rebuilds from: every edit while running arrives as a Swap.
    let mut document = document;
    let mut w = Worker {
        shared,
        path,
        registry,
        source_id,
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
                RunControl::Pause => w.running = false,
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
