//! The one place a driven [`Session`] is stepped: a robot whose external
//! controller (`controller_binding`) takes a body twist on the seam's command
//! channels ([`COMMAND_CHANNELS`]). Every host that drives such a robot
//! (Robot mode's `--robot FILE` run thread, Build mode's robot-system run
//! thread) owns a [`DriveHost`] on its run thread and feeds it requests; the
//! shared limiter and deadman (`kinematics::step`) run here on simulation
//! time, once per seam period, before `Session::step`.
//!
//! Also here: the request vocabulary every input shares ([`DriveRequest`]:
//! normalized axes, a profile action, stop), its one interpretation against a
//! robot's profile ([`DriveRequest::interpret`]) and the published
//! [`DriveStatus`]. No mixing happens here: the twist goes to the controller,
//! which mixes it into wheel commands.
//!
//! Pause ([`PAUSE_RULE`]): no simulation time passes while a run is paused,
//! so a request's age would freeze and it would drive again on resume with
//! no new input. Every host's pause path calls [`TwistState::pause`] (or
//! [`DriveHost::pause`]), which invalidates the request the way a replay's
//! end does; after resume the profile's on-loss rule runs until a fresh
//! request arrives.
use crate::controller_binding::{COMMAND_CHANNELS, ControlledRobot};
use crate::session::{EpisodeFrame, InputChannel, Scene, Session};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sim_domain_control::drive::kinematics::{self, Axes, BodyTwist, Deadman, Limits};
use sim_domain_control::drive::profile::{ActionRequest, DriveProfile, ResolvedDrive};

/// Largest heartbeat the `command.heartbeat` channel carries (2^53, exact in
/// f64): the runtime's own bound on that input (`controller_binding::HEARTBEAT_MAX`), as an integer.
pub const HEARTBEAT_MAX: u64 = crate::controller_binding::HEARTBEAT_MAX as u64;

/// The pause rule, in one sentence (cited by [`TwistState::pause`] and the
/// hosts that call it).
pub const PAUSE_RULE: &str = "A drive request live at Pause is invalidated like a replay's end (the request becomes zero and the deadman counts as expired, so on resume the profile's on-loss rule runs, ramping to zero at stop_decel or zeroing at once, until a fresh request arrives, while the commanded twist is kept), and every host's pause path calls TwistState::pause: Robot mode's run thread, Build mode's robot-system run thread and the browser's DriveSession.";

/// What a drive request asks for. Axes are normalized (-1..1) and scaled by
/// the robot's profile ([`kinematics::scale`], in [`DriveRequest::interpret`]),
/// so a twist outside the profile cannot be requested; actions are the
/// profile's, by name. Robot mode's `RobotAction::Drive` and Build mode's
/// `BuildAction::Drive` carry it; REST `robot_drive` and `system_drive` build
/// it with [`DriveRequest::from_fields`].
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DriveRequest {
    /// Normalized axes: forward (+ ahead), lateral (+ left), yaw (+ turn left, CCW).
    Axes { forward: f64, lateral: f64, yaw: f64 },
    /// One of the profile's named actions (`stop`: zero twist under the
    /// acceleration limit; `halt`: zero at once).
    Action { name: String },
    /// Request a zero twist (approached under the profile's acceleration limit).
    Stop,
}
impl DriveRequest {
    /// A REST command's fields as one request: exactly one of the axes
    /// (forward, lateral, yaw; absent ones are 0), `action` (a profile action
    /// name) or `stop: true`. `command` names the REST command in the errors
    /// (`robot_drive`, `system_drive`).
    pub fn from_fields(forward: Option<f64>, lateral: Option<f64>, yaw: Option<f64>, action: Option<String>, stop: Option<bool>, command: &str) -> Result<DriveRequest, String> {
        let axes = forward.is_some() || lateral.is_some() || yaw.is_some();
        match (axes, action, stop) {
            (true, None, None) => Ok(DriveRequest::Axes { forward: forward.unwrap_or(0.0), lateral: lateral.unwrap_or(0.0), yaw: yaw.unwrap_or(0.0) }),
            (false, Some(name), None) => Ok(DriveRequest::Action { name }),
            (false, None, Some(true)) => Ok(DriveRequest::Stop),
            (false, None, Some(false)) => Err(format!("{command} stop must be true; to drive give forward, lateral or yaw")),
            (axes, action, stop) => {
                let given: Vec<&str> = [(axes, "axes (forward/lateral/yaw)"), (action.is_some(), "action"), (stop.is_some(), "stop")].into_iter().filter(|(on, _)| *on).map(|(_, name)| name).collect();
                let given = if given.is_empty() { "none".to_string() } else { given.join(" and ") };
                Err(format!("{command} needs exactly one of axes (forward, lateral, yaw: each -1..1), action (a profile action name) or stop: true; given: {given}"))
            }
        }
    }

