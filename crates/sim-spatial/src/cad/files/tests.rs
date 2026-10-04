//! The file workflows without a window: RoboCAD's export formats and their
//! settings pinned against io/exporters.py and ui/widgets.py's
//! `ExportDialog`, one validation for the form and REST, the import, render
//! and path rules, new and open under `cad_open`'s rule, the mesh unit
//! guess gating OK, the path form's drafts and the action it writes, the
//! listing, the kept REST answers, and every control and command action
//! round-tripping through its REST form.
use super::form::{self, FileForm, Kind};
use super::jobs;
use super::formats::{self, Context, FORMAT_IDS, FORMATS};
use super::*;
use crate::app::actions::{self, Action};
use crate::cad::document::{Edit, EditDone};
use crate::cad::rest_form::rest_form;
use sim_runtime::cad_client::{DocState, Health};

/// The formats `Service.export` takes (api.py: stl, 3mf, step, iges, obj,
/// svg, drawing) with the settings io/exporters.py's dataclasses and
/// `Service.export`'s drawing branch read, and the dialog's defaults.
#[test]
fn every_exporters_py_format_is_listed_with_its_settings_and_defaults() {
    assert_eq!(FORMAT_IDS, ["stl", "3mf", "step", "iges", "obj", "svg", "drawing"]);
    assert_eq!(FORMATS.iter().map(|f| f.id).collect::<Vec<_>>(), FORMAT_IDS);
    let pinned: [(&str, &[(&str, &str)]); 7] = [
        ("stl", &[("binary", "binary"), ("unit", "mm"), ("tolerance", "0.05"), ("angular_deg", "20")]),
        ("3mf", &[("tolerance", "0.05"), ("colors", "true"), ("names", "true")]),
        ("step", &[("schema", "AP214"), ("names", "true"), ("colors", "true")]),
        ("iges", &[]),
        ("obj", &[("tolerance", "0.05"), ("scale", "1"), ("up_axis", "Z"), ("quads", "false"), ("ngons", "false"), ("mtl", "true"), ("uvs", "true")]),
        ("svg", &[("sketch", "")]),
        ("drawing", &[("views", "front,top,right,iso"), ("title", ""), ("section", "")]),
    ];
    for (id, settings) in pinned {
        let f = formats::format(id).unwrap();
        assert_eq!(f.settings.iter().map(|s| (s.name, s.default)).collect::<Vec<_>>(), settings.to_vec(), "{id}");
    }
    // The dialog's labels.
    let label = |f: &str, s: &str| formats::format(f).unwrap().settings.iter().find(|x| x.name == s).unwrap().label;
    assert_eq!(label("stl", "binary"), "Format");
    assert_eq!(label("stl", "tolerance"), "Chord tolerance (mm)");
    assert_eq!(label("stl", "angular_deg"), "Angular tolerance (°)");
    assert_eq!(label("obj", "quads"), "Quads where possible");
    assert_eq!(label("3mf", "colors"), "Write colours");
    assert_eq!(formats::format("step").unwrap().extensions, ["step", "stp"]);
    assert_eq!(formats::format("iges").unwrap().extensions, ["iges", "igs"]);
}

