//! Results without a window: the live link's Robot request through the
//! switch and the document registry, the export queue, the one stress
//! colouring rule shared with Robot mode, and the forms' default paths.
use super::export::{self, ExportRequest, Running, Written};
use super::forms::{FormKind, default_path};
use super::link::{self, LiveLink};
use super::overlay::{Inputs, cad_colours};
use super::{ExportKind, ResultsArgs, ResultsOp, command_action, model_path, specs};
use crate::app::ViewerMode;
use crate::app::actions::Act;
use crate::app::switch::{Document, WindowAction};
use crate::cad::actions::CadAction;
use crate::cad::document::{CadDocument, CadTarget};
use crate::document::{DocumentRegistry, Source};
use bevy::ecs::message::Messages;
use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use serde_json::json;
use sim_domain_robot::stress_results::{Hotspot, link_colours, stress_colour};
use std::path::{Path, PathBuf};
use std::time::Instant;

const RCAD: &str = "/work/arm/robot.rcad";

fn request(link: bool, label: &str) -> ExportRequest {
    ExportRequest { path: PathBuf::from(format!("/work/arm/{label}.simrobot.json")), flex: !link, planar: true, label: label.into(), link }
}

/// A finished export of `request` (as `export::start` would leave it once RoboCAD answered).
fn landed(request: ExportRequest, generation: u64) -> Running {
    Running { request, job: crate::jobs::Job::finished(generation, Ok(Written { links: 4, flexible: 0 })), started: Instant::now(), shown: 0 }
}

/// The live link's export, once written: the first after toggling on asks
/// the mode switch for Robot mode on the model (through the window action
/// the switcher handles); later ones make the registry's Robot entry follow
/// it, a reload of the same file bumping its revision.
#[test]
fn the_live_link_requests_the_robot_reload_through_the_switch_and_registry() {
    let mut world = World::default();
    world.init_resource::<Messages<Act<WindowAction>>>();
    world.init_resource::<Messages<Act<CadAction>>>();
    world.insert_resource(DocumentRegistry::default());
    world.insert_resource(LiveLink::default());
    let mut doc = CadDocument::new(CadTarget::File(PathBuf::from(RCAD)));
    let model = model_path(Path::new(RCAD));
    assert_eq!(model, PathBuf::from("/work/arm/robot.simrobot.json"));
    // Toggled on (`link::toggle` sets these, then starts the export).
    doc.results.link = Some(PathBuf::from(RCAD));
    doc.results.link_switch = true;
    doc.results.link_seeded = true;
    let first = ExportRequest { path: model.clone(), flex: false, planar: true, label: "live simulation model".into(), link: true };
    doc.results.exports.running = Some(landed(first.clone(), doc.generation));
    world.insert_resource(doc);
    world.run_system_once(link::receive).unwrap();
    // The link outlives the document.
    assert_eq!(world.resource::<LiveLink>().watching.as_deref(), Some(Path::new(RCAD)));
    let sent: Vec<Act<WindowAction>> = world.resource_mut::<Messages<Act<WindowAction>>>().drain().collect();
    assert_eq!(sent.len(), 1, "one switch request");
    let WindowAction::Switch(switch) = &sent[0].action else { panic!("not a switch") };
    assert_eq!(switch.mode, ViewerMode::Robot);
    assert_eq!(switch.document, Some(Document::Path(model.clone())));
    {
        let doc = world.resource::<CadDocument>();
        assert!(doc.results.exports.running.is_none() && doc.results.switch_to.is_none() && !doc.results.link_switch);
        assert_eq!(doc.results.exports.written.as_deref(), Some(model.as_path()));
    }
    // Robot mode opened it (the switch's arrival opens the registry entry).
    let id = world.resource_mut::<DocumentRegistry>().open(ViewerMode::Robot, crate::document::DocumentKind::Robot, Source::path(&model)).id;
    world.resource_mut::<DocumentRegistry>().close(ViewerMode::Robot);
    let before = world.resource::<DocumentRegistry>().revision(id).unwrap();
    // A save's export lands: no switch, the same entry one revision on.
    let generation = world.resource::<CadDocument>().generation;
    world.resource_mut::<CadDocument>().results.exports.running = Some(landed(first.clone(), generation));
    world.run_system_once(link::receive).unwrap();
    assert!(world.resource_mut::<Messages<Act<WindowAction>>>().drain().next().is_none(), "later saves do not leave CAD mode");
    let registry = world.resource::<DocumentRegistry>();
    let entry = registry.entry(ViewerMode::Robot).unwrap();
    assert_eq!((entry.id, entry.revision), (id, before + 1));
    assert_eq!(entry.source, Source::path(&model));
    // Another document in Robot mode is replaced by the link's model (remembered).
    let mut registry = DocumentRegistry::default();
    registry.open(ViewerMode::Robot, crate::document::DocumentKind::Robot, Source::Preset { id: "biped".into() });
    link::follow(&mut registry, None, &model);
    let entry = registry.entry(ViewerMode::Robot).unwrap();
    assert_eq!((entry.source.clone(), entry.presence), (Source::path(&model), crate::document::Presence::Remembered));
    // A link turned off meanwhile: the written model changes nothing.
    let mut doc = CadDocument::new(CadTarget::File(PathBuf::from(RCAD)));
    doc.results.link_switch = true;
    let landing = export::Landed { path: model.clone(), link: true };
    let mut registry = DocumentRegistry::default();
    link::after_write(&mut doc, &mut registry, None, &landing);
    assert!(doc.results.switch_to.is_none() && registry.entry(ViewerMode::Robot).is_none());
}

