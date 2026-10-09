//! The print jobs, run in process (RoboCAD's `print_jobs.py` `PrintJobs`
//! and its window's `_watch_print_job` / `print_jobs_status`).
//!
//! - **Starting** ([`start`]): a study's start (`studies::send`) or REST
//!   takes the shown snapshot (refused by name when the document moved
//!   since the form or selection was read, RoboCAD's `check_revision`) and
//!   runs `sim_cad::print::jobs::run` on one `Pool::Dedicated` job: split,
//!   analyze, plan, strength_split, assembly or coupons. The stress check
//!   and planner run in process too (`sim_runtime::print_tools`, the
//!   `sim-print` binary's library). Never a thread per job outside `jobs`.
//! - **Progress**: the job's fraction and message, read each frame;
//!   "kind: message (n %)" on the status line, written only when it changes.
//! - **Publishing**: a job that changes the document (split, analyze, plan,
//!   assembly with its exploded view) hands back a staged edit; it lands
//!   as one undo step through `local::start` once no other edit is in
//!   flight, and only if the document is still the snapshot the job read
//!   (otherwise the job fails by name: run it again). The kind's done text
//!   follows (the assembly guide or the coupons' folder opened through
//!   `jobs::open_local` on a `Pool::Io` job); failed: "Kind: error";
//!   cancelled: "kind cancelled".
//! - **Cancel** (RoboCAD's "Cancel the running jobs?"): asking opens the
//!   confirmation in the Print jobs section; Yes cancels each running job
//!   (it stops at its next check, and never publishes).
//! - **Leaving CAD mode** is refused while a job runs or waits to publish
//!   ([`PrintJobTracker::blockers`]): its result would land unseen.
//!
//! The tracker lives on the document; a new document generation drops its
//! jobs (each cancelled as its handle drops).
use crate::app::actions::Call;
use crate::app::{ViewerMode, ViewerSet};
use crate::cad::document::{CadDocument, EditDone};
use crate::jobs::{Job, Pool};
use bevy::prelude::*;
use serde_json::{Value, json};
use sim_api::Outcome;
use crate::cad::types::{PrintJob, SplitDone};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// How many jobs "Print jobs…" lists (the last eight).
pub(super) const SHOWN: usize = 8;
/// How many finished jobs are kept for the list and `cad_state`.
const KEPT: usize = 32;

/// `sim-print analyze|plan` in this process (`sim_runtime::print_tools`).
struct InProcess;
impl sim_cad::print::Runner for InProcess {
    fn run(&self, command: &str, study: &Path, out: &Path, progress: &dyn Fn(f64, &str) -> bool) -> Result<Value, String> {
        sim_runtime::print_tools::run(command, study, Some(out), &mut |f, m| progress(f, m))
    }
}

/// What a finished job hands back: its result, its folder, and the staged
/// edit to publish with its label and the snapshot it was made against.
struct Finished {
    result: Value,
    out_dir: Option<PathBuf>,
    edit: Option<(String, sim_cad::Edit)>,
}

/// A job in flight.
struct Running {
    id: String,
    work: Job<Finished>,
    /// The archive identity of the snapshot it read.
    identity: String,
    since: Instant,
}

/// A finished job's edit waiting for the edit slot.
struct Publish {
    id: String,
    label: String,
    edit: sim_cad::Edit,
    identity: String,
    result: Value,
}

/// The print jobs of this document: the list (oldest first), the work in
/// flight and the edits waiting to publish.
#[derive(Default)]
pub struct PrintJobTracker {
    /// The document generation the jobs belong to.
    generation: u64,
    /// Every job this document ran, oldest first (the last [`KEPT`]).
    pub(super) jobs: Vec<PrintJob>,
    running: Vec<Running>,
    publish: Vec<Publish>,
    /// Jobs started (ids are `p<n>`).
    started: u64,
    /// The Print jobs section is shown.
    pub(super) open: bool,
    /// "Cancel the running jobs?" is being asked.
    pub(super) confirming: bool,
    /// The progress line this tracker last wrote.
    shown: Option<String>,
    /// Guides and folders being opened (`Pool::Io`).
    opens: Vec<Job<()>>,
    /// Where job folders go; None: `sim_cad::print::runs_dir()`.
    pub(super) runs: Option<PathBuf>,
}

