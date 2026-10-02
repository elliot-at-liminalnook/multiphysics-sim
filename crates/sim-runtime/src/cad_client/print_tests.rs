//! The print client against the in-process fake RoboCAD of `tests.rs`:
//! answers as `api.py` `Service.print_request`, `print_jobs.py` (`Job.public`,
//! `split`'s summary) and the `/nodes/{id}/thin|validate` reads write them
//! (json.dumps' separators, 202 for a started job but a split job's 200), the exact request line
//! and body of every call (serde's field order; `json!` objects are
//! BTreeMaps, so their keys go out sorted), tolerant reads (registry order
//! kept, malformed entries dropped) and RoboCAD's original errors alongside outcome-uncertainty hints.
use super::tests::{Answer, assert_request, ok, serve};
use super::*;
use serde_json::{Map, Value, json};

/// `GET /print/registry` as `print_request` writes it over today's
/// registry (`usable_mm` are ints: build less 2 × the 2 mm margin), with a
/// printer RoboCAD could not have written between the real ones.
const REGISTRY: &str = r#"{"path": "/repo/library/printing/registry.json", "sha256": "ab12cd", "revision": 1, "printers": {"bambu-h2c": {"name": "Bambu Lab H2C", "usable_mm": [321, 316, 320]}, "bambu-p1s": {"name": "Bambu Lab P1S", "usable_mm": [252, 252, 256]}, "broken": {"name": 5, "usable_mm": [1, 2, 3]}, "bambu-a1-mini": {"name": "Bambu Lab A1 mini", "usable_mm": [176, 176, 180]}}, "materials": {"pla-basic": {"name": "PLA (PLA Basic class)", "cad_material": "pla"}, "petg-hf": {"name": "PETG (PETG HF class)", "cad_material": "petg"}}}"#;

/// `print_jobs.split`'s answer: `{**summary, revision, group, piece_nodes}`.
const SPLIT: &str = r#"{"source": "b1", "printer": "bambu-p1s", "usable_mm": [252, 252, 256], "registry_sha256": "ab12cd", "cuts": [{"point": [0.0, 0.0, 120.0], "normal": [0.0, 0.0, 1.0], "why": "height 300 > 256"}], "pieces": [{"index": 0, "size_mm": [40.0, 40.0, 120.0], "fits": true, "fits_upright": true, "volume_mm3": 192000.0}, {"index": 1, "size_mm": [40.0, 40.0, 180.0], "fits": true, "fits_upright": true, "volume_mm3": 288000.0}], "seams": [], "hardware": [{"item": "heat-set insert", "size": "M3", "count": 4}, {"item": "socket head screw", "size": "M3 × 12 mm", "count": 4}, {"item": "steel dowel pin", "size": "Ø4 × 16 mm", "count": 2}], "revision": 5, "group": "g1", "piece_nodes": ["p0", "p1"]}"#;

