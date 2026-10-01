//! CAD mode's document: what the window knows about one RoboCAD document,
//! reached only through RoboCAD's REST service (`sim_runtime::cad_client`).
//! RoboCAD's kernel and command layer own the document (undo, provenance and
//! the `.rcad` file); this resource holds the client, the connection, the
//! last snapshot RoboCAD gave and the work in flight. Nothing here writes a
//! `.rcad` file, mutates geometry or fills in a physical value.
use crate::jobs::{ChildProcess, Job, RunThread};
use bevy::prelude::*;
use serde_json::Value;
use serde::{Deserialize, Serialize};
use sim_runtime::cad_client::{Autosave, CadClient, CommandInfo, DocState, Health, NodeDetail, Selection, SelectionItem};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

/// What CAD mode was asked to show.
#[derive(Clone, Debug, PartialEq)]
pub enum CadTarget {
    /// A `.rcad` file: the viewer starts RoboCAD's headless service on it
    /// (`python -m robocad.api PATH --port N`) and stops that service when
    /// the document is closed.
    File(PathBuf),
    /// A RoboCAD service already running (its GUI on 8420 by default, or a
    /// headless one): attached to, never stopped.
    Service(String),
}
impl CadTarget {
    pub fn describe(&self) -> String {
        match self {
            CadTarget::File(p) => p.display().to_string(),
            CadTarget::Service(url) => format!("RoboCAD at {url}"),
        }
    }
    /// As `cad_state.target` and `viewer_mode`'s documents list it.
    pub fn json(&self) -> Value {
        match self {
            CadTarget::File(p) => serde_json::json!({"path": p}),
            CadTarget::Service(url) => serde_json::json!({"url": url}),
        }
    }
}

/// The connection to the RoboCAD service, as shown in the header.
#[derive(Clone, Debug, PartialEq)]
pub enum Connection {
    /// Starting the service or waiting for its first answer.
    Connecting { what: String, since: Instant },
    Connected,
    /// The last request failed: the error, verbatim (route and RoboCAD's
    /// message). The last snapshot stays on screen, marked stale.
    Lost { error: String, since: Instant },
}

/// A mutating request in flight (one at a time). While it runs, leaving CAD
/// mode is refused naming it (`app::switch::leaving_blockers`).
pub struct Edit {
    /// What it does, as the refusal and the header name it ("Undo", "Patch
    /// Bracket: visible", "Save", "Command view.fit").
    pub label: String,
    pub job: Job<EditDone>,
    pub started: Instant,
}

/// What a finished edit hands back: the outcome text and whether the
/// document must be refetched (always, after a mutation).
pub struct EditDone {
    pub message: String,
    pub result: Value,
}

/// The poll worker's command (`sync`).
pub enum PollCommand {
    /// Fetch `/doc` (and `/commands`, `/autosave`) now, whatever the revision.
    Refresh,
}

/// What the poll worker publishes, stamped with its own sequence number.
#[derive(Clone, Default)]
pub struct PollSnapshot {
    /// Bumped on every publish.
    pub seq: u64,
    /// `GET /`: Ok(health) or the error verbatim.
    pub health: Option<Result<Health, String>>,
    /// The `/doc` fetched for `health.revision` (None until the first one).
    pub doc: Option<DocState>,
    /// The revision and document id `doc` was fetched at.
    pub doc_key: Option<(Option<String>, u64)>,
    /// Why the last `/doc` refetch failed (the shown `doc` is then stale).
    pub doc_error: Option<String>,
    /// `GET /commands` (RoboCAD's GUI command registry; `{}` headless).
    pub commands: Option<Result<BTreeMap<String, CommandInfo>, String>>,
    /// `GET /autosave` (GUI only; headless answers 409, kept as the error).
    pub autosave: Option<Result<Autosave, String>>,
    /// `GET /selection`, fetched every poll (RoboCAD's selection changes do
    /// not bump the document revision), with the instant the request was
    /// sent: a selection read before our own push landed is not adopted.
    pub selection: Option<(Instant, Result<Selection, String>)>,
}

