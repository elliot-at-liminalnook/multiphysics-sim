//! Read-only review of a measured actuator identification archive (build
//! mode, Actuators tab → Measured evidence).
//!
//! `Builder::calibration_request` is the one entry point: the archive path
//! field, the first visit to the section, Reload/Cancel (also as `system_ui`
//! controls) and REST `system_calibration_review` all call it. A worker
//! thread runs [`review`], which calls the shared
//! `sim_runtime::experiment_comparison::hx_archive::load` with the workspace
//! root as the repository (the same loader `sim-viewer --experiments` uses);
//! `finish_calibration` installs the result on the UI thread. Every value
//! shown is the archive's own: pass/fail, RMSE and final error come from each
//! trial's `comparison` against its `limits`, and the split × outcome counts
//! only count those. Nothing is evaluated, edited or written here.
use super::ui::{Kit, Look, num, wrap, DANGER, FAINT, OK, SUBTLE, TEXT, WARN};
use super::*;
use sim_runtime::experiment_comparison::hx_archive::{self, Archive, Trial};
use std::path::Path;

/// The tracked identification archive, relative to the workspace root.
pub const DEFAULT_ARCHIVE: &str = "examples/actuators/hx30hm/pwm-full-range-identification";

/// Trials per page of the list (the rest are paged, not built).
pub const PAGE_ROWS: usize = 20;

/// The two parts of the Actuators tab.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActuatorView {
    /// The accepted registry and consumer checks.
    #[default]
    Registry,
    /// The identification archive review.
    Evidence,
}

/// Split filter. Held-out is every split other than `train` (the archive's
/// `held_out_*` splits), the same rule `sim-viewer --experiments` uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitFilter {
    #[default]
    All,
    Train,
    HeldOut,
}

/// Outcome filter over the archive's own `comparison.passes`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeFilter {
    #[default]
    All,
    Pass,
    Fail,
}

pub fn held_out(t: &Trial) -> bool {
    t.split != "train"
}

impl SplitFilter {
    fn keeps(self, t: &Trial) -> bool {
        match self {
            Self::All => true,
            Self::Train => !held_out(t),
            Self::HeldOut => held_out(t),
        }
    }
}

impl OutcomeFilter {
    fn keeps(self, t: &Trial) -> bool {
        match self {
            Self::All => true,
            Self::Pass => t.comparison.passes,
            Self::Fail => !t.comparison.passes,
        }
    }
}

/// One finished load, labelled with its own directory.
#[derive(Clone, Debug)]
pub struct Review {
    pub path: PathBuf,
    pub repository: PathBuf,
    pub archive: Archive,
    pub seconds: f64,
}

/// Pass/fail counts, by counting `comparison.passes` only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Count {
    pub total: usize,
    pub pass: usize,
    pub fail: usize,
}

impl Count {
    fn add(&mut self, t: &Trial) {
        self.total += 1;
        if t.comparison.passes { self.pass += 1 } else { self.fail += 1 }
    }
}

/// Counts per exact split string, plus train and held-out (all `held_out_*`) totals.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct Counts {
    pub by_split: BTreeMap<String, Count>,
    pub train: Count,
    pub held_out: Count,
    pub all: Count,
}

pub fn counts(archive: &Archive) -> Counts {
    let mut c = Counts::default();
    for t in &archive.trials {
        c.by_split.entry(t.split.clone()).or_default().add(t);
        if held_out(t) { c.held_out.add(t) } else { c.train.add(t) }
        c.all.add(t);
    }
    c
}

/// One trial as `system_state` and REST show it: the archive's fields, with
/// the comparison and limits in the traces' own unit. No samples here.
pub fn trial_json(t: &Trial) -> serde_json::Value {
    serde_json::json!({
        "id": t.id,
        "run": t.run,
        "device": t.device,
        "stage": t.stage,
        "kind": t.kind,
        "drive": t.drive,
        "duration_s": t.duration_s,
        "split": t.split,
        "held_out": held_out(t),
        "voltage_range_v": t.voltage_range_v,
        "temperature_range_c": t.temperature_range_c,
        "unit": t.measured.unit,
        "limits": {"rmse": t.limits.rmse, "final_abs_error": t.limits.final_abs_error},
        "comparison": {"passes": t.comparison.passes, "rmse": t.comparison.rmse, "maximum_abs_error": t.comparison.maximum_abs_error, "final_error": t.comparison.final_error},
        "samples": {"measured": t.measured.samples.len(), "predicted": t.predicted.samples.len()},
    })
}

