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
//!
//! `Builder::select_calibration_trial` is the one selection path (a trial
//! row click, the `calibration_trial` `system_ui` action and REST's `trial`
//! argument). The selected trial's archived measured and predicted traces are
//! drawn with the shared `crate::chart` raster by [`update_chart`] when the
//! selection or the load changes, not every frame.
use super::ui::num;
use super::*;
use crate::ui_kit::{ACCENT, ACCENT_BG, Corner, DANGER, FAINT, Kit, Look, OK, SUBTLE, TEXT, Tint, WARN, size, wrap};
use sim_runtime::experiment_comparison::Trace;
use sim_runtime::experiment_comparison::hx_archive::{self, Archive, Trial};
use std::path::Path;

/// The tracked identification archive, relative to the workspace root.
pub const DEFAULT_ARCHIVE: &str = "examples/actuators/hx30hm/pwm-full-range-identification";

/// Trials per page of the list (the rest are paged, not built).
pub const PAGE_ROWS: usize = 20;

/// Source labels of the two charted series.
pub const MEASURED_LABEL: &str = "measured (hardware archive)";
pub const PREDICTED_LABEL: &str = "predicted (fitted model, archive)";
/// Chart colours: measured, predicted (the shared chart palette).
const SERIES: [[u8; 3]; 2] = [crate::chart::COLORS[0], crate::chart::COLORS[1]];

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

/// One trace's true sample count and first/last samples (null when empty).
fn trace_json(trace: &Trace, source: &str) -> serde_json::Value {
    let end = |o: Option<&sim_runtime::experiment_comparison::Observation>| o.map(|o| serde_json::json!({"time_s": o.time_s, "value": o.value}));
    serde_json::json!({
        "source": source,
        "quantity": trace.quantity,
        "unit": trace.unit,
        "count": trace.samples.len(),
        "first": end(trace.samples.first()),
        "last": end(trace.samples.last()),
    })
}

/// The selected trial as `system_state.calibration_review.selected` shows it:
/// the archive's comparison, limits and trace endpoints, nothing recomputed.
pub fn selected_json(t: &Trial) -> serde_json::Value {
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
        "role": split_role(t),
        "quantity": t.measured.quantity,
        "unit": t.measured.unit,
        "limits": {"rmse": t.limits.rmse, "final_abs_error": t.limits.final_abs_error},
        "comparison": {"passes": t.comparison.passes, "rmse": t.comparison.rmse, "maximum_abs_error": t.comparison.maximum_abs_error, "final_error": t.comparison.final_error},
        "measured": trace_json(&t.measured, MEASURED_LABEL),
        "predicted": trace_json(&t.predicted, PREDICTED_LABEL),
    })
}

/// How a split is used: held-out trials are validation data.
pub fn split_role(t: &Trial) -> &'static str {
    if held_out(t) { "held-out (validation data)" } else { "train (fitting data)" }
}

