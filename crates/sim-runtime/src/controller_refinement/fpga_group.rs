//! Captured FPGA timing and the same controller, with electrically coupled axes.
use super::{calibration::Family, fpga, power};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupplyScenario {
    pub source: power::Setup,
    pub shared_resistance_ohm: f64,
    pub branch_resistance_ohm: BTreeMap<u8, f64>,
    pub evidence: String,
}

pub fn predict(
    recording: &fpga::Recording,
    family: &Family,
    scenario: &SupplyScenario,
    closed_loop: bool,
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize, usize),
) -> Result<serde_json::Value, String> {
    recording.validate()?;
    if scenario
        .branch_resistance_ohm
        .keys()
        .copied()
        .collect::<Vec<_>>()
        != recording.plan.ids
    {
        return Err(
            "Declare branch resistance explicitly for every recorded motor, with no extra IDs"
                .into(),
        );
    }
    let mut axes = Vec::new();
    let mut step = f64::INFINITY;
    for &id in &recording.plan.ids {
        let model = family.model(id)?;
        step = step.min(model.step_s);
        let mut times = Vec::new();
        for frame in &recording.frames {
            let observation = frame.observations.iter().find(|o| o.id == id).unwrap();
            times.push((observation.request_s + observation.completion_s) * 0.5);
            times.push(frame.command_receipt_s);
        }
        times.push(recording.stop_receipt_s);
        let first = recording.frames[0]
            .observations
            .iter()
            .find(|o| o.id == id)
            .unwrap();
        axes.push(crate::actuator_bench::AxisSetup {
            key: id.to_string(),
            model,
            temperature_c: first.telemetry.temperature_c as f64,
            command_and_sample_times_s: times,
            branch_resistance_ohm: scenario.branch_resistance_ohm[&id],
        });
    }
    let setup = crate::actuator_bench::GroupSetup {
        source: scenario.source.clone(),
        shared_resistance_ohm: scenario.shared_resistance_ohm,
        axes,
        evidence: scenario.evidence.clone(),
        seed: 0,
    };
    let mut bench = crate::actuator_bench::prepare_group(&setup)?;
    let mut outputs = BTreeMap::new();
    let mut electrical = BTreeMap::<u8, Vec<power::Sample>>::new();
    for &id in &recording.plan.ids {
        let output = Arc::new(Mutex::new(Vec::new()));
        fpga::attach_controller(
            &mut bench.runtime,
            bench.axes[&id.to_string()].controller,
            recording.clone(),
            (id - 4) as usize,
            closed_loop,
            output.clone(),
        )?;
        outputs.insert(id, output);
        electrical.insert(id, Vec::new());
    }
    let duration = recording.stop_receipt_s + step;
    let mut time = 0.;
    let mut next_electrical = 0.;
    let mut supply = Vec::new();
    while time < duration {
        if cancel.load(Ordering::Relaxed) {
            return Err("Cancelled".into());
        }
        progress((time / duration * 1000.) as usize, 1000);
        let dt = (duration - time).min(step);
        bench.runtime.advance(dt, step).map_err(|e| e.to_string())?;
        time += dt;
        // Electrical samples every millisecond (or every step if coarser).
        if time + 1e-12 >= next_electrical || time >= duration {
            let [v, i, p] = bench.supply();
            supply.push([time, v, i, p]);
            for &id in &recording.plan.ids {
                electrical
                    .get_mut(&id)
                    .unwrap()
                    .push(bench.electrical_sample(&id.to_string(), time).unwrap());
            }
            next_electrical = time + step.max(0.001);
        }
    }
    let mut predictions = Vec::new();
    for &id in &recording.plan.ids {
        let samples = outputs[&id].lock().unwrap().clone();
        if samples.len() != recording.frames.len() {
            return Err("Incomplete coupled observation schedule".into());
        }
        let errors = samples
            .iter()
            .zip(&recording.frames)
            .map(|(s, f)| {
                s[1] - f
                    .observations
                    .iter()
                    .find(|o| o.id == id)
                    .unwrap()
                    .telemetry
                    .position_raw as f64
            })
            .collect::<Vec<_>>();
        let rms = (errors.iter().map(|e| e * e).sum::<f64>() / errors.len() as f64).sqrt();
        predictions.push(serde_json::json!({"id":id,"rms_prediction_counts":rms,"rms_prediction_degrees":rms*360./4096.,
            "peak_prediction_counts":errors.iter().fold(0_f64,|m,e|m.max(e.abs())),"prediction_limit_counts":3.,"prediction_pass":rms<=3.,
            "samples_time_encoder_duty_angle":samples,"electrical":electrical[&id]}));
    }
    Ok(
        serde_json::json!({"mode":if closed_loop{"own_feedback"}else{"recorded_pwm_replay"},
        "recording_blake3":recording.fingerprint(),"family":family,"setup":setup,"step_s":step,
        "runtime":crate::physics_context::RuntimeIdentity::current(),
        "controller_ir_blake3":sim_domain_control::fixed_pd::implementation_identity(),
        "assumptions":"One shared Rust electrical circuit and separate mechanical shafts. Captured timing and initial temperature, quantized simulated feedback. Source, wire resistance, auxiliary load and current calibration remain hypotheses; no measured amp, watt or battery accuracy claim. Electrical samples omit switching peaks.",
        "predictions":predictions,"supply_samples_time_voltage_current_power":supply}),
    )
}
