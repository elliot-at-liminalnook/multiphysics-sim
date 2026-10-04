//! The controller leaderboard: reproducible controller evaluations
//! (`web/leaderboard/evaluations.json`), validated, gated and ranked exactly as
//! the browser's `web/viewer/leaderboard-model.mjs` does, so every viewer reads
//! one catalog with one rule. Missing evidence never satisfies a gate;
//! physical travel and compute throughput are distinct; speed ranks are
//! assigned only within identical comparison groups. Each entry is a tested
//! recipe (scene, config, task, seed and the evaluated input events) that a
//! viewer can load and run, or replay exactly ([`tested_preset`],
//! [`replay_recording`]); [`verify_sources`] checks the recipe and evidence
//! files against their recorded sha256 first.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Where the catalog lives, relative to the workspace root.
pub const CATALOG: &str = "web/leaderboard/evaluations.json";
/// The gates every validated entry passes (`leaderboard-model.mjs` requiredGates).
pub const REQUIRED_GATES: [&str; 8] = ["sustained_walk", "turn_reverse_stop", "disturbances", "terrain", "contact_motion", "numerical_accuracy", "browser_realtime", "replay_parity"];
/// The gates' display names (`leaderboard.js` labels).
pub const GATE_LABELS: [(&str, &str); 8] = [
    ("sustained_walk", "Sustained walk"),
    ("turn_reverse_stop", "Turning, reverse, stop"),
    ("disturbances", "Held-out disturbances"),
    ("terrain", "Harder terrain"),
    ("contact_motion", "Loaded contact motion"),
    ("numerical_accuracy", "Numerical accuracy"),
    ("browser_realtime", "Browser realtime"),
    ("replay_parity", "Replay and host parity"),
];
pub const ELIGIBLE_RULE: &str = "validated for its comparison group = every required gate passes (a missing or failed gate never does), the task passed, a finite non-negative sustained speed, at least 60 s simulated and a speed window of at least 30 s; speed ranks are assigned only within one comparison group (identical model, environment, fidelity and benchmark), fastest sustained speed first, ties by id";

/// The catalog as written.
pub struct Catalog {
    pub path: PathBuf,
    pub scope: Value,
    pub generator: Value,
    pub entries: Vec<Value>,
}

/// Reads and validates the catalog (version 1); every entry must pass [`validate_entry`].
pub fn read(path: &Path) -> Result<Catalog, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if v["version"] != 1 {
        return Err(format!("{}: unsupported evaluation catalog version {}", path.display(), v["version"]));
    }
    let entries = v["entries"].as_array().ok_or_else(|| format!("{}: no entries array", path.display()))?.clone();
    for (i, e) in entries.iter().enumerate() {
        validate_entry(e).map_err(|why| format!("{}: entries[{i}] ({}): {why}", path.display(), e["id"].as_str().unwrap_or("?")))?;
    }
    Ok(Catalog { path: path.to_path_buf(), scope: v["scope"].clone(), generator: v["generator"].clone(), entries })
}

fn is_hash(v: &Value) -> bool {
    v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}
fn is_source(v: &Value) -> bool {
    v["path"].as_str().is_some_and(|p| !p.is_empty()) && is_hash(&v["sha256"])
}
fn finite(v: &Value) -> Option<f64> {
    v.as_f64().filter(|x| x.is_finite())
}
fn safe_u64(v: &Value) -> Option<u64> {
    v.as_u64().filter(|x| *x <= (1u64 << 53) - 1)
}

