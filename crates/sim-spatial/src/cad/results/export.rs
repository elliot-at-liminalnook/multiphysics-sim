//! Physical export (RoboCAD's `sim_export_physical`, `sim_export` and
//! `_export_in_background`): `GET /physical?flex=…&planar=…` on a
//! `Pool::Dedicated` job, the answer written atomically (a temporary file
//! beside the target, then a rename) by the same job, so a cancel or a
//! failure never leaves a partial model and the `.rcad` is never written.
//!
//! - **One at a time.** A second request while one runs is refused with
//!   RoboCAD's text, "a model export is already running"; the live link's
//!   request queues instead (RoboCAD's `queue=True`: the latest wins) and
//!   starts when the running one ends.
//! - **Progress**: "exporting {label} in the background… n s" in the status
//!   line, refreshed once a second.
//! - **Cancel** requests cancellation and retains the job until completion. RoboCAD's request itself
//!   cannot be aborted (it derives the model to the end; api.py has no
//!   cancel route). Cancellation after the final check cannot revoke a rename. The job checks the
//!   cancel again just before the rename and then deletes its temporary
//!   file (each job's own name: pid and a process-wide counter). Leaving
//!   CAD mode, or opening another document, is refused while an export runs
//!   or is queued (`CadDocument::switch_blockers`), since it would drop it.
use super::ResultsState;
use crate::cad::document::CadDocument;
use crate::jobs::{Job, Pool};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Makes each temporary file name unique within the process.
static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// How long the export request may take: RoboCAD's `export_snapshot`
/// allows its child process an hour; a little more here.
pub(crate) const EXPORT_TIMEOUT: Duration = Duration::from_secs(3700);
/// RoboCAD's refusal of a second export.
pub(crate) const RUNNING: &str = "a model export is already running";

/// One export to make.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ExportRequest {
    pub path: PathBuf,
    pub flex: bool,
    pub planar: bool,
    /// RoboCAD's label: "physical model", "simulation model", "live simulation model".
    pub label: String,
    /// The live link's (queued while another runs, and Robot mode follows it).
    pub link: bool,
}

/// What a finished export wrote: the model's link count and flexible links.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Written {
    pub links: usize,
    pub flexible: usize,
}

/// The export in flight.
pub(crate) struct Running {
    pub request: ExportRequest,
    pub job: Job<Written>,
    pub started: Instant,
    /// Whole seconds last shown in the status line.
    pub shown: u64,
    pub cancel_requested: bool,
    /// The shown revision when it started: RoboCAD exports its document as
    /// it is when the request arrives, so an edit made while the export
    /// runs is not in the file ([`poll`] says so when it lands).
    pub revision: u64,
}

/// The exports: the running one, the queued live-link one, the last outcome.
#[derive(Default)]
pub(crate) struct Exports {
    pub running: Option<Running>,
    pub queued: Option<ExportRequest>,
    /// The last outcome: (label, path, Ok(message) | Err(message)).
    pub last: Option<(String, PathBuf, Result<String, String>)>,
    /// The last model written (what "Show in Robot mode" opens without a link).
    pub written: Option<PathBuf>,
}

impl Exports {
    pub(crate) fn json(&self) -> Value {
        let req = |r: &ExportRequest| json!({"label": r.label, "path": r.path, "flex": r.flex, "planar": r.planar, "link": r.link});
        json!({
            "running": self.running.as_ref().map(|r| {
                let mut v = req(&r.request);
                v["seconds"] = json!(r.started.elapsed().as_secs());
                v["cancel_requested"] = json!(r.cancel_requested);
                v
            }),
            "queued": self.queued.as_ref().map(req),
            "last": self.last.as_ref().map(|(label, path, r)| match r {
                Ok(m) => json!({"label": label, "path": path, "ok": true, "message": m}),
                Err(e) => json!({"label": label, "path": path, "ok": false, "message": e}),
            }),
            "written": self.written,
        })
    }
}

