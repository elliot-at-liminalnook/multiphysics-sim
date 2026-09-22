//! Captured PWM controller experiments. All policies see sampled encoder feedback only.
use crate::{experiment_study::ModelSettings, physics_context::RuntimeIdentity};
use serde::{Deserialize, Serialize};
use sim_core::{Channel, Contract, Coupler, CouplerError, QuantityKind};
use sim_domain_control::pwm_feedback::{Action, EncoderEstimate, Pid, PidState};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Knot {
    pub time_s: f64,
    pub position_rad: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Timing {
    pub period_s: f64,
    pub encoder_quantum_rad: f64,
    pub observation_delay_ticks: usize,
    pub command_delay_ticks: usize,
    pub velocity_filter_s: f64,
    pub maximum_observation_age_s: f64,
    pub evidence: String,
}
impl Default for Timing {
    fn default() -> Self {
        Self{period_s:0.02,encoder_quantum_rad:std::f64::consts::TAU/4096.,observation_delay_ticks:1,command_delay_ticks:1,velocity_filter_s:0.04,maximum_observation_age_s:0.08,evidence:"Provisional 50 Hz schedule and one-tick delays; bench timing must be measured before hardware validation".into()}
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Policy {
    RustPid {
        parameters: Pid,
    },
    Rhai {
        source: String,
        parameters: serde_json::Value,
        duty_limit: f64,
    },
}
impl Default for Policy {
    fn default() -> Self {
        Self::RustPid {
            parameters: Pid::default(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Experiment {
    pub version: u32,
    pub name: String,
    pub device: u8,
    pub component_id: Option<String>,
    pub fixture: String,
    pub controller: Policy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub electrical: Option<super::power::Controller>,
    pub timing: Timing,
    pub trajectory: Vec<Knot>,
    pub duration_s: f64,
    pub voltage_v: f64,
    pub temperature_c: f64,
    pub initial_encoder_rad: f64,
    pub seed: u64,
    pub limits: TrackingLimits,
    pub notes: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrackingLimits {
    pub rms_rad: f64,
    pub peak_rad: f64,
    pub settled_rad: f64,
    pub maximum_saturation_fraction: f64,
}
impl Default for Experiment {
    fn default() -> Self {
        Self{version:1,name:"Unloaded PWM position tracking".into(),device:4,component_id:None,fixture:"Unloaded bench, attached output hardware not yet documented".into(),controller:Policy::default(),electrical:None,timing:Timing::default(),trajectory:vec![Knot{time_s:0.,position_rad:0.},Knot{time_s:0.2,position_rad:0.},Knot{time_s:0.5,position_rad:0.15},Knot{time_s:1.2,position_rad:0.15},Knot{time_s:1.5,position_rad:0.}],duration_s:2.,voltage_v:12.,temperature_c:25.,initial_encoder_rad:0.,seed:0,limits:TrackingLimits{rms_rad:0.05,peak_rad:0.15,settled_rad:0.015,maximum_saturation_fraction:0.25},notes:"Provisional demonstration limits; set task tolerances before reserving validation measurements.".into()}
    }
}
impl Experiment {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(e) = &self.electrical {
            e.validate()?;
        }
        let t = &self.timing;
        if self.version != 1
            || self.device == 0
            || self.device > 253
            || self.name.is_empty()
            || self.fixture.is_empty()
            || [
                self.duration_s,
                self.voltage_v,
                self.temperature_c,
                self.initial_encoder_rad,
                t.period_s,
                t.encoder_quantum_rad,
                t.velocity_filter_s,
                t.maximum_observation_age_s,
            ]
            .iter()
            .any(|v| !v.is_finite())
            || !(0.001..=1.).contains(&t.period_s)
            || !(t.period_s..=60.).contains(&self.duration_s)
            || self.duration_s / t.period_s > 100_000.
            || t.encoder_quantum_rad <= 0.
            || t.encoder_quantum_rad > std::f64::consts::TAU
            || t.velocity_filter_s < 0.
            || t.maximum_observation_age_s < t.period_s
            || t.observation_delay_ticks > 100
            || t.command_delay_ticks > 100
            || self.voltage_v <= 0.
            || self.temperature_c <= -273.15
            || t.evidence.is_empty()
        {
            return Err(
                "Invalid captured controller experiment, timing or physical conditions".into(),
            );
        }
        if self.trajectory.len() < 2
            || self.trajectory[0].time_s != 0.
            || self.trajectory.iter().any(|k| {
                !k.time_s.is_finite()
                    || !k.position_rad.is_finite()
                    || k.time_s < 0.
                    || k.time_s > self.duration_s
            })
            || self
                .trajectory
                .windows(2)
                .any(|w| w[0].time_s >= w[1].time_s)
        {
            return Err(
                "Trajectory requires ordered finite knots beginning at t=0 within the duration"
                    .into(),
            );
        }
        match &self.controller {
            Policy::RustPid { parameters } => parameters.validate()?,
            Policy::Rhai {
                source,
                parameters,
                duty_limit,
            } => {
                if source.is_empty()
                    || !parameters.is_object()
                    || !duty_limit.is_finite()
                    || !(0. ..=1.).contains(duty_limit)
                    || *duty_limit == 0.
                {
                    return Err(
                        "Rhai controller requires source, parameter object and finite duty limit"
                            .into(),
                    );
                }
            }
        }
        let l = &self.limits;
        if [
            l.rms_rad,
            l.peak_rad,
            l.settled_rad,
            l.maximum_saturation_fraction,
        ]
        .iter()
        .any(|v| !v.is_finite() || *v < 0.)
            || l.maximum_saturation_fraction > 1.
        {
            return Err("Invalid tracking limits".into());
        }
        Ok(())
    }
    pub fn fingerprint(&self) -> String {
        blake3::hash(&serde_json::to_vec(self).unwrap())
            .to_hex()
            .to_string()
    }
    pub fn target(&self, time: f64) -> f64 {
        for w in self.trajectory.windows(2) {
            if time <= w[1].time_s {
                let f = ((time - w[0].time_s) / (w[1].time_s - w[0].time_s)).clamp(0., 1.);
                return w[0].position_rad + f * (w[1].position_rad - w[0].position_rad);
            }
        }
        self.trajectory.last().unwrap().position_rad
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Feedback {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub electrical: Option<super::power::Observation>,
    pub observed_s: f64,
    pub received_s: f64,
    pub encoder_rad: f64,
    pub request_s: f64,
    pub completion_s: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ControlFrame {
    pub time_s: f64,
    pub observation: Feedback,
    pub target_rad: f64,
    pub estimated_position_rad: f64,
    pub estimated_velocity_rad_s: f64,
    pub requested_duty: f64,
    pub applied_duty: f64,
    pub saturated: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub electrical_limit_reasons: Vec<String>,
}
/// Both a hardware adapter and a simulation adapter call this exact state machine.
pub struct ControllerSession {
    experiment: Experiment,
    pid: PidState,
    estimator: EncoderEstimate,
    script: Option<sim_script::RhaiController>,
    previous_tick: Option<f64>,
    commands: VecDeque<f64>,
}
fn policy_contract(electrical: Option<&super::power::Controller>) -> Contract {
    let mut contract = Contract {
        element: "pwm_controller".into(),
        period: 0.,
        sensors: vec![
            Channel {
                name: "target".into(),
                kind: QuantityKind::Angle,
            },
            Channel {
                name: "position".into(),
                kind: QuantityKind::Angle,
            },
            Channel {
                name: "velocity".into(),
                kind: QuantityKind::AngularVelocity,
            },
        ],
        actuators: vec![Channel {
            name: "duty".into(),
            kind: QuantityKind::Dimensionless,
        }],
    };
    if let Some(e) = electrical {
        contract.sensors.push(Channel {
            name: "supply_voltage".into(),
            kind: QuantityKind::Voltage,
        });
        if e.sensing.supply_current_quantum_a.is_some() {
            contract.sensors.push(Channel {
                name: "supply_current".into(),
                kind: QuantityKind::Current,
            });
            contract.sensors.push(Channel {
                name: "supply_power".into(),
                kind: QuantityKind::Power,
            });
        }
        if e.sensing.winding_current_quantum_a.is_some() {
            contract.sensors.push(Channel {
                name: "winding_current".into(),
                kind: QuantityKind::Current,
            });
        }
    }
    contract
}
impl ControllerSession {
    pub fn new(experiment: Experiment) -> Result<Self, String> {
        experiment.validate()?;
        let mut script = match &experiment.controller {
            Policy::RustPid { .. } => None,
            Policy::Rhai {
                source, parameters, ..
            } => Some(
                sim_script::RhaiController::with_seed_and_registry(
                    sim_script::Sources::single("controller.rhai", source),
                    sim_script::parameter_map(parameters).map_err(|e| e.to_string())?,
                    experiment.seed,
                    &crate::registry(),
                )
                .map_err(|e| e.to_string())?,
            ),
        };
        if let Some(s) = &mut script {
            let mut c = policy_contract(experiment.electrical.as_ref());
            c.period = experiment.timing.period_s;
            s.open(&c).map_err(|e| e.to_string())?;
        }
        Ok(Self {
            experiment,
            pid: Default::default(),
            estimator: Default::default(),
            script,
            previous_tick: None,
            commands: VecDeque::new(),
        })
    }
    pub fn tick(&mut self, time: f64, observation: Feedback) -> Result<ControlFrame, String> {
        let c = &self.experiment;
        let timing = &c.timing;
        if [
            time,
            observation.observed_s,
            observation.received_s,
            observation.encoder_rad,
            observation.request_s,
            observation.completion_s,
        ]
        .iter()
        .any(|v| !v.is_finite())
            || observation.observed_s > observation.received_s
            || observation.received_s > time
            || observation.request_s > observation.completion_s
            || observation.completion_s > observation.received_s
            || time - observation.observed_s > timing.maximum_observation_age_s
            || self.previous_tick.is_some_and(|t| time <= t)
        {
            return Err(
                "Invalid, future, stale or unordered feedback/tick; no duty command issued".into(),
            );
        }
        let dt = self
            .previous_tick
            .map(|t| time - t)
            .unwrap_or(timing.period_s);
        let relative = (observation.encoder_rad - c.initial_encoder_rad + std::f64::consts::PI)
            .rem_euclid(std::f64::consts::TAU)
            - std::f64::consts::PI;
        let (position, velocity) = self.estimator.observe(
            observation.observed_s,
            relative,
            std::f64::consts::TAU,
            timing.velocity_filter_s,
        )?;
        let target = c.target(time);
        let (compensation, electrical_limit_reasons) =
            match (&c.electrical, &observation.electrical) {
                (None, None) => (1., vec![]),
                (Some(config), Some(observed)) => {
                    config.validate_observation(observed)?;
                    (
                        config
                            .nominal_voltage_for_compensation_v
                            .map(|v| v / observed.supply_voltage_v)
                            .unwrap_or(1.),
                        config.violations(observed)?,
                    )
                }
                _ => {
                    return Err(
                        "Feedback electrical channels differ from controller declaration".into(),
                    );
                }
            };
        let action = if !electrical_limit_reasons.is_empty() {
            Action {
                duty: 0.,
                unsaturated: 0.,
                saturated: true,
            }
        } else {
            match &c.controller {
                Policy::RustPid { parameters } => {
                    let mut adjusted = parameters.clone();
                    // Scale the voltage demand before saturation. Scaling the duty limit
                    // instead incorrectly reduces maximum output above nominal voltage.
                    adjusted.kp *= compensation;
                    adjusted.ki *= compensation;
                    adjusted.kd *= compensation;
                    self.pid.step(&adjusted, target, position, velocity, dt)?
                }
                Policy::Rhai { duty_limit, .. } => {
                    let mut a = [0.];
                    let mut inputs = vec![target, position, velocity];
                    if let Some(e) = &observation.electrical {
                        inputs.push(e.supply_voltage_v);
                        if let Some(i) = e.supply_current_a {
                            inputs.extend([i, e.supply_voltage_v * i]);
                        }
                        if let Some(i) = e.winding_current_a {
                            inputs.push(i);
                        }
                    }
                    self.script
                        .as_mut()
                        .unwrap()
                        .sample(time, &inputs, &mut a)
                        .map_err(|e| e.to_string())?;
                    if !a[0].is_finite() {
                        return Err("Nonfinite scripted command".into());
                    }
                    Action {
                        duty: (a[0] * compensation).clamp(-*duty_limit, *duty_limit),
                        unsaturated: a[0] * compensation,
                        saturated: (a[0] * compensation).abs() > *duty_limit,
                    }
                }
            }
        };
        if !action.duty.is_finite() || !action.unsaturated.is_finite() {
            return Err("Electrical compensation produced a nonfinite command".into());
        }
        self.previous_tick = Some(time);
        self.commands.push_back(action.duty);
        let applied = if self.commands.len() > timing.command_delay_ticks {
            self.commands.pop_front().unwrap()
        } else {
            0.
        };
        Ok(ControlFrame {
            time_s: time,
            observation,
            target_rad: target,
            estimated_position_rad: position,
            estimated_velocity_rad_s: velocity,
            requested_duty: action.duty,
            applied_duty: applied,
            saturated: action.saturated,
            electrical_limit_reasons,
        })
    }
}
struct SimulationAdapter {
    controller: ControllerSession,
    observations: VecDeque<Feedback>,
    frames: Arc<Mutex<Vec<ControlFrame>>>,
}
impl Coupler for SimulationAdapter {
    fn sample(
        &mut self,
        t: f64,
        sensors: &[f64],
        actuators: &mut [f64],
    ) -> Result<(), CouplerError> {
        let timing = &self.controller.experiment.timing;
        let q = timing.encoder_quantum_rad;
        let angle = ((sensors[0] + self.controller.experiment.initial_encoder_rad) / q).round() * q;
        self.observations.push_back(Feedback {
            electrical: self
                .controller
                .experiment
                .electrical
                .as_ref()
                .map(|e| e.observe_simulation(sensors))
                .transpose()
                .map_err(CouplerError::Other)?,
            observed_s: t,
            received_s: t,
            encoder_rad: angle,
            request_s: t,
            completion_s: t,
        });
        let mut observed = if self.observations.len() > timing.observation_delay_ticks {
            self.observations.pop_front().unwrap()
        } else {
            self.observations.front().unwrap().clone()
        };
        observed.received_s = t;
        let frame = self
            .controller
            .tick(t, observed)
            .map_err(CouplerError::Other)?;
        actuators[0] = frame.applied_duty;
        self.frames
            .lock()
            .map_err(|_| CouplerError::Other("frame collector poisoned".into()))?
            .push(frame);
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrackingScore {
    pub rms_rad: f64,
    pub peak_rad: f64,
    pub settled_error_rad: f64,
    pub saturation_fraction: f64,
    pub error_sign_changes: usize,
    pub passes: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Run {
    pub version: u32,
    pub experiment: Experiment,
    pub model: ModelSettings,
    pub runtime: RuntimeIdentity,
    pub frames: Vec<ControlFrame>,
    pub truth: Vec<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub electrical: Option<super::power::Trace>,
    pub score: Option<TrackingScore>,
    pub failure: Option<String>,
    pub cancelled: bool,
    pub evidence_kind: String,
}
/// Reproduce saved controller calculations without rerunning the physical plant.
pub fn validate_frames(experiment: &Experiment, frames: &[ControlFrame]) -> Result<(), String> {
    if frames.len() > 100_000 {
        return Err("Too many captured controller frames".into());
    }
    let mut controller = ControllerSession::new(experiment.clone())?;
    for f in frames {
        if f.time_s < 0. || f.time_s > experiment.duration_s + 1e-9 || f.observation.request_s < 0.
        {
            return Err("Controller frame lies outside the experiment".into());
        }
        let replay = controller.tick(f.time_s, f.observation.clone())?;
        if replay.electrical_limit_reasons != f.electrical_limit_reasons {
            return Err(
                "Saved electrical controller decisions differ from their observations".into(),
            );
        }
        for (a, b) in [
            (replay.target_rad, f.target_rad),
            (replay.estimated_position_rad, f.estimated_position_rad),
            (replay.estimated_velocity_rad_s, f.estimated_velocity_rad_s),
            (replay.requested_duty, f.requested_duty),
            (replay.applied_duty, f.applied_duty),
        ] {
            if !b.is_finite() || (a - b).abs() > 1e-10 {
                return Err("Saved controller calculations do not reproduce".into());
            }
        }
        if replay.saturated != f.saturated {
            return Err("Saved saturation differs from controller calculation".into());
        }
    }
    Ok(())
}
impl Run {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(trace) = &self.electrical {
            trace.validate()?;
            if trace.samples[0].time_s != 0.
                || trace.samples.last().unwrap().time_s > self.experiment.duration_s + 1e-9
                || (self.failure.is_none()
                    && !self.cancelled
                    && (trace.samples.last().unwrap().time_s - self.experiment.duration_s).abs()
                        > 1e-9)
            {
                return Err(
                    "Electrical evidence does not span the captured simulation interval".into(),
                );
            }

            if self
                .model
                .power
                .as_ref()
                .is_none_or(|p| p.limits != trace.limits)
            {
                return Err(
                    "Electrical trace requires its captured power scenario and limits".into(),
                );
            }
        } else if self.model.power.is_some() {
            return Err("Power-enabled controller run is missing electrical evidence".into());
        }

        self.experiment.validate()?;
        self.model.validate()?;
        self.runtime.validate()?;
        validate_frames(&self.experiment, &self.frames)?;
        if self.version != 1
            || self.evidence_kind != "simulation_only"
            || self.truth.is_empty()
            || self.truth[0] != [0., 0.]
            || self.truth.iter().any(|p| {
                p.iter().any(|v| !v.is_finite())
                    || p[0] < 0.
                    || p[0] > self.experiment.duration_s + 1e-9
            })
            || self.truth.windows(2).any(|w| w[0][0] >= w[1][0])
        {
            return Err("Invalid simulation trace or evidence identity".into());
        }
        let incomplete = self.cancelled || self.failure.is_some();
        if !incomplete
            && (self.frames.is_empty()
                || (self.truth.last().unwrap()[0] - self.experiment.duration_s).abs() > 1e-9)
        {
            return Err("Successful simulation must cover the captured duration".into());
        }
        let expected = if incomplete {
            None
        } else {
            score(&self.frames, &self.experiment.limits)
        };
        if expected != self.score {
            return Err("Controller score does not match its complete captured frames".into());
        }
        Ok(())
    }
}
pub fn score(frames: &[ControlFrame], limits: &TrackingLimits) -> Option<TrackingScore> {
    if frames.is_empty() {
        return None;
    }
    let errors = frames
        .iter()
        .map(|f| f.target_rad - f.estimated_position_rad)
        .collect::<Vec<_>>();
    let rms = (errors.iter().map(|e| e * e).sum::<f64>() / errors.len() as f64).sqrt();
    let peak = errors.iter().map(|e| e.abs()).fold(0., f64::max);
    let tail = (errors.len() / 10).max(1);
    let settled = errors[errors.len() - tail..]
        .iter()
        .map(|e| e.abs())
        .fold(0., f64::max);
    let saturation = frames.iter().filter(|f| f.saturated).count() as f64 / frames.len() as f64;
    Some(TrackingScore {
        rms_rad: rms,
        peak_rad: peak,
        settled_error_rad: settled,
        saturation_fraction: saturation,
        error_sign_changes: errors.windows(2).filter(|w| w[0] * w[1] < 0.).count(),
        passes: rms <= limits.rms_rad
            && peak <= limits.peak_rad
            && settled <= limits.settled_rad
            && saturation <= limits.maximum_saturation_fraction,
    })
}
pub fn simulate(
    experiment: &Experiment,
    model: &ModelSettings,
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize, usize),
) -> Result<Run, String> {
    experiment.validate()?;
    model.validate()?;
    if model.conditions.command_delay_s != 0.
        || model
            .conditions
            .voltage_v
            .is_some_and(|v| v != experiment.voltage_v)
        || model
            .conditions
            .temperature_c
            .is_some_and(|v| v != experiment.temperature_c)
    {
        return Err("Controller experiments capture their own timing and conditions; clear conflicting pulse-replay overrides before running".into());
    }
    if experiment.electrical.is_some() && model.power.is_none() {
        return Err(
            "Electrical controller feedback requires an explicit instrumented power source".into(),
        );
    }
    let controller = ControllerSession::new(experiment.clone())?;
    let frames = Arc::new(Mutex::new(Vec::new()));
    let mut bench = crate::actuator_bench::prepare(
        model,
        experiment.voltage_v,
        experiment.temperature_c,
        crate::actuator_bench::Drive::Feedback {
            period_s: experiment.timing.period_s,
        },
    )?;
    bench.runtime.seed(experiment.seed);
    bench
        .runtime
        .attach(
            bench.controller.unwrap(),
            Box::new(SimulationAdapter {
                controller,
                observations: VecDeque::new(),
                frames: frames.clone(),
            }),
        )
        .map_err(|e| e.to_string())?;
    let count = (experiment.duration_s / experiment.timing.period_s).ceil() as usize;
    let mut truth = vec![[0., 0.]];
    let mut electrical = bench.electrical_sample(0.).into_iter().collect::<Vec<_>>();
    let mut failure = None;
    let mut time = 0.;
    for i in 0..count {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let target = ((i + 1) as f64 * experiment.timing.period_s).min(experiment.duration_s);
        while time < target {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            let step = (target - time).min(if model.power.is_some() {
                model.step_s
            } else {
                0.002
            });
            if let Err(e) = bench.runtime.advance(step, model.step_s) {
                failure = Some(e.to_string());
                break;
            }
            time += step;
            if let Some(sample) = bench.electrical_sample(time) {
                let outside = sample
                    .state_of_charge
                    .is_some_and(|soc| !(0. ..=1.).contains(&soc));
                electrical.push(sample);
                if outside {
                    failure=Some("Battery depleted or overcharged beyond the model's validated state-of-charge range".into());
                    break;
                }
            }
        }
        if failure.is_some() || cancel.load(Ordering::Relaxed) {
            break;
        }
        truth.push([time, bench.runtime.get(bench.angle)]);
        progress(i + 1, count);
    }
    let frames = frames.lock().map_err(|e| e.to_string())?.clone();
    let cancelled = cancel.load(Ordering::Relaxed);
    let score = if failure.is_none() && !cancelled {
        score(&frames, &experiment.limits)
    } else {
        None
    };
    Ok(Run {
        version: 1,
        experiment: experiment.clone(),
        model: model.clone(),
        runtime: RuntimeIdentity::current(),
        frames,
        truth,
        electrical: if let Some(power) = &model.power {
            Some(super::power::Trace::new(electrical, power.limits.clone())?)
        } else {
            None
        },
        score,
        failure,
        cancelled,
        evidence_kind: "simulation_only".into(),
    })
}
