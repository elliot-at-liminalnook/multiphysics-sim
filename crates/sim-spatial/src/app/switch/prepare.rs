//! What a switch needs before it can happen: the blockers that refuse
//! leaving the current mode, and the target mode's document (already in the
//! window, or a load off the UI thread).
use super::sources::{cad_target, exhibit_of, inspect_paths, lessons_of, path_of, source_document};
use super::{Arrival, Document, Documents, ModeSwitch, Work};
use crate::app::ViewerMode;
use crate::builder::Builder;
use crate::cad::{CadDocument, CadTarget};
use crate::document::DocumentRegistry;
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
pub(super) fn leaving_blockers(world: &mut World, current: ViewerMode, target: ViewerMode) -> Vec<String> {
    let mut blockers = Vec::new();
    // A new lessons folder brings its own builder in place of this one.
    let replacing = target == ViewerMode::Lessons && !world.contains_resource::<Learn>();
    if current.builder_family() && (current != target || replacing) {
        if let Some(reason) = world.get_resource::<crate::builder::calibration::study::forms::StudyUi>()
            .and_then(|ui| ui.blocking_reason()) {
            blockers.push(reason);
        }
        if let Some(studies) = world.get_resource::<crate::builder::calibration::study::StudyOwner>() {
            if let Some(reason) = studies.blocking_reason() {
                blockers.push(reason);
            }
        }
    }
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
        if let Some(mut experiments) = world.get_resource_mut::<crate::cad::experiments::ExperimentsState>() {
            experiments.request_cancel();
            blockers.extend(experiments.mode_blockers());
        }
        if let Some(mut review) = world.get_resource_mut::<crate::cad::experiment_review::ReviewState>() {
            review.request_cancel();
        }
        if let Some(mut motion) = world.get_resource_mut::<crate::cad::motion::MotionState>() {
            motion.request_cancel();
            blockers.extend(motion.mode_blockers());
        }
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

/// What a mode opens, as a person names it.
pub(super) fn wants(mode: ViewerMode) -> &'static str {
    match mode {
        ViewerMode::Build => "a system file (*.system.json)",
        ViewerMode::Lessons => "a lessons folder (<slug>/lesson.md entries)",
        ViewerMode::Robot => "a robot",
        ViewerMode::Place => "a scanned place (a folder holding place.json)",
        ViewerMode::Cad => "a local .rcad archive",
        ViewerMode::Inspect => "an assembly",
        ViewerMode::Phenomena => "no document",
    }
}

/// What the document picker offers for `mode` (its sections and the path field).
fn offers(mode: ViewerMode) -> &'static str {
    match mode {
        ViewerMode::Robot => "presets, recent and example files, or Open file…",
        ViewerMode::Lessons => "recent and example lesson folders, or Open file…",
        ViewerMode::Place => "recent and example places, or Open file…",
        ViewerMode::Cad => "local recent and example .rcad archives, or Open file…",
        _ => "recent and example files, or Open file…",
    }
}

/// The refusal of a switch to `mode` with no document and nothing to
/// reopen. Only a non-interactive request gets it (the window opens the
/// document picker instead, `switch::start`), so it names the window's way.
pub(super) fn needs(mode: ViewerMode) -> String {
    format!("{} mode needs {} and this window has none open: press {} in the mode switcher and choose one in its picker ({})", mode.name(), wants(mode), mode.label(), offers(mode))
}

/// Why a switch to `target` with no document would be refused: the mode
/// has no document in the window and nothing to reopen (Build: no builder;
/// Lessons: no lesson open or remembered; Robot, Place, CAD: none
/// remembered in the document registry). None for Inspect (it falls back
/// to the example assembly) and Phenomena (its exhibits are compiled in).
/// Pure: reads the world only.
pub(super) fn missing_document(world: &World, target: ViewerMode) -> Option<String> {
    let registry = world.resource::<DocumentRegistry>();
    let missing = match target {
        ViewerMode::Build => !world.contains_resource::<Builder>(),
        ViewerMode::Lessons => !world.contains_resource::<Learn>() && registry.source(target).and_then(lessons_of).is_none(),
        ViewerMode::Robot => registry.source(target).and_then(source_document).is_none(),
        ViewerMode::Place => registry.source(target).and_then(path_of).is_none(),
        ViewerMode::Cad => registry.source(target).and_then(cad_target).is_none(),
        ViewerMode::Inspect | ViewerMode::Phenomena => false,
    };
    missing.then(|| format!("{} mode needs {}; this window has none open yet.", target.label(), wants(target)))
}

