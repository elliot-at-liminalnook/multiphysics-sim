//! The command surfaces without a window: RoboCAD's command table against
//! the op catalogue, the menus, readiness, the key parser, and every
//! surfaces control round-tripping through its REST form.
use super::registry::{self, CATEGORIES, COMMANDS, CONTEXT, MAKE_UNIQUE, Native, Resolved, SELECT_RADIAL, SKETCH_CONTEXT, TOOLBAR, VIEW_RADIAL};
use super::{Surface, entries};
use crate::app::actions::{self, Action};
use crate::cad::actions::CadAction;
use crate::cad::rest_form::rest_form;
use crate::cad::document::{CadDocument, CadTarget, Connection, Edit, EditDone};
use crate::cad::keys::{Binding, parse};
use crate::cad::ops::{CATALOGUE, Flow, FormState, Needs, OpEntry};
use crate::cad::panel::own_controls;
use crate::ui_kit::form::FieldKind;
use crate::ui_kit::palette::conflicts;
use serde_json::{Value, json};
use sim_runtime::cad_client::{DocState, Health, History, NodeSummary, SelectionItem};
use std::collections::HashSet;

fn node(id: &str, kind: &str, name: &str) -> NodeSummary {
    NodeSummary { id: id.into(), kind: kind.into(), name: name.into(), visible: true, effective_visible: true, ..Default::default() }
}

/// A connected headless document with two bodies and an instance, nothing selected.
fn document() -> CadDocument {
    let mut doc = CadDocument::new(CadTarget::Service("http://127.0.0.1:8420".into()));
    doc.client = Some(sim_runtime::cad_client::CadClient::new("http://127.0.0.1:8420").unwrap());
    doc.connection = Connection::Connected;
    doc.health = Some(Health { ok: true, app: "robocad".into(), gui: false, nodes: 3, revision: 4, ..Default::default() });
    doc.doc = Some(DocState { nodes: vec![node("b1", "body", "Bracket"), node("b2", "body", "Plate"), node("i1", "instance", "Bracket copy")], history: History { undo: vec!["Move".into()], redo: vec![] }, revision: 4, ..Default::default() });
    doc
}

/// The catalogue's entries that are RoboCAD commands (not REST-only `ops.<name>`).
fn commands_in_catalogue() -> impl Iterator<Item = &'static OpEntry> {
    CATALOGUE.iter().filter(|e| !e.id.starts_with("ops."))
}

#[test]
fn the_table_is_robocads_registry() {
    assert_eq!(COMMANDS.len(), 183, "app.py:270-431 registers 183 commands");
    let mut ids = HashSet::new();
    for c in COMMANDS {
        assert!(ids.insert(c.id), "{} is listed twice", c.id);
        assert!(!c.label.is_empty() && !c.category.is_empty(), "{}", c.id);
    }
    // Loops expanded, tr() labels resolved, RoboCAD's own label quirks kept.
    let label = |id: &str| registry::command(id).unwrap_or_else(|| panic!("{id}")).label;
    assert_eq!(label("view.front"), "View front");
    assert_eq!(label("view.mode.shaded_edges"), "Display: shaded edges");
    assert_eq!(label("select.vertex"), "Select vertexs");
    assert_eq!(label("sketch.rectangle_center"), "Sketch: Rectangle (centre)");
    assert_eq!(label("view.fit"), "Fit All");
    assert_eq!(label("edit.copy"), "Copy with Placement");
    // keymap.json replaces inline keys and binds them; inline-only keys are listed, not bound.
    let cmd = |id: &str| registry::command(id).unwrap();
    assert_eq!(cmd("tool.annotate").keys, &["N"]);
    assert!(cmd("tool.annotate").bound);
    assert_eq!(cmd("robot.add_motor").keys, &["Ctrl+Shift+M"]);
    assert!(!cmd("robot.add_motor").bound && !cmd("robot.add_joint").bound && !cmd("simulation.experiment").bound);
    assert_eq!(cmd("command_palette").keys, &["Ctrl+Space", "Shift+F"]);
    assert_eq!(cmd("edit.delete").keys, &["Delete", "Backspace"]);
    assert!(registry::command("sketch.arc").is_none(), "keymap.json's sketch.arc names no command");
    // Its A is bound to the three-point arc instead, deliberately (the module doc).
    assert_eq!(cmd("sketch.arc_3pt").keys, &["A"]);
    assert!(cmd("sketch.arc_3pt").bound);
    assert_eq!(parse("A"), Ok(Binding::One(crate::cad::keys::Combo { ctrl: false, shift: false, alt: false, key: bevy::prelude::KeyCode::KeyA })));
}

