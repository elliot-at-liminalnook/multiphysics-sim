//! In-process source edits of the open archive (no service). One edit at a
//! time runs on a job: its work changes a copy of the manifest and entries
//! (`sim_cad::Edit`) against the current snapshot, the archive is rewritten
//! (`ArchiveDocument::apply`), and the next snapshot is rebuilt reusing every
//! unchanged body's exact geometry and display mesh (by fingerprint), so a
//! comment or a rename never re-tessellates. `sync::receive` lands it: the
//! previous snapshot goes on the undo stack. Undo and redo swap snapshots
//! (nothing recomputed); save writes the current archive's bytes atomically.
//!
//! The shown revision only moves forward (an undo shows a new revision of
//! older content), so caches keyed by revision never confuse two contents.
use super::document::{CadDocument, EditDone};
use super::sync::LocalSnapshot;
use crate::jobs::{Job, Pool};
use serde_json::{Value, json};
use sim_cad::geometry::BodyGeometry;
use crate::cad::types::MeshData;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// What an edit's work sees and changes (on the job thread).
pub(crate) struct Workspace<'a> {
    /// The archive as it is before this edit.
    pub archive: &'a sim_cad::ArchiveDocument,
    /// The current stamps of its pinned nodes (`annotations::Stamps`).
    pub stamps: &'a sim_cad::annotations::Stamps,
    /// Its bodies' exact properties (centroids for a move's default centre).
    pub geometry: &'a [BodyGeometry],
    /// The change being made.
    pub edit: sim_cad::Edit,
    pub cancelled: &'a dyn Fn() -> bool,
}
impl Workspace<'_> {
    /// Body `id`'s volume centroid (mm), or its bounding centre for a sheet.
    pub(crate) fn centroid(&self, id: &str) -> Option<[f64; 3]> {
        self.geometry.iter().find(|b| b.node_id == id).map(|b| b.properties.centroid_mm)
    }
}

/// The undo and redo stacks (label, the snapshot before / after) and the
/// identity of the archive as last opened or saved.
#[derive(Default)]
pub(crate) struct History {
    pub undo: Vec<(String, Arc<LocalSnapshot>)>,
    pub redo: Vec<(String, Arc<LocalSnapshot>)>,
    pub saved: Option<String>,
    /// The running edit's next snapshot, filled by its job.
    pending: Option<Arc<Mutex<Option<LocalSnapshot>>>>,
}
/// How many edits undo reaches back.
const DEPTH: usize = 200;

impl History {
    /// A new document: no history; `identity` is what is on disk.
    pub(crate) fn opened(identity: &str) -> Self {
        History { saved: Some(identity.to_string()), ..History::default() }
    }
    pub(crate) fn labels(&self) -> Value {
        json!({
            "undo": self.undo.iter().rev().map(|(l, _)| l).collect::<Vec<_>>(),
            "redo": self.redo.iter().rev().map(|(l, _)| l).collect::<Vec<_>>(),
        })
    }
}

/// Start an edit (refused by name with nothing changed while another runs,
/// during a preview unless `auxiliary`, or with no document): its sequence.
pub(crate) fn start(
    doc: &mut CadDocument,
    label: String,
    waited: bool,
    auxiliary: bool,
    work: impl FnOnce(&mut Workspace) -> Result<EditDone, String> + Send + 'static,
) -> Result<u64, String> {
    if let Some(why) = doc.edit_refusal_for(auxiliary) {
        return Err(why);
    }
    let current = doc.local.clone().ok_or("no CAD document is open")?;
    let slot: Arc<Mutex<Option<LocalSnapshot>>> = Arc::default();
    let out = slot.clone();
    let job = Job::spawn(Pool::Dedicated, doc.generation, format!("CAD edit: {label}"), move |ctx| {
        let cancelled = || ctx.cancelled();
        let mut ws = Workspace { archive: &current.archive, stamps: &current.pin_stamps, geometry: &current.geometry, edit: sim_cad::Edit::of(&current.archive), cancelled: &cancelled };
        let done = work(&mut ws)?;
        if ws.edit.manifest == current.archive.manifest && ws.edit.entries.is_empty() {
            return Ok(done);
        }
        ctx.message("Writing the archive");
        let archive = current.archive.apply(ws.edit)?;
        let next = rebuild(&current, archive, &cancelled, &|m| ctx.message(m))?;
        *out.lock().map_err(|_| "edit result lock poisoned")? = Some(next);
        Ok(done)
    });
    doc.edit_seq += 1;
    doc.edit_waited = waited;
    doc.history.pending = Some(slot);
    doc.edit = Some(super::document::Edit { label, job, started: Instant::now(), clear_selection: None, activates_plane: false, retarget: None });
    doc.touch();
    Ok(doc.edit_seq)
}