/// The parts registry the launch uses (`$SIM_PARTS_DIR`, else the workspace's).
fn registry() -> sim_core::BehaviorRegistry {
    sim_runtime::system_registry_in(crate::workspace::get().as_ref())
}

/// The target mode's document: what the window already has (or parked), a
/// load, or what the document registry remembers for it.
pub(super) fn prepare(world: &World, current: ViewerMode, request: &ModeSwitch) -> Result<Prepared, String> {
    let docs = world.resource::<Documents>();
    let registered = world.resource::<DocumentRegistry>();
    let remembered = registered.source(request.mode);
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
            let assembly = remembered.and_then(inspect_paths);
            if path.is_none() && registered.is_parked(ViewerMode::Inspect) {
                return Ok(Prepared::Now(Box::new(Arrival { unpark: Some(ViewerMode::Inspect), document: json!(assembly.as_ref().map(|(d, _)| d)), ..Default::default() })));
            }
            let (description, spatial) = match &path {
                Some(p) => crate::inspect_pair(p)?,
                None => assembly.unwrap_or_else(crate::default_inspect_paths),
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
                            return Err(format!("this window's builder has {} open: switch to Build mode, then open {} from the Systems tab's Open, which keeps a live run", b.path().display(), p.display()));
                        }
                        // A lesson's sandbox builder (no Open) is replaced by the file below.
                    }
                    _ => return Ok(Prepared::Now(Box::new(Arrival { unpark: (!in_family).then_some(ViewerMode::Build), document: json!(b.path()), ..Default::default() }))),
                }
            }
            let path = path.ok_or_else(|| needs(ViewerMode::Build))?;
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
                return Ok(Prepared::Now(Box::new(Arrival { unpark: (!in_family).then_some(ViewerMode::Build), document: json!(l.dir), ..Default::default() })));
            }
            let (dir, slug) = match path {
                Some(p) => (p, None),
                None => remembered.and_then(lessons_of).ok_or_else(|| needs(ViewerMode::Lessons))?,
            };
            if crate::launch::classify(&dir) != Ok(crate::launch::LaunchKind::Lessons) {
                return Err(format!("{}: not a lessons folder (no <slug>/lesson.md entries)", dir.display()));
            }
            let library = docs.library.clone()?;
            let what = dir.display().to_string();
            let job = Job::spawn(Pool::Compute, 0, format!("{what}: the lessons loader"), move |_| {
                let (learn, builder, scene, warning) = crate::lesson::open_lessons(dir.clone(), slug, library, registry())?;
                Ok(Box::new(Arrival { learn: Some(learn), builder: Some(builder), scene: Some(scene), document: json!({"dir": dir, "warning": warning}), ..Default::default() }))
            });
            Ok(Prepared::Load(what, Work::Job(job)))
        }
        ViewerMode::Robot => {
            let document = request.document.clone().or_else(|| remembered.and_then(source_document)).ok_or_else(|| needs(ViewerMode::Robot))?;
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
            let dir = path.or_else(|| remembered.and_then(path_of)).ok_or_else(|| needs(ViewerMode::Place))?;
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
                    return Err(format!("{url}: CAD service attachment awaiting Rust migration; open a local .rcad archive"));
                }
                (_, Some(p)) => {
                    if !p.to_string_lossy().ends_with(".rcad") {
                        return Err(format!("{}: cad mode opens a local *.rcad archive", p.display()));
                    }
                    // Known cost: one stat on the UI thread, as the other
                    // modes' paths get here; RoboCAD's service reads the file.
                    if !p.is_file() {
                        return Err(format!("{}: no such file", p.display()));
                    }
                    CadTarget::File(p)
                }
                _ => remembered.and_then(cad_target).ok_or_else(|| needs(ViewerMode::Cad))?,
            };
            Ok(Prepared::Now(Box::new(Arrival { document: target.json(), cad: Some(CadDocument::new(target)), ..Default::default() })))
        }
        // Nothing to load: the exhibits are compiled in. The run thread
        // builds them on OnEnter (`phenomena::enter`), off the UI thread,
        // opening the exhibit its registry entry names (`arrive` reopens it).
        ViewerMode::Phenomena => match &request.document {
            None => Ok(Prepared::Now(Box::new(Arrival { document: json!({"exhibit": exhibit_of(registered)}), ..Default::default() }))),
            Some(d) => Err(format!("phenomena mode takes no document ({} given): it opens the built-in exhibits; choose an exhibit in its gallery", d.describe())),
        },
    }
}