/// The file an export writes for a typed path: as typed when it ends in
/// `.json`, else with `.simrobot.json` appended (RoboCAD's "Sim model
/// (*.simrobot.json)" filter), so the `.rcad` is never the target.
pub(crate) fn model_file(path: &str) -> PathBuf {
    if path.ends_with(".json") { PathBuf::from(path) } else { PathBuf::from(format!("{path}.simrobot.json")) }
}

/// Whether `request` starts now (`Ok(Some(request))`), was queued
/// (`Ok(None)`: the live link's while another runs, the latest wins) or is
/// refused with RoboCAD's text.
pub(crate) fn admit(exports: &mut Exports, request: ExportRequest) -> Result<Option<ExportRequest>, String> {
    if exports.running.is_none() {
        return Ok(Some(request));
    }
    if request.link {
        exports.queued = Some(request);
        return Ok(None);
    }
    Err(RUNNING.into())
}

/// An export asked for (a button, the form, the live link).
pub(crate) fn request(doc: &mut CadDocument, request: ExportRequest) -> Result<Value, String> {
    let label = request.label.clone();
    match admit(&mut doc.results.exports, request)? {
        None => {
            let message = format!("{label}: queued; it starts when the running export ends");
            doc.show(Ok(message.clone()));
            Ok(json!({"queued": true, "message": message}))
        }
        Some(request) => {
            let path = request.path.clone();
            start(doc, request)?;
            Ok(json!({"started": true, "path": path, "message": format!("exporting {label} in the background…")}))
        }
    }
}

/// Starts `request` now (nothing may be running).
fn start(doc: &mut CadDocument, request: ExportRequest) -> Result<(), String> {
    if !doc.connected() {
        return Err(format!("not connected to RoboCAD: {}", doc.connection_line().0));
    }
    let client = doc.client.clone().ok_or("not connected to RoboCAD")?.with_timeout(EXPORT_TIMEOUT);
    let (path, flex, planar) = (request.path.clone(), request.flex, request.planar);
    let job = Job::spawn(Pool::Dedicated, doc.generation, format!("RoboCAD export: {}", request.label), move |ctx| {
        let model = client.physical_model(flex, planar).map_err(|e| e.to_string())?;
        // A cancel before the write leaves no file (RoboCAD's request had run to the end).
        if ctx.cancelled() {
            return Err("cancelled".into());
        }
        write_model(&path, &model, &|| ctx.cancelled())
    });
    doc.show(Ok(format!("exporting {} in the background…", request.label)));
    let revision = doc.shown_revision();
    doc.results.exports.running = Some(Running { request, job, started: Instant::now(), shown: 0, cancel_requested: false, revision });
    Ok(())
}

/// Python's truthiness of a JSON value (`export_worker`'s `l.get("flex")`).
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Writes `model` to `path` through a temporary file beside it and a
/// rename (`export_worker`'s `os.replace`); the summary RoboCAD prints.
/// `cancelled` is checked just before the rename: a cancel then deletes
/// the temporary file and leaves `path` as it was.
pub(crate) fn write_model(path: &Path, model: &Value, cancelled: &dyn Fn() -> bool) -> Result<Written, String> {
    let links = model["links"].as_array().ok_or_else(|| format!("RoboCAD's answer is not a physical model (no links list); nothing was written to {}", path.display()))?;
    let written = Written { links: links.len(), flexible: links.iter().filter(|l| l.get("flex").is_some_and(truthy)).count() };
    let bytes = serde_json::to_vec(model).map_err(|e| format!("{}: {e}", path.display()))?;
    // Unique per job: two exports to one path (one cancelled, one started) never share it.
    let n = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = PathBuf::from(format!("{}.{}.{n}.tmp", path.display(), std::process::id()));
    if let Err(e) = std::fs::write(&tmp, &bytes) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("{}: {e}", tmp.display()));
    }
    if cancelled() {
        let _ = std::fs::remove_file(&tmp);
        return Err("cancelled".into());
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("{}: {e}", path.display()));
    }
    Ok(written)
}

