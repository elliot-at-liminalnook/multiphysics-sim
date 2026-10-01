//! The file jobs (export, render, new, the unit guess) and the form's
//! directory listing: started from the handler, polled in JobResults,
//! shown in the status line, `cad_state.files` and the progress strip; a
//! REST caller waits on its job's sequence (`file_job` in its continuation);
//! a waited new's open runs in that wait, so its answer is the open's.
//!
//! Writes (export, render, new) are `complete_on_drop`: leaving CAD mode
//! drops `CadFiles` (`cad::clear`) but they run to the end and log their
//! outcome (`logged`), so a sent export is never silently lost. The unit
//! guess and the listing are reads, cancelled on drop.
use super::{CadFiles, FileArgs, FileOp};
use crate::app::ModeScope;
use crate::app::actions::{Act, Call};
use crate::cad::actions::{CadAction, Cx};
use crate::cad::document::CadDocument;
use crate::jobs::{Ctx, Job, Pool};
use crate::ui_kit::{BORDER, Kit, LEFT_WIDTH, STATUSBAR, SURFACE, TEXT, UiFonts, size};
use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use bevy::ui::prelude::AccessibleLabel;
use serde_json::{Value, json};
use sim_api::Outcome;
use sim_runtime::cad_client::{CadClient, CadError, FILE_TIMEOUT};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Job sequence numbers, never reused in the process (a REST waiter from
/// before CAD mode was re-entered can never match a newer job).
static SEQ: AtomicU64 = AtomicU64::new(0);

/// What follows a job's success.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Then {
    Nothing,
    /// New: open the created file (`cad_open`'s rule applied again as it
    /// stands then): by its REST caller's wait, else a UI `CadFile` open.
    Open { path: String },
    /// The unit guess: fill the import form's unit for this path.
    Guess { path: String },
}

/// One job in flight.
pub(crate) struct FileJob {
    pub seq: u64,
    /// export | render | new | guess_unit
    pub kind: &'static str,
    /// What it does, as the status line, the strip and refusals name it.
    pub label: String,
    pub job: Job<Value>,
    pub started: Instant,
    /// A REST caller waits for its answer.
    pub waited: bool,
    pub then: Then,
}
impl FileJob {
    pub(crate) fn json(&self) -> Value {
        let p = self.job.progress();
        json!({"seq": self.seq, "kind": self.kind, "label": self.label, "seconds": self.started.elapsed().as_secs(), "progress": p.message, "waited": self.waited})
    }
}

/// The document's client with the file timeout, or why there is none.
pub(super) fn client(doc: &CadDocument) -> Result<CadClient, String> {
    match &doc.client {
        Some(c) if doc.connected() => Ok(c.clone().with_timeout(FILE_TIMEOUT)),
        _ => Err(format!("not connected to RoboCAD: {}", doc.connection_line().0)),
    }
}

/// A failed request as a named refusal: "{label}: RoboCAD answered 422:
/// {RoboCAD's error}", or the transport's error.
pub(super) fn named(label: &str, e: &CadError) -> String {
    match e.status {
        Some(status) => format!("{label}: RoboCAD answered {status}: {}", e.message),
        None => format!("{label}: {e}"),
    }
}

/// The outcome, also logged (a write whose owner is gone still reports).
pub(super) fn logged(label: &str, result: Result<Value, String>) -> Result<Value, String> {
    match &result {
        Ok(v) => {
            // Outside the macro: tracing's expansion brings its own `Value`
            // trait into scope, which shadows serde_json's.
            let message = v.get("message").and_then(Value::as_str).unwrap_or(label);
            bevy::log::info!("{message}");
        }
        Err(e) => bevy::log::warn!("{e}"),
    }
    result
}

/// Starts `work` on a dedicated thread (`write`: complete on drop), shows
/// it in the status line and closes the path form (not for the unit guess,
/// which fills it). A REST caller waits (Pending with `file_job`).
#[allow(clippy::too_many_arguments)]
pub(super) fn start(cx: &mut Cx, call: &mut Call, kind: &'static str, label: String, write: bool, then: Then, work: impl FnOnce(&Ctx) -> Result<Value, String> + Send + 'static) -> Outcome {
    let Some(files) = cx.files.as_deref_mut() else { return Outcome::Done(Err("CAD mode's file workflows are not available in this window (no CAD panels)".into())) };
    let seq = SEQ.fetch_add(1, Ordering::Relaxed) + 1;
    let mut job = Job::spawn(Pool::Dedicated, seq, format!("cad {kind}"), work);
    if write {
        job = job.complete_on_drop();
    }
    let waited = call.rest();
    if !matches!(then, Then::Guess { .. }) {
        files.form = None;
    }
    files.jobs.push(FileJob { seq, kind, label: label.clone(), job, started: Instant::now(), waited, then });
    cx.doc.show(Ok(format!("{label}…")));
    if waited {
        *call.continuation = json!({"file_job": seq});
        Outcome::Pending
    } else {
        Outcome::Done(Ok(json!({"started": label, "job": seq})))
    }
}

