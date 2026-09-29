//! Automated leg characterization campaign (stages A–K of
//! `examples/actuators/hx30hm/hardware/characterization-campaign/PLAN.md`).
//!
//! Tests run against a [`Rig`]: the simulated bench in simulated time
//! ([`SimRig`], fast) or the hardware bus in real time. Every test runs under
//! a [`Guard`]: travel inset, supply sag and temperature gates, drift checks,
//! and a rehearsal prediction from a model rig with divergence abort. Results
//! are fitted into an actuator profile with uncertainty and provenance.
//!
//! Low fidelity by design: host-rate loops (10 ms), first-order motor models
//! and simple fits. Values are commissioning estimates until checked against
//! detailed simulation and independent measurement.
use super::virtual_bench::{Bench, ServoMode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub type R<T> = Result<T, String>;
const RAD: f64 = std::f64::consts::TAU / 4096.;

/// One reading of an axis.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Reading {
    pub t: f64,
    pub position: f64,
    pub speed: f64,
    pub voltage_v: f64,
    pub temperature_c: f64,
    /// Supply current from an external sensor, when fitted.
    pub supply_current_a: Option<f64>,
    /// Joint-side angle in motor counts, when a joint-side sensor is fitted.
    pub joint: Option<f64>,
}

/// A device the campaign drives: simulated bench or hardware.
pub trait Rig {
    fn now(&self) -> f64;
    fn period(&self) -> f64;
    fn read(&mut self, id: u8) -> R<Reading>;
    /// Open-loop PWM duty in [-1, 1] (PWM mode).
    fn drive(&mut self, id: u8, duty: f64) -> R<()>;
    /// Servo position-mode goal (continuous counts) with a speed limit.
    fn goal(&mut self, id: u8, counts: f64, speed: f64) -> R<()>;
    fn set_mode(&mut self, id: u8, mode: ServoMode) -> R<()>;
    /// Advance one control period (simulated) or wait for it (hardware).
    fn tick(&mut self) -> R<()>;
    /// Zero drive and torque off on every axis.
    fn stop(&mut self) -> R<()>;
    /// Attach or remove a known external load (duty-equivalent). Hardware
    /// returns an error asking the operator to do it.
    fn attach_load(&mut self, id: u8, duty: f64) -> R<()>;
    /// A condition the rig observed that must stop the current test (taken).
    fn alarm(&mut self) -> Option<Abort> {
        None
    }
}

/// Per-axis drive limits for a fragile transmission (e.g. a belt that skips
/// teeth under fast acceleration). Enforced on every drive command of the
/// campaign, whatever the stage, by [`LimitedRig`].
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AxisLimits {
    /// Largest |duty| ever commanded.
    #[serde(default)]
    pub max_duty: Option<f64>,
    /// Drive changes at most this fast (per s), up and down: steps become
    /// ramps, so the motor's acceleration stays near gain × slew.
    #[serde(default)]
    pub duty_slew_per_s: Option<f64>,
    /// Measured acceleration above this (counts/s², over ~50 ms) stops the
    /// test: either the limits are not holding or the transmission slipped
    /// (a skipped tooth releases load as a sudden speed jump).
    #[serde(default)]
    pub max_acceleration_counts_s2: Option<f64>,
    /// Stages not run on this axis (e.g. "F": the servo's own position loop
    /// accelerates as hard as it can).
    #[serde(default)]
    pub skip_stages: Vec<String>,
    /// Extra inset (counts) from each saved pose for all motion on this
    /// axis: ramped drive stops later. The saved poses still bound travel.
    #[serde(default)]
    pub extra_inset_counts: f64,
}

/// Applies [`AxisLimits`] to every command of an inner rig and watches
/// measured acceleration. Emergency stops still cut torque at once.
pub struct LimitedRig<'a> {
    inner: &'a mut dyn Rig,
    limits: std::collections::BTreeMap<u8, AxisLimits>,
    last: std::collections::BTreeMap<u8, (f64, f64)>,
    history: std::collections::BTreeMap<u8, std::collections::VecDeque<(f64, f64)>>,
    alarm: Option<Abort>,
}
impl<'a> LimitedRig<'a> {
    pub fn new(inner: &'a mut dyn Rig, limits: std::collections::BTreeMap<u8, AxisLimits>) -> Self {
        Self { inner, limits, last: Default::default(), history: Default::default(), alarm: None }
    }
    /// The duty sequence the limits will actually apply, from `start`.
    pub fn shape(limits: Option<&AxisLimits>, start: f64, duties: &[f64], dt: f64) -> Vec<f64> {
        let mut d = start;
        duties.iter().map(|&want| {
            let mut want = want;
            if let Some(m) = limits.and_then(|l| l.max_duty) { want = want.clamp(-m, m); }
            // Both ways: a motor answers a drive cut as sharply as a drive
            // step (the speed follows the drive through its lag), so cutting
            // drive would decelerate the transmission as hard as a step
            // accelerates it. Stopping distance grows accordingly; limited
            // axes work inside `extra_inset_counts`.
            d = match limits.and_then(|l| l.duty_slew_per_s) { Some(r) => want.clamp(d - r * dt, d + r * dt), None => want };
            d
        }).collect()
    }
    pub fn applied_duty(&self, id: u8) -> f64 {
        self.last.get(&id).map_or(0., |l| l.0)
    }
}
impl Rig for LimitedRig<'_> {
    fn now(&self) -> f64 { self.inner.now() }
    fn period(&self) -> f64 { self.inner.period() }
    fn read(&mut self, id: u8) -> R<Reading> {
        let r = self.inner.read(id)?;
        if let Some(cap) = self.limits.get(&id).and_then(|l| l.max_acceleration_counts_s2) {
            let h = self.history.entry(id).or_default();
            h.push_back((r.t, r.speed));
            while h.len() > 2 && r.t - h[1].0 >= 0.05 { h.pop_front(); }
            if let (Some(&(t0, v0)), true) = (h.front(), h.len() > 1) {
                let a = (r.speed - v0) / (r.t - t0).max(1e-3);
                if r.t - t0 >= 0.04 && a.abs() > cap && self.alarm.is_none() {
                    self.alarm = Some(Abort::Acceleration { axis: id, counts_s2: a, limit: cap });
                }
            }
        }
        Ok(r)
    }
    fn drive(&mut self, id: u8, duty: f64) -> R<()> {
        let now = self.inner.now();
        let applied = match self.limits.get(&id) {
            Some(l) => {
                let (last, at) = self.last.get(&id).copied().unwrap_or((0., now));
                let dt = (now - at).max(self.inner.period());
                Self::shape(Some(l), last, &[duty], dt)[0]
            }
            None => duty,
        };
        self.last.insert(id, (applied, now));
        self.inner.drive(id, applied)
    }
    fn goal(&mut self, id: u8, counts: f64, speed: f64) -> R<()> { self.inner.goal(id, counts, speed) }
    fn set_mode(&mut self, id: u8, mode: ServoMode) -> R<()> {
        if mode != ServoMode::Pwm && self.limits.get(&id).is_some_and(|l| l.duty_slew_per_s.is_some() || l.max_acceleration_counts_s2.is_some()) {
            return Err(format!("Axis {id} is acceleration-limited; servo modes cannot be limited and are not used on it"));
        }
        self.inner.set_mode(id, mode)
    }
    fn tick(&mut self) -> R<()> { self.inner.tick() }
    fn stop(&mut self) -> R<()> {
        self.last.clear();
        self.history.clear();
        self.inner.stop()
    }
    fn attach_load(&mut self, id: u8, duty: f64) -> R<()> { self.inner.attach_load(id, duty) }
    fn alarm(&mut self) -> Option<Abort> {
        self.alarm.take().or_else(|| self.inner.alarm())
    }
}

/// The simulated bench stepped in simulated time.
pub struct SimRig {
    pub bench: Bench,
    pub dt: f64,
}
impl SimRig {
    pub fn new(bench: Bench, dt: f64) -> Self {
        Self { bench, dt }
    }
    fn servo(&mut self, id: u8) -> R<&mut super::virtual_bench::Servo> {
        self.bench.servos.get_mut(id.wrapping_sub(1) as usize).ok_or_else(|| format!("no axis {id}"))
    }
}
impl Rig for SimRig {
    fn now(&self) -> f64 {
        self.bench.time
    }
    fn period(&self) -> f64 {
        self.dt
    }
    fn read(&mut self, id: u8) -> R<Reading> {
        let t = self.bench.time;
        let (v, i) = (self.bench.supply_v, self.bench.supply_current_a);
        let s = self.servo(id)?;
        Ok(Reading {
            t,
            position: s.position.round(),
            speed: s.speed,
            voltage_v: v,
            temperature_c: s.winding_c,
            supply_current_a: Some(i),
            joint: Some(s.joint),
        })
    }
    fn drive(&mut self, id: u8, duty: f64) -> R<()> {
        let s = self.servo(id)?;
        s.mode = ServoMode::Pwm;
        s.torque = true;
        s.pwm = (duty.clamp(-1., 1.) * 1000.).round() as i32;
        Ok(())
    }
    fn goal(&mut self, id: u8, counts: f64, speed: f64) -> R<()> {
        let s = self.servo(id)?;
        s.torque = true;
        s.goal_position = counts.round().rem_euclid(4096.) as i32;
        s.goal_speed = speed.abs().round() as i32;
        Ok(())
    }
    fn set_mode(&mut self, id: u8, mode: ServoMode) -> R<()> {
        let s = self.servo(id)?;
        s.mode = mode;
        if mode == ServoMode::Position {
            s.goal_position = s.position.round().rem_euclid(4096.) as i32;
        }
        Ok(())
    }
    fn tick(&mut self) -> R<()> {
        self.bench.advance(self.dt);
        Ok(())
    }
    fn stop(&mut self) -> R<()> {
        for s in &mut self.bench.servos {
            s.pwm = 0;
            s.goal_speed = 0;
            s.torque = false;
            s.mode = ServoMode::Pwm;
        }
        Ok(())
    }
    fn attach_load(&mut self, id: u8, duty: f64) -> R<()> {
        self.servo(id)?.attached_load_duty = duty;
        Ok(())
    }
}

/// Travel of one axis: saved poses (counts), role for reports.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Axis {
    pub id: u8,
    pub role: String,
    pub lower: f64,
    pub upper: f64,
}
impl Axis {
    pub fn center(&self) -> f64 {
        0.5 * (self.lower + self.upper)
    }
    pub fn span(&self) -> f64 {
        self.upper - self.lower
    }
}