/// The snapshot of `archive`, the edit of `old`'s: unchanged bodies keep
/// their exact geometry and display mesh; mass properties are derived again.
pub(crate) fn rebuild(old: &LocalSnapshot, archive: sim_cad::ArchiveDocument, cancelled: &dyn Fn() -> bool, progress: &dyn Fn(&str)) -> Result<LocalSnapshot, String> {
    let tolerance = |a: &sim_cad::ArchiveDocument, id: &str| a.node(id).and_then(|n| n["tessellation_tolerance"].as_f64()).unwrap_or(0.05);
    let reuse: HashMap<String, BodyGeometry> = old
        .geometry
        .iter()
        .filter_map(|b| Some((sim_cad::geometry::reuse_key(old.fingerprints.get(&b.node_id)?, tolerance(&old.archive, &b.node_id)), b.clone())))
        .collect();
    let tree = super::sync::tree(&archive)?;
    let fingerprints = sim_cad::geometry::fingerprints(&archive);
    let geometry = sim_cad::geometry::load_geometry_reusing(&archive, &reuse, cancelled, progress)?;
    progress("Deriving exact body and assembly mass properties");
    let masses = sim_cad::mass::derive_document_with(&archive, &geometry, cancelled, progress)?;
    let meshes = geometry
        .iter()
        .map(|body| {
            let id = &body.node_id;
            let same = old.fingerprints.get(id).is_some_and(|f| fingerprints.get(id) == Some(f)) && tolerance(&old.archive, id) == tolerance(&archive, id);
            let mesh = match old.meshes.get(id).filter(|_| same) {
                Some(m) => m.clone(),
                None => mesh(body),
            };
            (id.clone(), mesh)
        })
        .collect();
    // Pins' stamps: kept for nodes whose geometry did not change, computed for the rest.
    let mut pin_stamps = sim_cad::annotations::Stamps::new();
    for id in archive.manifest["annotations"].as_object().into_iter().flatten().filter_map(|(_, t)| t["anchor"]["node_id"].as_str()) {
        if pin_stamps.contains_key(id) || archive.node(id).is_none() {
            continue;
        }
        let same = old.fingerprints.get(id).is_some_and(|f| fingerprints.get(id) == Some(f));
        let s = match old.pin_stamps.get(id).filter(|_| same) {
            Some(s) => s.clone(),
            None => sim_cad::annotations::current_stamps(&archive, &sim_cad::annotations::Stamps::new(), id),
        };
        pin_stamps.insert(id.to_string(), s);
    }
    Ok(LocalSnapshot { archive: Arc::new(archive), tree, geometry, meshes, masses, fingerprints, pin_stamps })
}

/// Show `snapshot` as the document at the next revision.
pub(crate) fn install(doc: &mut CadDocument, snapshot: Arc<LocalSnapshot>) {
    let revision = doc.shown_revision() + 1;
    let mut tree = snapshot.tree.clone();
    tree.revision = revision;
    tree.dirty = doc.history.saved.as_deref() != Some(snapshot.archive.identity());
    doc.doc_key = Some((tree.document_id.clone(), revision));
    doc.physical = Some((revision, Ok(physical_json(&snapshot))));
    doc.doc = Some(tree);
    let old = doc.local.replace(snapshot);
    crate::jobs::drop_off_thread(old, "the previous CAD snapshot");
    doc.detail = None;
    doc.threads.read.again();
    doc.touch();
}