#[test]
fn every_surface_names_registry_commands() {
    for id in TOOLBAR.iter().chain(CONTEXT.iter()).chain([MAKE_UNIQUE.0].iter()).chain(VIEW_RADIAL.iter().map(|(_, id)| id)).chain(SELECT_RADIAL.iter().map(|(_, id)| id)) {
        assert!(registry::command(id).is_some(), "{id} is not a RoboCAD command");
    }
    for c in COMMANDS {
        assert!(CATEGORIES.contains(&registry::menu_of(c.category)), "{}", c.id);
    }
}

#[test]
fn every_catalogue_command_agrees_with_the_registry() {
    for e in commands_in_catalogue() {
        let c = registry::command(e.id).unwrap_or_else(|| panic!("catalogue entry {} is not a RoboCAD command", e.id));
        assert_eq!(c.label, e.label, "{}: label", e.id);
        assert_eq!(c.category, e.category, "{}: category", e.id);
        assert_eq!(c.keys, e.keys, "{}: keys", e.id);
        assert_eq!(c.native, Native::Op, "{}: the registry maps it elsewhere", e.id);
        assert!(matches!(registry::resolve(c), Resolved::Op(_)), "{}", e.id);
    }
    // A cad-modify command the catalogue does not list says so by name.
    for c in COMMANDS.iter().filter(|c| c.native == Native::Op && crate::cad::ops::entry(c.id).is_none()) {
        assert_eq!(registry::resolve(c), Resolved::Later("cad-modify"), "{}", c.id);
    }
}

#[test]
fn menus_put_general_window_and_tools_in_help() {
    for category in ["General", "Window", "Tools", "Help"] {
        assert_eq!(registry::menu_of(category), "Help");
    }
    assert_eq!(registry::menu_of("Modify"), "Modify");
    let doc = document();
    let own = own_controls(&doc);
    let help: Vec<String> = entries(&Surface::Menu { category: "Help".into() }, &doc, &own).into_iter().map(|e| e.id).collect();
    for id in ["command_palette", "components.show", "tool.select", "tool.move", "tool.set_pivot", "numeric.entry", "help.guide", "help.logs"] {
        assert!(help.contains(&id.to_string()), "{id} is not in Help: {help:?}");
    }
    // Registry order within a menu.
    let file: Vec<String> = entries(&Surface::Menu { category: "File".into() }, &doc, &own).into_iter().map(|e| e.id).collect();
    assert_eq!(file.first().map(String::as_str), Some("reference.import"));
    assert_eq!(file.get(1).map(String::as_str), Some("file.new"));
}

#[test]
fn readiness_refuses_with_the_entrys_refusal() {
    let doc = document();
    for e in CATALOGUE {
        let needs_something = match e.needs {
            Needs::Nothing => false,
            Needs::Nodes { min, .. } | Needs::Edges { min, .. } | Needs::Faces { min } => min > 0,
            _ => true,
        };
        let r = registry::readiness(e, &doc);
        // An interaction is started to pick (its refusals come when it runs); viewer state needs nothing.
        let started = matches!(e.flow, Flow::PickThenForm(_) | Flow::Sketch(_) | Flow::Extrude { .. } | Flow::PlanePick(_) | Flow::View(_));
        if started || !needs_something {
            assert!(r.is_ok(), "{}: {r:?}", e.id);
        } else {
            assert_eq!(r, Err(e.refusal.to_string()), "{}", e.id);
        }
    }
    // Later epics and deliberately different commands refuse by name.
    let own = own_controls(&doc);
    let front = registry::command("view.front").unwrap();
    assert_eq!(registry::ready(front, &doc, &own), Ok(()), "a named view runs since cad-views-export");
    let draft = registry::command("inspect.draft").unwrap();
    assert!(registry::ready(draft, &doc, &own).is_err_and(|e| e.starts_with("Draft-angle shading is not ported: ")));
    let guide = registry::command("help.guide").unwrap();
    assert_eq!(registry::ready(guide, &doc, &own), Err("User guide is not ported: RoboCAD shows only a path; the viewer's docs live in the repository".to_string()));
    // An action command is ready as its button: nothing to redo.
    assert!(registry::ready(registry::command("edit.redo").unwrap(), &doc, &own).is_err());
    assert!(registry::ready(registry::command("edit.undo").unwrap(), &doc, &own).is_ok());
}