/// Escalation gates and limits (defaults match PLAN.md).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Gates {
    pub max_sag_fraction: f64,
    pub max_temperature_c: f64,
    pub cool_down_to_c: f64,
    pub max_drift_counts: f64,
    /// Inset from each saved pose that motion must stay inside.
    pub inset_counts: f64,
    /// Braking assumed before stage C measures it (counts/s²).
    pub assumed_braking_counts_s2: f64,
    /// Rehearsal divergence band: absolute counts plus a fraction of the
    /// predicted excursion. `model_uncertainty` scales the fraction.
    pub divergence_counts: f64,
    pub model_uncertainty: f64,
}
impl Default for Gates {
    fn default() -> Self {
        Self {
            max_sag_fraction: 0.10,
            max_temperature_c: 50.,
            cool_down_to_c: 40.,
            max_drift_counts: 12.,
            inset_counts: 40.,
            assumed_braking_counts_s2: 800.,
            divergence_counts: 40.,
            model_uncertainty: 0.6,
        }
    }
}

/// One control-period sample of the axis under test.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Sample {
    pub t: f64,
    pub id: u8,
    pub position: f64,
    pub speed: f64,
    /// NaN (written as null) when the sample carried no new drive command.
    #[serde(deserialize_with = "nan_if_null")]
    pub duty: f64,
    pub voltage_v: f64,
    pub temperature_c: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supply_current_a: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub joint: Option<f64>,
}

fn nan_if_null<'de, D: serde::Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    Ok(Option::<f64>::deserialize(d)?.unwrap_or(f64::NAN))
}

/// Why a test stopped early.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Abort {
    Sag { voltage_v: f64, preflight_v: f64 },
    Temperature { celsius: f64 },
    Travel { position: f64 },
    Divergence { tick: usize, measured: f64, predicted: f64, band: f64 },
    Drift { counts: f64 },
    Collision { detail: String },
    /// Measured acceleration above an axis limit: limits not holding, or
    /// a slipped transmission (skipped belt tooth).
    Acceleration { axis: u8, counts_s2: f64, limit: f64 },
}

/// A lesson's bounded bench test (`sim-lab`): one axis at a fixed duty for
/// a few seconds, under the same guards as the campaign (sag, temperature,
/// travel with braking margin, rig alarms). It stops early rather than run
/// into the taught travel window, and always ends with the rig stopped.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LabStep {
    pub samples: Vec<Sample>,
    /// Mean speed over the last 40 % of the run (counts/s), when it ran long enough.
    pub steady_counts_s: Option<f64>,
    /// Why it ended early, if it did.
    pub stopped: Option<String>,
    pub seconds: f64,
}

/// Longest lab step (s) and largest |duty| a lesson may ask for.
pub const LAB_MAX_SECONDS: f64 = 5.0;
pub const LAB_MAX_DUTY: f64 = 0.5;

pub fn lab_step(session: &mut Session, id: u8, duty: f64, seconds: f64) -> R<LabStep> {
    if !(duty.is_finite() && duty.abs() <= LAB_MAX_DUTY) {
        return Err(format!("lab duty {duty} is outside ±{LAB_MAX_DUTY}"));
    }
    if !(seconds > 0.0 && seconds <= LAB_MAX_SECONDS) {
        return Err(format!("lab steps last at most {LAB_MAX_SECONDS} s"));
    }
    session.axis(id)?;
    let period = session.rig.period();
    // First back away to the far end of the taught window (same guards, same
    // drive, other direction), then let the axis settle: the test gets the
    // most room to reach a steady speed before it must brake.
    let mut last = session.rig.read(id)?;
    let back = (LAB_MAX_SECONDS / period) as usize;
    for _ in 0..back {
        if session.stop_needed(id, &last, period)? {
            break;
        }
        match session.step(id, Some(-duty))? {
            Ok(r) => last = r,
            Err(abort) => return Err(format!("guard stopped the move to the start: {abort:?}")),
        }
    }
    session.rig.stop()?;
    for _ in 0..(0.3 / period) as usize {
        session.rig.tick()?;
    }
    let ticks = (seconds / period).ceil() as usize;
    let first = session.samples.len();
    let mut stopped = None;
    let start = session.rig.read(id)?;
    let t0 = start.t;
    let mut last = start;
    for _ in 0..ticks {
        // Stop while there is still room to brake inside the taught window.
        if session.stop_needed(id, &last, period)? {
            stopped = Some("stopped before the end of the taught travel window".to_string());
            break;
        }
        match session.step(id, Some(duty))? {
            Ok(r) => last = r,
            Err(abort) => {
                stopped = Some(format!("guard stopped the test: {abort:?}"));
                break;
            }
        }
    }
    session.rig.stop()?;
    let samples: Vec<Sample> = session.samples[first..].iter().filter(|s| s.id == id).cloned().collect();
    let tail = &samples[(samples.len() * 3 / 5)..];
    let steady_counts_s = (tail.len() >= 5).then(|| tail.iter().map(|s| s.speed).sum::<f64>() / tail.len() as f64);
    Ok(LabStep { seconds: last.t - t0, samples, steady_counts_s, stopped })
}

/// Runs control periods for one test, recording samples and enforcing the
/// guard. `prediction` (from the rehearsal) enables divergence abort.
pub struct Session<'a> {
    pub rig: &'a mut dyn Rig,
    pub gates: Gates,
    pub axes: Vec<Axis>,
    pub preflight_v: f64,
    pub samples: Vec<Sample>,
    /// Rehearsal of the current open-loop segment: (first tick, predicted positions).
    pub prediction: Option<(usize, Vec<f64>)>,
    /// Model used to rehearse open-loop segments (predicted positions for a
    /// duty sequence from a start reading), e.g. [`model_predictor`].
    pub model: Option<Box<dyn Fn(u8, &Reading, &[f64], f64) -> Vec<f64> + 'a>>,
    /// Braking measured in stage C, per axis id and direction (0 = decreasing).
    pub braking: std::collections::BTreeMap<(u8, usize), f64>,
    /// Drive limits (see [`LimitedRig`]); rehearsals apply the same shaping.
    pub limits: std::collections::BTreeMap<u8, AxisLimits>,
    applied: std::collections::BTreeMap<u8, f64>,
    /// Saved poses for the travel guard, when wider than the working axes.
    pub travel: std::collections::BTreeMap<u8, (f64, f64)>,
    tick: usize,
    origin: std::collections::BTreeMap<u8, f64>,
}
impl<'a> Session<'a> {
    pub fn new(rig: &'a mut dyn Rig, gates: Gates, axes: Vec<Axis>) -> R<Self> {
        let first = axes.first().ok_or("no axes")?.id;
        let preflight_v = rig.read(first)?.voltage_v;
        Ok(Self { rig, gates, axes, preflight_v, samples: vec![], prediction: None, model: None, braking: Default::default(), limits: Default::default(), applied: Default::default(), travel: Default::default(), tick: 0, origin: Default::default() })
    }
    pub fn axis(&self, id: u8) -> R<Axis> {
        self.axes.iter().find(|a| a.id == id).cloned().ok_or_else(|| format!("axis {id} not in campaign"))
    }
    /// Braking for motion of `id` in `direction` (measured, else assumed).
    pub fn braking_for(&self, id: u8, direction: usize) -> f64 {
        self.braking.get(&(id, direction)).copied().unwrap_or(self.gates.assumed_braking_counts_s2)
    }
    /// Duty the axis is receiving now (after limits); 0 when stopped.
    pub fn applied(&self, id: u8) -> f64 {
        self.applied.get(&id).copied().unwrap_or(0.)
    }
    /// Extra time before a slew-limited axis's drive has ramped to zero:
    /// roughly half of it passes at full speed (linear speed decay).
    pub fn ramp_down_s(&self, id: u8) -> f64 {
        self.limits.get(&id).and_then(|l| l.duty_slew_per_s).map_or(0., |r| 0.5 * self.applied(id).abs() / r.max(1e-6))
    }
    /// Would coasting from `r` (with `reaction` s of delay) leave the inset band?
    pub fn stop_needed(&self, id: u8, r: &Reading, reaction: f64) -> R<bool> {
        let reaction = reaction + self.ramp_down_s(id);
        let a = self.axis(id)?;
        let direction = usize::from(r.speed > 0.);
        let v = r.speed.abs();
        let stop = r.position + r.speed.signum() * (v * reaction + v * v / (2. * self.braking_for(id, direction)));
        Ok(stop > a.upper - self.gates.inset_counts || stop < a.lower + self.gates.inset_counts)
    }
    /// One control period on `id` at `duty` (None = leave the command as is).
    pub fn step(&mut self, id: u8, duty: Option<f64>) -> R<Result<Reading, Abort>> {
        if let Some(d) = duty {
            self.rig.drive(id, d)?;
            let last = self.applied.get(&id).copied().unwrap_or(0.);
            self.applied.insert(id, LimitedRig::shape(self.limits.get(&id), last, &[d], self.rig.period())[0]);
        }
        self.rig.tick()?;
        let r = self.rig.read(id)?;
        // The duty the axis actually received (after any limits).
        let applied = if duty.is_some() { self.applied(id) } else { f64::NAN };
        let _ = self.origin.entry(id).or_insert(r.position);
        self.samples.push(Sample { t: r.t, id, position: r.position, speed: r.speed, duty: applied, voltage_v: r.voltage_v, temperature_c: r.temperature_c, supply_current_a: r.supply_current_a, joint: r.joint });
        let a = self.axis(id)?;
        let travel = self.travel.get(&id).copied().unwrap_or((a.lower, a.upper));
        let abort = if r.voltage_v < self.preflight_v * (1. - self.gates.max_sag_fraction) {
            Some(Abort::Sag { voltage_v: r.voltage_v, preflight_v: self.preflight_v })
        } else if let Some(a) = self.rig.alarm() {
            Some(a)
        } else if r.temperature_c > self.gates.max_temperature_c {
            Some(Abort::Temperature { celsius: r.temperature_c })
        } else if r.position > travel.1 || r.position < travel.0 {
            Some(Abort::Travel { position: r.position })
        } else {
            self.prediction.as_ref().and_then(|(first, p)| {
                let k = self.tick.checked_sub(*first)?;
                let predicted = *p.get(k)?;
                let excursion = (predicted - p[0]).abs();
                let band = self.gates.divergence_counts + self.gates.model_uncertainty * excursion;
                ((r.position - predicted).abs() > band).then_some(Abort::Divergence { tick: self.tick, measured: r.position, predicted, band })
            })
        };
        self.tick += 1;
        if let Some(abort) = abort {
            self.rig.stop()?;
            self.applied.clear();
            return Ok(Err(abort));
        }
        Ok(Ok(r))
    }
    /// Position hold with a PI loop for `duration` s; returns the mean duty
    /// over the last third (the effort needed to hold there).
    pub fn hold(&mut self, id: u8, target: f64, duration: f64, gain: f64) -> R<Result<f64, Abort>> {
        let n = (duration / self.rig.period()).ceil() as usize;
        let (mut integral, mut tail, mut count) = (0., 0., 0);
        let mut r = self.rig.read(id)?;
        for k in 0..n {
            let error = target - r.position;
            integral = (integral + error * self.rig.period()).clamp(-200., 200.);
            let duty = (gain * error + 1.5 * gain * integral - 0.25 * gain * r.speed * 0.05).clamp(-0.6, 0.6);
            r = match self.step(id, Some(duty))? {
                Ok(r) => r,
                Err(a) => return Ok(Err(a)),
            };
            if k >= 2 * n / 3 {
                tail += duty;
                count += 1;
            }
        }
        Ok(Ok(tail / count.max(1) as f64))
    }
    /// Move to `target` at `speed` counts/s (velocity loop), then hold briefly.
    pub fn move_to(&mut self, id: u8, target: f64, speed: f64, gain: f64) -> R<Result<Reading, Abort>> {
        let mut r = self.rig.read(id)?;
        let mut duty = 0.;
        for _ in 0..((10. + (target - r.position).abs() / speed.max(1.) * 3.) / self.rig.period()) as usize {
            let error = target - r.position;
            if error.abs() < 4. {
                break;
            }
            let v_ref = error.signum() * speed.min(error.abs() * 4.);
            // A slew-limited axis integrates from the duty it actually has (no windup).
            let from = if self.limits.get(&id).is_some_and(|l| l.duty_slew_per_s.is_some()) { self.applied(id) } else { duty };
            duty = (from + 0.00005 * (v_ref - r.speed)).clamp(-0.5, 0.5);
            let d = if (duty > 0.) != (error > 0.) { 0. } else { duty };
            r = match self.step(id, Some(d))? {
                Ok(r) => r,
                Err(a) => return Ok(Err(a)),
            };
        }
        match self.hold(id, target, 0.4, gain)? {
            Ok(_) => Ok(Ok(self.rig.read(id)?)),
            Err(a) => Ok(Err(a)),
        }
    }
    /// Zero drive and wait until the axis stops (≤ 2 s); returns the rest position.
    pub fn coast(&mut self, id: u8) -> R<Result<Reading, Abort>> {
        let mut last = self.rig.read(id)?;
        for _ in 0..(2. / self.rig.period()) as usize {
            last = match self.step(id, Some(0.))? {
                Ok(r) => r,
                Err(a) => return Ok(Err(a)),
            };
            if last.speed.abs() < 1. {
                break;
            }
        }
        Ok(Ok(last))
    }
    /// Rehearse an open-loop duty sequence from the present state with the
    /// model; subsequent steps are checked against it until `end_segment`.
    pub fn rehearse(&mut self, id: u8, duties: &[f64]) -> R<()> {
        if let Some(model) = &self.model {
            let r = self.rig.read(id)?;
            let start = self.applied.get(&id).copied().unwrap_or(0.);
            let shaped = LimitedRig::shape(self.limits.get(&id), start, duties, self.rig.period());
            let predicted = model(id, &r, &shaped, self.rig.period());
            self.prediction = Some((self.tick, predicted));
        }
        Ok(())
    }
    pub fn end_segment(&mut self) {
        self.prediction = None;
    }
    /// Wait with drive off until the axis cools to the cool-down temperature.
    pub fn cool_down(&mut self, id: u8, max_s: f64) -> R<f64> {
        self.rig.drive(id, 0.)?;
        let start = self.rig.now();
        loop {
            self.rig.tick()?;
            let r = self.rig.read(id)?;
            if r.temperature_c <= self.gates.cool_down_to_c || self.rig.now() - start > max_s {
                return Ok(self.rig.now() - start);
            }
        }
    }
}