/// The whole review (REST result): archive metadata, counts and every trial row.
pub fn review_json(r: &Review) -> serde_json::Value {
    let a = &r.archive;
    serde_json::json!({
        "path": r.path,
        "repository": r.repository,
        "label": a.label,
        "interpretation": a.interpretation,
        "split_policy": a.split_policy,
        "observation_blake3": a.observation_blake3,
        "model_blake3": a.model_blake3,
        "verified_inputs": a.verified_inputs,
        "input_blake3": a.input_blake3,
        "integrity_issues": a.integrity_issues,
        "trial_count": a.trials.len(),
        "counts": counts(a),
        "trials": a.trials.iter().map(trial_json).collect::<Vec<_>>(),
        "load_seconds": r.seconds,
    })
}

/// Load `dir` with the shared HX archive loader (the worker thread's call).
/// A `sweep.csv` directory or a file (e.g. a study) is refused by name; every
/// error names `dir`.
pub fn review(dir: &Path, repository: &Path) -> Result<Review, String> {
    let started = std::time::Instant::now();
    let shown = dir.display();
    if dir.is_file() {
        return Err(format!("{shown} is a file (a saved study?); the native review opens identification archive folders only. Reviewing studies is not supported yet; use sim-viewer --experiments."));
    }
    if dir.join("sweep.csv").exists() {
        return Err(format!("{shown} is a raw sweep folder (sweep.csv); sweep review is not supported in the native review yet; use sim-viewer --experiments."));
    }
    let archive = hx_archive::load(dir, repository).map_err(|e| format!("Could not load identification archive {shown}: {e}"))?;
    Ok(Review { path: dir.to_path_buf(), repository: repository.to_path_buf(), archive, seconds: started.elapsed().as_secs_f64() })
}

struct Job {
    seq: u64,
    path: PathBuf,
    receiver: Mutex<mpsc::Receiver<Result<Review, String>>>,
}

#[derive(Default)]
pub(super) struct CalibrationState {
    job: Option<Job>,
    seq: u64,
    /// Directory of the latest request (the path field's value).
    pub path: Option<PathBuf>,
    /// The latest good load, labelled with its own path.
    pub shown: Option<Review>,
    /// The latest failure (names the path); cleared by a good load.
    pub error: Option<String>,
    pub split: SplitFilter,
    pub outcome: OutcomeFilter,
    /// Page of the filtered list (0-based).
    pub page: usize,
    last: Option<(u64, Result<serde_json::Value, String>)>,
}

impl CalibrationState {
    pub fn pending(&self) -> Option<&Path> {
        self.job.as_ref().map(|j| j.path.as_path())
    }

    /// Indices of the trials the filters keep, in archive order.
    pub fn visible(&self) -> Vec<usize> {
        self.shown.as_ref().map(|r| r.archive.trials.iter().enumerate().filter(|(_, t)| self.split.keeps(t) && self.outcome.keeps(t)).map(|(i, _)| i).collect()).unwrap_or_default()
    }

    pub fn pages(&self, visible: usize) -> usize {
        visible.div_ceil(PAGE_ROWS).max(1)
    }

    fn phase(&self) -> &'static str {
        if self.job.is_some() {
            "loading"
        } else if self.error.is_some() {
            "failed"
        } else if self.shown.is_some() {
            "loaded"
        } else {
            "idle"
        }
    }
}

impl Builder {
    /// Load an identification archive off the UI thread (the shared path for
    /// the Measured evidence section, `system_ui` and REST
    /// `system_calibration_review`). `path: None` reloads the current one
    /// (the tracked default at first). Refuses while a load is pending.
    /// Read-only.
    pub fn calibration_request(&mut self, path: Option<PathBuf>) -> Result<u64, String> {
        let result = self.start_calibration(path);
        if let Err(e) = &result {
            self.calibration.error = Some(e.clone());
            self.action_error = Some(e.clone());
            self.status = e.clone();
        }
        self.panel_dirty = true;
        result
    }