/// Finished jobs' answers kept for their REST callers: a waiter polls
/// every frame, so only a caller that went away leaves one; the oldest go
/// first past this many.
pub(super) const KEPT_RESULTS: usize = 64;

/// Keeps a finished job's answer for its REST caller; past
/// [`KEPT_RESULTS`] the oldest (lowest sequence) goes, never by distance
/// from the newest, so a caller still polling finds its answer.
pub(super) fn keep_result(files: &mut CadFiles, seq: u64, entry: (Result<Value, String>, Then)) {
    files.results.insert(seq, entry);
    while files.results.len() > KEPT_RESULTS {
        files.results.pop_first();
    }
}

/// A REST caller's job: its outcome once it lands; a new's created file is
/// opened here, so the answer is the open's outcome.
pub(super) fn wait(cx: &mut Cx, call: &mut Call, seq: u64) -> Outcome {
    let Some(files) = cx.files.as_deref_mut() else {
        return Outcome::Done(Err("CAD mode closed while this request waited; a sent export, render or new file runs to its end and its outcome is logged".into()));
    };
    if let Some((result, then)) = files.results.remove(&seq) {
        return match (then, result) {
            // Opened even if the caller cancelled meanwhile: the file exists and nothing else opens it.
            (Then::Open { path }, Ok(_)) => open_created(cx, call, path),
            (_, result) => Outcome::Done(result),
        };
    }
    match files.jobs.iter_mut().find(|j| j.seq == seq) {
        Some(j) if call.cancelled => {
            j.waited = false;
            Outcome::Done(Err(format!("stopped waiting for {}: the request was already sent to RoboCAD, which runs it to the end (api.py has no cancel route); its outcome shows in cad_state.files.last", j.label)))
        }
        Some(_) => Outcome::Pending,
        None => Outcome::Done(Err("the job ended without an answer for this request; see cad_state.files.last".into())),
    }
}

/// New's REST caller: the created file's `cad_open` (synchronous: it
/// replaces the document and starts connecting, or is refused by name).
fn open_created(cx: &mut Cx, call: &mut Call, path: String) -> Outcome {
    let open = CadAction::CadOpen { path: Some(path.clone().into()), url: None };
    match crate::cad::actions::handle(&open, call, cx) {
        Outcome::Done(Ok(v)) => {
            let message = format!("Created {path}; {}", v.get("message").and_then(Value::as_str).unwrap_or("opening it"));
            Outcome::Done(Ok(json!({"created": path, "opened": v.get("opened"), "generation": v.get("generation"), "message": message})))
        }
        Outcome::Done(Err(e)) => Outcome::Done(Err(format!("Created {path}, but did not open it: {e}"))),
        other => other,
    }
}

/// The status line's text for a finished job.
fn message(job: &FileJob, value: &Value) -> String {
    if job.kind == "guess_unit" {
        let extent = value.get("extent").and_then(Value::as_f64).map_or_else(String::new, |e| format!(" (largest extent {e} in the file's units)"));
        return format!("RoboCAD guesses {} for {}{extent}", value.get("guess").and_then(Value::as_str).unwrap_or("?"), value.get("path").and_then(Value::as_str).unwrap_or(""));
    }
    value.get("message").and_then(Value::as_str).map_or_else(|| format!("{}: done", job.label), str::to_string)
}

/// JobResults: the listing, then each finished job: its outcome to its
/// REST caller, `last`, the status line, and what follows it (new opens
/// the created file; a guess fills the import form's unit).
pub(super) fn receive(files: Option<ResMut<CadFiles>>, mut doc: Option<ResMut<CadDocument>>, mut out: MessageWriter<Act<CadAction>>) {
    let Some(mut files) = files else { return };
    if files.listing.pending().is_some()
        && let Some((_, result)) = files.listing.poll()
    {
        files.listed = Some(result.unwrap_or_else(|e| Listing { error: Some(e), ..Default::default() }));
    }
    if files.jobs.is_empty() {
        return;
    }
    let mut finished = Vec::new();
    let mut i = 0;
    while i < files.jobs.len() {
        match files.jobs[i].job.poll() {
            Some(result) => finished.push((files.jobs.remove(i), result)),
            None => i += 1,
        }
    }
    for (job, result) in finished {
        if job.waited {
            keep_result(&mut files, job.seq, (result.clone(), job.then.clone()));
        }
        match (&job.then, &result) {
            // A waited new is opened by its caller (`wait`), which answers with the open's outcome.
            (Then::Open { .. }, Ok(_)) if job.waited => {}
            (Then::Open { path }, Ok(_)) => {
                out.write(Act::ui(CadAction::CadFile(FileArgs { op: FileOp::Open, path: Some(path.clone()), unit: None })));
            }
            (Then::Guess { path }, _) => {
                files.guess = Some((path.clone(), result.clone()));
                if let (Some(form), Ok(v)) = (files.form.as_mut(), &result) {
                    form.guessed(path, v);
                }
            }
            _ => {}
        }
        if let Some(doc) = doc.as_mut() {
            doc.show(result.as_ref().map(|v| message(&job, v)).map_err(Clone::clone));
        }
        files.last = Some((job.label.clone(), result));
    }
}