/// `leaderboard-model.mjs` validateEntry: the identity, recipe sources,
/// hashes, metrics, gates, replay events and evidence an entry must carry.
pub fn validate_entry(e: &Value) -> Result<(), String> {
    let id = e["id"].as_str().unwrap_or("");
    let bad = |what: &str| Err(format!("invalid controller evaluation entry: {what}"));
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') {
        return bad("id must be lowercase letters, digits and dashes");
    }
    for k in ["name", "description", "comparison_group"] {
        if e[k].as_str().is_none_or(str::is_empty) {
            return bad(&format!("{k} is required"));
        }
    }
    let load = &e["load"];
    if !is_hash(&load["asset_sha256"]) || safe_u64(&load["seed"]).is_none() || !["scene", "config", "task"].iter().all(|k| is_source(&load[*k])) {
        return bad("load needs asset_sha256, a non-negative integer seed and scene/config/task sources {path, sha256}");
    }
    if !["controller_sha256", "model_sha256", "environment_sha256", "fidelity_sha256", "cad_sha256"].iter().all(|k| is_hash(&e[*k])) || e["benchmark_version"].is_null() {
        return bad("controller, model, environment, fidelity and CAD sha256 and benchmark_version are required");
    }
    let m = &e["metrics"];
    if finite(&m["simulated_s"]).is_none_or(|s| s <= 0.0) || finite(&m["speed_window_s"]).is_none_or(|s| s < 0.0) {
        return bad("metrics need positive simulated_s and non-negative speed_window_s");
    }
    if !(m["sustained_speed_m_s"].is_null() || finite(&m["sustained_speed_m_s"]).is_some_and(|s| s >= 0.0)) {
        return bad("metrics.sustained_speed_m_s must be null or a non-negative number");
    }
    for g in REQUIRED_GATES {
        let gate = &e["gates"][g];
        if !["pass", "fail", "missing"].contains(&gate["status"].as_str().unwrap_or("")) || gate["detail"].as_str().is_none_or(str::is_empty) {
            return bad(&format!("gate {g} needs a status (pass | fail | missing) and a detail"));
        }
    }
    let replay = &e["replay"];
    let completed = safe_u64(&replay["completed_steps"]).filter(|n| *n > 0).ok_or("invalid controller evaluation entry: replay.completed_steps must be a positive integer")?;
    let events = replay["input_events"].as_array().ok_or("invalid controller evaluation entry: replay.input_events must be an array")?;
    let mut previous: Option<u64> = None;
    for ev in events {
        let at = safe_u64(&ev["at_step"]).filter(|s| *s < completed).ok_or("invalid controller evaluation entry: an input event's at_step must be an integer below completed_steps")?;
        if previous.is_some_and(|p| at <= p) {
            return bad("input events must be in strictly increasing at_step order");
        }
        previous = Some(at);
        if !ev["values"].as_array().is_some_and(|v| !v.is_empty() && v.iter().all(|x| finite(x).is_some())) {
            return bad("each input event needs a non-empty list of finite values");
        }
    }
    if !e["evidence"].as_array().is_some_and(|a| !a.is_empty() && a.iter().all(is_source)) {
        return bad("evidence must be a non-empty list of sources {path, sha256}");
    }
    if let Some(r) = e.get("command_response").filter(|r| !r.is_null()) {
        let cases = r["cases"].as_array();
        let duration = |v: &Value| v.is_null() || finite(v).is_some_and(|x| x >= 0.0);
        let ok = r["method"].as_str().is_some() && r["scope"].as_str().is_some() && cases.is_some_and(|c| {
            let mut seen = std::collections::BTreeSet::new();
            !c.is_empty() && c.iter().all(|c| {
                c["command"].as_str().is_some_and(|k| seen.insert(k.to_string()))
                    && ["simulated_response_s", "received_wall_s", "drawn_wall_s"].iter().all(|k| duration(&c[*k]))
                    && finite(&c["threshold"]).is_some_and(|t| t > 0.0)
                    && ["m", "rad"].contains(&c["threshold_unit"].as_str().unwrap_or(""))
                    && finite(&c["hold_s"]).is_some_and(|h| h > 0.0)
            })
        });
        if !ok {
            return Err("invalid command response evidence".into());
        }
    }
    Ok(())
}

/// `leaderboard-model.mjs` eligible (ELIGIBLE_RULE).
pub fn eligible(e: &Value) -> bool {
    let m = &e["metrics"];
    REQUIRED_GATES.iter().all(|g| e["gates"][*g]["status"] == "pass")
        && m["task_passed"] == true
        && finite(&m["sustained_speed_m_s"]).is_some_and(|s| s >= 0.0)
        && finite(&m["simulated_s"]).is_some_and(|s| s >= 60.0)
        && finite(&m["speed_window_s"]).is_some_and(|s| s >= 30.0)
}

/// `leaderboard-model.mjs` rankEntries: rank by id within each comparison group (ELIGIBLE_RULE).
pub fn rank_entries(entries: &[Value]) -> BTreeMap<String, usize> {
    let mut groups: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
    for e in entries.iter().filter(|e| eligible(e)) {
        groups.entry(e["comparison_group"].as_str().unwrap_or("").to_string()).or_default().push(e);
    }
    let mut ranks = BTreeMap::new();
    for group in groups.values_mut() {
        group.sort_by(|a, b| {
            let (sa, sb) = (a["metrics"]["sustained_speed_m_s"].as_f64().unwrap_or(0.0), b["metrics"]["sustained_speed_m_s"].as_f64().unwrap_or(0.0));
            sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a["id"].as_str().unwrap_or("").cmp(b["id"].as_str().unwrap_or("")))
        });
        for (i, e) in group.iter().enumerate() {
            ranks.insert(e["id"].as_str().unwrap_or("").to_string(), i + 1);
        }
    }
    ranks
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(&bytes)))
}

/// The recipe (scene, config, task) and evidence files of `e` under `root`
/// match their recorded sha256 (as `web/leaderboard/package.mjs` asserts
/// before packaging); a stale file is named.
pub fn verify_sources(root: &Path, e: &Value) -> Result<(), String> {
    let load = &e["load"];
    let sources = ["scene", "config", "task"].iter().map(|k| &load[*k]).chain(e["evidence"].as_array().into_iter().flatten());
    for s in sources {
        let rel = s["path"].as_str().unwrap_or("");
        let actual = sha256_file(&root.join(rel))?;
        if Some(actual.as_str()) != s["sha256"].as_str() {
            return Err(format!("stale leaderboard evidence or recipe: {rel} has sha256 {actual}, the evaluation recorded {}", s["sha256"].as_str().unwrap_or("?")));
        }
    }
    Ok(())
}

