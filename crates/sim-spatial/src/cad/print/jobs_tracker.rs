//! The one poller of RoboCAD's print jobs (ui/app.py:1144-1162
//! `_watch_print_job` and :1265-1275 `print_jobs_status`).
//!
//! - **Watched jobs**: the jobs this window started (a study start's
//!   answer, adopted in [`edit_answered`] when its edit is the one
//!   `studies::send` noted). RoboCAD's window polls each with a timer
//!   every 250 ms; here one `GET /print/jobs` on a `Pool::Dedicated` job
//!   is in flight at most, started every [`POLL_INTERVAL`] while a watched
//!   job runs or the Print jobs section is open. Never a thread or job per
//!   print job. `GET /print/jobs/{id}`'s `wait` is never honoured by
//!   RoboCAD (a GET has no body), so nothing waits on it.
//! - **Status line**, RoboCAD's texts: while running "kind: message (n %)"
//!   (RoboCAD's Qt hint " — Print ▸ Print jobs… to cancel" is left out:
//!   the Print jobs section has the Cancel button), written only when it
//!   changes; once done the kind's done text (and the assembly guide or
//!   the coupons' protocol folder opened through `jobs::open_local` on a
//!   `Pool::Io` job), then the document is refetched (`sync::refresh`) and
//!   the robot reads taken again for the jobs that publish (RoboCAD's
//!   `_refresh_panels`); failed: "Kind: error" (RoboCAD's warning title
//!   and text); cancelled: "kind cancelled". A poll error is shown once by
//!   name and the poll tried again.
//! - **Polls are stamped** (a counter moved as each poll starts) and each
//!   watched job keeps the stamp current when it was adopted: a list from
//!   a poll that started before the adoption may lack the job, so the job
//!   counts as gone only when a poll started after its adoption lacks it.
//! - **Cancel** (RoboCAD's "Cancel the running jobs?"): asking opens the
//!   confirmation in the Print jobs section; Yes sends exactly one `DELETE
//!   /print/jobs/{id}` per running job of the latest list (or only the one
//!   named) on one Dedicated job, then polls. Before any list was read in
//!   this generation it is refused by name and a poll started: nothing is
//!   answered or sent from a list never read.
//! - **Leaving CAD mode** is refused while a watched job runs and the
//!   window is connected ([`PrintJobTracker::blockers`]): a self-started
//!   RoboCAD stops when CAD mode closes, which would kill the job, and an
//!   attached one would finish unseen. Not connected, nothing can be
//!   confirmed (no poll can see the job end, and a lost service took its
//!   jobs with it), so nothing is held: holding would trap the user in CAD
//!   mode with no way to clear the blocker.
//!
//! The tracker lives on the document, so it belongs to one document; a new
//! connection generation (a restarted service) starts it over.
use crate::app::actions::Call;
use crate::app::{ViewerMode, ViewerSet};
use crate::cad::document::CadDocument;
use crate::jobs::{Job, Pool};
use bevy::prelude::*;
use serde_json::{Value, json};
use sim_api::Outcome;
use sim_runtime::cad_client::{PrintJob, SplitDone};
use std::path::Path;
use std::time::{Duration, Instant};

/// How often RoboCAD's job list is read while a watched job runs or the
/// section is open.
pub(super) const POLL_INTERVAL: Duration = Duration::from_millis(500);
/// How many jobs RoboCAD's "Print jobs…" lists (the last eight).
pub(super) const SHOWN: usize = 8;
/// Cancel before any list was read in this generation.
pub(super) const UNREAD: &str = "RoboCAD's print jobs have not been read yet; try again in a moment";

/// A job this window started, with the poll stamp current at its adoption.
#[derive(Clone, Debug)]
pub(super) struct Watched {
    pub(super) job: PrintJob,
    /// `PrintJobTracker::polls` when it was adopted: only a list from a
    /// later-started poll can say it is gone.
    adopted: u64,
}

