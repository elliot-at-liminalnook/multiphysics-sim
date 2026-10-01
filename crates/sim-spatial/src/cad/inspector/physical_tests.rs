//! The inspector's physical rows without a window: the exact measurement's
//! cancel rule (any selection change or edit drops the run, saying why),
//! RoboCAD's combination of the nodes' mass blocks, the joint physics
//! payloads in RoboCAD's SI units, the rows' values and " *" marks, the
//! results line and Python's `g` formatting.
use super::exact::{ExactRun, ExactState, Measured, Stamp, cancel_reason, combine, settle};
use super::physical_edit::{InspectorArgs, JointField, check_color, joint_override, parse_color, physical_specs, py_g};
use super::rows::{results_line, row, source_line};
use crate::app::actions::Action;
use crate::cad::actions::CadAction;
use crate::jobs::Job;
use serde_json::{Map, Value, json};
use sim_runtime::cad_client::MassBlock;

fn stamp() -> Stamp {
    Stamp { generation: 1, revision: 7, edit_seq: 3, nodes: vec!["b1".into(), "b2".into()] }
}

fn measured() -> Measured {
    Measured { items: 2, size: [10.0, 20.0, 30.0], volume: 6000.0, area: 2200.0, mass_g: 7.44, centroid: [1.0, 2.0, 3.0] }
}

/// A run in flight (its job stands in finished: the rule drops it before polling).
fn running(at: &Stamp) -> ExactState {
    ExactState { run: Some(ExactRun::new(at.clone(), Job::finished(1, Ok(measured())))), ..ExactState::default() }
}

/// A selection change drops the run (cancelling its job) and the status says why.
#[test]
fn a_selection_change_cancels_the_exact_measurement_and_says_why() {
    let then = stamp();
    let mut state = running(&then);
    let now = Stamp { nodes: vec!["b1".into()], ..then.clone() };
    assert!(settle(&mut state, &now));
    assert!(state.run.is_none(), "the job is gone");
    assert!(state.result.is_none(), "nothing landed for the old selection");
    let facts = state.facts(&now).expect("the status is shown");
    assert!(facts.contains("cancelled") && facts.contains("the selection changed"), "{facts}");
    // The status belongs to the state it was cancelled in.
    let later = Stamp { nodes: vec!["b2".into()], ..then };
    settle(&mut state, &later);
    assert_eq!(state.facts(&later), None);
}

/// Any edit (one sent from here: the edit sequence; or a newer revision shown) drops it too.
#[test]
fn an_edit_cancels_the_exact_measurement_and_says_why() {
    let then = stamp();
    let mut state = running(&then);
    let now = Stamp { edit_seq: then.edit_seq + 1, ..then.clone() };
    assert!(settle(&mut state, &now));
    assert!(state.run.is_none());
    assert!(state.facts(&now).unwrap().contains("the document was edited"));
    let mut state = running(&then);
    let now = Stamp { revision: then.revision + 1, ..then.clone() };
    settle(&mut state, &now);
    assert!(state.run.is_none() && state.facts(&now).unwrap().contains("the document was edited"));
    assert_eq!(cancel_reason(&then, &Stamp { generation: 2, ..then.clone() }), Some("the CAD document was replaced or reconnected"));
    assert_eq!(cancel_reason(&then, &then), None);
}

/// Unchanged, the result lands with RoboCAD's text; a later change drops it.
#[test]
fn a_finished_measurement_lands_with_robocads_text() {
    let at = stamp();
    let mut state = running(&at);
    assert!(settle(&mut state, &at));
    assert!(state.run.is_none());
    assert_eq!(state.facts(&at).unwrap(), "2 measured item(s)\nsize 10.000 × 20.000 × 30.000 mm\nvolume 6.000 cm³\narea 22.00 cm²\nmass 7.44 g\ncentroid (1.00, 2.00, 3.00)");
    let moved = Stamp { nodes: vec!["b2".into()], ..at };
    assert!(settle(&mut state, &moved));
    assert!(state.result.is_none(), "RoboCAD drops the result when the key changes");
}

