//! The CAD source's comment threads in Robot mode (cad-annotation-parity AN3;
//! native-viewer.md §7 "one annotations service, one thread panel"): the
//! comments of the CAD source an export names, shown beside the links they
//! are on, through the same `annotations::ThreadSource` and the same
//! `ui_kit::threads` panel as CAD mode.
//!
//! - **Where they are.** In the CAD source's `.rcad` (RoboCAD's format,
//!   kept compatible), which Robot mode reads and writes in process: no
//!   service, no HTTP. While Robot mode is shown the file is the only copy
//!   (CAD mode cannot be left with unsaved edits and drops its document).
//! - **Reads** ([`read`]): one `Pool::Io` job per (export, CAD source,
//!   epoch) lists the threads exactly as CAD mode does
//!   (`sim_cad::annotations::list` over the file's pinned stamps); every 2 s
//!   while shown, a probe re-reads when the file's size or modification time
//!   moved. The last list stays shown while a newer one is read, labelled
//!   with the archive revision it was read at.
//! - **Mapping** ([`link_of`], [`place`]): a thread's part is on link `i` when
//!   it is the link's CAD body (`links[i].id`) or one of its members, else
//!   the nearest ancestor that is (the manifest's node parents, cycle-safe). No
//!   name matching. A thread on no link, an evidence thread, and every
//!   thread of an export that is not current with its CAD file carry a
//!   warning line ([`link_note`]).
//! - **Changes** ([`RobotCadThreads`], Robot mode's ThreadSource; acts in
//!   `act`): reply, edit, delete a message and resolve, each the same
//!   `cad::threads::Request` CAD mode applies, run on a job that reopens the
//!   file, refuses when its bytes are not the ones read (changed on disk
//!   since: nothing written, the threads are read again), applies the
//!   request to the archive (`sim_cad::Edit`, revision + 1) and saves it
//!   atomically. New threads are placed in CAD mode; there is no undo here
//!   ([`NO_UNDO`]).
//! - **Selection.** Opening a thread selects it and its link; a part chip or
//!   a `[label](part:ID)` link whose node is on a link selects that link
//!   (`picked::select`, the one selection, keeping the inspector's scroll).
//! - **Open in CAD** (`act::open_in_cad`): the CAD document switch to the file, carrying
//!   the thread to reveal (`app::switch::ModeSwitch::reveal`); the switch
//!   installs it in CAD's state (`cad::threads::RevealThread`) only when it
//!   is accepted, so a refused request's reveal goes with it.
use super::{RobotView, Section};
use crate::annotations::{Committed, InFlight, ThreadSource};
use crate::app::ViewerMode;
use crate::cad::threads::{CadAnchor, DELETE_COMMENT, DELETE_THREAD, Request, plain, request_on, thread_of};
use crate::document::DocumentRegistry;
use crate::jobs::{Job, Pool};
use crate::ui_kit::threads::Shown;
use bevy::prelude::*;
use serde_json::{Value, json};
use sim_annotate::{Thread, ThreadCommand};
use sim_domain_robot::cad_link::CadLinkStatus;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

mod act;
mod panel;
mod read;
#[cfg(test)]
mod tests;

pub(crate) use act::{ThreadsAct, from_rest, handle};
pub(super) use panel::{ThreadsRoot, input};

/// The start of a thread's warning when its part is on no link of the export.
pub(crate) const NOT_ON_LINK: &str = "Not on any link of this export";
/// The ancestor walk's bound (a malformed tree never loops: visited nodes stop it too).
const MAX_DEPTH: usize = 512;

/// A link of the export as the mapping sees it: its name, its CAD body's
/// node id (may be empty) and its member nodes.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct LinkKeys {
    pub name: String,
    pub id: String,
    pub members: Vec<String>,
}
impl LinkKeys {
    /// The loaded model's links (none for a planar v2 file or before loading).
    pub(crate) fn of(view: &RobotView) -> Vec<LinkKeys> {
        view.model.iter().flat_map(|m| m.links.iter()).map(|l| LinkKeys { name: l.name.clone(), id: l.id.clone(), members: l.members.clone() }).collect()
    }
}

/// The archive manifest's node parents: id → parent id.
pub(crate) type Parents = BTreeMap<String, Option<String>>;

