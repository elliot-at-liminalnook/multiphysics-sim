//! Read-only gait-lab results browser (build mode, Gait lab tab).
//!
//! `Builder::gait_reports_request` is the one entry point: the results-root
//! path field, Reload, the first visit to the tab, and REST
//! `system_gait_reports` all call it. It scans the root on a worker thread
//! with `sim_runtime::gait_lab::scan_results` (report parsing, `kind`
//! dispatch and journal matching live there, not here); `finish_gait_reports`
//! installs the listing on the UI thread. Nothing is written and no
//! evaluation is started.
use super::ui::{Kit, Look, num, wrap, ACCENT_BG, HOVER_BG, DANGER, FAINT, OK, SUBTLE, TEXT, WARN};
use super::*;
use sim_runtime::gait_lab::{JointLine, LabReport, ResultsEntry, ResultsListing, scan_results};
use std::path::Path;

/// The tracked results root, relative to the workspace root.
pub const DEFAULT_RESULTS: &str = "examples/full-robot/measured-actuator-integration/gait-lab-2026-09-25/results";

/// Shown whenever the tab is: reports carry no runtime fingerprint.
pub const CAVEAT: &str = "Reports do not record the runtime fingerprint, so whether they are still qualified against the current code is unknown. Fast-fidelity results must be confirmed on the detailed model.";

/// One finished scan.
#[derive(Clone, Debug)]
pub struct Scan {
    pub listing: ResultsListing,
    pub seconds: f64,
}

/// The default results root: `DEFAULT_RESULTS` under the nearest ancestor of
/// the open system file, else the workspace root (`crate::workspace`) if it has it.
pub fn default_results(system: &Path) -> Result<PathBuf, String> {
    let system = std::path::absolute(system).unwrap_or_else(|_| system.to_path_buf());
    let root = crate::workspace::root();
    system.ancestors().skip(1).chain(root.iter().copied()).map(|d| d.join(DEFAULT_RESULTS)).find(|p| p.is_dir()).ok_or_else(|| {
        let root = root.map(|r| r.display().to_string()).unwrap_or_else(|e| format!("no workspace root ({e})"));
        format!("No gait-lab results found: {DEFAULT_RESULTS} is not under any folder above {} or the workspace root {root}. Type a results folder.", system.display())
    })
}

fn kind(r: &LabReport) -> &'static str {
    match r {
        LabReport::Gait(_) => "gait",
        LabReport::Pose(_) => "pose",
        LabReport::Maneuver(_) => "maneuver",
    }
}

/// An entry as `system_state` and REST show it: flat, with the report's own
/// fields under `report` or its error under `error`.
pub fn entry_json(e: &ResultsEntry) -> serde_json::Value {
    let mut v = serde_json::json!({"name": e.name, "directory": e.directory, "journal": e.journal});
    match &e.report {
        Ok(r) => {
            v["kind"] = kind(r).into();
            v["status"] = r.status().into();
            v["report"] = match r {
                LabReport::Gait(g) => serde_json::to_value(g),
                LabReport::Pose(p) => serde_json::to_value(p),
                LabReport::Maneuver(m) => serde_json::to_value(m),
            }
            .unwrap_or_default();
        }
        Err(msg) => {
            v["kind"] = serde_json::Value::Null;
            v["status"] = serde_json::Value::Null;
            v["error"] = msg.as_str().into();
        }
    }
    v
}

pub fn listing_json(s: &Scan) -> serde_json::Value {
    let l = &s.listing;
    serde_json::json!({"root": l.root, "journal": l.journal, "warnings": l.warnings, "entries": l.entries.iter().map(entry_json).collect::<Vec<_>>(), "load_seconds": s.seconds})
}

