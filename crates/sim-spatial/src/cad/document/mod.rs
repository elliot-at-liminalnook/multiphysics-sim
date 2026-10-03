//! One CAD source owner: the shared immutable archive and its local exact
//! geometry/mass snapshot. Pending replacement is jobs-owned and only lands
//! against the captured document generation/revision. Legacy DTO and tool state
//! fields remain as migration scaffolding, never a connected native service.
mod state;
mod types;

pub use types::{CadTarget, CadTool, Candidates, ChildSlot, Connected, Connection, Edit, EditDone, PollCommand, PollSnapshot, SelectMode, TreeRow};

use crate::jobs::{Job, RunThread};
use bevy::prelude::*;
use serde_json::Value;
use sim_runtime::cad_client::{Autosave, CadClient, CommandInfo, DocState, Health, NodeDetail, SelectionItem};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// CAD mode's document (present only in CAD mode; removed on exit).
#[derive(Resource)]
pub struct CadDocument {
    /// Local archive ownership; opaque contents remain in the shared document.
    pub(crate) local: Option<std::sync::Arc<super::sync::LocalSnapshot>>,
    /// Durable pending replacement. The current snapshot stays until success.
    pub(crate) local_load: Option<super::sync::LocalLoad>,
    pub(crate) load_sequence: u64,
    pub(crate) load_outcomes: HashMap<u64, Result<Value, String>>,
    /// Bumped on every (re)connect and open: results of an older generation
    /// are dropped.
    pub generation: u64,
    pub target: CadTarget,
    /// Legacy unmigrated control seam. Native local loading NEVER fills it.
    pub client: Option<CadClient>,
    /// The service this window started (empty when attached): the only
    /// RoboCAD the viewer ever stops. Shared with the connect job, which
    /// fills it as soon as it spawns the process ([`ChildSlot`]).
    pub child: ChildSlot,
    pub connection: Connection,
    /// The latest `GET /` (revision, dirty, gui, document id).
    pub health: Option<Health>,
    /// The latest `/doc` (tree, materials, selection, history).
    pub doc: Option<DocState>,
    /// The (document id, revision) `doc` shows.
    pub doc_key: Option<(Option<String>, u64)>,
    /// Why the shown document may be behind RoboCAD's ("refetching revision
    /// 12", or a failed refetch's error). None when it is current.
    pub stale: Option<String>,
    // The selection itself is the shared one (`crate::selection::Selection`,
    // `Item::Cad` under CAD's registry entry: `cad::selection::Shared`);
    // what follows is RoboCAD's echo of it and the viewer's mode and hover.
    /// The selection mode (what a 3D click picks). The viewer holds it: a
    /// headless RoboCAD stores only the items (api.py `set_selection`), a
    /// desktop one's `mode` is adopted when it changes there.
    pub select_mode: SelectMode,
    /// The item under the pointer (display only: the hover highlight).
    pub hover: Option<SelectionItem>,
    /// The Alt+click menu, while open.
    pub candidates: Option<Candidates>,
    /// The active tool.
    pub tool: CadTool,
    /// The active tool's own state: its target, pivot, the revision its
    /// preview began at, the numeric fields (`transform::ToolState`).
    pub tool_state: super::transform::ToolState,
    /// The op catalogue's state: the open form, the active interaction,
    /// the open command surface, the last copy and the analysis overlays
    /// (cad-modify, `ops`).
    pub ops: super::ops::OpsState,
    /// Why the poll's last `GET /selection` failed (None once one succeeds):
    /// RoboCAD's own selection changes are not seen meanwhile.
    pub selection_error: Option<String>,
    /// The inspected node's `GET /nodes/{id}`: (id, revision fetched at, result).
    pub detail: Option<(String, u64, Result<NodeDetail, String>)>,
    pub commands: Option<Result<BTreeMap<String, CommandInfo>, String>>,
    pub autosave: Option<Result<Autosave, String>>,
    /// `GET /physical?flex=0` for the inspector's physical section: (revision, result).
    pub physical: Option<(u64, Result<Value, String>)>,
    /// cad-physical-inspect: RoboCAD's robot description, per-node results,
    /// sensors, cables, settings and motor library as last read
    /// (`robot::data`), with the Robot panel's and the robot tools' state.
    pub robot: super::robot::RobotState,
    /// cad-physical-inspect: the materials panel (search, the open form).
    pub materials: super::materials::MaterialsState,
    /// cad-physical-inspect: the inspector's physical rows (drafts, the
    /// exact measurement job).
    pub physical_edit: super::inspector::PhysicalEdit,
    /// cad-physical-inspect: results, the stress overlay, physical export
    /// and the live link.
    pub results: super::results::ResultsState,
    /// cad-print: the checks, the remembered print values, the registry
    /// and study reads, and RoboCAD's print jobs as last polled.
    pub print: super::print::PrintState,
    /// cad-organize: the outliner's search, collapse state, range anchor,
    /// rename and drag (`tree::TreeState`; display state only).
    pub tree: super::tree::TreeState,
    /// cad-organize: RoboCAD's threads as last read, the Comments dock,
    /// the composer, pins and the temporary isolation (`threads::ThreadsState`).
    pub threads: super::threads::ThreadsState,
    /// cad-organize: the References dock, image textures, the calibrate
    /// tool and the linked system file's status (`references::ReferencesState`).
    pub references: super::references::ReferencesState,
    /// The mutating request in flight.
    pub edit: Option<Edit>,
    /// Service-owned component preparation held by the persistent components
    /// feature. Blocks source edits and document replacement until acknowledged.
    pub component_busy: Option<String>,
    /// Display-only captured/kinematic view; source geometry edits are refused.
    pub preview_read_only: bool,
    pub uncertain_edit: Option<String>,
    pub uncertain_history: Vec<Value>,
    /// The latest outcome line (Ok message or the refusal / error).
    pub status: Option<Result<String, String>>,
    /// Bumped whenever anything the panels show changes.
    pub revision: u64,
    // Work owned by `sync`.
    pub(super) connect: Option<Job<Connected>>,
    pub(super) poll: Option<RunThread<PollCommand, PollSnapshot>>,
    pub(super) seen_poll: u64,
    pub(super) detail_job: Option<Job<NodeDetail>>,
    pub(super) physical_job: Option<Job<Value>>,
    pub(super) selection_job: Option<Job<Vec<SelectionItem>>>,
    /// The (node id, revision) the detail job fetches or fetched.
    pub(super) detail_key: Option<(String, u64)>,
    /// The revision the physical job was started at.
    pub(super) physical_revision: u64,
    /// The last selection read from RoboCAD (adopted once when it changes).
    pub(super) remote_selection: Vec<SelectionItem>,
    /// Whether this connection has read RoboCAD's selection yet: its first
    /// read is adopted even when empty, so items the shared selection kept
    /// from an earlier visit to CAD mode never stay shown while RoboCAD
    /// holds another selection.
    pub(super) selection_read: bool,
    /// The last selection mode read from RoboCAD (a desktop window's; a
    /// headless service sends none), adopted once when it changes.
    pub(super) remote_mode: Option<SelectMode>,
    /// When our latest selection push was answered: a poll's selection read
    /// sent before then is not adopted (it predates the push).
    pub(super) selection_pushed_at: Option<Instant>,
    /// The selection changed while a push was in flight: the newest (the
    /// shared selection's CAD items) is pushed when that push answers, so
    /// pushes never overlap.
    pub(super) selection_again: bool,
    /// The shared selection's change count (`Selection::changed`) when CAD
    /// last pushed or adopted it: a count past it is a change not yet
    /// published (`selection::publish_changes`).
    pub(super) published_selection: u64,
    /// Bumped by `cad_refresh` and a Lost → Connected transition: the mesh
    /// cache then retries the bodies whose fetch or build failed.
    pub(super) mesh_retry: u64,
    /// The sequence of the latest edit, and whether a REST caller waits for it.
    pub(super) edit_seq: u64,
    pub(super) edit_waited: bool,
    /// Finished edits a REST caller waits for: seq → its answer.
    pub(super) edit_results: HashMap<u64, Result<Value, String>>,
    /// After an edit finishes, `health.dirty` is unknown until the poll has
    /// published this sequence (a `GET /` sent after the edit's answer).
    pub(super) dirty_known_at: Option<u64>,
    /// The URL the client talks to (the target's, or the self-started port).
    pub(super) url: Option<String>,
    /// The self-started service's stderr log (`service::log_path`).
    pub(super) log: Option<PathBuf>,
    /// The self-started service has exited: its exit, then its log tail.
    pub(super) child_exit: Option<String>,
    pub(super) exit_log: Option<Job<String>>,
    /// Whole seconds shown while connecting (the line is refreshed once a second).
    pub(super) shown_seconds: u64,
}

