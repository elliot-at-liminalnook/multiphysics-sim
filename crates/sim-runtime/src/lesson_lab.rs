//! `sim-lab` steps: what the model predicts for the test, what the actuator
//! registry measured at the same drive, and the bench run itself.
//!
//! The bench is the calibration server (`serve_actuator_calibration`), which
//! owns the serial bus, the taught travel windows, the proven watchdogs and
//! STOP. Lessons never drive a motor themselves: they ask that server for a
//! `lab_step`, which runs through the campaign's guarded session and refuses
//! unless the operator confirmed the checklist. Its address comes from
//! `SIM_BENCH_URL` (e.g. `http://127.0.0.1:8123`); without it there is no bench.
use crate::lesson_model::Model;
use serde_json::{Value, json};
use sim_lesson::blocks::Lab;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

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
    let id = lab.compare.as_ref().ok_or("the lab names no `compare` (a sim-measured block)")?;
    let m = model.lesson.measured(id).ok_or_else(|| format!("no sim-measured `{id}`"))?;
    let data = sim_lesson::blocks::load_data(&model.lesson.dir().join(&m.data))?;
    let mut vars = data.conditions.clone();
    vars.insert(m.x.field.clone(), lab.test.duty);
    let mut single = m.clone();
    single.only.clear();
    // One simulated point at the lab's drive: reuse the measured machinery.
    let mut set = std::collections::BTreeMap::new();
    for (k, e) in &m.set {
        set.insert(k.clone(), sim_script::expr::eval(e, &vars).map_err(|err| format!("set {k}: {err}"))?);
    }
    let (doc, _) = model.system(&m.system)?;
    let scene: sim_lesson::Scene = serde_norway::from_str(&format!("id: lab-{}\nsystem: {}", lab.id, m.system)).map_err(|e| e.to_string())?;
    let scene = sim_lesson::Scene { set, run: m.run.clone(), plots: vec![m.y.observe.clone()], ..scene };
    let sdoc = crate::lesson::scene_document(&doc, model.registry, &scene)?;
    let run = crate::lesson::scene_run(&sdoc, model.registry, &scene, &sim_script::presentation::Timeline::new(vec![])?, model.use_cache, None, &|_| {})?;
    let series = run.series(&m.y.observe).ok_or_else(|| format!("the run records no `{}`", m.y.observe))?;
    let model_value = crate::system_study::reduce(series, &sim_system::Metric { label: m.y.observe.clone(), observable: m.y.observe.clone(), reduce: m.y.reduce, window: m.y.window }).ok_or("no samples")?;
    let same: Vec<f64> = data.points.iter().filter(|p| p.get(&m.x.field).is_some_and(|x| (x - lab.test.duty).abs() < 1e-9)).filter_map(|p| p.get(&m.y.field).copied()).collect();
    let registry = (!same.is_empty()).then(|| same.iter().sum::<f64>() / same.len() as f64);
    Ok(LabPrediction { model: model_value, registry, unit: m.y.unit.clone(), fidelity: run.fidelity })
}

/// The configured bench, if any.
pub fn bench_url() -> Option<String> {
    std::env::var("SIM_BENCH_URL").ok().map(|u| u.trim().trim_end_matches('/').to_string()).filter(|u| !u.is_empty())
}

fn request(base: &str, method: &str, path: &str, body: Option<&Value>) -> Result<Value, String> {
    let rest = base.strip_prefix("http://").ok_or("SIM_BENCH_URL must start with http:// (the calibration server on this machine)")?;
    let host = rest.split('/').next().unwrap_or(rest);
    let addr = std::net::ToSocketAddrs::to_socket_addrs(host).map_err(|e| format!("{host}: {e}"))?.next().ok_or_else(|| format!("{host}: no address"))?;
    if !addr.ip().is_loopback() {
        return Err("the bench must be on this machine (a loopback address)".into());
    }
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).map_err(|e| format!("bench at {base}: {e}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok();
    let payload = body.map(|b| b.to_string()).unwrap_or_default();
    let head = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", payload.len());
    stream.write_all(head.as_bytes()).and_then(|_| stream.write_all(payload.as_bytes())).map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&raw);
    let (headers, body) = text.split_once("\r\n\r\n").ok_or("bench sent no body")?;
    let status: u16 = headers.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let value: Value = serde_json::from_str(body.trim()).map_err(|e| format!("bench reply: {e}"))?;
    if status >= 400 {
        return Err(value["error"].as_str().or(value.as_str()).map(String::from).unwrap_or_else(|| format!("bench answered {status}")));
    }
    Ok(value)
}

/// The bench's status (motors, taught poses, any running lab step).
pub fn bench_status(base: &str) -> Result<Value, String> {
    request(base, "GET", "/calibration/status", None)
}

/// Ask the bench to run a lab step. `confirmed` is the operator checklist.
pub fn bench_start(base: &str, lab: &Lab, confirmed: bool) -> Result<Value, String> {
    if !confirmed {
        return Err("tick every item of the checklist first".into());
    }
    request(base, "POST", "/calibration/command", Some(&json!({"action": "lab_step", "role": lab.joint, "duty": lab.test.duty, "seconds": lab.test.seconds, "supported": true})))
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
        let lab: Lab = serde_norway::from_str("id: l\njoint: knee\ntest: { duty: 0.15, seconds: 1.0 }").unwrap();
        assert!(bench_start("http://127.0.0.1:9", &lab, false).unwrap_err().contains("checklist"));
        assert!(request("http://10.0.0.1:80", "GET", "/", None).unwrap_err().contains("this machine"));
        assert!(request("https://127.0.0.1:1", "GET", "/", None).unwrap_err().contains("http://"));
        assert_eq!(bench_result(&json!({"lab": {"running": true}})).unwrap(), None);
        assert!(bench_result(&json!({"lab": {"running": false, "error": "Stop is latched"}})).unwrap_err().contains("latched"));
        assert_eq!(bench_result(&json!({"lab": {"running": false, "result": {"steady_rad_s": 0.44}}})).unwrap().unwrap()["steady_rad_s"], 0.44);
    }
}
