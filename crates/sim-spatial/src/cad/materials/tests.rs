//! The materials panel without a window: RoboCAD's list text and filter,
//! the controls' REST round trip, the dialogs' values and where they came
//! from, and what OK sends (only the changed keys, in SI).
use super::form::{self, Origin, Print, Submit};
use super::panel::{self, FORM, SEARCH};
use super::{Focus, MaterialsArgs, MaterialsOp, controls_of, list, matches, row_label, specs};
use crate::app::actions::{Action, control_matches};
use crate::cad::actions::CadAction;
use crate::cad::document::{CadDocument, CadTarget, Connection};
use serde_json::{Value, json};
use sim_runtime::cad_client::{DocState, Health, Material, NodeSummary, SelectionItem};

fn document() -> CadDocument {
    let mut doc = CadDocument::new(CadTarget::Service("http://127.0.0.1:8420".into()));
    doc.client = Some(sim_runtime::cad_client::CadClient::new("http://127.0.0.1:8420").unwrap());
    doc.connection = Connection::Connected;
    doc.health = Some(Health { ok: true, app: "robocad".into(), revision: 4, ..Default::default() });
    doc.doc_key = Some((None, 4));
    doc.doc = Some(DocState {
        nodes: vec![NodeSummary { id: "b1".into(), kind: "body".into(), name: "Bracket".into(), visible: true, effective_visible: true, ..Default::default() }],
        materials: vec![
            json!({"id": "pla", "name": "PLA", "density": 1.24, "color": [0.85, 0.85, 0.87], "tags": ["print", "plastic"], "engineering": {"yield_strength": 4.2e7}}),
            json!({"id": "al", "name": "Aluminium 6061", "density": 2.7, "color": [0.8, 0.82, 0.85], "tags": ["metal"], "engineering": {}}),
        ],
        revision: 4,
        ..Default::default()
    });
    doc
}

fn pla(doc: &CadDocument) -> Material {
    list(doc).into_iter().find(|m| m.id == "pla").unwrap()
}

/// RoboCAD's row text and its search over names and tags.
#[test]
fn rows_and_search_are_robocads() {
    let doc = document();
    let all = list(&doc);
    assert_eq!(all.len(), 2);
    assert_eq!(row_label(&all[0]), "■ PLA   1.24 g/cm³");
    assert_eq!(row_label(&all[1]), "■ Aluminium 6061   2.7 g/cm³");
    assert!(matches(&all[0], "pl") && matches(&all[0], "PLASTIC") && !matches(&all[0], "metal"));
    assert!(matches(&all[1], "ALUM") && matches(&all[1], "met") && !matches(&all[1], "print"));
}

/// Every control fits `cad:materials:<id>` and its REST form parses back
/// to the action the panel's button writes; Apply is refused with nothing selected.
#[test]
fn controls_fit_the_pattern_and_round_trip_through_rest() {
    let mut doc = document();
    doc.materials.current = Some("pla".into());
    doc.materials.form = Some(form::new_form(4));
    let selected = vec![SelectionItem("b1".into(), "body".into(), 0)];
    let controls = controls_of(&doc, &selected);
    let ids: Vec<&str> = controls.iter().map(|c| c.0.as_str()).collect();
    for want in ["cad:materials:apply", "cad:materials:new", "cad:materials:properties", "cad:materials:row-pla", "cad:materials:row-al", "cad:materials:form-ok", "cad:materials:form-cancel"] {
        assert!(ids.contains(&want), "{want}: {ids:?}");
    }
    for (id, _, action, _) in &controls {
        assert!(control_matches("cad:materials:<id>", id), "{id}");
        let Value::Object(mut args) = crate::cad::rest_form::rest_form(action) else { panic!("{id}") };
        let name = args.remove("command").and_then(|v| v.as_str().map(str::to_string)).unwrap();
        let parsed = <CadAction as Action>::parse(&sim_api::Command { command: name, args: Value::Object(args) }).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(&parsed, action, "{id}");
    }
    let apply = controls.iter().find(|c| c.0 == "cad:materials:apply").unwrap();
    assert_eq!(apply.2, MaterialsArgs::of(MaterialsOp::Apply, Some("pla")));
    assert!(apply.3.is_ok());
    let none = controls_of(&doc, &[]).into_iter().find(|c| c.0 == "cad:materials:apply").unwrap();
    assert!(none.3.is_err_and(|e| e.contains("Nothing selected")));
    for s in specs() {
        <CadAction as Action>::parse(&sim_api::Command { command: s.name.into(), args: s.example.clone() }).unwrap_or_else(|e| panic!("{}: {e}", s.name));
    }
}

