//! Calibrated electrical sidecars keep raw motor recordings immutable.
//! Power requires voltage and current measured at the same circuit and sample windows.
use super::{power, recording};
use crate::experiment_comparison::{self as comparison, Limits, Observation, Trace};
use serde::{Deserialize, Serialize};
use sim_core::QuantityKind;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Calibration {
    pub sensor: String,
    pub circuit_location: String,
    pub raw_unit: String,
    /// canonical value = raw * gain + offset; gain includes the signed polarity.
    pub gain: f64,
    pub offset: f64,
    pub evidence: String,
    pub uncertainty: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Channel {
    /// supply_voltage, supply_current, winding_voltage or winding_current.
    pub name: String,
    pub calibration: Calibration,
    pub raw_samples: Vec<Observation>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Measurements {
    pub version: u32,
    pub recording_hash: String,
    pub source_hashes: BTreeMap<String, String>,
    pub timing_evidence: String,
    pub channels: Vec<Channel>,
    /// Acceptance thresholds are declared in canonical V, A or W. Missing is unscored.
    pub limits: BTreeMap<String, Limits>,
}
fn quantity(name: &str) -> Result<(QuantityKind, &'static str), String> {
    match name {
        "supply_voltage" | "winding_voltage" => Ok((QuantityKind::Voltage, "V")),
        "supply_current" | "winding_current" => Ok((QuantityKind::Current, "A")),
        "supply_power" | "winding_power" => Ok((QuantityKind::Power, "W")),
        _ => Err(format!("Unknown electrical circuit channel {name}")),
    }
}
fn validate_trace(t: &Trace) -> Result<(), String> {
    comparison::compare(
        t,
        t,
        &Limits {
            rmse: 0.,
            final_abs_error: 0.,
        },
    )?;
    if t.samples.len() < 2 || t.samples[0].request_s < 0. {
        return Err("Electrical channels need at least two nonnegative sample windows".into());
    }
    Ok(())
}
impl Measurements {
    pub fn fingerprint(&self) -> String {
        blake3::hash(&serde_json::to_vec(self).unwrap())
            .to_hex()
            .to_string()
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || self.recording_hash.len() != 64
            || self.source_hashes.is_empty()
            || self.source_hashes.iter().any(|(k, v)| {
                k.trim().is_empty() || v.len() != 64 || !v.bytes().all(|c| c.is_ascii_hexdigit())
            })
            || self.timing_evidence.trim().is_empty()
            || self.channels.is_empty()
        {
            return Err(
                "Electrical measurements need recording/source identities and clock evidence"
                    .into(),
            );
        }
        let traces = self.traces()?;
        for (name, limit) in &self.limits {
            let t = traces
                .get(name)
                .ok_or("An electrical limit refers to an unmeasured channel")?;
            comparison::compare(t, t, limit)?;
        }
        Ok(())
    }
    pub fn validate_recording(&self, r: &recording::Recording) -> Result<(), String> {
        self.validate()?;
        r.validate()?;
        if self.recording_hash != r.fingerprint() {
            return Err("Electrical sidecar refers to a different recording".into());
        }
        for c in &self.channels {
            if c.raw_samples.last().unwrap().completion_s > r.stop_receipt_s {
                return Err(
                    "Electrical measurement extends beyond the recording stop receipt".into(),
                );
            }
        }
        Ok(())
    }
    pub fn servo_voltage(r: &recording::Recording) -> Result<Self, String> {
        let m = Self {
            version: 1,
            recording_hash: r.fingerprint(),
            source_hashes: r.source_hashes.clone(),
            timing_evidence: format!(
                "{} Voltage is in the same register transaction; internal sample age is unknown.",
                r.timing_evidence
            ),
            limits: BTreeMap::new(),
            channels: vec![Channel {
                name: "supply_voltage".into(),
                calibration: Calibration {
                    sensor: "Servo supply-voltage register".into(),
                    circuit_location:
                        "Supply at servo electronics; not measured at the battery terminals".into(),
                    raw_unit: "register count".into(),
                    gain: 0.1,
                    offset: 0.,
                    evidence:
                        "Protocol conversion of 0.1 V per count; not independently meter-calibrated"
                            .into(),
                    uncertainty: "0.1 V quantization; absolute error and sensor age unknown".into(),
                },
                raw_samples: r
                    .frames
                    .iter()
                    .map(|f| Observation {
                        time_s: f.control.observation.observed_s,
                        request_s: f.control.observation.request_s,
                        completion_s: f.control.observation.completion_s,
                        value: f.voltage_v / 0.1,
                    })
                    .collect(),
            }],
        };
        m.validate_recording(r)?;
        Ok(m)
    }
    pub fn traces(&self) -> Result<BTreeMap<String, Trace>, String> {
        let mut out = BTreeMap::new();
        for channel in &self.channels {
            let c = &channel.calibration;
            if [
                &c.sensor,
                &c.circuit_location,
                &c.raw_unit,
                &c.evidence,
                &c.uncertainty,
            ]
            .iter()
            .any(|s| s.trim().is_empty())
                || !c.gain.is_finite()
                || c.gain == 0.
                || !c.offset.is_finite()
                || channel.name.ends_with("power")
            {
                return Err("Measured electrical channels need explicit calibration, location, polarity and uncertainty; power is derived from synchronized V and A".into());
            }
            let (q, unit) = quantity(&channel.name)?;
            let trace = Trace {
                quantity: q.definition_id(),
                unit: unit.into(),
                samples: channel
                    .raw_samples
                    .iter()
                    .map(|s| Observation {
                        value: s.value * c.gain + c.offset,
                        ..s.clone()
                    })
                    .collect(),
            };
            validate_trace(&trace)?;
            if out.insert(channel.name.clone(), trace).is_some() {
                return Err("Duplicate electrical measurement channel".into());
            }
        }
        for circuit in ["supply", "winding"] {
            if let (Some(v), Some(i)) = (
                out.get(&format!("{circuit}_voltage")),
                out.get(&format!("{circuit}_current")),
            ) {
                let voltage_location = &self
                    .channels
                    .iter()
                    .find(|c| c.name == format!("{circuit}_voltage"))
                    .unwrap()
                    .calibration
                    .circuit_location;
                let current_location = &self
                    .channels
                    .iter()
                    .find(|c| c.name == format!("{circuit}_current"))
                    .unwrap()
                    .calibration
                    .circuit_location;
                if voltage_location != current_location {
                    return Err(
                        "Power requires matching explicit voltage/current circuit locations".into(),
                    );
                }
                if v.samples.len() != i.samples.len()
                    || v.samples.iter().zip(&i.samples).any(|(v, i)| {
                        v.time_s != i.time_s
                            || v.request_s != i.request_s
                            || v.completion_s != i.completion_s
                    })
                {
                    return Err("Power comparison requires synchronized voltage/current sample windows at the same circuit; no hidden interpolation of measured channels".into());
                }
                let trace = Trace {
                    quantity: QuantityKind::Power.definition_id(),
                    unit: "W".into(),
                    samples: v
                        .samples
                        .iter()
                        .zip(&i.samples)
                        .map(|(v, i)| Observation {
                            value: v.value * i.value,
                            ..v.clone()
                        })
                        .collect(),
                };
                validate_trace(&trace)?;
                out.insert(format!("{circuit}_power"), trace);
            }
        }
        Ok(out)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChannelComparison {
    pub name: String,
    pub measured: Trace,
    pub predicted: Trace,
    pub residuals: Vec<f64>,
    pub rmse: f64,
    pub maximum_abs_error: f64,
    pub final_error: f64,
    pub passes: Option<bool>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EnergyComparison {
    pub start_s: f64,
    pub end_s: f64,
    pub measured_drawn_j: f64,
    pub predicted_drawn_j: f64,
    pub measured_returned_j: f64,
    pub predicted_returned_j: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Evaluation {
    pub measurements: Measurements,
    pub prediction_hash: String,
    pub channels: Vec<ChannelComparison>,
    pub supply_energy: Option<EnergyComparison>,
    pub method: String,
}
pub fn prediction_hash(p: &recording::Prediction) -> String {
    blake3::hash(&serde_json::to_vec(p).unwrap())
        .to_hex()
        .to_string()
}
fn value(s: &power::Sample, name: &str) -> f64 {
    match name {
        "supply_voltage" => s.supply_voltage_v,
        "supply_current" => s.supply_current_a,
        "winding_voltage" => s.winding_voltage_v,
        "winding_current" => s.winding_current_a,
        "supply_power" => s.supply_power_w,
        "winding_power" => s.winding_power_w,
        _ => unreachable!(),
    }
}
/// Explicit linear sampling of the retained fine-step simulated trace, with no extrapolation.
fn sample(trace: &power::Trace, name: &str, t: f64) -> Result<f64, String> {
    let samples = &trace.samples;
    let j = samples.partition_point(|s| s.time_s < t);
    if let Some(s) = samples.get(j).filter(|s| s.time_s == t) {
        return Ok(value(s, name));
    }
    if j == 0 || j == samples.len() {
        return Err("Electrical comparison would extrapolate outside simulated coverage".into());
    }
    let (a, b) = (&samples[j - 1], &samples[j]);
    let f = (t - a.time_s) / (b.time_s - a.time_s);
    Ok(value(a, name) + (value(b, name) - value(a, name)) * f)
}
fn energy(t: &Trace) -> (f64, f64) {
    t.samples.windows(2).fold((0., 0.), |(draw, ret), w| {
        let dt = w[1].time_s - w[0].time_s;
        (
            draw + power::positive_area(w[0].value, w[1].value, dt),
            ret + power::positive_area(-w[0].value, -w[1].value, dt),
        )
    })
}
pub fn evaluate(m: &Measurements, p: &recording::Prediction) -> Result<Evaluation, String> {
    m.validate()?;
    if m.recording_hash != p.recording_hash {
        return Err("Electrical prediction and measurements refer to different recordings".into());
    }
    let trace = p
        .electrical
        .as_ref()
        .ok_or("Run a prediction with an explicit power source first")?;
    trace.validate()?;
    let imposed_voltage=p.model.power.as_ref().is_some_and(|s|s.source_component==sim_domain_electrical::voltage_history::VOLTAGE_HISTORY);
    let mut channels = vec![];
    let mut supply_energy = None;
    for (name, measured) in m.traces()? {
        let predicted = Trace {
            quantity: measured.quantity.clone(),
            unit: measured.unit.clone(),
            samples: measured
                .samples
                .iter()
                .map(|s| {
                    Ok(Observation {
                        value: sample(trace, &name, s.time_s)?,
                        ..s.clone()
                    })
                })
                .collect::<Result<_, String>>()?,
        };
        let unscored = Limits {
            rmse: f64::MAX,
            final_abs_error: f64::MAX,
        };
        let score = comparison::compare(
            &measured,
            &predicted,
            m.limits.get(&name).unwrap_or(&unscored),
        )?;
        if name == "supply_power" {
            let (md, mr) = energy(&measured);
            let (pd, pr) = energy(&predicted);
            supply_energy = Some(EnergyComparison {
                start_s: measured.samples[0].time_s,
                end_s: measured.samples.last().unwrap().time_s,
                measured_drawn_j: md,
                predicted_drawn_j: pd,
                measured_returned_j: mr,
                predicted_returned_j: pr,
            });
        }
        channels.push(ChannelComparison {
            name: name.clone(),
            measured,
            predicted,
            residuals: score.residuals,
            rmse: score.rmse,
            maximum_abs_error: score.maximum_abs_error,
            final_error: score.final_error,
            passes: (m.limits.contains_key(&name) && !(imposed_voltage && name=="supply_voltage")).then_some(score.passes),
        });
    }
    let max_step = trace
        .samples
        .windows(2)
        .map(|w| w[1].time_s - w[0].time_s)
        .fold(0., f64::max);
    Ok(Evaluation {
        measurements: m.clone(),
        prediction_hash: prediction_hash(p),
        channels,
        supply_energy,
        method: format!(
            "Simulated channels linearly sampled at measured timestamps; maximum retained simulation interval {max_step} s. No measured-channel interpolation, fitted time offset or extrapolation. Power is V*I at each synchronized sample, at its declared circuit location. Energy integrates that sampled curve over the displayed measurement interval, not the full run. Unknown calibration/clock errors remain limitations. Missing limits are unscored.{}",if imposed_voltage{" Supply voltage is an imposed waveform input and cannot independently pass validation. Other channel comparisons are conditional on that supplied waveform."}else{""}
        ),
    })
}
impl Evaluation {
    pub fn validate(&self, p: &recording::Prediction) -> Result<(), String> {
        if &evaluate(&self.measurements, p)? != self {
            return Err(
                "Electrical comparison changed from its original source, limits or prediction"
                    .into(),
            );
        }
        Ok(())
    }
}