/// The live link's first switch goes through the switch's own refusal:
/// while another export runs (one started from the queue) or is queued,
/// `CadDocument::switch_blockers` names it, so the switch is not sent, the
/// request is kept and the status line says why; "Show in Robot mode" is
/// refused too. Once nothing blocks, the next written link model switches.
#[test]
fn the_live_links_first_switch_is_kept_when_the_switch_would_be_refused() {
    let mut doc = CadDocument::new(CadTarget::File(PathBuf::from(RCAD)));
    let model = model_path(Path::new(RCAD));
    doc.results.link = Some(PathBuf::from(RCAD));
    doc.results.link_switch = true;
    doc.results.link_seeded = true;
    let other = request(false, "physical model");
    doc.results.exports.running = Some(landed(other.clone(), doc.generation));
    let blockers = doc.switch_blockers();
    assert!(blockers.iter().any(|b| b.starts_with("a model export is running: physical model to ") && b.ends_with("; wait or cancel it")), "{blockers:?}");
    assert!(blockers.iter().all(|b| !b.contains("cad_")), "no REST wording: {blockers:?}");
    assert!(link::switch_refusal(&doc).is_some());
    let show = link::show(&mut doc).unwrap_err();
    assert!(show.contains("a model export is running") && show.contains("wait for it or cancel it"), "{show}");
    // The link's model lands meanwhile: no switch, the request kept, the refusal shown.
    let mut registry = DocumentRegistry::default();
    let landing = export::Landed { path: model.clone(), link: true };
    link::after_write(&mut doc, &mut registry, None, &landing);
    assert!(doc.results.switch_to.is_none() && doc.results.link_switch, "the request is kept");
    let Some(Err(status)) = &doc.status else { panic!("the refusal is on the status line: {:?}", doc.status) };
    assert!(status.contains("Robot mode was not opened") && status.contains("a model export is running"), "{status}");
    assert_eq!(registry.entry(ViewerMode::Robot).map(|e| e.source.clone()), Some(Source::path(&model)), "the Robot entry follows meanwhile");
    // A queued export blocks as well.
    doc.results.exports.queued = Some(request(true, "live simulation model"));
    assert!(doc.switch_blockers().iter().any(|b| b.starts_with("a model export is queued: live simulation model to ")));
    // Nothing blocks any more: the next written link model switches.
    doc.results.exports.running = None;
    doc.results.exports.queued = None;
    assert!(doc.switch_blockers().is_empty() && link::switch_refusal(&doc).is_none());
    link::after_write(&mut doc, &mut registry, None, &landing);
    assert_eq!(doc.results.switch_to.as_deref(), Some(model.as_path()));
    assert!(!doc.results.link_switch);
}

