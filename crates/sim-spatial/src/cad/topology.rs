//! Sub-body topology of the shown bodies: RoboCAD's faces, edges (with the
//! sampled polylines its viewport draws and picks, `GET
//! /nodes/{id}/edges?samples=N`) and vertices, cached by (node, revision).
//! Picking, box select, the overlays, snapping, measure, push/pull and the
//! inspector read it; nothing here changes the document.
//!
//! - **What is fetched** ([`wanted`]): the selected nodes always (the
//!   inspector's sub-body details), and every drawn body while a sub-body
//!   selection mode or a tool that snaps or picks faces is active.
//! - **Stale data is dropped**: an entry of another revision is removed the
//!   moment the shown revision changes (overlays then wait for the refetch
//!   rather than show old topology), and everything goes on a new
//!   connection or document.
//! - **How**: one `Pool::Dedicated` job per node (network: the jobs module's
//!   pool rule), at most [`MAX_FETCHES`] at once, three requests each. A
//!   node without B-rep geometry (RoboCAD's 404 "has no geometry", e.g. a
//!   mesh node) has an empty topology; another failure is kept and retried
//!   on the next revision, `cad_refresh` or a reconnection.
use super::document::{CadDocument, CadTool, SelectMode};
use super::mesh::BODY_KINDS;
use crate::jobs::{Job, Pool};
use bevy::prelude::*;
use sim_runtime::cad_client::{CadError, EdgeInfo, FaceInfo, VertexInfo};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Samples per curved edge, as RoboCAD's `sample_edges` default (lines get 2).
pub const EDGE_SAMPLES: u32 = 24;
/// Topology requests in flight at once.
pub const MAX_FETCHES: usize = 2;

/// One node's faces, edges and vertices at one revision (mm, RoboCAD's frame).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NodeTopology {
    pub revision: u64,
    pub faces: Vec<FaceInfo>,
    pub edges: Vec<EdgeInfo>,
    pub vertices: Vec<VertexInfo>,
}

enum Slot {
    Ready(Arc<NodeTopology>),
    Failed { revision: u64, error: String },
}

struct Fetch {
    id: String,
    revision: u64,
    job: Job<NodeTopology>,
}

/// CAD mode's topology cache (inserted on entering CAD mode, removed on leaving).
#[derive(Resource, Default)]
pub struct CadTopology {
    generation: u64,
    document_id: Option<String>,
    revision: u64,
    retry: u64,
    entries: HashMap<String, Slot>,
    fetching: Vec<Fetch>,
    /// Bumped whenever an entry is added, replaced or dropped (overlays and
    /// caches derived from the topology rebuild on it).
    pub epoch: u64,
}

impl CadTopology {
    /// Node `id`'s topology at the shown revision, once fetched.
    pub fn get(&self, id: &str) -> Option<&Arc<NodeTopology>> {
        match self.entries.get(id) {
            Some(Slot::Ready(t)) if t.revision == self.revision => Some(t),
            _ => None,
        }
    }
    /// Why node `id`'s topology could not be fetched at the shown revision.
    pub fn error(&self, id: &str) -> Option<&str> {
        match self.entries.get(id) {
            Some(Slot::Failed { revision, error }) if *revision == self.revision => Some(error),
            _ => None,
        }
    }
    /// Whether node `id`'s topology is being fetched.
    pub fn pending(&self, id: &str) -> bool {
        self.fetching.iter().any(|f| f.id == id)
    }
    /// The revision the cache holds (the shown tree's).
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Every ready node, at the shown revision.
    pub fn ready(&self) -> impl Iterator<Item = (&String, &Arc<NodeTopology>)> {
        self.entries.iter().filter_map(|(id, s)| match s {
            Slot::Ready(t) if t.revision == self.revision => Some((id, t)),
            _ => None,
        })
    }
    /// Put a fetched topology in directly (tests and callers that already hold one).
    pub fn insert(&mut self, id: &str, topology: NodeTopology) {
        self.revision = topology.revision;
        self.entries.insert(id.to_string(), Slot::Ready(Arc::new(topology)));
        self.epoch += 1;
    }
}

/// Whether the active tool reads sub-body topology (snapping, face targets, measuring).
pub fn tool_needs_topology(tool: CadTool) -> bool {
    !matches!(tool, CadTool::Select)
}