    /// The first visit to the Measured evidence section loads the tracked
    /// default (once: not after a load, a failure or while one is pending).
    pub(super) fn calibration_first_visit(&mut self) {
        let c = &self.calibration;
        if c.shown.is_none() && c.error.is_none() && c.pending().is_none() && c.last.is_none() {
            let r = self.calibration_request(None);
            self.report(r);
        }
    }

    fn start_calibration(&mut self, path: Option<PathBuf>) -> Result<u64, String> {
        if let Some(pending) = self.calibration.pending() {
            return Err(format!("Still loading identification archive {}; wait for it before another request.", pending.display()));
        }
        let repository = crate::workspace::root().map_err(|e| format!("No workspace root to verify archive inputs against: {e}"))?.to_path_buf();
        let dir = match path.or_else(|| self.calibration.path.clone()) {
            Some(p) => std::path::absolute(&p).unwrap_or(p),
            None => repository.join(DEFAULT_ARCHIVE),
        };
        self.calibration.path = Some(dir.clone());
        let (send, receive) = mpsc::channel();
        let worker = dir.clone();
        std::thread::spawn(move || {
            let _ = send.send(review(&worker, &repository));
        });
        self.calibration.seq += 1;
        self.calibration.job = Some(Job { seq: self.calibration.seq, path: dir.clone(), receiver: Mutex::new(receive) });
        self.status = format!("Loading identification archive {} in the background…", dir.display());
        Ok(self.calibration.seq)
    }

    /// Stop waiting for a pending load (its result is dropped).
    pub fn cancel_calibration(&mut self) -> bool {
        let Some(job) = self.calibration.job.take() else { return false };
        self.status = format!("Loading identification archive {} cancelled.", job.path.display());
        self.calibration.last = Some((job.seq, Err(self.status.clone())));
        self.panel_dirty = true;
        true
    }

    /// Install a finished load. Returns true when one finished.
    pub(crate) fn finish_calibration(&mut self) -> bool {
        let Some(job) = &self.calibration.job else { return false };
        let polled = job.receiver.lock().map(|r| r.try_recv()).unwrap_or(Err(mpsc::TryRecvError::Disconnected));
        let result = match polled {
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => Err(format!("Could not load identification archive {}: the loader ended without a result.", job.path.display())),
            Ok(r) => r,
        };
        let job = self.calibration.job.take().expect("polled above");
        self.panel_dirty = true;
        match result {
            Ok(review) => {
                let c = counts(&review.archive);
                self.status = format!("Identification archive {}: {} trials; held-out {}/{} pass, train {}/{} pass (archive comparisons)", review.path.display(), c.all.total, c.held_out.pass, c.held_out.total, c.train.pass, c.train.total);
                let value = review_json(&review);
                self.calibration.shown = Some(review);
                self.calibration.error = None;
                self.calibration.page = 0;
                self.calibration.last = Some((job.seq, Ok(value)));
            }
            Err(e) => {
                self.action_error = Some(e.clone());
                self.status = e.clone();
                self.calibration.error = Some(e.clone());
                self.calibration.last = Some((job.seq, Err(e)));
            }
        }
        true
    }

    pub(super) fn set_calibration_filter(&mut self, split: Option<SplitFilter>, outcome: Option<OutcomeFilter>) {
        if let Some(s) = split {
            self.calibration.split = s;
        }
        if let Some(o) = outcome {
            self.calibration.outcome = o;
        }
        self.calibration.page = 0;
        self.panel_dirty = true;
    }

    pub(super) fn set_calibration_page(&mut self, page: usize) {
        let pages = self.calibration.pages(self.calibration.visible().len());
        self.calibration.page = page.min(pages - 1);
        self.panel_dirty = true;
    }

