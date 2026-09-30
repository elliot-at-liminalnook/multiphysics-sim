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

/// Note appended to a run whose document was edited while it was recording.
/// Its record then holds the final document, not the one every sample came from.
pub const EDITED_WHILE_RUNNING: &str = "edited while running";

impl RunRecord {
    /// The document changed mid-run, so no single document produced the series.
    pub fn edited_while_running(&self) -> bool {
        self.note.contains(EDITED_WHILE_RUNNING)
    }
}

/// What a replay found.
#[derive(Debug, Clone, Serialize)]
pub struct ReplayReport {
    /// Largest relative difference from the recorded series (0 when reproducible).
    pub max_rel_diff: f64,
    /// Recorded samples compared, over all series.
    pub samples: usize,
}

/// Rerun a record from its own document and settings; the largest relative
/// difference from the recorded series (0 when reproducible).
pub fn replay(record: &RunRecord, registry: &BehaviorRegistry) -> Result<f64, String> {
    replay_with_cancel(record, registry, None).map(|r| r.max_rel_diff)
}

/// [`replay`] that stops between simulation steps once `cancel` is set
/// (error [`system_builder::CANCELLED`]). The rerun is headless, from time 0,
/// with the record's document and config.
pub fn replay_with_cancel(record: &RunRecord, registry: &BehaviorRegistry, cancel: Option<&std::sync::atomic::AtomicBool>) -> Result<ReplayReport, String> {
    let labels: Vec<String> = record.series.iter().map(|s| s.label.clone()).collect();
    let again = system_builder::simulate_cancellable(&record.document, registry, record.duration, record.config.clone(), &labels, cancel)?;
    compare_series(&record.series, &again)
}

/// Compare recorded series with a rerun at every recorded sample time. The
/// rerun must have a sample at exactly each recorded time (runs saved from a
/// live window keep fewer samples than a headless run records); any recorded
/// sample it lacks is an error, never skipped.
pub fn compare_series(recorded: &[Series], replayed: &[Series]) -> Result<ReplayReport, String> {
    let mut worst = 0f64;
    let mut samples = 0;
    for s in recorded {
        let Some(t) = replayed.iter().find(|x| x.label == s.label) else { return Err(format!("replay lacks {}", s.label)) };
        if s.times.len() != s.values.len() {
            return Err(format!("recorded {} has {} times but {} values", s.label, s.times.len(), s.values.len()));
        }
        let scale = s.values.iter().fold(1e-12f64, |m, v| m.max(v.abs()));
        let mut j = 0;
        let mut matched = 0;
        for (time, a) in s.times.iter().zip(&s.values) {
            let near = |x: f64| (x - time).abs() <= 1e-9 * time.abs().max(1.);
            while j < t.times.len() && t.times[j] < *time && !near(t.times[j]) {
                j += 1;
            }
            if j < t.times.len() && near(t.times[j]) {
                worst = worst.max((a - t.values[j]).abs() / scale);
                matched += 1;
            }
        }
        if matched != s.values.len() {
            return Err(format!(
                "replay of {} has {} samples ({} at the recorded times) but the record has {}",
                s.label,
                t.values.len(),
                matched,
                s.values.len()
            ));
        }
        samples += matched;
    }
    Ok(ReplayReport { max_rel_diff: worst, samples })
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

#[cfg(test)]
mod tests {
    use super::*;
    fn series(label: &str, times: &[f64], values: &[f64]) -> Series {
        Series { observable: label.into(), label: label.into(), unit: String::new(), times: times.to_vec(), values: values.to_vec() }
    }

    #[test]
    fn replay_comparison_rejects_missing_samples_and_reports_differences() {
        let recorded = [series("drum.speed", &[0.1, 0.2, 0.3], &[1., 2., 4.])];
        // Identical rerun, recorded on a finer grid: reproduced exactly.
        let same = [series("drum.speed", &[0.05, 0.1, 0.15, 0.2, 0.25, 0.3], &[0., 1., 0., 2., 0., 4.])];
        let r = compare_series(&recorded, &same).unwrap();
        assert_eq!((r.max_rel_diff, r.samples), (0., 3));
        // Thinning may repeat the last recorded sample.
        let repeated = [series("drum.speed", &[0.1, 0.2, 0.3, 0.3], &[1., 2., 4., 4.])];
        assert_eq!(compare_series(&repeated, &same).unwrap().samples, 4);
        // A shorter rerun is an error naming the label and both lengths, not a pass.
        let short = [series("drum.speed", &[0.1, 0.2], &[1., 2.])];
        let e = compare_series(&recorded, &short).unwrap_err();
        assert!(e.contains("drum.speed") && e.contains("has 2 samples") && e.contains("record has 3"), "{e}");
        // Samples at other times are not compared by position.
        let shifted = [series("drum.speed", &[0.11, 0.21, 0.31], &[1., 2., 4.])];
        assert!(compare_series(&recorded, &shifted).is_err());
        // Differences are reported relative to the recorded magnitude.
        let off = [series("drum.speed", &[0.1, 0.2, 0.3], &[1., 2., 5.])];
        assert_eq!(compare_series(&recorded, &off).unwrap().max_rel_diff, 0.25);
        assert!(compare_series(&recorded, &[]).unwrap_err().contains("replay lacks drum.speed"));
    }
}