/// RoboCAD's print jobs as last polled, and the poll in flight.
#[derive(Default)]
pub struct PrintJobTracker {
    /// The connection generation the lists belong to.
    generation: u64,
    /// Whether the window was connected at the last tick (blockers hold only then).
    connected: bool,
    /// The jobs this window started that have not ended, as last polled.
    pub(super) watched: Vec<Watched>,
    /// `GET /print/jobs` as last polled, oldest first.
    pub(super) jobs: Vec<PrintJob>,
    /// A list has landed in this generation.
    pub(super) listed: bool,
    /// The poll in flight (at most one), with its stamp.
    poll: Option<(u64, Job<Vec<PrintJob>>)>,
    /// Polls started in this generation (the stamp of the latest).
    polls: u64,
    /// When the last poll started; None: poll at the next tick.
    polled_at: Option<Instant>,
    /// One poll is wanted even though the section is closed and nothing is
    /// watched (a cancel before any list was read).
    once: bool,
    /// The Print jobs section is shown.
    pub(super) open: bool,
    /// "Cancel the running jobs?" is being asked.
    pub(super) confirming: bool,
    /// The cancel in flight (its errors, naming each job).
    cancel: Option<Job<Vec<String>>>,
    /// The last poll's error (None once one succeeds).
    pub(super) error: Option<String>,
    /// The progress line this tracker last wrote.
    shown: Option<String>,
    /// Guides and folders being opened (`Pool::Io`).
    opens: Vec<Job<()>>,
}

impl PrintJobTracker {
    /// Why leaving CAD mode would drop a job this window started: one line
    /// per running watched job, while connected.
    pub(crate) fn blockers(&self) -> Vec<String> {
        if !self.connected {
            return Vec::new();
        }
        self.watched.iter().map(|w| &w.job).filter(|j| j.running()).map(|j| format!("a print job is running in RoboCAD: {} ({} %); wait for it, or cancel it in the Print jobs section", j.kind, percent(j))).collect()
    }
    /// Poll at the next tick.
    pub(super) fn request_poll(&mut self) {
        self.polled_at = None;
    }
    /// A poll is in flight.
    #[cfg(test)]
    pub(super) fn polling(&self) -> bool {
        self.poll.is_some()
    }
    /// The running jobs' ids (the last list's, then watched ones it lacks),
    /// or only `job` when it is one of them.
    pub(super) fn running(&self, job: Option<&str>) -> Vec<String> {
        let mut ids: Vec<String> = Vec::new();
        for j in self.jobs.iter().chain(self.watched.iter().map(|w| &w.job)) {
            if j.running() && !ids.contains(&j.id) && job.is_none_or(|id| id == j.id) {
                ids.push(j.id.clone());
            }
        }
        ids
    }
    /// The last [`SHOWN`] jobs as RoboCAD's lines.
    pub(super) fn lines(&self) -> Vec<String> {
        self.jobs[self.jobs.len().saturating_sub(SHOWN)..].iter().map(line).collect()
    }
}

/// Python's `f"{fraction * 100:.0f}"`: both format the exact binary value
/// (only an exact half, such as 0.125 × 100, is a tie).
pub(super) fn percent(j: &PrintJob) -> String {
    format!("{:.0}", j.fraction * 100.0)
}

/// RoboCAD's job line: "{kind} {id}: {state} {n} % {message}".
pub(super) fn line(j: &PrintJob) -> String {
    format!("{} {}: {} {} % {}", j.kind, j.id, j.state, percent(j), j.message)
}

/// The running line: "{kind}: {message or 'working'} ({n} %)".
pub(super) fn progress(j: &PrintJob) -> String {
    format!("{}: {} ({} %)", j.kind, if j.message.is_empty() { "working" } else { j.message.as_str() }, percent(j))
}

/// Python's `str.capitalize`: the first character upper case, the rest lower.
pub(super) fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map_or_else(String::new, |c| c.to_uppercase().chain(chars.flat_map(char::to_lowercase)).collect())
}