    /// REST `system_calibration_review`: start (or refuse) on the first
    /// call, then report the review once `finish_calibration` has run.
    pub(crate) fn calibration_rest(&mut self, args: &serde_json::Value, continuation: &mut serde_json::Value, cancelled: bool) -> sim_api::Outcome {
        let Some(seq) = continuation.get("calibration").and_then(|s| s.as_u64()) else {
            let path = match args.get("path") {
                None | Some(serde_json::Value::Null) => None,
                Some(serde_json::Value::String(s)) => Some(PathBuf::from(s)),
                Some(_) => return sim_api::Outcome::Done(Err("system_calibration_review: path must be a path string".into())),
            };
            return match self.calibration_request(path) {
                Ok(seq) => {
                    *continuation = serde_json::json!({"calibration": seq});
                    sim_api::Outcome::Pending
                }
                Err(e) => sim_api::Outcome::Done(Err(e)),
            };
        };
        match &self.calibration.last {
            Some((s, result)) if *s == seq => sim_api::Outcome::Done(result.clone()),
            _ if cancelled && self.calibration.job.as_ref().is_some_and(|j| j.seq == seq) => {
                self.cancel_calibration();
                sim_api::Outcome::Done(Err("cancelled".into()))
            }
            _ if self.calibration.job.as_ref().is_none_or(|j| j.seq != seq) => sim_api::Outcome::Done(Err("the calibration review request was superseded".into())),
            _ => sim_api::Outcome::Pending,
        }
    }

    pub(super) fn calibration_json(&self) -> serde_json::Value {
        let c = &self.calibration;
        let shown = c.shown.as_ref();
        let visible = c.visible();
        let pages = c.pages(visible.len());
        let page: Vec<serde_json::Value> = shown.map(|r| visible.iter().skip(c.page * PAGE_ROWS).take(PAGE_ROWS).map(|&i| trial_json(&r.archive.trials[i])).collect()).unwrap_or_default();
        serde_json::json!({
            "read_only": true,
            "phase": c.phase(),
            "pending": c.pending(),
            "requested": c.path,
            "error": c.error,
            "path": shown.map(|r| &r.path),
            "repository": shown.map(|r| &r.repository),
            "label": shown.map(|r| &r.archive.label),
            "interpretation": shown.map(|r| &r.archive.interpretation),
            "split_policy": shown.map(|r| &r.archive.split_policy),
            "observation_blake3": shown.map(|r| &r.archive.observation_blake3),
            "model_blake3": shown.map(|r| &r.archive.model_blake3),
            "verified_inputs": shown.map(|r| r.archive.verified_inputs),
            "declared_inputs": shown.map(|r| r.archive.input_blake3.len()),
            "integrity_issues": shown.map(|r| &r.archive.integrity_issues),
            "trial_count": shown.map(|r| r.archive.trials.len()),
            "counts": shown.map(|r| counts(&r.archive)),
            "filters": {"split": c.split, "outcome": c.outcome},
            "visible": shown.map(|r| visible.iter().map(|&i| r.archive.trials[i].id.as_str()).collect::<Vec<_>>()),
            "page": {"index": c.page, "pages": pages, "rows": PAGE_ROWS, "trials": page},
            "load_seconds": shown.map(|r| r.seconds),
            "last": c.last.as_ref().map(|(seq, r)| serde_json::json!({"seq": seq, "ok": r.is_ok(), "error": r.as_ref().err()})),
        })
    }
}

/// Encoder counts for a value in radians (display alongside the archive's rad).
fn counts_of(rad: f64) -> String {
    format!("{:.1}", rad / hx_archive::ENCODER_QUANTUM_RAD)
}

const HELD_OUT: Color = Color::srgb(0.690, 0.604, 0.902);