/// A save settles on what was recorded when it started: the link's
/// `.rcad` then (a Save As moves it with the document), and only as the
/// same edit of the same connection.
#[test]
fn a_save_settles_on_the_link_recorded_when_it_started() {
    let moved = "/work/arm/renamed.rcad";
    let mut doc = CadDocument::new(CadTarget::File(PathBuf::from(RCAD)));
    doc.results.link = Some(PathBuf::from(RCAD));
    doc.edit_seq = 1;
    super::note_save(&mut doc, Some(moved));
    assert!(matches!(&doc.results.waiting, Some(super::Noted { seq: 1, what: super::Waiting::Save { linked: Some(l), .. }, .. }) if l == Path::new(RCAD)));
    // RoboCAD's path moved before the save settled: the link still follows.
    doc.target = CadTarget::File(PathBuf::from(moved));
    doc.status = Some(Ok("Saved".into()));
    super::settle(&mut doc);
    assert!(doc.results.waiting.is_none());
    assert_eq!(doc.results.link.as_deref(), Some(Path::new(moved)));
    // An answer from an older connection (generation changed): not a success.
    let mut doc = CadDocument::new(CadTarget::File(PathBuf::from(RCAD)));
    doc.results.link = Some(PathBuf::from(RCAD));
    doc.edit_seq = 1;
    super::note_save(&mut doc, Some(moved));
    doc.generation += 1;
    doc.status = Some(Ok("an older line".into()));
    super::settle(&mut doc);
    assert!(doc.results.waiting.is_none());
    assert_eq!(doc.results.link.as_deref(), Some(Path::new(RCAD)), "nothing re-exported or moved");
}

/// One export at a time: a second is refused with RoboCAD's text; the
/// live link's queues behind it, the latest wins.
#[test]
fn the_export_queue_refuses_a_second_and_keeps_the_latest_link_request() {
    let mut exports = export::Exports::default();
    let first = request(false, "physical model");
    assert_eq!(export::admit(&mut exports, first.clone()), Ok(Some(first.clone())));
    exports.running = Some(landed(first, 1));
    assert_eq!(export::admit(&mut exports, request(false, "simulation model")), Err(export::RUNNING.to_string()));
    assert!(exports.queued.is_none());
    assert_eq!(export::admit(&mut exports, request(true, "save one")), Ok(None));
    assert_eq!(export::admit(&mut exports, request(true, "save two")), Ok(None));
    assert_eq!(exports.queued.as_ref().map(|q| q.label.as_str()), Some("save two"));
    // A typed path without .json gets RoboCAD's filter's extension; never the .rcad.
    assert_eq!(export::model_file("/w/robot.rcad"), PathBuf::from("/w/robot.rcad.simrobot.json"));
    assert_eq!(export::model_file("/w/robot.simrobot.json"), PathBuf::from("/w/robot.simrobot.json"));
}