/// The finished edit's next snapshot, if it changed anything.
pub(crate) fn take_pending(doc: &mut CadDocument) -> Option<LocalSnapshot> {
    doc.history.pending.take().and_then(|slot| slot.lock().ok()?.take())
}

/// A landed edit labelled `label`: the shown snapshot goes on the undo stack.
pub(crate) fn landed(doc: &mut CadDocument, label: String, next: LocalSnapshot) {
    if let Some(before) = doc.local.clone() {
        doc.history.undo.push((label, before));
        if doc.history.undo.len() > DEPTH {
            doc.history.undo.remove(0);
        }
    }
    doc.history.redo.clear();
    install(doc, Arc::new(next));
}

/// Undo (`redo` false) or redo the last edit: the label, or None when there
/// is nothing to undo or redo.
pub(crate) fn step(doc: &mut CadDocument, redo: bool) -> Result<Option<String>, String> {
    if let Some(label) = doc.edit_label() {
        return Err(format!("another CAD edit is in flight: {label}"));
    }
    if doc.preview_read_only {
        return Err("Return to live CAD before undoing; captured and kinematic previews are read-only".into());
    }
    let current = doc.local.clone().ok_or("no CAD document is open")?;
    let (from, to) = if redo { (&mut doc.history.redo, &mut doc.history.undo) } else { (&mut doc.history.undo, &mut doc.history.redo) };
    let Some((label, snapshot)) = from.pop() else { return Ok(None) };
    to.push((label.clone(), current));
    install(doc, snapshot);
    Ok(Some(label))
}

/// Write the current archive to `path` (its own file when None) atomically;
/// a save to another path makes that the document's file.
pub(crate) fn save(doc: &mut CadDocument, path: Option<PathBuf>) -> Result<Value, String> {
    if let Some(label) = doc.edit_label() {
        return Err(format!("another CAD edit is in flight: {label}; save once it lands"));
    }
    let local = doc.local.clone().ok_or("no CAD document is open")?;
    let target = match (&path, &doc.target) {
        (Some(p), _) => p.clone(),
        (None, super::CadTarget::File(p)) => p.clone(),
        (None, super::CadTarget::Service(_)) => return Err("this document has no file yet: save with a path".into()),
    };
    if target.extension().is_none_or(|e| e != "rcad") {
        return Err(format!("{}: a CAD document is saved as .rcad", target.display()));
    }
    local.archive.save(&target)?;
    doc.history.saved = Some(local.archive.identity().to_string());
    if let Some(tree) = doc.doc.as_mut() {
        tree.dirty = false;
    }
    let retargeted = path.is_some() && doc.target != super::CadTarget::File(target.clone());
    if retargeted {
        doc.target = super::CadTarget::File(target.clone());
    }
    doc.show(Ok(format!("Saved {}", display(&target))));
    Ok(json!({"saved": target, "identity": local.archive.identity(), "bytes": local.archive.original_bytes.len(), "retargeted": retargeted}))
}

fn display(p: &Path) -> String {
    p.file_name().map_or_else(|| p.display().to_string(), |f| f.to_string_lossy().into_owned())
}

/// A display mesh from exact geometry (the open's and an edit's).
pub(crate) fn mesh(body: &BodyGeometry) -> Arc<MeshData> {
    let faces: Vec<i64> = body.triangle_faces.iter().map(|f| i64::from(*f)).collect();
    Arc::new(MeshData { vertices: body.vertices_mm.clone(), triangles: body.triangles.clone(), face_count: faces.iter().max().map_or(0, |f| (*f + 1) as u64), triangle_face: faces })
}