/// The Measured evidence section: path, Reload/Cancel, archive metadata and
/// integrity, split × outcome counts, filters and a paged trial list.
pub(super) fn section(body: &mut ChildSpawnerCommands, k: &Kit, b: &Builder) {
    let c = &b.calibration;
    body.spawn(k.text("Measured identification evidence as archived: each hardware trial's measured encoder trace against the fitted model's prediction, with the archive's own pass/fail. Read-only; nothing is evaluated, refitted or promoted here.", 12., SUBTLE, 0));
    body.spawn(k.section("Archive"));
    let focused = b.input.as_ref().is_some_and(|i| i.purpose == Purpose::CalibrationArchive);
    let shown = if focused { b.input.as_ref().map(|i| i.buffer.clone()).unwrap_or_default() } else { c.path.as_ref().map(|p| p.display().to_string()).unwrap_or_default() };
    body.spawn(k.input(&shown, "Path to an identification archive folder · Enter to load", BuildAction::CalibrationPath, focused));
    body.spawn(Node { margin: UiRect::top(Val::Px(6.)), ..wrap() }).with_children(|r| {
        r.spawn(k.button("Reload", BuildAction::CalibrationReload, Look::Secondary, c.pending().is_none()));
        if c.pending().is_some() {
            r.spawn(k.button("Cancel", BuildAction::CancelCalibration, Look::Danger, true));
        }
    });
    if let Some(pending) = c.pending() {
        body.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Center, margin: UiRect::top(Val::Px(6.)), flex_shrink: 0., ..default() }, children![k.dot(ACCENT), k.text(format!("Loading {}…", pending.display()), 12., TEXT, 0)]));
    }
    if let Some(e) = &c.error {
        body.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Start, margin: UiRect::top(Val::Px(6.)), flex_shrink: 0., ..default() }, children![k.dot(DANGER), k.text(e, 12., DANGER, 0)]));
    }
    let Some(r) = &c.shown else {
        if c.pending().is_none() && c.error.is_none() {
            body.spawn(k.text("Not loaded yet.", 12., SUBTLE, 0));
        }
        return;
    };
    let a = &r.archive;
    if c.error.is_some() {
        body.spawn(k.text(format!("Still showing the last good load: {}", r.path.display()), 11.5, WARN, 1));
    }
    body.spawn(k.text(format!("Showing {}", r.path.display()), 11., FAINT, 0));
    k.property(body, "Label", &a.label, "", None::<BuildAction>, false);
    body.spawn(k.text("Interpretation", 11., FAINT, 2));
    body.spawn(k.text(&a.interpretation, 11.5, TEXT, 0));
    body.spawn(k.text("Split policy", 11., FAINT, 2));
    body.spawn(k.text(&a.split_policy, 11.5, TEXT, 0));

    body.spawn(k.section("Integrity"));
    body.spawn(k.text("observations.json blake3", 11., FAINT, 2));
    body.spawn(k.text(&a.observation_blake3, 10.5, TEXT, 0));
    body.spawn(k.text("results.json (model) blake3", 11., FAINT, 2));
    body.spawn(k.text(&a.model_blake3, 10.5, TEXT, 0));
    let all_verified = a.verified_inputs == a.input_blake3.len();
    k.property(body, "Verified inputs", &format!("{} of {}", a.verified_inputs, a.input_blake3.len()), "", None::<BuildAction>, false);
    body.spawn(k.text(format!("Inputs hashed against {}", r.repository.display()), 10.5, FAINT, 0));
    if a.integrity_issues.is_empty() {
        body.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Center, flex_shrink: 0., ..default() }, children![k.dot(if all_verified { OK } else { WARN }), k.text("Integrity issues: none", 11.5, TEXT, 0)]));
    } else {
        for issue in &a.integrity_issues {
            body.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Start, flex_shrink: 0., ..default() }, children![k.dot(WARN), k.text(issue, 11.5, WARN, 0)]));
        }
    }

    let n = counts(a);
    body.spawn(k.section(&format!("Trials  {}", a.trials.len())));
    body.spawn(k.text("Counts of the archive's own comparison.passes:", 11., FAINT, 0));
    let line = |label: &str, x: Count| format!("{label}: {}/{} pass · {} fail", x.pass, x.total, x.fail);
    body.spawn(k.text(line("Held-out (validation)", n.held_out), 12., HELD_OUT, 2));
    body.spawn(k.text(line("Train (fitting)", n.train), 12., TEXT, 1));
    for (split, x) in &n.by_split {
        body.spawn(k.text(line(&format!("  {split}"), *x), 11., SUBTLE, 0));
    }
    if let Some(t) = a.trials.first() {
        body.spawn(k.text(format!("Limits per trial: RMSE ≤ {} {u} ({} counts) and |final error| ≤ {} {u} ({} counts); 1 count = 2π/4096 rad.", num(t.limits.rmse), counts_of(t.limits.rmse), num(t.limits.final_abs_error), counts_of(t.limits.final_abs_error), u = t.measured.unit), 11., FAINT, 0));
    }

    body.spawn(Node { margin: UiRect::top(Val::Px(6.)), ..wrap() }).with_children(|chips| {
        for (label, f) in [("All splits", SplitFilter::All), ("Train", SplitFilter::Train), ("Held-out", SplitFilter::HeldOut)] {
            chips.spawn(k.button(label, BuildAction::CalibrationSplit(f), Look::Chip(c.split == f), true));
        }
    });
    body.spawn(Node { margin: UiRect::top(Val::Px(4.)), ..wrap() }).with_children(|chips| {
        for (label, f) in [("All outcomes", OutcomeFilter::All), ("Pass", OutcomeFilter::Pass), ("Fail", OutcomeFilter::Fail)] {
            chips.spawn(k.button(label, BuildAction::CalibrationOutcome(f), Look::Chip(c.outcome == f), true));
        }
    });
    let visible = c.visible();
    let pages = c.pages(visible.len());
    let page = c.page.min(pages - 1);
    body.spawn(k.text(format!("{} trials match · page {} of {pages}", visible.len(), page + 1), 11., FAINT, 0));
    body.spawn(Node { margin: UiRect::top(Val::Px(4.)), ..wrap() }).with_children(|r| {
        r.spawn(k.button("‹ Previous", BuildAction::CalibrationPage(page.saturating_sub(1)), Look::Secondary, page > 0));
        r.spawn(k.button("Next ›", BuildAction::CalibrationPage(page + 1), Look::Secondary, page + 1 < pages));
    });
    for &i in visible.iter().skip(page * PAGE_ROWS).take(PAGE_ROWS) {
        trial_row(body, k, &a.trials[i]);
    }
}

