//! Acceptance evidence for systems: a test (`SystemDocument::tests`) run on
//! the system as composed (its own controller blocks, no substitutes, no
//! privileged signals: requirements read the same observables anyone can),
//! each requirement judged pass, fail or not assessed, and the result bound
//! to a fingerprint of exactly what ran:
//!
//! - `model`: the document's physics hash (no display-only content);
//! - `artifacts`: the SHA-256 of every file the run read (FMU archives,
//!   generated assemblies' sources), by instance path;
//! - `settings`: the run settings and the test duration;
//! - `test`: the test itself.
//!
//! Evidence whose fingerprint no longer matches is stale and says what
//! changed; a test never run is not assessed. A requirement the run cannot
//! judge (the observable is missing, the run stopped before its window) is
//! never a pass.
use crate::system_builder;
use serde::{Deserialize, Serialize};
use sim_core::BehaviorRegistry;
use sim_system::{InstanceKind, Reduce, SystemDocument, SystemTest};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const SCHEMA: &str = "sim.system-evidence/1";
pub const VERDICT_RULE: &str = "passed only when every requirement passed; failed when any failed; otherwise incomplete (a requirement the run could not judge is never a pass)";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fingerprint {
    pub model: String,
    pub artifacts: BTreeMap<String, String>,
    pub settings: String,
    pub test: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pass,
    Fail,
    NotAssessed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Passed,
    Failed,
    Incomplete,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequirementResult {
    pub id: String,
    pub description: String,
    pub status: Status,
    /// The reduced value the judgement is on (None: not assessed).
    pub measured: Option<f64>,
    pub detail: String,
}

/// One test's evidence.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub schema: String,
    pub test: String,
    pub fingerprint: Fingerprint,
    pub verdict: Verdict,
    pub results: Vec<RequirementResult>,
    /// Why the run stopped early, when it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_error: Option<String>,
}

/// Whether evidence still describes the system.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Standing {
    /// Never run (or its evidence was removed).
    NotAssessed,
    /// Its fingerprint matches what the system is now.
    Current { verdict: Verdict },
    /// Something it ran has changed since: what.
    Stale { verdict: Verdict, changed: Vec<String> },
}

fn hash(value: &impl Serialize) -> String {
    blake3::hash(&serde_json::to_vec(&serde_json::to_value(value).expect("serializes")).expect("serializes")).to_hex().to_string()
}

fn sha256_file(path: &Path) -> String {
    std::fs::read(path).map(|b| sim_fmi::sha256_hex(&b)).unwrap_or_else(|e| format!("unreadable: {e}"))
}

/// Every file a run of `document` reads, by instance path (all levels).
fn artifacts(document: &SystemDocument, base: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    fn walk(document: &SystemDocument, base: &Path, definition: &str, path: &str, out: &mut BTreeMap<String, String>, depth: usize) {
        let Some(d) = document.definitions.get(definition) else { return };
        if depth > 64 {
            return;
        }
        for (name, instance) in &d.instances {
            let here = sim_system::join_path(path, name);
            match &instance.kind {
                InstanceKind::Block { implementation: sim_system::BlockSource::Fmu { path: fmu, .. }, .. } => {
                    out.insert(here, sha256_file(&base.join(fmu)));
                }
                InstanceKind::Generated { source, .. } => {
                    out.insert(here, sha256_file(&base.join(source)));
                }
                InstanceKind::Subsystem { definition } => walk(document, base, definition, &here, out, depth + 1),
                _ => {}
            }
        }
    }
    walk(document, base, &document.root, "", &mut out, 0);
    out
}

/// What `test` would run now.
pub fn fingerprint(document: &SystemDocument, base: &Path, test: &str) -> Result<Fingerprint, String> {
    let t = document.tests.get(test).ok_or_else(|| format!("no test `{test}`"))?;
    let config = system_builder::config_for(document);
    Ok(Fingerprint {
        model: document.physics_hash(),
        artifacts: artifacts(document, base),
        settings: hash(&(serde_json::to_value(&config).map_err(|e| e.to_string())?, t.duration_s)),
        test: hash(t),
    })
}

/// What differs between two fingerprints, in words.
pub fn changes(was: &Fingerprint, now: &Fingerprint) -> Vec<String> {
    let mut out = Vec::new();
    if was.model != now.model {
        out.push("the system's model changed (parts, parameters, wiring or block configuration)".to_owned());
    }
    for (path, sha) in &now.artifacts {
        match was.artifacts.get(path) {
            None => out.push(format!("`{path}` is new")),
            Some(old) if old != sha => out.push(format!("the artifact of `{path}` changed")),
            _ => {}
        }
    }
    for path in was.artifacts.keys().filter(|p| !now.artifacts.contains_key(*p)) {
        out.push(format!("`{path}` was removed"));
    }
    if was.settings != now.settings {
        out.push("the run settings changed".to_owned());
    }
    if was.test != now.test {
        out.push("the test changed".to_owned());
    }
    out
}

fn reduce(reduce: Reduce, times: &[f64], values: &[f64]) -> Option<f64> {
    let (first, last) = (*values.first()?, *values.last()?);
    Some(match reduce {
        Reduce::Final => last,
        Reduce::Mean => values.iter().sum::<f64>() / values.len() as f64,
        Reduce::Max => values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        Reduce::Min => values.iter().copied().fold(f64::INFINITY, f64::min),
        Reduce::Peak => values.iter().fold(0.0_f64, |m, v| m.max(v.abs())),
        Reduce::Change => last - first,
        Reduce::Integral => times.windows(2).zip(values.windows(2)).map(|(t, v)| 0.5 * (v[0] + v[1]) * (t[1] - t[0])).sum(),
    })
}

