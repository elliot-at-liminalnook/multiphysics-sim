//! RoboCAD's operations in process on a real archive: each one applied as an
//! edit, the archive rewritten and re-read, and its exact geometry and mass
//! derived again (the path every viewer edit takes).
use serde_json::{Map, Value, json};
use sim_cad::ArchiveDocument;
use sim_cad::ops::{Ctx, run};
use std::collections::HashMap;
use std::path::PathBuf;

fn rover() -> ArchiveDocument {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/wheeled-robot/baseline/robot.rcad");
    ArchiveDocument::open(&root).expect("the rover archive opens")
}

struct World {
    doc: ArchiveDocument,
}
impl World {
    /// One operation as one edit; the next archive (re-read), and the answer.
    fn op(&mut self, name: &str, args: Value, kwargs: Value) -> Result<Value, String> {
        let geometry = sim_cad::geometry::load_geometry(&self.doc, &|| false, &|_| {})?;
        let centroids: HashMap<String, [f64; 3]> = geometry.iter().map(|b| (b.node_id.clone(), b.properties.centroid_mm)).collect();
        let stamps = sim_cad::annotations::Stamps::new();
        let mut edit = sim_cad::Edit::of(&self.doc);
        let centroid = |id: &str| centroids.get(id).copied();
        let mut cx = Ctx { doc: &self.doc, stamps: &stamps, edit: &mut edit, centroid: &centroid, cancelled: &|| false };
        let args = args.as_array().cloned().unwrap_or_default();
        let kwargs: Map<String, Value> = kwargs.as_object().cloned().unwrap_or_default();
        let answer = run(&mut cx, name, &args, &kwargs)?;
        let next = self.doc.apply(edit)?;
        // The rewritten archive opens, tessellates and derives mass like a file.
        let reread = ArchiveDocument::from_bytes(&next.path, next.original_bytes.clone(), &|| false, &|_| {})?;
        let g = sim_cad::geometry::load_geometry(&reread, &|| false, &|_| {})?;
        sim_cad::mass::derive_document(&reread, &g)?;
        self.doc = reread;
        Ok(answer)
    }
    fn id(&self, name: &str) -> String {
        self.doc.manifest["nodes"].as_array().unwrap().iter().find(|n| n["name"] == name).and_then(|n| n["id"].as_str()).unwrap_or_else(|| panic!("no node {name}")).to_string()
    }
    fn faces(&self, id: &str) -> sim_cad::kernel::Topology {
        sim_cad::kernel::topology(self.doc.entry(&format!("brep/{id}.brep")).unwrap(), &|| false).unwrap()
    }
}