// ---------------------------------------------------------------- stages ---

/// Result of one stage on one axis.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StageResult {
    pub stage: String,
    pub id: u8,
    pub completed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abort: Option<Abort>,
    pub metrics: Value,
    pub samples: usize,
    pub simulated_s: f64,
}

fn result(stage: &str, id: u8, s: &Session, t0: f64, n0: usize, abort: Option<Abort>, metrics: Value) -> StageResult {
    StageResult { stage: stage.into(), id, completed: abort.is_none(), abort, metrics, samples: s.samples.len() - n0, simulated_s: s.rig.now() - t0 }
}
macro_rules! guard {
    ($e:expr, $stage:expr, $id:expr, $s:expr, $t0:expr, $n0:expr, $m:expr) => {
        match $e? {
            Ok(v) => v,
            Err(a) => return Ok(result($stage, $id, $s, $t0, $n0, Some(a), $m)),
        }
    };
}
/// Position bins across the inset range.
fn bins(a: &Axis, inset: f64, n: usize) -> Vec<f64> {
    (0..n).map(|k| a.lower + inset + (a.span() - 2. * inset) * (k as f64 + 0.5) / n as f64).collect()
}
fn linear_fit(xs: &[f64], ys: &[f64]) -> Option<(f64, f64, f64)> {
    let n = xs.len() as f64;
    if xs.len() < 2 {
        return None;
    }
    let (mx, my) = (xs.iter().sum::<f64>() / n, ys.iter().sum::<f64>() / n);
    let sxx: f64 = xs.iter().map(|x| (x - mx).powi(2)).sum();
    if sxx <= 0. {
        return None;
    }
    let slope = xs.iter().zip(ys).map(|(x, y)| (x - mx) * (y - my)).sum::<f64>() / sxx;
    let intercept = my - slope * mx;
    let rms = (xs.iter().zip(ys).map(|(x, y)| (y - intercept - slope * x).powi(2)).sum::<f64>() / n).sqrt();
    Some((slope, intercept, rms))
}

/// Stage A: slow constant-speed sweeps each way at several speeds. Per position
/// bin: friction = half the up/down duty difference, load = their mean;
/// viscous slope from the speed dependence.
pub fn stage_a(s: &mut Session, id: u8, speeds: &[f64]) -> R<StageResult> {
    let (t0, n0) = (s.rig.now(), s.samples.len());
    let a = s.axis(id)?;
    let inset = s.gates.inset_counts;
    let edges = bins(&a, inset, 8);
    let mut rows = Vec::new(); // (speed, direction, bin, duty)
    for &v in speeds {
        for direction in [1., -1.] {
            let start = if direction > 0. { a.lower + inset } else { a.upper - inset };
            let end = if direction > 0. { a.upper - inset } else { a.lower + inset };
            guard!(s.move_to(id, start, 150., 0.004), "A", id, s, t0, n0, json!({}));
            let mut duty: f64;
            let mut r = s.rig.read(id)?;
            let mut acc: Vec<(f64, usize)> = vec![(0., 0); edges.len()];
            while (end - r.position) * direction > 5. + r.speed.abs() * s.ramp_down_s(id) {
                duty = (s.applied(id) + 0.00008 * (direction * v - r.speed)).clamp(-0.6, 0.6);
                r = guard!(s.step(id, Some(duty)), "A", id, s, t0, n0, json!({}));
                if (r.speed - direction * v).abs() < 0.35 * v {
                    let k = ((r.position - (a.lower + inset)) / (a.span() - 2. * inset) * edges.len() as f64).floor();
                    if (0.0..edges.len() as f64).contains(&k) {
                        acc[k as usize].0 += duty;
                        acc[k as usize].1 += 1;
                    }
                }
                if s.samples.len() - n0 > 200_000 {
                    break;
                }
            }
            guard!(s.coast(id), "A", id, s, t0, n0, json!({}));
            for (k, (sum, count)) in acc.iter().enumerate() {
                if *count > 3 {
                    rows.push((v, direction, k, sum / *count as f64));
                }
            }
        }
    }
    let mut friction = Vec::new();
    let mut load = Vec::new();
    let slowest = speeds.iter().cloned().fold(f64::INFINITY, f64::min);
    for (k, x) in edges.iter().enumerate() {
        let up = rows.iter().find(|r| r.0 == slowest && r.1 > 0. && r.2 == k).map(|r| r.3);
        let down = rows.iter().find(|r| r.0 == slowest && r.1 < 0. && r.2 == k).map(|r| r.3);
        if let (Some(u), Some(d)) = (up, down) {
            friction.push((*x, 0.5 * (u - d)));
            load.push((*x, -0.5 * (u + d)));
        }
    }
    // Viscous: upward duty vs speed, averaged over bins.
    let by_speed: Vec<(f64, f64)> = speeds.iter().filter_map(|v| {
        let d: Vec<f64> = rows.iter().filter(|r| r.0 == *v && r.1 > 0.).map(|r| r.3).collect();
        (!d.is_empty()).then(|| (*v, d.iter().sum::<f64>() / d.len() as f64))
    }).collect();
    let viscous = linear_fit(&by_speed.iter().map(|p| p.0).collect::<Vec<_>>(), &by_speed.iter().map(|p| p.1).collect::<Vec<_>>());
    let mean_friction = friction.iter().map(|f| f.1).sum::<f64>() / friction.len().max(1) as f64;
    Ok(result("A", id, s, t0, n0, None, json!({
        "friction_duty_by_position": friction, "load_duty_by_position": load,
        "moving_friction_duty": mean_friction,
        "duty_per_count_s": viscous.map(|v| v.0), "speeds_counts_s": speeds,
    })))
}

/// Stage B: holds at positions, approached from both sides. Hold effort
/// mean = load, half-difference = static friction band. Self-locking: drift
/// with drive off. Optional known load gives compliance from the joint side.
pub fn stage_b(s: &mut Session, id: u8, positions: usize, known_load_duty: f64) -> R<StageResult> {
    let (t0, n0) = (s.rig.now(), s.samples.len());
    let a = s.axis(id)?;
    let mut rows = Vec::new();
    for x in bins(&a, s.gates.inset_counts * 2., positions) {
        let mut efforts = Vec::new();
        let mut offsets = Vec::new();
        for side in [-1., 1.] {
            guard!(s.move_to(id, x + side * 60., 150., 0.004), "B", id, s, t0, n0, json!({}));
            guard!(s.move_to(id, x, 60., 0.004), "B", id, s, t0, n0, json!({}));
            efforts.push(guard!(s.hold(id, x, 1.0, 0.004), "B", id, s, t0, n0, json!({})));
            let r = s.rig.read(id)?;
            offsets.push(r.joint.map(|j| r.position - j));
        }
        // Approaching from opposite sides takes up backlash in opposite senses.
        let backlash = match (offsets[0], offsets[1]) { (Some(a), Some(b)) => json!((a - b).abs()), _ => Value::Null };
        let before = s.rig.read(id)?.position;
        for _ in 0..(1.0 / s.rig.period()) as usize {
            guard!(s.step(id, Some(0.)), "B", id, s, t0, n0, json!({}));
        }
        let drift = s.rig.read(id)?.position - before;
        rows.push(json!({"position": x, "backlash_counts": backlash, "load_duty": -0.5 * (efforts[0] + efforts[1]), "static_band_duty": 0.5 * (efforts[1] - efforts[0]).abs(), "drift_off_counts": drift, "self_locking": drift.abs() <= 2.}));
    }
    // Compliance: joint-side deflection under a known extra load at the center.
    let mut compliance = Value::Null;
    if known_load_duty != 0. {
        let center = a.center();
        guard!(s.move_to(id, center, 100., 0.004), "B", id, s, t0, n0, json!({}));
        guard!(s.hold(id, center, 0.8, 0.004), "B", id, s, t0, n0, json!({}));
        let j0 = s.rig.read(id)?.joint;
        if s.rig.attach_load(id, known_load_duty).is_ok() {
            guard!(s.hold(id, center, 1.2, 0.004), "B", id, s, t0, n0, json!({}));
            let j1 = s.rig.read(id)?.joint;
            s.rig.attach_load(id, 0.)?;
            if let (Some(j0), Some(j1)) = (j0, j1) {
                compliance = json!((j0 - j1) / known_load_duty);
            }
        }
    }
    Ok(result("B", id, s, t0, n0, None, json!({"holds": rows, "compliance_counts_per_duty": compliance, "known_load_duty": known_load_duty})))
}