/// One row of the model tree, in RoboCAD's walk order (`CadDocument::rows`).
#[derive(Clone, Debug, PartialEq)]
pub struct TreeRow {
    pub id: String,
    pub depth: usize,
    pub kind: String,
    pub name: String,
    pub effective_visible: bool,
    pub visible: bool,
    pub locked: bool,
    /// RoboCAD's `disabled` flag (`NodeSummary::disabled`): the node and
    /// its subtree are left out of the model (and are not effectively visible).
    pub disabled: bool,
    pub selected: bool,
}

/// The RoboCAD service this window started, with one owner: the slot. The
/// connect job puts the process here the moment it spawns it, and the
/// document holds the same slot from the moment that job starts, so
/// closing the window or leaving CAD mode while it is still "Connecting…"
/// stops it at once ([`CadDocument::release_child`]: `ChildProcess::stop`
/// never blocks). A closed slot refuses a process put later (the connect
/// job then stops it itself), and the connect job's wait ends when it sees
/// the slot closed. Empty for an attached RoboCAD, which is never stopped.
#[derive(Clone, Default)]
pub struct ChildSlot(Arc<Mutex<SlotState>>);

#[derive(Default)]
struct SlotState {
    child: Option<ChildProcess>,
    closed: bool,
}

impl ChildSlot {
    fn lock(&self) -> MutexGuard<'_, SlotState> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }
    /// The process id while the slot holds the process (also once it has exited).
    pub fn pid(&self) -> Option<u32> {
        self.lock().child.as_ref().map(ChildProcess::id)
    }
    /// Whether the slot holds a process this window started.
    pub fn is_some(&self) -> bool {
        self.lock().child.is_some()
    }
    /// The connect job's: hold `child`, unless the slot was closed meanwhile
    /// (it is then handed back, for the job to stop).
    pub(super) fn put(&self, child: ChildProcess) -> Result<(), ChildProcess> {
        let mut state = self.lock();
        if state.closed {
            return Err(child);
        }
        state.child = Some(child);
        Ok(())
    }
    /// Take the process out (to stop or detach it); the slot stays open.
    pub(super) fn take(&self) -> Option<ChildProcess> {
        self.lock().child.take()
    }
    /// Close the slot (a process put later is refused) and take the process.
    pub(super) fn close(&self) -> Option<ChildProcess> {
        let mut state = self.lock();
        state.closed = true;
        state.child.take()
    }
    pub(super) fn closed(&self) -> bool {
        self.lock().closed
    }
    /// Non-blocking (`ChildProcess::exited`, a `try_wait`): the held
    /// process's exit; None while it runs or when the slot is empty.
    pub(super) fn exited(&self) -> Option<String> {
        self.lock().child.as_mut().and_then(ChildProcess::exited)
    }
    /// The slot holds a process that has not exited.
    pub(super) fn running(&self) -> bool {
        let mut state = self.lock();
        state.child.as_mut().is_some_and(|c| c.exited().is_none())
    }
}

/// RoboCAD's selection modes (`viewport.selection_mode`; keymap `select.*`):
/// what a click in the 3D view picks. A selection item's kind is its mode
/// (`[node, "face", i]`); body items have index 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectMode {
    #[default]
    Body,
    Face,
    Edge,
    Vertex,
    Point,
}
impl SelectMode {
    pub const ALL: [SelectMode; 5] = [SelectMode::Body, SelectMode::Face, SelectMode::Edge, SelectMode::Vertex, SelectMode::Point];
    /// RoboCAD's name (`/selection`'s `mode`, an item's kind).
    pub fn name(self) -> &'static str {
        match self {
            SelectMode::Body => "body",
            SelectMode::Face => "face",
            SelectMode::Edge => "edge",
            SelectMode::Vertex => "vertex",
            SelectMode::Point => "point",
        }
    }
    pub fn parse(name: &str) -> Option<SelectMode> {
        SelectMode::ALL.into_iter().find(|m| m.name() == name)
    }
    /// The mode button's label.
    pub fn label(self) -> &'static str {
        match self {
            SelectMode::Body => "Bodies",
            SelectMode::Face => "Faces",
            SelectMode::Edge => "Edges",
            SelectMode::Vertex => "Vertices",
            SelectMode::Point => "Points",
        }
    }
}