/// RoboCAD's done text for a job's result (ui/app.py:1181-1262), and the
/// local file or folder its handler opens.
pub(super) fn done_text(j: &PrintJob) -> (String, Option<String>) {
    let r = &j.result;
    let s = |v: &Value| v.as_str().map_or_else(|| v.to_string(), str::to_string);
    let len = |v: &Value| v.as_array().map_or(0, Vec::len);
    match j.kind.as_str() {
        "split" => match serde_json::from_value::<SplitDone>(r.clone()) {
            Ok(done) => (done.status(), None),
            Err(e) => (format!("split: done (RoboCAD's answer could not be read: {e})"), None),
        },
        "analyze" => {
            let parts = r["parts"].as_array().map_or(&[][..], Vec::as_slice);
            // Python's min: the first part with the least safety factor.
            let worst = parts.iter().filter_map(|p| p["safety_factor"].as_f64().map(|sf| (sf, p))).fold(None, |best: Option<(f64, &Value)>, (sf, p)| match best {
                Some((b, _)) if b <= sf => best,
                _ => Some((sf, p)),
            });
            match worst {
                Some((sf, p)) => (format!("strength: least safety factor {sf:.2} on {} ({}); Print ▸ Strength overlay shows where", s(&p["name"]), s(&p["mode"])), None),
                None => ("strength: done; RoboCAD's answer names no part with a safety factor".to_string(), None),
            }
        }
        "plan" => {
            let hours = r["total_hours"].as_f64().unwrap_or(f64::NAN);
            let grams = r["total_filament_g"].as_f64().unwrap_or(f64::NAN);
            (format!("plan: {} plate(s), about {hours:.1} h and {grams:.0} g (estimates); 3MF files in {}", len(&r["plate_files"]), s(&r["plates"])), None)
        }
        "strength_split" => (format!("{}: {}", s(&r["recommendation"]), s(&r["why"])), None),
        "assembly" => {
            let guide = r["guide"].as_str().map(str::to_string);
            (format!("assembly: {} steps; guide {}", len(&r["steps"]), s(&r["guide"])), guide)
        }
        "coupons" => {
            let folder = r["protocol"].as_str().and_then(|p| Path::new(p).parent()).map(|p| p.display().to_string());
            (format!("coupons: {} on {} plate(s); break them, fill results.json, then `sim-print promote results.json`", len(&r["coupons"]), len(&r["plates"])), folder)
        }
        other => (format!("{other}: done"), None),
    }
}

/// Whether a done job published a RoboCAD undo step (print_jobs.py
/// `_publish`): split, analyze and plan always, assembly with its exploded view.
pub(super) fn publishes(j: &PrintJob) -> bool {
    matches!(j.kind.as_str(), "split" | "analyze" | "plan") || (j.kind == "assembly" && j.result.get("exploded").is_some())
}

/// `sync::finish_edit`: edit `seq` answered (`result`: RoboCAD's answer
/// when it succeeded). If it is the print start `studies::send` noted, the
/// job RoboCAD answered is watched from now on, and polled at once.
pub(super) fn edit_answered(doc: &mut CadDocument, seq: u64, result: Option<&Value>) {
    let generation = doc.generation;
    let Some(started) = doc.print.studies.started.take() else { return };
    if started.seq != seq || started.generation != generation {
        return;
    }
    let Some(Ok(mut job)) = result.map(|v| serde_json::from_value::<PrintJob>(v.clone())) else { return };
    if job.id.is_empty() {
        return;
    }
    if job.kind.is_empty() {
        job.kind = started.kind.to_string();
    }
    let t = &mut doc.print.jobs;
    if t.generation != generation {
        reset(t, generation);
    }
    t.watched.retain(|w| w.job.id != job.id);
    // A poll in flight now started before this adoption: its list may lack the job.
    let adopted = t.polls;
    t.watched.push(Watched { job, adopted });
    t.request_poll();
    doc.touch();
}

/// A new connection generation: every job of the old one is gone with it.
fn reset(t: &mut PrintJobTracker, generation: u64) {
    let open = t.open;
    *t = PrintJobTracker { generation, open, ..PrintJobTracker::default() };
}