#[test]
fn keys_parse() {
    let shift_a = crate::cad::keys::Combo { ctrl: false, shift: true, alt: false, key: bevy::prelude::KeyCode::KeyA };
    let b = crate::cad::keys::Combo { ctrl: false, shift: false, alt: false, key: bevy::prelude::KeyCode::KeyB };
    assert_eq!(parse("Shift+A, B"), Ok(Binding::Chord(shift_a, b)));
    assert!(matches!(parse("Ctrl+Alt+U"), Ok(Binding::One(c)) if c.ctrl && c.alt && !c.shift));
    assert!(matches!(parse("Space"), Ok(Binding::One(c)) if c.key == bevy::prelude::KeyCode::Space));
    assert!(parse("Meta+Q").is_err() && parse("Ctrl+Nope").is_err());
    for c in COMMANDS {
        for k in c.keys {
            assert!(parse(k).is_ok(), "{}: {k} does not parse: {:?}", c.id, parse(k));
        }
    }
    for e in CATALOGUE {
        for k in e.keys {
            assert!(parse(k).is_ok(), "{}: {k}", e.id);
        }
    }
    // No two bound commands share a key sequence (the inline Ctrl+Shift+M is unbound).
    let mut seen = std::collections::HashMap::new();
    for c in COMMANDS.iter().filter(|c| c.bound) {
        for k in c.keys {
            if let Some(other) = seen.insert(parse(k).unwrap(), c.id) {
                panic!("{k} is bound to {other} and {}", c.id);
            }
        }
    }
}

#[test]
fn the_palette_shows_robocads_key_conflict() {
    let doc = document();
    let own = own_controls(&doc);
    let list = super::palette::palette_entries(&doc, &own);
    let same = list.iter().position(|e| e.id == "edit.select_same_material").unwrap();
    let motor = list.iter().position(|e| e.id == "robot.add_motor").unwrap();
    assert_eq!(conflicts(&list).get("ctrl+shift+m"), Some(&vec![same, motor]));
    assert_eq!(list[motor].note, "cad-physical-inspect");
    assert_eq!(list.iter().find(|e| e.id == "help.guide").unwrap().note, "not ported");
}

#[test]
fn the_context_menu_offers_make_unique_for_instances() {
    let mut doc = document();
    let ids = |doc: &CadDocument| entries(&Surface::Context { at: None }, doc, &own_controls(doc)).into_iter().map(|e| e.id).collect::<Vec<_>>();
    // RoboCAD's 14 first and unchanged, then the native Sketch section's 13.
    assert_eq!(ids(&doc), CONTEXT.iter().chain(SKETCH_CONTEXT.iter()).map(|s| s.to_string()).collect::<Vec<_>>());
    doc.selection = vec![SelectionItem("i1".into(), "body".into(), 0)];
    let with = ids(&doc);
    assert_eq!(with.len(), CONTEXT.len() + SKETCH_CONTEXT.len() + 1);
    assert_eq!(with.last().map(String::as_str), Some(MAKE_UNIQUE.0));
}

/// The 13 sketch tools are RoboCAD's sketch-shape loop (app.py:377-378), in its order.
#[test]
fn the_context_menus_sketch_section_is_robocads_sketch_tools() {
    let tools: Vec<&str> = COMMANDS.iter().filter(|c| c.category == "Sketch" && !["sketch.offset", "sketch.fillet", "sketch.join"].contains(&c.id)).map(|c| c.id).collect();
    assert_eq!(tools, SKETCH_CONTEXT.to_vec());
    let doc = document();
    let list = entries(&Surface::Context { at: None }, &doc, &own_controls(&doc));
    for (e, id) in list[CONTEXT.len()..].iter().zip(SKETCH_CONTEXT) {
        assert_eq!(e.id, id);
        assert_eq!(e.action, CadAction::CadInvoke { id: id.to_string() });
        assert!(e.closes, "{id}");
    }
}

