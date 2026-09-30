//! Read-only access to gait-lab results: the `report.yaml` each evaluation
//! writes into its results directory, and the `journal.jsonl` a results root
//! accumulates. Reports parse into the same types the writers use
//! ([`GaitReport`], [`PoseReport`], [`ManeuverReport`]); nothing is loosened
//! and nothing is written. Every failure names the file it came from.
//!
//! Reports do not record the runtime fingerprint or a timestamp. The only
//! time available is a journal line's `unix_s`; entries without a matching
//! journal line have none.
use super::{GaitReport, ManeuverReport, PoseReport};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// One parsed `report.yaml`, by its `kind`.
#[derive(Clone, Debug, Serialize)]
pub enum LabReport {
    /// No `kind` field: a gait evaluation.
    Gait(GaitReport),
    /// `kind: pose_sequence`.
    Pose(PoseReport),
    /// `kind: maneuver`.
    Maneuver(ManeuverReport),
}

impl LabReport {
    pub fn status(&self) -> &str {
        match self {
            Self::Gait(r) => &r.status,
            Self::Pose(r) => &r.status,
            Self::Maneuver(r) => &r.status,
        }
    }
}

/// One line of `journal.jsonl`, as [`super::journal`] writes it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JournalRecord {
    pub unix_s: u64,
    pub cached: bool,
    pub gait: String,
    pub status: String,
    pub speed_m_s: Option<f64>,
    /// The results directory as recorded: relative to the directory the CLI
    /// ran from (usually the repository root), or absolute.
    pub results: String,
    /// 1-based line number in `journal.jsonl`.
    #[serde(skip_deserializing)]
    pub line: usize,
}

/// One results directory under a root.
#[derive(Clone, Debug, Serialize)]
pub struct ResultsEntry {
    /// Directory name (the gait/pose/maneuver name plus a content hash).
    pub name: String,
    pub directory: String,
    /// The parsed report, or an error naming its `report.yaml`.
    pub report: Result<LabReport, String>,
    /// The latest journal line for this directory, if the root has one.
    pub journal: Option<JournalRecord>,
}

/// Everything readable under a results root.
#[derive(Clone, Debug, Serialize)]
pub struct ResultsListing {
    pub root: String,
    /// Sorted by directory name.
    pub entries: Vec<ResultsEntry>,
    /// Whether `root/journal.jsonl` exists (no journal means no timestamps).
    pub journal: bool,
    /// Problems that belong to no single entry: malformed journal lines
    /// (file and line number), unreadable directory entries.
    pub warnings: Vec<String>,
}

#[derive(Deserialize)]
struct Kind {
    kind: Option<String>,
}

/// Parse `dir/report.yaml`, dispatching on its optional `kind`: absent is a
/// gait report, `pose_sequence` a pose report, `maneuver` a maneuver report.
/// Any other kind, and every read or parse failure, is an error naming the
/// `report.yaml` path.
pub fn read_report(dir: &Path) -> Result<LabReport, String> {
    let path = dir.join("report.yaml");
    let at = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let text = fs::read_to_string(&path).map_err(|e| at(&e))?;
    let kind: Kind = serde_norway::from_str(&text).map_err(|e| at(&e))?;
    match kind.kind.as_deref() {
        None => serde_norway::from_str(&text).map(LabReport::Gait),
        Some("pose_sequence") => serde_norway::from_str(&text).map(LabReport::Pose),
        Some("maneuver") => serde_norway::from_str(&text).map(LabReport::Maneuver),
        Some(other) => return Err(at(&format!("unknown report kind {other:?} (expected none, pose_sequence or maneuver)"))),
    }
    .map_err(|e| at(&e))
}

/// Read every immediate subdirectory of `root` that holds a `report.yaml`,
/// plus `root/journal.jsonl` if present. Subdirectories without a
/// `report.yaml` (and plain files) are not results and are ignored; a
/// `report.yaml` that cannot be read or parsed is kept as an entry whose
/// report is an error naming the file. A missing or unreadable root is an
/// error naming the root.
///
/// Journal matching: a line belongs to an entry when the final path
/// component of its recorded `results` equals the entry's directory name.
/// The recorded path is relative to wherever the CLI ran (usually the
/// repository root) or absolute, so it cannot be resolved reliably from here;
/// the journal lives in the root it describes, and directory names end in a
/// content hash, so the name identifies the directory within the root. Of
/// the matching lines, the one with the largest `unix_s` wins, and among
/// equal times the later line in the file.
pub fn scan_results(root: &Path) -> Result<ResultsListing, String> {
    let dirs = fs::read_dir(root).map_err(|e| format!("{}: {e}", root.display()))?;
    let mut warnings = Vec::new();
    let mut found: Vec<PathBuf> = Vec::new();
    for item in dirs {
        match item {
            Ok(item) => {
                let path = item.path();
                if path.is_dir() && path.join("report.yaml").exists() {
                    found.push(path);
                }
            }
            Err(e) => warnings.push(format!("{}: {e}", root.display())),
        }
    }
    found.sort();
    if root.join("report.yaml").exists() {
        warnings.push(format!(
            "{} holds a report.yaml itself: it is one results directory, not a results root",
            root.display()
        ));
    }
    let journal_path = root.join("journal.jsonl");
    let journal = journal_path.exists();
    let lines = if journal { read_journal(&journal_path, &mut warnings) } else { vec![] };
    let entries = found
        .into_iter()
        .map(|dir| {
            let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let journal = lines
                .iter()
                .filter(|l| Path::new(&l.results).file_name().is_some_and(|n| n.to_string_lossy() == name))
                .max_by_key(|l| (l.unix_s, l.line))
                .cloned();
            ResultsEntry { report: read_report(&dir), directory: dir.display().to_string(), name, journal }
        })
        .collect();
    Ok(ResultsListing { root: root.display().to_string(), entries, journal, warnings })
}

