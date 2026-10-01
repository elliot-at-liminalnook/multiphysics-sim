//! Sketch geometry and plane-node frames of the shown document, cached by
//! (node, revision): `GET /nodes/{id}` for every sketch node (its `sketch`,
//! the `GET /nodes/{id}/sketch` answer) and every plane node (its `plane`)
//! of the shown tree.
//! The sketch tools (which sketch a shape goes to, tools.py:675-686),
//! extrude (the source sketch's plane), the plane quads, the sketch curves
//! drawn, the snap's sketch endpoints and the active plane's frame read it.
//! Nothing here changes the document.
//!
//! - **Current and last**: [`CadSketches::sketch`] and
//!   [`CadSketches::plane`] answer only at the shown revision (the tools'
//!   decisions are never made on old geometry); [`CadSketches::sketch_last`]
//!   and [`CadSketches::plane_last`] also answer the last read of an older
//!   revision until the refetch lands, so the display does not blink on
//!   every edit (as a body's mesh is drawn from its last tessellation).
//! - **Malformed planes**: a plane node read whose `plane` is missing or
//!   malformed is ready, not "still being read": [`CadSketches::plane_state`]
//!   says so by name, and [`sync`] marks the active plane with it
//!   (`CadActivePlane::frame` then refuses "node X has no valid plane frame").
//! - **Dropped curves**: a sketch is read as `SketchGeometry::from_value`
//!   reads it (from `GET /nodes/{id}`'s `sketch`, the same `Sketch.to_json`
//!   as `GET /nodes/{id}/sketch`, whose client method discards the count):
//!   curves without a kind are dropped and counted ([`CadSketches::dropped`]),
//!   so the edits that name curves by index refuse instead of indexing a
//!   shortened list.
//! - **How**: one `Pool::Dedicated` job per node (network), at most
//!   [`MAX_FETCHES`] at once. A failure is kept with its error and retried
//!   on the next revision, `cad_refresh` or a reconnection. Everything goes
//!   on a new connection or document.
use super::CadActivePlane;
use crate::cad::document::CadDocument;
use crate::jobs::{Job, Pool};
use bevy::prelude::*;
use sim_runtime::cad_client::{CadClient, PlaneFrame, SketchGeometry, plane_of};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Requests in flight at once.
pub const MAX_FETCHES: usize = 2;

/// What one node holds.
#[derive(Clone, Debug, PartialEq)]
pub enum Geometry {
    Sketch(Arc<SketchGeometry>),
    /// A plane node's frame; None when its `plane` is missing or malformed.
    Plane(Option<PlaneFrame>),
}

#[derive(Clone, Debug)]
enum Slot {
    /// `dropped`: a sketch's curves the read dropped (no kind); 0 for a plane.
    Ready { revision: u64, geometry: Geometry, dropped: usize },
    Failed { revision: u64, error: String },
}

struct Fetch {
    id: String,
    revision: u64,
    job: Job<(Geometry, usize)>,
}

/// The cache (inserted on entering CAD mode, removed on leaving).
#[derive(Resource, Default)]
pub struct CadSketches {
    generation: u64,
    document_id: Option<String>,
    revision: u64,
    retry: u64,
    entries: HashMap<String, Slot>,
    /// The last ready geometry of each node, whatever its revision (display only).
    last: HashMap<String, Geometry>,
    fetching: Vec<Fetch>,
    /// Bumped whenever an entry is added, replaced or dropped.
    pub epoch: u64,
}