/// The nodes whose topology is wanted now: the selected ones, plus every
/// drawn body in a sub-body mode or with a tool that snaps or picks faces.
pub fn wanted(doc: &CadDocument) -> HashSet<String> {
    let mut out: HashSet<String> = doc.selected_nodes().into_iter().collect();
    // A catalogue pick or place tool snaps and picks over every drawn body.
    if doc.select_mode != SelectMode::Body || tool_needs_topology(doc.tool) || doc.ops.active.is_some() {
        if let Some(state) = &doc.doc {
            out.extend(state.nodes.iter().filter(|n| n.effective_visible && BODY_KINDS.contains(&n.kind.as_str())).map(|n| n.id.clone()));
        }
    }
    out
}

/// One node's topology, fetched (on a Dedicated job).
fn fetch(client: &sim_runtime::cad_client::CadClient, id: &str, revision: u64) -> Result<NodeTopology, String> {
    let absent = |e: &CadError| e.not_found();
    let faces = match client.faces(id) {
        Ok(f) => f,
        // RoboCAD: "<name> has no geometry" (a mesh, group or plane node).
        Err(e) if absent(&e) => return Ok(NodeTopology { revision, ..Default::default() }),
        Err(e) => return Err(e.to_string()),
    };
    let edges = client.edges(id, Some(EDGE_SAMPLES)).map_err(|e| e.to_string())?;
    let vertices = client.vertices(id).map_err(|e| e.to_string())?;
    Ok(NodeTopology { revision, faces, edges, vertices })
}

/// SimSync: bring the cache to the shown revision and the wanted nodes.
pub(super) fn sync(doc: Option<Res<CadDocument>>, topology: Option<ResMut<CadTopology>>, redraw: Option<MessageWriter<bevy::window::RequestRedraw>>) {
    let (Some(doc), Some(mut topology)) = (doc, topology) else { return };
    let t = &mut *topology;
    let document_id = doc.doc_key.as_ref().and_then(|k| k.0.clone());
    let revision = doc.doc_key.as_ref().map_or(0, |k| k.1);
    if t.generation != doc.generation || t.document_id != document_id {
        *t = CadTopology { generation: doc.generation, document_id, revision, retry: doc.mesh_retry, epoch: t.epoch + 1, ..default() };
    }
    if t.revision != revision {
        // Another revision: what was fetched for the old one is not shown.
        t.revision = revision;
        t.entries.clear();
        t.fetching.clear();
        t.epoch += 1;
    }
    if t.retry != doc.mesh_retry {
        // `cad_refresh` or a reconnection: failed nodes are fetched again.
        t.retry = doc.mesh_retry;
        t.entries.retain(|_, s| matches!(s, Slot::Ready(_)));
    }
    let wanted = wanted(&doc);
    t.fetching.retain(|f| f.revision == revision && f.job.generation() == doc.generation && wanted.contains(&f.id));
    let mut i = 0;
    while i < t.fetching.len() {
        let Some(result) = t.fetching[i].job.poll() else {
            i += 1;
            continue;
        };
        let Fetch { id, revision: at, .. } = t.fetching.swap_remove(i);
        let slot = match result {
            Ok(topology) => Slot::Ready(Arc::new(topology)),
            Err(error) => Slot::Failed { revision: at, error },
        };
        t.entries.insert(id, slot);
        t.epoch += 1;
    }
    if let Some(client) = doc.client.clone().filter(|_| doc.connected() && doc.stale.is_none()) {
        let mut ids: Vec<&String> = wanted.iter().filter(|id| !t.entries.contains_key(*id) && !t.fetching.iter().any(|f| &f.id == *id)).collect();
        ids.sort_unstable();
        for id in ids {
            if t.fetching.len() >= MAX_FETCHES {
                break;
            }
            let (client, node) = (client.clone(), id.clone());
            let job = Job::spawn(Pool::Dedicated, doc.generation, "cad-topology-fetch", move |_| fetch(&client, &node, revision));
            t.fetching.push(Fetch { id: id.clone(), revision, job });
        }
    }
    // Without a window (the core plugin's tests) there is no redraw message.
    if let (false, Some(mut redraw)) = (t.fetching.is_empty(), redraw) {
        redraw.write(bevy::window::RequestRedraw);
    }
}