/// Stage C: braking. From the center, drive open-loop at increasing duty
/// until a target speed, then drive off; distance and deceleration per
/// direction. Speeds only rise while the stop still fits the band.
pub fn stage_c(s: &mut Session, id: u8, speeds: &[f64]) -> R<StageResult> {
    let (t0, n0) = (s.rig.now(), s.samples.len());
    let a = s.axis(id)?;
    let mut rows = Vec::new();
    let mut worst = [f64::INFINITY; 2];
    'levels: for &v in speeds {
        for direction in [1., -1.] {
            let start = if direction > 0. { a.lower + 0.3 * a.span() } else { a.upper - 0.3 * a.span() };
            guard!(s.move_to(id, start, 150., 0.004), "C", id, s, t0, n0, json!({"braking": rows}));
            let mut duty = 0.;
            let mut r = s.rig.read(id)?;
            let mut reached = false;
            for _ in 0..(3. / s.rig.period()) as usize {
                duty = (duty + 0.00012 * (direction * v - r.speed)).clamp(-1., 1.);
                r = guard!(s.step(id, Some(duty)), "C", id, s, t0, n0, json!({"braking": rows}));
                if (r.speed - direction * v).abs() < 0.1 * v {
                    reached = true;
                    break;
                }
                if s.stop_needed(id, &r, 0.05)? {
                    break;
                }
            }
            let (x0, v0) = (r.position, r.speed);
            let rest = guard!(s.coast(id), "C", id, s, t0, n0, json!({"braking": rows}));
            let distance = (rest.position - x0).abs().max(1.);
            let decel = v0 * v0 / (2. * distance);
            let d = usize::from(direction > 0.);
            worst[d] = worst[d].min(decel);
            rows.push(json!({"direction": direction, "target_counts_s": v, "speed_at_release": v0, "distance_counts": distance, "deceleration_counts_s2": decel, "reached": reached}));
            // Stop escalating if the next level's stop would not fit the band.
            if !reached {
                break 'levels;
            }
        }
    }
    for d in 0..2 {
        if worst[d].is_finite() {
            s.braking.insert((id, d), worst[d]);
        }
    }
    Ok(result("C", id, s, t0, n0, None, json!({"braking": rows, "deceleration_counts_s2": {"decreasing": worst[0], "increasing": worst[1]}})))
}

/// Stage D: constant-duty steps at several levels and positions, fitted as in
/// Tune; supply current and voltage give back-EMF, stall current and sag.
pub fn stage_d(s: &mut Session, id: u8, duties: &[f64]) -> R<StageResult> {
    use super::motor_identification::{StepTrace, design, fit_step_with};
    let (t0, n0) = (s.rig.now(), s.samples.len());
    let a = s.axis(id)?;
    let mut traces = Vec::new();
    let mut electrical = Vec::new();
    for &d in duties {
        for direction in [1., -1.] {
            let start = if direction > 0. { a.lower + 0.25 * a.span() } else { a.upper - 0.25 * a.span() };
            guard!(s.move_to(id, start, 150., 0.004), "D", id, s, t0, n0, json!({}));
            let begin = s.rig.now();
            let mut samples = Vec::new();
            let mut r = s.rig.read(id)?;
            samples.push((0., r.position as i32));
            let ticks = (0.8 / s.rig.period()) as usize;
            s.rehearse(id, &vec![direction * d; ticks])?;
            for _ in 0..ticks {
                r = guard!(s.step(id, Some(direction * d)), "D", id, s, t0, n0, json!({}));
                samples.push((r.t - begin, r.position as i32));
                if s.stop_needed(id, &r, 0.03)? {
                    break;
                }
            }
            s.end_segment();
            if let Some(i) = r.supply_current_a {
                electrical.push((d, r.speed.abs(), i, r.voltage_v));
            }
            guard!(s.coast(id), "D", id, s, t0, n0, json!({}));
            traces.push(StepTrace { duty: direction * d, samples });
        }
    }
    // Breakaway: slow ramp each way from the center.
    let mut breakaway = [0.; 2];
    for (k, direction) in [(0usize, -1.), (1usize, 1.)] {
        guard!(s.move_to(id, a.center(), 120., 0.004), "D", id, s, t0, n0, json!({}));
        guard!(s.hold(id, a.center(), 0.3, 0.004), "D", id, s, t0, n0, json!({}));
        let from = s.rig.read(id)?.position;
        let mut duty = 0.;
        loop {
            duty += 0.002;
            let r = guard!(s.step(id, Some(direction * duty)), "D", id, s, t0, n0, json!({}));
            if (r.position - from).abs() >= 3. || duty > 0.5 {
                breakaway[k] = duty;
                break;
            }
        }
        guard!(s.coast(id), "D", id, s, t0, n0, json!({}));
    }
    let fits: Vec<_> = traces.iter().filter_map(|t| fit_step_with(t, true)).collect();
    let tuning = design(&fits, breakaway, s.rig.period(), 0.08, "stage-D").ok();
    // Stall current per unit duty: current + back-EMF share; speed at full duty.
    let stall = electrical.iter().filter(|e| e.0 > 0.1).map(|e| e.2 / e.0.max(1e-6)).fold(0., f64::max);
    Ok(result("D", id, s, t0, n0, None, json!({
        "fits": fits, "steps_too_short": traces.len() - fits.len(), "traces": traces, "breakaway_duty": breakaway, "tuning": tuning,
        "electrical": electrical, "stall_current_estimate_a": stall,
        "minimum_voltage_v": s.samples[n0..].iter().map(|x| x.voltage_v).fold(f64::INFINITY, f64::min),
    })))
}

/// Stage E: small-signal sinusoidal duty around a PI hold at a few
/// frequencies; gain and phase of position per duty; fitted lag and delay.
pub fn stage_e(s: &mut Session, id: u8, frequencies: &[f64], amplitude: f64) -> R<StageResult> {
    let (t0, n0) = (s.rig.now(), s.samples.len());
    let a = s.axis(id)?;
    let center = a.center();
    guard!(s.move_to(id, center, 120., 0.004), "E", id, s, t0, n0, json!({}));
    let bias = guard!(s.hold(id, center, 0.6, 0.004), "E", id, s, t0, n0, json!({}));
    let mut rows = Vec::new();
    for &f in frequencies {
        // A weak position loop keeps the axis centred; the probe is the sine.
        let cycles = (f * 3.).clamp(3., 12.);
        let n = (cycles / f / s.rig.period()).ceil() as usize;
        let (mut c_in, mut s_in, mut c_out, mut s_out) = (0., 0., 0., 0.);
        let begin = s.rig.now();
        let mut r = s.rig.read(id)?;
        for _ in 0..n {
            let t = s.rig.now() - begin;
            let w = std::f64::consts::TAU * f * t;
            let probe = amplitude * w.sin();
            let duty = bias + probe + 0.0015 * (center - r.position);
            r = guard!(s.step(id, Some(duty)), "E", id, s, t0, n0, json!({"response": rows}));
            let t = r.t - begin;
            let w = std::f64::consts::TAU * f * t;
            // Input is the whole applied duty about the bias (probe plus the
            // centring feedback), so gain/phase are the plant's own.
            c_in += (duty - bias) * w.cos();
            s_in += (duty - bias) * w.sin();
            c_out += (r.position - center) * w.cos();
            s_out += (r.position - center) * w.sin();
        }
        let gain = (c_out.hypot(s_out)) / (c_in.hypot(s_in)).max(1e-12);
        // Correlating sin(ωt − φ) with (cos, sin) gives angle 90° + φ, so a lag
        // φ appears as out − in; report it as a negative phase.
        let phase = (s_in.atan2(c_in) - s_out.atan2(c_out)).to_degrees();
        let phase = ((phase + 540.) % 360.) - 180.;
        rows.push(json!({"frequency_hz": f, "gain_counts_per_duty": gain, "phase_deg": phase}));
    }
    // Fit phase ≈ −90° − atan(ωτ) − ωL over frequencies (grid search).
    let mut best = (f64::INFINITY, 0., 0.);
    for ti in 0..120 {
        let tau = 0.005 + ti as f64 * 0.005;
        for li in 0..40 {
            let lag = li as f64 * 0.0025;
            let err: f64 = rows.iter().map(|row| {
                let w = std::f64::consts::TAU * row["frequency_hz"].as_f64().unwrap();
                let model = -90. - (w * tau).atan().to_degrees() - (w * lag).to_degrees();
                let m = ((model + 540.) % 360.) - 180.;
                let d = row["phase_deg"].as_f64().unwrap() - m;
                (((d + 540.) % 360.) - 180.).powi(2)
            }).sum();
            if err < best.0 {
                best = (err, tau, lag);
            }
        }
    }
    let resonance = rows.windows(3).find(|w| {
        let g = |i: usize| w[i]["gain_counts_per_duty"].as_f64().unwrap() * w[i]["frequency_hz"].as_f64().unwrap();
        g(1) > 1.5 * g(0) && g(1) > 1.5 * g(2)
    }).map(|w| w[1]["frequency_hz"].clone());
    Ok(result("E", id, s, t0, n0, None, json!({"response": rows, "time_constant_s": best.1, "delay_s": best.2, "phase_rms_deg": (best.0 / rows.len().max(1) as f64).sqrt(), "resonance_hz": resonance,
        "fit_at_grid_edge": best.1 <= 0.005 || best.1 >= 0.6 || best.2 >= 0.0975, "amplitude_duty": amplitude})))
}