impl PrintJobTracker {
    /// Why leaving CAD mode would drop a job: one line per running or publishing job.
    pub(crate) fn blockers(&self) -> Vec<String> {
        self.jobs.iter().filter(|j| j.running()).map(|j| format!("a print job is running: {} ({} %); wait for it, or cancel it in the Print jobs section", j.kind, percent(j))).collect()
    }
    /// The running jobs' ids, or only `job` when it is one of them.
    pub(super) fn running(&self, job: Option<&str>) -> Vec<String> {
        self.jobs.iter().filter(|j| j.running() && job.is_none_or(|id| id == j.id)).map(|j| j.id.clone()).collect()
    }
    /// The last [`SHOWN`] jobs as RoboCAD's lines.
    pub(super) fn lines(&self) -> Vec<String> {
        self.jobs[self.jobs.len().saturating_sub(SHOWN)..].iter().map(line).collect()
    }
    fn job_mut(&mut self, id: &str) -> Option<&mut PrintJob> {
        self.jobs.iter_mut().find(|j| j.id == id)
    }
}

/// Python's `f"{fraction * 100:.0f}"`.
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
            Err(e) => (format!("split: done (its answer could not be read: {e})"), None),
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
                None => ("strength: done; no part has a safety factor".to_string(), None),
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

/// Whether a job kind publishes an undo step when done: split, analyze and
/// plan always, assembly with its exploded view.
pub(super) fn publishes(j: &PrintJob) -> bool {
    matches!(j.kind.as_str(), "split" | "analyze" | "plan") || (j.kind == "assembly" && j.result.get("exploded").is_some())
}

/// A new generation: every job of the old one is dropped (cancelled).
fn reset(t: &mut PrintJobTracker, generation: u64) {
    let (open, runs) = (t.open, t.runs.clone());
    let old = std::mem::take(t);
    crate::jobs::drop_off_thread(old.running.into_iter().map(|r| r.work).collect::<Vec<_>>(), "superseded print jobs");
    *t = PrintJobTracker { generation, open, runs, ..PrintJobTracker::default() };
}

/// Start print job `kind` with `body` (the REST body) on the shown
/// snapshot, read at `revision`: the job as it is listed.
pub(super) fn start(doc: &mut CadDocument, kind: &'static str, body: Value, revision: u64, message: String) -> Result<PrintJob, String> {
    if !sim_cad::print::jobs::KINDS.contains(&kind) {
        return Err(format!("{kind} is not a print job ({})", sim_cad::print::jobs::KINDS.join(", ")));
    }
    let snapshot = doc.local.clone().ok_or("no CAD document is open")?;
    if revision != doc.shown_revision() {
        return Err(format!("the document moved to revision {} since this was read at revision {revision}; read it again", doc.shown_revision()));
    }
    if doc.print.jobs.generation != doc.generation {
        reset(&mut doc.print.jobs, doc.generation);
    }
    let t = &mut doc.print.jobs;
    t.started += 1;
    let id = format!("p{}", t.started);
    let runs = t.runs.clone().unwrap_or_else(sim_cad::print::runs_dir);
    let identity = snapshot.archive.identity().to_string();
    let label = format!("print {kind} {id}");
    let work = Job::spawn(Pool::Dedicated, doc.generation, label, move |ctx| {
        let cancelled = || ctx.cancelled();
        let say = |f: f64, m: &str| {
            if f >= 0.0 {
                ctx.fraction(f.clamp(0.0, 1.0));
            }
            ctx.message(m);
            !ctx.cancelled()
        };
        let env = sim_cad::print::jobs::Env { doc: &snapshot.archive, revision, runs, runner: &InProcess, progress: &say, cancelled: &cancelled };
        let out = sim_cad::print::jobs::run(kind, &body, &env)?;
        if ctx.cancelled() {
            return Err("cancelled before publishing".into());
        }
        Ok(Finished { result: out.result, out_dir: out.out_dir, edit: out.edit })
    });
    let job = PrintJob { id: id.clone(), kind: kind.to_string(), state: "running".into(), message: message.clone(), ..PrintJob::default() };
    t.jobs.push(job.clone());
    t.running.push(Running { id, work, identity, since: Instant::now() });
    t.shown = None;
    doc.show(Ok(message));
    Ok(job)
}

