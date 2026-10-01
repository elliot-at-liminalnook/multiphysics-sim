//! What a switch needs before it can happen: the blockers that refuse
//! leaving the current mode, and the target mode's document (already in the
//! window, or a load off the UI thread).
use super::{Arrival, Document, Documents, ModeSwitch, Work};
use crate::app::ViewerMode;
use crate::builder::Builder;
use crate::cad::{CadDocument, CadTarget};
use crate::jobs::{Job, Pool};
use crate::lesson::Learn;
use crate::place_view::PlaceView;
use crate::robot::RobotView;
use bevy::prelude::*;
use serde_json::json;
use std::path::PathBuf;

/// What would be lost by leaving `current` for `target` now. Leaving the
/// builder's modes, and entering Lessons from Build, is refused on the
/// builder's `system_open` blockers and a pending open
/// (`Builder::switch_blockers`, open.rs); leaving them also on a lesson's
/// draft or capture; replacing the builder with a new lesson's also on a
/// live run of the system file (`Builder::replace_blockers`); leaving robot
/// mode on a recording being written or a replay; leaving CAD mode on an
/// edit in flight, its own service's unsaved edits, or a sketch shape in
/// progress (`cad::sketch_blocker`).
/// Lessons → Build over an open lesson is the lesson screen's own toggle,
/// never blocked (as before the modes were one app). Build → Lessons keeps a
/// live run, paused (`show_lessons`).
pub(super) fn leaving_blockers(world: &World, current: ViewerMode, target: ViewerMode) -> Vec<String> {
    let mut blockers = Vec::new();
    // A new lessons folder brings its own builder in place of this one.
    let replacing = target == ViewerMode::Lessons && !world.contains_resource::<Learn>();
    let overlay = current.builder_family() && target.builder_family() && !replacing;
    // The lesson screen would be drawn over a draft, a drag or work in progress.
    let entering_lessons = current == ViewerMode::Build && target == ViewerMode::Lessons;
    if let Some(b) = world.get_resource::<Builder>() {
        if replacing {
            blockers.extend(b.replace_blockers());
        } else if (current.builder_family() && !overlay) || entering_lessons {
            blockers.extend(b.switch_blockers());
        }
    }
    if current.builder_family() && !overlay {
        if let Some(l) = world.get_resource::<Learn>() {
            blockers.extend(l.switch_blockers());
        }
    }
    if current == ViewerMode::Robot {
        if let Some(view) = world.get_resource::<RobotView>() {
            blockers.extend(view.switch_blockers());
        }
    }
    // An edit in flight; a self-started service's unsaved edits (it stops
    // when CAD mode closes, and the viewer never saves for you).
    if current == ViewerMode::Cad {
        if let Some(doc) = world.get_resource::<CadDocument>() {
            blockers.extend(doc.switch_blockers());
            // A sketch shape with clicked points would be lost (cad-sketch).
            blockers.extend(crate::cad::sketch_blocker(doc));
        }
    }
    blockers
}

/// What the switch's success message adds when CAD mode is left with an
/// attached RoboCAD's unsaved edits (they stay in that service).
pub(super) fn leaving_note(world: &World, from: ViewerMode) -> Option<String> {
    (from == ViewerMode::Cad).then(|| world.get_resource::<CadDocument>().and_then(CadDocument::leaving_note)).flatten()
}

pub(super) enum Prepared {
    /// Nothing to load: the mode's own document is in the window (or parked).
    Now(Box<Arrival>),
    /// A document loads off the UI thread first.
    Load(String, Work),
}

/// The parts registry the launch uses (`$SIM_PARTS_DIR`, else the workspace's).
fn registry() -> sim_core::BehaviorRegistry {
    sim_runtime::system_registry_in(crate::workspace::get().as_ref())
}

