//! Shared circuit-observation accounting. These are sampled simulation values,
//! not a sensor model or a substitute for calibrated physical measurements.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SupplySample {
    pub time_s: f64,
    pub voltage_v: f64,
    /// Positive out of the source (discharge), negative into the source (charge).
    pub current_a: f64,
    pub power_w: f64,
    pub state_of_charge: Option<f64>,
}
impl SupplySample {
    pub fn validate(&self) -> Result<(), String> {
        if [self.time_s, self.voltage_v, self.current_a, self.power_w]
            .iter()
            .any(|v| !v.is_finite())
            || self.time_s < 0.
            || self.state_of_charge.is_some_and(|v| !v.is_finite())
            || !(self.voltage_v * self.current_a).is_finite()
            || (self.power_w - self.voltage_v * self.current_a).abs()
                > 1e-10 * (1. + self.power_w.abs())
        {
            return Err("Supply samples require finite, synchronized voltage/current/power and nonnegative time".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SupplySummary {
    pub minimum_voltage_v: f64,
    pub maximum_voltage_v: f64,
    pub peak_discharge_current_a: f64,
    pub peak_charge_current_a: f64,
    pub peak_draw_power_w: f64,
    pub peak_return_power_w: f64,
    pub drawn_energy_j: f64,
    pub returned_energy_j: f64,
    pub discharged_charge_c: f64,
    pub returned_charge_c: f64,
    pub maximum_sample_interval_s: f64,
    pub state_of_charge_in_range: bool,
}
/// Integrate the positive portion of a linearly interpolated segment, including
/// its exact zero crossing. Shared by bench, calibrated-measurement and robot reports.
pub(crate) fn positive_area(a: f64, b: f64, dt: f64) -> f64 {
    if a >= 0. && b >= 0. {
        0.5 * (a + b) * dt
    } else if a <= 0. && b <= 0. {
        0.
    } else {
        let positive = a.max(b);
        0.5 * positive * dt * positive / (a.abs() + b.abs())
    }
}
pub fn summarize_supply(samples: &[SupplySample]) -> Result<SupplySummary, String> {
    if samples.is_empty() || samples.windows(2).any(|w| w[0].time_s >= w[1].time_s) {
        return Err("Supply accounting requires ordered, nonempty samples".into());
    }
    let mut out = SupplySummary {
        minimum_voltage_v: f64::INFINITY,
        maximum_voltage_v: f64::NEG_INFINITY,
        peak_discharge_current_a: 0.,
        peak_charge_current_a: 0.,
        peak_draw_power_w: 0.,
        peak_return_power_w: 0.,
        drawn_energy_j: 0.,
        returned_energy_j: 0.,
        discharged_charge_c: 0.,
        returned_charge_c: 0.,
        maximum_sample_interval_s: 0.,
        state_of_charge_in_range: true,
    };
    for s in samples {
        s.validate()?;
        out.minimum_voltage_v = out.minimum_voltage_v.min(s.voltage_v);
        out.maximum_voltage_v = out.maximum_voltage_v.max(s.voltage_v);
        out.peak_discharge_current_a = out.peak_discharge_current_a.max(s.current_a);
        out.peak_charge_current_a = out.peak_charge_current_a.max(-s.current_a);
        out.peak_draw_power_w = out.peak_draw_power_w.max(s.power_w);
        out.peak_return_power_w = out.peak_return_power_w.max(-s.power_w);
        out.state_of_charge_in_range &= s.state_of_charge.is_none_or(|v| (0. ..=1.).contains(&v));
    }
    for w in samples.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        let dt = b.time_s - a.time_s;
        out.maximum_sample_interval_s = out.maximum_sample_interval_s.max(dt);
        out.drawn_energy_j += positive_area(a.power_w, b.power_w, dt);
        out.returned_energy_j += positive_area(-a.power_w, -b.power_w, dt);
        out.discharged_charge_c += positive_area(a.current_a, b.current_a, dt);
        out.returned_charge_c += positive_area(-a.current_a, -b.current_a, dt);
    }
    if [
        out.drawn_energy_j,
        out.returned_energy_j,
        out.discharged_charge_c,
        out.returned_charge_c,
    ]
    .iter()
    .any(|v| !v.is_finite())
    {
        return Err("Supply integral overflow".into());
    }
    Ok(out)
}
