//! Entering the target mode: the state change, a loaded document's result,
//! and installing what the switch brought on the mode's OnEnter.
use super::prepare::{leaving_blockers, leaving_note};
use super::{Arrival, Document, Switcher, Work, refusal, sources};
use crate::SpatialScene;
use crate::app::ViewerMode;
use crate::app::actions::Origin;
use crate::builder::Builder;
use crate::cad::CadDocument;
use crate::document::{DocumentRegistry, Parked, Source};
use crate::lesson::Learn;
use crate::place_view::PlaceView;
use crate::robot::RobotView;
use bevy::prelude::*;
use serde_json::json;
use std::time::Instant;

/// Hand the arrival to the target mode's OnEnter and set the state.
/// `document` is the one the request named: `handle` records it in the
/// recent documents once the state is entered.
#[allow(clippy::too_many_arguments)]
pub(super) fn enter(world: &mut World, switch: &mut Switcher, origin: Origin, from: ViewerMode, target: ViewerMode, mut arrival: Box<Arrival>, started: Instant, document: Option<Document>) {
    let message = match leaving_note(world, from) {
        Some(note) => format!("Switched to {} mode; {note}.", target.label()),
        None => format!("Switched to {} mode.", target.label()),
    };
    let summary = json!({
        "mode": target, "previous": from, "document": arrival.document.take(),
        "load_seconds": started.elapsed().as_secs_f64(),
        "message": message,
    });
    switch.arrival = Some(arrival);
    switch.entering = Some((origin, target, summary, document));
    switch.revision += 1;
    world.resource_mut::<NextState<ViewerMode>>().set(target);
}

/// JobResults: a switch's document has loaded (enter) or failed (refuse,
/// naming it; the current mode stays). Re-checks the blockers, as
/// `finish_open` does: something may have started while loading.
pub(crate) fn finish_load(world: &mut World) {
    let current = *world.resource::<State<ViewerMode>>().get();
    world.resource_scope(|world, mut switch: Mut<Switcher>| {
        let Some(mut pending) = switch.pending.take() else { return };
        let done = match &mut pending.work {
            Work::Job(job) => job.poll(),
            Work::Robot(view) => view.opened().map(|r| r.map(|()| Box::new(Arrival { document: json!(pending.what), ..Default::default() }))),
        };
        let Some(result) = done else {
            switch.pending = Some(pending);
            return;
        };
        let (origin, target, started, document) = (pending.origin, pending.mode, pending.started, pending.document.take());
        let mut arrival = match result {
            Ok(arrival) => arrival,
            Err(e) => {
                switch.finish(world, origin, Err(refusal(target, current, &format!("{} did not load: {}", pending.what, e.trim_end_matches('.')))));
                return;
            }
        };
        if let Work::Robot(view) = pending.work {
            arrival.robot = Some(*view);
        }
        let blockers = leaving_blockers(world, current, target);
        if !blockers.is_empty() {
            switch.finish(world, origin, Err(refusal(target, current, &blockers.join("; "))));
            crate::jobs::drop_off_thread(arrival, "a refused mode switch");
            return;
        }
        enter(world, &mut switch, origin, current, target, arrival, started, document);
    });
}

/// OnEnter of every mode: installs what the switch brought (documents
/// loaded off the UI thread, or the mode's parked scene) before the scope's
/// own OnEnter spawns the mode's entities, and opens the mode's entry in the
/// document registry: a document a load brought, or the mode's own reopened
/// (the same document again is a reload, its id kept and its revision + 1),
/// or its parked scene unparked (no new revision). At launch nothing was
/// brought (`run` inserts the first mode's documents and the registry); the
/// mode's open document is still made sure of ([`ensure_documents`]).
pub(crate) fn arrive(world: &mut World) {
    let mode = *world.resource::<State<ViewerMode>>().get();
    if let Some(arrival) = world.resource_mut::<Switcher>().arrival.take() {
        install(world, mode, *arrival);
    }
    ensure_documents(world, mode);
}

/// A drop off the UI thread of a parked scene being replaced.
fn drop_parked(world: &mut World, mode: ViewerMode, what: &str) {
    if let Some(old) = world.resource_mut::<DocumentRegistry>().take_parked(mode) {
        crate::jobs::drop_off_thread(old, what);
    }
}