/// `cad_print {op: cancel}`: `confirm` None asks (the section shows the
/// question), false closes the question, true cancels each running job
/// (or only `job`).
pub(super) fn cancel(doc: &mut CadDocument, _call: &mut Call, job: Option<&str>, confirm: Option<bool>) -> Outcome {
    let done = Outcome::Done;
    let t = &mut doc.print.jobs;
    if confirm == Some(false) {
        if !t.confirming {
            return done(Ok(json!({"confirming": false, "message": "There was no cancel confirmation open; nothing changed."})));
        }
        t.confirming = false;
        doc.touch();
        return done(Ok(json!({"confirming": false, "message": "The running print jobs were left running."})));
    }
    let ids = t.running(job);
    if ids.is_empty() {
        return done(Err(match job {
            Some(id) => format!("print job {id} is not running"),
            None => "No print jobs are running".to_string(),
        }));
    }
    if confirm.is_none() {
        t.confirming = true;
        t.open = true;
        doc.touch();
        return done(Ok(json!({"confirming": true, "running": ids, "message": "Cancel the running jobs?"})));
    }
    for r in t.running.iter().filter(|r| ids.contains(&r.id)) {
        r.work.cancel();
    }
    // A finished job waiting to publish never publishes once cancelled.
    let dropped: Vec<String> = t.publish.iter().filter(|p| ids.contains(&p.id)).map(|p| p.id.clone()).collect();
    t.publish.retain(|p| !ids.contains(&p.id));
    for id in &dropped {
        if let Some(j) = t.job_mut(id) {
            j.state = "cancelled".into();
        }
    }
    t.confirming = false;
    let n = ids.len();
    doc.show(Ok(format!("Cancelling {n} print job(s)…")));
    done(Ok(json!({"cancelling": ids})))
}

/// A job ended: its state, the status line, the guide or folder opened.
fn ended(doc: &mut CadDocument, id: &str) {
    doc.print.jobs.shown = None;
    let Some(j) = doc.print.jobs.jobs.iter().find(|j| j.id == id).cloned() else { return };
    match j.state.as_str() {
        "done" => {
            let (text, open) = done_text(&j);
            doc.show(Ok(text));
            if let Some(path) = open {
                let job = Job::spawn(Pool::Io, doc.generation, "open a print job's output", move |_| crate::jobs::open_local(Path::new(&path)).map_err(|e| format!("could not open {path}: {e}")));
                doc.print.jobs.opens.push(job);
            }
            if publishes(&j) {
                // The stress paint is keyed by each node's results: the robot reads again.
                doc.robot.data.invalidate();
            }
        }
        "failed" => doc.show(Err(format!("{}: {}", capitalize(&j.kind), j.error.as_deref().filter(|e| !e.is_empty()).unwrap_or("failed")))),
        _ => doc.show(Ok(format!("{} cancelled", j.kind))),
    }
}

/// Publish the edits waiting for the edit slot (one per frame at most).
fn publish(doc: &mut CadDocument) {
    if doc.print.jobs.publish.is_empty() || doc.edit.is_some() {
        return;
    }
    let p = doc.print.jobs.publish.remove(0);
    let current = doc.local.as_ref().map(|l| l.archive.identity().to_string());
    let outcome = if current.as_deref() != Some(p.identity.as_str()) {
        Err("the document changed while the job ran; its result was not published: run it again".to_string())
    } else {
        let message = format!("{} published", p.label);
        let edit = p.edit;
        let result = p.result.clone();
        super::super::local::start(doc, p.label.clone(), false, false, move |ws| {
            ws.edit = edit;
            Ok(EditDone { message, result })
        })
        .map(|_| ())
    };
    let t = &mut doc.print.jobs;
    if let Some(j) = t.job_mut(&p.id) {
        match outcome {
            Ok(()) => {
                j.state = "done".into();
                j.fraction = 1.0;
                j.result = p.result;
            }
            Err(e) => {
                j.state = "failed".into();
                j.error = Some(e);
            }
        }
    }
    ended(doc, &p.id);
}

