//! Pure drive kinematics: the body twist, normalized-axis scaling, the
//! acceleration limit, the deadman rule and the wheel mixers.
//!
//! Standard library only, and no `crate::` paths: the golden-vector generator
//! (`tests/fixtures/gen_drive_golden.rs`) compiles this file on its own with
//! `rustc`, so the committed golden file comes from this exact code.
//!
//! Conventions, shared with `SteeredGait::command` (contact_phase/steered.rs)
//! and `planar::advance_planar`: a twist is `[forward m/s, lateral m/s, yaw
//! rad/s]` in the body frame; lateral is positive to the left and yaw is
//! positive counter-clockwise seen from above. A wheel's *rolling rate* is
//! positive when it rolls the body forward; its joint rate is the rolling
//! rate times the wheel's joint sign (derived from the joint axis).

use std::fmt;

/// Index of each axis in a twist, an axes triple and a limits array.
pub const FORWARD: usize = 0;
pub const LATERAL: usize = 1;
pub const YAW: usize = 2;
/// Axis names in index order (also the profile's axis keys).
pub const AXIS_NAMES: [&str; 3] = ["forward", "lateral", "yaw"];
/// Speed units in axis order.
pub const SPEED_UNITS: [&str; 3] = ["m/s", "m/s", "rad/s"];
/// Acceleration units in axis order.
pub const ACCEL_UNITS: [&str; 3] = ["m/s^2", "m/s^2", "rad/s^2"];

/// A body-frame twist: forward and lateral in m/s, yaw in rad/s.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct BodyTwist {
    pub forward_m_s: f64,
    pub lateral_m_s: f64,
    pub yaw_rad_s: f64,
}

impl BodyTwist {
    pub const ZERO: BodyTwist = BodyTwist { forward_m_s: 0.0, lateral_m_s: 0.0, yaw_rad_s: 0.0 };
    pub fn new(forward_m_s: f64, lateral_m_s: f64, yaw_rad_s: f64) -> Self {
        Self { forward_m_s, lateral_m_s, yaw_rad_s }
    }
    /// `[forward, lateral, yaw]`, the order `SteeredGait::command` takes.
    pub fn to_array(self) -> [f64; 3] {
        [self.forward_m_s, self.lateral_m_s, self.yaw_rad_s]
    }
    pub fn from_array(a: [f64; 3]) -> Self {
        Self::new(a[FORWARD], a[LATERAL], a[YAW])
    }
    pub fn is_zero(self) -> bool {
        self.to_array().iter().all(|v| *v == 0.0)
    }
}

/// Normalized axis requests, each in -1..=1 (a device binding's output).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Axes {
    pub forward: f64,
    pub lateral: f64,
    pub yaw: f64,
}

impl Axes {
    pub const ZERO: Axes = Axes { forward: 0.0, lateral: 0.0, yaw: 0.0 };
    pub fn to_array(self) -> [f64; 3] {
        [self.forward, self.lateral, self.yaw]
    }
    pub fn from_array(a: [f64; 3]) -> Self {
        Self { forward: a[FORWARD], lateral: a[LATERAL], yaw: a[YAW] }
    }
}

/// A profile's limits in plain numbers, in axis order. An unsupported axis
/// has `supported[i] == false`; its speed is then always zero.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    pub supported: [bool; 3],
    /// Largest speed magnitude per axis (m/s, m/s, rad/s); nonnegative.
    pub max_speed: [f64; 3],
    /// Largest change of speed per second per axis (m/s^2, m/s^2, rad/s^2); positive.
    pub max_accel: [f64; 3],
}

/// What happens when requests stop arriving (the deadman expires).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OnLoss {
    /// Ramp every axis to zero, each at most `decel` per second (positive).
    Ramp { decel: [f64; 3] },
    /// Zero at once.
    Immediate,
}

/// The deadman rule: a request older than `timeout_s` is lost.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Deadman {
    pub timeout_s: f64,
    pub on_loss: OnLoss,
}

