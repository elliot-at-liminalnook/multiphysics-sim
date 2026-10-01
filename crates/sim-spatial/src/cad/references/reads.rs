//! The references' windowless reads and what follows them (JobResults,
//! after `sync::receive`; one system, [`receive`]):
//!
//! - **Placements**: every image node's `GET /nodes/{id}` (`NodeDetail`'s
//!   `image`, `ImagePlacement::of`) in one `Pool::Dedicated` job per
//!   (generation, shown revision, `cad_refresh` count). A newer revision drops
//!   an older read; while the newer one runs, the last placements stay and are
//!   labelled with the revision they were read at (the dock says "reading"),
//!   never presented as current.
//! - **System status**: `system_status()` per (generation, shown revision)
//!   while the dock is open (or Open in builder asked for it): "Reading the
//!   linked system file…" until then. Opening the dock and every link edit
//!   read it again (the file can change on disk without a RoboCAD revision).
//! - **Form**: the current image (the first image when none is current, as
//!   RoboCAD's list) and its form, reloaded from each newer placement.
//! - **After an import**: the align on its last image, once its placement is
//!   read (written as `CadReferences {op: align}`).
//! - **Open in builder**: the switch request becomes `Act<WindowAction>`
//!   (`system_link::switch_action`), as `results::link::receive` does.
//! - The path field's directory listing lands (`ui_kit::path_field::receive`).
//! - The calibrate tool ends when another tool or interaction took over
//!   (RoboCAD's `set_tool` replaces it).
use super::{ReferencesArgs, ReferencesOp, form::PlacementForm, images};
use crate::app::actions::Act;
use crate::app::switch::WindowAction;
use crate::app::{ViewerMode, ViewerSet};
use crate::cad::actions::CadAction;
use crate::cad::document::{CadDocument, CadTool};
use crate::jobs::{Job, Pool};
use bevy::ecs::message::Messages;
use bevy::prelude::*;
use sim_runtime::cad_client::{ImagePlacement, SystemStatus};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

/// (generation, shown revision).
pub(crate) type Key = (u64, u64);

/// One image's placement as read.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    /// RoboCAD's revision it was read at.
    pub revision: u64,
    pub placement: Result<ImagePlacement, String>,
}

/// The placements and the system status as read, and their jobs.
#[derive(Default)]
pub struct Reads {
    pub(crate) placements: HashMap<String, Placed>,
    /// The (key, refresh count) the placements were read at.
    pub(crate) placements_at: Option<(Key, u64)>,
    #[allow(clippy::type_complexity)]
    pub(crate) placement_job: Option<((Key, u64), Job<Vec<(String, Result<ImagePlacement, String>)>>)>,
    pub(crate) status: Option<(Key, Result<SystemStatus, String>)>,
    pub(crate) status_job: Option<(Key, Job<SystemStatus>)>,
    /// Open in builder, Accept changes or Unlink asked for the status with
    /// the dock closed (an atomic: the controls' readiness, which reads the
    /// document shared, asks for it too).
    pub(crate) want_status: AtomicBool,
}

/// Image `id`'s placement as last read (possibly at an older revision), or why not.
pub(crate) fn placement<'a>(doc: &'a CadDocument, id: &str) -> Result<&'a ImagePlacement, String> {
    match doc.references.reads.placements.get(id) {
        Some(Placed { placement: Ok(p), .. }) => Ok(p),
        Some(Placed { placement: Err(e), .. }) => Err(format!("the placement of {} could not be read: {e}", doc.node_name(id))),
        None => Err(format!("the placement of {} is being read from RoboCAD; try again in a moment", doc.node_name(id))),
    }
}

/// Image `id`'s placement read at the shown revision, or why not.
pub(crate) fn current_placement<'a>(doc: &'a CadDocument, id: &str) -> Result<&'a ImagePlacement, String> {
    let p = placement(doc, id)?;
    let at = doc.references.reads.placements.get(id).map_or(0, |p| p.revision);
    if at != doc.shown_revision() {
        return Err(format!("the placement of {} is being read again for revision {} (read at {at}); try again in a moment", doc.node_name(id), doc.shown_revision()));
    }
    Ok(p)
}

/// The current image: kept while it is an image of the shown tree, else the
/// first image (RoboCAD's `setCurrentRow(0)`); its form loaded from a newer
/// placement. True when anything changed.
pub(crate) fn follow_form(doc: &mut CadDocument) -> bool {
    let mut changed = false;
    if doc.doc.is_some() {
        let ids: Vec<String> = images(doc).into_iter().map(|n| n.id.clone()).collect();
        // An import's image is not in the tree until the refetch after it.
        let shown = doc.shown_revision();
        if doc.references.align_after.as_ref().is_some_and(|(id, at)| !ids.contains(id) && shown > *at) {
            doc.references.align_after = None;
            changed = true;
        }
        let importing = doc.references.align_after.as_ref().map(|(id, _)| id);
        let keep = doc.references.current.as_ref().is_some_and(|c| ids.contains(c) || importing == Some(c));
        if !keep {
            let first = ids.first().cloned();
            if doc.references.current != first {
                doc.references.current = first;
                doc.references.form = None;
                changed = true;
            }
        }
    }
    let Some(id) = doc.references.current.clone() else {
        if doc.references.form.take().is_some() {
            changed = true;
        }
        return changed;
    };
    let locked = super::image(doc, &id).is_some_and(|n| n.locked);
    let fresh = match doc.references.reads.placements.get(&id) {
        Some(Placed { revision, placement: Ok(p) }) => Some(PlacementForm::load(&id, *revision, p, locked)),
        _ => None,
    };
    let reload = match (&doc.references.form, &fresh) {
        (Some(f), Some(n)) => f.id != n.id || f.began != n.began,
        (None, Some(_)) => true,
        (Some(f), None) => f.id != id,
        (None, None) => false,
    };
    if reload {
        doc.references.form = fresh;
        if matches!(doc.references.focus, Some(super::Focus::Row(_))) {
            doc.references.focus = None;
        }
        changed = true;
    }
    changed
}