/// Stage F: servo position-mode steps; rise time, overshoot and settling,
/// fitted as a second-order response (natural frequency, damping).
pub fn stage_f(s: &mut Session, id: u8, step_counts: f64) -> R<StageResult> {
    let (t0, n0) = (s.rig.now(), s.samples.len());
    let a = s.axis(id)?;
    let center = a.center();
    guard!(s.move_to(id, center - step_counts / 2., 120., 0.004), "F", id, s, t0, n0, json!({}));
    s.rig.set_mode(id, ServoMode::Position)?;
    let mut rows = Vec::new();
    for (from, to) in [(center - step_counts / 2., center + step_counts / 2.), (center + step_counts / 2., center - step_counts / 2.)] {
        s.rig.goal(id, to, 0.)?;
        let begin = s.rig.now();
        let (mut t10, mut t90, mut peak) = (None, None, 0f64);
        let mut last_outside = 0.;
        for _ in 0..(1.5 / s.rig.period()) as usize {
            let r = guard!(s.step(id, None), "F", id, s, t0, n0, json!({"steps": rows}));
            let progress = (r.position - from) / (to - from);
            let t = r.t - begin;
            if t10.is_none() && progress >= 0.1 { t10 = Some(t); }
            if t90.is_none() && progress >= 0.9 { t90 = Some(t); }
            peak = peak.max(progress);
            if (r.position - to).abs() > 0.02 * (to - from).abs() { last_outside = t; }
        }
        let overshoot = (peak - 1.).max(0.);
        let zeta = if overshoot > 1e-4 { let l = overshoot.ln(); -l / (std::f64::consts::PI.powi(2) + l * l).sqrt() } else { 1. };
        let rise = t90.zip(t10).map(|(b, a)| b - a);
        let wn = rise.map(|r| (1.8 / r.max(1e-3)).min(200.));
        rows.push(json!({"from": from, "to": to, "rise_10_90_s": rise, "overshoot_fraction": overshoot, "settling_2pct_s": last_outside, "damping_ratio": zeta, "natural_frequency_rad_s": wn}));
    }
    s.rig.set_mode(id, ServoMode::Pwm)?;
    s.rig.drive(id, 0.)?;
    Ok(result("F", id, s, t0, n0, None, json!({"steps": rows})))
}

/// Stage G: effort ladder. From the middle, each direction, drive at rising
/// duty until the braking-limited stop point, then coast. Top speed and
/// initial acceleration per level; stops at the first failed gate.
pub fn stage_g(s: &mut Session, id: u8, duties: &[f64]) -> R<StageResult> {
    let (t0, n0) = (s.rig.now(), s.samples.len());
    let a = s.axis(id)?;
    let mut rows = Vec::new();
    for &d in duties {
        for direction in [1., -1.] {
            let start = if direction > 0. { a.lower + s.gates.inset_counts * 2. } else { a.upper - s.gates.inset_counts * 2. };
            guard!(s.move_to(id, start, 150., 0.004), "G", id, s, t0, n0, json!({"ladder": rows}));
            let home = s.rig.read(id)?.position;
            let begin = s.rig.now();
            let (mut top, mut accel) = (0f64, None);
            let ticks = (1.5 / s.rig.period()) as usize;
            s.rehearse(id, &vec![direction * d; ticks])?;
            for _ in 0..ticks {
                let r = guard!(s.step(id, Some(direction * d)), "G", id, s, t0, n0, json!({"ladder": rows}));
                top = top.max(r.speed.abs());
                if accel.is_none() && r.t - begin >= 0.05 {
                    accel = Some(r.speed.abs() / (r.t - begin));
                }
                if s.stop_needed(id, &r, 0.03)? {
                    break;
                }
            }
            s.end_segment();
            guard!(s.coast(id), "G", id, s, t0, n0, json!({"ladder": rows}));
            let back = guard!(s.move_to(id, home, 150., 0.004), "G", id, s, t0, n0, json!({"ladder": rows}));
            let drift = back.position - home;
            let min_v = s.samples[n0..].iter().map(|x| x.voltage_v).fold(f64::INFINITY, f64::min);
            let hot = s.rig.read(id)?.temperature_c;
            rows.push(json!({"duty": d, "direction": direction, "top_speed_counts_s": top, "acceleration_counts_s2": accel, "minimum_voltage_v": min_v, "temperature_c": hot, "return_error_counts": drift}));
            if drift.abs() > s.gates.max_drift_counts {
                return Ok(result("G", id, s, t0, n0, Some(Abort::Drift { counts: drift }), json!({"ladder": rows})));
            }
            if hot > s.gates.cool_down_to_c {
                s.cool_down(id, 600.)?;
            }
        }
    }
    let top = |dir: f64| rows.iter().filter(|r| r["direction"].as_f64() == Some(dir)).filter_map(|r| r["top_speed_counts_s"].as_f64()).fold(0., f64::max);
    let acc = |dir: f64| rows.iter().filter(|r| r["direction"].as_f64() == Some(dir)).filter_map(|r| r["acceleration_counts_s2"].as_f64()).fold(0., f64::max);
    Ok(result("G", id, s, t0, n0, None, json!({"ladder": rows,
        "top_speed_counts_s": {"increasing": top(1.), "decreasing": top(-1.)},
        "acceleration_counts_s2": {"increasing": acc(1.), "decreasing": acc(-1.)}})))
}

/// Stage H: move one axis in steps while the others hold; the others'
/// disturbance and the shared supply sag. `clear` vets joint combinations
/// (e.g. CAD self-collision) before any motion.
pub fn stage_h(s: &mut Session, ids: &[u8], clear: &dyn Fn(&[(u8, f64)]) -> R<()>) -> R<StageResult> {
    let (t0, n0) = (s.rig.now(), s.samples.len());
    let first = *ids.first().ok_or("no axes")?;
    let axes: Vec<Axis> = ids.iter().map(|i| s.axis(*i)).collect::<R<_>>()?;
    // Corners of the commanded box must be collision free.
    for mask in 0..(1u32 << axes.len()) {
        let combo: Vec<(u8, f64)> = axes.iter().enumerate().map(|(k, a)| (a.id, if mask & (1 << k) != 0 { a.upper - s.gates.inset_counts } else { a.lower + s.gates.inset_counts })).collect();
        if let Err(detail) = clear(&combo) {
            return Ok(result("H", 0, s, t0, n0, Some(Abort::Collision { detail }), json!({"combination": combo})));
        }
    }
    for a in &axes {
        guard!(s.move_to(a.id, a.center(), 150., 0.004), "H", first, s, t0, n0, json!({}));
    }
    // Slew-limited axes: softer position gain and a sine slow enough that
    // its peak acceleration stays within half the axis's cap.
    let limited = |id: u8| s.limits.get(&id).filter(|l| l.duty_slew_per_s.is_some()).cloned();
    let gain_of = |id: u8| if limited(id).is_some() { 0.0015 } else { 0.004 };
    let sine_hz = |a: &Axis| {
        let amplitude = 0.2 * a.span();
        match limited(a.id).and_then(|l| l.max_acceleration_counts_s2) {
            Some(cap) => ((0.5 * cap / amplitude).sqrt() / std::f64::consts::TAU).min(0.8),
            None => 0.8,
        }
    };
    let gains: std::collections::BTreeMap<u8, f64> = axes.iter().map(|a| (a.id, gain_of(a.id))).collect();
    let hz: std::collections::BTreeMap<u8, f64> = axes.iter().map(|a| (a.id, sine_hz(a))).collect();
    let mut rows = Vec::new();
    for mover in &axes {
        let holds: Vec<(u8, f64)> = axes.iter().filter(|a| a.id != mover.id).map(|a| (a.id, a.center())).collect();
        let mut disturbance = 0f64;
        let mut min_v = f64::INFINITY;
        let mut integral = std::collections::BTreeMap::<u8, f64>::new();
        for (k, target) in [mover.center() + 0.25 * mover.span(), mover.center() - 0.25 * mover.span(), mover.center()].iter().enumerate() {
            let _ = k;
            for _ in 0..(1.2 / s.rig.period()) as usize {
                // Holders: PI, one read each per period.
                for (hid, ht) in &holds {
                    let r = s.rig.read(*hid)?;
                    let e = ht - r.position;
                    let i = integral.entry(*hid).or_insert(0.);
                    *i = (*i + e * s.rig.period()).clamp(-200., 200.);
                    s.rig.drive(*hid, (gains[hid] * e + 0.006 * *i).clamp(-0.6, 0.6))?;
                    disturbance = disturbance.max(e.abs());
                }
                let r = s.rig.read(mover.id)?;
                let e = target - r.position;
                let duty = (gains[&mover.id] * e).clamp(-0.6, 0.6);
                let r = guard!(s.step(mover.id, Some(duty)), "H", first, s, t0, n0, json!({"coupling": rows}));
                min_v = min_v.min(r.voltage_v);
            }
        }
        rows.push(json!({"mover": mover.id, "max_holder_error_counts": disturbance, "minimum_voltage_v": min_v}));
    }
    // All together: simultaneous sines, combined sag.
    let mut min_v = f64::INFINITY;
    let begin = s.rig.now();
    for _ in 0..(2.0 / s.rig.period()) as usize {
        let t = s.rig.now() - begin;
        for (k, a) in axes.iter().enumerate().skip(1) {
            let target = a.center() + 0.2 * a.span() * (std::f64::consts::TAU * hz[&a.id] * t + k as f64).sin();
            let r = s.rig.read(a.id)?;
            s.rig.drive(a.id, (gains[&a.id] * (target - r.position)).clamp(-0.6, 0.6))?;
        }
        let a0 = &axes[0];
        let target = a0.center() + 0.2 * a0.span() * (std::f64::consts::TAU * hz[&a0.id] * t).sin();
        let r = s.rig.read(a0.id)?;
        let r = guard!(s.step(a0.id, Some((gains[&a0.id] * (target - r.position)).clamp(-0.6, 0.6))), "H", first, s, t0, n0, json!({"coupling": rows}));
        min_v = min_v.min(r.voltage_v);
    }
    for a in &axes {
        s.rig.drive(a.id, 0.)?;
    }
    Ok(result("H", 0, s, t0, n0, None, json!({"coupling": rows, "all_together_minimum_voltage_v": min_v, "preflight_v": s.preflight_v, "axes": ids})))
}

