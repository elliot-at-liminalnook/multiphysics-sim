//! The command surfaces without a window: RoboCAD's command table against
//! the op catalogue, the menus, readiness, the key parser, and every
//! surfaces control round-tripping through its REST form.
use super::registry::{self, CATEGORIES, COMMANDS, CONTEXT, MAKE_UNIQUE, Native, Resolved, SELECT_RADIAL, TOOLBAR, VIEW_RADIAL};
use super::{Surface, entries};
use crate::app::actions::{self, Action};
use crate::cad::actions::{CadAction, rest_form};
use crate::cad::document::{CadDocument, CadTarget, Connection};
use crate::cad::keys::{Binding, parse};
use crate::cad::ops::{CATALOGUE, FormState, Needs, OpEntry};
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
        if matches!(e.flow, crate::cad::ops::Flow::PickThenForm(_)) || !needs_something {
            assert!(r.is_ok(), "{}: {r:?}", e.id);
        } else {
            assert_eq!(r, Err(e.refusal.to_string()), "{}", e.id);
        }
    }
    // Later epics and GUI-only commands refuse by name.
    let own = own_controls(&doc);
    let front = registry::command("view.front").unwrap();
    assert_eq!(registry::ready(front, &doc, &own), Err("View front belongs to the cad-views-export epic; not in the native viewer yet".to_string()));
    let guide = registry::command("help.guide").unwrap();
    assert_eq!(registry::ready(guide, &doc, &own), Err("User guide is GUI-only: it runs in RoboCAD's desktop window".to_string()));
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
    assert_eq!(list.iter().find(|e| e.id == "help.guide").unwrap().note, "GUI-only");
}

#[test]
fn the_context_menu_offers_make_unique_for_instances() {
    let mut doc = document();
    let ids = |doc: &CadDocument| entries(&Surface::Context { at: None }, doc, &own_controls(doc)).into_iter().map(|e| e.id).collect::<Vec<_>>();
    assert_eq!(ids(&doc), CONTEXT.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    doc.selection = vec![SelectionItem("i1".into(), "body".into(), 0)];
    let with = ids(&doc);
    assert_eq!(with.last().map(String::as_str), Some(MAKE_UNIQUE.0));
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
