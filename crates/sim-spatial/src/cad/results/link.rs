//! The live link (RoboCAD's `sim_link_toggle` and `simbridge.SimLink`),
//! inside this window: no process is started.
//!
//! - **On** (refused "Save the document first: the link watches the saved
//!   file" without a document path): the document's `<stem>.simrobot.json`
//!   (`simbridge.sim_model_path`) is exported now, as RoboCAD's
//!   `SimLink.export` does (no flexible links, the x–z planar hint, label
//!   "live simulation model", queued behind a running export: the latest
//!   wins). When that first model is written this window switches to Robot
//!   mode on it, as RoboCAD launches its viewer. That switch is checked
//!   against the switch's own refusal (`CadDocument::switch_blockers` and
//!   the sketch blocker, what the mode switch checks when leaving CAD mode)
//!   first: when it would be refused (another export started from the
//!   queue, an edit in flight, unsaved edits in a self-started service…)
//!   the request is kept (`link_switch`), the status line names the
//!   refusal, and the switch is made after the next export of the link's
//!   model (the next save's).
//! - **Each successful save** (`files::save`, noticed by `note_save` and
//!   settled once RoboCAD answered) exports it again, when the link was on
//!   for this document as the save started (recorded then: a Save As may
//!   have moved RoboCAD's path by the time it settles). When written, the
//!   document registry's Robot entry follows (the same file again: its
//!   revision is bumped; another document there is replaced by this one,
//!   remembered), so Robot mode opens the new model the next time it is
//!   shown (it reads its file whenever it is entered). CAD mode is not left
//!   on every save. Robot mode is never the active mode while a save happens
//!   here (one mode at a time), so there is no in-place reload to request.
//! - **Show in Robot mode** switches on request (the link's model, else
//!   the last export written). While an export runs it is refused ("wait
//!   for it or request cancellation and wait for its outcome"): the switch would drop the export with the
//!   document, and the model on disk is still the previous one.
//! - The link outlives the document ([`LiveLink`], kept across mode
//!   switches): it watches a `.rcad` path, so returning to CAD mode on the
//!   same file keeps it on. simbridge's own watch-and-run stays for
//!   RoboCAD's window.
use super::export::{self, ExportRequest, Landed};
use super::{doc_path, link_active, model_path, settle};
use crate::app::ViewerMode;
use crate::app::actions::{Act, Origin};
use crate::app::switch::sources::kind;
use crate::app::switch::{Document, ModeSwitch, WindowAction};
use crate::cad::actions::CadAction;
use crate::cad::document::CadDocument;
use crate::document::{DocumentKind, DocumentRegistry, Source};
use crate::selection::Selection;
use bevy::ecs::message::Messages;
use bevy::prelude::*;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// RoboCAD's refusal without a document path.
pub(crate) const UNSAVED: &str = "Save the document first: the link watches the saved file";

/// The live link, kept across CAD mode's visits (the document is not).
#[derive(Resource, Default, Debug, PartialEq)]
pub struct LiveLink {
    /// The `.rcad` watched; None: off.
    pub(crate) watching: Option<PathBuf>,
}

/// The live link's export of `rcad`'s model.
fn link_request(rcad: &Path) -> ExportRequest {
    ExportRequest { path: model_path(rcad), flex: false, planar: true, label: "live simulation model".into(), link: true }
}

/// `op: link`.
pub(crate) fn toggle(doc: &mut CadDocument, open: Option<bool>) -> Result<Value, String> {
    let on = open.unwrap_or(!link_active(doc));
    if !on {
        let was = doc.results.link.take();
        doc.results.link_switch = false;
        // A queued link export is no longer wanted; a running one ends (RoboCAD's stop does not cancel it).
        if doc.results.exports.queued.as_ref().is_some_and(|q| q.link) {
            doc.results.exports.queued = None;
        }
        let message = "Simulation link stopped".to_string();
        doc.show(Ok(message.clone()));
        return Ok(json!({"link": false, "was": was, "message": message}));
    }
    let rcad = doc_path(doc).ok_or(UNSAVED)?;
    let started = export::request(doc, link_request(&rcad))?;
    doc.results.link = Some(rcad.clone());
    doc.results.link_switch = true;
    let message = "Simulation link: the model is re-exported on every save and Robot mode shows it".to_string();
    doc.show(Ok(message.clone()));
    Ok(json!({"link": true, "watching": rcad, "model": model_path(&rcad), "export": started, "message": message}))
}