impl CadSketches {
    fn current(&self, id: &str) -> Option<&Geometry> {
        match self.entries.get(id) {
            Some(Slot::Ready { revision, geometry, .. }) if *revision == self.revision => Some(geometry),
            _ => None,
        }
    }
    /// Sketch node `id`'s geometry at the shown revision, once read.
    pub fn sketch(&self, id: &str) -> Option<&Arc<SketchGeometry>> {
        match self.current(id) {
            Some(Geometry::Sketch(s)) => Some(s),
            _ => None,
        }
    }
    /// Plane node `id`'s frame at the shown revision, once read (None also
    /// for a read whose `plane` is malformed: [`Self::plane_state`] tells them apart).
    pub fn plane(&self, id: &str) -> Option<PlaneFrame> {
        match self.current(id) {
            Some(Geometry::Plane(p)) => *p,
            _ => None,
        }
    }
    /// Plane node `id` at the shown revision: None while it is not read
    /// (being read, or its read failed: [`Self::error`]); Some(Err) naming
    /// the node when it was read but its `plane` is missing or malformed.
    pub fn plane_state(&self, id: &str) -> Option<Result<PlaneFrame, String>> {
        match self.current(id) {
            Some(Geometry::Plane(Some(f))) => Some(Ok(*f)),
            Some(Geometry::Plane(None)) => Some(Err(format!("node {id} has no valid plane frame (RoboCAD's answer has no well-formed `plane`)"))),
            _ => None,
        }
    }
    /// How many of sketch `id`'s curves the read at the shown revision
    /// dropped (no kind): its curve indices do not match RoboCAD's when non-zero.
    pub fn dropped(&self, id: &str) -> usize {
        match self.entries.get(id) {
            Some(Slot::Ready { revision, dropped, .. }) if *revision == self.revision => *dropped,
            _ => 0,
        }
    }
    /// The last read sketch of node `id`, possibly of an older revision (display only).
    pub fn sketch_last(&self, id: &str) -> Option<&Arc<SketchGeometry>> {
        match self.last.get(id) {
            Some(Geometry::Sketch(s)) => Some(s),
            _ => None,
        }
    }
    /// The last read frame of plane node `id`, possibly of an older revision (display only).
    pub fn plane_last(&self, id: &str) -> Option<PlaneFrame> {
        match self.last.get(id) {
            Some(Geometry::Plane(p)) => *p,
            _ => None,
        }
    }
    /// Why node `id` could not be read at the shown revision.
    pub fn error(&self, id: &str) -> Option<&str> {
        match self.entries.get(id) {
            Some(Slot::Failed { revision, error }) if *revision == self.revision => Some(error),
            _ => None,
        }
    }
    /// Whether node `id` is being read.
    pub fn pending(&self, id: &str) -> bool {
        self.fetching.iter().any(|f| f.id == id)
    }
    /// The revision the cache holds (the shown tree's).
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Put geometry in directly at `revision` (tests and callers that hold one).
    pub fn insert(&mut self, id: &str, revision: u64, geometry: Geometry) {
        self.insert_read(id, revision, geometry, 0);
    }
    /// [`Self::insert`] with the count of curves the read dropped.
    pub fn insert_read(&mut self, id: &str, revision: u64, geometry: Geometry, dropped: usize) {
        self.revision = revision;
        self.last.insert(id.to_string(), geometry.clone());
        self.entries.insert(id.to_string(), Slot::Ready { revision, geometry, dropped });
        self.epoch += 1;
    }
}

/// The sketch and plane nodes of the shown tree, with their kinds.
pub fn wanted(doc: &CadDocument) -> HashMap<String, bool> {
    let Some(state) = &doc.doc else { return HashMap::new() };
    state.nodes.iter().filter(|n| n.kind == "sketch" || n.kind == "plane").map(|n| (n.id.clone(), n.kind == "sketch")).collect()
}

/// One node's geometry and, for a sketch, the curves its read dropped (on
/// a Dedicated job). Both kinds read `GET /nodes/{id}`: its `sketch` is
/// `Sketch.to_json`, exactly `GET /nodes/{id}/sketch`'s answer (api.py
/// `node_detail`), read here with the dropped count kept.
fn fetch(client: &CadClient, id: &str, sketch: bool) -> Result<(Geometry, usize), String> {
    let detail = client.node(id).map_err(|e| e.to_string())?;
    if !sketch {
        return Ok((Geometry::Plane(plane_of(&detail)), 0));
    }
    let value = detail.sketch.as_ref().filter(|v| !v.is_null()).ok_or_else(|| format!("{id} is not a sketch"))?;
    let (g, dropped) = SketchGeometry::from_value(value).map_err(|m| format!("GET /nodes/{id}: unexpected sketch: {m}"))?;
    Ok((Geometry::Sketch(Arc::new(g)), dropped))
}

