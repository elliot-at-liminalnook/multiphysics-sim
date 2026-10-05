//! Offline controller experiments. Uses the shared physical bench and FPGA integer
//! law; these artifacts are never represented as physical acquisition recordings.
use crate::{actuator_bench, experiment_study::ModelSettings};
use serde::{Deserialize, Serialize};
use sim_core::{Coupler, CouplerError};
use sim_domain_control::fixed_pd::{self, Gains};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Experiment {
    pub name: String,
    pub model: ModelSettings,
    pub gains: Gains,
    pub targets_counts: Vec<i16>,
    pub period_s: f64,
    /// Delay from sampling the encoder to applying the computed command.
    pub latency_s: f64,
    pub voltage_v: f64,
    pub temperature_c: f64,
}
impl Experiment {
    pub fn validate(&self) -> Result<(), String> {
        self.model.validate()?;
        self.gains.validate_power2()?;
        if self.name.is_empty()
            || self.targets_counts.len() < 2
            || self.targets_counts.len() > 12000
            || self.targets_counts[0] != 0
            || self.targets_counts.iter().any(|x| x.unsigned_abs() > 1000)
            || !self.period_s.is_finite()
            || !(0.001..=0.15).contains(&self.period_s)
            || !self.latency_s.is_finite()
            || !(0. ..=0.05).contains(&self.latency_s)
            || !self.voltage_v.is_finite()
            || self.voltage_v <= 0.
            || !self.temperature_c.is_finite()
            || self.temperature_c <= -273.15
            || self.model.conditions.voltage_v.is_some()
            || self.model.conditions.temperature_c.is_some()
            || self.model.conditions.command_delay_s != 0.
        {
            return Err(
                "Invalid explicit offline controller experiment or conflicting conditions".into(),
            );
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sample {
    pub time_s: f64,
    pub desired_counts: i16,
    pub previously_applied_target_counts: i16,
    pub encoder_counts: i16,
    pub angle_rad: f64,
    pub applied_pwm: i16,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Metrics {
    /// Error against the desired signal at the actual observation time (no time shifting).
    pub rms_degrees: f64,
    pub peak_degrees: f64,
    pub previous_target_rms_degrees: f64,
    pub saturated_fraction: f64,
    pub final_error_degrees: f64,
    pub tail_encoder_span_degrees: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResultTrace {
    pub simulation_only: bool,
    pub experiment: Experiment,
    pub runtime: crate::physics_context::RuntimeIdentity,
    pub controller_ir_blake3: String,
    pub samples: Vec<Sample>,
    pub metrics: Metrics,
    pub electrical: Vec<super::power::Sample>,
}
#[derive(Clone, Copy)]
enum Event {
    Sample(usize),
    Apply(usize),
    Stop,
}
struct Adapter {
    experiment: Experiment,
    events: Vec<Event>,
    event: usize,
    previous: u16,
    commands: Vec<i16>,
    applied_target: i16,
    output: Arc<Mutex<Vec<Sample>>>,
}
impl Coupler for Adapter {
    fn sample(
        &mut self,
        time_s: f64,
        sensors: &[f64],
        actions: &mut [f64],
    ) -> Result<(), CouplerError> {
        match self.events[self.event] {
            Event::Sample(i) => {
                let count = (sensors[0] * 4096. / std::f64::consts::TAU + 2048.).round();
                if !count.is_finite() || !(0. ..=4095.).contains(&count) {
                    return Err(CouplerError::Other(
                        "Simulation crossed encoder range".into(),
                    ));
                }
                let target = self.experiment.targets_counts[i];
                self.output.lock().unwrap().push(Sample {
                    time_s,
                    desired_counts: target,
                    previously_applied_target_counts: self.applied_target,
                    encoder_counts: count as i16 - 2048,
                    angle_rad: sensors[0],
                    applied_pwm: (actions[0] * 1000.).round() as i16,
                });
                self.commands[i] = fixed_pd::step(
                    self.experiment.gains,
                    (2048 + target) as u16,
                    count as u16,
                    if i == 0 { count as u16 } else { self.previous },
                    target
                        - if i == 0 {
                            0
                        } else {
                            self.experiment.targets_counts[i - 1]
                        },
                )
                .map_err(CouplerError::Other)?;
                self.previous = count as u16;
            }
            Event::Apply(i) => {
                actions[0] = self.commands[i] as f64 / 1000.;
                self.applied_target = self.experiment.targets_counts[i];
            }
            Event::Stop => actions[0] = 0.,
        }
        self.event += 1;
        Ok(())
    }
}
fn schedule(experiment: &Experiment) -> (Vec<(f64, Event)>, f64) {
    // A tiny separation makes zero-delay sample/apply deterministic for the scheduled coupler.
    let latency = experiment.latency_s.max(1e-8);
    let n = experiment.targets_counts.len();
    let duration = n as f64 * experiment.period_s + latency;
    let mut schedule = Vec::new();
    for i in 0..n {
        let t = 1e-7 + i as f64 * experiment.period_s;
        schedule.push((t, Event::Sample(i)));
        schedule.push((t + latency, Event::Apply(i)));
    }
    schedule.push((duration, Event::Stop));
    schedule.sort_by(|a, b| {
        a.0.total_cmp(&b.0)
            .then_with(|| matches!(a.1, Event::Apply(_)).cmp(&matches!(b.1, Event::Apply(_))))
    });
    // Coincident delayed writes are ordered after the current observation.
    for i in 1..schedule.len() {
        if schedule[i].0 <= schedule[i - 1].0 {
            schedule[i].0 = schedule[i - 1].0 + 1e-8;
        }
    }
    (schedule, duration)
}
pub fn simulate(experiment: &Experiment, cancel: &AtomicBool) -> Result<ResultTrace, String> {
    experiment.validate()?;
    let (schedule, duration) = schedule(experiment);
    let n = experiment.targets_counts.len();
    let mut bench = actuator_bench::prepare(
        &experiment.model,
        experiment.voltage_v,
        experiment.temperature_c,
        actuator_bench::Drive::Scheduled {
            times: schedule.iter().map(|e| e.0).collect(),
        },
    )?;
    let output = Arc::new(Mutex::new(Vec::new()));
    bench
        .runtime
        .bind_coupler(
            bench.controller.unwrap(),
            Box::new(Adapter {
                experiment: experiment.clone(),
                events: schedule.iter().map(|e| e.1).collect(),
                event: 0,
                previous: 2048,
                commands: vec![0; n],
                applied_target: 0,
                output: output.clone(),
            }), false,
        )
        .map_err(|e| e.to_string())?;
    let mut time = 0.;
    let mut electrical = Vec::new();
    let end = duration + experiment.model.step_s;
    while time < end {
        if cancel.load(Ordering::Relaxed) {
            return Err("Cancelled".into());
        }
        let dt = (end - time).min(experiment.model.step_s);
        bench
            .runtime
            .advance(dt, experiment.model.step_s)
            .map_err(|e| e.to_string())?;
        time += dt;
        if let Some(s) = bench.electrical_sample(time) {
            electrical.push(s);
        }
    }
    finish(
        experiment,
        output.lock().unwrap().clone(),
        electrical,
        duration,
    )
}
fn finish(
    experiment: &Experiment,
    samples: Vec<Sample>,
    electrical: Vec<super::power::Sample>,
    duration: f64,
) -> Result<ResultTrace, String> {
    let n = experiment.targets_counts.len();
    if samples.len() != n {
        return Err("Incomplete simulated tracking trace".into());
    }
    let q = 360. / 4096.;
    let rms = |previous: bool| {
        (samples
            .iter()
            .map(|s| {
                let target = if previous {
                    s.previously_applied_target_counts
                } else {
                    s.desired_counts
                };
                ((s.encoder_counts - target) as f64).powi(2)
            })
            .sum::<f64>()
            / n as f64)
            .sqrt()
            * q
    };
    let tail = samples
        .iter()
        .filter(|s| s.time_s >= duration - 0.5)
        .map(|s| s.encoder_counts);
    let metrics = Metrics {
        rms_degrees: rms(false),
        previous_target_rms_degrees: rms(true),
        peak_degrees: samples
            .iter()
            .map(|s| (s.encoder_counts - s.desired_counts).unsigned_abs())
            .max()
            .unwrap() as f64
            * q,
        saturated_fraction: samples
            .iter()
            .filter(|s| {
                experiment.gains.limit > 0 && s.applied_pwm.unsigned_abs() >= experiment.gains.limit
            })
            .count() as f64
            / n as f64,
        final_error_degrees: (samples[n - 1].encoder_counts - samples[n - 1].desired_counts) as f64
            * q,
        tail_encoder_span_degrees: (tail.clone().max().unwrap() - tail.min().unwrap()) as f64 * q,
    };
    Ok(ResultTrace {
        simulation_only: true,
        experiment: experiment.clone(),
        runtime: crate::physics_context::RuntimeIdentity::current(),
        controller_ir_blake3: sim_domain_control::fixed_pd::implementation_identity(),
        samples,
        metrics,
        electrical,
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GroupResult {
    pub simulation_only: bool,
    pub setup: actuator_bench::GroupSetup,
    pub axes: Vec<ResultTrace>,
    /// Time, shared bus volts, source amps, source watts.
    pub supply_time_voltage_current_power: Vec<[f64; 4]>,
}
/// Simultaneous independent shafts sharing the actual library electrical circuit.
/// Resistances and source characteristics are explicit experimental assumptions.
pub fn simulate_group(
    experiments: &[Experiment],
    source: super::power::Setup,
    shared_resistance_ohm: f64,
    branch_resistance_ohm: &[f64],
    evidence: String,
    cancel: &AtomicBool,
) -> Result<GroupResult, String> {
    if experiments.len() != branch_resistance_ohm.len() || experiments.is_empty() {
        return Err("Declare a branch for every simulated motor".into());
    }
    let mut axes = Vec::new();
    let mut schedules = Vec::new();
    let mut duration = 0f64;
    let mut step = f64::INFINITY;
    for (i, e) in experiments.iter().enumerate() {
        e.validate()?;
        let (schedule, end) = schedule(e);
        duration = duration.max(end);
        step = step.min(e.model.step_s);
        axes.push(actuator_bench::AxisSetup {
            key: i.to_string(),
            model: e.model.clone(),
            temperature_c: e.temperature_c,
            command_and_sample_times_s: schedule.iter().map(|s| s.0).collect(),
            branch_resistance_ohm: branch_resistance_ohm[i],
        });
        schedules.push(schedule);
    }
    let setup = actuator_bench::GroupSetup {
        source,
        shared_resistance_ohm,
        axes,
        evidence,
        seed: 0,
    };
    let mut bench = actuator_bench::prepare_group(&setup)?;
    let mut outputs = Vec::new();
    for (i, e) in experiments.iter().enumerate() {
        let output = Arc::new(Mutex::new(Vec::new()));
        bench
            .runtime
            .bind_coupler(
                bench.axes[&i.to_string()].controller,
                Box::new(Adapter {
                    experiment: e.clone(),
                    events: schedules[i].iter().map(|s| s.1).collect(),
                    event: 0,
                    previous: 2048,
                    commands: vec![0; e.targets_counts.len()],
                    applied_target: 0,
                    output: output.clone(),
                }), false,
            )
            .map_err(|e| e.to_string())?;
        outputs.push(output);
    }
    let mut time = 0.;
    let mut supply = Vec::new();
    let mut electrical = vec![Vec::new(); experiments.len()];
    while time < duration + step {
        if cancel.load(Ordering::Relaxed) {
            return Err("Cancelled".into());
        }
        let dt = (duration + step - time).min(step);
        bench.runtime.advance(dt, step).map_err(|e| e.to_string())?;
        time += dt;
        let [v, i, p] = bench.supply();
        supply.push([time, v, i, p]);
        for (axis, samples) in electrical.iter_mut().enumerate() {
            samples.push(bench.electrical_sample(&axis.to_string(), time).unwrap());
        }
    }
    let mut results = Vec::new();
    for (i, samples) in electrical.into_iter().enumerate() {
        results.push(finish(
            &experiments[i],
            outputs[i].lock().unwrap().clone(),
            samples,
            schedule(&experiments[i]).1,
        )?);
    }
    Ok(GroupResult {
        simulation_only: true,
        setup,
        axes: results,
        supply_time_voltage_current_power: supply,
    })
}