/// A document with a form open on the first catalogue entry that has a
/// choice and a checkbox (the Array dialog), if any.
fn with_form(mut doc: CadDocument) -> CadDocument {
    let entry = CATALOGUE.iter().find(|e| e.params.iter().any(|p| matches!(p.kind, FieldKind::Choice { .. })) && e.params.iter().any(|p| p.kind == FieldKind::Check));
    if let Some(e) = entry {
        doc.ops.form = Some(FormState { op: e.id, texts: e.params.iter().map(|p| p.default.to_string()).collect(), focus: None, select_all: false, began: 4, error: None });
    }
    doc
}

#[test]
fn every_surfaces_control_round_trips_through_rest() {
    let mut doc = with_form(document());
    doc.selection = vec![SelectionItem("b1".into(), "body".into(), 0)];
    let all = crate::cad::panel::controls(&doc);
    let patterns = <CadAction as Action>::controls();
    let ids: Vec<&str> = all.iter().map(|c| c.id.as_str()).collect();
    for expected in ["cad:op:tool.fillet", "cad:op:view.front", "cad:op:command_palette", "cad:surface:palette", "cad:surface:view_radial", "cad:surface:select_radial", "cad:surface:context", "cad:surface:closed", "cad:menu:File", "cad:menu:Help", "cad:delete"] {
        assert!(ids.contains(&expected), "{expected} is not listed");
    }
    if doc.ops.form.is_some() {
        assert!(ids.contains(&"cad:form:ok") && ids.contains(&"cad:form:cancel"));
        assert!(ids.iter().any(|i| i.starts_with("cad:form:set:")));
    }
    assert_eq!(all.iter().filter(|c| c.id.starts_with("cad:op:")).count(), COMMANDS.len());
    for c in &all {
        assert!(patterns.iter().any(|p| actions::control_matches(p, &c.id)), "{} fits no registered pattern", c.id);
        let Value::Object(mut args) = rest_form(&c.action) else { panic!("{}: not an object", c.id) };
        let name = args.remove("command").and_then(|v| v.as_str().map(str::to_string)).expect("a command name");
        let parsed = <CadAction as Action>::parse(&sim_api::Command { command: name.clone(), args: Value::Object(args) }).unwrap_or_else(|e| panic!("{}: {name} does not parse: {e}", c.id));
        assert_eq!(parsed, c.action, "{}: REST form and click differ", c.id);
    }
    // Delete is RoboCAD's: the whole selection in one step.
    assert_eq!(all.iter().find(|c| c.id == "cad:delete").unwrap().action, CadAction::CadInvoke { id: "edit.delete".into() });
    // A surface's REST form keeps its shape.
    assert_eq!(rest_form(&CadAction::CadSurface { surface: Surface::Menu { category: "Modify".into() } }), json!({"command": "cad_surface", "surface": {"kind": "menu", "category": "Modify"}}));
}

/// A popup stays inside the window: placed at the point asked for when it
/// fits, moved up and left near the edges, its rows capped to the room left.
#[test]
fn popups_are_kept_inside_the_window() {
    use bevy::math::Vec2;
    let window = Vec2::new(1280.0, 720.0);
    // Room below: where asked, rows capped at 550 px.
    assert_eq!(super::popup_place(Vec2::new(400.0, 100.0), window), (400.0, 100.0, 550.0, 872.0));
    // Low in the window: rows fit what is left above the bottom edge.
    let (_, top, rows, _) = super::popup_place(Vec2::new(400.0, 400.0), window);
    assert_eq!((top, rows), (400.0, 302.0));
    // Near the bottom: moved up so 160 px of rows fit.
    let (_, top, rows, _) = super::popup_place(Vec2::new(400.0, 650.0), window);
    assert_eq!((top, rows), (542.0, 160.0));
    // Near the right edge: moved left so its least width fits.
    let (left, _, _, max_width) = super::popup_place(Vec2::new(1250.0, 100.0), window);
    assert_eq!((left, max_width), (1052.0, 220.0));
    // A window shorter than the least rows: pinned at the top edge.
    let (_, top, rows, _) = super::popup_place(Vec2::new(10.0, 50.0), Vec2::new(400.0, 120.0));
    assert_eq!((top, rows), (8.0, 94.0));
}