/// `Job.public()` of a job just started.
fn started(id: &str, kind: &str) -> String {
    format!(r#"{{"id": "{id}", "kind": "{kind}", "state": "queued", "fraction": 0.0, "message": "", "error": null, "result": null, "out_dir": null, "seconds": 0.0}}"#)
}

fn op_answer(result: &str, undo: &str) -> Answer {
    ok(&format!(r#"{{"result": {result}, "history": {{"undo": ["{undo}"], "redo": []}}}}"#))
}

fn split_request() -> SplitRequest {
    let mut options = Map::new();
    options.insert("screw".into(), json!("M4"));
    options.insert("max_screws".into(), json!(4));
    SplitRequest { node: "b1".into(), printer: Some("bambu-p1s".into()), joint: Some("pins".into()), expected_revision: Some(4), options, ..SplitRequest::default() }
}

#[test]
fn registry_keeps_the_registry_order_and_drops_a_malformed_printer() {
    let (c, server) = serve(vec![ok(REGISTRY), ok(r#"{"path": null, "sha256": null, "revision": null, "printers": [], "materials": null}"#)]);
    let port = c.endpoint.port;
    let r = c.print_registry().unwrap();
    assert_eq!(r.printers.keys().collect::<Vec<_>>(), ["bambu-h2c", "bambu-p1s", "bambu-a1-mini"], "registry order (h2c first, though it sorts after a1-mini); the malformed printer dropped");
    assert_eq!(r.materials.keys().collect::<Vec<_>>(), ["pla-basic", "petg-hf"]);
    assert_eq!((r.path.as_deref(), r.sha256.as_deref(), &r.revision), (Some("/repo/library/printing/registry.json"), Some("ab12cd"), &json!(1)));
    let h2c = r.printers.get("bambu-h2c").unwrap();
    assert_eq!((h2c.name.as_str(), h2c.usable_mm.as_slice()), ("Bambu Lab H2C", &[321.0, 316.0, 320.0][..]));
    assert_eq!(h2c.label("bambu-h2c", |x| format!("{x}")), "bambu-h2c (321 × 316 × 320 mm)");
    assert_eq!(r.materials.get("petg-hf").unwrap().cad_material.as_deref(), Some("petg"));
    assert_eq!(r.printers.get("broken"), None);
    // Written back as an object in the same order, and read again unchanged.
    let text = serde_json::to_string(&r.printers).unwrap();
    assert!(text.starts_with(r#"{"bambu-h2c":{"name":"Bambu Lab H2C","usable_mm":[321.0,316.0,320.0]},"bambu-p1s":"#), "{text}");
    assert_eq!(serde_json::from_str::<Ordered<PrinterInfo>>(&text).unwrap(), r.printers);
    let empty = c.print_registry().unwrap();
    assert!(empty.printers.is_empty() && empty.materials.is_empty(), "anything but an object reads as empty");
    let seen = server.join().unwrap();
    for s in &seen {
        assert_request(s, "GET /print/registry HTTP/1.1", port, None);
    }
}

#[test]
fn study_reads_the_study_parts_and_split_groups() {
    let (c, server) = serve(vec![
        ok(r#"{"revision": 9, "study": {"printer": "bambu-p1s", "material": "petg-hf", "parts": [{"node": "a1", "fixtures": [{"region": {"bottom": true}}], "loads": []}, {"name": "no node"}, {"node": "b2"}]}, "splits": ["g1", 7, "g2"]}"#),
        ok(r#"{"revision": 3, "study": null, "splits": []}"#),
    ]);
    let port = c.endpoint.port;
    let s = c.print_study().unwrap();
    assert_eq!(s.revision, 9);
    assert!(s.has_study());
    assert_eq!(s.part_nodes(), ["a1", "b2"]);
    assert_eq!(s.splits, ["g1", "g2"], "a malformed id is dropped, not the list");
    assert_eq!(s.study["printer"], json!("bambu-p1s"));
    let none = c.print_study().unwrap();
    assert_eq!((none.revision, none.has_study(), none.part_nodes().len(), none.splits.len()), (3, false, 0, 0));
    // RoboCAD's `if not study`: an empty object is no study either.
    assert!(!serde_json::from_value::<PrintStudy>(json!({"revision": 1, "study": {}})).unwrap().has_study());
    let seen = server.join().unwrap();
    for s in &seen {
        assert_request(s, "GET /print/study HTTP/1.1", port, None);
    }
}

#[test]
fn split_now_answers_the_summary_and_split_job_starts_a_job() {
    // The split job answers 200 like the synchronous split (api.py:1412).
    let (c, server) = serve(vec![ok(SPLIT), ok(&started("3f2a9c1b0d", "split"))]);
    let port = c.endpoint.port;
    // `background` is the call's, not the request's.
    let done = c.print_split_now(&SplitRequest { background: true, ..split_request() }).unwrap();
    assert_eq!((done.revision, done.group.as_str(), done.piece_nodes.as_slice()), (5, "g1", &["p0".to_string(), "p1".to_string()][..]));
    assert_eq!((done.printer.as_deref(), done.usable_mm.as_slice(), done.registry_sha256.as_deref()), (Some("bambu-p1s"), &[252.0, 252.0, 256.0][..], Some("ab12cd")));
    assert_eq!(done.hardware[2], Hardware { item: "steel dowel pin".into(), size: "Ø4 × 16 mm".into(), count: 2 });
    assert_eq!(done.summary.keys().map(String::as_str).collect::<Vec<_>>(), ["cuts", "pieces", "seams", "source"]);
    assert_eq!(done.status(), "split into 2 pieces; hardware: 4× heat-set insert M3, 4× socket head screw M3 × 12 mm, 2× steel dowel pin Ø4 × 16 mm");
    let job = c.print_split_job(&split_request()).unwrap();
    assert_eq!((job.id.as_str(), job.kind.as_str(), job.state.as_str(), job.result.clone()), ("3f2a9c1b0d", "split", "queued", Value::Null));
    assert!(job.running() && !job.ended());
    let seen = server.join().unwrap();
    assert_request(&seen[0], "POST /print/split HTTP/1.1", port, Some(r#"{"node":"b1","printer":"bambu-p1s","joint":"pins","expected_revision":4,"max_screws":4,"screw":"M4"}"#));
    assert_request(&seen[1], "POST /print/split HTTP/1.1", port, Some(r#"{"node":"b1","printer":"bambu-p1s","joint":"pins","expected_revision":4,"background":true,"max_screws":4,"screw":"M4"}"#));
}

#[test]
fn split_options_cannot_override_the_request_fields() {
    let (c, server) = serve(vec![ok(SPLIT), ok(&started("3f2a9c1b0d", "split"))]);
    let port = c.endpoint.port;
    // Sent after the fields, each would win in Python's `json.loads`.
    let mut request = split_request();
    for (k, v) in [("background", json!(true)), ("node", json!("zz")), ("printer", json!("bambu-x1")), ("joint", json!("dovetail")), ("name", json!("Other")), ("expected_revision", json!(99))] {
        request.options.insert(k.into(), v);
    }
    let done = c.print_split_now(&request).unwrap();
    assert_eq!(done.group, "g1", "answered as the synchronous split, not a job");
    let job = c.print_split_job(&SplitRequest { name: Some("Femur pieces".into()), ..request.clone() }).unwrap();
    assert_eq!(job.state, "queued");
    assert_eq!(request.options.len(), 8, "the caller's request is left as it was");
    let seen = server.join().unwrap();
    assert_request(&seen[0], "POST /print/split HTTP/1.1", port, Some(r#"{"node":"b1","printer":"bambu-p1s","joint":"pins","expected_revision":4,"max_screws":4,"screw":"M4"}"#));
    assert_request(&seen[1], "POST /print/split HTTP/1.1", port, Some(r#"{"node":"b1","printer":"bambu-p1s","joint":"pins","name":"Femur pieces","expected_revision":4,"background":true,"max_screws":4,"screw":"M4"}"#));
}

#[test]
fn print_start_sends_each_job_kind_and_refuses_others_unsent() {
    let kinds = ["analyze", "plan", "assembly", "coupons", "strength_split"];
    let (c, server) = serve(kinds.iter().enumerate().map(|(i, k)| Answer::Json(202, started(&format!("00000000a{i}"), k))).collect());
    let port = c.endpoint.port;
    let bodies = [
        json!({"printer": "bambu-h2c", "material": "pla-basic", "parts": [{"node": "a1", "fixtures": [], "loads": []}], "expected_revision": 4}),
        json!({"parts": [{"node": "a1"}], "safety_target": 2.5}),
        json!({"group": "g1", "exploded": false}),
        json!({"group": "g1", "material": "petg-hf"}),
        json!({"node": "a1", "part": {"node": "a1"}}),
    ];
    for (i, (kind, body)) in kinds.iter().zip(&bodies).enumerate() {
        let job = c.print_start(kind, body).unwrap();
        assert_eq!((job.id.as_str(), job.kind.as_str(), job.running()), (format!("00000000a{i}").as_str(), *kind, true));
    }
    for kind in ["split", "jobs", "../ops/delete"] {
        let e = c.print_start(kind, &json!({})).unwrap_err();
        assert_eq!((e.method, e.route.clone(), e.status), ("-", format!("/print/{kind}"), None));
        assert!(e.message.starts_with(&format!("{kind} is not a print job")), "{e:?}");
    }
    let seen = server.join().unwrap();
    assert_eq!(seen.len(), 5, "a refused kind sends nothing");
    let sent = [
        r#"{"expected_revision":4,"material":"pla-basic","parts":[{"fixtures":[],"loads":[],"node":"a1"}],"printer":"bambu-h2c"}"#,
        r#"{"parts":[{"node":"a1"}],"safety_target":2.5}"#,
        r#"{"exploded":false,"group":"g1"}"#,
        r#"{"group":"g1","material":"petg-hf"}"#,
        r#"{"node":"a1","part":{"node":"a1"}}"#,
    ];
    for ((s, kind), body) in seen.iter().zip(kinds).zip(sent) {
        assert_request(s, &format!("POST /print/{kind} HTTP/1.1"), port, Some(body));
    }
}

#[test]
fn jobs_list_oldest_first_and_drop_a_malformed_job() {
    let list = r#"[{"id": "1111111111", "kind": "split", "state": "done", "fraction": 1.0, "message": "cutting Femur for the bambu-p1s", "error": null, "result": {"revision": 5, "group": "g1", "piece_nodes": ["p0"], "hardware": []}, "out_dir": null, "seconds": 2.41}, {"id": 7, "kind": "plan"}, {"id": "2222222222", "kind": "analyze", "state": "failed", "fraction": 0.1, "message": "finding where parts are held and loaded", "error": "parts[0].node 'zz' is not a body", "result": null, "out_dir": "/repo/runs/cad-print/untitled-strength-20261001-101500-ab12", "seconds": 0.12}, {"id": "3333333333", "kind": "coupons", "state": "running", "fraction": 0.5, "message": "making coupons", "error": null, "result": null, "out_dir": null, "seconds": 1.0}, {"id": "4444444444", "kind": "plan", "state": "cancelled", "fraction": 0.3, "message": "", "error": "cancelled", "result": null, "out_dir": null, "seconds": 4.0}]"#;
    let (c, server) = serve(vec![ok(list), ok("{}")]);
    let port = c.endpoint.port;
    let jobs = c.print_jobs().unwrap();
    assert_eq!(jobs.iter().map(|j| j.id.as_str()).collect::<Vec<_>>(), ["1111111111", "2222222222", "3333333333", "4444444444"]);
    assert_eq!(jobs.iter().map(PrintJob::running).collect::<Vec<_>>(), [false, false, true, false]);
    assert!(jobs.iter().filter(|j| j.state != "running").all(PrintJob::ended));
    assert_eq!((jobs[1].error.as_deref(), jobs[1].out_dir.as_deref(), jobs[1].seconds), (Some("parts[0].node 'zz' is not a body"), Some("/repo/runs/cad-print/untitled-strength-20261001-101500-ab12"), 0.12));
    let split: SplitDone = serde_json::from_value(jobs[0].result.clone()).unwrap();
    assert_eq!((split.group.as_str(), split.status()), ("g1", "split into 1 pieces; hardware: ".to_string()));
    let e = c.print_jobs().unwrap_err();
    assert_eq!((e.method, e.route.as_str(), e.status, e.message.as_str()), ("GET", "/print/jobs", None, "unexpected answer: expected a list of print jobs, got {}"));
    let seen = server.join().unwrap();
    for s in &seen {
        assert_request(s, "GET /print/jobs HTTP/1.1", port, None);
    }
}

#[test]
fn job_reads_and_cancel_sends_a_bare_delete() {
    let done = format!(r#"{{"id": "3f2a9c1b0d", "kind": "split", "state": "done", "fraction": 1.0, "message": "cutting Femur for the bambu-p1s", "error": null, "result": {SPLIT}, "out_dir": null, "seconds": 3.5}}"#);
    let running = r#"{"id": "5e6f7a8b9c", "kind": "analyze", "state": "running", "fraction": 0.431, "message": "voxelising", "error": null, "result": null, "out_dir": "/repo/runs/cad-print/untitled-strength-20261001-101500-ab12", "seconds": 3.27}"#;
    // `str(KeyError(...))` is the message's repr: RoboCAD's text carries the quotes.
    let unknown = || Answer::Json(404, r#"{"error": "'no print job x'"}"#.into());
    let (c, server) = serve(vec![ok(&done), ok(running), unknown(), unknown()]);
    let port = c.endpoint.port;
    let job = c.print_job("3f2a9c1b0d").unwrap();
    assert!(job.ended() && !job.running());
    let split: SplitDone = serde_json::from_value(job.result).unwrap();
    assert_eq!(split.status(), "split into 2 pieces; hardware: 4× heat-set insert M3, 4× socket head screw M3 × 12 mm, 2× steel dowel pin Ø4 × 16 mm");
    let cancelling = c.cancel_print_job("5e6f7a8b9c").unwrap();
    assert_eq!((cancelling.state.as_str(), cancelling.fraction, cancelling.message.as_str()), ("running", 0.431, "voxelising"), "still running until its next check");
    for e in [c.print_job("x").unwrap_err(), c.cancel_print_job("x").unwrap_err()] {
        assert!(e.not_found(), "{e:?}");
        assert_eq!(e.message, "'no print job x'");
    }
    let seen = server.join().unwrap();
    assert_request(&seen[0], "GET /print/jobs/3f2a9c1b0d HTTP/1.1", port, None);
    assert_request(&seen[1], "DELETE /print/jobs/5e6f7a8b9c HTTP/1.1", port, None);
    assert_request(&seen[2], "GET /print/jobs/x HTTP/1.1", port, None);
    assert_request(&seen[3], "DELETE /print/jobs/x HTTP/1.1", port, None);
}

#[test]
fn thin_walls_and_validation_read_tolerantly() {
    let thin = r#"[{"point": [1.0, 2.0, 3.0], "thickness": 0.8, "face": 4}, {"point": [1.0, 2.0], "thickness": 0.5, "face": 1}, {"point": [0.5, 0.0, -2.25], "thickness": 1.1, "face": 0}]"#;
    let report = r#"{"valid": false, "watertight": false, "issues": [{"severity": "error", "message": "shell is open", "location": [0.0, 1.0, 2.0], "fix": "sew faces"}, {"severity": "warning", "message": "small edge", "location": null, "fix": null}, {"severity": 3}], "summary": "error: shell is open (sew faces); warning: small edge"}"#;
    let (c, server) = serve(vec![ok(thin), ok("[]"), Answer::Json(404, r#"{"error": "Sketch has no geometry"}"#.into()), ok(report)]);
    let port = c.endpoint.port;
    let regions = c.thin_walls("b1", 1.2).unwrap();
    assert_eq!(regions, [ThinRegion { point: [1.0, 2.0, 3.0], thickness: 0.8, face: 4 }, ThinRegion { point: [0.5, 0.0, -2.25], thickness: 1.1, face: 0 }], "the malformed region is dropped");
    assert!(c.thin_walls("b1", 0.5).unwrap().is_empty());
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let e = c.thin_walls("b1", bad).unwrap_err();
        assert_eq!((e.method, e.route.as_str(), e.status), ("-", "/nodes/b1/thin", None), "refused unsent: {e:?}");
        assert!(e.message.starts_with("the wall threshold must be a finite number of mm"), "{e:?}");
    }
    let e = c.thin_walls("s1", 1.2).unwrap_err();
    assert_eq!((e.status, e.message.as_str(), e.route.as_str()), (Some(404), "Sketch has no geometry", "/nodes/s1/thin?threshold=1.2"));
    let v = c.validate_node("b1").unwrap();
    assert_eq!((v.valid, v.watertight, v.issues.len()), (false, false, 2));
    assert_eq!(v.issues[0], ValidationIssue { severity: "error".into(), message: "shell is open".into(), location: Some([0.0, 1.0, 2.0]), fix: Some("sew faces".into()) });
    assert_eq!((v.issues[1].location, v.issues[1].fix.clone()), (None, None));
    assert_eq!(v.summary, "error: shell is open (sew faces); warning: small edge");
    let seen = server.join().unwrap();
    assert_request(&seen[0], "GET /nodes/b1/thin?threshold=1.2 HTTP/1.1", port, None);
    assert_request(&seen[1], "GET /nodes/b1/thin?threshold=0.5 HTTP/1.1", port, None);
    assert_request(&seen[2], "GET /nodes/s1/thin?threshold=1.2 HTTP/1.1", port, None);
    assert_request(&seen[3], "GET /nodes/b1/validate HTTP/1.1", port, None);
}

#[test]
fn print_ops_send_the_python_signature() {
    let (c, server) = serve(vec![
        op_answer(r#""b1""#, "Clearance"),
        op_answer(r#""b1""#, "M4 counterbore"),
        op_answer(r#""g7""#, "Split for printing"),
        // An unknown option key is a TypeError `Service.op` does not map: the handler's 500.
        Answer::Json(500, r#"{"error": "TypeError: SplitOptions.__init__() got an unexpected keyword argument 'colour'", "trace": "Traceback ..."}"#.into()),
        Answer::Json(400, r#"{"error": "face index 9 out of range (0..5)"}"#.into()),
        Answer::Json(422, r#"{"error": "set_cylinder_radius: radius must be positive"}"#.into()),
    ]);
    let port = c.endpoint.port;
    assert_eq!(c.clearance("b1", &[3, 4], 0.2).unwrap().history.undo, ["Clearance"]);
    let spec = FastenerSpec { size: "M4".into(), kind: "counterbore".into(), extra_clearance: 0.2, depth: None };
    assert_eq!(spec.label(), "M4 counterbore");
    assert_eq!(FastenerSpec::default().label(), "M3 clearance");
    c.fastener_hole("b1", 2, [5.0, 6.0, 10.0], &spec).unwrap();
    let mut options = Map::new();
    options.insert("printer".into(), json!("bambu-a1-mini"));
    options.insert("joint".into(), json!("dovetail"));
    let split = c.print_split_op("b1", &options).unwrap();
    assert_eq!((split.result, split.history.undo), (json!("g7"), vec!["Split for printing".to_string()]));
    let mut unknown = Map::new();
    unknown.insert("colour".into(), json!("red"));
    let e = c.print_split_op("b1", &unknown).unwrap_err();
    // A mutating 5xx preserves the server refusal and status while keeping
    // the source outcome uncertain: the command may have committed first.
    assert_eq!((e.method, e.route.as_str(), e.status), ("POST", "/ops/print_split", Some(500)));
    assert_eq!(e.message.strip_suffix(": RoboCAD may still apply it; refresh before retrying"), Some("TypeError: SplitOptions.__init__() got an unexpected keyword argument 'colour'"));
    let e = c.clearance("b1", &[9], 0.2).unwrap_err();
    assert_eq!((e.status, e.message.as_str()), (Some(400), "face index 9 out of range (0..5)"));
    let e = c.clearance("b1", &[3], -9.0).unwrap_err();
    assert_eq!((e.method, e.status, e.message.as_str()), ("POST", Some(422), "set_cylinder_radius: radius must be positive"));
    let seen = server.join().unwrap();
    assert_request(&seen[0], "POST /ops/clearance HTTP/1.1", port, Some(r#"{"args":["b1",[{"face":3,"node":"b1"},{"face":4,"node":"b1"}],0.2],"kwargs":{}}"#));
    assert_request(&seen[1], "POST /ops/fastener_hole HTTP/1.1", port, Some(r#"{"args":["b1",{"face":2,"node":"b1"},[5.0,6.0,10.0],{"depth":null,"extra_clearance":0.2,"kind":"counterbore","size":"M4"}],"kwargs":{}}"#));
    assert_request(&seen[2], "POST /ops/print_split HTTP/1.1", port, Some(r#"{"args":["b1"],"kwargs":{"joint":"dovetail","printer":"bambu-a1-mini"}}"#));
    assert_request(&seen[3], "POST /ops/print_split HTTP/1.1", port, Some(r#"{"args":["b1"],"kwargs":{"colour":"red"}}"#));
    assert_request(&seen[4], "POST /ops/clearance HTTP/1.1", port, Some(r#"{"args":["b1",[{"face":9,"node":"b1"}],0.2],"kwargs":{}}"#));
    assert_request(&seen[5], "POST /ops/clearance HTTP/1.1", port, Some(r#"{"args":["b1",[{"face":3,"node":"b1"}],-9.0],"kwargs":{}}"#));
}

#[test]
fn split_errors_carry_robocad_text_and_status() {
    let (c, server) = serve(vec![
        Answer::Json(409, r#"{"error": "Expected document revision 4; current revision is 6. Fetch the current document and rebuild the candidate before applying it."}"#.into()),
        Answer::Json(422, r#"{"error": "split: node b1 is not a body"}"#.into()),
        // An unknown printer is a KeyError: 404, its repr as the text
        // (print_registry.py `printer`).
        Answer::Json(404, r#"{"error": "\"printer 'bambu-x1' is not in the print registry (have: bambu-h2c, bambu-p1s, bambu-a1-mini)\""}"#.into()),
        // The split job checks nothing before starting: queued, 200 ...
        ok(&started("7c1d2e3f4a", "split")),
        // ... and the unknown printer fails the job, `str(e)` as its error.
        ok(r#"{"id": "7c1d2e3f4a", "kind": "split", "state": "failed", "fraction": 0.0, "message": "", "error": "\"printer 'bambu-x1' is not in the print registry (have: bambu-h2c, bambu-p1s, bambu-a1-mini)\"", "result": null, "out_dir": null, "seconds": 0.01}"#),
        Answer::Json(409, r#"{"error": "Expected document revision 4; current revision is 6. Fetch the current document and rebuild the candidate before applying it."}"#.into()),
    ]);
    let port = c.endpoint.port;
    let e = c.print_split_now(&split_request()).unwrap_err();
    assert_eq!((e.status, e.message.as_str()), (Some(409), "Expected document revision 4; current revision is 6. Fetch the current document and rebuild the candidate before applying it."));
    let e = c.print_split_now(&SplitRequest { node: "b1".into(), ..SplitRequest::default() }).unwrap_err();
    assert_eq!((e.status, e.message.as_str(), e.route.as_str()), (Some(422), "split: node b1 is not a body", "/print/split"));
    let unknown_printer = "\"printer 'bambu-x1' is not in the print registry (have: bambu-h2c, bambu-p1s, bambu-a1-mini)\"";
    let bad = SplitRequest { node: "b1".into(), printer: Some("bambu-x1".into()), ..SplitRequest::default() };
    let e = c.print_split_now(&bad).unwrap_err();
    assert_eq!((e.status, e.message.as_str()), (Some(404), unknown_printer));
    let job = c.print_split_job(&bad).unwrap();
    assert_eq!((job.state.as_str(), job.error.as_deref()), ("queued", None), "the start succeeds whatever the body");
    let failed = c.print_job(&job.id).unwrap();
    assert!(failed.ended() && !failed.running());
    assert_eq!((failed.state.as_str(), failed.error.as_deref(), failed.result), ("failed", Some(unknown_printer), Value::Null));
    // The job-start routes take their snapshot first: a stale revision is a synchronous 409.
    let e = c.print_start("analyze", &json!({"expected_revision": 4, "parts": []})).unwrap_err();
    assert!(e.no_gui() && e.message.starts_with("Expected document revision 4"), "{e:?}");
    let seen = server.join().unwrap();
    assert_request(&seen[0], "POST /print/split HTTP/1.1", port, Some(r#"{"node":"b1","printer":"bambu-p1s","joint":"pins","expected_revision":4,"max_screws":4,"screw":"M4"}"#));
    assert_request(&seen[1], "POST /print/split HTTP/1.1", port, Some(r#"{"node":"b1"}"#));
    assert_request(&seen[2], "POST /print/split HTTP/1.1", port, Some(r#"{"node":"b1","printer":"bambu-x1"}"#));
    assert_request(&seen[3], "POST /print/split HTTP/1.1", port, Some(r#"{"node":"b1","printer":"bambu-x1","background":true}"#));
    assert_request(&seen[4], "GET /print/jobs/7c1d2e3f4a HTTP/1.1", port, None);
    assert_request(&seen[5], "POST /print/analyze HTTP/1.1", port, Some(r#"{"expected_revision":4,"parts":[]}"#));
}

#[test]
fn choice_lists_match_robocad_dialogs() {
    assert_eq!(SPLIT_JOINTS[0], "auto");
    assert!(FASTENER_SIZES.contains(&"M2.5") && FASTENER_KINDS.contains(&"insert"));
}