/// The target mode's document: what the window already has, or a load.
pub(super) fn prepare(world: &World, current: ViewerMode, request: &ModeSwitch) -> Result<Prepared, String> {
    let docs = world.resource::<Documents>();
    let target = request.mode;
    let path = match &request.document {
        None => None,
        Some(Document::Path(p)) => Some(p.clone()),
        Some(Document::Preset(id)) if target != ViewerMode::Robot => return Err(format!("preset `{id}` opens robot mode only")),
        Some(Document::Preset(_)) => None,
        Some(Document::Url(url)) if target != ViewerMode::Cad => return Err(format!("url `{url}` opens cad mode only (a running RoboCAD service)")),
        Some(Document::Url(_)) => None,
    };
    let in_family = current.builder_family();
    match target {
        ViewerMode::Inspect => {
            if path.is_none() && docs.parked_inspect.is_some() {
                return Ok(Prepared::Now(Box::new(Arrival { unpark_inspect: true, document: json!(docs.inspect.as_ref().map(|(d, _)| d)), ..Default::default() })));
            }
            let (description, spatial) = match &path {
                Some(p) => crate::inspect_pair(p)?,
                None => docs.inspect.clone().unwrap_or_else(crate::default_inspect_paths),
            };
            let what = description.display().to_string();
            let job = Job::spawn(Pool::Compute, 0, format!("{what}: the inspect loader"), move |_| {
                let mut scene = crate::load_inspect(&description, &spatial)?;
                scene.connect_annotations(PathBuf::from(format!("{}.annotations.json", description.display())));
                Ok(Box::new(Arrival { scene: Some(scene), document: json!(description), inspect: Some((description, spatial)), ..Default::default() }))
            });
            Ok(Prepared::Load(what, Work::Job(job)))
        }
        ViewerMode::Build => {
            if let Some(b) = world.get_resource::<Builder>() {
                match &path {
                    Some(p) if !crate::builder::open::same_file(p, b.path()) => {
                        if world.contains_resource::<Learn>() {
                            return Err(format!("the builder shows a lesson's sandbox copy ({}); leave lessons before opening {}", b.path().display(), p.display()));
                        }
                        if b.can_open() {
                            return Err(format!("this window's builder has {} open: switch to build mode, then open {} with system_open (the Systems tab), which keeps a live run", b.path().display(), p.display()));
                        }
                        // A lesson's sandbox builder (no Open) is replaced by the file below.
                    }
                    _ => return Ok(Prepared::Now(Box::new(Arrival { unpark_builder: !in_family, document: json!(b.path()), ..Default::default() }))),
                }
            }
            let path = path.ok_or("build mode needs a system file and this window has none open: give one, e.g. viewer_mode {\"mode\":\"build\",\"path\":\"….system.json\"}")?;
            if !path.is_file() {
                return Err(format!("{}: no such file", path.display()));
            }
            let library = docs.library.clone()?;
            let shell = crate::builder::open::Shell { launch: path.clone(), annotations: None, schematic: None, models: docs.models.clone() };
            let what = path.display().to_string();
            let job = Job::spawn(Pool::Compute, 0, format!("{what}: the build loader"), move |_| {
                let (builder, scene, models) = crate::builder::open::open_build(path.clone(), library, registry(), shell)?;
                Ok(Box::new(Arrival { builder: Some(builder), scene: Some(scene), models: Some(models), document: json!(path), ..Default::default() }))
            });
            Ok(Prepared::Load(what, Work::Job(job)))
        }
        ViewerMode::Lessons => {
            if let Some(l) = world.get_resource::<Learn>() {
                if let Some(p) = path.as_ref().filter(|p| !crate::builder::open::same_file(p, &l.dir)) {
                    return Err(format!("lessons from {} are open in this window; leave lessons before opening {}", l.dir.display(), p.display()));
                }
                return Ok(Prepared::Now(Box::new(Arrival { unpark_builder: !in_family, document: json!(l.dir), ..Default::default() })));
            }
            let (dir, slug) = match path {
                Some(p) => (p, None),
                None => docs.lessons.clone().ok_or("lessons mode needs a lessons folder (<slug>/lesson.md entries) and this window has none open: give one, e.g. viewer_mode {\"mode\":\"lessons\",\"path\":\"lessons\"}")?,
            };
            if crate::launch::classify(&dir) != Ok(crate::launch::LaunchKind::Lessons) {
                return Err(format!("{}: not a lessons folder (no <slug>/lesson.md entries)", dir.display()));
            }
            let library = docs.library.clone()?;
            let what = dir.display().to_string();
            let job = Job::spawn(Pool::Compute, 0, format!("{what}: the lessons loader"), move |_| {
                let (learn, builder, scene, warning) = crate::lesson::open_lessons(dir.clone(), slug.clone(), library, registry())?;
                Ok(Box::new(Arrival { learn: Some(learn), builder: Some(builder), scene: Some(scene), document: json!({"dir": dir, "warning": warning}), lessons: Some((dir, slug)), ..Default::default() }))
            });
            Ok(Prepared::Load(what, Work::Job(job)))
        }
        ViewerMode::Robot => {
            let document = request.document.clone().or_else(|| docs.robot.clone()).ok_or("robot mode needs a robot and this window has none open: give path (a *.simrobot.json) or preset (an id listed by robot_presets), e.g. viewer_mode {\"mode\":\"robot\",\"preset\":\"robot-measured-400hz\"}")?;
            let view = match &document {
                Document::Path(p) => {
                    if !p.to_string_lossy().ends_with(".simrobot.json") {
                        return Err(format!("{}: robot mode opens a *.simrobot.json file", p.display()));
                    }
                    if !p.is_file() {
                        return Err(format!("{}: no such file", p.display()));
                    }
                    RobotView::open(p.clone()).with_presets(docs.presets.clone())
                }
                Document::Preset(id) => {
                    let presets = docs.presets.clone().map(Ok).unwrap_or_else(crate::robot::preset::default_file)?;
                    RobotView::open_preset(&presets, id)?
                }
                Document::Url(url) => return Err(format!("url `{url}` opens cad mode only (a running RoboCAD service)")),
            };
            Ok(Prepared::Load(document.describe(), Work::Robot(Box::new(view))))
        }
        ViewerMode::Place => {
            let dir = path.or_else(|| docs.place.clone()).ok_or("place mode needs a scanned place (a sim-place build directory holding place.json) and this window has none open: give one, e.g. viewer_mode {\"mode\":\"place\",\"path\":\"…/place\"}")?;
            let what = dir.display().to_string();
            let job = Job::spawn(Pool::Compute, 0, format!("{what}: the place loader"), move |_| {
                let place = PlaceView::open(dir.clone())?;
                Ok(Box::new(Arrival { place: Some(place), document: json!(dir), ..Default::default() }))
            });
            Ok(Prepared::Load(what, Work::Job(job)))
        }
        // Nothing to load first: connecting (or starting a self-started
        // service, up to `service::START_TIMEOUT` for a large document) is
        // CAD mode's own job, shown in its header and `cad_state.connection`,
        // so the switch never waits on RoboCAD.
        ViewerMode::Cad => {
            let target = match (&request.document, path) {
                (Some(Document::Url(url)), _) => {
                    sim_runtime::cad_client::CadClient::new(url).map_err(|e| e.to_string())?;
                    CadTarget::Service(url.clone())
                }
                (_, Some(p)) => {
                    if !p.to_string_lossy().ends_with(".rcad") {
                        return Err(format!("{}: cad mode opens a *.rcad file (or url, a running RoboCAD)", p.display()));
                    }
                    // Known cost: one stat on the UI thread, as the other
                    // modes' paths get here; RoboCAD's service reads the file.
                    if !p.is_file() {
                        return Err(format!("{}: no such file", p.display()));
                    }
                    CadTarget::File(p)
                }
                _ => docs.cad.clone().unwrap_or_else(|| CadTarget::Service(sim_runtime::cad_client::DEFAULT_URL.into())),
            };
            Ok(Prepared::Now(Box::new(Arrival { document: target.json(), cad: Some(CadDocument::new(target)), ..Default::default() })))
        }
        // Nothing to load: the exhibits are compiled in. The run thread
        // builds them on OnEnter (`phenomena::enter`), off the UI thread,
        // opening `Documents::exhibit`.
        ViewerMode::Phenomena => match &request.document {
            None => Ok(Prepared::Now(Box::new(Arrival { document: json!({"exhibit": docs.exhibit}), ..Default::default() }))),
            Some(d) => Err(format!("phenomena mode takes no document ({} given); it opens the built-in exhibits: switch with viewer_mode {{\"mode\":\"phenomena\"}}, then phenomena_select", d.describe())),
        },
    }
}