/// The reads at the current keys: an older key's job dropped, a result
/// landed, a read started when needed. True when anything shown changed.
pub(crate) fn tick(doc: &mut CadDocument) -> bool {
    let now: Key = (doc.generation, doc.shown_revision());
    let at = (now, doc.mesh_retry);
    let mut changed = false;
    let client = doc.client.clone().filter(|_| doc.connected());
    let ids: Vec<String> = images(doc).into_iter().map(|n| n.id.clone()).collect();
    let reads = &mut doc.references.reads;
    // Placements.
    if reads.placement_job.as_ref().is_some_and(|(k, _)| *k != at) {
        reads.placement_job = None;
        changed = true;
    }
    let landed = reads.placement_job.as_ref().and_then(|(k, job)| job.poll().map(|r| (*k, r)));
    if let Some((k, result)) = landed {
        reads.placement_job = None;
        reads.placements_at = Some(k);
        match result {
            Ok(list) => reads.placements = list.into_iter().map(|(id, placement)| (id, Placed { revision: k.0.1, placement })).collect(),
            // The whole read failed: every image of the tree says why (none stays "reading").
            Err(e) => reads.placements = ids.iter().map(|id| (id.clone(), Placed { revision: k.0.1, placement: Err(e.clone()) })).collect(),
        }
        changed = true;
    }
    let reads = &mut doc.references.reads;
    if doc.doc.is_some() && reads.placement_job.is_none() && reads.placements_at != Some(at) {
        if ids.is_empty() {
            reads.placements.clear();
            reads.placements_at = Some(at);
            changed = true;
        } else if let Some(c) = client.clone() {
            let job = Job::spawn(Pool::Dedicated, now.0, "cad reference placements", move |ctx| {
                let mut out = Vec::new();
                for id in ids {
                    if ctx.cancelled() {
                        return Err("superseded by a newer revision".to_string());
                    }
                    let p = c.node(&id).map_err(|e| e.to_string()).and_then(|d| ImagePlacement::of(&d).ok_or_else(|| format!("{} has no image placement", d.summary.name)));
                    out.push((id, p));
                }
                Ok(out)
            });
            reads.placement_job = Some((at, job));
            changed = true;
        }
    }
    // The system status, while the dock is open or Open in builder waits for it.
    if reads.status_job.as_ref().is_some_and(|(k, _)| *k != now) {
        reads.status_job = None;
    }
    let landed = reads.status_job.as_ref().and_then(|(k, job)| job.poll().map(|r| (*k, r)));
    if let Some((k, result)) = landed {
        reads.status_job = None;
        reads.status = Some((k, result));
        reads.want_status.store(false, Ordering::Relaxed);
        changed = true;
    }
    let wanted = (doc.references.open || reads.want_status.load(Ordering::Relaxed)) && reads.status_job.is_none() && reads.status.as_ref().is_none_or(|(k, _)| *k != now);
    if wanted && doc.doc.is_some()
        && let Some(c) = client
    {
        reads.status_job = Some((now, Job::spawn(Pool::Dedicated, now.0, "cad system status", move |_| c.system_status().map_err(|e| e.to_string()))));
        changed = true;
    }
    // Another tool or interaction replaced the calibrate tool.
    if doc.references.calibrate.is_some() && (doc.tool != CadTool::Select || doc.ops.active.is_some()) {
        doc.references.calibrate = None;
        changed = true;
    }
    changed |= follow_form(doc);
    changed
}

/// JobResults (windowless): [`tick`], the listing, the align after an import
/// and Open in builder's switch.
pub(super) fn receive(doc: Option<ResMut<CadDocument>>, cad: Option<ResMut<Messages<Act<CadAction>>>>, window: Option<ResMut<Messages<Act<WindowAction>>>>) {
    let Some(mut doc) = doc else { return };
    // Not marked changed unless something shown changed (the panels and
    // other readers follow `CadDocument`'s change detection).
    let d = doc.bypass_change_detection();
    let mut changed = tick(d);
    if crate::ui_kit::path_field::receive(&mut d.references.listing, &mut d.references.listed) {
        changed = true;
    }
    // After an import: align on the last image once its placement is read.
    if let Some((id, _)) = d.references.align_after.clone()
        && current_placement(d, &id).is_ok()
    {
        d.references.align_after = None;
        if let Some(mut out) = cad {
            out.write(Act::quiet(ReferencesArgs::on(ReferencesOp::Align, &id).action()));
        }
        changed = true;
    }
    if let Some(path) = d.references.switch_to.take() {
        if let Some(mut window) = window {
            window.write(super::system_link::switch_action(path));
        }
        changed = true;
    }
    if changed {
        d.touch();
        doc.set_changed();
    }
}

/// CadCorePlugin: [`receive`] (JobResults, after `sync::receive`).
pub(super) fn build_core(app: &mut App) {
    app.add_systems(Update, receive.after(crate::cad::sync::receive).in_set(ViewerSet::JobResults).run_if(in_state(ViewerMode::Cad)));
}