#[test]
fn settings_fill_defaults_and_refuse_by_name() {
    let cx = Context { sketch: None, title: "turntable.rcad".into(), section: None };
    let stl = formats::format("stl").unwrap();
    let s = formats::settings(stl, &Map::new(), &cx).unwrap();
    assert_eq!(Value::Object(s), json!({"binary": true, "unit": "mm", "tolerance": 0.05, "angular_deg": 20.0}));
    let given = |v: Value| v.as_object().unwrap().clone();
    // A unit expression where a number goes, as the form's fields take them.
    let s = formats::settings(stl, &given(json!({"tolerance": "0.1mm", "binary": false})), &cx).unwrap();
    assert_eq!((s["tolerance"].clone(), s["binary"].clone()), (json!(0.1), json!(false)));
    let e = formats::settings(stl, &given(json!({"colour": true})), &cx).unwrap_err();
    assert!(e.contains("STL has no setting \"colour\"") && e.contains("binary, unit, tolerance, angular_deg"), "{e}");
    let e = formats::settings(stl, &given(json!({"tolerance": 2.0})), &cx).unwrap_err();
    assert!(e.contains("tolerance (Chord tolerance (mm))") && e.contains("outside"), "{e}");
    let e = formats::settings(stl, &given(json!({"unit": "yd"})), &cx).unwrap_err();
    assert!(e.contains("mm, cm, m, in, ft"), "{e}");
    assert!(formats::settings(formats::format("iges").unwrap(), &given(json!({"schema": "AP214"})), &cx).unwrap_err().contains("its settings: none"));
    // The sketch SVG: the selected sketch, else named.
    let svg = formats::format("svg").unwrap();
    assert!(formats::settings(svg, &Map::new(), &cx).unwrap_err().contains("select one"));
    let with_sketch = Context { sketch: Some("s1".into()), ..cx.clone() };
    assert_eq!(formats::settings(svg, &Map::new(), &with_sketch).unwrap()["sketch"], json!("s1"));
    // The drawing: four views, the file name, the section tool's plane while on.
    let drawing = formats::format("drawing").unwrap();
    let d = formats::settings(drawing, &Map::new(), &cx).unwrap();
    assert_eq!(Value::Object(d), json!({"views": ["front", "top", "right", "iso"], "title": "turntable.rcad"}));
    assert!(formats::settings(drawing, &given(json!({"section": true})), &cx).unwrap_err().contains("section tool is off"));
    let plane = json!({"origin": [0.0, 0.0, 5.0], "normal": [0.0, 0.0, 1.0], "x_axis": [1.0, 0.0, 0.0]});
    let on = Context { section: Some(plane.clone()), ..cx.clone() };
    assert_eq!(formats::settings(drawing, &Map::new(), &on).unwrap()["section"], plane);
    assert!(formats::settings(drawing, &given(json!({"section": false})), &on).unwrap().get("section").is_none());
    assert_eq!(formats::settings(drawing, &given(json!({"section": "xz"})), &cx).unwrap()["section"], json!("xz"));
    assert!(formats::settings(drawing, &given(json!({"views": ["front", "side"]})), &cx).unwrap_err().contains("front, top, right, iso"));
    assert!(formats::settings(drawing, &given(json!({"views": []})), &cx).is_err());
}

#[test]
fn paths_imports_and_renders_are_checked_before_anything_is_sent() {
    assert!(absolute("model.rcad", "Save As…").unwrap_err().contains("not an absolute path"));
    assert!(absolute("/tmp/", "Export").unwrap_err().contains("names a directory"));
    assert!(absolute("  ", "Export").unwrap_err().contains("needs a path"));
    if let Ok(home) = std::env::var("HOME") {
        assert_eq!(absolute("~/a.stl", "Export").unwrap(), format!("{}/a.stl", home.trim_end_matches('/')));
    }
    assert_eq!(import_args("/tmp/a.STEP", None).unwrap(), ("/tmp/a.STEP".to_string(), None));
    assert!(import_args("/tmp/a.stl", None).unwrap_err().contains("a mesh needs unit (mm, cm, m, in, ft)"));
    assert_eq!(import_args("/tmp/a.stl", Some("in")).unwrap().1.as_deref(), Some("in"));
    assert!(import_args("/tmp/a.stl", Some("yd")).unwrap_err().contains("not one of"));
    assert!(import_args("/tmp/a.step", Some("mm")).unwrap_err().contains("meshes only"));
    assert!(import_args("/tmp/a.dxf", None).unwrap_err().contains("RoboCAD imports"));
    let ok = RenderArgs { view: Some("1,-1,0.5".into()), w: Some(800), h: Some(600), mode: Some("xray".into()), section: Some("z:12.5".into()), ..Default::default() };
    assert_eq!(render_request(&ok).unwrap().route(), "/render?view=1%2C-1%2C0.5&w=800&h=600&mode=xray&section=z%3A12.5");
    for (bad, why) in [
        (RenderArgs { view: Some("sideways".into()), ..Default::default() }, "view"),
        (RenderArgs { mode: Some("matcap".into()), ..Default::default() }, "mode"),
        (RenderArgs { w: Some(8), ..Default::default() }, "w 8"),
        (RenderArgs { section: Some("q:1".into()), ..Default::default() }, "section"),
        (RenderArgs { tolerance: Some(0.0), ..Default::default() }, "tolerance"),
    ] {
        assert!(render_request(&bad).unwrap_err().contains(why), "{why}");
    }
}