/// `cad_print {op: cancel}`: `confirm` None asks (the section shows the
/// question), false closes the question (saying so when none was open),
/// true sends one `DELETE /print/jobs/{id}` per running job of the latest
/// list (or only `job`) on one job, then polls. Refused with [`UNREAD`],
/// and a poll started, while no list was read in this generation.
pub(super) fn cancel(doc: &mut CadDocument, _call: &mut Call, job: Option<&str>, confirm: Option<bool>) -> Outcome {
    let done = Outcome::Done;
    if confirm == Some(false) {
        let t = &mut doc.print.jobs;
        if !t.confirming {
            return done(Ok(json!({"confirming": false, "message": "There was no cancel confirmation open; nothing changed."})));
        }
        t.confirming = false;
        doc.touch();
        return done(Ok(json!({"confirming": false, "message": "The running print jobs were left running."})));
    }
    // A new generation not ticked yet: its lists are gone (`tick` would
    // reset them too, and with them the poll asked for below).
    let generation = doc.generation;
    if doc.print.jobs.generation != generation {
        reset(&mut doc.print.jobs, generation);
    }
    // Never read in this generation: no answer or DELETE from a list never
    // read; ask RoboCAD once (even with the section closed) and refuse.
    // Otherwise the latest list (and the watched jobs) decide.
    if !doc.print.jobs.listed {
        if !doc.connected() {
            return done(Err(format!("RoboCAD's print jobs have not been read yet and cannot be now: not connected to RoboCAD: {}", doc.connection_line().0)));
        }
        let t = &mut doc.print.jobs;
        t.once = true;
        t.request_poll();
        doc.touch();
        return done(Err(UNREAD.to_string()));
    }
    let ids = doc.print.jobs.running(job);
    if ids.is_empty() {
        return done(Err(match job {
            Some(id) => format!("print job {id} is not running (as RoboCAD last listed its jobs)"),
            None => "No print jobs are running".to_string(),
        }));
    }
    if confirm.is_none() {
        let t = &mut doc.print.jobs;
        t.confirming = true;
        t.open = true;
        t.request_poll();
        doc.touch();
        return done(Ok(json!({"confirming": true, "running": ids, "message": "Cancel the running jobs?"})));
    }
    if doc.print.jobs.cancel.is_some() {
        return done(Err("the running print jobs are already being cancelled".into()));
    }
    let Some(client) = doc.client.clone().filter(|_| doc.connected()) else {
        return done(Err(format!("not connected to RoboCAD: {}", doc.connection_line().0)));
    };
    let sent = ids.clone();
    let job = Job::spawn(Pool::Dedicated, doc.generation, "cad print cancel", move |_| {
        // Exactly one DELETE per job; an error names its job and the rest still go.
        Ok(sent.iter().filter_map(|id| client.cancel_print_job(id).err().map(|e| format!("{id}: {e}"))).collect::<Vec<String>>())
    });
    let t = &mut doc.print.jobs;
    t.cancel = Some(job);
    t.confirming = false;
    let n = ids.len();
    doc.show(Ok(format!("Cancelling {n} print job(s)…")));
    done(Ok(json!({"cancelling": ids})))
}

/// The landed list of poll `stamp`: the watched jobs' progress or end on
/// the status line.
fn land(doc: &mut CadDocument, stamp: u64, result: Result<Vec<PrintJob>, String>) {
    let list = match result {
        Ok(list) => list,
        Err(e) => {
            let first = doc.print.jobs.error.as_deref() != Some(e.as_str());
            doc.print.jobs.error = Some(e.clone());
            if first {
                doc.show(Err(format!("RoboCAD's print jobs could not be read: {e}; trying again")));
            }
            return;
        }
    };
    let t = &mut doc.print.jobs;
    let changed = t.jobs != list || t.error.is_some() || !t.listed;
    t.error = None;
    t.listed = true;
    // A landed list answers a one-shot request made while this poll was in flight.
    t.once = false;
    t.jobs = list;
    let mut running_line = None;
    let mut ended: Vec<Result<PrintJob, PrintJob>> = Vec::new();
    for w in std::mem::take(&mut t.watched) {
        match t.jobs.iter().find(|j| j.id == w.job.id) {
            Some(j) if j.running() => {
                running_line = Some(progress(j));
                t.watched.push(Watched { job: j.clone(), adopted: w.adopted });
            }
            Some(j) => ended.push(Ok(j.clone())),
            // Polled before the adoption: this list cannot know the job yet.
            None if stamp <= w.adopted => t.watched.push(w),
            // RoboCAD no longer lists it.
            None => ended.push(Err(w.job)),
        }
    }
    if let Some(text) = running_line
        && t.shown.as_ref() != Some(&text)
    {
        t.shown = Some(text.clone());
        doc.show(Ok(text));
    }
    for j in ended {
        finish(doc, j);
    }
    if changed {
        doc.touch();
    }
}