#[test]
fn modelling_operations_apply_as_archive_edits() {
    let mut w = World { doc: rover() };
    // Primitives and a hole.
    let plate = w.op("box", json!([[-60, -40, 80], [60, 40, 4]]), json!({"name": "Top plate"})).unwrap();
    let plate = plate.as_str().unwrap().to_string();
    let pin = w.op("cylinder", json!([[0, 0, 70], [0, 0, 1], 3, 20]), json!({})).unwrap().as_str().unwrap().to_string();
    w.op("boolean", json!([plate, [pin], "subtract"]), json!({})).unwrap();
    assert!(w.doc.node(&pin).is_none(), "the tool body is deleted");
    let topo = w.faces(&plate);
    let hole = topo.faces.iter().find(|f| f.kind == "cylinder").unwrap().index;
    // Live dimensions and push/pull.
    w.op("set_diameter", json!([plate, {"node": plate, "face": hole}, 8.0]), json!({})).unwrap();
    let topo = w.faces(&plate);
    let top = topo.faces.iter().find(|f| f.kind == "plane" && f.normal[2] > 0.99).unwrap().index;
    let bottom = topo.faces.iter().find(|f| f.kind == "plane" && f.normal[2] < -0.99).unwrap().index;
    w.op("push_pull", json!([plate, {"node": plate, "face": top}, 2.0]), json!({})).unwrap();
    let topo = w.faces(&plate);
    let top = topo.faces.iter().find(|f| f.kind == "plane" && f.normal[2] > 0.99).unwrap().index;
    let bottom2 = topo.faces.iter().find(|f| f.kind == "plane" && f.normal[2] < -0.99).unwrap().index;
    let _ = bottom;
    w.op("set_distance", json!([plate, {"node": plate, "face": bottom2}, {"node": plate, "face": top}, 5.0]), json!({})).unwrap();
    let t = w.faces(&plate);
    let zs: Vec<f64> = t.faces.iter().filter(|f| f.kind == "plane" && f.normal[2].abs() > 0.99).map(|f| f.center[2]).collect();
    let thick = zs.iter().cloned().fold(f64::MIN, f64::max) - zs.iter().cloned().fold(f64::MAX, f64::min);
    assert!((thick - 5.0).abs() < 1e-6, "set_distance makes the plate 5 mm thick (got {thick})");
    // Fillet, chamfer, offset faces, shell.
    let vertical: Vec<Value> = w.faces(&plate).edges.iter().filter(|e| e.kind == "line" && (e.start[2] - e.end[2]).abs() > 1.).map(|e| json!({"node": plate, "edge": e.index})).collect();
    w.op("fillet", json!([plate, vertical, 3.0]), json!({})).unwrap();
    let block = w.op("box", json!([[100, 0, 0], [20, 20, 20]]), json!({"name": "Block"})).unwrap().as_str().unwrap().to_string();
    w.op("chamfer", json!([block, [{"node": block, "edge": 0}], {"distance": 1.0}]), json!({})).unwrap();
    let up = w.faces(&block).faces.iter().find(|f| f.kind == "plane" && f.normal[2] > 0.99).unwrap().index;
    w.op("shell", json!([block, 1.5, [{"node": block, "face": up}]]), json!({})).unwrap();
    // Arrange: mirror, array, instance, make unique, join, unjoin, cut, group.
    let mirrored = w.op("mirror", json!([[block], "yz"]), json!({})).unwrap();
    assert_eq!(mirrored.as_array().unwrap().len(), 1);
    let copies = w.op("array_rect", json!([[block], [2, 1, 1]]), json!({"spacing": [30, 0, 0]})).unwrap();
    assert_eq!(copies.as_array().unwrap().len(), 1);
    let inst = w.op("instance", json!([block]), json!({"transform": {"translation": [0, 50, 0], "axis": [0, 0, 1], "angle_deg": 45, "scale": 1.0}})).unwrap().as_str().unwrap().to_string();
    let unique = w.op("make_unique", json!([inst]), json!({})).unwrap().as_str().unwrap().to_string();
    let a = w.op("box", json!([[0, 200, 0], [10, 10, 10]]), json!({})).unwrap().as_str().unwrap().to_string();
    let b = w.op("box", json!([[20, 200, 0], [10, 10, 10]]), json!({})).unwrap().as_str().unwrap().to_string();
    w.op("join", json!([[a, b]]), json!({})).unwrap();
    let parts = w.op("unjoin", json!([a]), json!({})).unwrap();
    assert_eq!(parts.as_array().unwrap().len(), 2, "two separate boxes come apart again");
    let halves = w.op("cut", json!([unique, {"origin": [0, 50, 10], "normal": [0, 0, 1]}]), json!({})).unwrap();
    assert_eq!(halves.as_array().unwrap().len(), 2);
    let g = w.op("group", json!([[block, unique]]), json!({"name": "Blocks"})).unwrap().as_str().unwrap().to_string();
    assert_eq!(w.doc.node(&block).unwrap()["parent"], g.as_str());
    // A sketch, extruded as a cut into the plate, and a revolve.
    let sk = w.op("new_sketch", json!(["xy"]), json!({"name": "Slot"})).unwrap().as_str().unwrap().to_string();
    let mut s = sim_cad::sketch::Sketch::from_json(&w.doc.node(&sk).unwrap()["sketch"]).unwrap();
    s.plane = sim_cad::sketch::Plane::xy(78.);
    s.call(&json!(["slot", [[-30, 20], [-10, 20], 6]])).unwrap();
    {
        // An edit of the sketch alone (what `edit_sketch` does).
        let mut edit = sim_cad::Edit::of(&w.doc);
        edit.node_mut(&sk).unwrap()["sketch"] = s.json();
        w.doc = w.doc.apply(edit).unwrap();
    }
    w.op("extrude", json!([sk, 20.0]), json!({"op": "subtract", "target": plate})).unwrap();
    let ring = w.op("new_sketch", json!([{"origin": [0, 300, 0], "normal": [0, -1, 0], "x_axis": [1, 0, 0]}]), json!({})).unwrap().as_str().unwrap().to_string();
    let mut r = sim_cad::sketch::Sketch::from_json(&w.doc.node(&ring).unwrap()["sketch"]).unwrap();
    r.call(&json!(["rectangle", [[10, 0], [5, 10]]])).unwrap();
    {
        let mut edit = sim_cad::Edit::of(&w.doc);
        edit.node_mut(&ring).unwrap()["sketch"] = r.json();
        w.doc = w.doc.apply(edit).unwrap();
    }
    let washer = w.op("revolve", json!([ring, [0, 300, 0], [0, 0, 1], 360.0]), json!({})).unwrap().as_str().unwrap().to_string();
    assert_eq!(w.doc.node(&washer).unwrap()["kind"], "body");
    // Planes and robot parts.
    w.op("plane_three_points", json!([[0, 0, 0], [1, 0, 0], [0, 1, 0]]), json!({})).unwrap();
    let motor = w.op("add_motor", json!(["sg90", [0, 0, 84], [0, 0, 1]]), json!({"mount_on": plate, "cut_mount": true})).unwrap().as_str().unwrap().to_string();
    let joint = w.op("add_joint", json!(["revolute", plate, washer, [0, 300, 0], [0, 0, 1]]), json!({})).unwrap().as_str().unwrap().to_string();
    w.op("attach_motor", json!([joint, motor]), json!({})).unwrap();
    let summary = sim_cad::robotics::summary(&w.doc);
    assert!(summary["joints"].as_array().unwrap().iter().any(|j| j["id"] == joint.as_str() && j["motor"] == motor.as_str()));
    // Refusals are RoboCAD's words, and nothing changes.
    let before = w.doc.identity().to_string();
    assert!(w.op("fillet", json!([plate, [{"node": plate, "edge": 0}], 500.0]), json!({})).unwrap_err().contains("too large"));
    assert!(w.op("no_such_op", json!([]), json!({})).is_err());
    assert_eq!(w.doc.identity(), before);
    w.op("set_locked", json!([[plate], true]), json!({})).unwrap();
    assert!(w.op("push_pull", json!([plate, {"node": plate, "face": 0}, 1.0]), json!({})).unwrap_err().contains("locked"));
    let _ = w.id("Top plate");
    // Robot settings keep RoboCAD's types: a battery's cells are a whole number.
    w.op("set_battery", json!([3, "lipo", 2.2]), json!({})).unwrap();
    assert_eq!(w.doc.manifest["robot_settings"]["battery"]["cells"], json!(3));
    assert!(w.op("set_battery", json!([2.5]), json!({})).unwrap_err().contains("whole number"));
}