/// Node `id`'s exact mass block (RoboCAD's `node_detail` `mass`): volume,
/// mass from its material, centroid and bounding box (mm, g). None without geometry.
pub(crate) fn mass_block(local: &LocalSnapshot, id: &str) -> Option<crate::cad::types::MassBlock> {
    let g = local.geometry.iter().find(|b| b.node_id == id)?;
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for v in &g.vertices_mm {
        for i in 0..3 {
            lo[i] = lo[i].min(v[i]);
            hi[i] = hi[i].max(v[i]);
        }
    }
    let mass_g = local.masses.bodies.get(id).map(|m| m.mass_kg * 1000.);
    Some(crate::cad::types::MassBlock {
        volume_mm3: Some(g.properties.volume_mm3),
        area_mm2: None,
        mass_g,
        centroid: g.properties.centroid_mm.iter().map(|v| Some(*v)).collect(),
        bbox_min: lo.iter().map(|v| Some(*v)).collect(),
        bbox_max: hi.iter().map(|v| Some(*v)).collect(),
        size: (0..3).map(|i| Some(hi[i] - lo[i])).collect(),
    })
}

/// What `cad_state.physical` and the materials dialog read: the exact masses
/// with the document's materials by id (RoboCAD's `/physical` `materials`).
pub(crate) fn physical_json(snapshot: &LocalSnapshot) -> serde_json::Value {
    let mut v = super::sync::value(&snapshot.masses);
    let materials: serde_json::Map<String, serde_json::Value> = snapshot.archive.manifest["materials"].as_array().into_iter().flatten().filter_map(|m| Some((m["id"].as_str()?.to_string(), m.clone()))).collect();
    v["materials"] = serde_json::Value::Object(materials);
    v
}

/// Tests: an in-process archive holding `tree`'s nodes (no geometry), so a
/// fixture document built from a `DocState` edits through the same local
/// path as an opened file. Roots and children follow the tree when it names
/// them, else its parents.
#[cfg(test)]
pub(crate) fn test_snapshot(tree: &crate::cad::types::DocState) -> LocalSnapshot {
    let empty = sim_cad::edit::empty_archive(None).expect("an empty archive");
    let path = Path::new("/test/fixture.rcad");
    let archive = sim_cad::ArchiveDocument::from_bytes(path, empty, &|| false, &|_| {}).expect("the empty archive reads");
    let mut edit = sim_cad::edit::Edit::of(&archive);
    let nodes: Vec<Value> = tree
        .nodes
        .iter()
        .map(|n| {
            let mut v = serde_json::to_value(n).expect("a node serializes");
            // Component occurrences need their definitions in the graph; the fixture keeps only the tree.
            if let Some(o) = v.as_object_mut() {
                o.remove("component_instance");
                o.remove("component_member");
            }
            if n.children.is_empty() {
                v["children"] = json!(tree.nodes.iter().filter(|c| c.parent.as_deref() == Some(n.id.as_str())).map(|c| &c.id).collect::<Vec<_>>());
            }
            v
        })
        .collect();
    let roots: Vec<&String> = if tree.roots.is_empty() { tree.nodes.iter().filter(|n| n.parent.is_none()).map(|n| &n.id).collect() } else { tree.roots.iter().collect() };
    edit.manifest["nodes"] = json!(nodes);
    edit.manifest["roots"] = json!(roots);
    edit.manifest["materials"] = if tree.materials.is_empty() { json!([{"id": "pla", "name": "PLA", "density": 1240.0}]) } else { json!(tree.materials) };
    let archive = archive.apply(edit).expect("the fixture archive applies");
    let masses = sim_cad::mass::derive_document_with(&archive, &[], &|| false, &|_| {}).expect("an empty assembly derives");
    LocalSnapshot { archive: Arc::new(archive), tree: tree.clone(), geometry: Vec::new(), meshes: HashMap::new(), masses, fingerprints: HashMap::new(), pin_stamps: Default::default() }
}

#[cfg(test)]
impl CadDocument {
    /// Tests: open the fixture's tree in process (`test_snapshot`) and mark
    /// the document shown, as a landed local open leaves it.
    pub(crate) fn open_fixture(&mut self) {
        let tree = self.doc.clone().unwrap_or_default();
        self.local = Some(Arc::new(test_snapshot(&tree)));
        self.connection = super::document::Connection::Connected;
    }
}
