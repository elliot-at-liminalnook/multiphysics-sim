//! The materials, results and physical-model client against the in-process
//! fake RoboCAD of `tests.rs`: answers as `api.py` writes them (json.dumps'
//! separators; `Material.to_json`, `results_margins`, the `/results/nodes`
//! gap route), the exact request line and body of every write, tolerant
//! reads (a `NaN` margin, a malformed entry) and original errors alongside outcome-uncertainty hints.
use super::tests::{Answer, assert_request, ok, serve};
use super::*;
use serde_json::{Map, json};

/// `Material.to_json()` of PETG, then one RoboCAD could not have written.
const MATERIALS: &str = r#"[{"id": "petg", "name": "PETG", "density": 1.27, "color": [0.75, 0.82, 0.9], "roughness": 0.35, "metallic": 0.0, "tags": ["print", "plastic"], "engineering": {"youngs_modulus": 2500000000.0}}, {"id": "bad", "density": "heavy"}]"#;

/// `GET /results/nodes` after loading a results file with a link (with a
/// hotspot), a joint and a motor block: margins as `results_margins`
/// writes them (a NaN, a malformed entry added).
const NODES: &str = r#"{"revision": 12, "path": "/tmp/leg.simresult.json", "loaded": "2026-10-01T10:00:00", "stale": true, "provenance": null, "margins": {"t1": {"yield_margin": 2.75, "peak_stress_pa": 12000000.0, "peak_temperature_c": null, "tg_margin_c": NaN}, "k1": {"bearing_margin": 3.0, "screw_shear_margin": null, "peak_reaction_force_n": 4.2}, "m1": {"stall_margin": 0.4, "peak_current_a": 1.1, "peak_winding_c": null, "mount_tg_margin_c": null}, "x": "bad"}, "nodes": {"t1": {"results": {"section": "links", "peak_stress_pa": 12000000.0, "yield_margin": 2.75, "hotspot": {"cells": [[0, 0, 0]], "stress_pa": [12000000.0]}}, "yield_strength_pa": 45000000.0}, "k1": {"results": {"section": "joints", "peak_reaction_force_n": 4.2, "bearing_margin": 3.0}, "yield_strength_pa": null}, "m1": {"results": {"section": "motors", "stall_margin": 0.4, "peak_current_a": 1.1}, "yield_strength_pa": 35000000.0}}}"#;

