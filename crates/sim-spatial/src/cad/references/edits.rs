//! The references' edits: each one RoboCAD call through `actions::edit_at`
//! (refused by name, nothing sent, with an edit in flight, no connection, a
//! shown tree behind RoboCAD's or values read at an older revision).
//!
//! - **add** (references.py:174-184): `import_references(paths, active plane
//!   or XY)`; "{n} reference image(s) added • Calibrate scale before tracing";
//!   the last image becomes current, the dock shows and the view aligns on it
//!   once its placement is read (`edit_answered`, `reads`). The files are
//!   RoboCAD's to read (no file I/O here); a path must be absolute and name
//!   an image (`image_path`; deliberately stricter than RoboCAD's drop).
//! - **visible** (:166-168): `update_reference(id, visible)`.
//! - **placement** (Apply placement, :186-193): one `update_reference` with
//!   every value (`form::update`): "Reference placement updated • Ctrl+Z undoes".
//! - **remove** (:221-222): RoboCAD's delete of the node (`CadDelete`).
//! - **link** (:105-109), **accept** (:111-113), **unlink** (:115-117):
//!   `link_system(path)`, `refresh_system_link()`, `unlink_system()`; the
//!   status is read again when each answers.
use super::{Pending, ReferencesArgs, ReferencesOp, form, image, reads, target};
use crate::app::actions::Call;
use crate::cad::actions::{CadAction, Cx, edit_at};
use crate::cad::document::{CadDocument, EditDone};
use crate::cad::sketch::BasePlane;
use crate::cad::sync::value;
use serde_json::Value;
use sim_api::Outcome;
use sim_runtime::cad_client::ReferenceUpdate;

/// Remember edit `outcome` as this part's (when it was started).
fn started(doc: &mut CadDocument, outcome: Outcome, kind: Pending) -> Outcome {
    if !matches!(outcome, Outcome::Done(Err(_))) {
        doc.references.pending = Some((doc.edit_seq, kind));
    }
    outcome
}

/// `~/` expanded; refused unless absolute (RoboCAD would resolve a relative
/// path against its own working directory).
fn absolute(path: &str) -> Result<String, String> {
    let p = crate::ui_kit::path_field::expand(path.trim());
    if p.is_empty() || !std::path::Path::new(&p).is_absolute() {
        return Err(format!("{}: type an absolute path (~/ works)", if path.trim().is_empty() { "(empty)" } else { path.trim() }));
    }
    Ok(p)
}

/// An image path Add sends: absolute, a file (not a folder) with one of
/// RoboCAD's dialog suffixes. Deliberately different from RoboCAD's drop,
/// which sends any dropped file and lets Pillow refuse it: refused here by
/// name before anything is sent (no stat on the UI thread: a name without
/// an image suffix, or ending in "/", is not sent).
pub(crate) fn image_path(path: &str) -> Result<String, String> {
    let p = absolute(path)?;
    if p.ends_with('/') {
        return Err(format!("{p} is a folder: choose image files"));
    }
    let exts = super::input::suffixes(super::BrowseKind::Images);
    if !crate::ui_kit::path_field::matches(&p, exts) {
        return Err(format!("{p} is not a reference image ({})", exts.join(", ")));
    }
    Ok(p)
}