/// `unix_s` as a UTC date and time (proleptic Gregorian).
pub fn utc(unix_s: u64) -> String {
    let (days, secs) = ((unix_s / 86400) as i64, unix_s % 86400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC", secs / 3600, secs / 60 % 60, secs % 60)
}

struct Job {
    seq: u64,
    root: PathBuf,
    receiver: Mutex<mpsc::Receiver<Result<Scan, String>>>,
}

#[derive(Default)]
pub(super) struct GaitLabState {
    job: Option<Job>,
    seq: u64,
    /// Root of the latest request (the path field's value).
    pub root: Option<PathBuf>,
    /// The latest good scan, labelled with its own root.
    pub shown: Option<Scan>,
    /// The latest failure (names the path); cleared by a good scan.
    pub error: Option<String>,
    /// Directory name of the entry shown in detail.
    pub selected: Option<String>,
    last: Option<(u64, Result<serde_json::Value, String>)>,
}

impl GaitLabState {
    pub fn pending(&self) -> Option<&Path> {
        self.job.as_ref().map(|j| j.root.as_path())
    }
}

impl Builder {
    /// Scan a gait-lab results root off the UI thread (the shared path for
    /// the Gait lab tab, `system_ui` and REST `system_gait_reports`).
    /// `dir: None` rescans the current root (the default at first). Refuses
    /// while a scan is pending. Read-only.
    pub fn gait_reports_request(&mut self, dir: Option<PathBuf>) -> Result<u64, String> {
        let result = self.start_gait_reports(dir);
        if let Err(e) = &result {
            self.gait_lab.error = Some(e.clone());
            self.action_error = Some(e.clone());
            self.status = e.clone();
        }
        self.panel_dirty = true;
        result
    }

    fn start_gait_reports(&mut self, dir: Option<PathBuf>) -> Result<u64, String> {
        if let Some(pending) = self.gait_lab.pending() {
            return Err(format!("Still reading gait-lab results {}; wait for it before another request.", pending.display()));
        }
        let root = match dir.or_else(|| self.gait_lab.root.clone()) {
            Some(p) => std::path::absolute(&p).unwrap_or(p),
            None => default_results(&self.store.path)?,
        };
        self.gait_lab.root = Some(root.clone());
        let (send, receive) = mpsc::channel();
        let worker = root.clone();
        std::thread::spawn(move || {
            let started = std::time::Instant::now();
            let _ = send.send(scan_results(&worker).map(|listing| Scan { listing, seconds: started.elapsed().as_secs_f64() }).map_err(|e| format!("Could not read gait-lab results: {e}")));
        });
        self.gait_lab.seq += 1;
        self.gait_lab.job = Some(Job { seq: self.gait_lab.seq, root: root.clone(), receiver: Mutex::new(receive) });
        self.status = format!("Reading gait-lab results {} in the background…", root.display());
        Ok(self.gait_lab.seq)
    }

    /// Stop waiting for a pending scan (its result is dropped).
    pub fn cancel_gait_reports(&mut self) -> bool {
        let Some(job) = self.gait_lab.job.take() else { return false };
        self.status = format!("Reading gait-lab results {} cancelled.", job.root.display());
        self.gait_lab.last = Some((job.seq, Err(self.status.clone())));
        self.panel_dirty = true;
        true
    }

    /// Install a finished scan. Returns true when one finished.
    pub(crate) fn finish_gait_reports(&mut self) -> bool {
        let Some(job) = &self.gait_lab.job else { return false };
        let polled = job.receiver.lock().map(|r| r.try_recv()).unwrap_or(Err(mpsc::TryRecvError::Disconnected));
        let result = match polled {
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => Err(format!("Could not read gait-lab results {}: the reader ended without a result.", job.root.display())),
            Ok(r) => r,
        };
        let job = self.gait_lab.job.take().expect("polled above");
        self.panel_dirty = true;
        match result {
            Ok(scan) => {
                let l = &scan.listing;
                let bad = l.entries.iter().filter(|e| e.report.is_err()).count();
                self.status = format!("Gait-lab results {}: {} reports{}", l.root, l.entries.len(), if bad == 0 { String::new() } else { format!(", {bad} unreadable") });
                if !self.gait_lab.selected.as_ref().is_some_and(|s| l.entries.iter().any(|e| &e.name == s)) {
                    self.gait_lab.selected = l.entries.first().map(|e| e.name.clone());
                }
                let value = listing_json(&scan);
                self.gait_lab.shown = Some(scan);
                self.gait_lab.error = None;
                self.gait_lab.last = Some((job.seq, Ok(value)));
            }
            Err(e) => {
                self.action_error = Some(e.clone());
                self.status = e.clone();
                self.gait_lab.error = Some(e.clone());
                self.gait_lab.last = Some((job.seq, Err(e)));
            }
        }
        true
    }

    /// Show one entry of the current listing in detail.
    pub(crate) fn select_gait_report(&mut self, name: String) -> Result<(), String> {
        let known = self.gait_lab.shown.as_ref().is_some_and(|s| s.listing.entries.iter().any(|e| e.name == name));
        if !known {
            return Err(format!("No gait-lab report named {name} in the shown results."));
        }
        self.gait_lab.selected = Some(name);
        self.panel_dirty = true;
        Ok(())
    }

    /// REST `system_gait_reports`: start (or refuse) on the first call, then
    /// report the listing once `finish_gait_reports` has run.
    pub(crate) fn gait_reports_rest(&mut self, args: &serde_json::Value, continuation: &mut serde_json::Value, cancelled: bool) -> sim_api::Outcome {
        let Some(seq) = continuation.get("gait_reports").and_then(|s| s.as_u64()) else {
            let dir = match args.get("dir") {
                None | Some(serde_json::Value::Null) => None,
                Some(serde_json::Value::String(s)) => Some(PathBuf::from(s)),
                Some(_) => return sim_api::Outcome::Done(Err("system_gait_reports: dir must be a path string".into())),
            };
            return match self.gait_reports_request(dir) {
                Ok(seq) => {
                    *continuation = serde_json::json!({"gait_reports": seq});
                    sim_api::Outcome::Pending
                }
                Err(e) => sim_api::Outcome::Done(Err(e)),
            };
        };
        match &self.gait_lab.last {
            Some((s, result)) if *s == seq => sim_api::Outcome::Done(result.clone()),
            _ if cancelled && self.gait_lab.job.as_ref().is_some_and(|j| j.seq == seq) => {
                self.cancel_gait_reports();
                sim_api::Outcome::Done(Err("cancelled".into()))
            }
            _ if self.gait_lab.job.as_ref().is_none_or(|j| j.seq != seq) => sim_api::Outcome::Done(Err("the gait report request was superseded".into())),
            _ => sim_api::Outcome::Pending,
        }
    }

    pub(super) fn gait_reports_json(&self) -> serde_json::Value {
        let g = &self.gait_lab;
        let shown = g.shown.as_ref();
        serde_json::json!({
            "read_only": true,
            "caveat": CAVEAT,
            "pending": g.pending(),
            "requested": g.root,
            "root": shown.map(|s| &s.listing.root),
            "journal": shown.map(|s| s.listing.journal),
            "warnings": shown.map(|s| &s.listing.warnings),
            "entries": shown.map(|s| s.listing.entries.iter().map(entry_json).collect::<Vec<_>>()),
            "load_seconds": shown.map(|s| s.seconds),
            "selected": g.selected,
            "error": g.error,
            "last": g.last.as_ref().map(|(seq, r)| serde_json::json!({"seq": seq, "ok": r.is_ok(), "error": r.as_ref().err()})),
        })
    }
}

fn status_color(status: &str) -> Color {
    match status {
        "passed" | "ready" => OK,
        "rejected" => DANGER,
        "blocked" => WARN,
        "screened_out" => Color::srgb(0.690, 0.604, 0.902),
        "invalid" => Color::srgb(0.937, 0.357, 0.620),
        _ => SUBTLE,
    }
}

fn or_none(v: Option<f64>, unit: &str) -> String {
    v.map(|v| format!("{} {unit}", num(v)).trim_end().to_string()).unwrap_or_else(|| "none".into())
}

/// The Gait lab tab: a compact list of every report under the root, and the
/// selected report in full below it.
pub(super) fn tab(body: &mut ChildSpawnerCommands, k: &Kit, b: &Builder) {
    let g = &b.gait_lab;
    body.spawn(k.text("Gait-lab results as recorded: each report.yaml under a results folder, with its journal time when one exists. Read-only; nothing is evaluated here.", 12., SUBTLE, 0));
    body.spawn(k.text(CAVEAT, 11.5, WARN, 1));
    body.spawn(k.section("Results folder"));
    let focused = b.input.as_ref().is_some_and(|i| i.purpose == Purpose::GaitResults);
    let shown = if focused { b.input.as_ref().map(|i| i.buffer.clone()).unwrap_or_default() } else { g.root.as_ref().map(|p| p.display().to_string()).unwrap_or_default() };
    body.spawn(k.input(&shown, "Path to a gait-lab results folder · Enter to read", BuildAction::GaitResultsPath, focused));
    body.spawn(Node { margin: UiRect::top(Val::Px(6.)), ..wrap() }).with_children(|r| {
        r.spawn(k.button("Reload", BuildAction::GaitReload, Look::Secondary, g.pending().is_none()));
        if g.pending().is_some() {
            r.spawn(k.button("Cancel", BuildAction::CancelGaitReports, Look::Danger, true));
        }
    });
    if let Some(pending) = g.pending() {
        body.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Center, margin: UiRect::top(Val::Px(6.)), flex_shrink: 0., ..default() }, children![k.dot(ACCENT), k.text(format!("Reading {}…", pending.display()), 12., TEXT, 0)]));
    }
    if let Some(e) = &g.error {
        body.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Start, margin: UiRect::top(Val::Px(6.)), flex_shrink: 0., ..default() }, children![k.dot(DANGER), k.text(e, 12., DANGER, 0)]));
    }
    let Some(scan) = &g.shown else {
        if g.pending().is_none() && g.error.is_none() {
            body.spawn(k.text("Not read yet.", 12., SUBTLE, 0));
        }
        return;
    };
    let l = &scan.listing;
    if g.error.is_some() {
        body.spawn(k.text(format!("Still showing the last good read: {}", l.root), 11.5, WARN, 1));
    }
    body.spawn(k.text(format!("Showing {}", l.root), 11., FAINT, 0));
    body.spawn(k.text(if l.journal { "journal.jsonl present: gait entries with a journal line show its time." } else { "No journal.jsonl here, so no entry has a recorded time." }, 11., FAINT, 0));
    for w in &l.warnings {
        body.spawn(k.text(format!("⚠ {w}"), 11.5, WARN, 0));
    }

    body.spawn(k.section(&format!("Reports  {}", l.entries.len())));
    if l.entries.is_empty() {
        body.spawn(k.text("No report.yaml in any subfolder.", 12., SUBTLE, 0));
    }
    for e in &l.entries {
        let selected = g.selected.as_deref() == Some(e.name.as_str());
        let (color, line) = match &e.report {
            Ok(r) => {
                let speed = match r {
                    LabReport::Gait(gr) => format!(" · {}", or_none(gr.speed_m_s, "m/s")),
                    _ => String::new(),
                };
                (status_color(r.status()), format!("{} · {}{speed}", kind(r), r.status()))
            }
            Err(_) => (DANGER, "unreadable".to_string()),
        };
        body.spawn((
            Button,
            BuildAction::GaitReportSelect(e.name.clone()),
            ui::Tint { idle: if selected { ACCENT_BG } else { Color::NONE }, hover: if selected { ACCENT_BG } else { HOVER_BG } },
            Node { padding: UiRect::axes(Val::Px(8.), Val::Px(4.)), column_gap: Val::Px(8.), align_items: AlignItems::Center, border: UiRect::left(Val::Px(2.)), flex_shrink: 0., ..default() },
            BorderColor(if selected { ACCENT } else { Color::NONE }),
            BorderRadius::all(Val::Px(4.)),
            BackgroundColor(if selected { ACCENT_BG } else { Color::NONE }),
            children![
                k.dot(color),
                (Node { flex_direction: FlexDirection::Column, flex_grow: 1., min_width: Val::Px(0.), ..default() }, children![k.text(&e.name, 12., TEXT, 1), k.text(line, 11., color, 0)])
            ],
        ));
        if let Err(msg) = &e.report {
            body.spawn(k.text(msg, 11., DANGER, 0));
        }
    }

    let Some(e) = g.selected.as_ref().and_then(|s| l.entries.iter().find(|e| &e.name == s)) else { return };
    body.spawn(k.section(&format!("Report  {}", e.name)));
    match &e.report {
        Err(msg) => {
            body.spawn(k.text(msg, 12., DANGER, 0));
        }
        Ok(r) => {
            body.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Center, flex_shrink: 0., ..default() }, children![k.dot(status_color(r.status())), k.text(format!("{} · {}", kind(r), r.status()), 12.5, status_color(r.status()), 2)]));
            let (summary, reasons, joints) = match r {
                LabReport::Gait(x) => (&x.summary, &x.reasons, &x.joints),
                LabReport::Pose(x) => (&x.summary, &x.reasons, &x.joints),
                LabReport::Maneuver(x) => (&x.summary, &x.reasons, &x.joints),
            };
            body.spawn(k.text(summary, 12., TEXT, 0));
            match r {
                LabReport::Gait(x) => {
                    body.spawn(k.text("Fidelity", 11., FAINT, 2));
                    body.spawn(k.text(&x.fidelity, 11.5, if x.fidelity.starts_with("fast") { WARN } else { TEXT }, 0));
                    k.property(body, "Gait", &x.gait, "", None::<BuildAction>, false);
                    k.property(body, "Speed", &or_none(x.speed_m_s, "m/s"), "", None::<BuildAction>, false);
                    k.property(body, "Forward distance", &or_none(x.forward_distance_m, "m"), "", None::<BuildAction>, false);
                    k.property(body, "Simulated", &x.simulated_s.map(|s| format!("{} s", num(s))).unwrap_or_else(|| "not simulated".into()), "", None::<BuildAction>, false);
                    if !x.gates.is_empty() {
                        body.spawn(k.section("Gates"));
                        for gate in &x.gates {
                            body.spawn(Node { flex_direction: FlexDirection::Column, padding: UiRect::vertical(Val::Px(2.)), flex_shrink: 0., ..default() }).with_children(|row| {
                                row.spawn(Node { justify_content: JustifyContent::SpaceBetween, column_gap: Val::Px(8.), ..default() }).with_children(|top| {
                                    top.spawn(k.text(&gate.gate, 11.5, TEXT, 1));
                                    top.spawn(k.text(if gate.ok { "✓ ok" } else { "✗ failed" }, 11.5, if gate.ok { OK } else { DANGER }, 2));
                                });
                                row.spawn(k.text(format!("value {} · limit {}", num(gate.value), num(gate.limit)), 11., SUBTLE, 0));
                            });
                        }
                    }
                }
                LabReport::Pose(x) => {
                    k.property(body, "Sequence", &x.sequence, "", None::<BuildAction>, false);
                    k.property(body, "Period", &or_none(x.period_s, "s"), "", None::<BuildAction>, false);
                }
                LabReport::Maneuver(x) => {
                    k.property(body, "Maneuver", &x.maneuver, "", None::<BuildAction>, false);
                    k.property(body, "Gait", &x.gait, "", None::<BuildAction>, false);
                    k.property(body, "Checked", &num(x.checked_s), "s", None::<BuildAction>, false);
                    k.property(body, "Travel x, y", &format!("{}, {}", num(x.travel_m[0]), num(x.travel_m[1])), "m", None::<BuildAction>, false);
                    k.property(body, "Turned", &num(x.turned_deg), "°", None::<BuildAction>, false);
                    k.property(body, "Max marker error", &num(x.maximum_marker_error_m), "m", None::<BuildAction>, false);
                    k.property(body, "Maneuver overlap", &num(x.maneuver_overlap_mm), "mm", None::<BuildAction>, false);
                    k.property(body, "Gait overlap", &num(x.gait_overlap_mm), "mm", None::<BuildAction>, false);
                }
            }
            if !reasons.is_empty() {
                body.spawn(k.section("Reasons"));
                for reason in reasons {
                    body.spawn(k.text(format!("· {reason}"), 11.5, TEXT, 0));
                }
            }
            if !joints.is_empty() {
                body.spawn(k.section("Joint speed (top 5)"));
                for j in joints.iter().take(5) {
                    joint_row(body, k, j);
                }
                if joints.len() > 5 {
                    body.spawn(k.text(format!("{} more in system_state.gait_reports", joints.len() - 5), 10.5, FAINT, 0));
                }
            }
            body.spawn(k.section("Files"));
            let (file, dir, compiled) = match r {
                LabReport::Gait(x) => (&x.file, &x.results_directory, x.compiled_gait.as_ref()),
                LabReport::Pose(x) => (&x.file, &x.results_directory, x.compiled_gait.as_ref()),
                LabReport::Maneuver(x) => (&x.file, &x.results_directory, None),
            };
            body.spawn(k.text(format!("file {file}"), 10.5, SUBTLE, 0));
            body.spawn(k.text(format!("results {dir}"), 10.5, SUBTLE, 0));
            if let Some(c) = compiled {
                body.spawn(k.text(format!("compiled {c}"), 10.5, SUBTLE, 0));
            }
        }
    }
    body.spawn(k.section("Journal"));
    match &e.journal {
        Some(j) => {
            k.property(body, "Recorded", &utc(j.unix_s), "", None::<BuildAction>, false);
            k.property(body, "unix_s", &j.unix_s.to_string(), "", None::<BuildAction>, false);
            k.property(body, "Cached", if j.cached { "yes (reused an earlier result)" } else { "no" }, "", None::<BuildAction>, false);
            k.property(body, "Journal line", &j.line.to_string(), "", None::<BuildAction>, false);
        }
        None => {
            body.spawn(k.text("No journal entry (no timestamp recorded).", 11.5, SUBTLE, 0));
        }
    }
}