/// `op: export_cancel`.
pub(crate) fn cancel(doc: &mut CadDocument) -> Result<Value, String> {
    let Some(running) = doc.results.exports.running.as_mut() else { return Err("no export is running".into()) };
    running.cancel_requested = true;
    running.job.cancel();
    let label = running.request.label.clone();
    let message = format!("{label} cancellation requested; waiting for the export outcome");
    doc.show(Ok(message.clone()));
    Ok(json!({"cancelling": label, "message": message, "note": "RoboCAD's request runs to the end. Cancellation before the final rename leaves the destination unchanged; a completed rename cannot be revoked. Queued exports wait for this job's terminal outcome."}))
}

/// Starts the queued export, if any; its label, or the reason it could not start.
fn start_queued(doc: &mut CadDocument) -> Option<Value> {
    let next = doc.results.exports.queued.take()?;
    let label = next.label.clone();
    Some(match start(doc, next) {
        Ok(()) => json!(label),
        Err(e) => {
            doc.show(Err(format!("{label} export not started: {e}")));
            json!({"label": label, "error": e})
        }
    })
}

/// What a finished export means for the live link: (written model, link export).
pub(crate) struct Landed {
    pub path: PathBuf,
    pub link: bool,
}

/// JobResults: the running export's progress line (once a second) and its
/// outcome; the queued one then starts. Returns a model written now.
pub(crate) fn poll(doc: &mut CadDocument) -> Option<Landed> {
    let results: &mut ResultsState = &mut doc.results;
    let running = results.exports.running.as_mut()?;
    let Some(outcome) = running.job.poll() else {
        let seconds = running.started.elapsed().as_secs();
        if seconds != running.shown {
            running.shown = seconds;
            // RoboCAD's progress text; after a cancel request it keeps running until its outcome, and says so.
            let cancelling = if running.cancel_requested { " (cancellation requested; waiting for the outcome)" } else { "" };
            let line = format!("exporting {} in the background… {seconds} s{cancelling}", running.request.label);
            doc.show(Ok(line));
        }
        return None;
    };
    let Running { request, cancel_requested, revision, job, .. } = results.exports.running.take()?;
    let (label, path) = (request.label.clone(), request.path.clone());
    // The document this export was made for: a job of an older connection
    // or document (generation) never drives the live link or Robot mode.
    let same_document = job.generation() == doc.generation;
    let now = doc.shown_revision();
    let landed = match outcome {
        Ok(w) => {
            let mut message = format!("{label} written: {} ({} links, {} flexible)", path.display(), w.links, w.flexible);
            if cancel_requested { message.push_str("; cancellation arrived after the final publication check and could not revoke the write"); }
            if !same_document {
                message.push_str("; it was exported from the document as it was before CAD mode reconnected or opened another one, so Robot mode does not follow it");
            } else if revision != now {
                message.push_str(&format!("; it holds the document at revision {revision}, when the export started: the document changed while it ran (now revision {now})"));
            }
            doc.show(Ok(message.clone()));
            doc.results.exports.last = Some((label, path.clone(), Ok(message)));
            doc.results.exports.written = Some(path.clone());
            same_document.then_some(Landed { path, link: request.link })
        }
        // A cancel that arrived before publication: nothing was written (RoboCAD's "{label} export cancelled").
        Err(e) if cancel_requested && e == "cancelled" => {
            let message = format!("{label} export cancelled: nothing was written to {}", path.display());
            doc.show(Ok(message.clone()));
            doc.results.exports.last = Some((label, path, Err(message)));
            None
        }
        Err(e) => {
            let message = format!("{label} export failed: {e}");
            doc.show(Err(message.clone()));
            doc.results.exports.last = Some((label, path, Err(message)));
            None
        }
    };
    start_queued(doc);
    landed
}