/// An open document with (`dirty`) or without unsaved edits.
fn document(dirty: bool) -> CadDocument {
    let mut doc = CadDocument::new(CadTarget::File("/work/turntable.rcad".into()));
    doc.health = Some(Health { ok: true, app: "robocad".into(), dirty, path: Some("/work/turntable.rcad".into()), revision: 3, ..Default::default() });
    doc.doc = Some(DocState { revision: 3, ..Default::default() });
    doc.doc_key = Some((None, 3));
    doc.open_fixture();
    doc.doc.as_mut().unwrap().dirty = dirty;
    doc
}

#[test]
fn new_and_open_use_cad_opens_rule_and_never_discard() {
    let none = BTreeMap::new();
    let open = FileForm::new(Kind::File(FileOp::Open), "/work/", "turntable", None, &Context::default(), &none).unwrap();
    // A clean document: nothing to say.
    assert_eq!(form::open_rule(&open, &document(false)), None);
    // Unsaved edits are never discarded: opening another file is refused until they are saved.
    let why = form::open_rule(&open, &document(true)).unwrap().unwrap_err();
    assert!(why.contains("unsaved work"), "{why}");
    // An edit in flight: cad_open's refusal, shown before OK.
    let mut busy = document(false);
    busy.edit = Some(Edit { label: "Patch Bracket: visible".into(), job: crate::jobs::Job::finished(0, Ok(EditDone { message: String::new(), result: Value::Null })), started: std::time::Instant::now(), clear_selection: None, activates_plane: false, retarget: None });
    let why = form::open_rule(&open, &busy).unwrap().unwrap_err();
    assert!(why.contains("a CAD edit is in flight: Patch Bracket: visible"), "{why}");
    // Other forms say nothing; the REST form has no discard field.
    let save = FileForm::new(Kind::File(FileOp::SaveAs), "/work/", "turntable", None, &Context::default(), &none).unwrap();
    assert_eq!(form::open_rule(&save, &busy), None);
    let e = <CadAction as Action>::parse(&sim_api::Command { command: "cad_file".into(), args: json!({"op": "open", "path": "/a.rcad", "discard": true}) }).unwrap_err();
    assert!(e.contains("discard"), "{e}");
    assert_eq!(start_dir(&document(false)), ("/work/".to_string(), "turntable".to_string()));
}

#[test]
fn waited_answers_are_kept_by_count_not_by_distance() {
    let mut files = CadFiles::default();
    let open = jobs::Then::Open { path: "/a.rcad".into() };
    jobs::keep_result(&mut files, 3, (Ok(json!({"created": "/a.rcad"})), open.clone()));
    // Far newer jobs do not push out an answer whose caller still polls.
    jobs::keep_result(&mut files, 1_000, (Ok(Value::Null), jobs::Then::Nothing));
    assert_eq!(files.results.get(&3), Some(&(Ok(json!({"created": "/a.rcad"})), open)));
    for seq in 2_000..2_000 + jobs::KEPT_RESULTS as u64 {
        jobs::keep_result(&mut files, seq, (Ok(Value::Null), jobs::Then::Nothing));
    }
    assert_eq!(files.results.len(), jobs::KEPT_RESULTS);
    assert!(!files.results.contains_key(&3) && !files.results.contains_key(&1_000), "the oldest go first");
}