fn joint_row(body: &mut ChildSpawnerCommands, k: &Kit, j: &JointLine) {
    let percent = j.percent_of_limit.map(|p| format!("{}% of limit", num(p))).unwrap_or_else(|| "none".into());
    let color = match j.percent_of_limit { Some(p) if p > 100. => DANGER, Some(p) if p > 80. => WARN, _ => TEXT };
    body.spawn(Node { flex_direction: FlexDirection::Column, padding: UiRect::vertical(Val::Px(2.)), flex_shrink: 0., ..default() }).with_children(|row| {
        row.spawn(Node { justify_content: JustifyContent::SpaceBetween, column_gap: Val::Px(8.), ..default() }).with_children(|top| {
            top.spawn(k.text(&j.joint, 11.5, TEXT, 1));
            top.spawn(k.text(percent, 11.5, color, 1));
        });
        row.spawn(k.text(format!("peak {} · limit {}", or_none(j.reference_peak_rad_s, "rad/s"), or_none(j.motor_limit_rad_s, "rad/s")), 11., SUBTLE, 0));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait(b: &mut Builder) {
        for _ in 0..1200 {
            if b.finish_gait_reports() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        panic!("gait-lab scan did not finish");
    }

    #[test]
    fn gait_reports_load_and_name_bad_paths() {
        let root = std::path::absolute(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap();
        let system = root.join("examples/systems-builder/motor-driver-board/board.system.json");
        let registry = sim_runtime::registry_with_parts(&root.join("library/parts")).0;
        let mut b = Builder::open(system, root.join("library/systems"), registry).unwrap();
        assert_eq!(utc(1790438434), "2026-09-26 16:00:34 UTC");

        // Tracked, read only: two screened-out gaits with journal lines.
        let screen = root.join("examples/full-robot/measured-actuator-integration/gait-lab-2026-09-25/results-legscreen");
        let seq = b.gait_reports_request(Some(screen.clone())).unwrap();
        assert!(b.gait_lab.pending().is_some());
        assert!(b.gait_reports_request(None).unwrap_err().contains("Still reading"));
        wait(&mut b);
        assert_eq!(b.gait_lab.last.as_ref().unwrap().0, seq);
        let state = b.state_json();
        let g = &state["gait_reports"];
        let entries = g["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|e| e["kind"] == "gait" && e["status"] == "screened_out"), "{entries:?}");
        assert!(entries[0]["report"]["speed_m_s"].is_null() && entries[0]["report"]["fidelity"].is_string());
        assert_eq!(entries[0]["journal"]["unix_s"], 1790438434u64);
        assert_eq!(g["selected"], entries[0]["name"]);
        b.select_gait_report(entries[1]["name"].as_str().unwrap().into()).unwrap();
        assert!(b.select_gait_report("nope".into()).is_err());

        // A missing root fails on the worker, names the path, keeps the last good listing.
        let missing = root.join("target/no-such-gait-results");
        b.gait_reports_request(Some(missing.clone())).unwrap();
        wait(&mut b);
        let e = b.gait_lab.error.clone().unwrap();
        assert!(e.contains(&missing.display().to_string()), "{e}");
        assert_eq!(b.gait_lab.shown.as_ref().unwrap().listing.root, screen.display().to_string());
        assert_eq!(b.state_json()["gait_reports"]["entries"].as_array().unwrap().len(), 2);
    }
}