/// Add, visible, placement, remove, link, accept and unlink.
pub(super) fn handle(args: &ReferencesArgs, call: &mut Call, cx: &mut Cx) -> Outcome {
    let done = |r: Result<Value, String>| Outcome::Done(r);
    match args.op {
        ReferencesOp::Add => {
            let paths = match args.paths.as_deref().filter(|p| !p.is_empty()).ok_or("add takes paths: the image files RoboCAD reads (absolute)".to_string()).and_then(|p| p.iter().map(|s| image_path(s)).collect::<Result<Vec<_>, _>>()) {
                Ok(p) => p,
                Err(e) => return settle_browse(cx.doc, done(Err(e))),
            };
            // `self.app.viewport.active_plane or Plane.xy()`.
            let plane = cx.plane.arg_or(BasePlane::Xy);
            let sent = paths.clone();
            let outcome = edit_at(cx.doc, call, None, "Import references".into(), move |c| {
                c.import_references(&paths, Some(&plane)).map(|r| {
                    let n = r.result.as_array().map_or(0, Vec::len);
                    EditDone { message: format!("{n} reference image(s) added • Calibrate scale before tracing"), result: value(&r) }
                })
            });
            let outcome = started(cx.doc, outcome, Pending::Import(sent));
            settle_browse(cx.doc, outcome)
        }
        ReferencesOp::Visible => {
            let id = match args.id.as_ref().ok_or("visible takes id and visible".to_string()).and_then(|id| target(cx.doc, Some(id))) {
                Ok(id) => id,
                Err(e) => return done(Err(e)),
            };
            let Some(visible) = args.visible else { return done(Err("visible takes visible: true or false".into())) };
            let name = cx.doc.node_name(&id);
            let update = ReferenceUpdate { visible: Some(visible), ..Default::default() };
            let message = format!("{name} {}", if visible { "shown" } else { "hidden" });
            edit_at(cx.doc, call, args.revision, format!("Edit reference {name}"), move |c| c.update_reference(&id, &update).map(|r| EditDone { message, result: value(&r) }))
        }
        ReferencesOp::Placement => placement(args, call, cx),
        ReferencesOp::Remove => {
            let id = match target(cx.doc, args.id.as_ref()) {
                Ok(id) => id,
                Err(e) => return done(Err(e)),
            };
            // RoboCAD's delete (`ops.delete([id])`), the one delete path.
            crate::cad::actions::handle(&CadAction::CadDelete { id }, call, cx)
        }
        ReferencesOp::Link => {
            let path = match args.path.as_deref().ok_or("link takes path: an absolute *.system.json".to_string()).and_then(absolute) {
                Ok(p) => p,
                Err(e) => return settle_browse(cx.doc, done(Err(e))),
            };
            let (shown, sent) = (path.clone(), path.clone());
            let outcome = edit_at(cx.doc, call, None, "Link system file".into(), move |c| c.link_system(&path).map(|r| EditDone { message: format!("Linked system file {shown}"), result: value(&r) }));
            let outcome = started(cx.doc, outcome, Pending::Link(sent));
            settle_browse(cx.doc, outcome)
        }
        ReferencesOp::Accept => {
            if let Err(e) = super::system_link::accept_ready(cx.doc) {
                return done(Err(format!("Accept changes: {e}")));
            }
            let outcome = edit_at(cx.doc, call, None, "Accept system file changes".into(), |c| c.refresh_system_link().map(|r| EditDone { message: "System file changes accepted".into(), result: value(&r) }));
            started(cx.doc, outcome, Pending::Status)
        }
        ReferencesOp::Unlink => {
            if let Err(e) = super::system_link::unlink_ready(cx.doc) {
                return done(Err(format!("Unlink: {e}")));
            }
            let outcome = edit_at(cx.doc, call, None, "Unlink system file".into(), |c| c.unlink_system().map(|r| EditDone { message: "System file unlinked".into(), result: value(&r) }));
            started(cx.doc, outcome, Pending::Status)
        }
        _ => done(Err("not a references edit".into())),
    }
}

/// After a path field's submit: an accepted request closes the field; a
/// refusal stays in it.
fn settle_browse(doc: &mut CadDocument, outcome: Outcome) -> Outcome {
    if let Some(b) = doc.references.browse.as_mut() {
        match &outcome {
            Outcome::Done(Err(e)) => b.error = Some(e.clone()),
            _ => {
                doc.references.browse = None;
                if doc.references.focus == Some(super::Focus::Path) {
                    doc.references.focus = None;
                }
            }
        }
        doc.touch();
    }
    outcome
}

/// Apply placement (references.py:186-193): the given values, the image's
/// current ones for any left out; one `update_reference`.
fn placement(args: &ReferencesArgs, call: &mut Call, cx: &mut Cx) -> Outcome {
    let done = |r: Result<Value, String>| Outcome::Done(r);
    let id = match target(cx.doc, args.id.as_ref()) {
        Ok(id) => id,
        Err(e) => return done(Err(e)),
    };
    let (current, at) = match reads::placement(cx.doc, &id) {
        Ok(p) => (form::current_values(p), cx.doc.references.reads.placements.get(&id).map(|p| p.revision)),
        Err(e) => return done(Err(e)),
    };
    let v = form::Values {
        width: args.width.unwrap_or(current.width),
        origin: args.origin.unwrap_or(current.origin),
        rotation_deg: args.rotation_deg.unwrap_or(current.rotation_deg),
        opacity_pct: args.opacity_pct.unwrap_or(current.opacity_pct),
    };
    let locked = args.locked.unwrap_or_else(|| image(cx.doc, &id).is_some_and(|n| n.locked));
    let update = match form::update(v, args.plane.unwrap_or_default(), locked, cx.plane) {
        Ok(u) => u,
        Err(e) => return refused(cx.doc, e),
    };
    // The revision the values were read at: the caller's, else the placement's.
    let began = args.revision.or(at);
    let name = cx.doc.node_name(&id);
    let outcome = edit_at(cx.doc, call, began, format!("Edit reference {name}"), move |c| c.update_reference(&id, &update).map(|r| EditDone { message: "Reference placement updated • Ctrl+Z undoes".into(), result: value(&r) }));
    if let Outcome::Done(Err(e)) = &outcome {
        return refused(cx.doc, e.clone());
    }
    started(cx.doc, outcome, Pending::Placement)
}

/// A refused Apply: shown in the dock under the button too.
fn refused(doc: &mut CadDocument, e: String) -> Outcome {
    if let Some(f) = doc.references.form.as_mut() {
        f.error = Some(e.clone());
        doc.touch();
    }
    Outcome::Done(Err(e))
}