/// New…: RoboCAD's defaults, its id rule and its three decimals.
#[test]
fn a_new_material_is_robocads_dialog() {
    let mut f = form::new_form(4);
    assert_eq!(f.texts, vec!["Custom".to_string(), "1.2".to_string()]);
    form::set(&mut f, "name", "Carbon PETG").unwrap();
    form::set(&mut f, "density", "1.30049").unwrap();
    let Submit::New(m) = form::submit(&f).unwrap() else { panic!("not a new material") };
    assert_eq!((m.id.as_deref(), m.name.as_str(), m.density), (Some("carbon_petg"), "Carbon PETG", 1.3));
    assert!(m.color.is_none() && m.tags.is_none(), "RoboCAD's defaults are its own");
    form::set(&mut f, "density", "30").unwrap();
    assert!(form::ok_ready(&f).is_err() && form::submit(&f).is_err());
    assert!(form::set(&mut f, "colour", "1").unwrap_err().contains("name, density"));
}

/// Engineering properties: the override, else the physical model's
/// default, else nothing; OK sends only the changed keys, in SI.
#[test]
fn engineering_properties_show_their_origin_and_send_only_changes() {
    let mut doc = document();
    let without = form::properties_form(&doc, &pla(&doc));
    let origin = |f: &form::MaterialForm, key: &str| f.fields.iter().find(|x| x.key == key).unwrap().origin;
    assert_eq!(origin(&without, "yield_strength"), Origin::Set);
    assert_eq!(origin(&without, "youngs_modulus"), Origin::Unreported);
    assert_eq!(without.print, Print::Unknown);
    doc.physical = Some((4, Ok(json!({"materials": {"pla": {
        "youngs_modulus": 3.5e9, "poisson": 0.36, "yield_strength": 5.0e7, "ultimate_strength": 6.0e7, "glass_transition_c": 60.0,
        "thermal_conductivity": 0.13, "specific_heat": 1800.0, "thermal_expansion": 6.8e-5, "bearing_pressure": 1.5e7,
        "friction": {"pla": {"static": 0.42, "kinetic": 0.35}, "steel": {"static": 0.36, "kinetic": 0.3}, "world": {"static": 0.6, "kinetic": 0.5}},
        "print": {"anisotropy_z": 0.6, "layer_adhesion_factor": 0.7},
    }}}))));
    let mut f = form::properties_form(&doc, &pla(&doc));
    let text = |f: &form::MaterialForm, key: &str| f.texts[f.fields.iter().position(|x| x.key == key).unwrap()].clone();
    assert_eq!((text(&f, "youngs_modulus"), origin(&f, "youngs_modulus")), ("3.5".to_string(), Origin::Default));
    assert_eq!(text(&f, "yield_strength"), "42", "the override wins over the default");
    assert_eq!((text(&f, "friction_self"), text(&f, "friction_steel"), text(&f, "anisotropy_z")), ("0.35".into(), "0.3".into(), "0.6".into()));
    assert!(f.fields.iter().find(|x| x.key == "youngs_modulus").unwrap().label.ends_with("RoboCAD's default"));
    assert_eq!(form::submit(&f).unwrap(), Submit::Nothing);
    form::set(&mut f, "youngs_modulus", "4").unwrap();
    form::set(&mut f, "friction_steel", "0.25").unwrap();
    form::set(&mut f, "anisotropy_z", "0.5").unwrap();
    let Submit::Props { id, props, .. } = form::submit(&f).unwrap() else { panic!("not a properties edit") };
    assert_eq!(id, "pla");
    let keys: Vec<&str> = props.keys().map(String::as_str).collect();
    assert_eq!(keys.len(), 3, "{keys:?}");
    assert_eq!(props["youngs_modulus"], json!(4.0 / 1e-9));
    assert_eq!(props["friction"]["self"]["kinetic"], json!(0.35));
    assert_eq!(props["friction"]["steel"], json!({"static": 0.25 * 1.2, "kinetic": 0.25}));
    assert_eq!(props["friction"]["world"]["kinetic"], json!(0.35));
    assert_eq!(props["print"], json!({"anisotropy_z": 0.5}));
    // An emptied field is refused, not sent as nothing.
    form::set(&mut f, "poisson", "").unwrap();
    assert!(form::submit(&f).unwrap_err().contains("Poisson ratio"));
    // A material without a print block keeps no anisotropy.
    doc.physical = Some((4, Ok(json!({"materials": {"pla": {"print": null}}}))));
    let mut f = form::properties_form(&doc, &pla(&doc));
    assert_eq!((f.print.clone(), origin(&f, "anisotropy_z")), (Print::NotPrinted, Origin::NotPrinted));
    form::set(&mut f, "anisotropy_z", "0.5").unwrap();
    assert!(form::submit(&f).unwrap_err().contains("not a printed material"));
}