/// The link node `node` is on: the first link whose body or members name it,
/// else the nearest ancestor's (cycle-safe, bounded). None: on no link.
pub(crate) fn link_of<'a>(links: &[LinkKeys], parents: &'a Parents, node: &'a str) -> Option<usize> {
    let direct = |n: &str| -> Option<usize> {
        if n.is_empty() {
            return None;
        }
        links.iter().position(|l| l.id == n || l.members.iter().any(|m| m.as_str() == n))
    };
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut at: &str = node;
    for _ in 0..MAX_DEPTH {
        if let Some(i) = direct(at) {
            return Some(i);
        }
        if !seen.insert(at) {
            return None;
        }
        match parents.get(at) {
            Some(Some(parent)) => at = parent.as_str(),
            _ => return None,
        }
    }
    None
}

/// The link an anchor (a part chip, a part link in a message) is on.
pub(crate) fn anchor_link(anchor: &CadAnchor, links: &[LinkKeys], parents: &Parents) -> Option<usize> {
    link_of(links, parents, anchor.node()?)
}

/// Why every thread may be on another part now: the export is not known
/// to match its CAD file (None: current, or not checked).
pub(crate) fn link_note(status: Option<&CadLinkStatus>) -> Option<&'static str> {
    Some(match status? {
        CadLinkStatus::Current { .. } => return None,
        CadLinkStatus::Stale { .. } => "The export is older than the CAD file: this comment may be on a different part now",
        CadLinkStatus::NoRecordedHash { .. } => "The export recorded no CAD hash: this comment may be on a different part now",
        CadLinkStatus::Missing { .. } => "The export's CAD file was not found: this comment may be on a different part now",
        CadLinkStatus::NoSourceFile => "The export names no CAD file: this comment may be on a different part now",
        CadLinkStatus::Unreadable { .. } => "The CAD file could not be read to compare with the export: this comment may be on a different part now",
    })
}

/// Where a thread is in this export: its link and its warning line.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Placed {
    pub link: Option<usize>,
    pub warning: Option<String>,
}

/// A thread (as `thread_of` shows it) placed on the export's links; `note`
/// is [`link_note`]'s line, joined after the thread's own.
pub(crate) fn place(thread: &Thread<CadAnchor>, links: &[LinkKeys], parents: &Parents, note: Option<&str>) -> Placed {
    let (link, unplaced) = match thread.targets.first() {
        Some(CadAnchor::Surface { node_id, node_name, .. }) => match link_of(links, parents, node_id) {
            Some(i) => (Some(i), None),
            None => (None, Some(format!("{NOT_ON_LINK}: {}", if node_name.is_empty() { node_id } else { node_name }))),
        },
        _ => (None, Some(format!("{NOT_ON_LINK}: experiment evidence"))),
    };
    let lines: Vec<&str> = [unplaced.as_deref(), note].into_iter().flatten().collect();
    Placed { link, warning: (!lines.is_empty()).then(|| lines.join("\n")) }
}

/// The CAD file the export resolves to (none when missing or not named).
pub(crate) fn cad_path(status: &CadLinkStatus) -> Option<&Path> {
    match status {
        CadLinkStatus::Current { path, .. } | CadLinkStatus::Stale { path, .. } | CadLinkStatus::NoRecordedHash { path, .. } | CadLinkStatus::Unreadable { path, .. } => Some(path.as_path()),
        CadLinkStatus::Missing { .. } | CadLinkStatus::NoSourceFile => None,
    }
}

/// The CAD source's name as the window says it.
pub(crate) fn source_file(status: Option<&CadLinkStatus>) -> String {
    match status {
        Some(CadLinkStatus::Missing { file, .. }) => file.clone(),
        Some(s) => cad_path(s).map_or_else(|| "this export's CAD source".to_string(), |p| p.file_name().map_or_else(|| p.display().to_string(), |f| f.to_string_lossy().into_owned())),
        None => "this export's CAD source".to_string(),
    }
}

/// A change refused while the CAD source's comments are not read.
pub(crate) fn not_open(file: &str) -> String {
    format!("the comments of {file} are not read yet (or could not be read): refresh, or open it in CAD mode")
}

/// Where the comments are when they cannot be shown here.
pub(crate) fn lives_in(file: &str) -> String {
    format!("Comments live in the CAD source {file}: open it in CAD mode to see them")
}