/// The written model is the whole answer, through a temporary file and a
/// rename; the summary counts links and flexible ones as RoboCAD's worker does.
#[test]
fn an_export_writes_atomically() {
    let dir = std::env::temp_dir().join(format!("sim-spatial-results-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("robot.simrobot.json");
    let model = json!({"version": 4, "links": [{"name": "a", "flex": {"modes": 2}}, {"name": "b", "flex": null}, {"name": "c"}]});
    assert_eq!(export::write_model(&path, &model, &|| false), Ok(Written { links: 3, flexible: 1 }));
    let back: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(back, model);
    let left: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(Result::ok).map(|e| e.file_name()).collect();
    assert_eq!(left.len(), 1, "no temporary file left: {left:?}");
    assert!(export::write_model(&path, &json!({"error": "x"}), &|| false).is_err());
    // Cancelled just before the rename: the model stays as it was and no temporary file is left.
    assert_eq!(export::write_model(&path, &json!({"links": []}), &|| true), Err("cancelled".to_string()));
    let back: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(back, model);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "the cancelled write's temporary file is deleted");
    let _ = std::fs::remove_dir_all(&dir);
}

/// CAD's overlay colours through Robot mode's function: a vertex (mm,
/// RoboCAD's frame) moved into the link frame (m, about the block's com)
/// gets exactly `stress_colour` of the nearest cell, and the shared
/// `link_colours` is `stress_colour` per position.
#[test]
fn the_cad_overlay_and_robot_mode_share_one_colour_rule() {
    let hotspot = Hotspot { cells: vec![[0.010, 0.0, 0.0], [0.0, 0.050, 0.0]], stress_pa: vec![2.5e6, 1.0e3] };
    let p = [0.011, 0.001, 0.0];
    assert_eq!(link_colours(&hotspot, 3.0e7, [p]), vec![stress_colour(&hotspot, 3.0e7, p)]);
    let com = [0.1, 0.2, 0.3];
    let inputs = Inputs { hotspot: hotspot.clone(), yield_pa: Some(3.0e7), com_m: Some(com) };
    // (0.111, 0.201, 0.3) m in the world is (0.011, 0.001, 0) in the link frame.
    let vertex_mm = [111.0f32, 201.0, 300.0];
    let got = cad_colours(&inputs, com, &[vertex_mm]);
    let expected = stress_colour(&hotspot, 3.0e7, [f64::from(111.0f32) * 1e-3 - 0.1, f64::from(201.0f32) * 1e-3 - 0.2, f64::from(300.0f32) * 1e-3 - 0.3]);
    assert_eq!(got, vec![expected]);
    // The nearest cell (2.5 MPa of 30 MPa) decides it.
    assert_eq!(expected, stress_colour(&Hotspot { cells: vec![[0.010, 0.0, 0.0]], stress_pa: vec![2.5e6] }, 3.0e7, [0.011, 0.001, 0.0]));
    // Without a material yield: RoboCAD's fallback, the largest stress (≥ 1 Pa).
    let no_yield = Inputs { yield_pa: None, ..inputs };
    assert_eq!(cad_colours(&no_yield, com, &[vertex_mm]), vec![stress_colour(&hotspot, 2.5e6, [f64::from(111.0f32) * 1e-3 - 0.1, f64::from(201.0f32) * 1e-3 - 0.2, f64::from(300.0f32) * 1e-3 - 0.3])]);
    // A node's block parses as the file's link block does.
    let block = json!({"cells": [[0.010, 0.0, 0.0], [0.0, 0.050, 0.0]], "stress_pa": [2.5e6, 1.0e3]});
    assert_eq!(Hotspot::from_block(&block), Some(hotspot.clone()));
    assert_eq!(Hotspot::from_results(&json!({"links": {"arm": {"hotspot": block}}}), "arm"), Some(hotspot));
    // A NaN stress (written as null) drops its cell, not the next cell's stress.
    let holed = json!({"cells": [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]], "stress_pa": [5.0, null, 7.0]});
    assert_eq!(Hotspot::from_block(&holed), Some(Hotspot { cells: vec![[0.0, 0.0, 0.0], [2.0, 0.0, 0.0]], stress_pa: vec![5.0, 7.0] }));
}

/// RoboCAD's load dialog starts at `<stem>.simresult.json` beside the
/// document; identification at its folder; an export at `<stem>.simrobot.json`.
#[test]
fn the_forms_start_where_robocad_does() {
    let doc = Path::new("/work/arm/robot.rcad");
    assert_eq!(default_path(FormKind::Load, Some(doc), "/home/u"), "/work/arm/robot.simresult.json");
    assert_eq!(default_path(FormKind::Identify, Some(doc), "/home/u"), "/work/arm/");
    assert_eq!(default_path(FormKind::Export(ExportKind::Simulation), Some(doc), "/home/u"), "/work/arm/robot.simrobot.json");
    assert_eq!(default_path(FormKind::Load, None, "/home/u/"), "/home/u/");
    assert_eq!(default_path(FormKind::Export(ExportKind::Physical), None, "/home/u"), "/home/u/untitled.simrobot.json");
}

/// Every RoboCAD command this part runs maps to an action, the dialog ones
/// opening the form; the spec's example parses as the command.
#[test]
fn commands_and_the_spec_map_to_actions() {
    for id in ["robot.load_results", "robot.apply_identification", "view.stress", "print.overlay", "sim.export_physical", "sim.export", "sim.link"] {
        assert!(command_action(id).is_some(), "{id}");
    }
    assert_eq!(command_action("sim.export"), Some(CadAction::CadResults(ResultsArgs { op: ResultsOp::Export, kind: Some(ExportKind::Simulation), ..Default::default() })));
    for s in specs() {
        let mut v = s.example.clone();
        v["command"] = json!(s.name);
        let action: CadAction = serde_json::from_value(v).unwrap_or_else(|e| panic!("{}: {e}", s.name));
        assert!(matches!(action, CadAction::CadResults(ResultsArgs { op: ResultsOp::Load, .. })));
    }
}