fn install(world: &mut World, mode: ViewerMode, arrival: Arrival) {
    let Arrival { scene, link, builder, learn, models, robot, place, cad, unpark, inspect, document: _ } = arrival;
    if let Some((description, spatial)) = inspect {
        // A loaded assembly replaces the parked one.
        drop_parked(world, ViewerMode::Inspect, "the replaced inspected scene");
        sources::open(world, ViewerMode::Inspect, Source::Assembly { description, spatial });
    }
    if let Some(parked) = unpark {
        match world.resource_mut::<DocumentRegistry>().unpark(parked) {
            Some(Parked::Inspect(parked)) => {
                let (scene, link) = *parked;
                world.insert_resource(scene);
                if let Some(link) = link {
                    world.insert_resource(link);
                }
            }
            Some(Parked::Builder(scene)) => world.insert_resource(*scene),
            None => {}
        }
    }
    if let Some(builder) = builder {
        // A new builder brings its own scene.
        drop_parked(world, ViewerMode::Build, "the replaced builder's scene");
        let source = Source::path(builder.path());
        if let Some(old) = world.remove_resource::<Builder>() {
            crate::jobs::drop_off_thread(old, "the replaced builder");
        }
        world.insert_resource(builder);
        sources::open(world, ViewerMode::Build, source);
    }
    // Within Build/Lessons the scene stays: a new builder recompiles it (as a lesson's scene builder does).
    if let Some(scene) = scene.filter(|_| !world.contains_resource::<SpatialScene>()) {
        world.insert_resource(scene);
    }
    if let Some(link) = link {
        world.insert_resource(link);
    }
    if let Some(learn) = learn {
        let source = lessons_source(&learn);
        if let Some(old) = world.remove_resource::<Learn>() {
            crate::jobs::drop_off_thread(old, "the replaced lessons");
        }
        world.insert_resource(learn);
        sources::open(world, ViewerMode::Lessons, source);
    }
    if let Some(models) = models {
        world.insert_resource(models);
    }
    if let Some(robot) = robot {
        let source = sources::document_source(&robot.document());
        world.insert_resource(robot);
        sources::open(world, ViewerMode::Robot, source);
    }
    if let Some(place) = place {
        let source = Source::path(place.dir.clone());
        world.insert_resource(place);
        sources::open(world, ViewerMode::Place, source);
    }
    if let Some(cad) = cad {
        sources::open(world, ViewerMode::Cad, sources::cad_source(&cad.target));
        // Not expected (CAD mode is entered from another mode, whose exit took the old one); a guard.
        // Its self-started service is released like leave_cad's, so unsaved edits are kept.
        if let Some(mut old) = world.remove_resource::<CadDocument>() {
            old.local_load = None;
            crate::jobs::drop_off_thread(old, "the replaced CAD document");
        }
        world.insert_resource(cad);
    }
    // A switch to the exhibits reopens them on the exhibit last shown.
    if mode == ViewerMode::Phenomena {
        let exhibit = sources::exhibit_of(world.resource::<DocumentRegistry>());
        sources::open(world, ViewerMode::Phenomena, Source::Exhibit { exhibit });
    }
}

/// Lessons' source: its folder and the lesson open in it.
fn lessons_source(learn: &Learn) -> Source {
    Source::Lessons { dir: learn.dir.clone(), lesson: learn.slug().map(str::to_string) }
}

/// The documents `mode` shows are open in the registry, derived from what
/// is in the window (opened only when not already open on the same
/// document): at launch, in a test harness, and Build ↔ Lessons, where the
/// builder's entry stays open (Lessons draws over the builder, and its
/// selection items are the builder's). Inspect's scene names no file: its
/// entry comes from the launch or a load.
fn ensure_documents(world: &mut World, mode: ViewerMode) {
    match mode {
        ViewerMode::Build | ViewerMode::Lessons => {
            if let Some(source) = world.get_resource::<Builder>().map(|b| Source::path(b.path())) {
                sources::ensure_open(world, ViewerMode::Build, source);
            }
            if let Some(source) = world.get_resource::<Learn>().map(lessons_source) {
                sources::ensure_open(world, ViewerMode::Lessons, source);
            }
        }
        ViewerMode::Robot => {
            if let Some(source) = world.get_resource::<RobotView>().map(|v| sources::document_source(&v.document())) {
                sources::ensure_open(world, mode, source);
            }
        }
        ViewerMode::Place => {
            if let Some(source) = world.get_resource::<PlaceView>().map(|p| Source::path(p.dir.clone())) {
                sources::ensure_open(world, mode, source);
            }
        }
        ViewerMode::Cad => {
            if let Some(source) = world.get_resource::<CadDocument>().map(|d| sources::cad_source(&d.target)) {
                sources::ensure_open(world, mode, source);
            }
        }
        ViewerMode::Phenomena => {
            let exhibit = sources::exhibit_of(world.resource::<DocumentRegistry>());
            sources::ensure_open(world, mode, Source::Exhibit { exhibit });
        }
        ViewerMode::Inspect => {}
    }
}