fn trial_row(body: &mut ChildSpawnerCommands, k: &Kit, t: &Trial) {
    let held = held_out(t);
    let x = &t.comparison;
    let u = &t.measured.unit;
    let (verdict, color) = if x.passes { ("✓ pass", OK) } else { ("× fail", DANGER) };
    body.spawn((
        Node { flex_direction: FlexDirection::Column, padding: UiRect::axes(Val::Px(8.), Val::Px(3.)), margin: UiRect::top(Val::Px(3.)), border: UiRect::left(Val::Px(2.)), flex_shrink: 0., ..default() },
        BorderColor(if held { HELD_OUT } else { Color::NONE }),
    ))
    .with_children(|row| {
        row.spawn(Node { justify_content: JustifyContent::SpaceBetween, column_gap: Val::Px(8.), ..default() }).with_children(|top| {
            top.spawn(k.text(&t.id, 11.5, TEXT, 1));
            top.spawn(k.text(verdict, 11.5, color, 2));
        });
        row.spawn(k.text(format!("{} · {}", if held { "held-out (validation)" } else { "train (fitting)" }, t.split), 10.5, if held { HELD_OUT } else { SUBTLE }, 1));
        row.spawn(k.text(format!("device {} · stage {} · {} · drive {:+.1}% · {} ms", t.device, t.stage, t.kind, t.drive * 100., num(t.duration_s * 1000.)), 10.5, SUBTLE, 0));
        row.spawn(k.text(format!("{}–{} V · {}–{} °C", num(t.voltage_range_v[0]), num(t.voltage_range_v[1]), num(t.temperature_range_c[0]), num(t.temperature_range_c[1])), 10.5, SUBTLE, 0));
        row.spawn(k.text(format!("RMSE {} {u} ({} counts), limit {} {u} · final error {} {u} ({} counts), limit |·| {} {u}", num(x.rmse), counts_of(x.rmse), num(t.limits.rmse), num(x.final_error), counts_of(x.final_error), num(t.limits.final_abs_error)), 10.5, color, 0));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calibration_review_loads_tracked_archive_and_names_bad_paths() {
        // The workspace root the handler resolves (the repository, from the test's directory).
        let root = crate::workspace::root().unwrap().to_path_buf();
        assert!(root.join(DEFAULT_ARCHIVE).is_dir(), "{}", root.display());
        let dir = root.join(DEFAULT_ARCHIVE);
        let r = review(&dir, &root).unwrap();
        let a = &r.archive;
        assert_eq!(a.trials.len(), 216);
        let n = counts(a);
        let pass = |f: &dyn Fn(&Trial) -> bool| a.trials.iter().filter(|t| f(t) && t.comparison.passes).count();
        assert_eq!(n.held_out.total, a.trials.iter().filter(|t| t.split != "train").count());
        assert_eq!(n.held_out.pass, pass(&|t| t.split != "train"));
        assert_eq!(n.train.pass, pass(&|t| t.split == "train"));
        assert_eq!((n.train.total, n.held_out.total), (54, 162));
        assert_eq!(n.all.pass + n.all.fail, 216);
        // The archive's own comparisons agree with its README ("81/162 held-out trials").
        assert_eq!(n.held_out.pass, 81);
        eprintln!("held-out {}/{} pass, train {}/{} pass; by split {:?}", n.held_out.pass, n.held_out.total, n.train.pass, n.train.total, n.by_split);
        assert!(a.observation_blake3.len() == 64 && a.model_blake3.len() == 64);
        assert_eq!(a.verified_inputs, a.input_blake3.len(), "{:?}", a.integrity_issues);
        assert_eq!(review_json(&r)["trials"].as_array().unwrap().len(), 216);

        let missing = root.join("target/no-such-archive");
        let e = review(&missing, &root).unwrap_err();
        assert!(e.contains(&missing.display().to_string()), "{e}");
        let file = dir.join("README.md");
        let e = review(&file, &root).unwrap_err();
        assert!(e.contains(&file.display().to_string()) && e.contains("not supported"), "{e}");
        let sweep = root.join("examples/actuators/hx30hm/hardware/2026-09-11-nine-servos/pwm-individual-full-range");
        let e = review(&sweep, &root).unwrap_err();
        assert!(e.contains(&sweep.display().to_string()) && e.contains("sweep.csv") && e.contains("not supported"), "{e}");

        // The Builder handler: worker thread, pending refusal, state, filters, last good kept.
        let system = root.join("examples/systems-builder/motor-driver-board/board.system.json");
        let registry = sim_runtime::registry_with_parts(&root.join("library/parts")).0;
        let mut b = Builder::open(system, root.join("library/systems"), registry).unwrap();
        let wait = |b: &mut Builder| {
            for _ in 0..1200 {
                if b.finish_calibration() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            panic!("archive load did not finish");
        };
        b.calibration_first_visit();
        assert_eq!(b.calibration.pending(), Some(dir.as_path()));
        assert_eq!(b.calibration_json()["phase"], "loading");
        assert!(b.calibration_request(None).unwrap_err().contains("Still loading"));
        wait(&mut b);
        b.set_calibration_filter(Some(SplitFilter::HeldOut), Some(OutcomeFilter::Pass));
        let state = b.state_json();
        let c = &state["calibration_review"];
        assert_eq!(c["phase"], "loaded");
        assert_eq!(c["path"], dir.display().to_string());
        assert_eq!(c["trial_count"], 216);
        assert_eq!(c["counts"]["held_out"]["pass"], 81);
        assert_eq!(c["visible"].as_array().unwrap().len(), 81);
        assert_eq!(c["page"]["trials"].as_array().unwrap().len(), PAGE_ROWS);
        assert!(c["page"]["trials"].as_array().unwrap().iter().all(|t| t["held_out"] == true && t["comparison"]["passes"] == true));
        b.calibration_first_visit();
        assert!(b.calibration.pending().is_none(), "first visit loads once");
        b.calibration_request(Some(missing.clone())).unwrap();
        wait(&mut b);
        let c = b.calibration_json();
        assert_eq!(c["phase"], "failed");
        assert!(c["error"].as_str().unwrap().contains(&missing.display().to_string()));
        assert_eq!(c["requested"], missing.display().to_string());
        assert_eq!(c["path"], dir.display().to_string(), "the last good archive stays, with its own path");
    }
}