/// Why a kinematic request was refused. Every variant names the axis or
/// quantity, so a REST caller or a log line can say what was wrong.
#[derive(Clone, Debug, PartialEq)]
pub enum KinematicsError {
    /// A NaN or infinite input; `what` names it (for example "axes.yaw").
    NotFinite { what: String },
    /// A normalized axis outside -1..=1.
    OutOfRange { axis: &'static str, value: f64 },
    /// A nonzero request on an axis the profile or mixer does not support.
    Unsupported { axis: &'static str, value: f64 },
    /// A twist faster than the profile allows on an axis.
    OutOfProfile { axis: &'static str, value: f64, max: f64 },
    /// A negative or non-finite step, timeout or limit; `what` names it.
    BadParameter { what: String },
}

impl KinematicsError {
    /// A short kind name, the `error` value of the golden file's refusal cases.
    pub fn kind(&self) -> &'static str {
        match self {
            KinematicsError::NotFinite { .. } => "not_finite",
            KinematicsError::OutOfRange { .. } => "out_of_range",
            KinematicsError::Unsupported { .. } => "unsupported",
            KinematicsError::OutOfProfile { .. } => "out_of_profile",
            KinematicsError::BadParameter { .. } => "bad_parameter",
        }
    }
}

impl fmt::Display for KinematicsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KinematicsError::NotFinite { what } => write!(f, "{what} must be a finite number"),
            KinematicsError::OutOfRange { axis, value } => write!(f, "axis `{axis}` = {value} is outside -1..1"),
            KinematicsError::Unsupported { axis, value } => write!(f, "axis `{axis}` = {value} is not supported by this drive"),
            KinematicsError::OutOfProfile { axis, value, max } => write!(f, "`{axis}` speed {value} exceeds the profile's {max}"),
            KinematicsError::BadParameter { what } => write!(f, "{what} must be finite and in range"),
        }
    }
}

impl std::error::Error for KinematicsError {}

fn finite(value: f64, what: impl FnOnce() -> String) -> Result<f64, KinematicsError> {
    if value.is_finite() { Ok(value) } else { Err(KinematicsError::NotFinite { what: what() }) }
}

fn check_limits(limits: &Limits) -> Result<(), KinematicsError> {
    for i in 0..3 {
        if !(limits.max_speed[i].is_finite() && limits.max_speed[i] >= 0.0) {
            return Err(KinematicsError::BadParameter { what: format!("max_speed.{}", AXIS_NAMES[i]) });
        }
        if !(limits.max_accel[i].is_finite() && limits.max_accel[i] > 0.0) {
            return Err(KinematicsError::BadParameter { what: format!("max_accel.{}", AXIS_NAMES[i]) });
        }
    }
    Ok(())
}

/// Normalized axes to a twist: each axis times the profile's max speed. A
/// twist outside the profile cannot come out of this, so it is the only way
/// a device binding's axes become a request. NaN, an axis outside -1..=1 and
/// a nonzero unsupported axis are refused by name.
pub fn scale(axes: Axes, limits: &Limits) -> Result<BodyTwist, KinematicsError> {
    check_limits(limits)?;
    let a = axes.to_array();
    let mut out = [0.0; 3];
    for i in 0..3 {
        let v = finite(a[i], || format!("axes.{}", AXIS_NAMES[i]))?;
        if !(-1.0..=1.0).contains(&v) {
            return Err(KinematicsError::OutOfRange { axis: AXIS_NAMES[i], value: v });
        }
        if !limits.supported[i] {
            if v != 0.0 {
                return Err(KinematicsError::Unsupported { axis: AXIS_NAMES[i], value: v });
            }
            continue;
        }
        out[i] = v * limits.max_speed[i];
    }
    Ok(BodyTwist::from_array(out))
}

/// Check a twist against the profile: finite, within each axis's max speed,
/// zero on unsupported axes.
pub fn check_twist(twist: BodyTwist, limits: &Limits) -> Result<(), KinematicsError> {
    check_limits(limits)?;
    let t = twist.to_array();
    for i in 0..3 {
        let v = finite(t[i], || format!("twist.{}", AXIS_NAMES[i]))?;
        if !limits.supported[i] && v != 0.0 {
            return Err(KinematicsError::Unsupported { axis: AXIS_NAMES[i], value: v });
        }
        if v.abs() > limits.max_speed[i] {
            return Err(KinematicsError::OutOfProfile { axis: AXIS_NAMES[i], value: v, max: limits.max_speed[i] });
        }
    }
    Ok(())
}

