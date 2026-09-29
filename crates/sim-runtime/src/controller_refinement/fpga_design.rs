//! Controller design on a captured timing schedule. These runs are simulations, not measurements.
use super::{calibration::Family, fpga};
use crate::physics_context::RuntimeIdentity;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Profile {
    pub amplitude_counts: f64,
    pub frequency_hz: f64,
    pub harmonic_ratio: f64,
    pub harmonic_fraction: f64,
    pub ramp_s: f64,
    pub phase_spread_turns: f64,
}
impl Default for Profile {
    fn default() -> Self {
        Self {
            amplitude_counts: 40.,
            frequency_hz: 0.3,
            harmonic_ratio: 2.3,
            harmonic_fraction: 0.3,
            ramp_s: 1.,
            phase_spread_turns: 1.,
        }
    }
}
impl Profile {
    pub fn apply(&self, template: &fpga::Plan) -> Result<fpga::Plan, String> {
        template.validate()?;
        let duration = (template.targets.len() - 1) as f64 * template.period_s;
        if [
            self.amplitude_counts,
            self.frequency_hz,
            self.harmonic_ratio,
            self.harmonic_fraction,
            self.ramp_s,
            self.phase_spread_turns,
        ]
        .iter()
        .any(|x| !x.is_finite())
            || !(0. ..=80.).contains(&self.amplitude_counts)
            || self.frequency_hz <= 0.
            || self.harmonic_ratio < 1.
            || self.frequency_hz * self.harmonic_ratio >= 0.5 / template.period_s
            || !(0. ..=1.).contains(&self.harmonic_fraction)
            || self.ramp_s <= 0.
            || self.ramp_s > duration / 2.
            || !(0. ..=1.).contains(&self.phase_spread_turns)
        {
            return Err("Profile needs finite bounded amplitude, resolvable frequencies and a ramp no longer than half the trajectory".into());
        }
        let smooth = |x: f64| sim_domain_control::smooth_return::rest_to_rest(x.clamp(0., 1.))[0];
        let mut plan = template.clone();
        for (tick, targets) in plan.targets.iter_mut().enumerate() {
            let t = tick as f64 * plan.period_s;
            let envelope = smooth(t / self.ramp_s).min(smooth((duration - t) / self.ramp_s));
            *targets = [0; 9];
            for (index, id) in plan.ids.iter().enumerate() {
                let phase = std::f64::consts::TAU * self.phase_spread_turns * index as f64
                    / plan.ids.len() as f64;
                let w = std::f64::consts::TAU * self.frequency_hz * t;
                let wave = (1. - self.harmonic_fraction) * (w + phase).sin()
                    + self.harmonic_fraction * (self.harmonic_ratio * w - phase).sin();
                targets[(*id - 4) as usize] =
                    (envelope * self.amplitude_counts * wave).round() as i16;
            }
        }
        plan.validate()?;
        Ok(plan)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Experiment {
    pub timing_recording_hash: String,
    pub plan: fpga::Plan,
}
impl Experiment {
    pub fn validate(&self, r: &fpga::Recording) -> Result<(), String> {
        r.validate()?;
        self.plan.validate()?;
        if self.timing_recording_hash != r.fingerprint()
            || self.plan.ids != r.plan.ids
            || self.plan.period_s != r.plan.period_s
            || self.plan.targets.len() != r.frames.len()
            || self.plan.bitstream_blake3 != r.plan.bitstream_blake3
            || self.plan.bitstream_path != r.plan.bitstream_path
            || self.plan.gains.limit > r.plan.gains.limit
        {
            return Err("Design must preserve the source timing, motors, firmware and commissioned duty ceiling".into());
        }
        self.plan.parameters(0, &r.home)?;
        for tick in 1..self.plan.targets.len() {
            self.plan.parameters(tick, &r.home)?;
        }
        Ok(())
    }
    /// Export only a plan; this function never opens hardware or loads firmware.
    pub fn export_new(&self, r: &fpga::Recording, path: &std::path::Path) -> Result<(), String> {
        self.validate(r)?;
        crate::experiment_study::write_new(
            path,
            &serde_json::to_vec_pretty(&self.plan).map_err(|e| e.to_string())?,
        )
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Tracking {
    pub rms_counts: f64,
    pub peak_counts: f64,
    pub saturated_fraction: f64,
    pub passes: bool,
}
fn tracking(
    plan: &fpga::Plan,
    id: u8,
    home: u16,
    samples: &[[f64; 4]],
) -> Result<Tracking, String> {
    if samples.len() != plan.targets.len() || samples.iter().flatten().any(|v| !v.is_finite()) {
        return Err("Incomplete or nonfinite design samples".into());
    }
    let axis = (id - 4) as usize;
    let errors: Vec<_> = samples
        .iter()
        .enumerate()
        .map(|(i, p)| p[1] - home as f64 - plan.targets[i.saturating_sub(1)][axis] as f64)
        .collect();
    let rms = (errors.iter().map(|e| e * e).sum::<f64>() / errors.len() as f64).sqrt();
    let peak = errors.iter().fold(0f64, |a, e| a.max(e.abs()));
    let saturated = samples
        .iter()
        .filter(|s| plan.gains.limit > 0 && s[2].abs() >= plan.gains.limit as f64)
        .count() as f64
        / samples.len() as f64;
    Ok(Tracking {
        rms_counts: rms,
        peak_counts: peak,
        saturated_fraction: saturated,
        passes: rms <= plan.rms_limit_counts && peak <= plan.peak_limit_counts,
    })
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Axis {
    pub id: u8,
    pub simulation: fpga::Simulation,
    pub tracking: Tracking,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Run {
    pub experiment: Experiment,
    pub models: std::collections::BTreeMap<u8, crate::experiment_study::ModelSettings>,
    pub axes: Vec<Axis>,
    pub failures: Vec<(u8, String)>,
    pub cancelled: bool,
    pub runtime: RuntimeIdentity,
    pub controller_ir_blake3: String,
}
impl Run {
    pub fn validate(&self, r: &fpga::Recording) -> Result<(), String> {
        self.experiment.validate(r)?;
        self.runtime.validate()?;
        if self.controller_ir_blake3.len() != 64
            || self.models.keys().copied().collect::<Vec<_>>() != r.plan.ids
        {
            return Err("Missing controller or per-motor model identity".into());
        }
        for model in self.models.values() {
            model.validate()?;
        }
        let mut ids = Vec::new();
        for a in &self.axes {
            if !r.plan.ids.contains(&a.id) {
                return Err("Unknown simulated motor".into());
            }
            let actual = tracking(
                &self.experiment.plan,
                a.id,
                r.home[(a.id - 4) as usize],
                &a.simulation.samples,
            )?;
            if actual != a.tracking {
                return Err("Design tracking score differs from saved samples".into());
            }
            for (sample, frame) in a.simulation.samples.iter().zip(&r.frames) {
                let o = frame.observations.iter().find(|o| o.id == a.id).unwrap();
                if (sample[0] - (o.request_s + o.completion_s) * 0.5).abs() > 1e-7
                    || !(0. ..=4095.).contains(&sample[1])
                    || sample[1].fract() != 0.
                    || sample[2].abs() > self.experiment.plan.gains.limit as f64
                {
                    return Err("Invalid design feedback or schedule".into());
                }
            }
            if !a.simulation.voltage_v.is_finite() || !a.simulation.temperature_c.is_finite() {
                return Err("Invalid simulated conditions".into());
            }
            for sample in &a.simulation.electrical {
                sample.validate()?;
            }
            ids.push(a.id);
        }
        for (id, error) in &self.failures {
            if error.is_empty() {
                return Err("Unexplained motor failure".into());
            }
            ids.push(*id);
        }
        ids.sort();
        if ids != r.plan.ids {
            return Err("Design must account for every motor exactly once".into());
        }
        Ok(())
    }
}
pub fn simulate(
    experiment: &Experiment,
    r: &fpga::Recording,
    family: &Family,
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize, usize),
) -> Result<Run, String> {
    experiment.validate(r)?;
    let models = r
        .plan
        .ids
        .iter()
        .map(|id| Ok((*id, family.model(*id)?)))
        .collect::<Result<_, String>>()?;
    let mut run = Run {
        experiment: experiment.clone(),
        models,
        axes: vec![],
        failures: vec![],
        cancelled: false,
        runtime: RuntimeIdentity::current(),
        controller_ir_blake3: blake3::hash(include_bytes!(
            "../../../sim-domain-control/src/fixed_pd.rs"
        ))
        .to_hex()
        .to_string(),
    };
    for (index, &id) in r.plan.ids.iter().enumerate() {
        let outcome = fpga::simulate_cancellable(
            r,
            id,
            &run.models[&id],
            true,
            Some(&experiment.plan),
            cancel,
            |n, _| progress(index * 1000 + n, r.plan.ids.len() * 1000),
        )
        .and_then(|simulation| {
            let tracking = tracking(
                &experiment.plan,
                id,
                r.home[(id - 4) as usize],
                &simulation.samples,
            )?;
            Ok(Axis {
                id,
                simulation,
                tracking,
            })
        });
        match outcome {
            Ok(axis) => run.axes.push(axis),
            Err(e) => run.failures.push((id, e)),
        }
    }
    run.cancelled = cancel.load(Ordering::Relaxed);
    run.validate(r)?;
    Ok(run)
}
