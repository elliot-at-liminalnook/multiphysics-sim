//! Electrical scenarios and evidence. Supply current is positive on discharge;
//! winding current is a different circuit quantity and is never substituted for it.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Setup {
    /// Registry component with electrical p/n terminals. Reuses its physical equations.
    pub source_component: String,
    pub source_parameters: BTreeMap<String, f64>,
    /// Explicit electronics/other-load approximation, positive consumption at the bus.
    pub auxiliary_current_a: f64,
    pub evidence: String,
    pub limits: Limits,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Limits {
    pub minimum_voltage_v: Option<f64>,
    pub maximum_discharge_current_a: Option<f64>,
    pub maximum_charge_current_a: Option<f64>,
    pub maximum_winding_current_a: Option<f64>,
    pub maximum_draw_power_w: Option<f64>,
    pub maximum_return_power_w: Option<f64>,
}
impl Setup {
    pub fn validate(&self) -> Result<(), String> {
        if self.evidence.trim().is_empty()
            || !self.auxiliary_current_a.is_finite()
            || self.auxiliary_current_a < 0.
        {
            return Err(
                "Power scenario requires source evidence and finite nonnegative auxiliary current"
                    .into(),
            );
        }
        crate::registry()
            .get(&self.source_component.as_str().into())
            .map_err(|e| e.to_string())?
            .validate_parameters(&self.source_parameters)
            .map_err(|e| e.to_string())?;
        if self.source_component == sim_domain_electrical::voltage_history::VOLTAGE_HISTORY {
            sim_domain_electrical::voltage_history::VoltageHistory::from_parameters(&self.source_parameters).map_err(|e|e.to_string())?;
        }
        for value in [
            self.limits.minimum_voltage_v,
            self.limits.maximum_discharge_current_a,
            self.limits.maximum_charge_current_a,
            self.limits.maximum_winding_current_a,
            self.limits.maximum_draw_power_w,
            self.limits.maximum_return_power_w,
        ]
        .into_iter()
        .flatten()
        {
            if !value.is_finite() || value < 0. {
                return Err("Electrical limits must be finite and nonnegative".into());
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub time_s: f64,
    pub supply_voltage_v: f64,
    pub supply_current_a: f64,
    pub winding_voltage_v: f64,
    pub winding_current_a: f64,
    pub supply_power_w: f64,
    pub winding_power_w: f64,
    pub state_of_charge: Option<f64>,
}
impl Sample {
    pub fn validate(&self) -> Result<(), String> {
        if [
            self.time_s,
            self.supply_voltage_v,
            self.supply_current_a,
            self.winding_voltage_v,
            self.winding_current_a,
            self.supply_power_w,
            self.winding_power_w,
        ]
        .iter()
        .any(|v| !v.is_finite())
            || self.time_s < 0.
            || self.state_of_charge.is_some_and(|v| !v.is_finite())
        {
            return Err("Nonfinite electrical observation".into());
        }
        for (p, v, i) in [
            (
                self.supply_power_w,
                self.supply_voltage_v,
                self.supply_current_a,
            ),
            (
                self.winding_power_w,
                self.winding_voltage_v,
                self.winding_current_a,
            ),
        ] {
            if !(v * i).is_finite() || (p - v * i).abs() > 1e-10 * (1. + p.abs()) {
                return Err("Electrical power must use voltage and current from the same circuit and instant".into());
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub minimum_voltage_v: f64,
    pub maximum_voltage_v: f64,
    pub peak_discharge_current_a: f64,
    pub peak_charge_current_a: f64,
    pub peak_winding_current_a: f64,
    pub peak_draw_power_w: f64,
    pub peak_return_power_w: f64,
    pub drawn_energy_j: f64,
    pub returned_energy_j: f64,
    pub violations: Vec<String>,
    /// None means no electrical acceptance limits were declared.
    pub passes: Option<bool>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Trace {
    pub samples: Vec<Sample>,
    pub limits: Limits,
    pub summary: Summary,
    pub interpretation: String,
}
impl Trace {
    pub fn new(samples: Vec<Sample>, limits: Limits) -> Result<Self, String> {
        let summary = summarize(&samples, &limits)?;
        Ok(Self{samples,limits,summary,interpretation:"Shared-circuit simulation. Positive supply current/power discharges the source; negative values return energy. Winding and supply channels remain separate. Peaks are sampled, not PWM-switching peaks. Energy integrates the sampled power curve. Source, electronics load and battery discharge curve are declared hypotheses until measured.".into()})
    }
    pub fn validate(&self) -> Result<(), String> {
        if summarize(&self.samples, &self.limits)? != self.summary
            || self.interpretation.trim().is_empty()
        {
            return Err("Electrical summary differs from its retained samples and limits".into());
        }
        Ok(())
    }
}
pub(super) use crate::electrical::positive_area;
pub fn summarize(samples: &[Sample], limits: &Limits) -> Result<Summary, String> {
    if samples.is_empty() || samples.windows(2).any(|w| w[0].time_s >= w[1].time_s) {
        return Err("Electrical trace requires ordered samples".into());
    }
    for s in samples {
        s.validate()?;
    }
    let mut out = Summary {
        minimum_voltage_v: f64::INFINITY,
        maximum_voltage_v: f64::NEG_INFINITY,
        peak_discharge_current_a: 0.,
        peak_charge_current_a: 0.,
        peak_winding_current_a: 0.,
        peak_draw_power_w: 0.,
        peak_return_power_w: 0.,
        drawn_energy_j: 0.,
        returned_energy_j: 0.,
        violations: vec![],
        passes: None,
    };
    for s in samples {
        out.minimum_voltage_v = out.minimum_voltage_v.min(s.supply_voltage_v);
        out.maximum_voltage_v = out.maximum_voltage_v.max(s.supply_voltage_v);
        out.peak_discharge_current_a = out.peak_discharge_current_a.max(s.supply_current_a);
        out.peak_charge_current_a = out.peak_charge_current_a.max(-s.supply_current_a);
        out.peak_winding_current_a = out.peak_winding_current_a.max(s.winding_current_a.abs());
        out.peak_draw_power_w = out.peak_draw_power_w.max(s.supply_power_w);
        out.peak_return_power_w = out.peak_return_power_w.max(-s.supply_power_w);
    }
    for w in samples.windows(2) {
        let dt = w[1].time_s - w[0].time_s;
        out.drawn_energy_j += positive_area(w[0].supply_power_w, w[1].supply_power_w, dt);
        out.returned_energy_j += positive_area(-w[0].supply_power_w, -w[1].supply_power_w, dt);
    }
    let checks = [
        (
            "minimum bus voltage",
            limits.minimum_voltage_v,
            out.minimum_voltage_v,
            true,
        ),
        (
            "discharge current",
            limits.maximum_discharge_current_a,
            out.peak_discharge_current_a,
            false,
        ),
        (
            "charging current",
            limits.maximum_charge_current_a,
            out.peak_charge_current_a,
            false,
        ),
        (
            "winding current",
            limits.maximum_winding_current_a,
            out.peak_winding_current_a,
            false,
        ),
        (
            "draw power",
            limits.maximum_draw_power_w,
            out.peak_draw_power_w,
            false,
        ),
        (
            "returned power",
            limits.maximum_return_power_w,
            out.peak_return_power_w,
            false,
        ),
    ];
    let declared = checks.iter().any(|(_, limit, _, _)| limit.is_some());
    for (label, limit, value, minimum) in checks {
        if let Some(limit) = limit {
            if !limit.is_finite() || limit < 0. {
                return Err("Invalid electrical comparison limit".into());
            }
            if if minimum {
                value < limit
            } else {
                value > limit
            } {
                out.violations
                    .push(format!("{label}: observed {value}, limit {limit}"));
            }
        }
    }
    if samples
        .iter()
        .any(|s| s.state_of_charge.is_some_and(|v| !(0. ..=1.).contains(&v)))
    {
        out.violations.push("Battery state of charge outside validated [0,1] range; depletion/overcharge continuation is not a valid battery prediction".into());
    }
    out.passes = declared.then_some(out.violations.is_empty());
    Ok(out)
}

/// Declared controller-visible channels, sampled with the encoder observation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sensing {
    pub voltage_quantum_v: f64,
    pub supply_current_quantum_a: Option<f64>,
    pub winding_current_quantum_a: Option<f64>,
    pub evidence: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Controller {
    pub sensing: Sensing,
    pub nominal_voltage_for_compensation_v: Option<f64>,
    pub limits: Limits,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Observation {
    pub supply_voltage_v: f64,
    pub supply_current_a: Option<f64>,
    pub winding_current_a: Option<f64>,
}
impl Controller {
    pub fn validate(&self) -> Result<(), String> {
        let s = &self.sensing;
        if s.evidence.trim().is_empty()
            || !s.voltage_quantum_v.is_finite()
            || s.voltage_quantum_v <= 0.
            || [
                s.supply_current_quantum_a,
                s.winding_current_quantum_a,
                self.nominal_voltage_for_compensation_v,
            ]
            .into_iter()
            .flatten()
            .any(|v| !v.is_finite() || v <= 0.)
        {
            return Err(
                "Electrical feedback requires positive sensor resolution and explicit evidence"
                    .into(),
            );
        }
        for v in [
            self.limits.minimum_voltage_v,
            self.limits.maximum_discharge_current_a,
            self.limits.maximum_charge_current_a,
            self.limits.maximum_winding_current_a,
            self.limits.maximum_draw_power_w,
            self.limits.maximum_return_power_w,
        ]
        .into_iter()
        .flatten()
        {
            if !v.is_finite() || v < 0. {
                return Err("Invalid controller electrical limit".into());
            }
        }
        if s.supply_current_quantum_a.is_none()
            && [
                self.limits.maximum_discharge_current_a,
                self.limits.maximum_charge_current_a,
                self.limits.maximum_draw_power_w,
                self.limits.maximum_return_power_w,
            ]
            .iter()
            .any(|v| v.is_some())
        {
            return Err(
                "Supply current/power limits require a declared calibrated supply-current channel"
                    .into(),
            );
        }
        if s.winding_current_quantum_a.is_none() && self.limits.maximum_winding_current_a.is_some()
        {
            return Err("Winding current limit requires a declared winding-current channel".into());
        }
        Ok(())
    }
    pub fn observe_simulation(&self, sensors: &[f64]) -> Result<Observation, String> {
        if sensors.len() != 4 {
            return Err("Electrical feedback requires an instrumented physical source".into());
        }
        let quantize = |v: f64, q: f64| (v / q).round() * q;
        let result = Observation {
            supply_voltage_v: quantize(sensors[1], self.sensing.voltage_quantum_v),
            supply_current_a: self
                .sensing
                .supply_current_quantum_a
                .map(|q| quantize(sensors[2], q)),
            winding_current_a: self
                .sensing
                .winding_current_quantum_a
                .map(|q| quantize(sensors[3], q)),
        };
        self.validate_observation(&result)?;
        Ok(result)
    }
    pub fn validate_observation(&self, o: &Observation) -> Result<(), String> {
        if !o.supply_voltage_v.is_finite()
            || o.supply_voltage_v <= 0.
            || o.supply_current_a
                .is_some_and(|i| !(i * o.supply_voltage_v).is_finite())
            || o.supply_current_a.is_some() != self.sensing.supply_current_quantum_a.is_some()
            || o.winding_current_a.is_some() != self.sensing.winding_current_quantum_a.is_some()
            || [o.supply_current_a, o.winding_current_a]
                .into_iter()
                .flatten()
                .any(|v| !v.is_finite())
        {
            return Err("Electrical feedback is missing, nonfinite or differs from declared sensor channels".into());
        }
        Ok(())
    }
    pub fn violations(&self, o: &Observation) -> Result<Vec<String>, String> {
        self.validate_observation(o)?;
        let mut violations = vec![];
        if self
            .limits
            .minimum_voltage_v
            .is_some_and(|l| o.supply_voltage_v < l)
        {
            violations.push("Supply undervoltage".into());
        }
        for (name, value, limit) in [
            (
                "Discharge current",
                o.supply_current_a,
                self.limits.maximum_discharge_current_a,
            ),
            (
                "Charge current",
                o.supply_current_a.map(|v| -v),
                self.limits.maximum_charge_current_a,
            ),
            (
                "Winding current",
                o.winding_current_a.map(f64::abs),
                self.limits.maximum_winding_current_a,
            ),
            (
                "Draw power",
                o.supply_current_a.map(|i| i * o.supply_voltage_v),
                self.limits.maximum_draw_power_w,
            ),
            (
                "Returned power",
                o.supply_current_a.map(|i| -i * o.supply_voltage_v),
                self.limits.maximum_return_power_w,
            ),
        ] {
            if let Some(limit) = limit {
                if value.ok_or("Missing electrical limit channel")? > limit {
                    violations.push(name.into());
                }
            }
        }
        Ok(violations)
    }
}