/// Stage I: replay a joint reference (counts vs time per axis) with PI plus
/// feed-forward tracking; tracking RMS and peak per axis.
pub fn stage_i(s: &mut Session, reference: &[(u8, Vec<(f64, f64)>)], speed_gain: f64) -> R<StageResult> {
    let (t0, n0) = (s.rig.now(), s.samples.len());
    let first = reference.first().map(|r| r.0).ok_or("empty reference")?;
    let sample = |pts: &[(f64, f64)], t: f64| -> (f64, f64) {
        let i = pts.partition_point(|p| p.0 <= t).clamp(1, pts.len() - 1);
        let (a, b) = (pts[i - 1], pts[i]);
        let u = ((t - a.0) / (b.0 - a.0)).clamp(0., 1.);
        (a.1 + u * (b.1 - a.1), (b.1 - a.1) / (b.0 - a.0))
    };
    for (id, pts) in reference {
        guard!(s.move_to(*id, pts[0].1, 150., 0.004), "I", first, s, t0, n0, json!({}));
    }
    let duration = reference.iter().map(|r| r.1.last().unwrap().0).fold(0., f64::max);
    let begin = s.rig.now();
    let mut errors: std::collections::BTreeMap<u8, Vec<f64>> = Default::default();
    while s.rig.now() - begin < duration {
        let t = s.rig.now() - begin;
        for (k, (id, pts)) in reference.iter().enumerate() {
            let (x, v) = sample(pts, t);
            let r = s.rig.read(*id)?;
            let e = x - r.position;
            errors.entry(*id).or_default().push(e);
            let duty = (v / speed_gain + 0.004 * e + 0.05 * v.signum() * (v.abs() > 1.) as i32 as f64).clamp(-1., 1.);
            if k + 1 == reference.len() {
                guard!(s.step(*id, Some(duty)), "I", first, s, t0, n0, json!({}));
            } else {
                s.rig.drive(*id, duty)?;
            }
        }
    }
    for (id, _) in reference {
        s.rig.drive(*id, 0.)?;
    }
    let rows: Vec<Value> = errors.iter().map(|(id, e)| {
        let rms = (e.iter().map(|x| x * x).sum::<f64>() / e.len() as f64).sqrt();
        let peak = e.iter().fold(0f64, |m, x| m.max(x.abs()));
        json!({"id": id, "rms_counts": rms, "peak_counts": peak})
    }).collect();
    Ok(result("I", 0, s, t0, n0, None, json!({"tracking": rows, "duration_s": duration, "first_axis": first})))
}

/// Stage J: duty cycling in the middle band until the temperature rise per
/// minute falls below `plateau_c_per_min` (or `max_s`); fits a first-order
/// rise: time constant, steady rise, thermal resistance from electrical power.
pub fn stage_j(s: &mut Session, id: u8, duty: f64, max_s: f64, plateau_c_per_min: f64) -> R<StageResult> {
    let (t0, n0) = (s.rig.now(), s.samples.len());
    let a = s.axis(id)?;
    guard!(s.move_to(id, a.center(), 150., 0.004), "J", id, s, t0, n0, json!({}));
    let start_temp = s.rig.read(id)?.temperature_c;
    let begin = s.rig.now();
    let mut history: Vec<(f64, f64, f64)> = Vec::new(); // t, temperature, power
    let mut direction = 1.;
    let mut r = s.rig.read(id)?;
    let saved = s.gates.max_temperature_c;
    let stopped_by;
    // Thermal test may reach the gate; it stops there, not above it.
    loop {
        // Reverse early enough for a slew-limited axis's ramp.
        if (r.position - a.center()).abs() + r.speed.abs() * s.ramp_down_s(id) > 0.2 * a.span() && (r.position - a.center()) * direction > 0. {
            direction = -(r.position - a.center()).signum();
        }
        r = match s.step(id, Some(direction * duty))? {
            Ok(r) => r,
            Err(Abort::Temperature { .. }) => { stopped_by = "temperature gate"; break }
            Err(e) => { s.gates.max_temperature_c = saved; return Ok(result("J", id, s, t0, n0, Some(e), json!({}))); }
        };
        let t = r.t - begin;
        let power = r.supply_current_a.map(|i| i * r.voltage_v).unwrap_or(f64::NAN);
        if history.last().is_none_or(|h| t - h.0 >= 1.) {
            history.push((t, r.temperature_c, power));
        }
        if t > max_s {
            stopped_by = "time limit";
            break;
        }
        // Plateau: within 90% of the fitted final rise and the last minute
        // rose less than `plateau_c_per_min`.
        if t > 60. && history.len() % 30 == 0 {
            let then = history.iter().rev().find(|h| t - h.0 >= 60.).map(|h| h.1).unwrap_or(r.temperature_c);
            let (_, _, rise_inf) = first_order_fit(&history, start_temp);
            if r.temperature_c - then < plateau_c_per_min && r.temperature_c - start_temp >= 0.9 * rise_inf {
                stopped_by = "plateau";
                break;
            }
        }
    }
    s.rig.drive(id, 0.)?;
    let end = history.last().map(|h| h.1).unwrap_or(start_temp);
    let rise = end - start_temp;
    let tau = history.iter().find(|h| h.1 - start_temp >= 0.632 * rise).map(|h| h.0);
    let first_order = first_order_fit(&history, start_temp);
    let power = history.iter().filter(|h| h.2.is_finite()).map(|h| h.2).sum::<f64>() / history.len().max(1) as f64;
    Ok(result("J", id, s, t0, n0, None, json!({
        "start_c": start_temp, "end_c": end, "rise_c": rise, "time_to_63pct_s": tau,
        "mean_supply_power_w": power, "stopped_by": stopped_by,
        "fitted_time_constant_s": first_order.1, "fitted_plateau_rise_c": first_order.2,
        "fit_rms_c": (first_order.0 / history.len().max(1) as f64).sqrt(),
        "history": history.iter().step_by(10).collect::<Vec<_>>(),
    })))
}

/// First-order heating fit T = T0 + ΔT∞·(1 − e^(−t/τ)) to (t, T, ·) history:
/// for each τ the best ΔT∞ is linear least squares; τ is picked by residual.
/// Works before the plateau. Returns (squared error, τ, ΔT∞).
fn first_order_fit(history: &[(f64, f64, f64)], start: f64) -> (f64, f64, f64) {
    let mut best = (f64::INFINITY, f64::NAN, f64::NAN);
    for k in 0..400 {
        let tau = 5. * 1.02f64.powi(k);
        let basis: Vec<f64> = history.iter().map(|h| 1. - (-h.0 / tau).exp()).collect();
        let bb: f64 = basis.iter().map(|b| b * b).sum();
        if bb <= 0. { continue; }
        let rise = basis.iter().zip(history).map(|(b, h)| b * (h.1 - start)).sum::<f64>() / bb;
        let err: f64 = basis.iter().zip(history).map(|(b, h)| (h.1 - start - rise * b).powi(2)).sum();
        if err < best.0 { best = (err, tau, rise); }
    }
    best
}

// ------------------------------------------------------ model and fitting ---

/// Predicted positions for a duty sequence with a bench motor model: the
/// rehearsal used for divergence checks.
pub fn model_predictor(model: super::virtual_bench::MotorModel) -> impl Fn(u8, &Reading, &[f64], f64) -> Vec<f64> {
    move |_id, r, duties, dt| {
        let mut servo = super::virtual_bench::Servo::new(r.position, model.clone());
        servo.speed = r.speed;
        servo.torque = true;
        duties.iter().map(|d| {
            servo.pwm = (d * 1000.).round() as i32;
            let n = (dt / 0.001).round().max(1.) as usize;
            for _ in 0..n {
                servo.step(dt / n as f64);
            }
            servo.position
        }).collect()
    }
}

/// A fitted quantity with uncertainty and where it came from.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Fitted {
    pub name: String,
    pub value: f64,
    pub unit: String,
    /// One-sigma-like spread: repeat difference and fit residual, or a
    /// declared default fraction when only one measurement exists.
    pub uncertainty: f64,
    pub source: String,
}

/// Declarative campaign plan (see PLAN.md). Absent stages are skipped.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub axes: Vec<Axis>,
    #[serde(default)]
    pub gates: Gates,
    #[serde(default)]
    pub a_speeds_counts_s: Vec<f64>,
    #[serde(default)]
    pub b_positions: usize,
    #[serde(default)]
    pub b_known_load_duty: f64,
    #[serde(default)]
    pub c_speeds_counts_s: Vec<f64>,
    #[serde(default)]
    pub d_duties: Vec<f64>,
    #[serde(default)]
    pub e_frequencies_hz: Vec<f64>,
    #[serde(default)]
    pub e_amplitude_duty: f64,
    #[serde(default)]
    pub f_step_counts: f64,
    #[serde(default)]
    pub g_duties: Vec<f64>,
    #[serde(default)]
    pub h_multi_axis: bool,
    /// Stage I reference: per axis, (time s, counts) points.
    #[serde(default)]
    pub i_reference: Vec<(u8, Vec<(f64, f64)>)>,
    /// Slow the replayed reference by this factor (≥ 1) for early passes.
    #[serde(default = "unit")]
    pub i_time_scale: f64,
    #[serde(default)]
    pub j_duty: f64,
    #[serde(default)]
    pub j_max_s: f64,
    #[serde(default)]
    pub k_repeat: bool,
    /// Relative importance of each fitted quantity to the gait search
    /// (name → weight) for test selection; unlisted weigh 0.3.
    #[serde(default)]
    pub sensitivity: std::collections::BTreeMap<String, f64>,
    /// Drive limits per axis role (e.g. "belt/hip"), for fragile transmissions.
    #[serde(default)]
    pub limits: std::collections::BTreeMap<String, AxisLimits>,
}

fn unit() -> f64 {
    1.
}
/// Everything a campaign produced for one rig.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub stages: Vec<StageResult>,
    pub fitted: Vec<(u8, Vec<Fitted>)>,
    pub simulated_s: f64,
    pub samples: Vec<Sample>,
}

fn metric<'a>(stages: &'a [StageResult], stage: &str, id: u8) -> Option<&'a Value> {
    stages.iter().rev().find(|s| s.stage == stage && s.id == id && s.completed).map(|s| &s.metrics)
}
fn metric_first<'a>(stages: &'a [StageResult], stage: &str, id: u8) -> Option<&'a Value> {
    stages.iter().find(|s| s.stage == stage && s.id == id && s.completed).map(|s| &s.metrics)
}