    /// The twist this request asks the run thread for, and whether it is a
    /// halt (zero at once): axes scaled by the profile's max speeds
    /// (`kinematics::scale`, which refuses a value outside -1..1, a non-finite
    /// one and a nonzero axis the profile does not support, naming the axis);
    /// an action by its name in the profile (`stop` or `halt`; an unknown name
    /// is refused listing the profile's actions); stop as a zero twist.
    pub fn interpret(&self, controlled: &ControlledRobot) -> Result<(BodyTwist, bool), String> {
        self.interpret_with(&controlled.profile, &controlled.resolved)
    }
    /// [`DriveRequest::interpret`] against a profile and its resolved form
    /// directly (the embedded drive, `crate::embedded_drive`, has no
    /// [`ControlledRobot`]).
    pub fn interpret_with(&self, profile: &DriveProfile, resolved: &ResolvedDrive) -> Result<(BodyTwist, bool), String> {
        match self {
            DriveRequest::Axes { forward, lateral, yaw } => {
                let twist = kinematics::scale(Axes { forward: *forward, lateral: *lateral, yaw: *yaw }, &resolved.limits())
                    .map_err(|e| format!("drive request refused: {e} (profile {})", resolved.profile))?;
                Ok((twist, false))
            }
            DriveRequest::Action { name } => match profile.action(name)?.request {
                ActionRequest::Stop => Ok((BodyTwist::ZERO, false)),
                ActionRequest::Halt => Ok((BodyTwist::ZERO, true)),
            },
            DriveRequest::Stop => Ok((BodyTwist::ZERO, false)),
        }
    }
}

/// Session inputs must be exactly the four command channels, in order, or
/// the actions [`TwistState::advance`] returns would be misassigned.
pub fn check_inputs(inputs: &[InputChannel]) -> Result<(), String> {
    if inputs.iter().map(|c| c.name.as_str()).eq(COMMAND_CHANNELS) {
        Ok(())
    } else {
        Err(format!("the drive session's command inputs are [{}]; the run thread sends [{}] (controller_binding::COMMAND_CHANNELS)", inputs.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", "), COMMAND_CHANNELS.join(", ")))
    }
}