/// Generations are unique across documents, so a mesh or job result of a
/// replaced document can never match the new one.
static GENERATION: AtomicU64 = AtomicU64::new(0);
pub(super) fn next_generation() -> u64 {
    GENERATION.fetch_add(1, Ordering::Relaxed) + 1
}

impl CadDocument {
    /// A document not yet connected: CAD mode's OnEnter (or `cad_open`)
    /// starts the connection on a job (`sync::start`). No I/O here.
    pub fn new(target: CadTarget) -> Self {
        let url = match &target {
            CadTarget::Service(url) => Some(url.clone()),
            CadTarget::File(_) => None,
        };
        let what = match &target {
            CadTarget::File(p) => format!("opening local archive {}", p.display()),
            CadTarget::Service(url) => format!("connecting to RoboCAD at {url}"),
        };
        Self {
            local: None, local_load: None, load_sequence: 0, load_outcomes: HashMap::new(),
            generation: next_generation(),
            target,
            client: None,
            child: ChildSlot::default(),
            connection: Connection::Connecting { what, since: Instant::now() },
            health: None,
            doc: None,
            doc_key: None,
            stale: None,
            select_mode: SelectMode::Body,
            hover: None,
            candidates: None,
            tool: CadTool::Select,
            tool_state: Default::default(),
            ops: Default::default(),
            selection_error: None,
            detail: None,
            commands: None,
            autosave: None,
            physical: None,
            robot: Default::default(),
            materials: Default::default(),
            physical_edit: Default::default(),
            results: Default::default(),
            print: Default::default(),
            tree: Default::default(),
            threads: Default::default(),
            references: Default::default(),
            edit: None,
            component_busy: None,
            preview_read_only: false,
            uncertain_edit: None,
            uncertain_history: Vec::new(),
            status: None,
            revision: 0,
            connect: None,
            poll: None,
            seen_poll: 0,
            detail_job: None,
            physical_job: None,
            selection_job: None,
            detail_key: None,
            physical_revision: 0,
            remote_selection: Vec::new(),
            selection_read: false,
            remote_mode: None,
            selection_pushed_at: None,
            selection_again: false,
            published_selection: 0,
            mesh_retry: 0,
            edit_seq: 0,
            edit_waited: false,
            edit_results: HashMap::new(),
            dirty_known_at: None,
            url,
            log: None,
            child_exit: None,
            exit_log: None,
            shown_seconds: 0,
        }
    }
}