/// Fit an axis's quantities from its stage results. Repeated stages (K)
/// give the uncertainty; otherwise 25% (explicitly a default).
pub fn fit_axis(stages: &[StageResult], id: u8) -> Vec<Fitted> {
    let mut out = Vec::new();
    let mut push = |name: &str, unit: &str, source: &str, get: &dyn Fn(&Value) -> Option<f64>, stage: &str| {
        let (Some(last), Some(first)) = (metric(stages, stage, id).and_then(get), metric_first(stages, stage, id).and_then(get)) else { return };
        let repeated = stages.iter().filter(|s| s.stage == stage && s.id == id && s.completed).count() > 1;
        // Repeats give the spread; a 5% floor stands for resolution and model
        // error a deterministic repeat cannot show.
        let uncertainty = if repeated { ((last - first).abs() / 2.).max(0.05 * last.abs()) } else { 0.25 * last.abs() };
        out.push(Fitted { name: name.into(), value: 0.5 * (first + last), unit: unit.into(), uncertainty, source: source.into() });
    };
    push("speed_gain", "counts/s per duty", "D steps (slope between drive levels)", &|m| m["tuning"]["gain_counts_s_per_duty"].as_f64(), "D");
    push("time_constant", "s", "D steps (63% rise)", &|m| m["tuning"]["time_constant_s"].as_f64(), "D");
    push("moving_friction", "duty", "A sweeps (half up/down difference)", &|m| m["moving_friction_duty"].as_f64(), "A");
    push("breakaway", "duty", "D ramps (first 3-count motion)", &|m| Some(0.5 * (m["breakaway_duty"][0].as_f64()? + m["breakaway_duty"][1].as_f64()?)), "D");
    push("stall_current", "A per duty", "D supply current with back-EMF", &|m| m["stall_current_estimate_a"].as_f64().filter(|v| *v > 0.), "D");
    push("delay", "s", "E phase fit", &|m| m["delay_s"].as_f64(), "E");
    push("small_signal_time_constant", "s", "E phase fit", &|m| m["time_constant_s"].as_f64(), "E");
    push("servo_damping_ratio", "1", "F position-mode steps", &|m| m["steps"][0]["damping_ratio"].as_f64(), "F");
    push("servo_natural_frequency", "rad/s", "F position-mode steps", &|m| m["steps"][0]["natural_frequency_rad_s"].as_f64(), "F");
    push("braking_decreasing", "counts/s²", "C coast-down", &|m| m["deceleration_counts_s2"]["decreasing"].as_f64(), "C");
    push("braking_increasing", "counts/s²", "C coast-down", &|m| m["deceleration_counts_s2"]["increasing"].as_f64(), "C");
    push("top_speed_increasing", "counts/s", "G effort ladder", &|m| m["top_speed_counts_s"]["increasing"].as_f64(), "G");
    push("top_speed_decreasing", "counts/s", "G effort ladder", &|m| m["top_speed_counts_s"]["decreasing"].as_f64(), "G");
    push("acceleration_increasing", "counts/s²", "G effort ladder", &|m| m["acceleration_counts_s2"]["increasing"].as_f64(), "G");
    push("acceleration_decreasing", "counts/s²", "G effort ladder", &|m| m["acceleration_counts_s2"]["decreasing"].as_f64(), "G");
    push("compliance", "counts per duty", "B known load, joint side", &|m| m["compliance_counts_per_duty"].as_f64(), "B");
    push("backlash", "counts", "B two-sided approach, joint side", &|m| {
        let v: Vec<f64> = m["holds"].as_array()?.iter().filter_map(|h| h["backlash_counts"].as_f64()).collect();
        (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
    }, "B");
    // Gravity: load(x) = c + g·cos(θ) + h·sin(θ), fitted from A's load profile.
    if let Some(profile) = metric(stages, "A", id).and_then(|m| m["load_duty_by_position"].as_array()) {
        let pts: Vec<(f64, f64)> = profile.iter().filter_map(|p| Some((p[0].as_f64()?, p[1].as_f64()?))).collect();
        if pts.len() >= 3 {
            // Least squares on [1, cos, sin].
            let rows: Vec<[f64; 3]> = pts.iter().map(|(x, _)| { let t = x * RAD; [1., t.cos(), t.sin()] }).collect();
            let mut ata = [[0.; 3]; 3];
            let mut atb = [0.; 3];
            for (r, (_, y)) in rows.iter().zip(&pts) {
                for i in 0..3 { atb[i] += r[i] * y; for j in 0..3 { ata[i][j] += r[i] * r[j]; } }
            }
            if let Some(c) = solve3(ata, atb) {
                let amplitude = c[1].hypot(c[2]);
                let zero = c[2].atan2(c[1]) / RAD;
                let residual = (pts.iter().zip(&rows).map(|((_, y), r)| (y - c[0] - c[1] * r[1] - c[2] * r[2]).powi(2)).sum::<f64>() / pts.len() as f64).sqrt();
                out.push(Fitted { name: "gravity_amplitude".into(), value: amplitude, unit: "duty".into(), uncertainty: residual.max(0.1 * amplitude), source: "A load profile, cosine fit".into() });
                out.push(Fitted { name: "gravity_zero".into(), value: zero.rem_euclid(4096.), unit: "counts".into(), uncertainty: 64., source: "A load profile, cosine fit".into() });
                out.push(Fitted { name: "constant_load".into(), value: c[0], unit: "duty".into(), uncertainty: residual.max(0.005), source: "A load profile, cosine fit".into() });
            }
        }
    }
    if let Some(j) = metric(stages, "J", id) {
        let fitted = j["fitted_time_constant_s"].as_f64().zip(j["fitted_plateau_rise_c"].as_f64()).filter(|(t, r)| t.is_finite() && r.is_finite());
        if let (Some((tau, rise)), Some(power)) = (fitted, j["mean_supply_power_w"].as_f64()) {
            // Supply power includes every axis and mechanical output, so this
            // is a lower bound on the winding's own thermal resistance.
            let r_th = rise / power.max(1e-6);
            let rms = j["fit_rms_c"].as_f64().unwrap_or(0.);
            let stopped_early = j["stopped_by"].as_str() != Some("plateau");
            let spread = if stopped_early { 0.3 } else { 0.1 };
            out.push(Fitted { name: "thermal_resistance_lower_bound".into(), value: r_th, unit: "K/W".into(), uncertainty: 0.5 * r_th, source: "J first-order plateau rise / supply power (bound)".into() });
            out.push(Fitted { name: "thermal_time_constant".into(), value: tau, unit: "s".into(), uncertainty: (spread * tau).max(rms), source: format!("J first-order fit (stopped by {})", j["stopped_by"].as_str().unwrap_or("?")) });
        }
    }
    out
}
fn solve3(a: [[f64; 3]; 3], b: [f64; 3]) -> Option<[f64; 3]> {
    let det = |m: [[f64; 3]; 3]| m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let d = det(a);
    if d.abs() < 1e-12 { return None; }
    let mut x = [0.; 3];
    for i in 0..3 {
        let mut m = a;
        for r in 0..3 { m[r][i] = b[r]; }
        x[i] = det(m) / d;
    }
    Some(x)
}

/// A bench motor model from fitted quantities, starting from `prior`.
pub fn fitted_model(prior: &super::virtual_bench::MotorModel, fitted: &[Fitted]) -> super::virtual_bench::MotorModel {
    let get = |n: &str| fitted.iter().find(|f| f.name == n).map(|f| f.value);
    let mut m = prior.clone();
    if let Some(v) = get("speed_gain") { m.speed_gain = v; }
    if let Some(v) = get("time_constant").or(get("small_signal_time_constant")) { m.lag_s = v.max(0.005); }
    if let Some(v) = get("moving_friction") { m.moving_friction_duty = v.max(0.); }
    if let Some(v) = get("breakaway") { m.breakaway_duty = v.max(m.moving_friction_duty); }
    if let Some(v) = get("gravity_amplitude") { m.gravity_duty = v; }
    if let Some(v) = get("gravity_zero") { m.gravity_zero_counts = v; }
    if let Some(v) = get("constant_load") { m.load_duty = v; }
    if let Some(v) = get("compliance") { m.compliance_counts_per_duty = v; }
    if let Some(v) = get("backlash") { m.backlash_counts = v; }
    if let Some(v) = get("stall_current") { m.stall_current_a = v; }
    m
}

/// Replay every recorded open-loop segment (constant duty) through a model
/// and return the RMS position error (counts) against the measurement.
pub fn replay_error(samples: &[Sample], id: u8, model: &super::virtual_bench::MotorModel, dt: f64) -> f64 {
    let predictor = model_predictor(model.clone());
    let mine: Vec<&Sample> = samples.iter().filter(|s| s.id == id).collect();
    let (mut sum, mut n) = (0., 0usize);
    let mut k = 0;
    while k + 1 < mine.len() {
        let d = mine[k + 1].duty;
        let mut end = k + 1;
        while end < mine.len() && mine[end].duty == d && d.abs() > 0.1 {
            end += 1;
        }
        if end - k > 10 {
            let start = Reading { position: mine[k].position, speed: mine[k].speed, ..Default::default() };
            let duties: Vec<f64> = mine[k + 1..end].iter().map(|s| s.duty).collect();
            let predicted = predictor(id, &start, &duties, dt);
            for (p, m) in predicted.iter().zip(&mine[k + 1..end]) {
                sum += (p - m.position).powi(2);
                n += 1;
            }
            k = end;
        } else {
            k += 1;
        }
    }
    if n == 0 { f64::NAN } else { (sum / n as f64).sqrt() }
}

/// Run a plan on one rig. `model` rehearses open-loop segments (divergence
/// abort); `clear` vets multi-joint combinations. A failed stage stops later
/// escalation stages (C, D, E, G, H, I, J) on that axis.
pub fn run(plan: &Plan, rig: &mut dyn Rig, model: Option<super::virtual_bench::MotorModel>, clear: &dyn Fn(&[(u8, f64)]) -> R<()>, progress: &mut dyn FnMut(&str)) -> R<Report> {
    run_with(plan, rig, model.map(|m| Box::new(model_predictor(m)) as Predictor), clear, progress, &[], &mut |_| {})
}

/// Rehearsal model: predicted positions of axis `id` for a duty sequence from
/// a start reading, one per control period of `dt` s.
pub type Predictor<'a> = Box<dyn Fn(u8, &Reading, &[f64], f64) -> Vec<f64> + 'a>;

/// A predictor with one bench model per axis id (axes without a model are
/// not rehearsed: an empty prediction disables divergence checks there).
pub fn per_axis_predictor<'a>(models: std::collections::BTreeMap<u8, super::virtual_bench::MotorModel>) -> Predictor<'a> {
    let predictors: std::collections::BTreeMap<u8, _> = models.into_iter().map(|(id, m)| (id, model_predictor(m))).collect();
    Box::new(move |id, r, duties, dt| predictors.get(&id).map(|p| p(id, r, duties, dt)).unwrap_or_default())
}

/// [`run`], resumable: completed stage results in `resume` (receipts of an
/// earlier, interrupted run of the same plan) are reused instead of rerun
/// (K repeats always rerun). `receipt` receives each stage result as soon
/// as it finishes so a caller can store it before anything else happens.
pub fn run_with<'p>(
    plan: &Plan,
    rig: &'p mut dyn Rig,
    predictor: Option<Predictor<'p>>,
    clear: &dyn Fn(&[(u8, f64)]) -> R<()>,
    progress: &mut dyn FnMut(&str),
    resume: &[StageResult],
    receipt: &mut dyn FnMut(&StageResult),
) -> R<Report> {
    let reused = |stage: &str, id: u8| resume.iter().find(|r| r.stage == stage && r.id == id && r.completed).cloned();
    let t0 = rig.now();
    let limits: std::collections::BTreeMap<u8, AxisLimits> = plan.axes.iter().filter_map(|a| plan.limits.get(&a.role).map(|l| (a.id, l.clone()))).collect();
    let skipped = |name: &str, id: u8| limits.get(&id).is_some_and(|l| l.skip_stages.iter().any(|x| x == name));
    let mut limited = LimitedRig::new(rig, limits.clone());
    // Limited axes work inside a wider inset; the travel guard keeps the saved poses.
    let working: Vec<Axis> = plan.axes.iter().map(|a| {
        let extra = limits.get(&a.id).map_or(0., |l| l.extra_inset_counts.max(0.).min(0.25 * a.span()));
        Axis { lower: a.lower + extra, upper: a.upper - extra, ..a.clone() }
    }).collect();
    let mut s = Session::new(&mut limited, plan.gates.clone(), working)?;
    s.travel = plan.axes.iter().map(|a| (a.id, (a.lower, a.upper))).collect();
    s.limits = limits.clone();
    s.model = predictor;
    let mut stages: Vec<StageResult> = Vec::new();
    let ids: Vec<u8> = plan.axes.iter().map(|a| a.id).collect();
    let blocked = |stages: &[StageResult], id: u8| stages.iter().any(|r| r.id == id && !r.completed);
    for &id in &ids {
        let mut run_stage = |name: &str, s: &mut Session, stages: &mut Vec<StageResult>, f: &mut dyn FnMut(&mut Session) -> R<StageResult>| -> R<()> {
            if blocked(stages, id) && !matches!(name, "A" | "B") { return Ok(()); }
            if skipped(name, id) {
                progress(&format!("axis {id}: stage {name} skipped (axis limits)"));
                return Ok(());
            }
            if let Some(r) = reused(name, id) {
                progress(&format!("axis {id}: stage {name} (reused receipt)"));
                if let Some(b) = r.metrics["deceleration_counts_s2"].as_object() {
                    for (k, direction) in [("decreasing", 0usize), ("increasing", 1)] {
                        if let Some(v) = b.get(k).and_then(|v| v.as_f64()) { s.braking.insert((id, direction), v); }
                    }
                }
                stages.push(r);
                return Ok(());
            }
            // Preflight: start every stage at or below the cool-down temperature.
            if s.rig.read(id)?.temperature_c > s.gates.cool_down_to_c {
                progress(&format!("axis {id}: cooling before stage {name}"));
                s.cool_down(id, 900.)?;
            }
            progress(&format!("axis {id}: stage {name}"));
            let r = f(s)?;
            s.rig.stop()?;
            s.applied.clear();
            receipt(&r);
            stages.push(r);
            Ok(())
        };
        if !plan.a_speeds_counts_s.is_empty() { run_stage("A", &mut s, &mut stages, &mut |s| stage_a(s, id, &plan.a_speeds_counts_s))?; }
        if plan.b_positions > 0 { run_stage("B", &mut s, &mut stages, &mut |s| stage_b(s, id, plan.b_positions, plan.b_known_load_duty))?; }
        if !plan.c_speeds_counts_s.is_empty() { run_stage("C", &mut s, &mut stages, &mut |s| stage_c(s, id, &plan.c_speeds_counts_s))?; }
        if !plan.d_duties.is_empty() { run_stage("D", &mut s, &mut stages, &mut |s| stage_d(s, id, &plan.d_duties))?; }
        // The probe must exceed the breakaway measured in D, or the axis
        // sticks and the response is friction, not the plant.
        let breakaway = metric(&stages, "D", id).and_then(|m| m["breakaway_duty"].as_array().map(|b| b.iter().filter_map(|x| x.as_f64()).fold(0., f64::max))).unwrap_or(0.);
        let amplitude = plan.e_amplitude_duty.max(0.01).max(1.5 * breakaway);
        if !plan.e_frequencies_hz.is_empty() { run_stage("E", &mut s, &mut stages, &mut |s| stage_e(s, id, &plan.e_frequencies_hz, amplitude))?; }
        if plan.f_step_counts > 0. { run_stage("F", &mut s, &mut stages, &mut |s| stage_f(s, id, plan.f_step_counts))?; }
        if !plan.g_duties.is_empty() { run_stage("G", &mut s, &mut stages, &mut |s| stage_g(s, id, &plan.g_duties))?; }
        if plan.j_max_s > 0. { run_stage("J", &mut s, &mut stages, &mut |s| stage_j(s, id, plan.j_duty.max(0.1), plan.j_max_s, 0.5))?; }
    }
    if plan.h_multi_axis && ids.len() > 1 {
        if let Some(r) = reused("H", 0) {
            progress("multi-axis: stage H (reused receipt)");
            stages.push(r);
        } else {
            progress("multi-axis: stage H");
            let r = stage_h(&mut s, &ids, clear)?;
            s.rig.stop()?;
            s.applied.clear();
            receipt(&r);
            stages.push(r);
        }
    }
    if let (false, Some(r)) = (plan.i_reference.is_empty(), reused("I", 0)) {
        progress("gait replay: stage I (reused receipt)");
        stages.push(r);
    } else if !plan.i_reference.is_empty() {
        progress("gait replay: stage I");
        let gain = fit_axis(&stages, plan.i_reference[0].0).iter().find(|f| f.name == "speed_gain").map(|f| f.value).unwrap_or(3000.);
        let scale = plan.i_time_scale.max(1.);
        let slowed: Vec<(u8, Vec<(f64, f64)>)> = plan.i_reference.iter().map(|(id, pts)| (*id, pts.iter().map(|(t, x)| (t * scale, *x)).collect())).collect();
        let mut r = stage_i(&mut s, &slowed, gain)?;
        r.metrics["time_scale"] = json!(scale);
        s.rig.stop()?;
            s.applied.clear();
        receipt(&r);
        stages.push(r);
    }
    if plan.k_repeat {
        for &id in &ids {
            if blocked(&stages, id) { continue; }
            progress(&format!("axis {id}: stage K (repeat A, D, G)"));
            for (name, r) in [
                ("A", if plan.a_speeds_counts_s.is_empty() { None } else { Some(stage_a(&mut s, id, &plan.a_speeds_counts_s[..1])?) }),
                ("D", if plan.d_duties.is_empty() { None } else { Some(stage_d(&mut s, id, &plan.d_duties)?) }),
                ("G", if plan.g_duties.is_empty() { None } else { Some(stage_g(&mut s, id, &plan.g_duties)?) }),
            ] {
                if let Some(mut r) = r {
                    r.metrics["repeat_of"] = json!(name);
                    r.metrics["repeat"] = json!("K");
                    s.rig.stop()?;
            s.applied.clear();
                    receipt(&r);
                    stages.push(r);
                }
            }
        }
    }
    let fitted = ids.iter().map(|id| (*id, fit_axis(&stages, *id))).collect();
    let simulated_s = s.rig.now() - t0;
    Ok(Report { stages, fitted, simulated_s, samples: s.samples })
}

