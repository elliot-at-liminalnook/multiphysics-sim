//! Conditioning motor identification on observed terminal voltage. This imposes
//! the captured disturbance; it does not identify or validate a battery model.
use super::{fpga, fpga_review, power};
use crate::experiment_study::ModelSettings;
use serde::{Deserialize, Serialize};
use sim_domain_electrical::voltage_history::{VOLTAGE_HISTORY, VoltageHistory};
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicBool, Ordering},
};

pub const ASSUMPTIONS: &str = "Terminal voltage is imposed from this motor's telemetry using linear interpolation at request/reply midpoints and held endpoints. Internal sample age and voltage calibration are unknown; telemetry has 0.1 V resolution. Voltage agreement is not battery validation. Current, power and motion are predictions conditional on this voltage. The voltage_v scalar is only the recorded mean. The controller uses its own simulated encoder in closed-loop mode.";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Conditioning {
    pub recording_blake3: String,
    pub motor_id: u8,
    pub replaced_source: Option<power::Setup>,
}

pub fn condition_model(
    r: &fpga::Recording,
    id: u8,
    model: &ModelSettings,
) -> Result<(ModelSettings, Conditioning), String> {
    r.validate()?;
    model.validate()?;
    if !r.plan.ids.contains(&id) {
        return Err("Voltage conditioning motor absent from recording".into());
    }
    if model.conditions.voltage_v.is_some() {
        return Err("Remove the fixed voltage override before imposing measured voltage".into());
    }
    let points: Vec<_> = r
        .frames
        .iter()
        .map(|f| {
            let o = f.observations.iter().find(|o| o.id == id).unwrap();
            [(o.request_s + o.completion_s) * 0.5, o.telemetry.voltage_v]
        })
        .collect();
    if points.iter().any(|p| p[1] <= 0.) {
        return Err("Measured supply voltage must be positive".into());
    }
    let mut parameters = BTreeMap::from([("count".into(), points.len() as f64)]);
    for (i, [t, v]) in points.into_iter().enumerate() {
        parameters.insert(format!("time.{i}"), t);
        parameters.insert(format!("voltage.{i}"), v);
    }
    VoltageHistory::from_parameters(&parameters).map_err(|e| e.to_string())?;
    let conditioning = Conditioning {
        recording_blake3: r.fingerprint(),
        motor_id: id,
        replaced_source: model.power.clone(),
    };
    let mut resolved = model.clone();
    resolved.power = Some(power::Setup {
        source_component: VOLTAGE_HISTORY.into(),
        source_parameters: parameters,
        auxiliary_current_a: model.power.as_ref().map_or(0., |p| p.auxiliary_current_a),
        limits: model
            .power
            .as_ref()
            .map(|p| p.limits.clone())
            .unwrap_or_default(),
        evidence: format!(
            "FPGA recording {} motor {id}. {ASSUMPTIONS} Auxiliary current is retained from the replaced scenario, or explicitly omitted when none was supplied.",
            r.fingerprint()
        ),
    });
    resolved.validate()?;
    Ok((resolved, conditioning))
}

impl Conditioning {
    pub fn validate(&self, r: &fpga::Recording, p: &fpga_review::Prediction) -> Result<(), String> {
        let mut original = p.model.clone();
        original.power = self.replaced_source.clone();
        let (expected, source) = condition_model(r, p.id, &original)?;
        if *self != source || p.model != expected || !p.assumptions.contains(ASSUMPTIONS) {
            return Err(
                "Voltage-conditioned prediction differs from its source recording or model".into(),
            );
        }
        let history = VoltageHistory::from_parameters(&expected.power.unwrap().source_parameters)
            .map_err(|e| e.to_string())?;
        if p.electrical.is_empty()
            || p.electrical[0].time_s > p.model.step_s + 1e-9
            || p.electrical.last().unwrap().time_s
                < r.stop_receipt_s
            || p.electrical.windows(2).any(|w| {
                w[0].time_s >= w[1].time_s
                    || w[1].time_s - w[0].time_s > p.model.step_s + 1e-9
            })
            || p.electrical
                .iter()
                .any(|s| (s.supply_voltage_v - history.voltage(s.time_s)).abs() > 1e-5)
        {
            return Err("Missing or inconsistent imposed-voltage runtime trace".into());
        }
        Ok(())
    }
}

pub fn predict(
    r: &fpga::Recording,
    id: u8,
    model: &ModelSettings,
    mode: fpga_review::Mode,
    cancel: &AtomicBool,
    progress: impl FnMut(usize, usize),
) -> Result<fpga_review::Prediction, String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("Cancelled".into());
    }
    if !mode.measured_voltage() {
        return Err("Measured-voltage prediction mode required".into());
    }
    let (resolved, conditioning) = condition_model(r, id, model)?;
    let mut p: fpga_review::Prediction = serde_json::from_value(fpga::predict_cancellable(
        r,
        id,
        &resolved,
        mode.closed_loop(),
        cancel,
        progress,
    )?)
    .map_err(|e| e.to_string())?;
    p.mode = mode;
    p.assumptions = format!(
        "{ASSUMPTIONS} PWM application uses the captured bridge receipt, not a measured switching timestamp. Independent motor plant; shared source/wiring coupling is not validated."
    );
    p.voltage_input = Some(conditioning);
    p.validate(r)?;
    Ok(p)
}
