//! Sketch geometry and plane-node frames of the shown document, cached by
//! (node, revision): `GET /nodes/{id}/sketch` for every sketch node and
//! `GET /nodes/{id}` (its `plane`) for every plane node of the shown tree.
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
//! - **How**: one `Pool::Dedicated` job per node (network), at most
//!   [`MAX_FETCHES`] at once. A failure is kept with its error and retried
//!   on the next revision, `cad_refresh` or a reconnection. Everything goes
//!   on a new connection or document.
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
    Ready { revision: u64, geometry: Geometry },
    Failed { revision: u64, error: String },
}

struct Fetch {
    id: String,
    revision: u64,
    job: Job<Geometry>,
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
            Some(Slot::Ready { revision, geometry }) if *revision == self.revision => Some(geometry),
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
    /// Plane node `id`'s frame at the shown revision, once read.
    pub fn plane(&self, id: &str) -> Option<PlaneFrame> {
        match self.current(id) {
            Some(Geometry::Plane(p)) => *p,
            _ => None,
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
        self.revision = revision;
        self.last.insert(id.to_string(), geometry.clone());
        self.entries.insert(id.to_string(), Slot::Ready { revision, geometry });
        self.epoch += 1;
    }
}

/// The sketch and plane nodes of the shown tree, with their kinds.
pub fn wanted(doc: &CadDocument) -> HashMap<String, bool> {
    let Some(state) = &doc.doc else { return HashMap::new() };
    state.nodes.iter().filter(|n| n.kind == "sketch" || n.kind == "plane").map(|n| (n.id.clone(), n.kind == "sketch")).collect()
}

/// One node's geometry (on a Dedicated job).
fn fetch(client: &CadClient, id: &str, sketch: bool) -> Result<Geometry, String> {
    if sketch {
        client.sketch(id).map(|s| Geometry::Sketch(Arc::new(s))).map_err(|e| e.to_string())
    } else {
        client.node(id).map(|d| Geometry::Plane(plane_of(&d))).map_err(|e| e.to_string())
    }
}

/// SimSync: bring the cache to the shown revision and the shown tree's
/// sketch and plane nodes.
pub(in crate::cad) fn sync(doc: Option<Res<CadDocument>>, cache: Option<ResMut<CadSketches>>, redraw: Option<MessageWriter<bevy::window::RequestRedraw>>) {
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
            Ok(geometry) => {
                c.last.insert(id.clone(), geometry.clone());
                Slot::Ready { revision: at, geometry }
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
    if let (false, Some(mut redraw)) = (c.fetching.is_empty(), redraw) {
        redraw.write(bevy::window::RequestRedraw);
    }
}