/// Rank fitted quantities by relative uncertainty × gait sensitivity and
/// name the stage that measures each; the top entries are where test time
/// should go next.
pub fn select_tests(report: &Report, sensitivity: &std::collections::BTreeMap<String, f64>) -> Vec<Value> {
    let mut rows: Vec<(f64, Value)> = report.fitted.iter().flat_map(|(id, fits)| fits.iter().map(move |f| {
        // Relative uncertainty, capped at 1 (a quantity indistinguishable from zero).
        let rel = (f.uncertainty / f.value.abs().max(1e-9)).min(1.);
        let w = sensitivity.get(&f.name).copied().unwrap_or(0.3);
        (rel * w, json!({"axis": id, "quantity": f.name, "relative_uncertainty": rel, "sensitivity": w, "score": rel * w, "measure_with": f.source}))
    })).collect();
    rows.sort_by(|a, b| b.0.total_cmp(&a.0));
    rows.into_iter().map(|r| r.1).collect()
}

/// Promotion outputs: output-shaft quantities with provenance next to the
/// profile's current estimates, and a gait-search screen/governor patch.
/// `coordinates` maps axis id to the CAD motor coordinates it represents.
pub fn promotion(report: &Report, prior: &Value, coordinates: &std::collections::BTreeMap<u8, Vec<String>>, provenance: &str) -> Value {
    let mut per_axis = Vec::new();
    let mut speed_limits = serde_json::Map::new();
    let (mut governor_speed, mut governor_accel) = (f64::INFINITY, f64::INFINITY);
    for (id, fits) in &report.fitted {
        let get = |n: &str| fits.iter().find(|f| f.name == n).map(|f| f.value);
        let no_load = get("speed_gain").map(|k| k * RAD);
        // Motor capability at full drive: gain beyond moving friction. The
        // speed reached in G is limited by the travel available and is
        // reported separately; it must not become a speed limit.
        let capability = get("speed_gain").map(|k| k * (1. - get("moving_friction").unwrap_or(0.)) * RAD);
        let reached = get("top_speed_increasing").zip(get("top_speed_decreasing")).map(|(a, b)| a.min(b) * RAD);
        let accel = get("acceleration_increasing").zip(get("acceleration_decreasing")).map(|(a, b)| a.min(b) * RAD);
        per_axis.push(json!({
            "axis": id,
            "measured": fits,
            "derived": {
                "no_load_output_speed_rad_s": no_load,
                "full_drive_speed_rad_s": capability,
                "top_speed_reached_in_travel_rad_s": reached,
                "measured_acceleration_rad_s2": accel,
            },
            "prior_estimates": {
                "no_load_speed_rad_s": prior["no_load_speed_rad_s"],
                "gear_friction_n_m": prior["gear_friction"], "backlash_rad": prior["backlash"], "latency_s": prior["latency"],
            },
            "provenance": provenance,
        }));
        if let Some(top) = capability {
            for c in coordinates.get(id).into_iter().flatten() {
                speed_limits.insert(c.clone(), json!(0.8 * top));
            }
            governor_speed = governor_speed.min(0.8 * top);
        }
        if let Some(a) = accel {
            governor_accel = governor_accel.min(0.5 * a);
        }
    }
    json!({
        "actuators": per_axis,
        "gait_search_patch": {
            "maximum_reference_speed_rad_s_by_coordinate": speed_limits,
            "governor_speed_rad_s_upper": governor_speed.is_finite().then_some(governor_speed),
            "governor_acceleration_rad_s2_upper": governor_accel.is_finite().then_some(governor_accel),
            "rule": "0.8 × full-drive speed (fitted gain beyond moving friction), not the travel-limited speed reached in G; 0.5 × measured acceleration; apply only after replay agreement and CAD promotion",
        },
    })
}