/// A trace as chart points (time_s, value); every archived sample, the
/// raster only draws them.
fn points(trace: &Trace) -> Vec<[f64; 2]> {
    trace.samples.iter().map(|o| [o.time_s, o.value]).collect()
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

/// A load in progress (its generation is the request's `seq`).
struct Job {
    path: PathBuf,
    work: crate::jobs::Job<Review>,
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
    /// Selected trial id (in `shown`).
    pub selected: Option<String>,
    /// Trial to select once the pending load finishes (REST `path` + `trial`).
    want: Option<String>,
    /// Bumped on every installed load (redraws the chart).
    generation: u64,
    /// The chart texture (shared raster) and what it currently shows.
    pub chart: Option<Handle<Image>>,
    drawn: Option<(u64, Option<String>)>,
    /// Chart axes as drawn: value range and time window.
    pub axes: ((f64, f64), (f64, f64)),
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

    /// The selected trial in the shown archive.
    pub fn selected_trial(&self) -> Option<&Trial> {
        let id = self.selected.as_deref()?;
        self.shown.as_ref()?.archive.trials.iter().find(|t| t.id == id)
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
        let worker = dir.clone();
        self.calibration.seq += 1;
        let lost = format!("Could not load identification archive {}: the loader", dir.display());
        let work = crate::jobs::Job::spawn(crate::jobs::Pool::Io, self.calibration.seq, lost, move |_| review(&worker, &repository));
        self.calibration.job = Some(Job { path: dir.clone(), work });
        self.status = format!("Loading identification archive {} in the background…", dir.display());
        Ok(self.calibration.seq)
    }

    /// Stop waiting for a pending load (its result is dropped).
    pub fn cancel_calibration(&mut self) -> bool {
        let Some(job) = self.calibration.job.take() else { return false };
        self.status = format!("Loading identification archive {} cancelled.", job.path.display());
        self.calibration.last = Some((job.work.generation(), Err(self.status.clone())));
        self.panel_dirty = true;
        true
    }

    /// Install a finished load. Returns true when one finished.
    pub(crate) fn finish_calibration(&mut self) -> bool {
        let Some(job) = &self.calibration.job else { return false };
        let Some(result) = job.work.poll() else { return false };
        let seq = self.calibration.job.take().expect("polled above").work.generation();
        self.panel_dirty = true;
        match result {
            Ok(review) => {
                let c = counts(&review.archive);
                self.status = format!("Identification archive {}: {} trials; held-out {}/{} pass, train {}/{} pass (archive comparisons)", review.path.display(), c.all.total, c.held_out.pass, c.held_out.total, c.train.pass, c.train.total);
                let mut value = review_json(&review);
                self.calibration.shown = Some(review);
                self.calibration.error = None;
                self.calibration.page = 0;
                self.calibration.generation += 1;
                // A reload without the selected trial clears the selection.
                if let Some(id) = self.calibration.selected.clone() {
                    if self.calibration.selected_trial().is_none() {
                        self.calibration.selected = None;
                        self.status = format!("{} · trial {id} is not in this archive; selection cleared", self.status);
                    }
                }
                let mut result = Ok(value.clone());
                if let Some(id) = self.calibration.want.take() {
                    match self.select_calibration_trial(&id) {
                        Ok(()) => {
                            value["selected"] = self.calibration.selected_trial().map(selected_json).unwrap_or_default();
                            result = Ok(value);
                        }
                        Err(e) => result = Err(e),
                    }
                }
                self.calibration.last = Some((seq, result));
            }
            Err(e) => {
                self.calibration.want = None;
                self.action_error = Some(e.clone());
                self.status = e.clone();
                self.calibration.error = Some(e.clone());
                self.calibration.last = Some((seq, Err(e)));
            }
        }
        true
    }

    /// Select a trial of the shown archive (the one path for a row click,
    /// the `calibration_trial` `system_ui` action and REST `trial`). An
    /// unknown id is an error naming it; the previous selection stays.
    pub fn select_calibration_trial(&mut self, id: &str) -> Result<(), String> {
        let result = match &self.calibration.shown {
            None => Err(format!("No identification archive is loaded; cannot select trial {id}.")),
            Some(r) => match r.archive.trials.iter().find(|t| t.id == id) {
                None => Err(format!("Trial {id} is not in identification archive {}; the selection is unchanged.", r.path.display())),
                Some(t) => {
                    self.status = format!("Trial {id}: {} · {} (archive comparison)", split_role(t), if t.comparison.passes { "pass" } else { "fail" });
                    Ok(())
                }
            },
        };
        match &result {
            Ok(()) => self.calibration.selected = Some(id.to_string()),
            Err(e) => {
                self.action_error = Some(e.clone());
                self.status = e.clone();
            }
        }
        self.panel_dirty = true;
        result
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
            let trial = match args.get("trial") {
                None | Some(serde_json::Value::Null) => None,
                Some(serde_json::Value::String(s)) => Some(s.clone()),
                Some(_) => return sim_api::Outcome::Done(Err("system_calibration_review: trial must be a trial id string".into())),
            };
            // A trial without a path selects within the shown archive, no reload.
            if let (None, Some(id)) = (&path, &trial) {
                return sim_api::Outcome::Done(self.select_calibration_trial(id).map(|()| serde_json::json!({"selected": self.calibration.selected_trial().map(selected_json)})));
            }
            return match self.calibration_request(path) {
                Ok(seq) => {
                    self.calibration.want = trial;
                    *continuation = serde_json::json!({"calibration": seq});
                    sim_api::Outcome::Pending
                }
                Err(e) => sim_api::Outcome::Done(Err(e)),
            };
        };
        match &self.calibration.last {
            Some((s, result)) if *s == seq => sim_api::Outcome::Done(result.clone()),
            _ if cancelled && self.calibration.job.as_ref().is_some_and(|j| j.work.generation() == seq) => {
                self.cancel_calibration();
                sim_api::Outcome::Done(Err("cancelled".into()))
            }
            _ if self.calibration.job.as_ref().is_none_or(|j| j.work.generation() != seq) => sim_api::Outcome::Done(Err("the calibration review request was superseded".into())),
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
            "selected": c.selected_trial().map(selected_json),
            "chart": c.selected_trial().map(|_| serde_json::json!({"raster": "crate::chart (shared)", "drawn": c.drawn.as_ref().is_some_and(|d| d.0 == c.generation && d.1 == c.selected), "series": [MEASURED_LABEL, PREDICTED_LABEL], "x": "time_s (s)", "value_range": [c.axes.0.0, c.axes.0.1], "time_window_s": [c.axes.1.0, c.axes.1.1]})),
            "load_seconds": shown.map(|r| r.seconds),
            "last": c.last.as_ref().map(|(seq, r)| serde_json::json!({"seq": seq, "ok": r.is_ok(), "error": r.as_ref().err()})),
        })
    }
}

/// Redraw the selected trial's chart with the shared raster when the
/// selection or the shown load changes (not every frame). An empty trace
/// contributes no line.
pub(super) fn update_chart(mut builder: ResMut<Builder>, mut images: ResMut<Assets<Image>>) {
    let c = &builder.calibration;
    let key = (c.generation, c.selected.clone());
    if c.drawn.as_ref() == Some(&key) {
        return;
    }
    let (measured, predicted) = c.selected_trial().map(|t| (points(&t.measured), points(&t.predicted))).unwrap_or_default();
    let traces: Vec<(&[[f64; 2]], [u8; 3])> = [(measured.as_slice(), SERIES[0]), (predicted.as_slice(), SERIES[1])].into_iter().filter(|(p, _)| !p.is_empty()).collect();
    let (pixels, range, window) = crate::chart::rasterize_span(&traces, None);
    let handle = builder.calibration.chart.get_or_insert_with(|| images.add(crate::chart::blank_image())).clone();
    if let Some(mut image) = images.get_mut(&handle) {
        image.data = Some(pixels);
    }
    let c = &mut builder.calibration;
    c.axes = (range, window);
    c.drawn = Some(key);
    builder.panel_dirty = true;
}

mod panel;
pub(super) use panel::section;

#[cfg(test)]
mod tests;
