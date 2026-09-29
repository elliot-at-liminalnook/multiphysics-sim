//! Run history: every run kept with the exact document, seed, settings and
//! results, so it can be compared with others and replayed. Records live in
//! `<system>.runs/` next to the system file (outputs, not sources: keep them
//! out of version control unless one becomes a baseline).
use crate::system_builder::{self, Series};
use crate::system_session::SessionConfig;
use serde::{Deserialize, Serialize};
use sim_core::BehaviorRegistry;
use sim_system::SystemDocument;
use std::path::{Path, PathBuf};

pub const SCHEMA: &str = "sim.run/1";
/// Points kept per series in a record.
const POINTS: usize = 2000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRecord {
    pub schema: String,
    pub id: String,
    /// Seconds since the Unix epoch.
    pub created: u64,
    pub note: String,
    /// Content hash of `document` (revision-independent).
    pub content_hash: String,
    pub document: SystemDocument,
    pub config: SessionConfig,
    pub duration: f64,
    pub series: Vec<Series>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunSummary {
    pub id: String,
    pub created: u64,
    pub note: String,
    pub revision: u64,
    pub content_hash: String,
    pub duration: f64,
    pub seed: u64,
    pub finals: Vec<(String, f64)>,
}

/// `<dir>/<stem>.runs` for a system file.
pub fn dir_for(system_path: &Path) -> PathBuf {
    let name = system_path.file_name().map(|n| n.to_string_lossy().trim_end_matches(".system.json").trim_end_matches(".json").to_string()).unwrap_or_else(|| "system".into());
    system_path.with_file_name(format!("{name}.runs"))
}

fn thin(mut s: Series) -> Series {
    if s.times.len() > POINTS {
        let step = s.times.len().div_ceil(POINTS);
        let keep = |v: &Vec<f64>| v.iter().step_by(step).copied().chain(v.last().copied()).collect::<Vec<_>>();
        s.times = keep(&s.times);
        s.values = keep(&s.values);
    }
    s
}

impl RunRecord {
    pub fn new(document: &SystemDocument, config: SessionConfig, duration: f64, series: Vec<Series>, note: &str) -> Self {
        let created = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let content_hash = document.content_hash();
        let id = format!("{created}-{}", &content_hash[..8]);
        Self { schema: SCHEMA.into(), id, created, note: note.into(), content_hash, document: document.clone(), config, duration, series: series.into_iter().map(thin).collect() }
    }
    pub fn summary(&self) -> RunSummary {
        RunSummary {
            id: self.id.clone(),
            created: self.created,
            note: self.note.clone(),
            revision: self.document.revision,
            content_hash: self.content_hash.clone(),
            duration: self.duration,
            seed: self.config.seed,
            finals: self.series.iter().filter_map(|s| s.values.last().map(|v| (s.label.clone(), *v))).collect(),
        }
    }
}

/// Run headlessly through the shared session and keep the record.
pub fn record(document: &SystemDocument, registry: &BehaviorRegistry, duration: f64, config: SessionConfig, select: &[String], note: &str) -> Result<RunRecord, String> {
    let series = system_builder::simulate(document, registry, duration, config.clone(), select)?;
    Ok(RunRecord::new(document, config, duration, series, note))
}

pub fn save(dir: &Path, record: &RunRecord) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut path = dir.join(format!("{}.json", record.id));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{}-{n}.json", record.id));
        n += 1;
    }
    sim_system::store::write_atomic(&path, &serde_json::to_vec(record).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(path)
}

pub fn load(path: &Path) -> Result<RunRecord, String> {
    let record: RunRecord = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    if record.schema != SCHEMA {
        return Err(format!("{} is not a {SCHEMA} record", path.display()));
    }
    Ok(record)
}

/// Saved runs, newest first.
pub fn list(dir: &Path) -> Vec<(PathBuf, RunSummary)> {
    let mut out: Vec<(PathBuf, RunSummary)> = std::fs::read_dir(dir)
        .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).filter_map(|p| load(&p).ok().map(|r| (p, r.summary()))).collect())
        .unwrap_or_default();
    out.sort_by(|a, b| b.1.created.cmp(&a.1.created).then(b.0.cmp(&a.0)));
    out
}

/// Rerun a record from its own document and settings; the largest relative
/// difference from the recorded series (0 when reproducible).
pub fn replay(record: &RunRecord, registry: &BehaviorRegistry) -> Result<f64, String> {
    let labels: Vec<String> = record.series.iter().map(|s| s.label.clone()).collect();
    let again = RunRecord::new(&record.document, record.config.clone(), record.duration, system_builder::simulate(&record.document, registry, record.duration, record.config.clone(), &labels)?, "replay");
    let mut worst = 0f64;
    for s in &record.series {
        let Some(t) = again.series.iter().find(|x| x.label == s.label) else { return Err(format!("replay lacks {}", s.label)) };
        let scale = s.values.iter().fold(1e-12f64, |m, v| m.max(v.abs()));
        for (a, b) in s.values.iter().zip(&t.values) {
            worst = worst.max((a - b).abs() / scale);
        }
    }
    Ok(worst)
}

/// Several runs side by side, in the shape the study views use.
pub fn compare(records: &[RunRecord], metrics: &[sim_system::Metric]) -> crate::system_study::StudyResult {
    crate::system_study::StudyResult {
        name: "runs".into(),
        kind: "compare",
        parameter: None,
        source_hash: records.iter().map(|r| &r.content_hash[..8]).collect::<Vec<_>>().join("+"),
        variants: records
            .iter()
            .map(|r| crate::system_study::VariantResult {
                label: format!("{} · rev {}{}", r.id, r.document.revision, if r.note.is_empty() { String::new() } else { format!(" · {}", r.note) }),
                value: None,
                metrics: metrics.iter().map(|m| (m.label.clone(), r.series.iter().find(|s| s.label == m.observable).and_then(|s| crate::system_study::reduce(s, m)).unwrap_or(f64::NAN))).collect(),
                series: r.series.clone(),
                derived: Vec::new(),
                error: None,
                wall_seconds: 0.,
            })
            .collect(),
    }
}
