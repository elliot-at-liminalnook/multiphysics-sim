//! The document's value types: the target, the connection, edits in
//! flight and their results, the poll's snapshot, tree rows, the child
//! service's slot, the selection modes, the tools, the Alt menu and the
//! input focus flag.
use crate::jobs::{ChildProcess, Job};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sim_runtime::cad_client::{Autosave, CadClient, CommandInfo, DocState, Health, Selection, SelectionItem};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

/// What CAD mode was asked to show.
#[derive(Clone, Debug, PartialEq)]
pub enum CadTarget {
    /// A compatible archive owned and queried in process through sim-cad.
    File(PathBuf),
    /// Legacy target shape; native entry points refuse attachment by name.
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
    /// The selection when an operation that clears it started
    /// (`ops::started`): cleared once the edit succeeds, if it is still
    /// this selection (RoboCAD clears after its Ops call returns, so a
    /// failed edit keeps the user's picks). None for every other edit.
    pub clear_selection: Option<Vec<SelectionItem>>,
    /// The edit creates a plane node (a plane tool's op): once it succeeds,
    /// the node RoboCAD answers becomes the active plane
    /// (`ops::OpsState::plane_created`; RoboCAD's `set_active_plane(pid)`).
    pub activates_plane: bool,
    /// A save to a path (File > Save As…, `cad_save {path}`; `files::save`
    /// sets it on the edit it started, by `edit_seq`): once it succeeds, a
    /// self-started document's target becomes the saved file, so the mode
    /// reopens it (RoboCAD's `Document.save` made it the service's file).
    /// An attached service keeps its URL. None otherwise.
    pub retarget: Option<std::path::PathBuf>,
}

/// What a finished edit hands back: the outcome text and whether the
/// document must be refetched (always, after a mutation).
pub struct EditDone {
    pub message: String,
    pub result: Value,
}

/// The poll worker's command (`sync`).
pub enum PollCommand {
    /// Fetch `/doc` (and `/commands`) now, whatever the revision. (`/autosave`
    /// is read every poll tick from a desktop window: its state changes
    /// without the revision moving.)
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
    /// `GET /autosave`, read every poll tick from a desktop window (`health.gui`;
    /// None from a headless service, which has no autosave).
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
/// stops it at once ([`CadDocument::release_child`](super::CadDocument::release_child): `ChildProcess::stop`
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
    pub(in crate::cad) fn put(&self, child: ChildProcess) -> Result<(), ChildProcess> {
        let mut state = self.lock();
        if state.closed {
            return Err(child);
        }
        state.child = Some(child);
        Ok(())
    }
    /// Take the process out (to stop or detach it); the slot stays open.
    pub(in crate::cad) fn take(&self) -> Option<ChildProcess> {
        self.lock().child.take()
    }
    /// Close the slot (a process put later is refused) and take the process.
    pub(in crate::cad) fn close(&self) -> Option<ChildProcess> {
        let mut state = self.lock();
        state.closed = true;
        state.child.take()
    }
    pub(in crate::cad) fn closed(&self) -> bool {
        self.lock().closed
    }
    /// Non-blocking (`ChildProcess::exited`, a `try_wait`): the held
    /// process's exit; None while it runs or when the slot is empty.
    pub(in crate::cad) fn exited(&self) -> Option<String> {
        self.lock().child.as_mut().and_then(ChildProcess::exited)
    }
    /// The slot holds a process that has not exited.
    pub(in crate::cad) fn running(&self) -> bool {
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
    /// The shown revision the candidates were gathered at: a choice's face,
    /// edge, vertex, point and curve items carry it into the shared
    /// selection, so a choice from a menu the tree has since moved past is
    /// refused by name (`selection::select`).
    pub revision: Option<u64>,
}

/// A finished connect: the client, the first health, and whether this
/// window started the service (its process is in the document's
/// [`ChildSlot`] already).
pub struct Connected {
    pub client: CadClient,
    pub health: Health,
    pub self_started: bool,
}