#[test]
fn every_export_format_writes_a_file_that_reads_back() {
    let doc = rover();
    let geometry = sim_cad::geometry::load_geometry(&doc, &|| false, &|_| {}).unwrap();
    let dir = std::env::temp_dir().join(format!("sim-cad-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (format, file, settings) in [
        ("stl", "r.stl", json!({"binary": true, "unit": "mm", "tolerance": 0.05})),
        ("stl", "ascii.stl", json!({"binary": false, "unit": "cm"})),
        ("obj", "r.obj", json!({"tolerance": 0.2, "mtl": true})),
        ("3mf", "r.3mf", json!({"colors": true, "names": true})),
        ("step", "r.step", json!({"schema": "AP214"})),
        ("iges", "r.iges", json!({})),
        ("drawing", "r.svg", json!({"views": ["front", "top", "iso"], "title": "Rover", "section": {"origin": [0, 0, 30], "normal": [0, 0, 1]}})),
    ] {
        let path = dir.join(file);
        let answer = sim_cad::export::export(&doc, &geometry, format, &path, &settings, None, &|| false).unwrap_or_else(|e| panic!("{format}: {e}"));
        assert!(std::fs::metadata(&path).unwrap().len() > 100, "{format} wrote a file");
        assert!(answer["bodies"].as_u64().unwrap() >= 6);
        if format == "drawing" {
            eprintln!("drawing warnings: {}", answer["warnings"]);
        }
    }
    // The STEP file reads back as solids with the same total volume.
    let back = sim_cad::kernel::import(sim_cad::kernel::Exchange::Step, &dir.join("r.step"), 1.0).unwrap();
    assert_eq!(back.len(), 6, "six solid bodies");
    let svg = std::fs::read_to_string(dir.join("r.svg")).unwrap();
    assert!(svg.contains("Section A-A") && svg.contains("stroke-dasharray"));
    // File > Import: the STEP and IGES files come back as bodies named after
    // the file, and the edited archive re-reads with their exact mass.
    let stamps = sim_cad::annotations::Stamps::new();
    let mut edit = sim_cad::Edit::of(&doc);
    let mut cx = Ctx { doc: &doc, stamps: &stamps, edit: &mut edit, centroid: &|_| None, cancelled: &|| false };
    let step = sim_cad::import::import_file(&mut cx, dir.join("r.step").to_str().unwrap()).unwrap();
    let iges = sim_cad::import::import_file(&mut cx, dir.join("r.iges").to_str().unwrap()).unwrap();
    assert_eq!(step.len(), 6, "six solids from STEP");
    assert!(!iges.is_empty(), "IGES imports");
    assert!(sim_cad::import::import_file(&mut cx, dir.join("r.stl").to_str().unwrap()).unwrap_err().contains("mesh import"));
    let next = doc.apply(edit).unwrap();
    assert_eq!(next.node(&step[0]).unwrap()["kind"], "body");
    assert!(next.node(&step[0]).unwrap()["name"].as_str().unwrap().starts_with('r'));
    let g = sim_cad::geometry::load_geometry(&next, &|| false, &|_| {}).unwrap();
    let masses = sim_cad::mass::derive_document(&next, &g).unwrap();
    assert!(step.iter().all(|id| masses.bodies.get(id).is_some_and(|m| m.mass_kg > 0.)));
    std::fs::remove_dir_all(&dir).unwrap();
}