/// The run thread's drive state: the latest request, the limited twist it
/// commanded and the heartbeat. Pure (no session), so it is unit-tested alone.
/// A request stays live while its sim-time age is below the deadman timeout;
/// a replay's end ([`TwistState::replay_ended`]) and a pause
/// ([`TwistState::pause`], [`PAUSE_RULE`]) end it at once.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TwistState {
    pub request: BodyTwist,
    pub commanded: BodyTwist,
    /// Sim time (s) of the last fresh request (0 before any: the zero request at t = 0).
    pub last_request_s: f64,
    /// Fresh requests so far (`command.heartbeat`).
    pub heartbeat: u64,
    /// The last period's deadman verdict.
    pub expired: bool,
    /// The last request was a halt.
    pub halted: bool,
    /// The action last sent: `[forward, lateral, yaw, heartbeat]` (COMMAND_CHANNELS order).
    pub sent: [f64; 4],
    /// Seam periods stepped since the build.
    pub periods: u64,
}
impl Default for TwistState {
    fn default() -> Self {
        Self { request: BodyTwist::ZERO, commanded: BodyTwist::ZERO, last_request_s: 0.0, heartbeat: 0, expired: false, halted: false, sent: [0.0; 4], periods: 0 }
    }
}
impl TwistState {
    /// A fresh request at sim time `now_s`: checked against the profile,
    /// heartbeat + 1; a halt zeroes the request and the commanded twist at once.
    pub fn request(&mut self, request: BodyTwist, halt: bool, now_s: f64, limits: &Limits) -> Result<(), String> {
        kinematics::check_twist(request, limits).map_err(|e| format!("drive request refused: {e}"))?;
        if !now_s.is_finite() {
            return Err(format!("drive request refused: the simulation time {now_s} is not finite"));
        }
        if self.heartbeat >= HEARTBEAT_MAX {
            return Err(format!("drive request refused: command.heartbeat is exhausted at {HEARTBEAT_MAX}; Reset starts a new session"));
        }
        self.heartbeat += 1;
        self.last_request_s = now_s;
        self.halted = halt;
        if halt {
            self.request = BodyTwist::ZERO;
            self.commanded = BodyTwist::ZERO;
        } else {
            self.request = request;
        }
        Ok(())
    }
    /// One seam period starting at sim time `now_s`: the shared rule
    /// (`kinematics::step` with age = now − last request) and the action to
    /// send. The result is clamped into ±max_speed (0 on an unsupported axis):
    /// mathematically a no-op, it removes the rounding of `p + (r − p)` that
    /// could otherwise exceed the session's input bounds by one ulp.
    pub fn advance(&mut self, now_s: f64, period_s: f64, limits: &Limits, deadman: &Deadman) -> Result<[f64; 4], String> {
        let age = now_s - self.last_request_s;
        let c = kinematics::step(self.commanded, self.request, period_s, age, limits, deadman).map_err(|e| format!("drive limiter at t = {now_s:.3} s: {e}"))?;
        let t = c.twist.to_array();
        let bounded: [f64; 3] = std::array::from_fn(|i| if limits.supported[i] { t[i].max(-limits.max_speed[i]).min(limits.max_speed[i]) } else { 0.0 });
        self.commanded = BodyTwist::from_array(bounded);
        self.expired = c.expired;
        self.sent = [bounded[0], bounded[1], bounded[2], self.heartbeat as f64];
        self.periods += 1;
        Ok(self.sent)
    }
    /// A recorded action re-sent by a replay: the state shows what was sent
    /// (the original request is not in the recording, so it is shown as the
    /// commanded twist). Called after the period stepped, with `now_s` the
    /// sim time at the START of that period: a live request is stamped with
    /// the time the next period starts at, so a replayed one is too.
    pub fn replayed(&mut self, action: &[f64], now_s: f64) {
        let at = |i: usize| action.get(i).copied().unwrap_or(0.0);
        let twist = BodyTwist::new(at(0), at(1), at(2));
        let heartbeat = at(3).max(0.0) as u64;
        if heartbeat != self.heartbeat || self.periods == 0 {
            self.last_request_s = now_s;
        }
        (self.request, self.commanded, self.heartbeat, self.halted) = (twist, twist, heartbeat, false);
        self.sent = [at(0), at(1), at(2), at(3)];
        self.periods += 1;
    }
    /// A drive replay ended (finished, failed or cancelled) at sim time
    /// `now_s`: the recorded requests were not live, so nothing may keep
    /// driving after it. The request is zero and the deadman counts as
    /// expired (the last request is stamped 2 × timeout_s before `now_s`, so
    /// the next period's age is past the timeout without relying on rounding
    /// at exactly timeout_s): the next Run ramps the commanded twist to zero
    /// by the stop rule until a fresh request arrives. The heartbeat and
    /// periods are kept (they count what the session received).
    pub fn replay_ended(&mut self, now_s: f64, deadman: &Deadman) {
        self.invalidate(now_s, deadman);
    }
    /// The shared invalidation behind [`TwistState::replay_ended`] and [`TwistState::pause`].
    fn invalidate(&mut self, now_s: f64, deadman: &Deadman) {
        self.request = BodyTwist::ZERO;
        self.halted = false;
        self.expired = true;
        self.last_request_s = self.last_request_s.min(now_s - 2.0 * deadman.timeout_s);
    }
    /// The run paused at sim time `now_s` ([`PAUSE_RULE`]): no sim time passes
    /// while paused, so a request live at Pause would otherwise keep its age
    /// and drive again on resume for up to `timeout_s` with no new input.
    /// Pause invalidates it the way a replay's end does
    /// ([`TwistState::replay_ended`]): the request is zero and the deadman
    /// counts as expired, so after resume the profile's on-loss rule
    /// (`kinematics::step`: ramp to stop, or zero at once) runs until a
    /// fresh [`TwistState::request`] arrives. The commanded twist is kept
    /// (a ramp-to-stop profile decelerates from it; nothing lurches), and so
    /// are the heartbeat, the periods and the last action sent. Pausing again
    /// at the same time changes nothing; pausing with no request (or a zero
    /// one) only marks the deadman expired, so a stopped robot stays stopped.
    /// A halt as the last request stays reported (`halted`): the halt already
    /// zeroed the commanded twist, and the status keeps saying it was a halt.
    pub fn pause(&mut self, now_s: f64, deadman: &Deadman) {
        let halted = self.halted;
        self.invalidate(now_s, deadman);
        self.halted = halted;
    }
    pub fn status(&self, time_s: f64) -> DriveStatus {
        DriveStatus { request: self.request, commanded: self.commanded, heartbeat: self.heartbeat, age_s: time_s - self.last_request_s, expired: self.expired, halted: self.halted, time_s, periods: self.periods }
    }
}