fn op_answer(result: &str, undo: &str) -> Answer {
    ok(&format!(r#"{{"result": {result}, "history": {{"undo": ["{undo}"], "redo": []}}}}"#))
}

#[test]
fn materials_read_tolerantly_and_add_sends_only_the_given_keys() {
    let added = r#"{"id": "carbon_pla", "name": "Carbon PLA", "density": 1.3, "color": [0.1, 0.1, 0.1], "roughness": 0.5, "metallic": 0.0, "tags": [], "engineering": {}}"#;
    let (c, server) = serve(vec![ok(MATERIALS), Answer::Json(201, added.into())]);
    let port = c.endpoint.port;
    let list = c.materials().unwrap();
    assert_eq!(list.len(), 1, "the malformed material is dropped, not the list");
    let petg = &list[0];
    assert_eq!((petg.id.as_str(), petg.density, petg.color, petg.roughness), ("petg", 1.27, [0.75, 0.82, 0.9], 0.35));
    assert_eq!(petg.tags, ["print", "plastic"]);
    assert_eq!(petg.engineering.get("youngs_modulus"), Some(&json!(2.5e9)));
    // `DocState::materials` entries read the same way; a missing field is Material's default.
    let m = Material::of(&json!({"id": "x", "name": "X", "density": 2.0})).unwrap();
    assert_eq!((m.color, m.roughness, m.metallic), ([0.7, 0.7, 0.72], 0.5, 0.0));
    assert_eq!(Material::of(&json!("petg")), None);
    let request = NewMaterial { name: "Carbon PLA".into(), density: 1.3, color: Some([0.1, 0.1, 0.1]), ..NewMaterial::default() };
    assert_eq!(c.add_material(&request).unwrap().id, "carbon_pla");
    let seen = server.join().unwrap();
    assert_request(&seen[0], "GET /materials HTTP/1.1", port, None);
    assert_request(&seen[1], "POST /materials HTTP/1.1", port, Some(r#"{"name":"Carbon PLA","density":1.3,"color":[0.1,0.1,0.1]}"#));
}

#[test]
fn results_nodes_reads_margins_and_blocks_per_node() {
    let empty = r#"{"revision": 3, "path": null, "loaded": null, "stale": null, "provenance": null, "margins": {}, "nodes": {}}"#;
    let (c, server) = serve(vec![ok(NODES), ok(empty)]);
    let port = c.endpoint.port;
    let r = c.results_nodes().unwrap();
    assert_eq!((r.revision, r.path.as_deref(), r.stale), (12, Some("/tmp/leg.simresult.json"), Some(true)));
    assert_eq!(r.margins.keys().collect::<Vec<_>>(), ["k1", "m1", "t1"], "the malformed entry is dropped");
    let t1 = &r.margins["t1"];
    assert_eq!((t1.yield_margin, t1.peak_stress_pa, t1.peak_temperature_c, t1.tg_margin_c), (Some(2.75), Some(1.2e7), None, None), "null and NaN read as None");
    assert_eq!((t1.bearing_margin, t1.stall_margin), (None, None), "a link carries no joint or motor margins");
    assert_eq!((r.margins["k1"].bearing_margin, r.margins["k1"].peak_reaction_force_n), (Some(3.0), Some(4.2)));
    assert_eq!((r.margins["m1"].stall_margin, r.margins["m1"].peak_current_a), (Some(0.4), Some(1.1)));
    let link = &r.nodes["t1"];
    assert_eq!((link.results["section"].as_str(), link.yield_strength_pa), (Some("links"), Some(4.5e7)));
    assert_eq!(link.results["hotspot"]["stress_pa"], json!([1.2e7]));
    assert_eq!(r.nodes["k1"].yield_strength_pa, None);
    let none = c.results_nodes().unwrap();
    assert_eq!((none.revision, none.stale, none.margins.len(), none.nodes.len()), (3, None, 0, 0));
    let seen = server.join().unwrap();
    for s in &seen {
        assert_request(s, "GET /results/nodes HTTP/1.1", port, None);
    }
}

#[test]
fn results_files_are_named_by_path_and_errors_pass_through() {
    let loaded = r#"{"version": 1, "links": {"thigh": {"peak_stress_pa": 5000000.0, "yield_margin": 8.0}}, "joints": {}, "motors": {}, "path": "/tmp/r.simresult.json", "loaded": "2026-10-01T10:00:00", "stale": true}"#;
    let (c, server) = serve(vec![
        ok("{}"),
        ok(loaded),
        ok(r#"{"hip": {"backlash": 0.01, "source_log": null, "fitted_at": "2026-10-01T10:00:00"}}"#),
        Answer::Json(500, r#"{"error": "FileNotFoundError: [Errno 2] No such file or directory: '/nope.json'", "trace": "Traceback ..."}"#.into()),
    ]);
    let port = c.endpoint.port;
    assert_eq!(c.results().unwrap(), json!({}));
    assert_eq!(c.load_results("/tmp/r.simresult.json").unwrap()["links"]["thigh"]["yield_margin"], json!(8.0));
    assert_eq!(c.apply_identification("/tmp/fit.json").unwrap()["hip"]["backlash"], json!(0.01));
    let e = c.load_results("/nope.json").unwrap_err();
    // A mutating 5xx preserves the server refusal and status while keeping
    // the source outcome uncertain: the command may have committed first.
    assert_eq!((e.method, e.route.as_str(), e.status), ("POST", "/results/load", Some(500)));
    assert_eq!(e.message.strip_suffix(": RoboCAD may still apply it; refresh before retrying"), Some("FileNotFoundError: [Errno 2] No such file or directory: '/nope.json'"));
    let seen = server.join().unwrap();
    assert_request(&seen[0], "GET /results HTTP/1.1", port, None);
    assert_request(&seen[1], "POST /results/load HTTP/1.1", port, Some(r#"{"path":"/tmp/r.simresult.json"}"#));
    assert_request(&seen[2], "POST /identification/apply HTTP/1.1", port, Some(r#"{"path":"/tmp/fit.json"}"#));
    assert_request(&seen[3], "POST /results/load HTTP/1.1", port, Some(r#"{"path":"/nope.json"}"#));
}

#[test]
fn physical_model_asks_for_flex_and_planar_and_never_a_path() {
    let planar = r#"{"version": 4, "planar": {"normal": [0.0, -1.0, 0.0], "origin": [0.0, 0.0, 0.0]}}"#;
    let (c, server) = serve(vec![ok(r#"{"version": 4, "planar": null}"#), ok(planar), ok(planar), ok(r#"{"version": 4, "planar": null}"#)]);
    let port = c.endpoint.port;
    assert_eq!(c.physical_model(true, false).unwrap()["planar"], json!(null));
    let model = c.physical_model(false, true).unwrap();
    assert_eq!((model["version"].as_u64(), &model["planar"]["normal"]), (Some(PHYSICAL_SCHEMA_VERSION), &json!([0.0, -1.0, 0.0])));
    c.physical_model(true, true).unwrap();
    c.physical_model(false, false).unwrap();
    let seen = server.join().unwrap();
    assert_request(&seen[0], "GET /physical?flex=1 HTTP/1.1", port, None);
    assert_request(&seen[1], "GET /physical?flex=0&planar=1 HTTP/1.1", port, None);
    assert_request(&seen[2], "GET /physical?flex=1&planar=1 HTTP/1.1", port, None);
    assert_request(&seen[3], "GET /physical?flex=0 HTTP/1.1", port, None);
}

#[test]
fn material_and_joint_physics_ops_send_the_python_signature() {
    let (c, server) = serve(vec![
        op_answer("null", "Material"),
        op_answer("null", "Color"),
        op_answer("null", "Color"),
        op_answer(r#"{"youngs_modulus": 2500000000.0, "poisson": 0.38}"#, "Material properties"),
        op_answer(r#"{"friction": {"coulomb": 0.004}}"#, "Joint physics"),
        Answer::Json(422, r#"{"error": "unknown material property stiffness; one of ('youngs_modulus', 'poisson')"}"#.into()),
    ]);
    let port = c.endpoint.port;
    let ids = vec!["a1".to_string(), "b2".to_string()];
    c.set_material(&ids, "petg").unwrap();
    c.set_color(&ids[..1], Some([1.0, 0.0, 0.0])).unwrap();
    c.set_color(&ids[..1], None).unwrap();
    let mut props = Map::new();
    props.insert("youngs_modulus".into(), json!(2.5e9));
    assert_eq!(c.set_material_props("petg", &props).unwrap().result["poisson"], json!(0.38));
    let mut overrides = Map::new();
    overrides.insert("friction".into(), json!({"coulomb": 0.004}));
    c.set_joint_physics("j1", &overrides).unwrap();
    let mut bad = Map::new();
    bad.insert("stiffness".into(), json!(1.0));
    let e = c.set_material_props("petg", &bad).unwrap_err();
    assert_eq!((e.status, e.message.as_str()), (Some(422), "unknown material property stiffness; one of ('youngs_modulus', 'poisson')"));
    let seen = server.join().unwrap();
    assert_request(&seen[0], "POST /ops/set_material HTTP/1.1", port, Some(r#"{"args":[["a1","b2"],"petg"],"kwargs":{}}"#));
    assert_request(&seen[1], "POST /ops/set_color HTTP/1.1", port, Some(r#"{"args":[["a1"],[1.0,0.0,0.0]],"kwargs":{}}"#));
    assert_request(&seen[2], "POST /ops/set_color HTTP/1.1", port, Some(r#"{"args":[["a1"],null],"kwargs":{}}"#));
    assert_request(&seen[3], "POST /ops/set_material_props HTTP/1.1", port, Some(r#"{"args":["petg"],"kwargs":{"youngs_modulus":2500000000.0}}"#));
    assert_request(&seen[4], "POST /ops/set_joint_physics HTTP/1.1", port, Some(r#"{"args":["j1"],"kwargs":{"friction":{"coulomb":0.004}}}"#));
    assert_request(&seen[5], "POST /ops/set_material_props HTTP/1.1", port, Some(r#"{"args":["petg"],"kwargs":{"stiffness":1.0}}"#));
}