/// The form's directory listing: subdirectories and the files with one of
/// `extensions` (hidden ones left out), directories first, by name.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Listing {
    /// What was asked: the directory and the extensions (`FileForm::listing_key`).
    pub key: String,
    pub dir: String,
    /// (name, is a directory), at most [`MAX_LISTED`].
    pub entries: Vec<(String, bool)>,
    /// Entries left out past [`MAX_LISTED`].
    pub more: usize,
    pub error: Option<String>,
}
impl Listing {
    pub(crate) fn json(&self) -> Value {
        json!({"dir": self.dir, "entries": self.entries.iter().map(|(n, d)| json!({"name": n, "dir": d})).collect::<Vec<_>>(), "more": self.more, "error": self.error})
    }
}

/// Entries a listing keeps.
const MAX_LISTED: usize = 200;

/// Reads `dir` (on `Pool::Io`).
pub(crate) fn list(key: String, dir: String, extensions: &[&str]) -> Listing {
    let read = match std::fs::read_dir(&dir) {
        Ok(read) => read,
        Err(e) => return Listing { key, dir: dir.clone(), error: Some(format!("{dir}: {e}")), ..Default::default() },
    };
    let mut entries: Vec<(String, bool)> = read
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                return None;
            }
            // Follows symlinks, so a linked directory lists as one.
            let is_dir = std::fs::metadata(e.path()).map(|m| m.is_dir()).unwrap_or(false);
            let ext = sim_runtime::cad_client::extension(&name);
            (is_dir || extensions.contains(&ext.as_str())).then_some((name, is_dir))
        })
        .collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));
    let more = entries.len().saturating_sub(MAX_LISTED);
    entries.truncate(MAX_LISTED);
    Listing { key, dir, entries, more, error: None }
}

/// Starts reading the listing `key` names.
pub(super) fn request_listing(files: &mut CadFiles, key: String, dir: String, extensions: Vec<&'static str>) {
    files.listing.start(Pool::Io, "cad file listing", move |_| Ok(list(key, dir, &extensions)));
}

/// The progress strip's root.
#[derive(Component)]
pub(super) struct StripRoot;

/// Present: the jobs in flight with their seconds, at the 3D view's bottom
/// left (rebuilt when a line changes, at most once a second per job).
pub(super) fn strip(mut commands: Commands, files: Option<Res<CadFiles>>, fonts: Res<UiFonts>, roots: Query<Entity, With<StripRoot>>, mut last: Local<Option<String>>) {
    let lines: Vec<String> = files.as_deref().map_or_else(Vec::new, |f| f.jobs.iter().map(|j| format!("{}… {} s", j.label, j.started.elapsed().as_secs())).collect());
    let key = lines.join("\n");
    let shown = roots.iter().next().is_some();
    if last.as_deref() == Some(key.as_str()) && shown == !lines.is_empty() {
        return;
    }
    *last = Some(key);
    for root in &roots {
        commands.entity(root).despawn();
    }
    if lines.is_empty() {
        return;
    }
    let k = Kit::new(&fonts);
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(LEFT_WIDTH + 8.0),
                bottom: Val::Px(STATUSBAR + 8.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                padding: UiRect::all(Val::Px(8.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(6.0)),
                ..default()
            },
            BackgroundColor(SURFACE),
            BorderColor::all(BORDER),
            FocusPolicy::Block,
            GlobalZIndex(40),
            AccessibleLabel::new("CAD file jobs in flight"),
            StripRoot,
            DespawnOnExit(ModeScope::Cad),
        ))
        .with_children(|p| {
            p.spawn(k.caption("RoboCAD is working (runs to the end once sent)"));
            for line in lines {
                p.spawn(k.text(line, size::SMALL, TEXT, 0));
            }
        });
}