/// One frame: a new generation starts over; progress is read, ended jobs
/// land, waiting edits publish.
pub(super) fn tick(doc: &mut CadDocument) {
    let generation = doc.generation;
    let mut statuses: Vec<Result<String, String>> = Vec::new();
    let t = &mut doc.print.jobs;
    if t.generation != generation {
        reset(t, generation);
    }
    t.opens.retain(|j| match j.poll() {
        None => true,
        Some(Ok(())) => false,
        Some(Err(e)) => {
            statuses.push(Err(e));
            false
        }
    });
    let mut touched = false;
    let mut line_now = None;
    let mut finished: Vec<String> = Vec::new();
    let mut i = 0;
    while i < t.running.len() {
        let r = &t.running[i];
        let p = r.work.progress();
        let landed = r.work.poll();
        let id = r.id.clone();
        let identity = r.identity.clone();
        let Some(j) = t.jobs.iter_mut().find(|j| j.id == id) else {
            t.running.remove(i);
            continue;
        };
        if let Some(f) = p.fraction
            && (f - j.fraction).abs() > 1e-9
        {
            j.fraction = f;
            touched = true;
        }
        if !p.message.is_empty() && p.message != j.message {
            j.message = p.message.clone();
            touched = true;
        }
        let Some(result) = landed else {
            line_now = Some(progress(j));
            i += 1;
            continue;
        };
        let cancelled = matches!(&result, Err(e) if e.contains("cancelled"));
        j.seconds = t.running[i].since.elapsed().as_secs_f64();
        match result {
            Ok(f) => {
                j.out_dir = f.out_dir.map(|p| p.display().to_string());
                match f.edit {
                    Some((label, edit)) => {
                        j.message = "publishing".into();
                        t.publish.push(Publish { id: id.clone(), label, edit, identity, result: f.result });
                    }
                    None => {
                        j.state = "done".into();
                        j.fraction = 1.0;
                        j.result = f.result;
                        finished.push(id.clone());
                    }
                }
            }
            Err(e) if cancelled => {
                j.state = "cancelled".into();
                j.error = Some(e);
                finished.push(id.clone());
            }
            Err(e) => {
                j.state = "failed".into();
                j.error = Some(e);
                finished.push(id.clone());
            }
        }
        t.running.remove(i);
        touched = true;
    }
    if t.jobs.len() > KEPT {
        let keep_from = t.jobs.len() - KEPT;
        let (old, _) = t.jobs.split_at(keep_from);
        if old.iter().all(|j| !j.running()) {
            t.jobs.drain(..keep_from);
        }
    }
    if let Some(text) = line_now
        && t.shown.as_ref() != Some(&text)
    {
        t.shown = Some(text.clone());
        statuses.push(Ok(text));
    }
    for status in statuses {
        doc.show(status);
    }
    for id in finished {
        ended(doc, &id);
    }
    publish(doc);
    if touched {
        doc.touch();
    }
}

/// `cad_state.print.jobs`.
pub(super) fn state_json(doc: &CadDocument) -> Value {
    let t = &doc.print.jobs;
    let job = |j: &PrintJob| json!({"id": j.id, "kind": j.kind, "state": j.state, "fraction": j.fraction, "message": j.message, "error": j.error, "out_dir": j.out_dir, "result": j.result, "line": line(j)});
    json!({
        "jobs": t.jobs[t.jobs.len().saturating_sub(SHOWN)..].iter().map(job).collect::<Vec<_>>(),
        "running": t.running(None),
        "publishing": t.publish.iter().map(|p| p.id.clone()).collect::<Vec<_>>(),
        "confirming": t.confirming,
        "blockers": t.blockers(),
    })
}

/// `cad_print {op: job, job}`: one job as listed (its result once done).
pub(super) fn job_json(doc: &CadDocument, id: &str) -> Result<Value, String> {
    let j = doc.print.jobs.jobs.iter().find(|j| j.id == id).ok_or_else(|| format!("no print job {id}"))?;
    serde_json::to_value(j).map_err(|e| e.to_string())
}

/// JobResults: [`tick`].
fn poll(doc: Option<ResMut<CadDocument>>) {
    let Some(mut doc) = doc else { return };
    tick(&mut doc);
}

/// CadCorePlugin: the tracker (JobResults, after `sync::receive`).
pub(super) fn build_core(app: &mut App) {
    app.add_systems(Update, poll.after(crate::cad::CadSet::Results).in_set(ViewerSet::JobResults).run_if(in_state(ViewerMode::Cad)));
}