/// The tested recipe as a preset entry (`package.mjs`: id `tested-<id>`,
/// mode embedded, the load's scene/config/task and seed), with the entry's
/// description and limitations as its evidence text.
pub fn tested_preset(e: &Value) -> Value {
    let load = &e["load"];
    json!({"id": format!("tested-{}", e["id"].as_str().unwrap_or("")), "label": format!("Tested recipe · {}", e["name"].as_str().unwrap_or("")), "mode": "embedded",
        "scene": load["scene"]["path"], "config": load["config"]["path"], "task": load["task"]["path"], "seed": load["seed"], "asset_sha256": load["asset_sha256"],
        "description": e["description"], "readiness": "Experimental evaluation · inspect the controller leaderboard for failed and missing gates.", "evidence": e["limitations"],
        "leaderboard_entry": e["id"]})
}

/// The evaluated input events as the environment recording the browser
/// replays (`viewer.js` "Replay tested inputs": version 1,
/// `sampled_environment_recording`, the recipe's task and the runtime's
/// version-3 `embedded_session` with the entry's seed, completed steps and
/// input events), typed by the shared recording types.
pub fn replay_recording(e: &Value, scene: crate::session::Scene, config: crate::embedded::Config, task: crate::environment::Task) -> Result<crate::environment::EnvironmentRecording, String> {
    let replay = &e["replay"];
    let input_events = serde_json::from_value(replay["input_events"].clone()).map_err(|err| format!("leaderboard entry {}: replay.input_events: {err}", e["id"]))?;
    Ok(crate::environment::EnvironmentRecording {
        version: 1,
        kind: "sampled_environment_recording".into(),
        task,
        error: None,
        runtime: crate::embedded::EmbeddedRecording {
            version: 3,
            kind: "embedded_session".into(),
            runtime_identity: None,
            scene,
            config,
            seed: e["load"]["seed"].as_u64().unwrap_or(0),
            completed_steps: replay["completed_steps"].as_u64().unwrap_or(0) as usize,
            failure: None,
            input_events,
        },
    })
}

/// The values the evaluated run starts with (the input event at step 0), when it declares one.
pub fn initial_inputs(e: &Value) -> Option<Vec<f64>> {
    e["replay"]["input_events"].as_array()?.iter().find(|ev| ev["at_step"] == 0)?["values"].as_array()?.iter().map(Value::as_f64).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, group: &str, speed: Option<f64>, all_pass: bool) -> Value {
        let gates: serde_json::Map<String, Value> = REQUIRED_GATES.iter().map(|g| (g.to_string(), json!({"status": if all_pass { "pass" } else { "missing" }, "detail": "d"}))).collect();
        let h = "a".repeat(64);
        json!({"id": id, "name": id, "description": "x", "comparison_group": group,
            "load": {"asset_sha256": h, "seed": 0, "scene": {"path": "s", "sha256": h}, "config": {"path": "c", "sha256": h}, "task": {"path": "t", "sha256": h}},
            "controller_sha256": h, "model_sha256": h, "environment_sha256": h, "fidelity_sha256": h, "cad_sha256": h, "benchmark_version": "1",
            "metrics": {"simulated_s": 60.0, "speed_window_s": 30.0, "sustained_speed_m_s": speed, "task_passed": true},
            "gates": gates, "replay": {"completed_steps": 10, "input_events": [{"at_step": 0, "values": [0.0, 1.0]}]}, "evidence": [{"path": "e", "sha256": h}]})
    }

    #[test]
    fn entries_validate_rank_within_groups_and_missing_gates_never_pass() {
        let a = entry("fast", "g1", Some(0.02), true);
        let b = entry("slow", "g1", Some(0.01), true);
        let c = entry("other", "g2", Some(0.05), true);
        let d = entry("gated", "g1", Some(0.09), false);
        for e in [&a, &b, &c, &d] {
            validate_entry(e).unwrap();
        }
        assert!(eligible(&a) && !eligible(&d), "a missing gate never satisfies it");
        let ranks = rank_entries(&[a.clone(), b, c, d]);
        assert_eq!((ranks["fast"], ranks["slow"], ranks["other"]), (1, 2, 1));
        assert!(!ranks.contains_key("gated"));
        assert_eq!(initial_inputs(&a), Some(vec![0.0, 1.0]));
        assert_eq!(tested_preset(&a)["id"], "tested-fast");
        let mut bad = a;
        bad["replay"]["input_events"] = json!([{"at_step": 3, "values": [1.0]}, {"at_step": 2, "values": [1.0]}]);
        assert!(validate_entry(&bad).unwrap_err().contains("increasing"));
    }

    #[test]
    fn the_repository_catalog_reads() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let catalog = read(&root.join(CATALOG)).unwrap();
        assert!(!catalog.entries.is_empty());
    }
}
