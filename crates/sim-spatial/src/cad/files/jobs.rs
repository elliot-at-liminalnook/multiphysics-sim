//! The file jobs (export, render, new, the unit guess) and the form's
//! directory listing (the kit path field's `Listing`, read by
//! `ui_kit::path_field::request` from `form::input` and received here by
//! `path_field::receive`): started from the handler, polled in JobResults,
//! shown in the status line, `cad_state.files` and the progress strip; a
//! REST caller waits on its job's sequence (`file_job` in its continuation);
//! a waited new's open runs in that wait, so its answer is the open's.
//!
//! Writes (export, render, new) are `complete_on_drop`: leaving CAD mode
//! drops `CadFiles` (`cad::clear`) but they run to the end and log their
//! outcome (`logged`), so a sent export is never silently lost. The unit
//! guess and the listing are reads, cancelled on drop. An export or a
//! render is cancelled only by asking ([`cancel`]: the strip's Cancel,
//! `cad_file {op: cancel}`), and its outcome says what was written.
use super::{CadFiles, FileArgs, FileOp};
use crate::app::ModeScope;
use crate::app::actions::{Act, Call};
use crate::cad::actions::{CadAction, Cx};
use crate::cad::document::CadDocument;
use crate::jobs::{Ctx, Job, Pool};
use crate::ui_kit::path_field;
use crate::ui_kit::{BORDER, Kit, LEFT_WIDTH, Look, STATUSBAR, SURFACE, TEXT, UiFonts, above_strip, size};
use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use bevy::ui::prelude::AccessibleLabel;
use serde_json::{Value, json};
use sim_api::Outcome;
use crate::cad::types::CadError;
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
    /// A cancel was asked ([`cancel`]); the job still runs to RoboCAD's answer.
    pub cancelled: bool,
}
impl FileJob {
    pub(crate) fn json(&self) -> Value {
        let p = self.job.progress();
        json!({"seq": self.seq, "kind": self.kind, "label": self.label, "seconds": self.started.elapsed().as_secs(), "progress": p.message, "waited": self.waited, "cancellable": cancellable(self.kind), "cancel_asked": self.cancelled})
    }
}

/// The jobs a cancel applies to: the writes whose outcome a cancel can
/// change or must name (a new's file is RoboCAD's, created at once; a unit
/// guess is a read).
pub(super) fn cancellable(kind: &str) -> bool {
    matches!(kind, "export" | "render")
}

/// What a cancel of `job` can do, as the status line says it.
fn cancel_note(job: &FileJob) -> String {
    if job.kind == "render" {
        format!("Cancelling {}: RoboCAD finishes drawing (api.py has no cancel route for /render), and this window does not write the PNG unless it already has; the outcome follows", job.label)
    } else {
        format!("Cancel asked for {}, but RoboCAD writes the file anyway (api.py has no cancel route for /export); the outcome follows when it answers", job.label)
    }
}

/// `cad_file {op: cancel, job?}`: the export and render jobs in flight
/// (`seq`: that one; None: every one) are asked to stop (`Job::cancel`; they
/// stay `complete_on_drop` and report their outcome, which says what was
/// written: see the closures in `files::export` and `files::render`). A job
/// whose thread has not started yet sends nothing (`jobs::Job`).
pub(super) fn cancel(files: &mut CadFiles, seq: Option<u64>) -> Result<Value, String> {
    let mut asked = Vec::new();
    let mut notes = Vec::new();
    for job in files.jobs.iter_mut().filter(|j| cancellable(j.kind) && seq.is_none_or(|s| s == j.seq)) {
        job.job.cancel();
        job.cancelled = true;
        asked.push(job.seq);
        notes.push(cancel_note(job));
    }
    if asked.is_empty() {
        return Err(match seq {
            Some(s) => format!("no export or render job {s} is in flight; it may have ended already"),
            None => "no export or render is in flight".to_string(),
        });
    }
    Ok(json!({"cancelled": asked, "message": notes.join("; ")}))
}

/// The open archive's snapshot, or why there is none.
pub(super) fn local(doc: &CadDocument) -> Result<std::sync::Arc<crate::cad::sync::LocalSnapshot>, String> {
    doc.local.clone().filter(|_| doc.connected()).ok_or_else(|| format!("no CAD document is open: {}", doc.connection_line().0))
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
    files.jobs.push(FileJob { seq, kind, label: label.clone(), job, started: Instant::now(), waited, then, cancelled: false });
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
            Outcome::Done(Err(format!("stopped waiting for {}: the request was already sent to RoboCAD, which runs it to the end (api.py has no cancel route); this stops only the wait (cad_file {{op: cancel, job: {}}} keeps a render's PNG unwritten); its outcome shows in cad_state.files.last", j.label, j.seq)))
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
        // The load runs on; `files::handle` forwards the wait to cad_open.
        Outcome::Pending => {
            call.continuation["created"] = json!(path);
            Outcome::Pending
        }
        other => created(other, &path),
    }
}

