//! Three distinct questions: command response, controller calculations, closed-loop prediction.
use super::control::{self, ControlFrame, ControllerSession, Experiment, Feedback};
use crate::{
    actuator_bench::{self, Drive},
    experiment_comparison::{Comparison, Limits, Observation, Trace, compare},
    experiment_study::ModelSettings,
    physics_context::RuntimeIdentity,
};
use serde::{Deserialize, Serialize};
use sim_core::{Coupler, CouplerError, QuantityKind};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MeasuredFrame {
    pub control: ControlFrame,
    pub command_request_s: f64,
    pub command_receipt_s: f64,
    pub drive_counts: i16,
    pub voltage_v: f64,
    pub temperature_c: f64,
    pub current_raw_uncalibrated: u16,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Recording {
    pub version: u32,
    pub experiment: Experiment,
    pub runtime: RuntimeIdentity,
    pub frames: Vec<MeasuredFrame>,
    pub stop_request_s: f64,
    pub stop_receipt_s: f64,
    pub completed: bool,
    pub failure: Option<String>,
    pub stop_verified: bool,
    pub initial_registers: serde_json::Value,
    pub transactions_origin_host_s: f64,
    pub timing_evidence: String,
    pub source_hashes: std::collections::BTreeMap<String, String>,
}
impl Recording {
    pub fn fingerprint(&self) -> String {
        blake3::hash(&serde_json::to_vec(self).unwrap())
            .to_hex()
            .to_string()
    }
    pub fn validate(&self) -> Result<(), String> {
        self.experiment.validate()?;
        self.runtime.validate()?;
        if self.version != 1
            || (self.frames.is_empty() && (self.completed || self.failure.is_none()))
            || self.frames.len() > 100_000
            || self.timing_evidence.trim().is_empty()
            || self.source_hashes.is_empty()
        {
            return Err(
                "Recording requires frames, timing conventions, runtime and source identities"
                    .into(),
            );
        }
        if !self.stop_request_s.is_finite()
            || !self.stop_receipt_s.is_finite()
            || self.stop_request_s < 0.
            || self.stop_receipt_s < self.stop_request_s
            || self.stop_receipt_s > 65.
        {
            return Err("Invalid stop timing".into());
        }
        if self.completed && (self.failure.is_some() || !self.stop_verified) {
            return Err(
                "Completed hardware evidence requires verified stop and no execution failure"
                    .into(),
            );
        }
        if !self.transactions_origin_host_s.is_finite() || self.transactions_origin_host_s < 0. {
            return Err("Invalid hardware transaction clock origin".into());
        }
        control::validate_frames(
            &self.experiment,
            &self
                .frames
                .iter()
                .map(|f| f.control.clone())
                .collect::<Vec<_>>(),
        )?;
        let mut session = ControllerSession::new(self.experiment.clone())?;
        let mut previous_receipt = 0.;
        for frame in &self.frames {
            let f = &frame.control;
            let o = &f.observation;
            if [
                frame.command_request_s,
                frame.command_receipt_s,
                frame.voltage_v,
                frame.temperature_c,
            ]
            .iter()
            .any(|x| !x.is_finite())
                || o.request_s < previous_receipt
                || o.request_s < 0.
                || o.observed_s < o.request_s
                || o.observed_s > o.completion_s
                || frame.command_request_s < f.time_s
                || frame.command_receipt_s < frame.command_request_s
                || frame.command_receipt_s > self.stop_request_s
                || frame.voltage_v <= 0.
                || frame.temperature_c <= -273.15
                || frame.drive_counts.unsigned_abs() > 1000
            {
                return Err("Invalid command/observation transaction order or conditions".into());
            }
            let replay = session.tick(f.time_s, o.clone())?;
            for (a, b) in [
                (replay.requested_duty, f.requested_duty),
                (replay.applied_duty, f.applied_duty),
                (replay.estimated_position_rad, f.estimated_position_rad),
                (replay.estimated_velocity_rad_s, f.estimated_velocity_rad_s),
                (replay.target_rad, f.target_rad),
            ] {
                if !b.is_finite() || (a - b).abs() > 1e-10 {
                    return Err("Captured controller calculations do not reproduce from captured source/feedback".into());
                }
            }
            if (f.applied_duty * 1000.).round() as i16 != frame.drive_counts
                || f.saturated != replay.saturated
            {
                return Err("Captured command quantization or saturation differs from controller calculation".into());
            }
            previous_receipt = frame.command_receipt_s;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    RecordedCommandReplay,
    ClosedLoopPrediction,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Prediction {
    pub purpose: Purpose,
    pub recording_hash: String,
    pub model: ModelSettings,
    pub runtime: RuntimeIdentity,
    pub measured: Trace,
    pub predicted: Trace,
    pub model_error: Comparison,
    pub limits: Limits,
    pub simulated_frames: Vec<ControlFrame>,
    pub measured_tracking: Option<control::TrackingScore>,
    pub simulated_tracking: Option<control::TrackingScore>,
    pub assumptions: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub electrical: Option<super::power::Trace>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voltage_input: Option<VoltageInput>,
}
/// Measured voltage is an imposed actuator-test condition, never an independent
/// validation of a battery model. Preserve the source definition it replaced.
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct VoltageInput {
    pub measurements:super::electrical_measurements::Measurements,
    pub replaced_source:Option<super::power::Setup>,
}
fn voltage_parameters(r:&Recording,m:&super::electrical_measurements::Measurements)->Result<std::collections::BTreeMap<String,f64>,String>{
    m.validate_recording(r)?;
    let traces=m.traces()?;
    let v=traces.get("supply_voltage").ok_or("Measured voltage conditioning requires supply_voltage")?;
    if v.samples.iter().any(|s|s.value<=0.){return Err("Bench supply-voltage input must use positive V at the servo supply node".into());}
    let mut points=vec![];
    if v.samples[0].time_s>0. {points.push([0.,r.experiment.voltage_v]);}
    points.extend(v.samples.iter().map(|s|[s.time_s,s.value]));
    if points.last().unwrap()[0]<r.stop_receipt_s {points.push([r.stop_receipt_s,points.last().unwrap()[1]]);}
    let mut parameters=std::collections::BTreeMap::from([("count".into(),points.len() as f64)]);
    for (i,p) in points.iter().enumerate(){parameters.insert(format!("time.{i}"),p[0]);parameters.insert(format!("voltage.{i}"),p[1]);}
    sim_domain_electrical::voltage_history::VoltageHistory::from_parameters(&parameters).map_err(|e|e.to_string())?;
    Ok(parameters)
}
const VOLTAGE_INPUT_ASSUMPTIONS:&str="Measured voltage is imposed as an actuator-test input, not an independently predicted battery response. Linear interpolation at captured observation times; startup uses the captured initial voltage at t=0 when the first sample is later. The last measured voltage is explicitly held through the stop receipt. Sensor quantization, calibration and unknown sample age remain limitations. Controller feedback still comes from the independently simulated, quantized channels. Voltage agreement cannot be scored as model validation; current, power and motion are conditional on this voltage input.";
pub fn predict_with_voltage(r:&Recording,model:&ModelSettings,purpose:Purpose,limits:&Limits,measurements:&super::electrical_measurements::Measurements,cancel:&AtomicBool,progress:impl FnMut(usize,usize))->Result<Prediction,String>{
    let parameters=voltage_parameters(r,measurements)?;
    let mut resolved=model.clone();
    let original=resolved.power.clone();
    resolved.power=Some(super::power::Setup{source_component:sim_domain_electrical::voltage_history::VOLTAGE_HISTORY.into(),source_parameters:parameters,auxiliary_current_a:original.as_ref().map(|s|s.auxiliary_current_a).unwrap_or(0.),limits:original.as_ref().map(|s|s.limits.clone()).unwrap_or_default(),evidence:format!("Imposed voltage measurement {}. {} Auxiliary load {}.",measurements.fingerprint(),VOLTAGE_INPUT_ASSUMPTIONS,if original.is_some(){"retains the selected scenario"}else{"explicitly omitted for the legacy fixture"})});
    let mut prediction=predict(r,&resolved,purpose,limits,cancel,progress)?;
    prediction.voltage_input=Some(VoltageInput{measurements:measurements.clone(),replaced_source:original});
    prediction.assumptions.push_str(" ");prediction.assumptions.push_str(VOLTAGE_INPUT_ASSUMPTIONS);
    prediction.validate(r)?;Ok(prediction)
}
impl Recording {
    pub fn measured_trace(&self) -> Trace {
        Trace {
            quantity: QuantityKind::Angle.definition_id(),
            unit: "rad".into(),
            samples: self
                .frames
                .iter()
                .map(|f| Observation {
                    time_s: f.control.observation.observed_s,
                    value: f.control.estimated_position_rad,
                    request_s: f.control.observation.request_s,
                    completion_s: f.control.observation.completion_s,
                })
                .collect(),
        }
    }
}
impl Prediction {
    pub fn validate(&self, recording: &Recording) -> Result<(), String> {
        recording.validate()?;
        self.model.validate()?;
        self.runtime.validate()?;
        if self.recording_hash != recording.fingerprint()
            || !recording.completed
            || self.measured != recording.measured_trace()
            || self.assumptions.trim().is_empty()
        {
            return Err("Prediction must retain the exact completed source recording and timing assumptions".into());
        }
        if compare(&self.measured, &self.predicted, &self.limits)? != self.model_error {
            return Err("Saved hardware comparison does not match its traces".into());
        }
        match (&self.model.power, &self.electrical) {
            (Some(setup), Some(trace)) => {
                trace.validate()?;
                if trace.limits != setup.limits
                    || trace.samples[0].time_s != 0.
                    || trace.samples.last().unwrap().time_s
                        < self
                            .predicted
                            .samples
                            .last()
                            .ok_or("Missing prediction samples")?
                            .time_s
                {
                    return Err("Electrical prediction must cover its measured schedule and retain its limits".into());
                }
            }
            (None, None) => (),
            _ => return Err("Electrical prediction differs from captured power setup".into()),
        }
        if let Some(input)=&self.voltage_input {
            if let Some(original)=&input.replaced_source {original.validate()?;}
            let parameters=voltage_parameters(recording,&input.measurements)?;
            let source=self.model.power.as_ref().ok_or("Missing imposed voltage source")?;
            if source.auxiliary_current_a!=input.replaced_source.as_ref().map(|s|s.auxiliary_current_a).unwrap_or(0.) || source.limits!=input.replaced_source.as_ref().map(|s|s.limits.clone()).unwrap_or_default(){return Err("Imposed voltage changed the retained auxiliary load or electrical limits".into());}
            if source.source_component!=sim_domain_electrical::voltage_history::VOLTAGE_HISTORY || source.source_parameters!=parameters || !self.assumptions.contains(VOLTAGE_INPUT_ASSUMPTIONS) {
                return Err("Imposed voltage no longer matches its frozen measurements and boundary assumptions".into());
            }
        }
        let measured = recording
            .frames
            .iter()
            .map(|f| f.control.clone())
            .collect::<Vec<_>>();
        if control::score(&measured, &recording.experiment.limits) != self.measured_tracking {
            return Err("Measured tracking score differs from recording".into());
        }
        match self.purpose {
            Purpose::RecordedCommandReplay => {
                if !self.simulated_frames.is_empty() || self.simulated_tracking.is_some() {
                    return Err(
                        "Command replay cannot claim independent closed-loop tracking".into(),
                    );
                }
            }
            Purpose::ClosedLoopPrediction => {
                control::validate_frames(&recording.experiment, &self.simulated_frames)?;
                if self.simulated_frames.len() != recording.frames.len() {
                    return Err("Incomplete simulated feedback schedule".into());
                }
                for ((f, m), p) in self
                    .simulated_frames
                    .iter()
                    .zip(&recording.frames)
                    .zip(&self.predicted.samples)
                {
                    let mut expected = m.control.observation.clone();
                    expected.encoder_rad = f.observation.encoder_rad;
                    expected.electrical = f.observation.electrical.clone();
                    if f.time_s != m.control.time_s
                        || f.observation != expected
                        || (f.estimated_position_rad - p.value).abs() > 1e-10
                    {
                        return Err("Simulated feedback does not match captured timing and predicted observations".into());
                    }
                }
                if control::score(&self.simulated_frames, &recording.experiment.limits)
                    != self.simulated_tracking
                {
                    return Err("Simulated tracking score differs from captured frames".into());
                }
            }
        }
        Ok(())
    }
}
#[derive(Clone, Copy)]
enum Event {
    Observe(usize),
    Control(usize),
    Apply(usize),
}
struct Adapter {
    recording: Recording,
    purpose: Purpose,
    events: Vec<(f64, Vec<Event>)>,
    next: usize,
    session: ControllerSession,
    observations: Vec<Feedback>,
    commands: Vec<f64>,
    frames: Arc<Mutex<Vec<ControlFrame>>>,
    samples: Arc<Mutex<Vec<Observation>>>,
}
impl Coupler for Adapter {
    fn sample(
        &mut self,
        _t: f64,
        sensors: &[f64],
        actuators: &mut [f64],
    ) -> Result<(), CouplerError> {
        let (time, events) = self
            .events
            .get(self.next)
            .ok_or_else(|| CouplerError::Other("Unexpected recording event".into()))?
            .clone();
        for event in events {
            match event {
                Event::Observe(i) => {
                    let old = &self.recording.frames[i].control.observation;
                    let q = self.recording.experiment.timing.encoder_quantum_rad;
                    let absolute =
                        ((sensors[0] + self.recording.experiment.initial_encoder_rad) / q).round()
                            * q;
                    let mut feedback = old.clone();
                    feedback.encoder_rad = absolute.rem_euclid(std::f64::consts::TAU);
                    feedback.electrical = self
                        .recording
                        .experiment
                        .electrical
                        .as_ref()
                        .map(|e| e.observe_simulation(sensors))
                        .transpose()
                        .map_err(CouplerError::Other)?;
                    self.observations.push(feedback);
                    self.samples
                        .lock()
                        .map_err(|e| CouplerError::Other(e.to_string()))?
                        .push(Observation {
                            time_s: old.observed_s,
                            value: absolute - self.recording.experiment.initial_encoder_rad,
                            request_s: old.request_s,
                            completion_s: old.completion_s,
                        });
                }
                Event::Control(i) => {
                    if self.purpose == Purpose::ClosedLoopPrediction {
                        let frame = self
                            .session
                            .tick(time, self.observations[i].clone())
                            .map_err(CouplerError::Other)?;
                        self.commands
                            .push((frame.applied_duty * 1000.).round() / 1000.);
                        self.frames
                            .lock()
                            .map_err(|e| CouplerError::Other(e.to_string()))?
                            .push(frame);
                    }
                }
                Event::Apply(i) => {
                    actuators[0] = if self.purpose == Purpose::RecordedCommandReplay {
                        self.recording.frames[i].drive_counts as f64 / 1000.
                    } else {
                        self.commands[i]
                    };
                }
            }
        }
        self.next += 1;
        Ok(())
    }
}
pub fn predict(
    recording: &Recording,
    model: &ModelSettings,
    purpose: Purpose,
    limits: &Limits,
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize, usize),
) -> Result<Prediction, String> {
    recording.validate()?;
    model.validate()?;
    if !recording.completed || !recording.stop_verified {
        return Err(
            "Incomplete hardware trials are retained but cannot pass model validation".into(),
        );
    }
    if model.conditions.command_delay_s != 0.
        || model
            .conditions
            .voltage_v
            .is_some_and(|v| v != recording.experiment.voltage_v)
        || model
            .conditions
            .temperature_c
            .is_some_and(|v| v != recording.experiment.temperature_c)
    {
        return Err("Recorded schedule and conditions are authoritative; clear conflicting pulse-replay overrides".into());
    }
    if recording.experiment.electrical.is_some() && model.power.is_none() {
        return Err("Electrical feedback replay requires an instrumented power source".into());
    }
    let mut events = vec![];
    for (i, f) in recording.frames.iter().enumerate() {
        events.push((f.control.observation.observed_s, Event::Observe(i)));
        events.push((f.control.time_s, Event::Control(i)));
        events.push((
            (f.command_request_s + f.command_receipt_s) * 0.5,
            Event::Apply(i),
        ));
    }
    events.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut grouped: Vec<(f64, Vec<Event>)> = vec![];
    for (time, event) in events {
        if let Some(last) = grouped.last_mut().filter(|last| last.0 == time) {
            last.1.push(event);
        } else {
            grouped.push((time, vec![event]));
        }
    }
    let frames = Arc::new(Mutex::new(vec![]));
    let samples = Arc::new(Mutex::new(vec![]));
    let times = grouped.iter().map(|(t, _)| *t).collect::<Vec<_>>();
    let mut bench = actuator_bench::prepare(
        model,
        recording.experiment.voltage_v,
        recording.experiment.temperature_c,
        Drive::Scheduled {
            times: times.clone(),
        },
    )?;
    bench.runtime.seed(recording.experiment.seed);
    bench
        .runtime
        .attach(
            bench.controller.unwrap(),
            Box::new(Adapter {
                recording: recording.clone(),
                purpose,
                events: grouped,
                next: 0,
                session: ControllerSession::new(recording.experiment.clone())?,
                observations: vec![],
                commands: vec![],
                frames: frames.clone(),
                samples: samples.clone(),
            }),
        )
        .map_err(|e| e.to_string())?;
    let mut electrical = bench.electrical_sample(0.).into_iter().collect::<Vec<_>>();
    let mut time = 0.;
    for (i, target) in times.iter().enumerate() {
        while time < *target {
            if cancel.load(Ordering::Relaxed) {
                return Err("Cancelled".into());
            }
            let dt = (*target - time).min(if model.power.is_some() {
                model.step_s
            } else {
                0.002
            });
            bench
                .runtime
                .advance(dt, model.step_s)
                .map_err(|e| e.to_string())?;
            time += dt;
            if let Some(sample) = bench.electrical_sample(time) {
                sample.validate()?;
                if sample
                    .state_of_charge
                    .is_some_and(|v| !(0. ..=1.).contains(&v))
                {
                    return Err("Battery state of charge outside [0,1]; prediction cannot continue beyond depletion/overcharge".into());
                }
                electrical.push(sample);
            }
        }
        progress(i + 1, times.len());
    }
    // An event at t=0 is processed by the first advance; the final sample precedes its command.
    let measured = recording.measured_trace();
    let predicted = Trace {
        quantity: measured.quantity.clone(),
        unit: measured.unit.clone(),
        samples: samples.lock().map_err(|e| e.to_string())?.clone(),
    };
    let model_error = compare(&measured, &predicted, limits)?;
    let simulated_frames = frames.lock().map_err(|e| e.to_string())?.clone();
    let original_frames = recording
        .frames
        .iter()
        .map(|f| f.control.clone())
        .collect::<Vec<_>>();
    let electrical = model
        .power
        .as_ref()
        .map(|p| super::power::Trace::new(electrical, p.limits.clone()))
        .transpose()?;
    let source_assumption = model.power.as_ref().map(|p| {
        if p.source_component==sim_domain_electrical::voltage_history::VOLTAGE_HISTORY {format!("Source voltage is an imposed piecewise-linear history with held endpoints: {}. This does not independently predict supply/battery voltage.",p.evidence)}
        else {format!("Source uses declared {}: {}. Voltage response is predicted; measured voltage is not injected.",p.source_component,p.evidence)}
    }).unwrap_or_else(|| "Supply held at captured initial voltage; measured voltage variation is not injected.".into());
    Ok(Prediction {
        voltage_input:None,
        electrical,
        purpose,
        recording_hash: recording.fingerprint(),
        model: model.clone(),
        runtime: RuntimeIdentity::current(),
        measured,
        predicted,
        model_error,
        limits: limits.clone(),
        measured_tracking: control::score(&original_frames, &recording.experiment.limits),
        simulated_tracking: control::score(&simulated_frames, &recording.experiment.limits),
        simulated_frames,
        assumptions: format!(
            "Exact captured host observation/control/command schedule. Sensor sampling and command application are assumed at transaction midpoints; device sample age is unknown. Closed-loop prediction uses its own quantized simulated encoder feedback, the captured controller and command quantization. Recorded-command replay freezes measured commands. {source_assumption} Temperature is held at captured initial value. Proprietary firmware, winding heating and loaded-joint accuracy remain unmodeled. Startup is stationary with zero current. No fitted alignment or extrapolation."
        ),
    })
}