/// The cad-sketch epic's commands: (id, RoboCAD's label, its keys), in
/// registry order. Every one is a catalogue operation now.
const SKETCH_EPIC: [(&str, &str, &[&str]); 30] = [
    ("tool.extrude", "Extrude", &["X"]),
    ("tool.revolve", "Revolve", &["Shift+R"]),
    ("tool.sweep", "Sweep (profile + path from selection)", &[]),
    ("tool.pipe", "Pipe along selected curve…", &[]),
    ("tool.loft", "Loft selected sketches", &[]),
    ("tool.fill", "Fill / patch selected curve", &[]),
    ("tool.plane", "Plane from face", &["Ctrl+P"]),
    ("tool.plane_three", "Plane from three points", &[]),
    ("tool.plane_camera", "Plane from two points (camera)", &[]),
    ("tool.plane_mid", "Midplane between two faces", &[]),
    ("tool.plane_xy", "Active plane: XY", &[]),
    ("tool.plane_xz", "Active plane: XZ", &[]),
    ("tool.plane_yz", "Active plane: YZ", &[]),
    ("tool.plane_2d_snap", "Toggle 2D snapping to the active plane", &[]),
    ("sketch.line", "Sketch: Line", &["L"]),
    ("sketch.rectangle", "Sketch: Rectangle", &["Shift+L"]),
    ("sketch.rectangle_center", "Sketch: Rectangle (centre)", &[]),
    ("sketch.circle", "Sketch: Circle", &["C"]),
    ("sketch.circle_2pt", "Sketch: Circle (two points)", &[]),
    ("sketch.circle_3pt", "Sketch: Circle (three points)", &[]),
    ("sketch.arc_3pt", "Sketch: Arc (three points)", &["A"]),
    ("sketch.polygon", "Sketch: Polygon", &["Shift+P"]),
    ("sketch.slot", "Sketch: Slot", &["Shift+S"]),
    ("sketch.spline", "Sketch: Spline", &["Shift+C"]),
    ("sketch.ellipse", "Sketch: Ellipse", &[]),
    ("sketch.spiral", "Sketch: Spiral", &[]),
    ("sketch.text", "Sketch: Text", &["T"]),
    ("sketch.offset", "Sketch: offset selected curve…", &[]),
    ("sketch.fillet", "Sketch: fillet corner…", &[]),
    ("sketch.join", "Sketch: join curves", &[]),
];

#[test]
fn every_cad_sketch_command_runs_through_the_catalogue() {
    for (id, label, keys) in SKETCH_EPIC {
        let c = registry::command(id).unwrap_or_else(|| panic!("{id} is not a RoboCAD command"));
        assert_eq!((c.label, c.keys), (label, keys), "{id}: the registry");
        assert_eq!(c.native, Native::Op, "{id}");
        let e = crate::cad::ops::entry(id).unwrap_or_else(|| panic!("{id} is not in the op catalogue"));
        assert_eq!((e.label, e.category, e.keys), (c.label, c.category, c.keys), "{id}: the catalogue");
        assert_eq!(registry::resolve(c), Resolved::Op(e), "{id}");
        assert!(registry::note(c).is_empty(), "{id} still carries a note: {}", registry::note(c));
    }
    // No command is left to the cad-sketch epic.
    for c in COMMANDS {
        assert_ne!(c.native, Native::Later("cad-sketch"), "{}", c.id);
        assert_ne!(registry::resolve(c), Resolved::Later("cad-sketch"), "{}", c.id);
    }
    // Keys bound in RoboCAD's keymap stay bound; A is the native arc binding.
    for (id, _, keys) in SKETCH_EPIC {
        assert_eq!(registry::command(id).unwrap().bound, !keys.is_empty(), "{id}");
    }
}

#[test]
fn the_toolbars_sketch_and_extrude_buttons_are_enabled_operations() {
    let doc = document();
    let own = own_controls(&doc);
    for id in ["sketch.rectangle", "sketch.circle", "sketch.slot", "tool.extrude"] {
        assert!(TOOLBAR.contains(&id), "{id}");
        let c = registry::command(id).unwrap();
        assert!(matches!(registry::resolve(c), Resolved::Op(e) if e.id == id), "{id}");
        assert_eq!(registry::ready(c, &doc, &own), Ok(()), "{id}");
    }
}