/// A change refused while another is being written.
pub(crate) const BUSY: &str = "Another comment change is being written to the CAD file; wait for it";
/// A change refused because the file's bytes are not the ones the threads were read from.
pub(crate) fn changed_on_disk(file: &Path) -> String {
    format!("{} changed on disk since its comments were read: nothing was written; they are read again", file.display())
}
/// Robot mode writes each change straight to the file: no undo stack here.
pub(crate) const NO_UNDO: &str = "Comment changes made here are saved to the CAD file at once and are not undone here; open the file in CAD mode to undo (each later change there is one undo step)";

/// What a read is about: the export and its CAD source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Base {
    pub simrobot: PathBuf,
    pub cad: PathBuf,
}

/// The read's subject now (None: no CAD file resolved).
pub(crate) fn base_of(view: &RobotView) -> Option<Base> {
    let cad = cad_path(view.cad_link.as_ref()?)?.to_path_buf();
    Some(Base { simrobot: view.path.clone(), cad })
}

/// Robot mode's thread state (a resource, reset on leaving Robot mode):
/// the read, the changes in flight and the panel's draft. The draft lives
/// here, not in drawn entities, so it survives redraws and re-reads.
#[derive(Resource)]
pub(crate) struct RobotThreads {
    pub(crate) read: read::ReadState,
    /// Changes sent and not yet answered (`annotations::InFlight`).
    pub(crate) in_flight: InFlight,
    /// Each change's job, by its sequence.
    commits: Vec<(u64, Job<Value>)>,
    next: u64,
    /// Landed answers a REST caller may still be waiting for (bounded).
    answers: BTreeMap<u64, Result<Option<String>, String>>,
    pub(crate) filter: Shown,
    /// The thread shown.
    pub(crate) current: Option<String>,
    /// The message whose menu (Edit, Delete) is open.
    pub(crate) menu: Option<String>,
    /// The composer's text (mirrored from its kit field).
    pub(crate) compose: String,
    /// The message being edited.
    pub(crate) editing: Option<String>,
    pub(crate) author: String,
    /// The composer's post in flight: its sequence and the body sent.
    pub(crate) sending: Option<(u64, String)>,
    /// The last refusal or error, under the composer.
    pub(crate) error: Option<String>,
    /// The composer has the kit's keyboard (mirrored by `panel::input`).
    pub(crate) focus: bool,
    /// The composer is to take the keyboard (Edit message filled it).
    pub(crate) claim: bool,
    /// The composer is to let the keyboard go (its draft ended).
    pub(crate) release: bool,
    /// A REST caller asked: read even while the section is hidden.
    pub(crate) asked: bool,
}
impl Default for RobotThreads {
    fn default() -> Self {
        Self {
            read: read::ReadState::default(),
            in_flight: InFlight::default(),
            commits: Vec::new(),
            next: 0,
            answers: BTreeMap::new(),
            filter: Shown::Open,
            current: None,
            menu: None,
            compose: String::new(),
            editing: None,
            author: "You".into(),
            sending: None,
            error: None,
            focus: false,
            claim: false,
            release: false,
            asked: false,
        }
    }
}
impl RobotThreads {
    /// A draft is open.
    pub(crate) fn drafting(&self) -> bool {
        !self.compose.is_empty() || self.editing.is_some()
    }
    fn end_draft(&mut self) {
        self.compose.clear();
        self.editing = None;
        self.error = None;
        self.release = true;
    }
    /// The threads as read for `base`.
    pub(crate) fn open_listed(&self, base: Option<&Base>) -> Option<&read::Listed> {
        let base = base?;
        if !self.read.open(base) {
            return None;
        }
        self.read.listed_for(base)
    }
}