/// What the active plane's `unusable` mark should be (`CadActivePlane::frame`):
/// a plane node read at the shown revision without a valid frame.
pub(crate) fn unusable(plane: &CadActivePlane, cache: &CadSketches) -> Option<(String, String)> {
    match &plane.plane {
        Some(super::ActivePlane::Node { id, .. }) => match cache.plane_state(id) {
            Some(Err(why)) => Some((id.clone(), why)),
            _ => None,
        },
        _ => None,
    }
}

/// SimSync: bring the cache to the shown revision and the shown tree's
/// sketch and plane nodes.
pub(in crate::cad) fn sync(doc: Option<Res<CadDocument>>, cache: Option<ResMut<CadSketches>>, plane: Option<ResMut<CadActivePlane>>, redraw: Option<MessageWriter<bevy::window::RequestRedraw>>) {
    let (Some(doc), Some(mut cache)) = (doc, cache) else { return };
    let c = &mut *cache;
    let document_id = doc.doc_key.as_ref().and_then(|k| k.0.clone());
    let revision = doc.shown_revision();
    if c.generation != doc.generation || c.document_id != document_id {
        *c = CadSketches { generation: doc.generation, document_id, revision, retry: doc.mesh_retry, epoch: c.epoch + 1, ..default() };
    }
    if c.revision != revision {
        // Another revision: what was read for the old one is not current (`last` keeps it for display).
        c.revision = revision;
        c.entries.clear();
        c.fetching.clear();
        c.epoch += 1;
    }
    if c.retry != doc.mesh_retry {
        c.retry = doc.mesh_retry;
        c.entries.retain(|_, s| matches!(s, Slot::Ready { .. }));
    }
    let wanted = wanted(&doc);
    // A node gone from the tree is gone from the display too.
    let before = c.last.len();
    c.last.retain(|id, _| wanted.contains_key(id));
    if c.last.len() != before {
        c.epoch += 1;
    }
    c.fetching.retain(|f| f.revision == revision && f.job.generation() == doc.generation && wanted.contains_key(&f.id));
    let mut i = 0;
    while i < c.fetching.len() {
        let Some(result) = c.fetching[i].job.poll() else {
            i += 1;
            continue;
        };
        let Fetch { id, revision: at, .. } = c.fetching.swap_remove(i);
        let slot = match result {
            Ok((geometry, dropped)) => {
                c.last.insert(id.clone(), geometry.clone());
                Slot::Ready { revision: at, geometry, dropped }
            }
            Err(error) => Slot::Failed { revision: at, error },
        };
        c.entries.insert(id, slot);
        c.epoch += 1;
    }
    if let Some(client) = doc.client.clone().filter(|_| doc.connected() && doc.stale.is_none()) {
        let fetching: HashSet<&String> = c.fetching.iter().map(|f| &f.id).collect();
        let mut ids: Vec<(&String, bool)> = wanted.iter().filter(|(id, _)| !c.entries.contains_key(*id) && !fetching.contains(id)).map(|(id, s)| (id, *s)).collect();
        ids.sort_unstable();
        let starts: Vec<(String, bool)> = ids.into_iter().take(MAX_FETCHES.saturating_sub(c.fetching.len())).map(|(id, s)| (id.clone(), s)).collect();
        for (id, sketch) in starts {
            let (client, node) = (client.clone(), id.clone());
            let job = Job::spawn(Pool::Dedicated, doc.generation, "cad-sketch-fetch", move |_| fetch(&client, &node, sketch));
            c.fetching.push(Fetch { id, revision, job });
        }
    }
    // The active plane node read without a valid frame is said so (not "reading").
    if let Some(mut plane) = plane {
        let want = unusable(&plane, c);
        if plane.unusable != want {
            plane.unusable = want;
        }
    }
    if let (false, Some(mut redraw)) = (c.fetching.is_empty(), redraw) {
        redraw.write(bevy::window::RequestRedraw);
    }
}