#[test]
fn the_export_form_starts_from_the_document_and_writes_the_rest_action() {
    let cx = Context { sketch: None, title: "turntable.rcad".into(), section: None };
    let mut remembered = BTreeMap::new();
    remembered.insert("step".to_string(), json!({"schema": "AP242", "names": false, "colors": true}).as_object().unwrap().clone());
    let mut form = FileForm::new(Kind::Export, "/work/", "turntable", None, &cx, &remembered).unwrap();
    assert_eq!((form.text("format"), form.text("path")), ("stl", "/work/turntable.stl"));
    assert_eq!(form.title(), "Export STL");
    let names: Vec<String> = form.rows().into_iter().map(|r| r.name).collect();
    assert_eq!(names, ["format", "path", "stl.binary", "stl.unit", "stl.tolerance", "stl.angular_deg"]);
    // A new format moves the extension and shows its settings, as last sent.
    form.set("format", "step".into());
    assert_eq!(form.text("path"), "/work/turntable.step");
    assert_eq!((form.text("step.schema"), form.text("step.names")), ("AP242", "false"));
    let CadAction::CadExport(args) = form.action().unwrap() else { panic!("not an export") };
    assert_eq!(args, ExportArgs { format: Some("step".into()), path: Some("/work/turntable.step".into()), settings: json!({"schema": "AP242", "names": false, "colors": true}).as_object().unwrap().clone(), ids: None });
    // The drawing: four view checkboxes and the section (off while the tool is).
    form.set("format", "drawing".into());
    form.set("drawing.view.iso", "false".into());
    let CadAction::CadExport(args) = form.action().unwrap() else { panic!("not an export") };
    assert_eq!(Value::Object(args.settings), json!({"views": ["front", "top", "right"], "title": "turntable.rcad", "section": false}));
    // A bad number keeps the form open with the reason.
    form.set("format", "stl".into());
    form.set("stl.tolerance", "abc".into());
    assert!(form.action().unwrap_err().starts_with("Chord tolerance (mm)"));
}

#[test]
fn the_file_forms_rows_units_guess_and_listing_picks() {
    let cx = Context::default();
    let none = BTreeMap::new();
    let mut import = FileForm::new(Kind::File(FileOp::Import), "/data/", "turntable", None, &cx, &none).unwrap();
    assert_eq!(import.rows().len(), 1);
    assert_eq!(import.ask_guess(), None, "a directory names no mesh");
    import.pick("/data", "scan.stl", false);
    assert_eq!(import.text("path"), "/data/scan.stl");
    assert_eq!(import.rows().iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["path", "unit"]);
    // A mesh path asks RoboCAD's guess once; until it lands OK is refused by name, never mm by default.
    assert_eq!(import.ask_guess().as_deref(), Some("/data/scan.stl"));
    assert_eq!(import.ask_guess(), None, "asked once per path");
    assert!(import.action().unwrap_err().starts_with("Choose the units of scan.stl (mm, cm, m, in, ft)"));
    assert!(!import.json()["ok_ready"].as_bool().unwrap());
    import.guessed("/elsewhere/scan.stl", &json!({"guess": "m"}));
    assert_eq!(import.text("unit"), "", "a guess for another path is ignored");
    import.guessed("/data/scan.stl", &json!({"guess": "in", "extent": 10.0}));
    assert_eq!(import.text("unit"), "in");
    assert!(import.json()["ok_ready"].as_bool().unwrap());
    import.set("unit", "cm".into());
    import.guessed("/data/scan.stl", &json!({"guess": "m"}));
    assert_eq!(import.text("unit"), "cm", "a unit chosen by hand is kept");
    assert_eq!(import.action().unwrap(), CadAction::CadFile(FileArgs { op: FileOp::Import, path: Some("/data/scan.stl".into()), unit: Some("cm".into()), job: None }));
    // Another mesh starts over: its own guess, or a unit chosen for it.
    import.set("path", "/data/part.obj".into());
    assert_eq!(import.ask_guess().as_deref(), Some("/data/part.obj"));
    assert!(import.action().is_err());
    import.set("unit", "mm".into());
    assert_eq!(import.action().unwrap(), CadAction::CadFile(FileArgs { op: FileOp::Import, path: Some("/data/part.obj".into()), unit: Some("mm".into()), job: None }));
    // A STEP file takes no unit and needs no guess.
    import.set("path", "/data/part.step".into());
    assert_eq!(import.ask_guess(), None);
    assert_eq!(import.action().unwrap(), CadAction::CadFile(FileArgs { op: FileOp::Import, path: Some("/data/part.step".into()), unit: None, job: None }));
    let mut save = FileForm::new(Kind::File(FileOp::SaveAs), "/work/", "turntable", None, &cx, &none).unwrap();
    assert_eq!(save.text("path"), "/work/turntable.rcad");
    save.pick("/work", "old", true);
    assert_eq!(save.text("path"), "/work/old/turntable.rcad", "a write keeps its file name when descending");
    save.up();
    save.up();
    assert_eq!(save.text("path"), "/turntable.rcad");
    let open = FileForm::new(Kind::File(FileOp::Open), "/work/", "turntable", None, &cx, &none).unwrap();
    assert!(open.action().unwrap_err().contains("file name"));
    assert_eq!(open.listing_key().unwrap().1, "/work/");
    let render = FileForm::new(Kind::Render, "/work/", "turntable", None, &cx, &none).unwrap();
    let CadAction::CadRender(r) = render.action().unwrap() else { panic!("not a render") };
    assert_eq!((r.path.as_deref(), r.view.as_deref(), r.w, r.h, r.edges, r.labels), (Some("/work/turntable-iso.png"), Some("iso"), Some(1200), Some(900), Some(true), Some(false)));
}