fn block(lo: [f64; 3], hi: [f64; 3], volume: f64, area: f64, mass: f64, c: [f64; 3]) -> Option<MassBlock> {
    let v = |a: [f64; 3]| a.iter().map(|x| Some(*x)).collect::<Vec<_>>();
    Some(MassBlock { volume_mm3: Some(volume), area_mm2: Some(area), mass_g: Some(mass), centroid: v(c), bbox_min: v(lo), bbox_max: v(hi), size: Vec::new() })
}

/// `selection_properties`: union of the boxes, sums, mass-weighted
/// centroid; nodes without a body skipped; a null is an error naming it.
#[test]
fn nodes_combine_as_robocads_selection_properties() {
    let blocks = vec![
        ("A".to_string(), block([0.0, 0.0, 0.0], [10.0, 10.0, 10.0], 1000.0, 600.0, 1.0, [5.0, 5.0, 5.0])),
        ("Group".to_string(), None),
        ("B".to_string(), block([20.0, -5.0, 0.0], [30.0, 5.0, 2.0], 200.0, 280.0, 3.0, [25.0, 0.0, 1.0])),
    ];
    let m = combine(&blocks).unwrap();
    assert_eq!(m.items, 2);
    assert_eq!(m.size, [30.0, 15.0, 10.0]);
    assert_eq!((m.volume, m.area, m.mass_g), (1200.0, 880.0, 4.0));
    assert_eq!(m.centroid, [20.0, 1.25, 2.0]);
    assert_eq!(combine(&[("Group".into(), None)]).unwrap_err(), "No measurable geometry selected");
    let mut null = block([0.0; 3], [1.0; 3], 1.0, 1.0, 1.0, [0.5; 3]);
    null.as_mut().unwrap().mass_g = None;
    assert!(combine(&[("C".into(), null)]).unwrap_err().contains("C's mass"));
}

fn object(v: Value) -> Map<String, Value> {
    v.as_object().cloned().unwrap()
}

/// Each row's payload is RoboCAD's (widgets.py:513-541, 653-672), in SI.
#[test]
fn joint_physics_rows_send_robocads_si_payloads() {
    assert_eq!(joint_override(JointField::Clearance, "0.2").unwrap(), object(json!({"clearance": 0.2 / 1e3})));
    assert_eq!(joint_override(JointField::Wobble, "1").unwrap(), object(json!({"wobble": 1.0 / (180.0 / std::f64::consts::PI)})));
    assert_eq!(joint_override(JointField::Coulomb, "5").unwrap(), object(json!({"friction": {"coulomb": 5.0 * 1e-3}})));
    assert_eq!(joint_override(JointField::Viscous, "0.2").unwrap(), object(json!({"friction": {"viscous": 0.2 * 1e-3}})));
    assert_eq!(joint_override(JointField::RadialStiffness, "1e5").unwrap(), object(json!({"stiffness": {"radial": 1e5}})));
    assert_eq!(joint_override(JointField::FlexPatchRadius, "5").unwrap(), object(json!({"flex_patch_radius": 5.0 * 1e-3})));
    assert_eq!(joint_override(JointField::FlexPatchRadius, " ").unwrap(), object(json!({"flex_patch_radius": null})));
    assert_eq!(joint_override(JointField::DriveBacklash, "2").unwrap(), object(json!({"drive_backlash": {"width_rad": 2f64.to_radians(), "provenance": "estimated", "reference": "Estimate authored in the CAD joint inspector"}})));
    assert_eq!(joint_override(JointField::DriveBacklash, "").unwrap(), object(json!({"drive_backlash": {"width_rad": null, "provenance": "unmeasured", "reference": "Not measured"}})));
    assert!(joint_override(JointField::Clearance, "").unwrap_err().starts_with("Radial clearance (mm): "));
    assert_eq!(parse_color("1, 0.5, 0").unwrap(), [1.0, 0.5, 0.0]);
    assert!(parse_color("1, 2, 0").unwrap_err().contains("0–1"));
    assert!(check_color([0.0, f64::NAN, 0.0]).is_err());
}

