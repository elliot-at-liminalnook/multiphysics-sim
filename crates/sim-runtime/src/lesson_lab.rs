//! `sim-lab` steps: what the model predicts for the test, what the actuator
//! registry measured at the same drive, and the bench run itself.
//!
//! Native hosts request the shared in-process calibration application through
//! caller-owned jobs. This module supplies prediction/result parsing and typed
//! request validation, never a hardware HTTP connection.
use crate::lesson_model::Model;
use serde_json::{Value, json};
use sim_lesson::blocks::Lab;

/// The model's steady value for the lab's drive, and the registry's
/// measured mean at the same drive (when the compared data has it).
#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub struct LabPrediction {
    pub model: f64,
    pub registry: Option<f64>,
    pub unit: String,
    pub fidelity: String,
}

/// Predict the lab step with the `sim-measured` block it compares against.
pub fn prediction(model: &Model, lab: &Lab) -> Result<LabPrediction, String> {
    let id = lab
        .compare
        .as_ref()
        .ok_or("the lab names no `compare` (a sim-measured block)")?;
    let m = model
        .lesson
        .measured(id)
        .ok_or_else(|| format!("no sim-measured `{id}`"))?;
    let data = sim_lesson::blocks::load_data(&model.lesson.dir().join(&m.data))?;
    let mut vars = data.conditions.clone();
    vars.insert(m.x.field.clone(), lab.test.duty);
    let mut single = m.clone();
    single.only.clear();
    // One simulated point at the lab's drive: reuse the measured machinery.
    let mut set = std::collections::BTreeMap::new();
    for (k, e) in &m.set {
        set.insert(
            k.clone(),
            sim_script::expr::eval(e, &vars).map_err(|err| format!("set {k}: {err}"))?,
        );
    }
    let (doc, _) = model.system(&m.system)?;
    let scene: sim_lesson::Scene =
        serde_norway::from_str(&format!("id: lab-{}\nsystem: {}", lab.id, m.system))
            .map_err(|e| e.to_string())?;
    let scene = sim_lesson::Scene {
        set,
        run: m.run.clone(),
        plots: vec![m.y.observe.clone()],
        ..scene
    };
    let sdoc = crate::lesson::scene_document(&doc, model.registry, &scene)?;
    let run = crate::lesson::scene_run(
        &sdoc,
        model.registry,
        &scene,
        &sim_script::presentation::Timeline::new(vec![])?,
        model.use_cache,
        None,
        &|_| {},
    )?;
    let series = run
        .series(&m.y.observe)
        .ok_or_else(|| format!("the run records no `{}`", m.y.observe))?;
    let model_value = crate::system_study::reduce(
        series,
        &sim_system::Metric {
            label: m.y.observe.clone(),
            observable: m.y.observe.clone(),
            reduce: m.y.reduce,
            window: m.y.window,
        },
    )
    .ok_or("no samples")?;
    let same: Vec<f64> = data
        .points
        .iter()
        .filter(|p| {
            p.get(&m.x.field)
                .is_some_and(|x| (x - lab.test.duty).abs() < 1e-9)
        })
        .filter_map(|p| p.get(&m.y.field).copied())
        .collect();
    let registry = (!same.is_empty()).then(|| same.iter().sum::<f64>() / same.len() as f64);
    Ok(LabPrediction {
        model: model_value,
        registry,
        unit: m.y.unit.clone(),
        fidelity: run.fidelity,
    })
}

/// Explicit local configuration. Obsolete server settings refuse by name.
pub fn bench_config() -> Result<Option<std::path::PathBuf>, String> {
    if std::env::var_os("SIM_BENCH_URL").is_some() {
        return Err("SIM_BENCH_URL is obsolete: use SIM_BENCH_CONFIG with a local calibration JSON file; no server is contacted".into());
    }
    Ok(std::env::var_os("SIM_BENCH_CONFIG")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from))
}
/// Validate the operator confirmation and build the shared application request.
pub fn bench_request(lab: &Lab, confirmed: bool) -> Result<Value, String> {
    if !confirmed {
        return Err("tick every item of the checklist first".into());
    }
    Ok(
        json!({"action":"lab_step", "role":lab.joint, "duty":lab.test.duty, "seconds":lab.test.seconds, "supported":true}),
    )
}

/// The finished lab step in a status reply: Ok(None) while it runs.
pub fn bench_result(status: &Value) -> Result<Option<Value>, String> {
    let lab = &status["lab"];
    if lab["running"].as_bool() == Some(true) {
        return Ok(None);
    }
    if let Some(e) = lab["error"].as_str() {
        return Err(e.to_string());
    }
    Ok(lab.get("result").cloned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_bench_must_be_local_and_confirmed() {
        let lab: Lab =
            serde_norway::from_str("id: l\njoint: knee\ntest: { duty: 0.15, seconds: 1.0 }")
                .unwrap();
        assert!(
            bench_request(&lab, false)
                .unwrap_err()
                .contains("checklist")
        );
        assert_eq!(bench_request(&lab, true).unwrap()["action"], "lab_step");
        assert_eq!(
            bench_result(&json!({"lab": {"running": true}})).unwrap(),
            None
        );
        assert!(
            bench_result(&json!({"lab": {"running": false, "error": "Stop is latched"}}))
                .unwrap_err()
                .contains("latched")
        );
        assert_eq!(
            bench_result(&json!({"lab": {"running": false, "result": {"steady_rad_s": 0.44}}}))
                .unwrap()
                .unwrap()["steady_rad_s"],
            0.44
        );
    }
}