/// The Create, Sketch and Planes menus and the palette list the cad-sketch
/// commands in RoboCAD's registry order.
#[test]
fn menus_and_palette_list_the_cad_sketch_commands_in_robocads_order() {
    let doc = document();
    let own = own_controls(&doc);
    let menu = |category: &str| entries(&Surface::Menu { category: category.into() }, &doc, &own).into_iter().map(|e| e.id).collect::<Vec<_>>();
    let epic = |category: &str| SKETCH_EPIC.iter().filter(|(id, ..)| registry::command(id).unwrap().category == category).map(|(id, ..)| id.to_string()).collect::<Vec<_>>();
    let create = menu("Create");
    assert_eq!(create, ["tool.box", "tool.box_center", "tool.cylinder", "tool.sphere", "tool.extrude", "tool.revolve", "tool.sweep", "tool.pipe", "tool.loft", "tool.fill", "components.make"].map(String::from).to_vec());
    assert_eq!(menu("Sketch"), epic("Sketch"));
    assert_eq!(menu("Planes"), epic("Planes"));
    let palette: Vec<String> = super::palette::palette_entries(&doc, &own).into_iter().map(|e| e.id).filter(|id| SKETCH_EPIC.iter().any(|(s, ..)| s == id)).collect();
    assert_eq!(palette, SKETCH_EPIC.map(|(id, ..)| id.to_string()).to_vec());
}

/// `system_ui` lists `cad:op:<id>` for each, writing the menu entry's
/// `CadInvoke`; the active plane (viewer state) is never refused for an
/// edit in flight, the sketch tools are.
#[test]
fn system_ui_reaches_every_cad_sketch_command_as_a_click_does() {
    let mut doc = document();
    let all = crate::cad::panel::controls(&doc);
    for (id, label, _) in SKETCH_EPIC {
        let control = all.iter().find(|c| c.id == format!("cad:op:{id}")).unwrap_or_else(|| panic!("cad:op:{id} is not listed"));
        assert_eq!((control.label.as_str(), &control.action), (label, &CadAction::CadInvoke { id: id.to_string() }), "{id}");
    }
    let own = own_controls(&doc);
    let planes = entries(&Surface::Menu { category: "Planes".into() }, &doc, &own);
    let menu_entry = planes.iter().find(|e| e.id == "tool.plane_xy").unwrap();
    let control = all.iter().find(|c| c.id == "cad:op:tool.plane_xy").unwrap();
    assert_eq!(control.action, menu_entry.action, "activate and a click write the same action");
    assert_eq!(rest_form(&control.action), json!({"command": "cad_invoke", "id": "tool.plane_xy"}));
    doc.edit = Some(Edit { label: "Patch Bracket: visible".into(), job: crate::jobs::Job::finished(0, Ok(EditDone { message: String::new(), result: Value::Null })), started: std::time::Instant::now(), clear_selection: None, activates_plane: false, retarget: None });
    let all = crate::cad::panel::controls(&doc);
    let ready = |id: &str| all.iter().find(|c| c.id == format!("cad:op:{id}")).unwrap().ready.clone();
    if crate::cad::ops::entry("tool.plane_xy").is_some_and(|e| matches!(e.flow, Flow::View(_))) {
        assert_eq!(ready("tool.plane_xy"), Ok(()));
    }
    assert!(ready("sketch.line").is_err_and(|e| e.contains("in flight")), "{:?}", ready("sketch.line"));
}

/// `cad_sketch` is a registered CAD-mode capability whose example parses.
#[test]
fn cad_sketch_is_a_capability_with_a_valid_example() {
    let specs = <CadAction as Action>::commands();
    let spec = specs.iter().find(|s| s.name == "cad_sketch").expect("cad_sketch is registered");
    assert!(spec.modes.contains(&crate::app::ViewerMode::Cad));
    let parsed = <CadAction as Action>::parse(&sim_api::Command { command: "cad_sketch".into(), args: spec.example.clone() }).unwrap_or_else(|e| panic!("the example does not parse: {e}"));
    assert!(matches!(parsed, CadAction::CadSketch { ref calls, .. } if !calls.is_empty()), "{parsed:?}");
    assert!(actions::command_modes("cad_sketch").is_some_and(|m| m.contains(&crate::app::ViewerMode::Cad)));
}