/// A save of this document succeeded: the link's model is exported again
/// when the link watched this document as the save started (`linked`,
/// recorded by `note_save`); a Save As (`path`) moves the link with the
/// document, as RoboCAD's follows `doc.path`. A link stopped or pointed
/// elsewhere while the save ran stays as it is now.
pub(crate) fn saved(doc: &mut CadDocument, path: Option<String>, linked: Option<PathBuf>) {
    let Some(was) = linked else { return };
    if doc.results.link.as_ref() != Some(&was) {
        return;
    }
    let rcad = path.map_or(was, PathBuf::from);
    doc.results.link = Some(rcad.clone());
    if let Err(e) = export::request(doc, link_request(&rcad)) {
        doc.show(Err(format!("Live link: the model was not exported after the save: {e}")));
    }
}

/// The model "Show in Robot mode" opens: the link's, else the last export written.
pub(crate) fn shown_model(doc: &CadDocument) -> Result<PathBuf, String> {
    if link_active(doc)
        && let Some(rcad) = &doc.results.link
    {
        return Ok(model_path(rcad));
    }
    doc.results.exports.written.clone().filter(|p| p.to_string_lossy().ends_with(".simrobot.json")).ok_or_else(|| "no model exported yet: export the simulation model or start the live link first".to_string())
}

/// What "Show in Robot mode" would open now, or why not: refused while an
/// export runs (leaving CAD mode would drop it, and the file on disk is
/// still the previous model).
pub(crate) fn show_target(doc: &CadDocument) -> Result<PathBuf, String> {
    if let Some(running) = &doc.results.exports.running {
        return Err(format!("a model export is running: {} to {}; wait for it or cancel it, then show it in Robot mode", running.request.label, running.request.path.display()));
    }
    shown_model(doc)
}

/// Why leaving CAD mode now would be refused: the mode switch's own CAD
/// checks (`switch::prepare::leaving_blockers` for a CAD → Robot switch).
pub(crate) fn switch_refusal(doc: &CadDocument) -> Option<String> {
    let mut blockers = doc.switch_blockers();
    blockers.extend(crate::cad::sketch_blocker(doc));
    (!blockers.is_empty()).then(|| blockers.join("; "))
}

/// `op: show_robot`.
pub(crate) fn show(doc: &mut CadDocument) -> Result<Value, String> {
    let model = show_target(doc)?;
    doc.results.switch_to = Some(model.clone());
    Ok(json!({"switching": "robot", "model": model, "message": format!("Switching to Robot mode on {}", model.display())}))
}

/// A link model was written: the switch to make (the first after toggling
/// on), or the registry's Robot entry follows it ([`follow`]). A switch
/// the mode switch would refuse ([`switch_refusal`]) is not sent: the
/// request is kept for the next written link model, the entry follows this
/// one, and the status line says why.
pub(crate) fn after_write(doc: &mut CadDocument, registry: &mut DocumentRegistry, selection: Option<&mut Selection>, landed: &Landed) {
    if !landed.link || !link_active(doc) || doc.results.link.as_deref().map(model_path).as_deref() != Some(landed.path.as_path()) {
        return;
    }
    if doc.results.link_switch {
        match switch_refusal(doc) {
            None => {
                doc.results.link_switch = false;
                doc.results.switch_to = Some(landed.path.clone());
                return;
            }
            Some(why) => {
                // The link's own queued export may have just started (`export::poll` starts it before this).
                let when = if doc.results.exports.running.is_some() { "when the export now running is written" } else { "when the link's model is next exported (the next save)" };
                doc.show(Err(format!("Live link: {} written, but Robot mode was not opened: {why}. It opens {when}, or use Show in Robot mode", landed.path.display())))
            }
        }
    }
    follow(registry, selection, &landed.path);
}