/// The tools of RoboCAD's this mode has (`ui/tools.py`): Select, the
/// transform gizmo's three modes, push/pull, offset face and measure.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CadTool {
    #[default]
    Select,
    Move,
    Rotate,
    Scale,
    PushPull,
    OffsetFace,
    Measure,
}
impl CadTool {
    pub const ALL: [CadTool; 7] = [CadTool::Select, CadTool::Move, CadTool::Rotate, CadTool::Scale, CadTool::PushPull, CadTool::OffsetFace, CadTool::Measure];
    /// RoboCAD's tool name (`Tool.name`; `tool.<name>` in its keymap).
    pub fn name(self) -> &'static str {
        match self {
            CadTool::Select => "select",
            CadTool::Move => "move",
            CadTool::Rotate => "rotate",
            CadTool::Scale => "scale",
            CadTool::PushPull => "push_pull",
            CadTool::OffsetFace => "offset_face",
            CadTool::Measure => "measure",
        }
    }
    pub fn parse(name: &str) -> Option<CadTool> {
        CadTool::ALL.into_iter().find(|t| t.name() == name)
    }
    /// The tool button's label (RoboCAD's toolbar names).
    pub fn label(self) -> &'static str {
        match self {
            CadTool::Select => "Select",
            CadTool::Move => "Move",
            CadTool::Rotate => "Rotate",
            CadTool::Scale => "Scale",
            CadTool::PushPull => "Push/Pull",
            CadTool::OffsetFace => "Offset face",
            CadTool::Measure => "Measure",
        }
    }
}

/// The Alt+click disambiguation menu (RoboCAD's `disambiguation_menu`,
/// ui/widgets.py:888-894): the stacked candidates under the cursor, and
/// how choosing one applies (Shift extends, Ctrl toggles).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Candidates {
    pub items: Vec<SelectionItem>,
    pub extend: bool,
    pub toggle: bool,
}

/// True while one of CAD mode's text fields has the keyboard (D2's panel
/// sets it): CAD mode's keys (`keys`) are ignored meanwhile.
#[derive(Resource, Default)]
pub struct CadInputFocus(pub bool);

