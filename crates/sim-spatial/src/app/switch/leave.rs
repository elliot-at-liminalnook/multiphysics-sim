//! Leaving a mode: each scope's OnExit removes (or parks) its resources,
//! parking its scene on its document registry entry or closing the entry
//! with the source the mode reopens, and the lesson screen is shown or
//! hidden with Lessons.
use super::sources;
use crate::app::ViewerMode;
use crate::builder::Builder;
use crate::cad::{CadDocument, CadTarget};
use crate::document::{DocumentRegistry, Parked, Source};
use crate::lesson::Learn;
use crate::place_view::PlaceView;
use crate::robot::RobotView;
use crate::{SelectionLink, SpatialScene};
use bevy::prelude::*;

/// Park `parked` on `mode`'s registry entry; with no entry (nothing names
/// what it shows) it is dropped off the UI thread, and the mode loads again.
fn park(world: &mut World, mode: ViewerMode, parked: Parked, what: &str) {
    if let Err(parked) = world.resource_mut::<DocumentRegistry>().park(mode, parked) {
        crate::jobs::drop_off_thread(parked, what);
    }
}

/// OnExit(Inspect): the inspected scene and its selection link are parked
/// on Inspect's registry entry, to come back as they were.
pub(super) fn leave_inspect(world: &mut World) {
    let scene = world.remove_resource::<SpatialScene>();
    let link = world.remove_resource::<SelectionLink>();
    match scene {
        Some(scene) => park(world, ViewerMode::Inspect, Parked::Inspect(Box::new((scene, link))), "an inspected scene with no document"),
        None => world.resource_mut::<DocumentRegistry>().close(ViewerMode::Inspect),
    }
    clear_render(world);
}

/// OnExit(Build/Lessons): the builder stays, paused (no physics runs
/// unseen), with its scene parked on Build's registry entry; the lesson and
/// its jobs are dropped, its entry closed on the lesson last shown.
pub(super) fn leave_builder(world: &mut World) {
    match world.remove_resource::<SpatialScene>() {
        Some(scene) => park(world, ViewerMode::Build, Parked::Builder(Box::new(scene)), "a builder scene with no document"),
        None => world.resource_mut::<DocumentRegistry>().close(ViewerMode::Build),
    }
    if let Some(learn) = world.remove_resource::<Learn>() {
        let source = Source::Lessons { dir: learn.dir.clone(), lesson: learn.slug().map(str::to_string) };
        crate::jobs::drop_off_thread(learn, "the lessons");
        sources::left(world, ViewerMode::Lessons, Some(source));
    }
    if let Some(mut builder) = world.get_resource_mut::<Builder>() {
        builder.leave_scope();
    }
    // The lesson reader's text size is theirs, not the other modes'.
    if let Some(mut scale) = world.get_resource_mut::<UiScale>() {
        if scale.0 != 1.0 {
            scale.0 = 1.0;
        }
    }
    clear_render(world);
}

/// A scene `render` still running belongs to the scene that was left.
fn clear_render(world: &mut World) {
    if let Some(mut rest) = world.get_resource_mut::<crate::rest::Rest>() {
        rest.1 = None;
    }
}

/// OnExit(Robot): the view (its run, gait and playback threads, its loads)
/// is removed and dropped off the UI thread.
pub(super) fn leave_robot(world: &mut World) {
    if let Some(view) = world.remove_resource::<RobotView>() {
        let source = sources::document_source(&view.document());
        crate::jobs::drop_off_thread(view, "the robot view");
        sources::left(world, ViewerMode::Robot, Some(source));
    } else {
        sources::left(world, ViewerMode::Robot, None);
    }
}

/// OnExit(Cad): the document is removed; its self-started RoboCAD service is
/// released here, synchronously and without blocking
/// (`CadDocument::release_child`: the child slot is closed, so a service
/// still starting while "Connecting…" is stopped too; killed and reaped,
/// never an attached one); the rest is dropped off the UI thread (its poll
/// worker joins) and CAD mode's other resources go (`cad::clear`). Leaving
/// with a self-started document's unsaved edits, or edits whose saved state
/// can't be confirmed, is refused (`leaving_blockers`); if edits appear
/// between that check and this exit, the service is left running instead
/// and CAD mode reattaches to its URL next time, so they are not lost. A
/// pending thread reveal (`cad::threads::RevealThread`) is dropped.
pub(super) fn leave_cad(world: &mut World) {
    if let Some(mut doc) = world.remove_resource::<CadDocument>() {
        let target = match doc.release_child("leaving CAD mode") {
            Some(url) => CadTarget::Service(url),
            None => doc.target.clone(),
        };
        crate::jobs::drop_off_thread(doc, "the CAD document");
        sources::left(world, ViewerMode::Cad, Some(sources::cad_source(&target)));
    } else {
        sources::left(world, ViewerMode::Cad, None);
    }
    // A thread another mode asked to reveal that had not landed belongs to
    // this visit: it must not open on a later one.
    if let Some(mut reveal) = world.get_resource_mut::<crate::cad::threads::RevealThread>() {
        reveal.set_if_neq(crate::cad::threads::RevealThread(None));
    }
    crate::cad::clear(world);
}

/// OnExit(Place): the walkthrough's model is dropped.
pub(super) fn leave_place(world: &mut World) {
    let source = world.remove_resource::<PlaceView>().map(|place| Source::path(place.dir.clone()));
    sources::left(world, ViewerMode::Place, source);
}

/// OnEnter(Lessons): the lesson screen is shown (its own bookkeeping), and
/// the builder under it is paused (`Builder::pause_for_learn`): a live run is
/// paused and kept, not dropped (Run resumes it in build mode). Every entry
/// to Lessons from Build goes through [`handle`](super::handle), which refuses on a draft
/// or drag (`leaving_blockers`); one begun in the frame between that check
/// and this state change is ended (a drag) or kept without keys (a draft).
pub(super) fn show_lessons(learn: Option<ResMut<Learn>>, builder: Option<ResMut<Builder>>) {
    if let Some(mut learn) = learn {
        if let Some(mut builder) = builder {
            builder.pause_for_learn();
        }
        learn.show(true);
    }
}
/// OnExit(Lessons): the builder is shown (or the lesson is closed next).
pub(super) fn hide_lessons(learn: Option<ResMut<Learn>>) {
    if let Some(mut learn) = learn {
        learn.show(false);
    }
}