/// Judge every requirement of `test` on `series` (a run that reached `reached` s).
fn judge(test: &SystemTest, series: &[system_builder::Series], reached: f64, run_error: Option<&str>) -> Vec<RequirementResult> {
    test.requirements.iter().map(|r| {
        let description = if r.description.is_empty() {
            let bound = match (r.min, r.max) {
                (Some(a), Some(b)) => format!("within [{a}, {b}]"),
                (Some(a), None) => format!("at least {a}"),
                (None, Some(b)) => format!("at most {b}"),
                (None, None) => String::new(),
            };
            format!("{:?} of {} {}{}", r.reduce, r.observable, bound, r.window.map(|[a, b]| format!(" over {a}–{b} s")).unwrap_or_default()).to_lowercase()
        } else {
            r.description.clone()
        };
        let not_assessed = |detail: String| RequirementResult { id: r.id.clone(), description: description.clone(), status: Status::NotAssessed, measured: None, detail };
        let matching: Vec<&system_builder::Series> = series.iter().filter(|s| s.label == r.observable).collect();
        let s = match matching.as_slice() {
            [one] => *one,
            [] => return not_assessed(format!("no observable `{}` in this system", r.observable)),
            _ => return not_assessed(format!("`{}` names several observables", r.observable)),
        };
        let [from, to] = r.window.unwrap_or([0.0, test.duration_s]);
        if reached + 1e-9 < to {
            return not_assessed(format!("the run stopped at {reached:.3} s, before the window's end ({to} s){}", run_error.map(|e| format!(": {e}")).unwrap_or_default()));
        }
        let (times, values): (Vec<f64>, Vec<f64>) = s.times.iter().zip(&s.values).filter(|(t, _)| **t >= from - 1e-12 && **t <= to + 1e-12).map(|(t, v)| (*t, *v)).unzip();
        let Some(value) = reduce(r.reduce, &times, &values) else { return not_assessed(format!("no samples of `{}` in the window", r.observable)) };
        if !value.is_finite() {
            return not_assessed(format!("{:?} of `{}` is not finite", r.reduce, r.observable));
        }
        let ok = r.min.is_none_or(|m| value >= m) && r.max.is_none_or(|m| value <= m);
        RequirementResult { id: r.id.clone(), description, status: if ok { Status::Pass } else { Status::Fail }, measured: Some(value), detail: format!("{value} {}", s.unit) }
    }).collect()
}

/// Run `test` on the document stored in `base` and judge it.
pub fn assess(document: &SystemDocument, registry: &BehaviorRegistry, base: &Path, test: &str, cancel: Option<&std::sync::atomic::AtomicBool>) -> Result<Evidence, String> {
    let fingerprint = fingerprint(document, base, test)?;
    let t = &document.tests[test];
    let observables: Vec<String> = t.requirements.iter().map(|r| r.observable.clone()).collect();
    let config = system_builder::config_for(document);
    let (series, reached, run_error) = match system_builder::simulate_cancellable_at(document, registry, base, t.duration_s, config, &observables, cancel) {
        Ok(series) => (series, t.duration_s, None),
        Err(e) if e == system_builder::CANCELLED => return Err(e),
        Err(e) => (Vec::new(), 0.0, Some(e)),
    };
    // Keep only exact matches (the selection matches by substring).
    let series: Vec<system_builder::Series> = series.into_iter().filter(|s| observables.contains(&s.label)).collect();
    let results = judge(t, &series, reached, run_error.as_deref());
    let verdict = if results.iter().any(|r| r.status == Status::Fail) {
        Verdict::Failed
    } else if results.iter().all(|r| r.status == Status::Pass) {
        Verdict::Passed
    } else {
        Verdict::Incomplete
    };
    Ok(Evidence { schema: SCHEMA.into(), test: test.into(), fingerprint, verdict, results, run_error })
}

/// Where a system file's evidence is kept: `<stem>.evidence.json` beside it.
pub fn path_for(system: &Path) -> PathBuf {
    let name = system.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let stem = name.strip_suffix(".system.json").or_else(|| name.strip_suffix(".json")).unwrap_or(&name).to_owned();
    system.with_file_name(format!("{stem}.evidence.json"))
}

/// All kept evidence, by test name (empty when none).
pub fn load(system: &Path) -> Result<BTreeMap<String, Evidence>, String> {
    match std::fs::read(path_for(system)) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path_for(system).display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(e) => Err(format!("{}: {e}", path_for(system).display())),
    }
}

/// Keep `evidence` (replacing that test's previous evidence).
pub fn save(system: &Path, evidence: &Evidence) -> Result<(), String> {
    let mut all = load(system)?;
    all.insert(evidence.test.clone(), evidence.clone());
    let bytes = serde_json::to_vec_pretty(&all).map_err(|e| e.to_string())?;
    sim_system::store::write_atomic(&path_for(system), &bytes).map_err(|e| e.to_string())
}

/// Where every test of the system stands now.
pub fn standing(document: &SystemDocument, system: &Path) -> Result<BTreeMap<String, Standing>, String> {
    let base = system.parent().unwrap_or(Path::new("."));
    let kept = load(system)?;
    document.tests.keys().map(|name| {
        let now = fingerprint(document, base, name)?;
        let standing = match kept.get(name) {
            None => Standing::NotAssessed,
            Some(e) => {
                let changed = changes(&e.fingerprint, &now);
                if changed.is_empty() { Standing::Current { verdict: e.verdict } } else { Standing::Stale { verdict: e.verdict, changed } }
            }
        };
        Ok((name.clone(), standing))
    }).collect()
}