/// Values come from the physical model (merged), " *" from the declared
/// overrides, "Unmeasured" (empty, provenance unmeasured) without a width.
#[test]
fn rows_show_robocads_values_and_override_marks() {
    let phys = object(json!({
        "source": "declared", "pin_radius": 0.0015, "hole_radius": 0.00165, "contact_length": 0.006,
        "clearance": 0.00015, "wobble": 0.05, "flex_patch_radius": 0.004,
        "drive_backlash": {"width_rad": null, "provenance": "unmeasured", "reference": "Radial bearing clearance does not determine drive-connection rotational lost motion"},
        "friction": {"coulomb": 0.0021, "viscous": 0.0002}, "stiffness": {"radial": 1.25e6},
    }));
    let over = object(json!({"clearance": 0.0002, "friction": {"coulomb": 0.003}}));
    let clearance = row(JointField::Clearance, &phys, Some(Some(&over)));
    assert_eq!((clearance.label.as_str(), clearance.text.as_str()), ("Radial clearance (mm) *", "0.2"));
    let wobble = row(JointField::Wobble, &phys, Some(Some(&over)));
    assert_eq!((wobble.label.as_str(), wobble.text.as_str()), ("Wobble (°)", "2.865"));
    let coulomb = row(JointField::Coulomb, &phys, Some(Some(&over)));
    assert_eq!((coulomb.label.as_str(), coulomb.text.as_str()), ("Coulomb friction (mN·m) *", "3"));
    let viscous = row(JointField::Viscous, &phys, None);
    assert_eq!((viscous.label.as_str(), viscous.text.as_str()), ("Viscous (mN·m·s)", "0.2"));
    assert_eq!(row(JointField::RadialStiffness, &phys, None).text, "1.25e+06");
    assert_eq!(row(JointField::FlexPatchRadius, &phys, None).text, "4");
    let drive = row(JointField::DriveBacklash, &phys, Some(None));
    assert_eq!((drive.label.as_str(), drive.text.as_str()), ("Drive backlash (°; unmeasured)", ""));
    assert!(drive.note.unwrap().contains("Radial bearing clearance"));
    let declared = object(json!({"drive_backlash": {"width_rad": 0.01, "provenance": "estimated", "reference": "Estimate authored in the CAD joint inspector"}}));
    let drive = row(JointField::DriveBacklash, &phys, Some(Some(&declared)));
    assert_eq!((drive.label.as_str(), drive.text.as_str()), ("Drive backlash (°; estimated)", "0.57296"));
    assert_eq!(source_line(&phys), "source: declared, pin Ø3.00 mm in Ø3.30 mm over 6.0 mm; * = overridden");
    // A value the model lacks is left empty, not 0.
    assert_eq!(row(JointField::Clearance, &Map::new(), None).text, "");
}

#[test]
fn the_results_line_and_g_format_are_robocads() {
    let r = json!({"section": "links", "peak_stress_pa": 12345678.0, "yield_margin": 3.14159, "tg_margin_c": 20.0, "max_deflection_m": null});
    assert_eq!(results_line(&r).unwrap(), "Results: peak_stress_pa 1.23e+07, yield_margin 3.14");
    assert_eq!(results_line(&json!({"section": "links"})), None);
    for (v, p, want) in [(0.00015, 4, "0.00015"), (0.000015, 4, "1.5e-05"), (1.5e8, 3, "1.5e+08"), (3.5, 4, "3.5"), (123456.0, 4, "1.235e+05"), (1234.0, 4, "1234"), (0.0, 4, "0"), (-2.5, 3, "-2.5"), (100.0, 3, "100")] {
        assert_eq!(py_g(v, p), want, "{v} .{p}g");
    }
}

/// The spec's example parses as the command, and the args round-trip.
#[test]
fn cad_inspector_parses_and_round_trips() {
    for s in physical_specs() {
        let parsed = <CadAction as Action>::parse(&sim_api::Command { command: s.name.into(), args: s.example.clone() }).unwrap_or_else(|e| panic!("{}: {e}", s.name));
        let CadAction::CadInspector(args) = parsed else { panic!("not cad_inspector") };
        assert_eq!(args.field, Some(JointField::Clearance));
        let back: InspectorArgs = serde_json::from_value(serde_json::to_value(&args).unwrap()).unwrap();
        assert_eq!(back, args);
    }
}