/// The Robot entry shows `model` next: the same file again is a reload
/// (revision + 1); another document there is replaced (remembered, its
/// selection items forgotten).
pub(crate) fn follow(registry: &mut DocumentRegistry, selection: Option<&mut Selection>, model: &Path) {
    let source = Source::path(model);
    let entry = registry.entry(ViewerMode::Robot).map(|e| (e.id, e.kind == DocumentKind::Robot && e.source.same_document(&source)));
    match entry {
        Some((id, true)) => {
            registry.bump(id);
        }
        other => {
            registry.remember(ViewerMode::Robot, kind(ViewerMode::Robot), source);
            if let (Some((old, _)), Some(selection)) = (other, selection) {
                selection.forget(old);
            }
        }
    }
}

/// `cad_state.results.link`.
pub(crate) fn json(doc: &CadDocument) -> Value {
    let r = &doc.results;
    json!({
        "on": link_active(doc),
        "watching": r.link,
        "model": r.link.as_deref().map(model_path),
        "switch_on_next_write": r.link_switch,
        "other_document": r.link.is_some() && !link_active(doc),
        "show_robot": shown_model(doc).ok(),
    })
}

/// The window action a switch request becomes.
pub(crate) fn switch_action(model: PathBuf) -> Act<WindowAction> {
    Act { action: WindowAction::Switch(ModeSwitch { mode: ViewerMode::Robot, document: Some(Document::Path(model)) }), origin: Origin::Ui }
}

/// JobResults (windowless): the link mirrored with [`LiveLink`], the edit
/// this part waits for settled, a profiles file read sent to RoboCAD, the
/// exports polled, the registry following a written link model and a
/// switch request written for the mode switch.
#[allow(clippy::type_complexity)]
pub(super) fn receive(
    doc: Option<ResMut<CadDocument>>,
    mut live: ResMut<LiveLink>,
    registry: Option<ResMut<DocumentRegistry>>,
    mut selection: Option<ResMut<Selection>>,
    (window, cad): (Option<ResMut<Messages<Act<WindowAction>>>>, Option<ResMut<Messages<Act<CadAction>>>>),
) {
    let Some(mut doc) = doc else { return };
    // Read first: a `ResMut` deref marks the document changed.
    let idle = doc.results.link_seeded
        && doc.results.link == live.watching
        && doc.results.waiting.is_none()
        && doc.results.profiles_read.is_none()
        && doc.results.exports.running.is_none()
        && doc.results.switch_to.is_none()
        && doc.results.listing.pending().is_none();
    if idle {
        return;
    }
    let doc = &mut *doc;
    // The link: seeded once per document from the one that outlives it, then mirrored.
    if !doc.results.link_seeded {
        doc.results.link_seeded = true;
        if doc.results.link.is_none() {
            doc.results.link = live.watching.clone();
        }
    }
    if doc.results.link != live.watching {
        live.watching = doc.results.link.clone();
    }
    if crate::ui_kit::path_field::receive(&mut doc.results.listing, &mut doc.results.listed) {
        doc.touch();
    }
    settle(doc);
    if let Some((_, job, _, _)) = &doc.results.profiles_read
        && let Some(result) = job.poll()
    {
        let (path, _, mut captured, revision) = doc.results.profiles_read.take().unwrap();
        match result {
            Ok(profiles) => {
                let valid = matches!(&captured, CadAction::Captured { source, .. } if crate::cad::activation::current(source, doc)) && revision == doc.revision;
                doc.results.profiles_retained = Some((path.clone(), profiles.clone()));
                if valid && cad.is_some() {
                    if let CadAction::Captured { action, .. } = &mut captured {
                        **action = CadAction::CadResults(super::ResultsArgs { op: super::ResultsOp::Profiles, profiles: Some(profiles), revision: Some(revision), ..Default::default() });
                    }
                    if let Some(mut out) = cad { out.write(Act::ui(captured)); }
                } else {
                    doc.show(Err(format!("Actuator profiles not applied: captured document changed or action consumer unavailable; input retained ({path})")));
                }
            }
            Err(e) => doc.show(Err(format!("Actuator profiles not applied: {e} ({path})"))),
        }
    }
    if let Some(landed) = export::poll(doc) {
        match registry {
            Some(mut registry) => after_write(doc, &mut registry, selection.as_deref_mut(), &landed),
            // Without a registry (a bare test world) the first switch still goes out.
            None => after_write(doc, &mut DocumentRegistry::default(), selection.as_deref_mut(), &landed),
        }
    }
    if let Some(model) = doc.results.switch_to.take()
        && let Some(mut window) = window
    {
        window.write(switch_action(model));
    }
}