/// CAD mode's document (present only in CAD mode; removed on exit).
#[derive(Resource)]
pub struct CadDocument {
    /// Bumped on every (re)connect and open: results of an older generation
    /// are dropped.
    pub generation: u64,
    pub target: CadTarget,
    /// The client, once the service answers (None while connecting).
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
    /// The selection as RoboCAD's items `[node, kind, index]` (kind body,
    /// face, edge, vertex or point), as last pushed to or read from RoboCAD's
    /// `/selection`.
    pub selection: Vec<SelectionItem>,
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
    /// The mutating request in flight.
    pub edit: Option<Edit>,
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
    /// The last selection mode read from RoboCAD (a desktop window's; a
    /// headless service sends none), adopted once when it changes.
    pub(super) remote_mode: Option<SelectMode>,
    /// When our latest selection push was answered: a poll's selection read
    /// sent before then is not adopted (it predates the push).
    pub(super) selection_pushed_at: Option<Instant>,
    /// The selection changed while a push was in flight: the newest
    /// (`selection`) is pushed when that push answers, so pushes never overlap.
    pub(super) selection_again: bool,
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

/// A finished connect: the client, the first health, and whether this
/// window started the service (its process is in the document's
/// [`ChildSlot`] already).
pub struct Connected {
    pub client: CadClient,
    pub health: Health,
    pub self_started: bool,
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
            CadTarget::File(p) => format!("starting RoboCAD's service on {}", p.display()),
            CadTarget::Service(url) => format!("connecting to RoboCAD at {url}"),
        };
        Self {
            generation: next_generation(),
            target,
            client: None,
            child: ChildSlot::default(),
            connection: Connection::Connecting { what, since: Instant::now() },
            health: None,
            doc: None,
            doc_key: None,
            stale: None,
            selection: Vec::new(),
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
            edit: None,
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
            remote_mode: None,
            selection_pushed_at: None,
            selection_again: false,
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

    /// The model tree in RoboCAD's walk order (`/doc` nodes), with each
    /// row's depth from its parent chain. Empty until the first `/doc`.
    pub(crate) fn rows(&self) -> Vec<TreeRow> {
        let Some(doc) = &self.doc else { return Vec::new() };
        let parents: HashMap<&str, Option<&str>> = doc.nodes.iter().map(|n| (n.id.as_str(), n.parent.as_deref())).collect();
        doc.nodes
            .iter()
            .map(|n| {
                // Bounded by the node count: a malformed parent cycle cannot hang the UI.
                let mut depth = 0;
                let mut parent = n.parent.as_deref();
                while let Some(p) = parent {
                    if depth >= doc.nodes.len() {
                        break;
                    }
                    depth += 1;
                    parent = parents.get(p).copied().flatten();
                }
                TreeRow {
                    id: n.id.clone(),
                    depth,
                    kind: n.kind.clone(),
                    name: n.name.clone(),
                    effective_visible: n.effective_visible,
                    visible: n.visible,
                    locked: n.locked,
                    disabled: n.disabled,
                    selected: self.selection.iter().any(|s| s.0 == n.id),
                }
            })
            .collect()
    }

    /// The node's name in the shown tree (its id when unknown).
    pub(crate) fn node_name(&self, id: &str) -> String {
        self.doc.as_ref().and_then(|d| d.nodes.iter().find(|n| n.id == id)).map(|n| n.name.clone()).unwrap_or_else(|| id.to_string())
    }

    /// Whether the shown tree has node `id` (true while no tree is shown yet).
    pub(crate) fn has_node(&self, id: &str) -> bool {
        self.doc.as_ref().is_none_or(|d| d.nodes.iter().any(|n| n.id == id))
    }

    /// The URL the window talks to, once known.
    pub(crate) fn url(&self) -> Option<&str> {
        self.url.as_deref()
    }

    /// The service: self-started (with its pid) or attached, its URL, and
    /// RoboCAD's GUI or headless service once it has answered.
    pub(crate) fn service_line(&self) -> String {
        let url = self.url.as_deref().unwrap_or("a port not yet chosen");
        let mut line = match (&self.target, self.child.pid()) {
            (CadTarget::File(p), Some(pid)) if self.connect.is_some() => format!("Starting RoboCAD's headless service (pid {pid}) on {}", p.display()),
            (CadTarget::File(_), Some(pid)) => format!("Self-started RoboCAD (pid {pid}) at {url}"),
            (CadTarget::File(p), None) if self.connect.is_some() => format!("Starting RoboCAD's headless service on {}", p.display()),
            (CadTarget::File(p), None) => format!("RoboCAD's service for {} is not running", p.display()),
            (CadTarget::Service(_), _) => format!("Attached to RoboCAD at {url} (never stopped by this window)"),
        };
        if let Some(h) = &self.health {
            line.push_str(if h.gui { " · desktop GUI" } else { " · headless" });
            if !h.version.is_empty() {
                line.push_str(&format!(" · RoboCAD {}", h.version));
            }
        }
        line
    }

    /// The header's connection line and whether it is an error.
    pub(crate) fn connection_line(&self) -> (String, bool) {
        match &self.connection {
            Connection::Connecting { what, since } => (format!("Connecting: {what} ({} s)", since.elapsed().as_secs()), false),
            Connection::Connected => {
                let mut line = String::from("Connected");
                if let Some((_, revision)) = &self.doc_key {
                    line.push_str(&format!(" · revision {revision}"));
                }
                match self.unsaved() {
                    Some(true) => line.push_str(" · unsaved edits in RoboCAD"),
                    None => line.push_str(" · saved state being refetched"),
                    Some(false) => {}
                }
                if let Some(stale) = &self.stale {
                    line.push_str(&format!(" · tree may be behind RoboCAD: {stale}"));
                }
                if let Some(e) = &self.selection_error {
                    line.push_str(&format!(" · RoboCAD's selection could not be read: {e}"));
                }
                (line, self.doc_error_shown() || self.selection_error.is_some())
            }
            Connection::Lost { error, .. } => (format!("Not connected: {error}"), true),
        }
    }

    /// The stale reason is a failed refetch (an error), not a refetch under way.
    fn doc_error_shown(&self) -> bool {
        self.stale.as_ref().is_some_and(|s| !s.starts_with("refetching revision"))
    }

    /// The document's file name (RoboCAD's path, else the target's), or
    /// "untitled" for a document RoboCAD has not saved.
    pub(crate) fn document_name(&self) -> String {
        let path = self.health.as_ref().and_then(|h| h.path.clone()).or_else(|| match &self.target {
            CadTarget::File(p) => Some(p.display().to_string()),
            CadTarget::Service(_) => None,
        });
        match path {
            Some(p) => std::path::Path::new(&p).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or(p),
            None => "untitled document".into(),
        }
    }

    /// The edit in flight, by name.
    pub(crate) fn edit_label(&self) -> Option<&str> {
        self.edit.as_ref().map(|e| e.label.as_str())
    }

    /// The first selected node (the inspected one).
    pub(crate) fn selected(&self) -> Option<&str> {
        self.selection.first().map(|i| i.0.as_str())
    }

    /// The selected nodes, each once, in selection order (RoboCAD's
    /// `Selection.nodes`).
    pub(crate) fn selected_nodes(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for SelectionItem(node, ..) in &self.selection {
            if !out.contains(node) {
                out.push(node.clone());
            }
        }
        out
    }

    /// The selected items of one kind ("face", "edge", …) as (node, index).
    pub(crate) fn selected_of(&self, kind: &str) -> Vec<(String, i64)> {
        self.selection.iter().filter(|i| i.1 == kind).map(|i| (i.0.clone(), i.2)).collect()
    }

    /// RoboCAD's revision the shown tree (and meshes, topology) is at.
    pub(crate) fn shown_revision(&self) -> u64 {
        self.doc_key.as_ref().map_or(0, |k| k.1)
    }

    /// Why a tool's commit cannot be sent now: an edit in flight or no
    /// connection (`edit_refusal`), the shown document behind RoboCAD's
    /// (`stale`), or RoboCAD's revision changed since the drag or preview
    /// began at `began` (the preview was computed on geometry that is gone).
    /// Nothing is sent when refused.
    pub(crate) fn commit_refusal(&self, began: Option<u64>) -> Option<String> {
        if let Some(why) = self.edit_refusal() {
            return Some(why);
        }
        if let Some(stale) = &self.stale {
            return Some(format!("the shown document is behind RoboCAD's ({stale}); nothing was sent"));
        }
        match (began, self.health.as_ref().map(|h| h.revision)) {
            (Some(began), Some(now)) if began != now || began != self.shown_revision() => {
                Some(format!("the document changed since the preview began (revision {began}, now {now}); nothing was sent: redo the drag or the entry"))
            }
            _ => None,
        }
    }

    /// The service answers and the window may send it requests.
    pub(crate) fn connected(&self) -> bool {
        self.client.is_some() && self.connection == Connection::Connected
    }

    /// Whether RoboCAD holds unsaved edits: `health.dirty`, or None when
    /// that cannot be confirmed: not connected (the last `health` may be
    /// old; RoboCAD may hold edits made since), an edit in flight, or just
    /// after one until a successful `GET /` sent after it has been read.
    pub(crate) fn unsaved(&self) -> Option<bool> {
        if self.dirty_known_at.is_some() || self.edit.is_some() || !self.connected() {
            return None;
        }
        self.health.as_ref().map(|h| h.dirty)
    }

    /// A self-started service that has answered in this connection (so it
    /// may hold edits) and has not exited (an exited one's edits are gone).
    fn child_may_hold_edits(&self) -> bool {
        self.client.is_some() && self.child_exit.is_none() && self.child.running()
    }

    /// Why a mutating request cannot be sent now (refusals name it).
    pub(crate) fn edit_refusal(&self) -> Option<String> {
        if let Some(label) = self.edit_label() {
            return Some(format!("another CAD edit is in flight: {label}"));
        }
        if !self.connected() {
            return Some(format!("not connected to RoboCAD: {}", self.connection_line().0));
        }
        None
    }

    /// What leaving CAD mode (or replacing this document) would lose: an
    /// edit in flight, or unsaved edits in a service this window started
    /// (it stops when the document closes; the viewer never saves for you).
    pub(crate) fn switch_blockers(&self) -> Vec<String> {
        let mut blockers = Vec::new();
        if let Some(label) = self.edit_label() {
            blockers.push(format!("a CAD edit is in flight: {label}"));
        }
        if self.edit.is_none() && self.child_may_hold_edits() {
            let name = self.document_name();
            let pid = self.child.pid().map_or_else(String::new, |p| format!(" (pid {p})"));
            match self.unsaved() {
                Some(false) => {}
                Some(true) => blockers.push(format!("{name} has unsaved edits in the RoboCAD service this window started{pid}, which stops when CAD mode closes: save first (cad_save or the Save button)")),
                None if !self.connected() => blockers.push(format!(
                    "{name} may have unsaved edits in the RoboCAD service this window started{pid}, and its saved state can't be confirmed while the window is not connected to it ({}); that service stops when CAD mode closes: Refresh (cad_refresh) to reconnect and save first, or stop that process yourself to discard its edits",
                    self.connection_line().0
                )),
                None => blockers.push(format!("{name} may have unsaved edits in the RoboCAD service this window started{pid} (an edit just finished and RoboCAD's state is being refetched), which stops when CAD mode closes: wait a moment, or save first (cad_save or the Save button)")),
            }
        }
        blockers
    }

    /// Leaving an attached document with unsaved edits is allowed: the
    /// switch's message says where the edits stay.
    pub(crate) fn leaving_note(&self) -> Option<String> {
        if self.child.is_some() {
            return None;
        }
        let url = self.url.as_deref().unwrap_or("its URL");
        match self.unsaved() {
            Some(true) => Some(format!("RoboCAD at {url} keeps the unsaved edits to {}", self.document_name())),
            // Not confirmed now, but the last answer said dirty.
            None if self.health.as_ref().is_some_and(|h| h.dirty) => Some(format!("RoboCAD at {url} had unsaved edits to {} when last read; any it still holds stay there", self.document_name())),
            _ => None,
        }
    }

    /// Mark something shown as changed (the panels refresh on it).
    pub(crate) fn touch(&mut self) {
        self.revision += 1;
    }

    /// Show an outcome line.
    pub(crate) fn show(&mut self, status: Result<String, String>) {
        self.status = Some(status);
        self.touch();
    }

    /// Before the document is dropped (window close, leaving CAD mode,
    /// `cad_open`): the child slot is closed (a service still starting is
    /// stopped by its connect job the moment it would be put there) and a
    /// self-started service is stopped, unless it has answered in this
    /// connection, has not exited and may hold unsaved edits (dirty, or not
    /// confirmable: not connected, an edit in flight or just finished). Then
    /// it is detached (left running, its URL logged) so the edits are not
    /// lost: unsaved edits are RoboCAD's, and the viewer never saves on its
    /// own. Synchronous and non-blocking (`ChildProcess::stop`/`detach`).
    /// Returns the URL of a service left running.
    pub(crate) fn release_child(&mut self, why: &str) -> Option<String> {
        let keep = self.child_may_hold_edits() && self.unsaved() != Some(false);
        let child = self.child.close()?;
        if keep {
            let state = if self.unsaved() == Some(true) { "holds unsaved edits" } else { "may hold unsaved edits (its saved state could not be confirmed)" };
            bevy::log::warn!(
                "{why}: the RoboCAD service this window started (pid {}) {state} to {}; it is left running at {} so they are not lost: open it there (sim-spatial --cad-url {}) and save, or stop it",
                child.id(),
                self.document_name(),
                self.url.as_deref().unwrap_or("its URL"),
                self.url.as_deref().unwrap_or("URL")
            );
            child.detach();
            self.url.clone()
        } else {
            child.stop();
            None
        }
    }
}
