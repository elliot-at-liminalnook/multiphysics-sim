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
//! - **The saved file's hash** (`source.cad_sha256`, which RoboCAD's `GET
//!   /physical` does not write): the job reads RoboCAD's `GET /` before
//!   the model and again after hashing the file, and records the sha256 of
//!   the `.rcad` only when that file is RoboCAD's document as saved through
//!   the whole export (`cad_client::stamp_saved_source`: a path, absolute,
//!   no unsaved edits, the same file, document and revision both times);
//!   otherwise the model is written without it and the reason is kept
//!   ([`Written`], `cad_state.results.exports.last`, the status line). The
//!   live link's exports follow the same rule. `source.file` stays as
//!   RoboCAD wrote it and must name that same file (its document's path,
//!   absolute when stamped), which `sim_domain_robot::cad_link::status`
//!   uses as is. **Remaining limit**: RoboCAD's `dirty` and `revision`
//!   track only its in-memory document, so a `.rcad` overwritten on disk
//!   by another program after RoboCAD loaded or saved it would be hashed
//!   and stamped although the model came from the document in memory;
//!   closing that needs RoboCAD to report the digest of the file it last
//!   loaded or saved.
//! - **Completion**: each started or queued export has a process-wide
//!   sequence number (`seq` in `cad_results {op: export}`'s answer
//!   `{started, path, seq, message}` or `{queued, seq, message}`, in
//!   `queued` and `running`; a queued one keeps it when it starts); its
//!   outcome (also "not started" for a queued one replaced, dropped or
//!   failing to start) lands in `recent` (the last [`RECENT`]
//!   outcomes, oldest first, each as `last`) and in `last` (the newest),
//!   so a REST caller polls `cad_state.results.exports.recent` for its
//!   seq: a queued live-link export or another caller's landing after it
//!   replaces `last`, not the caller's entry in `recent`.
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
use sim_domain_robot::cad_link::sha256_file;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Makes each temporary file name unique within the process.
static TMP_SEQ: AtomicU64 = AtomicU64::new(0);
/// Export sequence numbers, never reused in the process (also across
/// documents, so a REST caller's seq can only ever name its own export).
static EXPORT_SEQ: AtomicU64 = AtomicU64::new(0);
/// How many finished exports' outcomes `Exports::recent` keeps.
pub(crate) const RECENT: usize = 8;

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

/// What a finished export wrote: the model's link count and flexible
/// links, and the `.rcad` hash recorded as `source.cad_sha256` or why none was.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Written {
    pub links: usize,
    pub flexible: usize,
    pub cad_sha256: Option<String>,
    pub cad_sha256_reason: Option<String>,
}

/// The last finished export (`cad_state.results.exports.last`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Last {
    /// The export's sequence number (its `cad_results` answer's `seq`).
    pub seq: u64,
    pub label: String,
    pub path: PathBuf,
    /// Ok(message): written; Err(message): failed or cancelled, nothing written.
    pub outcome: Result<String, String>,
    /// The hash recorded as `source.cad_sha256` (written exports only).
    pub cad_sha256: Option<String>,
    /// Why a written export records no `cad_sha256`.
    pub cad_sha256_reason: Option<String>,
}

/// The export in flight.
pub(crate) struct Running {
    /// This export's process-wide sequence number (`EXPORT_SEQ`).
    pub seq: u64,
    pub request: ExportRequest,
    pub job: Job<Written>,
    pub started: Instant,
    /// Whole seconds last shown in the status line.
    pub shown: u64,
    pub cancel_requested: bool,
    /// The revision the viewer showed when the export was asked for. RoboCAD
    /// builds the model from its document when the request arrives, which is
    /// not known here, so this only tells [`poll`] that the document changed
    /// while the export ran (later edits may then not be in the file).
    pub revision: u64,
}