/// The form's listing is the kit path field's: its key is the path's
/// directory and the operation's extensions, a relative path asks none,
/// and the kit's reader lists those extensions (any case) and directories.
#[test]
fn the_forms_listing_is_the_kit_path_fields() {
    let cx = Context::default();
    let none = BTreeMap::new();
    let open = FileForm::new(Kind::File(FileOp::Open), "/work/", "turntable", None, &cx, &none).unwrap();
    assert_eq!(open.listing_key(), Some(("/work/|rcad".to_string(), "/work/".to_string())));
    let export = FileForm::new(Kind::Export, "/work/", "turntable", Some("stl"), &cx, &none).unwrap();
    assert_eq!(export.listing_key().map(|k| k.0).as_deref(), Some("/work/|stl"));
    let mut relative = open.clone();
    relative.set("path", "work/turntable.rcad".into());
    assert_eq!(relative.listing_key(), None);
    let dir = std::env::temp_dir().join(format!("cad-files-listing-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("Sub")).unwrap();
    for f in ["b.rcad", "A.RCAD", "notes.txt", ".hidden.rcad"] {
        std::fs::write(dir.join(f), b"x").unwrap();
    }
    let listing = crate::ui_kit::path_field::list("k".into(), dir.display().to_string(), &["rcad"]);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(listing.error, None);
    assert_eq!(listing.entries, vec![("Sub".to_string(), true), ("A.RCAD".to_string(), false), ("b.rcad".to_string(), false)]);
}

#[test]
fn controls_and_command_actions_round_trip_through_rest() {
    let doc = document(false);
    let mut files = CadFiles::default();
    files.form = Some(FileForm::new(Kind::File(FileOp::Open), "/work/", "turntable", None, &Context::default(), &BTreeMap::new()).unwrap());
    let controls = control_list(&doc, Some(&files));
    let ids: Vec<&str> = controls.iter().map(|c| c.0.as_str()).collect();
    assert_eq!(ids, ["cad:file:new", "cad:file:open", "cad:file:save_as", "cad:file:import", "cad:file:export", "cad:file:export_drawing", "cad:file:render", "cad:file:close"]);
    let mut actions_: Vec<CadAction> = controls.into_iter().map(|c| c.2).collect();
    for id in ["file.new", "file.open", "file.save_as", "file.import", "file.export", "file.export_drawing"] {
        actions_.push(command_action(id).unwrap_or_else(|| panic!("{id}")));
    }
    assert_eq!(command_action("file.quit"), None);
    actions_.push(CadAction::CadFile(FileArgs { op: FileOp::Import, path: Some("/a.stl".into()), unit: Some("in".into()), job: None }));
    actions_.push(CadAction::CadExport(ExportArgs { format: Some("stl".into()), path: Some("/a.stl".into()), settings: json!({"binary": false}).as_object().unwrap().clone(), ids: Some(vec!["n1".into()]) }));
    actions_.push(CadAction::CadRender(RenderArgs { path: Some("/a.png".into()), w: Some(640), tolerance: Some(0.1), labels: Some(true), ..Default::default() }));
    for action in actions_ {
        let Value::Object(mut args) = rest_form(&action) else { panic!("not an object") };
        let name = args.remove("command").and_then(|v| v.as_str().map(str::to_string)).unwrap();
        let parsed = <CadAction as Action>::parse(&sim_api::Command { command: name.clone(), args: Value::Object(args) }).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(parsed, action);
    }
    for (id, ..) in control_list(&doc, None) {
        assert!(<CadAction as Action>::controls().iter().any(|p| actions::control_matches(p, &id)), "{id}");
        assert_eq!(id.split(':').count(), 3, "{id}: one segment after cad:file:");
    }
    // Each spec's example parses (the registry test checks them all, too).
    for s in specs() {
        <CadAction as Action>::parse(&sim_api::Command { command: s.name.into(), args: s.example.clone() }).unwrap_or_else(|e| panic!("{}: {e}", s.name));
    }
}

#[test]
fn a_cancel_asks_exports_and_renders_and_says_what_it_can_stop() {
    let job = |seq: u64, kind: &'static str| jobs::FileJob { seq, kind, label: format!("{kind} {seq}"), job: crate::jobs::Job::finished(seq, Ok(Value::Null)), started: std::time::Instant::now(), waited: false, then: jobs::Then::Nothing, cancelled: false };
    let mut files = CadFiles::default();
    files.jobs = vec![job(1, "export"), job(2, "render"), job(3, "guess_unit")];
    // A read is not a write a cancel applies to; an unknown job is named.
    assert!(jobs::cancel(&mut files, Some(3)).unwrap_err().contains("no export or render job 3"), "the unit guess is a read");
    assert!(jobs::cancel(&mut files, Some(9)).unwrap_err().contains("job 9"));
    let render = jobs::cancel(&mut files, Some(2)).unwrap();
    assert_eq!(render["cancelled"], json!([2]));
    assert!(render["message"].as_str().unwrap().contains("does not write the PNG"), "{render}");
    let controls = control_list(&document(false), Some(&files));
    let cancel: Vec<(&str, bool)> = controls.iter().filter(|c| c.0.starts_with("cad:file:cancel-")).map(|c| (c.0.as_str(), c.3.is_ok())).collect();
    assert_eq!(cancel, [("cad:file:cancel-1", true), ("cad:file:cancel-2", false)], "asked once, then disabled saying so");
    // Without a job: every export and render; an export's note says RoboCAD writes it anyway.
    let all = jobs::cancel(&mut files, None).unwrap();
    assert_eq!(all["cancelled"], json!([1, 2]));
    assert!(all["message"].as_str().unwrap().contains("RoboCAD writes the file anyway"), "{all}");
    assert!(files.jobs.iter().all(|j| j.cancelled == (j.kind != "guess_unit")));
    // The control's action round-trips through REST; job belongs to cancel only.
    let action = CadAction::CadFile(FileArgs { op: FileOp::Cancel, job: Some(2), ..Default::default() });
    let Value::Object(mut args) = rest_form(&action) else { panic!("not an object") };
    let name = args.remove("command").and_then(|v| v.as_str().map(str::to_string)).unwrap();
    assert_eq!(<CadAction as Action>::parse(&sim_api::Command { command: name, args: Value::Object(args) }).unwrap(), action);
}
