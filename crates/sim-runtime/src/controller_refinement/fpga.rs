//! FPGA controller plans and evidence. The host schedules transactions, never computes drive.
use crate::acquisition::servo_bus::Telemetry;
use serde::{Deserialize, Serialize};
use sim_domain_control::fixed_pd::Gains;

/// Explicit physical stop scope; never infer a smaller scope from missing replies.
pub fn validate_physical_scope(scope: &[u8], active: &[u8]) -> Result<(), String> {
    if scope.is_empty()
        || scope.iter().any(|id| !(4..=12).contains(id))
        || scope.windows(2).any(|w| w[0] >= w[1])
        || active.is_empty()
        || active.windows(2).any(|w| w[0] >= w[1])
        || active.iter().any(|id| !scope.contains(id))
    {
        return Err(
            "Physical scope must be sorted unique IDs 4..12 containing every active motor".into(),
        );
    }
    Ok(())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub control: String,
    pub name: String,
    pub role: String,
    pub ids: Vec<u8>,
    pub period_s: f64,
    pub gains: Gains,
    /// Nine relative targets per tick, ID 4 first. Quantized before hardware execution.
    pub targets: Vec<[i16; 9]>,
    pub rms_limit_counts: f64,
    pub peak_limit_counts: f64,
    pub bitstream_path: String,
    pub bitstream_blake3: String,
}
impl Plan {
    pub fn validate(&self) -> Result<(), String> {
        self.validate_period_profile(false)
    }
    /// Offline qualification only. Acquisition continues to use `validate`.
    pub fn validate_offline_rate_study(&self) -> Result<(), String> {
        self.validate_period_profile(true)
    }
    fn validate_period_profile(&self, offline: bool) -> Result<(), String> {
        self.gains.validate()?;
        if !["fpga_pd", "fpga_device_pd"].contains(&self.control.as_str())
            || self.name.is_empty()
            || !["training", "validation", "timing"].contains(&self.role.as_str())
            || self.ids.is_empty()
            || self.ids.iter().any(|i| !(4..=12).contains(i))
            || self.ids.windows(2).any(|w| w[0] >= w[1])
            || !self.period_s.is_finite()
            || !(if self.control == "fpga_device_pd" {
                if self.ids.len() <= 3 { if offline { 0.0025 } else { 0.01 } } else { 0.04 }
            } else {
                0.05
            }..=0.15)
                .contains(&self.period_s)
            || self.targets.len() < 2
            || self.targets.len() as f64 * self.period_s > 12.
            || (self.gains.limit > 100
                && (self.control != "fpga_device_pd" || self.period_s > 0.04))
            || self.targets[0] != [0; 9]
            || self.targets.iter().flatten().any(|v| v.unsigned_abs() > 80)
            || self
                .targets
                .windows(2)
                .any(|w| (0..9).any(|i| (w[1][i] - w[0][i]).unsigned_abs() > 32))
            || !self.rms_limit_counts.is_finite()
            || self.rms_limit_counts <= 0.
            || !self.peak_limit_counts.is_finite()
            || self.peak_limit_counts < self.rms_limit_counts
            || self.bitstream_blake3.len() != 64
        {
            return Err("Invalid bounded FPGA controller plan".into());
        }
        Ok(())
    }
    pub fn parameters(&self, tick: usize, home: &[u16; 9]) -> Result<Vec<u8>, String> {
        self.validate()?;
        let target = self.targets.get(tick).ok_or("Tick outside plan")?;
        let mut bytes = Vec::new();
        let mask = self.ids.iter().fold(0u16, |m, i| m | (1 << (*i - 4)));
        for word in [
            mask,
            self.gains.kp_q8,
            self.gains.kd_q8,
            self.gains.kv_q8,
            self.gains.limit,
        ] {
            bytes.extend(word.to_le_bytes());
        }
        for i in 0..9 {
            let position = home[i] as i32 + target[i] as i32;
            if !(0..=4095).contains(&position) {
                return Err("Target crosses encoder boundary".into());
            }
            bytes.extend((position as u16).to_le_bytes());
            bytes.extend(
                (target[i]
                    - if tick == 0 {
                        0
                    } else {
                        self.targets[tick - 1][i]
                    })
                .to_le_bytes(),
            );
        }
        Ok(bytes)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Observation {
    pub id: u8,
    pub request_s: f64,
    pub completion_s: f64,
    pub telemetry: Telemetry,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Frame {
    pub tick: usize,
    pub observations: Vec<Observation>,
    pub command_request_s: f64,
    pub command_receipt_s: f64,
    pub pwm_readback: [i16; 9],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub torque_readback: Option<[u8; 9]>,
    pub arithmetic_matches: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Recording {
    pub version: u32,
    pub plan: Plan,
    pub home: [u16; 9],
    pub frames: Vec<Frame>,
    pub completed: bool,
    pub failure: Option<String>,
    pub stop_verified: bool,
    pub stop_request_s: f64,
    pub stop_receipt_s: f64,
    pub transactions_origin_host_s: f64,
    pub sources: std::collections::BTreeMap<String, String>,
    pub initial: serde_json::Value,
    pub recovery: serde_json::Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Dataset {
    pub recordings: Vec<Recording>,
    /// Explicit identification input. False preserves legacy mean/scenario
    /// voltage behavior and serialized dataset fingerprints.
    #[serde(default, skip_serializing_if = "voltage_not_conditioned")]
    pub measured_voltage: bool,
}
fn voltage_not_conditioned(value: &bool) -> bool { !value }
impl Recording {
    /// Preserve interrupted acquisitions without making them eligible for scoring.
    pub fn validate_capture(&self) -> Result<(), String> {
        self.plan.validate()?;
        if self.version != 1
            || self.frames.len() > self.plan.targets.len()
            || self.home.iter().any(|v| *v > 4095)
            || self.frames.iter().enumerate().any(|(i, f)| {
                f.tick != i
                    || !f.command_request_s.is_finite()
                    || !f.command_receipt_s.is_finite()
                    || f.observations.iter().any(|o| {
                        !self.plan.ids.contains(&o.id)
                            || !o.request_s.is_finite()
                            || !o.completion_s.is_finite()
                            || o.telemetry.position_raw > 4095
                            || !o.telemetry.voltage_v.is_finite()
                    })
            })
        {
            return Err("Malformed FPGA acquisition".into());
        }
        if self.completed {
            self.validate()?;
        }
        Ok(())
    }
    pub fn fingerprint(&self) -> String {
        blake3::hash(&serde_json::to_vec(self).expect("serializable recording"))
            .to_hex()
            .to_string()
    }
}
impl super::calibration_data::CalibrationData for Dataset {
    fn cases(&self) -> Result<Vec<super::calibration_data::Case>, String> {
        use crate::experiment_comparison::{Limits, Observation, Trace};
        let q = std::f64::consts::TAU / 4096.;
        let mut cases = Vec::new();
        for r in &self.recordings {
            r.validate()?;
            if r.plan.role == "timing" {
                return Err("Timing commissioning is not model fitting evidence".into());
            }
            for &id in &r.plan.ids {
                let axis = (id - 4) as usize;
                let samples = r
                    .frames
                    .iter()
                    .map(|f| {
                        let o = f.observations.iter().find(|o| o.id == id).unwrap();
                        Observation {
                            time_s: (o.request_s + o.completion_s) * 0.5,
                            value: (o.telemetry.position_raw as f64 - r.home[axis] as f64) * q,
                            request_s: o.request_s,
                            completion_s: o.completion_s,
                        }
                    })
                    .collect();
                cases.push(super::calibration_data::Case {
                    id: format!("{}/{id}", r.fingerprint()),
                    device: id,
                    split: if r.plan.role == "training" {
                        "train"
                    } else {
                        "held_out_fpga"
                    }
                    .into(),
                    measured: Trace {
                        quantity: sim_core::QuantityKind::Angle.definition_id(),
                        unit: "rad".into(),
                        samples,
                    },
                    limits: Limits {
                        rmse: 3. * q,
                        final_abs_error: 5. * q,
                    },
                    resolution: q,
                });
            }
        }
        Ok(cases)
    }
    fn predict(
        &self,
        id: &str,
        model: &crate::experiment_study::ModelSettings,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<crate::experiment_comparison::Trace, String> {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("Cancelled".into());
        }
        let (hash, axis) = id.split_once('/').ok_or("Invalid FPGA case ID")?;
        let device = axis.parse::<u8>().map_err(|e| e.to_string())?;
        let r = self
            .recordings
            .iter()
            .find(|r| r.fingerprint() == hash)
            .ok_or("Missing frozen FPGA recording")?;
        let resolved;
        let model = if self.measured_voltage {
            resolved = super::fpga_voltage::condition_model(r, device, model)?.0;
            &resolved
        } else { model };
        let p = predict_cancellable(r, device, model, false, cancel, |_, _| {})?;
        let mut trace = self
            .cases()?
            .into_iter()
            .find(|c| c.id == id)
            .ok_or("Unknown FPGA case")?
            .measured;
        let samples = p["samples_time_encoder_duty_angle"]
            .as_array()
            .ok_or("Missing prediction")?;
        for (o, p) in trace.samples.iter_mut().zip(samples) {
            o.value = p[3].as_f64().ok_or("Invalid predicted physical angle")?;
        }
        Ok(trace)
    }
    fn fingerprint(&self) -> String {
        blake3::hash(&serde_json::to_vec(self).expect("serializable dataset"))
            .to_hex()
            .to_string()
    }
}
impl Recording {
    pub fn validate(&self) -> Result<(), String> {
        self.plan.validate()?;
        if !self.completed
            || !self.stop_verified
            || self.failure.is_some()
            || self.frames.len() != self.plan.targets.len()
        {
            return Err("Completed stopped recording required for prediction".into());
        }
        let mut end = 0.;
        let mut previous = self.home;
        for (i, f) in self.frames.iter().enumerate() {
            if f.tick != i
                || !f.arithmetic_matches
                || !f.command_request_s.is_finite()
                || !f.command_receipt_s.is_finite()
                || f.command_request_s < end
                || f.command_receipt_s <= f.command_request_s
                || f.observations.len() != self.plan.ids.len()
            {
                return Err("Invalid FPGA frame ordering or arithmetic audit".into());
            }
            for (id, o) in self.plan.ids.iter().zip(&f.observations) {
                if f.torque_readback
                    .is_some_and(|v| v[(*id - 4) as usize] != 1)
                {
                    return Err("Torque disabled during captured controller frame".into());
                }
                if *id != o.id
                    || !o.request_s.is_finite()
                    || !o.completion_s.is_finite()
                    || o.request_s < end
                    || o.completion_s <= o.request_s
                    || o.completion_s > f.command_request_s
                {
                    return Err("Invalid feedback window".into());
                }
                let axis = (*id - 4) as usize;
                let delta = self.plan.targets[i][axis]
                    - if i == 0 {
                        0
                    } else {
                        self.plan.targets[i - 1][axis]
                    };
                let target = self.home[axis] as i32 + self.plan.targets[i][axis] as i32;
                if !(0..=4095).contains(&target) {
                    return Err("Target outside encoder range".into());
                }
                let expected = sim_domain_control::fixed_pd::step(
                    self.plan.gains,
                    target as u16,
                    o.telemetry.position_raw,
                    if i == 0 {
                        o.telemetry.position_raw
                    } else {
                        previous[axis]
                    },
                    delta,
                )?;
                if expected != f.pwm_readback[axis] {
                    return Err("Recorded PWM does not reproduce shared controller".into());
                }
                previous[axis] = o.telemetry.position_raw;
            }
            end = f.command_receipt_s;
        }
        if !self.stop_receipt_s.is_finite() || self.stop_receipt_s <= end {
            return Err("Invalid final stop timing".into());
        }
        Ok(())
    }
    pub fn scores(&self) -> serde_json::Value {
        let mut scores = serde_json::Map::new();
        for &id in &self.plan.ids {
            let axis = (id - 4) as usize;
            let mut errors = Vec::new();
            let mut volts = Vec::new();
            let mut temp = 0u8;
            let mut current = 0u16;
            let mut saturated = 0;
            let mut positions = Vec::new();
            for frame in &self.frames {
                if let Some(o) = frame.observations.iter().find(|o| o.id == id) {
                    // Observation precedes this tick's write: compare to the last applied setpoint.
                    let target = self.home[axis] as i32
                        + self.plan.targets[frame.tick.saturating_sub(1)][axis] as i32;
                    errors.push(o.telemetry.position_raw as f64 - target as f64);
                    positions.push(o.telemetry.position_raw);
                    volts.push(o.telemetry.voltage_v);
                    temp = temp.max(o.telemetry.temperature_c);
                    current = current.max(o.telemetry.current_raw);
                    saturated += usize::from(
                        self.plan.gains.limit > 0
                            && frame.pwm_readback[axis].unsigned_abs() == self.plan.gains.limit,
                    );
                }
            }
            if errors.is_empty() {
                continue;
            }
            let rms = (errors.iter().map(|x| x * x).sum::<f64>() / errors.len() as f64).sqrt();
            let peak = errors.iter().fold(0f64, |a, x| a.max(x.abs()));
            scores.insert(id.to_string(),serde_json::json!({"rms_counts":rms,"rms_degrees":rms*360./4096.,"peak_counts":peak,"peak_degrees":peak*360./4096.,"saturated_fraction":saturated as f64/errors.len() as f64,"voltage_min_v":volts.iter().copied().fold(f64::INFINITY,f64::min),"voltage_max_v":volts.iter().copied().fold(f64::NEG_INFINITY,f64::max),"max_temperature_c":temp,"max_current_raw_uncalibrated":current,"tracking_pass":self.completed&&rms<=self.plan.rms_limit_counts&&peak<=self.plan.peak_limit_counts,"encoder_span_counts":positions.iter().max().unwrap()-positions.iter().min().unwrap(),"positive_observed_travel_counts":positions.windows(2).map(|w|(w[1] as i32-w[0] as i32).max(0)).sum::<i32>(),"negative_observed_travel_counts":positions.windows(2).map(|w|(w[0] as i32-w[1] as i32).max(0)).sum::<i32>(),"torque_enable_audited_every_frame":self.frames.iter().all(|f|f.torque_readback.is_some_and(|v|v[axis]==1))}));
        }
        serde_json::json!({"motors":scores,"target_comparison":"Encoder observation versus previous applied discrete setpoint; target changes are held between FPGA batches. Position is internal encoder feedback, not an independent output shaft measurement.","electrical":"Servo voltage in 0.1 V counts; current remains uncalibrated raw counts. Measured amps, watts and battery validation unavailable."})
    }
}

/// Own-feedback prediction at the captured observation and command windows.
/// Uses the shared MotorUnit and integer law; never feeds measured positions to the simulation controller.
pub fn predict(
    recording: &Recording,
    id: u8,
    model: &crate::experiment_study::ModelSettings,
    closed_loop: bool,
) -> Result<serde_json::Value, String> {
    predict_cancellable(
        recording,
        id,
        model,
        closed_loop,
        &std::sync::atomic::AtomicBool::new(false),
        |_, _| {},
    )
}

pub fn predict_cancellable(
    recording: &Recording,
    id: u8,
    model: &crate::experiment_study::ModelSettings,
    closed_loop: bool,
    cancel: &std::sync::atomic::AtomicBool,
    progress: impl FnMut(usize, usize),
) -> Result<serde_json::Value, String> {
    let output = simulate_cancellable(recording, id, model, closed_loop, None, cancel, progress)?;
    let predicted = output.samples;
    let electrical = output.electrical;
    let voltage = output.voltage_v;
    let temperature = output.temperature_c;
    let observations = recording
        .frames
        .iter()
        .map(|f| f.observations.iter().find(|o| o.id == id).unwrap())
        .collect::<Vec<_>>();
    let errors = predicted
        .iter()
        .zip(&observations)
        .map(|(p, o)| p[1] - o.telemetry.position_raw as f64)
        .collect::<Vec<_>>();
    let rms = (errors.iter().map(|e| e * e).sum::<f64>() / errors.len() as f64).sqrt();
    Ok(
        serde_json::json!({"mode":if closed_loop{"own_feedback"}else{"recorded_pwm_replay"},"id":id,"model":model,"recording_blake3":blake3::hash(&serde_json::to_vec(recording).map_err(|e|e.to_string())?).to_hex().to_string(),"controller_ir_blake3":sim_domain_control::fixed_pd::implementation_identity(),"runtime":crate::physics_context::RuntimeIdentity::current(),"assumptions":"Sensor time approximated by captured request/reply midpoint; PWM applied at bridge receipt, not measured H-bridge switching time. Constant supply set to mean captured voltage unless model declares a power source. Independent single-axis plant: shared battery sag and wiring not validated. Encoder quantized to 4096 counts/turn.","voltage_v":voltage,"temperature_c":temperature,"rms_prediction_counts":rms,"rms_prediction_degrees":rms*360./4096.,"peak_prediction_counts":errors.iter().fold(0f64,|a,e|a.max(e.abs())),"prediction_limit_counts":3.,"prediction_pass":rms<=3.,"samples_time_encoder_duty_angle":predicted,"electrical":electrical}),
    )
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Simulation {
    pub samples: Vec<[f64; 4]>,
    pub electrical: Vec<super::power::Sample>,
    pub voltage_v: f64,
    pub temperature_c: f64,
}
/// Common execution adapter for captured-controller prediction and new controller design.
pub(crate) fn simulate_cancellable(
    recording: &Recording,
    id: u8,
    model: &crate::experiment_study::ModelSettings,
    closed_loop: bool,
    design_plan: Option<&Plan>,
    cancel: &std::sync::atomic::AtomicBool,
    mut progress: impl FnMut(usize, usize),
) -> Result<Simulation, String> {
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("Cancelled".into());
    }
    use std::sync::{Arc, Mutex};
    recording.validate()?;
    if let Some(plan) = design_plan {
        plan.validate()?;
        if !closed_loop
            || plan.ids != recording.plan.ids
            || plan.targets.len() != recording.frames.len()
            || plan.period_s != recording.plan.period_s
        {
            return Err("Controller design must retain the captured timing and motor set".into());
        }
    }
    if model.conditions.voltage_v.is_some()
        || model.conditions.temperature_c.is_some()
        || model.conditions.command_delay_s != 0.
    {
        return Err("FPGA prediction uses captured conditions and command windows; clear conflicting replay overrides".into());
    }
    let axis = (id.checked_sub(4).ok_or("Unknown ID")?) as usize;
    if !recording.plan.ids.contains(&id) {
        return Err("ID absent from recording".into());
    }
    let observations = recording
        .frames
        .iter()
        .map(|f| f.observations.iter().find(|o| o.id == id).unwrap())
        .collect::<Vec<_>>();
    let voltage = observations
        .iter()
        .map(|o| o.telemetry.voltage_v)
        .sum::<f64>()
        / observations.len() as f64;
    let temperature = observations[0].telemetry.temperature_c as f64;
    let mut times = Vec::new();
    for (f, o) in recording.frames.iter().zip(&observations) {
        times.push((o.request_s + o.completion_s) * 0.5);
        times.push(f.command_receipt_s);
    }
    times.push(recording.stop_receipt_s);
    let mut bench = crate::actuator_bench::prepare(
        model,
        voltage,
        temperature,
        crate::actuator_bench::Drive::Scheduled { times },
    )?;
    let predicted = Arc::new(Mutex::new(Vec::<[f64; 4]>::new()));
    let mut adapted_recording = recording.clone();
    if let Some(plan) = design_plan {
        adapted_recording.plan = plan.clone();
    }
    attach_controller(
        &mut bench.runtime,
        bench.controller.unwrap(),
        adapted_recording,
        axis,
        closed_loop,
        predicted.clone(),
    )?;
    let duration = recording.stop_receipt_s + model.step_s;
    let mut time = 0.;
    let mut electrical = Vec::new();
    while time < duration {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("Cancelled".into());
        }
        progress((time / duration * 1000.) as usize, 1000);
        let dt = (duration - time).min(model.step_s);
        bench
            .runtime
            .advance(dt, model.step_s)
            .map_err(|e| e.to_string())?;
        time += dt;
        if let Some(s) = bench.electrical_sample(time) {
            electrical.push(s);
        }
    }
    let predicted = predicted.lock().unwrap().clone();
    if predicted.len() != observations.len() {
        return Err("Incomplete simulated observation schedule".into());
    }
    Ok(Simulation {
        samples: predicted,
        electrical,
        voltage_v: voltage,
        temperature_c: temperature,
    })
}

/// Identical encoder quantization, integer controller and command timing for
/// independent and electrically coupled plants.
pub(super) fn attach_controller(
    runtime: &mut sim_compile::Runtime,
    controller: sim_core::BehaviorId,
    recording: Recording,
    axis: usize,
    closed_loop: bool,
    output: std::sync::Arc<std::sync::Mutex<Vec<[f64; 4]>>>,
) -> Result<(), String> {
    use sim_core::{Coupler, CouplerError};
    use std::sync::{Arc, Mutex};
    struct Adapter {
        r: Recording,
        axis: usize,
        event: usize,
        position: u16,
        previous: u16,
        closed_loop: bool,
        output: Arc<Mutex<Vec<[f64; 4]>>>,
    }
    impl Coupler for Adapter {
        fn sample(
            &mut self,
            t: f64,
            sensors: &[f64],
            actions: &mut [f64],
        ) -> Result<(), CouplerError> {
            if self.event >= self.r.frames.len() * 2 {
                actions[0] = 0.;
                self.event += 1;
                return Ok(());
            }
            let tick = self.event / 2;
            if self.event % 2 == 0 {
                let counts = (sensors[0] * 4096. / std::f64::consts::TAU
                    + self.r.home[self.axis] as f64)
                    .round();
                if !counts.is_finite() || !(0. ..=4095.).contains(&counts) {
                    return Err(CouplerError::Other(
                        "Simulation crossed encoder range".into(),
                    ));
                }
                self.position = counts as u16;
                if tick == 0 {
                    self.previous = self.position;
                }
                self.output
                    .lock()
                    .unwrap()
                    .push([t, counts, actions[0] * 1000., sensors[0]]);
            } else {
                let delta = self.r.plan.targets[tick][self.axis]
                    - if tick == 0 {
                        0
                    } else {
                        self.r.plan.targets[tick - 1][self.axis]
                    };
                let target = (self.r.home[self.axis] as i32
                    + self.r.plan.targets[tick][self.axis] as i32)
                    as u16;
                let duty = if self.closed_loop {
                    if (target as i32 - self.position as i32).abs() > 100
                        || (self.position as i32 - self.r.home[self.axis] as i32).abs() > 180
                    {
                        return Err(CouplerError::Other(
                            "Simulated controller exceeded FPGA tracking/travel supervision".into(),
                        ));
                    }
                    sim_domain_control::fixed_pd::step(
                        self.r.plan.gains,
                        target,
                        self.position,
                        self.previous,
                        delta,
                    )
                    .map_err(CouplerError::Other)?
                } else {
                    self.r.frames[tick].pwm_readback[self.axis]
                };
                actions[0] = duty as f64 / 1000.;
                self.previous = self.position;
            }
            self.event += 1;
            Ok(())
        }
    }
    let home = recording.home[axis];
    runtime
        .bind_coupler(
            controller,
            Box::new(Adapter {
                r: recording,
                axis,
                event: 0,
                position: home,
                previous: home,
                closed_loop,
                output,
            }), false,
        )
        .map_err(|e| e.to_string())
}
