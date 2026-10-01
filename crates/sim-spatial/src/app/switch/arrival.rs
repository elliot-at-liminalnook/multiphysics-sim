//! Entering the target mode: the state change, a loaded document's result,
//! and installing what the switch brought on the mode's OnEnter.
use super::prepare::{leaving_blockers, leaving_note};
use super::{Arrival, Documents, Switcher, Work, refusal};
use crate::SpatialScene;
use crate::app::ViewerMode;
use crate::app::actions::Origin;
use crate::builder::Builder;
use crate::cad::CadDocument;
use crate::lesson::Learn;
use bevy::prelude::*;
use serde_json::json;
use std::time::Instant;

/// Hand the arrival to the target mode's OnEnter and set the state.
pub(super) fn enter(world: &mut World, switch: &mut Switcher, origin: Origin, from: ViewerMode, target: ViewerMode, mut arrival: Box<Arrival>, started: Instant) {
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
    switch.entering = Some((origin, target, summary));
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
        let (origin, target, started) = (pending.origin, pending.mode, pending.started);
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
        enter(world, &mut switch, origin, current, target, arrival, started);
    });
}

/// OnEnter of every mode: installs what the switch brought (documents
/// loaded off the UI thread, or the mode's parked scene) before the scope's
/// own OnEnter spawns the mode's entities. Nothing at launch: `run` inserts
/// the first mode's documents.
pub(crate) fn arrive(world: &mut World) {
    let Some(arrival) = world.resource_mut::<Switcher>().arrival.take() else { return };
    let Arrival { scene, link, builder, learn, models, robot, place, cad, unpark_inspect, unpark_builder, inspect, lessons, document: _ } = *arrival;
    let (parked_inspect, parked_builder) = {
        let mut docs = world.resource_mut::<Documents>();
        if inspect.is_some() {
            docs.inspect = inspect;
            docs.parked_inspect = None;
        }
        if lessons.is_some() {
            docs.lessons = lessons;
        }
        if builder.is_some() {
            // A new builder brings its own scene.
            docs.parked_builder = None;
        }
        (if unpark_inspect { docs.parked_inspect.take() } else { None }, if unpark_builder { docs.parked_builder.take() } else { None })
    };
    if let Some(parked) = parked_inspect {
        let (scene, link) = *parked;
        world.insert_resource(scene);
        if let Some(link) = link {
            world.insert_resource(link);
        }
    }
    if let Some(scene) = parked_builder {
        world.insert_resource(*scene);
    }
    if let Some(builder) = builder {
        if let Some(old) = world.remove_resource::<Builder>() {
            crate::jobs::drop_off_thread(old, "the replaced builder");
        }
        world.insert_resource(builder);
    }
    // Within Build/Lessons the scene stays: a new builder recompiles it (as a lesson's scene builder does).
    if let Some(scene) = scene.filter(|_| !world.contains_resource::<SpatialScene>()) {
        world.insert_resource(scene);
    }
    if let Some(link) = link {
        world.insert_resource(link);
    }
    if let Some(learn) = learn {
        if let Some(old) = world.remove_resource::<Learn>() {
            crate::jobs::drop_off_thread(old, "the replaced lessons");
        }
        world.insert_resource(learn);
    }
    if let Some(models) = models {
        world.insert_resource(models);
    }
    if let Some(robot) = robot {
        world.insert_resource(robot);
    }
    if let Some(place) = place {
        world.insert_resource(place);
    }
    if let Some(cad) = cad {
        world.resource_mut::<Documents>().cad = Some(cad.target.clone());
        // Not expected (CAD mode is entered from another mode, whose exit took the old one); a guard.
        // Its self-started service is released like leave_cad's, so unsaved edits are kept.
        if let Some(mut old) = world.remove_resource::<CadDocument>() {
            // release_child logs the URL of a service it leaves running.
            old.release_child("the replaced CAD document");
            crate::jobs::drop_off_thread(old, "the replaced CAD document");
        }
        world.insert_resource(cad);
    }
}