/// What a run thread publishes about the drive, each chunk and after each request.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DriveStatus {
    pub request: BodyTwist,
    pub commanded: BodyTwist,
    pub heartbeat: u64,
    /// Sim seconds since the last fresh request.
    pub age_s: f64,
    /// The deadman expired at the last period.
    pub expired: bool,
    pub halted: bool,
    /// Sim time (s) of this status.
    pub time_s: f64,
    pub periods: u64,
}
impl DriveStatus {
    pub fn json(&self) -> Value {
        json!({"request": twist_json(self.request), "commanded": twist_json(self.commanded), "heartbeat": self.heartbeat, "age_s": self.age_s,
            "expired": self.expired, "halted": self.halted, "time_s": self.time_s, "periods": self.periods})
    }
}
/// A twist with its units.
pub fn twist_json(t: BodyTwist) -> Value {
    json!({"forward_m_s": t.forward_m_s, "lateral_m_s": t.lateral_m_s, "yaw_rad_s": t.yaw_rad_s})
}

/// A driven session: the shared [`Session`] (the robot's `PhysicalRobot`
/// with its external controller attached on the seam through the session's
/// `EpisodeCoupler` and its command channels) and the [`TwistState`] that
/// feeds it. Build it, step it and request twists on one run thread; every
/// host that drives a bound robot steps it here and nowhere else.
pub struct DriveHost {
    pub session: Session,
    pub twist: TwistState,
    limits: Limits,
    deadman: Deadman,
}
impl DriveHost {
    /// `Session::new(scene, seed)` (which starts the controller program and
    /// attaches it on the seam; errors name the program), then the command
    /// inputs are checked against [`COMMAND_CHANNELS`]. The limits and
    /// deadman are `controlled`'s resolved profile: pass the binding the
    /// scene was built from (`controller_binding::scene`), or for a replay
    /// the binding whose identity the recording was checked against.
    pub fn new(scene: Scene, seed: u64, controlled: &ControlledRobot) -> Result<Self, String> {
        let session = Session::new(scene, seed)?;
        check_inputs(&session.inputs)?;
        Ok(Self { session, twist: TwistState::default(), limits: controlled.resolved.limits(), deadman: controlled.resolved.deadman() })
    }
    pub fn limits(&self) -> &Limits {
        &self.limits
    }
    pub fn deadman(&self) -> &Deadman {
        &self.deadman
    }
    /// Simulation time (s).
    pub fn time(&self) -> f64 {
        self.session.robot.time()
    }
    /// The scene's horizon is reached: `Session::step` refuses further periods.
    pub fn ended(&self) -> bool {
        self.time() >= self.session.scene.duration_s - 1e-10
    }
    /// A fresh request at the current simulation time ([`TwistState::request`]).
    pub fn request(&mut self, request: BodyTwist, halt: bool) -> Result<(), String> {
        let now = self.time();
        self.twist.request(request, halt, now, &self.limits)
    }
    /// One seam period: the shared limiter and deadman on simulation time
    /// ([`TwistState::advance`]), then `Session::step` with
    /// `[forward, lateral, yaw, heartbeat]`. Computed on a copy and committed
    /// only when the step ran, so a failed step leaves the status describing
    /// the last period that did. A controller that exits, times out or sends
    /// a malformed reply fails the step with an error naming the program
    /// (`external controller (python) <script> on <element>`), and the
    /// session then refuses further steps until it is rebuilt.
    pub fn step(&mut self) -> Result<EpisodeFrame, String> {
        let now = self.time();
        let mut next = self.twist;
        let action = next.advance(now, self.session.scene.period_s, &self.limits, &self.deadman)?;
        let frame = self.session.step(&action)?;
        self.twist = next;
        Ok(frame)
    }
    /// One recorded action re-sent by a replay through `Session::step`
    /// ([`TwistState::replayed`], stamped at the period's start).
    pub fn step_recorded(&mut self, action: &[f64]) -> Result<EpisodeFrame, String> {
        let start = self.time();
        let frame = self.session.step(action)?;
        self.twist.replayed(action, start);
        Ok(frame)
    }
    /// A drive replay ended now ([`TwistState::replay_ended`]).
    pub fn replay_ended(&mut self) {
        let now = self.time();
        self.twist.replay_ended(now, &self.deadman);
    }
    /// The run paused now ([`TwistState::pause`]): a request live at Pause
    /// does not drive after resume until a fresh one arrives.
    pub fn pause(&mut self) {
        let now = self.time();
        self.twist.pause(now, &self.deadman);
    }
    /// The drive status now.
    pub fn status(&self) -> DriveStatus {
        self.twist.status(self.time())
    }
}

#[cfg(test)]
mod tests;