/// The exports: the running one, the queued live-link one, the last outcome.
#[derive(Default)]
pub(crate) struct Exports {
    pub running: Option<Running>,
    pub queued: Option<ExportRequest>,
    /// The queued export's sequence number (given when it was queued and
    /// kept when it starts; meaningful only while `queued` is Some).
    pub queued_seq: u64,
    /// The last outcome.
    pub last: Option<Last>,
    /// The last [`RECENT`] outcomes, oldest first (`last` is the newest).
    pub recent: std::collections::VecDeque<Last>,
    /// The last model written (what "Show in Robot mode" opens without a link).
    pub written: Option<PathBuf>,
}

impl Exports {
    pub(crate) fn json(&self) -> Value {
        let req = |r: &ExportRequest| json!({"label": r.label, "path": r.path, "flex": r.flex, "planar": r.planar, "link": r.link});
        json!({
            "running": self.running.as_ref().map(|r| {
                let mut v = req(&r.request);
                v["seq"] = json!(r.seq);
                v["seconds"] = json!(r.started.elapsed().as_secs());
                v["cancel_requested"] = json!(r.cancel_requested);
                v
            }),
            "queued": self.queued.as_ref().map(|q| {
                let mut v = req(q);
                v["seq"] = json!(self.queued_seq);
                v
            }),
            "last": self.last.as_ref().map(last_json),
            "recent": self.recent.iter().map(last_json).collect::<Vec<_>>(),
            "written": self.written,
        })
    }
    /// A finished export's outcome: `last`, and kept in `recent` (bounded).
    pub(crate) fn land(&mut self, outcome: Last) {
        self.recent.push_back(outcome.clone());
        while self.recent.len() > RECENT {
            self.recent.pop_front();
        }
        self.last = Some(outcome);
    }
}