/// The acceleration limit: each axis moves from `previous` toward `request`
/// by at most `max_accel * dt_s`. Pure; `dt_s` must be finite and >= 0.
pub fn limit(previous: BodyTwist, request: BodyTwist, dt_s: f64, max_accel: [f64; 3]) -> Result<BodyTwist, KinematicsError> {
    if !(dt_s.is_finite() && dt_s >= 0.0) {
        return Err(KinematicsError::BadParameter { what: "dt_s".into() });
    }
    let (p, r) = (previous.to_array(), request.to_array());
    let mut out = [0.0; 3];
    for i in 0..3 {
        finite(p[i], || format!("previous.{}", AXIS_NAMES[i]))?;
        finite(r[i], || format!("request.{}", AXIS_NAMES[i]))?;
        if !(max_accel[i].is_finite() && max_accel[i] > 0.0) {
            return Err(KinematicsError::BadParameter { what: format!("max_accel.{}", AXIS_NAMES[i]) });
        }
        let step = max_accel[i] * dt_s;
        out[i] = p[i] + (r[i] - p[i]).clamp(-step, step);
    }
    Ok(BodyTwist::from_array(out))
}

/// True when a request `age_s` old is lost. A NaN or negative age counts as
/// lost, so a broken clock stops the robot rather than keeping it moving.
pub fn deadman_expired(age_s: f64, deadman: &Deadman) -> bool {
    !(age_s >= 0.0 && age_s < deadman.timeout_s)
}

/// One limited, deadman-checked command: the twist to send this step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Commanded {
    pub twist: BodyTwist,
    /// The deadman expired: the request was ignored and the stop rule applied.
    pub expired: bool,
}

/// One control step of the shared rule, pure in (previous twist, request,
/// dt, age since the last request). A live request is checked against the
/// profile and approached under `max_accel`; a lost one (age >= timeout)
/// is replaced by the stop rule: a ramp to zero under its decel limit, or
/// zero at once.
pub fn step(previous: BodyTwist, request: BodyTwist, dt_s: f64, age_s: f64, limits: &Limits, deadman: &Deadman) -> Result<Commanded, KinematicsError> {
    if !(deadman.timeout_s.is_finite() && deadman.timeout_s > 0.0) {
        return Err(KinematicsError::BadParameter { what: "deadman.timeout_s".into() });
    }
    if deadman_expired(age_s, deadman) {
        let twist = match deadman.on_loss {
            OnLoss::Immediate => BodyTwist::ZERO,
            OnLoss::Ramp { decel } => limit(previous, BodyTwist::ZERO, dt_s, decel)?,
        };
        return Ok(Commanded { twist, expired: true });
    }
    check_twist(request, limits)?;
    Ok(Commanded { twist: limit(previous, request, dt_s, limits.max_accel)?, expired: false })
}

fn positive(value: f64, what: &str) -> Result<f64, KinematicsError> {
    if value.is_finite() && value > 0.0 { Ok(value) } else { Err(KinematicsError::BadParameter { what: what.into() }) }
}

fn signs<const N: usize>(signs: [f64; N]) -> Result<[f64; N], KinematicsError> {
    for (i, s) in signs.iter().enumerate() {
        if *s != 1.0 && *s != -1.0 {
            return Err(KinematicsError::BadParameter { what: format!("signs[{i}] (must be +1 or -1)") });
        }
    }
    Ok(signs)
}

/// A two-wheel differential drive. Joint rates are `[left, right]` in rad/s.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DifferentialDrive {
    /// Distance between the two wheels' contact lines (m).
    pub track_width_m: f64,
    pub wheel_radius_m: f64,
    /// Joint sign per wheel `[left, right]`: joint rate = sign × rolling rate.
    pub signs: [f64; 2],
}