fn read_journal(path: &Path, warnings: &mut Vec<String>) -> Vec<JournalRecord> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            warnings.push(format!("{}: {e}", path.display()));
            return vec![];
        }
    };
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<JournalRecord>(line) {
            Ok(mut r) => {
                r.line = i + 1;
                out.push(r);
            }
            Err(e) => warnings.push(format!("{} line {}: {e}", path.display(), i + 1)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_reports_by_kind_with_errors_and_journal_times() {
        let root = std::env::temp_dir().join(format!("gait-lab-reports-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let write = |dir: &str, text: &str| {
            fs::create_dir_all(root.join(dir)).unwrap();
            fs::write(root.join(dir).join("report.yaml"), text).unwrap();
        };
        write(
            "walk-aaaa",
            "gait: walk\nfile: walk.yaml\nstatus: screened_out\nsummary: s\nspeed_m_s: null\nforward_distance_m: null\n\
             simulated_s: null\ngates: []\nreasons: [too fast]\njoints: []\nfidelity: detailed model\n\
             timing: {preparation_s: 0.5, simulation_s: 0.0, simulated_s: 0.0}\nresults_directory: r/walk-aaaa\ncompiled_gait: null\n",
        );
        write(
            "crouch-bbbb",
            "kind: pose_sequence\nsequence: crouch\nfile: c.yaml\nstatus: ready\nsummary: s\nperiod_s: 5.0\nreasons: []\n\
             joints: []\nresults_directory: r/crouch-bbbb\ncompiled_gait: null\n",
        );
        write("broken-cccc", "gait: [unterminated\n");
        write("odd-dddd", "kind: maneuver_trace\n");
        fs::create_dir_all(root.join("no-report")).unwrap();
        fs::write(
            root.join("journal.jsonl"),
            "{\"cached\":false,\"gait\":\"walk\",\"results\":\"examples/x/r/walk-aaaa\",\"speed_m_s\":null,\"status\":\"screened_out\",\"summary\":\"\",\"unix_s\":100}\n\
             not json\n\
             {\"cached\":true,\"gait\":\"walk\",\"results\":\"examples/x/r/walk-aaaa\",\"speed_m_s\":null,\"status\":\"screened_out\",\"summary\":\"\",\"unix_s\":200}\n",
        )
        .unwrap();

        let listing = scan_results(&root).unwrap();
        let names: Vec<_> = listing.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["broken-cccc", "crouch-bbbb", "odd-dddd", "walk-aaaa"]);
        let [broken, crouch, odd, walk] = &listing.entries[..] else { unreachable!() };
        assert!(matches!(&walk.report, Ok(LabReport::Gait(r)) if r.status == "screened_out" && r.speed_m_s.is_none()));
        assert!(matches!(&crouch.report, Ok(LabReport::Pose(r)) if r.period_s == Some(5.0)));
        let bad = root.join("broken-cccc").join("report.yaml").display().to_string();
        assert!(broken.report.as_ref().unwrap_err().contains(&bad));
        let odd_err = odd.report.as_ref().unwrap_err();
        assert!(odd_err.contains(&root.join("odd-dddd").join("report.yaml").display().to_string()) && odd_err.contains("maneuver_trace"));
        let j = walk.journal.as_ref().unwrap();
        assert_eq!((j.unix_s, j.cached, j.line), (200, true, 3));
        assert!(crouch.journal.is_none());
        assert_eq!(listing.warnings.len(), 1);
        assert!(listing.warnings[0].contains("journal.jsonl line 2"));
        let missing = root.join("missing");
        assert!(scan_results(&missing).unwrap_err().contains(&missing.display().to_string()));
        fs::remove_dir_all(&root).unwrap();

        // Tracked results, read only.
        let tracked = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/full-robot/measured-actuator-integration/gait-lab-2026-09-25/results-legscreen");
        let listing = scan_results(&tracked).unwrap();
        assert_eq!(listing.entries.len(), 2);
        assert!(listing.warnings.is_empty());
        for e in &listing.entries {
            assert!(matches!(&e.report, Ok(LabReport::Gait(r)) if r.status == "screened_out"), "{}", e.name);
            assert_eq!(e.journal.as_ref().map(|j| j.unix_s), Some(1790438434));
        }
    }
}