/// One outcome as `cad_state.results.exports.last` and `recent` show it.
fn last_json(l: &Last) -> Value {
    let (ok, message) = match &l.outcome {
        Ok(m) => (true, m),
        Err(e) => (false, e),
    };
    json!({"seq": l.seq, "label": l.label, "path": l.path, "ok": ok, "message": message, "cad_sha256": l.cad_sha256, "cad_sha256_reason": l.cad_sha256_reason})
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

/// The next export sequence number (process-wide, never reused).
fn next_seq() -> u64 {
    EXPORT_SEQ.fetch_add(1, Ordering::Relaxed) + 1
}

/// The queued export, dropped without starting: landed as not started
/// (`ok: false`, `reason`) so a REST caller polling its seq learns it.
pub(crate) fn drop_queued(exports: &mut Exports, reason: &str) {
    if let Some(q) = exports.queued.take() {
        let message = format!("{} export not started: {reason}; nothing was written to {}", q.label, q.path.display());
        exports.land(Last { seq: exports.queued_seq, label: q.label, path: q.path, outcome: Err(message), cad_sha256: None, cad_sha256_reason: None });
    }
}

/// An export asked for (a button, the form, the live link). The answer
/// is `{started, path, seq, message}` or, for a live-link export queued
/// behind a running one, `{queued, seq, message}`; either seq's outcome
/// lands in `Exports::recent` (a queued one replaced by a newer one, or
/// whose start fails, lands as not started).
pub(crate) fn request(doc: &mut CadDocument, request: ExportRequest) -> Result<Value, String> {
    let label = request.label.clone();
    // A live-link request queued behind a running export replaces the
    // queued one (the latest wins): taken here, landed as not started below.
    let replaced = if request.link && doc.results.exports.running.is_some() { doc.results.exports.queued.take().map(|q| (doc.results.exports.queued_seq, q)) } else { None };
    match admit(&mut doc.results.exports, request) {
        Err(e) => Err(e),
        Ok(None) => {
            let seq = next_seq();
            if let Some((old, q)) = replaced {
                let message = format!("{} export not started: replaced by a newer live-link export (seq {seq}); nothing was written to {}", q.label, q.path.display());
                doc.results.exports.land(Last { seq: old, label: q.label, path: q.path, outcome: Err(message), cad_sha256: None, cad_sha256_reason: None });
            }
            doc.results.exports.queued_seq = seq;
            let message = format!("{label}: queued; it starts when the running export ends; poll cad_state.results.exports.recent for seq {seq} (last is the newest)");
            doc.show(Ok(message.clone()));
            Ok(json!({"queued": true, "seq": seq, "message": message}))
        }
        Ok(Some(request)) => {
            let path = request.path.clone();
            let seq = next_seq();
            start(doc, request, seq)?;
            Ok(json!({"started": true, "path": path, "seq": seq, "message": format!("exporting {label} in the background…; poll cad_state.results.exports.recent for seq {seq} (last is the newest)")}))
        }
    }
}

/// Starts `request` now as export `seq` (nothing may be running): the
/// in-process physical export (`sim_cad::physical::export`) of the open
/// document's current snapshot (archive, exact geometry, exact masses) on a
/// dedicated job, written atomically. `source.cad_sha256` is recorded only
/// when the snapshot is the saved file ([`saved_hash`]); otherwise the
/// reason is kept, and Robot mode's `cad_link` reports no recorded hash.
/// Flexible links are not ported: every export is rigid and says so in
/// `source.not_modelled` (a `physical` or `simulation` request too).
fn start(doc: &mut CadDocument, request: ExportRequest, seq: u64) -> Result<(), String> {
    let local = doc.local.clone().ok_or_else(|| format!("no CAD document is open: {}", doc.connection_line().0))?;
    let saved = doc.history.saved.clone();
    let file = match &doc.target {
        crate::cad::CadTarget::File(p) => Some(p.clone()),
        crate::cad::CadTarget::Service(_) => None,
    };
    let (path, planar) = (request.path.clone(), request.planar);
    let job = Job::spawn(Pool::Dedicated, doc.generation, format!("Physical export: {}", request.label), move |ctx| {
        ctx.message("Deriving the physical model");
        let registry = registry_json();
        let opts = sim_cad::physical::Options {
            planar: planar.then_some(([0.0, -1.0, 0.0], [0.0, 0.0, 0.0])),
            registry: registry.as_ref().map(|(json, sha)| sim_cad::materials::Registry { json, sha256: sha }),
            exported_at: crate::robot::recording::stamp(crate::robot::recording::now_ms()),
        };
        let mut model = sim_cad::physical::export(&local.archive, &local.geometry, &local.masses, &opts)?;
        if ctx.cancelled() {
            return Err("cancelled".into());
        }
        let stamp = saved_hash(&local.archive, saved.as_deref(), file.as_deref());
        if let Ok(hash) = &stamp {
            model["source"]["cad_sha256"] = json!(hash);
        }
        let mut written = write_model(&path, &model, &|| ctx.cancelled())?;
        match stamp {
            Ok(hash) => written.cad_sha256 = Some(hash),
            Err(reason) => written.cad_sha256_reason = Some(reason),
        }
        Ok(written)
    });
    doc.show(Ok(format!("exporting {} in the background…", request.label)));
    let revision = doc.shown_revision();
    doc.results.exports.running = Some(Running { seq, request, job, started: Instant::now(), shown: 0, cancel_requested: false, revision });
    Ok(())
}

/// The saved `.rcad`'s SHA-256 when the exported snapshot is that file's
/// content (no unsaved edits, and the bytes on disk still match), or why not.
pub(crate) fn saved_hash(archive: &sim_cad::ArchiveDocument, saved: Option<&str>, file: Option<&Path>) -> Result<String, String> {
    let file = file.ok_or("the document has no file yet: save it so the export can name the file it came from")?;
    if saved != Some(archive.identity()) {
        return Err(format!("{} has unsaved edits: save it so the export can name the file it came from", file.display()));
    }
    let hash = sha256_file(file).map_err(|e| format!("{}: could not be hashed: {e}", file.display()))?;
    if archive.identity().trim_start_matches("sha256:") != hash.trim_start_matches("sha256:") {
        return Err(format!("{} changed on disk since it was opened or saved here", file.display()));
    }
    Ok(hash)
}

/// The print registry (`library/printing/registry.json` under the workspace
/// root) and its SHA-256, for filament engineering values; None without one.
fn registry_json() -> Option<(Value, String)> {
    let path = crate::workspace::root().ok()?.join("library/printing/registry.json");
    let bytes = std::fs::read(&path).ok()?;
    let json = serde_json::from_slice(&bytes).ok()?;
    Some((json, sha256_file(&path).ok()?))
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
    let written = Written { links: links.len(), flexible: links.iter().filter(|l| l.get("flex").is_some_and(truthy)).count(), cad_sha256: None, cad_sha256_reason: None };
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

/// Starts the queued export, if any, under the seq it was queued with; its
/// label, or the reason it could not start (then landed as not started in
/// `Exports::recent`, `ok: false`, so its REST caller learns it).
fn start_queued(doc: &mut CadDocument) -> Option<Value> {
    let next = doc.results.exports.queued.take()?;
    let seq = doc.results.exports.queued_seq;
    let (label, path) = (next.label.clone(), next.path.clone());
    Some(match start(doc, next, seq) {
        Ok(()) => json!(label),
        Err(e) => {
            let message = format!("{label} export not started: {e}; nothing was written to {}", path.display());
            doc.show(Err(message.clone()));
            doc.results.exports.land(Last { seq, label: label.clone(), path, outcome: Err(message), cad_sha256: None, cad_sha256_reason: None });
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
    let Running { seq, request, cancel_requested, revision, job, .. } = results.exports.running.take()?;
    let (label, path) = (request.label.clone(), request.path.clone());
    // The document this export was made for: a job of an older connection
    // or document (generation) never drives the live link or Robot mode.
    let same_document = job.generation() == doc.generation;
    let now = doc.shown_revision();
    let landed = match outcome {
        Ok(w) => {
            let mut message = format!("{label} written: {} ({} links, {} flexible)", path.display(), w.links, w.flexible);
            match (&w.cad_sha256, &w.cad_sha256_reason) {
                (Some(hash), _) => message.push_str(&format!("; source.cad_sha256 {hash} of the saved .rcad")),
                (None, Some(reason)) => message.push_str(&format!("; no source.cad_sha256: {reason}")),
                (None, None) => {}
            }
            if cancel_requested { message.push_str("; cancellation arrived after the final publication check and could not revoke the write"); }
            if !same_document {
                message.push_str("; it was exported from the document as it was before CAD mode reconnected or opened another one, so Robot mode does not follow it");
            } else if revision != now {
                message.push_str(&format!("; the export started at revision {revision} and the document changed while it ran (now revision {now}), so later edits may not be in it"));
            }
            doc.show(Ok(message.clone()));
            doc.results.exports.land(Last { seq, label, path: path.clone(), outcome: Ok(message), cad_sha256: w.cad_sha256, cad_sha256_reason: w.cad_sha256_reason });
            // Only this document's model is what "Show in Robot mode" opens
            // (`link::shown_model`); an older one's leaves the previous model.
            if same_document {
                doc.results.exports.written = Some(path.clone());
                Some(Landed { path, link: request.link })
            } else {
                None
            }
        }
        // A cancel was requested and the job failed: nothing was published.
        // Every `Err` from the job comes before or instead of the rename
        // (`write_model`, the cancel check in `start`, `physical_model`), and
        // `crate::jobs` reports a job cancelled before it ran as
        // "{name} was cancelled before it started." (RoboCAD: "{label} export cancelled").
        Err(e) if cancel_requested => {
            let mut message = format!("{label} export cancelled: nothing was written to {}", path.display());
            if e != "cancelled" { message.push_str(&format!(" ({e})")); }
            doc.show(Ok(message.clone()));
            doc.results.exports.land(Last { seq, label, path, outcome: Err(message), cad_sha256: None, cad_sha256_reason: None });
            None
        }
        Err(e) => {
            let message = format!("{label} export failed: {e}");
            doc.show(Err(message.clone()));
            doc.results.exports.land(Last { seq, label, path, outcome: Err(message), cad_sha256: None, cad_sha256_reason: None });
            None
        }
    };
    start_queued(doc);
    landed
}