/// New's answer once the created file's open has landed (or was refused).
pub(super) fn created(outcome: Outcome, path: &str) -> Outcome {
    match outcome {
        Outcome::Done(Ok(v)) => {
            let message = format!("Created {path}; {}", v.get("message").and_then(Value::as_str).unwrap_or("opened it"));
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
    let files = &mut *files;
    path_field::receive(&mut files.listing, &mut files.listed);
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
            keep_result(files, job.seq, (result.clone(), job.then.clone()));
        }
        match (&job.then, &result) {
            // A waited new is opened by its caller (`wait`), which answers with the open's outcome.
            (Then::Open { .. }, Ok(_)) if job.waited => {}
            (Then::Open { path }, Ok(_)) => {
                out.write(Act::ui(CadAction::CadFile(FileArgs { op: FileOp::Open, path: Some(path.clone()), unit: None, job: None })));
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

/// The progress strip's root.
#[derive(Component)]
pub(super) struct StripRoot;

/// One job's line in the strip (its sequence number): its seconds are
/// updated in place, so the strip and its Cancel buttons are not rebuilt
/// every second (a press across a rebuild would be lost).
#[derive(Component)]
pub(super) struct StripLine(u64);

/// Present: the jobs in flight with their seconds, at the 3D view's bottom
/// left, each export and render with its Cancel ([`cancel`], a
/// `CadButton` of `cad_file {op: cancel, job}`, the `cad:file:cancel-<job>`
/// control). Rebuilt when the jobs, a cancel asked or the document source
/// changes; the seconds change in place.
pub(super) fn strip(mut commands: Commands, files: Option<Res<CadFiles>>, doc: Option<Res<CadDocument>>, fonts: Res<UiFonts>, roots: Query<Entity, With<StripRoot>>, mut texts: Query<(&StripLine, &mut Text)>, mut last: Local<Option<String>>) {
    let jobs: &[FileJob] = files.as_deref().map_or(&[][..], |f| f.jobs.as_slice());
    let line = |j: &FileJob| {
        let asked = if j.cancelled { " (cancel asked)" } else { "" };
        format!("{}… {} s{asked}", j.label, j.started.elapsed().as_secs())
    };
    // (sequence, label, the job's Cancel while one can still be asked), and
    // the document source the buttons are stamped for (`activation::stamp`).
    let shape: Vec<(u64, &str, bool)> = jobs.iter().map(|j| (j.seq, j.label.as_str(), cancellable(j.kind) && !j.cancelled)).collect();
    let key = format!("{shape:?}|source={:?}", doc.as_deref().map(crate::cad::activation::render_key));
    let shown = roots.iter().next().is_some();
    if last.as_deref() == Some(key.as_str()) && shown == !jobs.is_empty() {
        for (StripLine(seq), mut text) in &mut texts {
            if let Some(j) = jobs.iter().find(|j| j.seq == *seq) {
                let now = line(j);
                if text.0 != now {
                    text.0 = now;
                }
            }
        }
        return;
    }
    *last = Some(key);
    for root in &roots {
        commands.entity(root).despawn();
    }
    if jobs.is_empty() {
        return;
    }
    let k = Kit::new(&fonts);
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(LEFT_WIDTH + 8.0),
                // Above the status bar, which sits on the switcher strip.
                bottom: above_strip(STATUSBAR + 8.0),
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
            p.spawn(k.caption("RoboCAD is working (it runs a sent request to the end)"));
            for (j, (seq, _, cancel)) in jobs.iter().zip(&shape) {
                p.spawn(Node { column_gap: Val::Px(8.0), align_items: AlignItems::Center, ..default() }).with_children(|row| {
                    row.spawn((k.text(line(j), size::SMALL, TEXT, 0), StripLine(*seq)));
                    if *cancel {
                        let action = CadAction::CadFile(FileArgs { op: FileOp::Cancel, job: Some(*seq), ..Default::default() });
                        row.spawn(k.button("Cancel", crate::cad::panel::CadButton(action), Look::Ghost, true));
                    }
                });
            }
        });
}