impl DifferentialDrive {
    pub fn new(track_width_m: f64, wheel_radius_m: f64, joint_signs: [f64; 2]) -> Result<Self, KinematicsError> {
        Ok(Self { track_width_m: positive(track_width_m, "track_width_m")?, wheel_radius_m: positive(wheel_radius_m, "wheel_radius_m")?, signs: signs(joint_signs)? })
    }
    /// Twist to joint rates `[left, right]` (rad/s). A differential drive
    /// cannot move sideways, so a nonzero lateral speed is refused.
    pub fn mix(&self, twist: BodyTwist) -> Result<[f64; 2], KinematicsError> {
        for (i, v) in twist.to_array().iter().enumerate() {
            finite(*v, || format!("twist.{}", AXIS_NAMES[i]))?;
        }
        if twist.lateral_m_s != 0.0 {
            return Err(KinematicsError::Unsupported { axis: "lateral", value: twist.lateral_m_s });
        }
        let half = 0.5 * self.track_width_m * twist.yaw_rad_s;
        let left = (twist.forward_m_s - half) / self.wheel_radius_m;
        let right = (twist.forward_m_s + half) / self.wheel_radius_m;
        Ok([self.signs[0] * left, self.signs[1] * right])
    }
    /// Joint rates `[left, right]` back to the twist they produce (lateral 0).
    pub fn unmix(&self, joint_rates: [f64; 2]) -> BodyTwist {
        let left = self.signs[0] * joint_rates[0] * self.wheel_radius_m;
        let right = self.signs[1] * joint_rates[1] * self.wheel_radius_m;
        BodyTwist::new(0.5 * (left + right), 0.0, (right - left) / self.track_width_m)
    }
}

/// A four-wheel mecanum drive in the X roller layout (rollers at 45°, the
/// front-left and rear-right rollers' axes forming an X seen from above).
/// Joint rates are `[front_left, front_right, rear_left, rear_right]` in rad/s.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mecanum {
    pub track_width_m: f64,
    /// Distance between the front and rear axles (m).
    pub wheelbase_m: f64,
    pub wheel_radius_m: f64,
    /// Joint sign per wheel in mixer order.
    pub signs: [f64; 4],
}

impl Mecanum {
    pub fn new(track_width_m: f64, wheelbase_m: f64, wheel_radius_m: f64, joint_signs: [f64; 4]) -> Result<Self, KinematicsError> {
        Ok(Self {
            track_width_m: positive(track_width_m, "track_width_m")?,
            wheelbase_m: positive(wheelbase_m, "wheelbase_m")?,
            wheel_radius_m: positive(wheel_radius_m, "wheel_radius_m")?,
            signs: signs(joint_signs)?,
        })
    }
    fn lever_m(&self) -> f64 {
        0.5 * (self.track_width_m + self.wheelbase_m)
    }
    /// Twist to joint rates `[fl, fr, rl, rr]` (rad/s).
    pub fn mix(&self, twist: BodyTwist) -> Result<[f64; 4], KinematicsError> {
        for (i, v) in twist.to_array().iter().enumerate() {
            finite(*v, || format!("twist.{}", AXIS_NAMES[i]))?;
        }
        let [vx, vy, w] = twist.to_array();
        let k = self.lever_m() * w;
        let r = self.wheel_radius_m;
        let rolling = [(vx - vy - k) / r, (vx + vy + k) / r, (vx + vy - k) / r, (vx - vy + k) / r];
        Ok(std::array::from_fn(|i| self.signs[i] * rolling[i]))
    }
    /// Joint rates `[fl, fr, rl, rr]` back to the twist (the least-squares
    /// inverse; exact for rates produced by `mix`).
    pub fn unmix(&self, joint_rates: [f64; 4]) -> BodyTwist {
        let [fl, fr, rl, rr]: [f64; 4] = std::array::from_fn(|i| self.signs[i] * joint_rates[i] * self.wheel_radius_m);
        BodyTwist::new(0.25 * (fl + fr + rl + rr), 0.25 * (-fl + fr + rl - rr), 0.25 * (-fl + fr - rl + rr) / self.lever_m())
    }
}
