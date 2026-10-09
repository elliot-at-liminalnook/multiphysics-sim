//! Units, measured expectations, comparisons and replay for captured runs
//! (RoboCAD's `experiment_results.py`). The viewer and REST review the same
//! signals, sample alignment and pass/fail decisions.
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// One named series with its own time base, unit and stable CAD ids.
#[derive(Clone, Debug, PartialEq)]
pub struct Series {
    pub t: Vec<f64>,
    pub values: Vec<f64>,
    pub unit: String,
    pub identity: String,
    pub node_ids: Vec<String>,
    /// linear | hold.
    pub interpolation: String,
    pub extra: Map<String, Value>,
}

impl Series {
    pub fn json(&self) -> Value {
        let mut v = json!({"t": self.t, "values": self.values, "unit": self.unit, "identity": self.identity, "node_ids": self.node_ids, "interpolation": self.interpolation});
        for (k, x) in &self.extra {
            v[k.as_str()] = x.clone();
        }
        v
    }
}

fn floats(v: &Value) -> Vec<f64> {
    v.as_array().into_iter().flatten().filter_map(Value::as_f64).collect()
}

fn dedup(ids: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in ids {
        if !id.is_empty() && !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

fn strs(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_string)).collect()
}

/// Every signal of a result: component traces, joint angles, motor blocks,
/// flex boundaries and the controller's frames, checked finite and increasing.
pub fn signals(result: &Value) -> Result<BTreeMap<String, Series>, String> {
    let mut out: BTreeMap<String, Series> = BTreeMap::new();
    let trace = &result["trace"];
    let times = floats(&trace["t"]);
    let mapping: BTreeMap<(String, String), &Value> = result["cad_mapping"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| ((m["section"].as_str().unwrap_or("links").to_string(), m["name"].as_str().unwrap_or("").to_string()), m))
        .collect();
    let add = |out: &mut BTreeMap<String, Series>, key: String, values: Vec<f64>, unit: &str, section: Option<&str>, name: Option<&str>, t: Option<Vec<f64>>, interpolation: &str| {
        let m = section.zip(name).and_then(|(s, n)| mapping.get(&(s.to_string(), n.to_string())).copied());
        let mut identity = key.clone();
        let mut node_ids = Vec::new();
        if let Some(m) = m {
            if let Some(id) = m["id"].as_str().filter(|s| !s.is_empty()) {
                let role = if key.starts_with("controller/") { key.split('/').nth(1).unwrap_or("plant") } else { "plant" };
                let quantity = key.rsplit('/').next().unwrap_or("").rsplit('.').next().unwrap_or("");
                identity = format!("{role}/{}/{id}/{quantity}", section.unwrap_or(""));
            }
            node_ids = dedup(m["id"].as_str().map(str::to_string).into_iter().chain(strs(&m["members"])).chain(strs(&m["related_ids"])));
        }
        out.insert(key, Series { t: t.unwrap_or_else(|| times.clone()), values, unit: unit.to_string(), identity, node_ids, interpolation: interpolation.to_string(), extra: Map::new() });
    };
    let by_length = |key: &str| {
        let mut v: Vec<Value> = result[key].as_array().cloned().unwrap_or_default();
        v.sort_by_key(|m| std::cmp::Reverse(m["native_name"].as_str().map_or(0, str::len)));
        v
    };
    let graph = by_length("component_graph_mapping");
    let script = by_length("script_component_mapping");
    for (name, values) in trace["signals"].as_object().into_iter().flatten() {
        let unit = result["signal_units"][name.as_str()].as_str().unwrap_or("unknown");
        add(&mut out, name.clone(), floats(values), unit, None, None, None, "linear");
        let starts = |m: &&Value| m["native_name"].as_str().is_some_and(|n| name.starts_with(&format!("{n}.")));
        let component = graph.iter().find(starts);
        let s = out.get_mut(name).expect("just added");
        if let Some(c) = component {
            let native = c["native_name"].as_str().unwrap_or("");
            s.extra.insert("component_id".into(), c["id"].clone());
            s.extra.insert("component_name".into(), c["name"].clone());
            s.identity = format!("component/{}/{}", c["id"].as_str().unwrap_or(""), &name[native.len() + 1..]);
            s.node_ids = c["body_id"].as_str().map(|b| vec![b.to_string()]).unwrap_or_default();
        }
        if let Some(d) = component.or_else(|| script.iter().find(starts))
            && let Some(source) = d["source"].as_str()
        {
            s.extra.insert("source".into(), json!({"path": source, "line": d["line"].as_u64().unwrap_or(1).max(1), "column": d["column"].as_u64().unwrap_or(1).max(1)}));
        }
    }
    for (name, values) in trace["joints"].as_object().into_iter().flatten() {
        add(&mut out, format!("joints/{name}/angle"), floats(values), "rad", Some("joints"), Some(name), None, "linear");
    }
    for (name, block) in trace["motors"].as_object().into_iter().flatten() {
        for (field, values) in block.as_object().into_iter().flatten() {
            let unit = match field.as_str() {
                "current" => "A",
                "winding_c" => "°C",
                "torque_nm" => "N·m",
                _ => "unknown",
            };
            add(&mut out, format!("motors/{name}/{field}"), floats(values), unit, Some("motors"), Some(name), None, "linear");
        }
    }
    for (name, boundaries) in trace["flex"].as_object().into_iter().flatten() {
        for (bi, b) in boundaries.as_array().into_iter().flatten().enumerate() {
            let points = b["point_m"].as_array().cloned().unwrap_or_default();
            let displacements: Vec<[f64; 3]> = b["displacement_m"].as_array().into_iter().flatten().map(vector).collect::<Result<_, _>>().map_err(|_| format!("Invalid flex vector for {name}"))?;
            if points.len() != times.len() || displacements.len() != times.len() {
                return Err(format!("Missing synchronized flex samples for {name}"));
            }
            for p in &points {
                vector(p).map_err(|_| format!("Invalid flex vector for {name}"))?;
            }
            let prefix = format!("flex/{name}/{bi}:{}", b["name"].as_str().unwrap_or(""));
            let boundary_identity = b["id"].as_str().map(str::to_string).unwrap_or_else(|| format!("{bi}:{}", b["name"].as_str().unwrap_or("")));
            for (axis, field) in ["dx", "dy", "dz", "magnitude"].iter().enumerate() {
                let values: Vec<f64> = displacements.iter().map(|v| if axis < 3 { v[axis] } else { (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt() }).collect();
                let key = format!("{prefix}/{field}");
                add(&mut out, key.clone(), values, "m", Some("links"), Some(name), None, "linear");
                let s = out.get_mut(&key).expect("just added");
                s.identity += &format!("/flex/{boundary_identity}");
            }
        }
    }
    let frames = result["controller_frames"].as_array().cloned().unwrap_or_default();
    let contract = &result["controller_contract"];
    for (section, channels) in [("sensors", &contract["sensors"]), ("commands", &contract["actuators"])] {
        for channel in channels.as_array().into_iter().flatten() {
            let name = channel["name"].as_str().unwrap_or("");
            let component = name.rsplit_once('.').map_or(name, |(a, _)| a);
            let cad_section = if mapping.contains_key(&("motors".to_string(), component.to_string())) { "motors" } else { "joints" };
            let values: Vec<f64> = frames.iter().map(|f| f[section][name].as_f64().unwrap_or(f64::NAN)).collect();
            let t: Vec<f64> = frames.iter().map(|f| f["t"].as_f64().unwrap_or(f64::NAN)).collect();
            add(&mut out, format!("controller/{section}/{name}"), values, channel["unit"].as_str().unwrap_or("unknown"), Some(cad_section), Some(component), Some(t), "hold");
        }
    }
    for (name, s) in &out {
        if s.t.len() != s.values.len() || !s.t.iter().chain(&s.values).all(|v| v.is_finite()) {
            return Err(format!("Invalid or non-finite samples in {name}"));
        }
        if s.t.windows(2).any(|w| w[1] <= w[0]) {
            return Err(format!("Non-increasing sample times in {name}"));
        }
    }
    Ok(out)
}

fn vector(v: &Value) -> Result<[f64; 3], ()> {
    let a = v.as_array().filter(|a| a.len() == 3).ok_or(())?;
    let mut out = [0.0; 3];
    for (i, x) in a.iter().enumerate() {
        out[i] = x.as_f64().filter(|f| f.is_finite() && !x.is_boolean()).ok_or(())?;
    }
    Ok(out)
}

/// The value at `time` inside the recorded interval (never extrapolated).
pub fn value_at(s: &Series, time: f64) -> Result<f64, String> {
    if s.t.is_empty() || time < s.t[0] || time > s.t[s.t.len() - 1] {
        return Err("Time is outside the recorded signal interval".into());
    }
    let i = s.t.partition_point(|x| *x <= time).saturating_sub(1).min(s.t.len() - 1);
    if i == s.t.len() - 1 || s.interpolation == "hold" {
        return Ok(s.values[i]);
    }
    let f = (time - s.t[i]) / (s.t[i + 1] - s.t[i]);
    Ok(s.values[i] + f * (s.values[i + 1] - s.values[i]))
}

/// The recorded sample nearest `time`.
pub fn sample_index(times: &[f64], time: f64) -> Result<usize, String> {
    if times.is_empty() {
        return Err("Run contains no replay samples".into());
    }
    let i = times.partition_point(|x| *x < time).min(times.len() - 1);
    Ok(if i > 0 && (times[i - 1] - time).abs() <= (times[i] - time).abs() { i - 1 } else { i })
}

/// World-mm delta matrices at sample `index`, for every member of each link.
pub fn replay_matrices(result: &Value, index: usize) -> Result<BTreeMap<String, Value>, String> {
    let times = floats(&result["trace"]["t"]);
    if index >= times.len() {
        return Err("Replay sample index is out of range".into());
    }
    let mut out = BTreeMap::new();
    for link in result["cad_mapping"].as_array().into_iter().flatten() {
        if link["section"].as_str().unwrap_or("links") != "links" {
            continue;
        }
        let name = link["name"].as_str().unwrap_or("");
        let frames = result["trace"]["poses"][name].as_array().cloned().unwrap_or_default();
        if frames.len() != times.len() {
            return Err(format!("Missing synchronized poses for {name}"));
        }
        let m = &frames[index];
        let ok = m.as_array().is_some_and(|rows| rows.len() == 4 && rows.iter().all(|r| r.as_array().is_some_and(|c| c.len() == 4 && c.iter().all(|x| x.as_f64().is_some_and(f64::is_finite)))));
        if !ok {
            return Err(format!("Invalid pose for {name}"));
        }
        for nid in link["id"].as_str().map(str::to_string).into_iter().chain(strs(&link["members"])) {
            out.insert(nid, m.clone());
        }
    }
    Ok(out)
}

/// World-mm flex boundary arrows at sample `index` (display only).
pub fn replay_flex(result: &Value, index: usize, scale: f64) -> Result<Vec<Value>, String> {
    if !(scale.is_finite() && scale > 0.0) {
        return Err("Flex display scale must be finite and positive".into());
    }
    let times = floats(&result["trace"]["t"]);
    if index >= times.len() {
        return Err("Replay sample index is out of range".into());
    }
    let links: BTreeMap<&str, &Value> = result["cad_mapping"].as_array().into_iter().flatten().filter(|m| m["section"].as_str().unwrap_or("links") == "links").filter_map(|m| m["name"].as_str().map(|n| (n, m))).collect();
    let mut out = Vec::new();
    for (name, boundaries) in result["trace"]["flex"].as_object().into_iter().flatten() {
        let link = links.get(name.as_str());
        for (bi, b) in boundaries.as_array().into_iter().flatten().enumerate() {
            let (pts, ds) = (b["point_m"].as_array().cloned().unwrap_or_default(), b["displacement_m"].as_array().cloned().unwrap_or_default());
            if pts.len() != times.len() || ds.len() != times.len() {
                return Err(format!("Missing synchronized flex samples for {name}"));
            }
            let p = vector(&pts[index]).map_err(|_| format!("Invalid flex vector for {name}"))?;
            let d = vector(&ds[index]).map_err(|_| format!("Invalid flex vector for {name}"))?;
            let ids = link.map(|l| dedup(l["id"].as_str().map(str::to_string).into_iter().chain(strs(&l["members"])))).unwrap_or_default();
            let point_mm = p.map(|v| v * 1000.0);
            let tip_mm: [f64; 3] = std::array::from_fn(|i| (p[i] + scale * d[i]) * 1000.0);
            out.push(json!({
                "name": format!("{name}/{}", b["name"].as_str().unwrap_or("")), "boundary": bi, "node_ids": ids,
                "point_mm": point_mm, "tip_mm": tip_mm, "displacement_m": d,
            }));
        }
    }
    Ok(out)
}

/// Explicit units and sample-window reductions (sample statistics, time in seconds).
pub fn evaluate_expectations(result: &Value, expectations: &Value) -> Result<Value, String> {
    let catalogue = signals(result)?;
    let list = expectations.as_array().ok_or("expectations must be an array")?;
    let mut metrics = Vec::new();
    for spec in list {
        let allowed = ["name", "signal", "unit", "reduction", "start", "end", "target", "min", "max"];
        let m = spec.as_object().filter(|m| m.keys().all(|k| allowed.contains(&k.as_str()))).ok_or("Unknown expectation field")?;
        let name = m.get("name").or(m.get("signal")).and_then(Value::as_str).unwrap_or("expectation").to_string();
        let signal = m.get("signal").and_then(Value::as_str).unwrap_or("");
        let series = catalogue.get(signal).ok_or_else(|| format!("{name}: unknown signal {signal}"))?;
        if m.get("unit").and_then(Value::as_str) != Some(series.unit.as_str()) || series.unit == "unknown" {
            return Err(format!("{name}: expected explicit unit {}", series.unit));
        }
        let num = |k: &str| -> Result<Option<f64>, String> {
            match m.get(k) {
                None => Ok(None),
                Some(v) => v.as_f64().filter(|x| x.is_finite() && !v.is_boolean()).map(Some).ok_or_else(|| format!("{name}: {k} must be finite")),
            }
        };
        let (start, end, target, min, max) = (num("start")?, num("end")?, num("target")?, num("min")?, num("max")?);
        if (min.is_none() && max.is_none()) || min.unwrap_or(f64::NEG_INFINITY) > max.unwrap_or(f64::INFINITY) {
            return Err(format!("{name}: provide valid min/max bounds"));
        }
        let (lo, hi) = (start.unwrap_or(f64::NEG_INFINITY), end.unwrap_or(f64::INFINITY));
        let y: Vec<f64> = series.t.iter().zip(&series.values).filter(|(t, _)| lo <= **t && **t <= hi).map(|(_, v)| *v).collect();
        if y.is_empty() {
            return Err(format!("{name}: expectation window contains no samples"));
        }
        let n = y.len() as f64;
        let reduction = m.get("reduction").and_then(Value::as_str).unwrap_or("max_abs");
        let value = match reduction {
            "rmse" => {
                let target = target.ok_or_else(|| format!("{name}: rmse requires a target"))?;
                (y.iter().map(|v| (v - target).powi(2)).sum::<f64>() / n).sqrt()
            }
            "max_abs" => y.iter().map(|v| v.abs()).fold(f64::NEG_INFINITY, f64::max),
            "max" => y.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            "min" => y.iter().copied().fold(f64::INFINITY, f64::min),
            "final" => y[y.len() - 1],
            "mean" => y.iter().sum::<f64>() / n,
            "rms" => (y.iter().map(|v| v * v).sum::<f64>() / n).sqrt(),
            other => return Err(format!("{name}: unknown reduction {other}")),
        };
        let mut metric = spec.clone();
        metric["name"] = json!(name);
        metric["reduction"] = json!(reduction);
        metric["value"] = json!(value);
        metric["samples"] = json!(y.len());
        metric["passed"] = json!(min.unwrap_or(f64::NEG_INFINITY) <= value && value <= max.unwrap_or(f64::INFINITY));
        metrics.push(metric);
    }
    let status = if metrics.is_empty() { "unchecked" } else if metrics.iter().all(|m| m["passed"] == true) { "passed" } else { "failed" };
    Ok(json!({"status": status, "convention": "sample statistics; time in seconds", "metrics": metrics}))
}

/// Common signals aligned to the candidate's samples over the shared interval.
pub fn compare(baseline: &Value, candidate: &Value) -> Result<Value, String> {
    if baseline["preflight"] == true || candidate["preflight"] == true {
        return Err("Build-only checks have no simulation samples to compare".into());
    }
    let (a, b) = (signals(baseline)?, signals(candidate)?);
    let by_identity: BTreeMap<&str, (&String, &Series)> = a.iter().map(|(k, s)| (s.identity.as_str(), (k, s))).collect();
    let mut differences = Map::new();
    for (key, right) in &b {
        let Some((baseline_key, left)) = by_identity.get(right.identity.as_str()) else { continue };
        if left.unit != right.unit || left.unit == "unknown" {
            differences.insert(key.clone(), json!({"comparable": false, "reason": "units differ or are unknown"}));
            continue;
        }
        if left.t.is_empty() || right.t.is_empty() {
            continue;
        }
        let t: Vec<f64> = right.t.iter().copied().filter(|v| left.t[0] <= *v && *v <= left.t[left.t.len() - 1]).collect();
        let delta: Vec<f64> = t.iter().map(|v| Ok(value_at(right, *v)? - value_at(left, *v)?)).collect::<Result<_, String>>()?;
        let rms = (!delta.is_empty()).then(|| (delta.iter().map(|v| v * v).sum::<f64>() / delta.len() as f64).sqrt());
        let max_abs = (!delta.is_empty()).then(|| delta.iter().map(|v| v.abs()).fold(0.0, f64::max));
        differences.insert(key.clone(), json!({"comparable": !t.is_empty(), "unit": right.unit, "t": t, "delta": delta, "baseline_signal": baseline_key, "identity": right.identity, "rms_delta": rms, "max_abs_delta": max_abs}));
    }
    let (pa, pb) = (&baseline["provenance"], &candidate["provenance"]);
    let mut changed = Map::new();
    for field in ["source_hash", "parameters_hash", "controller_hash", "physical_hash", "component_graph_hash", "cad_derivation_hash", "binary_hash", "seed"] {
        if pa[field] != pb[field] {
            changed.insert(field.into(), json!({"baseline": pa[field], "candidate": pb[field]}));
        }
    }
    let same_scenario = ["source_hash", "parameters_hash", "seed"].iter().all(|k| pa[*k] == pb[*k]);
    let same_settings = baseline["settings"] == candidate["settings"];
    let same_interface = baseline["controller_interface"] == candidate["controller_interface"];
    let mut objectives = Map::new();
    for (name, right) in candidate["objectives"].as_object().into_iter().flatten() {
        let Some(left) = baseline["objectives"].get(name) else { continue };
        let comparable = left["unit"] == right["unit"] && left.get("definition") == right.get("definition");
        let delta = if comparable { right["value"].as_f64().zip(left["value"].as_f64()).map(|(r, l)| r - l) } else { None };
        objectives.insert(name.clone(), json!({"comparable": comparable, "unit": right["unit"], "baseline": left["value"], "candidate": right["value"], "delta": delta, "reason": if comparable { Value::Null } else { json!("Units or objective definitions differ") }}));
    }
    let mut caveats = Vec::new();
    if !same_scenario {
        caveats.push("System source, scenario parameters or seed differ");
    }
    if !same_settings {
        caveats.push("Simulation settings differ");
    }
    if !same_interface {
        caveats.push("Controller interfaces differ");
    }
    Ok(json!({
        "baseline": baseline["run_id"], "candidate": candidate["run_id"],
        "same_scenario": same_scenario, "same_settings": same_settings, "same_controller_interface": same_interface,
        "caveats": caveats,
        "alignment": "candidate sample times over shared interval; linear plant traces, held controller samples",
        "changed_inputs": changed, "signals": differences, "objectives": objectives,
        "baseline_evaluation": baseline["evaluation"], "candidate_evaluation": candidate["evaluation"],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(values: Vec<f64>) -> Value {
        json!({"run_id": "r", "trace": {"t": [0.0, 0.1, 0.2], "joints": {"hip": values}}, "cad_mapping": [{"section": "joints", "name": "hip", "id": "j1"}], "provenance": {}})
    }

    #[test]
    fn expectations_reduce_a_window_and_name_their_units() {
        let r = result(vec![0.0, 0.5, 1.0]);
        let e = evaluate_expectations(&r, &json!([{"name": "reach", "signal": "joints/hip/angle", "unit": "rad", "reduction": "final", "min": 0.9}])).unwrap();
        assert_eq!(e["status"], "passed");
        assert_eq!(e["metrics"][0]["value"], 1.0);
        assert!(evaluate_expectations(&r, &json!([{"signal": "joints/hip/angle", "unit": "m", "max": 1}])).unwrap_err().contains("expected explicit unit rad"));
        let s = &signals(&r).unwrap()["joints/hip/angle"];
        assert_eq!(s.identity, "plant/joints/j1/angle");
        assert!((value_at(s, 0.05).unwrap() - 0.25).abs() < 1e-12);
    }

    #[test]
    fn a_comparison_aligns_by_identity() {
        let c = compare(&result(vec![0.0, 0.5, 1.0]), &result(vec![0.0, 0.6, 1.0])).unwrap();
        let d = &c["signals"]["joints/hip/angle"];
        assert_eq!(d["comparable"], true);
        assert!((d["max_abs_delta"].as_f64().unwrap() - 0.1).abs() < 1e-12);
        assert_eq!(c["same_scenario"], true);
    }
}