/// Robot mode's ThreadSource over the CAD source's threads (the CAD anchor):
/// the threads as read, each change one write of the file on a job (see the module doc).
pub(crate) struct RobotCadThreads<'a> {
    pub st: &'a mut RobotThreads,
    /// The export's CAD source now.
    pub base: Option<Base>,
    /// The CAD source's name (the refusal names it).
    pub file: String,
}
impl RobotCadThreads<'_> {
    /// The subject and the bytes' identity the shown threads were read from, when read.
    fn opened(&self) -> Option<(Base, String)> {
        let listed = self.st.open_listed(self.base.as_ref())?;
        Some((listed.base.clone(), listed.identity.clone()))
    }
}
impl ThreadSource for RobotCadThreads<'_> {
    type Anchor = CadAnchor;
    /// Never written: `sim_cad::annotations` assigns ids.
    const THREAD_ID: &'static str = "new-thread";
    const COMMENT_ID: &'static str = "new-comment";
    fn threads(&self) -> BTreeMap<String, Thread<CadAnchor>> {
        self.st.open_listed(self.base.as_ref()).map(|l| l.threads.iter().map(|t| (t.id.clone(), thread_of(t))).collect()).unwrap_or_default()
    }
    /// A linked part is the same part whatever its label (as CAD mode's source).
    fn same(a: &CadAnchor, b: &CadAnchor) -> bool {
        match (a, b) {
            (CadAnchor::Part { node_id: x, .. }, CadAnchor::Part { node_id: y, .. }) => x == y,
            _ => a == b,
        }
    }
    fn commit(&mut self, label: &str, command: ThreadCommand<CadAnchor>) -> Result<Committed, String> {
        if matches!(command, ThreadCommand::Undo | ThreadCommand::Redo) {
            return Err(NO_UNDO.into());
        }
        let Some((base, identity)) = self.opened() else { return Err(not_open(&self.file)) };
        // One change at a time: each is checked against the bytes the threads were read from.
        if self.st.in_flight.busy() {
            return Err(BUSY.into());
        }
        let threads = self.threads();
        let (label, thread, request) = request_on(&threads, label, command)?;
        self.st.next += 1;
        let seq = self.st.next;
        let cad = base.cad;
        // A started change runs to its end (a half-made write is never left: the save is atomic).
        let job = Job::spawn(Pool::Dedicated, seq, "robot-cad-thread-change", move |_| commit_job(&cad, &identity, request)).complete_on_drop();
        self.st.commits.push((seq, job));
        self.st.in_flight.submitted(seq, &label, thread);
        Ok(Committed::Pending(seq))
    }
}

/// The change's job: the file still holds the bytes the threads were read
/// from, the request applied to them as CAD mode applies it (a person's
/// change), then an atomic save. (Messages made here never start with
/// "RoboCAD ", so `plain` keeps them.)
pub(crate) fn commit_job(cad: &Path, identity: &str, request: Request) -> Result<Value, String> {
    let archive = sim_cad::ArchiveDocument::open(cad)?;
    if archive.identity() != identity {
        return Err(changed_on_disk(cad));
    }
    let stamps = sim_cad::annotations::pinned_stamps(&archive);
    let no = || false;
    let mut ws = crate::cad::local::Workspace { archive: &archive, stamps: &stamps, geometry: &[], edit: sim_cad::Edit::of(&archive), cancelled: &no };
    let result = request.apply(&mut ws, sim_cad::annotations::AuthorKind::Person)?;
    let next = archive.apply(ws.edit)?;
    next.save(cad)?;
    Ok(result)
}

/// A change's answer, landed (JobResults): the threads are read again; a
/// post's draft ends when it was written unchanged; an error stays under
/// the composer.
pub(crate) fn land(st: &mut RobotThreads, seq: u64, result: Result<Value, String>) {
    let id = result.as_ref().ok().and_then(|v| v.get("id")).and_then(Value::as_str).map(str::to_string);
    let Some(landed) = st.in_flight.land(seq, result.map(|_| id)) else { return };
    st.read.again();
    let sent = st.sending.take_if(|(s, _)| *s == seq);
    match &landed.result {
        Ok(_) => {
            if let Some((_, body)) = &sent {
                if st.compose == *body {
                    st.end_draft();
                } else {
                    // Typed further while in flight: kept as a plain reply draft.
                    st.editing = None;
                }
            }
            if landed.label == DELETE_COMMENT {
                st.menu = None;
            }
            if landed.label == DELETE_THREAD && st.current == landed.thread {
                st.current = None;
            }
        }
        Err(e) => st.error = Some(plain(e)),
    }
    st.answers.insert(seq, landed.result);
    while st.answers.len() > 64 {
        st.answers.pop_first();
    }
}