/// Opening a dialog takes the keyboard from the other fields (the kit's
/// one focus: the panel's input gives it to the dialog's first field and
/// the field that had it is told); a properties dialog opened before
/// RoboCAD's physical model arrived takes its defaults when it lands,
/// keeping a typed field, once.
#[test]
fn a_properties_dialog_takes_the_keyboard_and_the_defaults_when_they_land() {
    use crate::app::actions::Act;
    use crate::cad::numeric::NUMERIC;
    use crate::ui_kit::text::{FieldEvent, FieldMsg, TextDraft, TextEntryPlugin, TextFieldApp, TextFocus, Typing};
    use bevy::ecs::message::Messages;
    use bevy::ecs::system::RunSystemOnce;
    use bevy::prelude::App;
    let mut doc = document();
    let m = pla(&doc);
    let opened = form::properties_form(&doc, &m);
    super::open(&mut doc, opened);
    assert!(doc.materials.claimed && doc.materials.focus == Some(Focus::Field(0)));
    let first = doc.materials.form.as_ref().unwrap().texts[0].clone();
    // The numeric bar has the keyboard when the dialog opens.
    let mut app = App::new();
    app.add_plugins(TextEntryPlugin)
        .add_message::<Act<CadAction>>()
        .add_text_field(NUMERIC, crate::cad::numeric::field())
        .add_text_field(SEARCH, panel::search_field())
        .add_text_field(FORM, panel::form_field());
    app.world_mut().run_system_once(|mut t: TextFocus| assert!(t.focus(NUMERIC, "5"))).unwrap();
    app.insert_resource(doc);
    app.world_mut().run_system_once(panel::input).unwrap();
    assert!(app.world_mut().run_system_once(|t: Typing| t.focused(FORM)).unwrap(), "the dialog's field has the keyboard");
    assert_eq!(app.world_mut().run_system_once(|t: TextFocus| t.draft(FORM).cloned()).unwrap(), Some(TextDraft::new(first, true)), "its first row, selected");
    let told: Vec<FieldMsg> = app.world_mut().resource_mut::<Messages<FieldMsg>>().drain().collect();
    assert!(told.iter().any(|m| m.field == NUMERIC && m.event == FieldEvent::Blur), "the numeric bar is told: {told:?}");
    let mut doc = app.world_mut().remove_resource::<CadDocument>().unwrap();
    assert!(!doc.materials.claimed && doc.materials.focus == Some(Focus::Field(0)) && doc.materials.select_all);
    assert!(super::wants_physical(&doc), "the dialog reads the physical model");
    assert_eq!(super::refilled_form(&doc), None, "nothing has landed");
    form::set(doc.materials.form.as_mut().unwrap(), "poisson", "0.4").unwrap();
    // A model for another revision is not this dialog's.
    doc.physical = Some((3, Ok(json!({"materials": {"pla": {"youngs_modulus": 3.5e9, "poisson": 0.36}}}))));
    assert_eq!(super::refilled_form(&doc), None);
    doc.physical = Some((4, Ok(json!({"materials": {"pla": {"youngs_modulus": 3.5e9, "poisson": 0.36}}}))));
    let f = super::refilled_form(&doc).expect("the defaults landed");
    let at = |key: &str| f.fields.iter().position(|x| x.key == key).unwrap();
    assert_eq!((f.texts[at("youngs_modulus")].as_str(), f.fields[at("youngs_modulus")].origin), ("3.5", Origin::Default));
    assert_eq!((f.texts[at("poisson")].as_str(), f.opened[at("poisson")].as_str()), ("0.4", "0.36"), "the typed text is kept");
    assert_eq!(f.texts[at("yield_strength")], "42", "the override still wins");
    doc.materials.form = Some(f);
    assert_eq!(super::refilled_form(&doc), None, "once");
}

/// Written T49 fixture: rendered OK refuses a reopened same-document dialog.
#[test]
fn rendered_material_ok_retains_modal_session() {
    use bevy::prelude::*;
    use bevy::ecs::system::RunSystemOnce;
    use crate::ui_kit::{UiFonts, activation::Activated, form::FormHit};
    let mut doc = document();
    doc.materials.form = Some(form::new_form(4));
    doc.materials.form_sequence = 1;
    let mut world = World::new();
    world.insert_resource(doc);
    world.insert_resource(UiFonts { regular: default(), italic: default(), mono: default(), icons: default(), medium: default(), semibold: default() });
    world.run_system_once(panel::draw_form).unwrap();
    let mut parts = world.query::<(Entity, &panel::FormPart)>();
    let ok = parts.iter(&world).find(|(_, part)| matches!(part.0, FormHit::Ok)).unwrap().0;
    world.run_system_once(crate::cad::activation::stamp).unwrap();
    world.entity_mut(ok).insert(Activated);
    let intent = crate::cad::activation::guard(world.resource::<CadDocument>(), MaterialsArgs::of(MaterialsOp::FormSubmit, None));
    world.resource_mut::<CadDocument>().materials.form_sequence += 1;
    world.run_system_once(crate::cad::activation::refuse).unwrap();
    assert!(world.get::<Activated>(ok).is_none());
    let CadAction::Captured { source, .. } = intent else { panic!("missing modal source") };
    assert!(!crate::cad::activation::current(&source, world.resource::<CadDocument>()));
}
