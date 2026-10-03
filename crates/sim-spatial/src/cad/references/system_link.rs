//! The linked system file (system_link.py, commands.py:1112-1136,
//! references.py:90-130): the status line, the readiness of Accept changes,
//! Open in builder and Unlink, and Open in builder.
//!
//! **Open in builder** (references.py:119-130) starts no process: RoboCAD's
//! `subprocess.Popen(sim-spatial --system …)` would be a second window. This
//! window switches to Build mode on the linked file instead
//! (`WindowAction::Switch(ModeSwitch { mode: Build, document:
//! Document::Path })`, written by `reads::receive` as `results::link::receive`
//! writes the live link's switch). The switch's own `prepare` opens it
//! through the document registry (`app/switch/prepare.rs`, Build: the build
//! loader on a job) and refuses by name what it refuses (the builder holds
//! another file it can open itself, a lesson's sandbox). Refused here first,
//! with nothing written: RoboCAD's "Link an existing system file first"
//! when unlinked or missing, and whatever the mode switch would refuse when
//! leaving CAD mode (`CadDocument::switch_blockers` and the sketch blocker,
//! `results::link::switch_refusal`'s check).
use crate::app::ViewerMode;
use crate::app::actions::{Act, Origin};
use crate::app::switch::{Document, ModeSwitch, WindowAction};
use crate::cad::document::CadDocument;
use serde_json::{Value, json};
use sim_runtime::cad_client::{LinkState, SystemStatus};
use std::path::PathBuf;
use std::sync::atomic::Ordering;

/// RoboCAD's refusal of Open in builder (references.py:125).
pub(crate) const LINK_FIRST: &str = "Link an existing system file first";
/// Shown until the status is read.
pub(crate) const READING: &str = "Reading the linked system file…";

/// The status as last read in this connection (any revision).
pub(crate) fn status(doc: &CadDocument) -> Option<&Result<SystemStatus, String>> {
    doc.references.reads.status.as_ref().filter(|(k, _)| k.0 == doc.generation).map(|(_, r)| r)
}

/// The status read at the shown revision. When there is none, the read is
/// asked for (also with the dock closed: `reads::tick` starts it), so a
/// refusal "Reading the linked system file…" resolves by itself.
fn current(doc: &CadDocument) -> Option<&SystemStatus> {
    let s = fresh(doc);
    if s.is_none() {
        doc.references.reads.want_status.store(true, Ordering::Relaxed);
    }
    s
}

/// The status read at the shown revision, without asking for a read (the
/// REST snapshot reads it every frame).
fn fresh(doc: &CadDocument) -> Option<&SystemStatus> {
    doc.references.reads.status.as_ref().filter(|(k, _)| *k == (doc.generation, doc.shown_revision())).and_then(|(_, r)| r.as_ref().ok())
}

/// Why the status at the shown revision is not known: being read, or the read's error.
fn unknown(doc: &CadDocument) -> String {
    match status(doc) {
        Some(Err(e)) if doc.references.reads.status.as_ref().is_some_and(|(k, _)| *k == (doc.generation, doc.shown_revision())) => format!("the linked system file's status could not be read: {e}"),
        _ => READING.to_string(),
    }
}

/// The dock's status line: RoboCAD's text (`SystemStatus::line`), or
/// "Reading the linked system file…" until it is read, or why it could not be.
pub(crate) fn line(doc: &CadDocument) -> String {
    match status(doc) {
        None => READING.to_string(),
        Some(Ok(s)) => s.line(),
        Some(Err(e)) => format!("The linked system file's status could not be read: {e}"),
    }
}

/// Accept changes: only when the linked file changed since it was linked.
pub(crate) fn accept_ready(doc: &CadDocument) -> Result<(), String> {
    match current(doc).map(|s| s.state) {
        Some(LinkState::Changed) => Ok(()),
        Some(LinkState::Unlinked) => Err("No system file is linked".into()),
        Some(LinkState::Missing) => Err("the linked system file is missing".into()),
        Some(_) => Err("the linked system file has not changed since it was linked".into()),
        None => Err(unknown(doc)),
    }
}

/// Unlink: only with a link (a missing file is still linked).
pub(crate) fn unlink_ready(doc: &CadDocument) -> Result<(), String> {
    match current(doc).map(|s| s.state) {
        Some(LinkState::Unlinked) => Err("No system file is linked".into()),
        Some(_) => Ok(()),
        None => Err(unknown(doc)),
    }
}

/// What leaving CAD mode now would be refused for: the live link's own
/// check (`results::link::switch_refusal`, the mode switch's CAD checks).
fn switch_refusal(doc: &CadDocument) -> Option<String> {
    crate::cad::results::switch_refusal(doc)
}

/// The linked file Open in builder opens, or why not.
pub(crate) fn builder_target(doc: &CadDocument) -> Result<PathBuf, String> {
    // The status of the shown revision (a link edit or an older read may name another file).
    let Some(s) = current(doc) else { return Err(format!("{}; try again in a moment", unknown(doc))) };
    if matches!(s.state, LinkState::Unlinked | LinkState::Missing) {
        return Err(LINK_FIRST.into());
    }
    let path = s.path.as_deref().filter(|p| !p.is_empty()).ok_or("RoboCAD's status names no path for the linked system file")?;
    let path = PathBuf::from(path);
    if !path.is_absolute() {
        return Err(format!("the linked system file's path {} is not absolute; link it again", path.display()));
    }
    Ok(path)
}

/// Open in builder's readiness (the control's).
pub(crate) fn open_ready(doc: &CadDocument) -> Result<(), String> {
    builder_target(doc)?;
    switch_refusal(doc).map_or(Ok(()), |why| Err(format!("Build mode can't be opened now: {why}")))
}

/// `op: open_builder`: the switch request, or the refusal by name. Without
/// a window (`windowed` false: CAD mode's core alone, no 3D view) there is
/// no window to switch, and nothing is requested.
pub(crate) fn open_builder(doc: &mut CadDocument, windowed: bool) -> Result<Value, String> {
    let path = builder_target(doc)?;
    if !windowed {
        return Err(format!("Not opening {} in Build mode: there is no window to switch", path.display()));
    }
    if let Some(why) = switch_refusal(doc) {
        return Err(format!("Not opening {} in Build mode: {why}", path.display()));
    }
    doc.references.switch_to = Some(path.clone());
    // The switch is applied later by the window's handler, which may still
    // refuse it (e.g. the builder holds another file): its line says so.
    let message = format!("Switching to Build mode on {} (the mode switcher's line says if the switch is refused)", path.display());
    doc.show(Ok(message.clone()));
    Ok(json!({"switching": "build", "path": path, "message": message}))
}

/// The window action Open in builder becomes.
pub(crate) fn switch_action(path: PathBuf) -> Act<WindowAction> {
    Act { action: WindowAction::Switch(ModeSwitch { mode: ViewerMode::Build, document: Some(Document::Path(path)), reveal: None }), origin: Origin::Ui }
}

/// `cad_state.references.system`.
pub(crate) fn json(doc: &CadDocument) -> Value {
    let read = status(doc);
    json!({
        "line": line(doc),
        "status": read.and_then(|r| r.as_ref().ok()),
        "read_at": doc.references.reads.status.as_ref().map(|(k, _)| k.1),
        "current": fresh(doc).is_some(),
        "reading": doc.references.reads.status_job.is_some(),
        "switch_to": doc.references.switch_to,
    })
}