/// Why there is no CAD file for this export.
fn source_line(status: &CadLinkStatus) -> String {
    match status {
        CadLinkStatus::Missing { file, tried } => format!("The CAD source {file} this export names was not found ({} places tried)", tried.len()),
        CadLinkStatus::NoSourceFile => "This export names no CAD source file".to_string(),
        other => lives_in(&source_file(Some(other))),
    }
}

/// The section's status line: where the comments are and what was read.
pub(crate) fn line(view: &RobotView, st: &RobotThreads) -> String {
    let Some(status) = view.cad_link.as_ref() else {
        return "The export's CAD source is not known (the robot is loading, or this file has no CAD link).".to_string();
    };
    if cad_path(status).is_none() {
        return format!("{}: its comments cannot be read.", source_line(status));
    }
    let file = source_file(Some(status));
    let Some(base) = base_of(view) else { return lives_in(&file) };
    match st.read.reach(&base) {
        None if st.read.reading() => format!("Reading the comments of {file}…"),
        None => lives_in(&file),
        Some(read::Reach::Failed(e)) => format!("{file} could not be read ({e}). {}", lives_in(&file)),
        Some(read::Reach::Open) => match st.read.listed_for(&base) {
            Some(l) => format!("Comments of {file} as read at revision {}", l.revision),
            None => lives_in(&file),
        },
    }
}

/// Each listed thread's place (by id).
pub(crate) fn placements(listed: &read::Listed, links: &[LinkKeys], note: Option<&str>) -> BTreeMap<String, Placed> {
    listed.threads.iter().map(|t| (t.id.clone(), place(&thread_of(t), links, &listed.parents, note))).collect()
}

/// `robot_state.cad_threads`.
pub(crate) fn state_json(view: &RobotView, st: &RobotThreads) -> Value {
    let base = base_of(view);
    let listed = st.open_listed(base.as_ref());
    let links = LinkKeys::of(view);
    let note = link_note(view.cad_link.as_ref());
    let threads: Vec<Value> = listed
        .map(|l| {
            l.threads
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    let placed = place(&thread_of(t), &links, &l.parents, note);
                    json!({
                        "id": t.id,
                        "number": i + 1,
                        "node_id": t.anchor.node_id,
                        "node_name": t.node_name,
                        "link": placed.link.map(|k| json!({"index": k, "name": links[k].name})),
                        "warning": placed.warning,
                        "resolved": t.resolved(),
                        "messages": t.comments.iter().map(|c| json!({"id": c.id, "author": c.author, "body": c.body, "created_at": c.created_at, "updated_at": c.updated_at})).collect::<Vec<_>>(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let mut out = json!({
        "open": listed.is_some(),
        "source": view.cad_link.as_ref().and_then(cad_path),
        "line": line(view, st),
        "read_at_revision": listed.map(|l| l.revision),
        "reading": st.read.reading(),
        "threads": threads,
    });
    out["filter"] = json!(st.filter);
    out["current"] = json!(st.current);
    out["menu"] = json!(st.menu);
    out["draft"] = json!({"text": st.compose, "editing": st.editing, "author": st.author, "sending": st.sending.as_ref().map(|s| s.0), "error": st.error});
    out["in_flight"] = json!(st.in_flight.busy());
    out["undo"] = json!(NO_UNDO);
    out
}

/// OnExit(Robot): nothing read or drafted is kept (changes already sent run to their end).
fn reset(mut st: ResMut<RobotThreads>) {
    *st = RobotThreads::default();
}

/// RobotPlugin: the state, the composer's kit field, the reads and changes
/// (JobResults) and the section (Present). The composer's input runs in
/// robot mode's Input chain (`panel::input`).
pub(super) fn build(app: &mut App) {
    use crate::app::{ModeScope, ViewerSet};
    use crate::ui_kit::text::TextFieldApp;
    app.init_resource::<RobotThreads>()
        .add_text_field(panel::COMPOSE, panel::compose_field())
        .add_systems(OnExit(ModeScope::Robot), reset)
        .add_systems(Update, (read::results.in_set(ViewerSet::JobResults), panel::draw.in_set(ViewerSet::Present)).run_if(in_state(ViewerMode::Robot)));
}

/// The section is shown.
pub(crate) fn shown(view: &RobotView) -> bool {
    view.section == Section::Comments
}