/// cad-views-export's View rows run natively: every View-menu command but
/// the deliberately different ones; the view radial's eight entries all run;
/// camera rows write their camera intent, display rows their display action.
#[test]
fn the_views_export_rows_run_natively() {
    use super::registry::{CameraCmd, DisplayCmd, Do};
    use crate::cad::display::{DisplayArgs, DisplayMode, SectionArgs};
    use crate::cad::views::{ViewsArgs, ViewsOp};
    use crate::camera::{CameraAction, ViewPreset};
    let doc = document();
    let own = own_controls(&doc);
    for c in COMMANDS.iter().filter(|c| c.id.starts_with("view.") || c.id.starts_with("inspect.") || c.id.starts_with("bridge.")) {
        assert_ne!(c.native, Native::Later("cad-views-export"), "{} is still left for later", c.id);
    }
    for (label, id) in VIEW_RADIAL {
        let e = entries(&Surface::ViewRadial { at: None }, &doc, &own).into_iter().find(|e| e.id == id).unwrap();
        assert_eq!(e.ready, Ok(()), "{label} ({id})");
    }
    let resolved = |id: &str| registry::resolve(registry::command(id).unwrap());
    assert_eq!(resolved("view.front"), Resolved::Camera(CameraCmd::Preset(ViewPreset::Front)));
    assert_eq!(CameraCmd::Preset(ViewPreset::Bottom).action(), Some(CameraAction::View { view: ViewPreset::Bottom }));
    assert_eq!(CameraCmd::Ortho.action(), Some(CameraAction::Projection { orthographic: None }));
    assert_eq!(resolved("view.focus"), Resolved::Camera(CameraCmd::Focus));
    assert_eq!(resolved("view.mode_next"), Resolved::Action(CadAction::CadDisplay(DisplayArgs { next: true, ..DisplayArgs::default() })));
    assert_eq!(resolved("inspect.normals"), Resolved::Action(CadAction::CadDisplay(DisplayArgs { mode: Some(DisplayMode::Xray), ..DisplayArgs::default() })), "RoboCAD's normal shading is xray");
    assert_eq!(resolved("view.section"), Resolved::Action(CadAction::CadSection(SectionArgs::default())));
    assert_eq!(resolved("view.saved_views"), Resolved::Action(CadAction::CadViews(ViewsArgs { op: ViewsOp::Panel, ..ViewsArgs::default() })));
    assert_eq!(Do::Display(DisplayCmd::Grid).action(), DisplayCmd::Grid.action());
    for id in ["view.isolate", "view.hide", "view.show_all"] {
        assert!(matches!(resolved(id), Resolved::Op(e) if e.id == id), "{id} is a catalogue operation");
    }
    for id in ["bridge.start", "bridge.stop", "bridge.share"] {
        assert!(matches!(resolved(id), Resolved::Different(why) if why.starts_with("RoboCAD-GUI-only")), "{id}");
    }
    // Isolate and Hide need a selection; Show All does not.
    let ready = |doc: &CadDocument, id: &str| registry::ready(registry::command(id).unwrap(), doc, &own_controls(doc));
    assert!(ready(&doc, "view.isolate").is_err() && ready(&doc, "view.hide").is_err());
    assert_eq!(ready(&doc, "view.show_all"), Ok(()));
    let mut picked = document();
    picked.selection = vec![SelectionItem("b1".into(), "body".into(), 0)];
    assert_eq!((ready(&picked, "view.isolate"), ready(&picked, "view.hide")), (Ok(()), Ok(())));
}

/// The File menu's rows run the files part's actions (no cad-views-export
/// row is left for later), and every `Do::File` id has an action.
#[test]
fn the_file_rows_run_the_files_actions() {
    for c in COMMANDS.iter() {
        assert_ne!(c.native, Native::Later("cad-views-export"), "{} is still left for later", c.id);
        if let Native::Action(super::registry::Do::File(id)) = c.native {
            assert_eq!(id, c.id, "a file row names its own id");
            assert!(crate::cad::files::command_action(id).is_some(), "{id}: files::command_action has no action");
        }
    }
    let resolved = |id: &str| registry::resolve(registry::command(id).unwrap());
    for id in ["file.new", "file.open", "file.save_as", "file.import", "file.export", "file.export_drawing"] {
        assert_eq!(resolved(id), Resolved::Action(crate::cad::files::command_action(id).unwrap()), "{id}");
    }
    assert!(matches!(resolved("edit.preferences"), Resolved::Different(_)));
}