/// A watched job ended (`Err`: RoboCAD no longer lists it).
fn finish(doc: &mut CadDocument, job: Result<PrintJob, PrintJob>) {
    doc.print.jobs.shown = None;
    let j = match job {
        Ok(j) => j,
        Err(w) => {
            doc.show(Err(format!("{} {}: RoboCAD no longer lists this print job", w.kind, w.id)));
            return;
        }
    };
    match j.state.as_str() {
        "done" => {
            let (text, open) = done_text(&j);
            doc.show(Ok(text));
            if let Some(path) = open {
                let job = Job::spawn(Pool::Io, doc.generation, "open a print job's output", move |_| crate::jobs::open_local(Path::new(&path)).map_err(|e| format!("could not open {path}: {e}")));
                doc.print.jobs.opens.push(job);
            }
            if publishes(&j) {
                // A publish is an edit (one RoboCAD undo step): the saved
                // state is unknown until the refetch answers, so
                // `switch_blockers` never sees a stale "saved" in the frame
                // this job's own blocker goes away.
                crate::cad::sync::refresh(doc, true);
                // The robot reads again: the stress paint is keyed by each
                // node's results (`GET /results/nodes`), so a finished
                // analyze recolours from the new read without an explicit clear.
                doc.robot.data.invalidate();
            }
        }
        "failed" => doc.show(Err(format!("{}: {}", capitalize(&j.kind), j.error.as_deref().filter(|e| !e.is_empty()).unwrap_or("failed")))),
        _ => doc.show(Ok(format!("{} cancelled", j.kind))),
    }
}

/// One frame: a new generation starts over; landed opens, cancels and
/// polls are applied; a poll is started when due.
pub(super) fn tick(doc: &mut CadDocument) {
    let generation = doc.generation;
    let connected = doc.connected();
    let mut statuses: Vec<Result<String, String>> = Vec::new();
    let t = &mut doc.print.jobs;
    let mut touched = t.connected != connected;
    t.connected = connected;
    if t.generation != generation {
        reset(t, generation);
        t.connected = connected;
        touched = true;
    }
    t.opens.retain(|j| match j.poll() {
        None => true,
        Some(Ok(())) => false,
        Some(Err(e)) => {
            statuses.push(Err(e));
            false
        }
    });
    let cancelled = t.cancel.as_ref().and_then(Job::poll);
    if let Some(result) = cancelled {
        t.cancel = None;
        t.request_poll();
        touched = true;
        match result {
            Ok(errors) if errors.is_empty() => {}
            Ok(errors) => statuses.push(Err(format!("RoboCAD did not cancel every print job: {}", errors.join("; ")))),
            Err(e) => statuses.push(Err(e)),
        }
    }
    let landed = t.poll.as_ref().and_then(|(stamp, j)| j.poll().map(|r| (*stamp, j.generation(), r)));
    for status in statuses {
        doc.show(status);
    }
    if let Some((stamp, g, result)) = landed {
        doc.print.jobs.poll = None;
        // `land` touches the document only when what it shows changed.
        if g == generation {
            land(doc, stamp, result);
        }
    }
    let t = &doc.print.jobs;
    let due = t.poll.is_none() && (t.open || t.once || !t.watched.is_empty()) && t.polled_at.is_none_or(|at| at.elapsed() >= POLL_INTERVAL);
    if due && let Some(client) = doc.client.clone().filter(|_| connected) {
        let job = Job::spawn(Pool::Dedicated, generation, "cad print jobs", move |_| client.print_jobs().map_err(|e| e.to_string()));
        let t = &mut doc.print.jobs;
        t.polls += 1;
        t.poll = Some((t.polls, job));
        t.once = false;
        t.polled_at = Some(Instant::now());
    }
    if touched {
        doc.touch();
    }
}

/// `cad_state.print.jobs`.
pub(super) fn state_json(doc: &CadDocument) -> Value {
    let t = &doc.print.jobs;
    let job = |j: &PrintJob| json!({"id": j.id, "kind": j.kind, "state": j.state, "fraction": j.fraction, "message": j.message, "error": j.error, "out_dir": j.out_dir, "seconds": j.seconds, "line": line(j)});
    json!({
        "watched": t.watched.iter().map(|w| job(&w.job)).collect::<Vec<_>>(),
        "jobs": t.jobs[t.jobs.len().saturating_sub(SHOWN)..].iter().map(job).collect::<Vec<_>>(),
        "polling": t.poll.is_some(),
        "interval_ms": POLL_INTERVAL.as_millis() as u64,
        "confirming": t.confirming,
        "cancelling": t.cancel.is_some(),
        "error": t.error,
        "blockers": t.blockers(),
    })
}

/// JobResults: [`tick`].
fn poll(doc: Option<ResMut<CadDocument>>) {
    let Some(mut doc) = doc else { return };
    tick(&mut doc);
}

/// CadCorePlugin: the poller (JobResults, after `sync::receive`, which
/// adopts a started job through [`edit_answered`]).
pub(super) fn build_core(app: &mut App) {
    app.add_systems(Update, poll.after(crate::cad::CadSet::Results).in_set(ViewerSet::JobResults).run_if(in_state(ViewerMode::Cad)));
}
