//! RoboCAD's comment threads in Robot mode (cad-annotation-parity AN3;
//! native-viewer.md §7 "one annotations service, one thread panel"): the
//! comments of the CAD source an export names, shown beside the links they
//! are on, through the same `annotations::ThreadSource` and the same
//! `ui_kit::threads` panel as CAD mode.
//!
//! - **Where they are.** RoboCAD keeps the threads in its document. Robot
//!   mode starts no RoboCAD and reads no `.rcad`: it reads them only from the
//!   service the window remembers attaching to in CAD mode (the document
//!   registry's CAD source when it is `Source::Url`), and only when that
//!   service's document is the export's CAD source (`GET /` `path` against
//!   `cad_link`'s resolved path, both canonicalized in the job:
//!   [`same_source`]). Otherwise the section says where the comments live
//!   and offers Open in CAD; no thread is shown.
//! - **Reads** ([`read`]): health, threads and nodes on one job per
//!   (service, export, CAD source, epoch); every 2 s while shown, a probe
//!   re-reads when RoboCAD's revision moved, so an edit made in a RoboCAD
//!   window appears without a refresh. The last list stays shown while a
//!   newer one is read, labelled with the revision it was read at.
//! - **Mapping** ([`link_of`], [`place`]): a thread's part is on link `i` when
//!   it is the link's CAD body (`links[i].id`) or one of its members, else
//!   the nearest ancestor that is (RoboCAD's node parents, cycle-safe). No
//!   name matching. A thread on no link, an evidence thread, and every
//!   thread of an export that is not current with its CAD file carry a
//!   warning line ([`link_note`]).
//! - **Changes** ([`RobotCadThreads`], Robot mode's ThreadSource; acts in `act`): reply, edit, delete a message and
//!   resolve, each one RoboCAD call (`cad::threads::request_on`, the same
//!   requests as CAD mode) on a job that first checks RoboCAD still has the
//!   CAD source open at the revision the threads were read at. New threads
//!   are placed in CAD mode; undo is RoboCAD's.
//! - **Selection.** Opening a thread selects it and its link; a part chip or
//!   a `[label](part:ID)` link whose node is on a link selects that link
//!   (`picked::select`, the one selection, keeping the inspector's scroll).
//! - **Open in CAD** (`act::open_in_cad`): the CAD document switch, with the
//!   thread to reveal in CAD's state (`cad::threads::RevealThread`).
use super::{RobotView, Section};
use crate::annotations::{Committed, InFlight, ThreadSource};
use crate::app::ViewerMode;
use crate::cad::threads::{CadAnchor, DELETE_COMMENT, DELETE_THREAD, Request, UNDO_IS_ROBOCADS, plain, request_on, thread_of};
use crate::document::{DocumentRegistry, Source};
use crate::jobs::{Job, Pool};
use crate::ui_kit::threads::Shown;
use bevy::prelude::*;
use serde_json::{Value, json};
use sim_annotate::{Thread, ThreadCommand};
use sim_domain_robot::cad_link::CadLinkStatus;
use sim_runtime::cad_client::CadClient;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

mod act;
mod panel;
mod read;
#[cfg(test)]
mod tests;

pub(crate) use act::{ThreadsAct, cad_document, from_rest, handle, reveal_target};
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

/// RoboCAD's node parents (`GET /nodes`): id → parent id.
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

/// Whether RoboCAD's document is the export's CAD source, given both paths
/// canonical (the job canonicalizes them; None: a new, unsaved document).
pub(crate) fn same_source(service: Option<&Path>, cad: &Path) -> bool {
    service.is_some_and(|s| s == cad)
}

/// [`same_source`] for RoboCAD's `GET /` path. Touches the filesystem: jobs only.
fn service_has(service: Option<&str>, cad: &Path) -> bool {
    let Some(service) = service.filter(|s| !s.is_empty()) else { return false };
    match (std::fs::canonicalize(service), std::fs::canonicalize(cad)) {
        (Ok(a), Ok(b)) => same_source(Some(a.as_path()), &b),
        _ => false,
    }
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

/// A change refused while the CAD source is not open in an attached RoboCAD.
pub(crate) fn not_open(file: &str) -> String {
    format!("{file} is not open in CAD mode: open it there to reply")
}

/// Where the comments are when they cannot be shown here.
pub(crate) fn lives_in(file: &str) -> String {
    format!("Comments live in the CAD source {file}: open it in CAD mode to see them")
}

/// A change refused while another is being sent.
pub(crate) const BUSY: &str = "Another comment change is being sent to RoboCAD; wait for it";
/// A change refused because RoboCAD's document was reopened or replaced since the threads were read.
pub(crate) const REPLACED: &str = "RoboCAD's document was reopened or replaced since its comments were read: they are read again";
/// A change refused because RoboCAD's document moved since the threads were read.
pub(crate) fn moved(read: u64, now: u64) -> String {
    format!("RoboCAD's document moved since its comments were read (revision {read}, now {now}): they are read again")
}

/// The RoboCAD service this window attached to in CAD mode and remembers
/// (the registry's CAD source when it is a URL; a self-started one was
/// stopped when CAD mode was left, unless it held unsaved edits, and is then
/// remembered by URL too).
pub(crate) fn attached_url(registry: &DocumentRegistry) -> Option<String> {
    match registry.source(ViewerMode::Cad)? {
        Source::Url { url } => Some(url.clone()),
        _ => None,
    }
}

/// What a read is about: the attached service, the export and its CAD source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Base {
    pub url: String,
    pub simrobot: PathBuf,
    pub cad: PathBuf,
}

/// The read's subject now (None: no attached service, or no CAD file resolved).
pub(crate) fn base_of(view: &RobotView, registry: &DocumentRegistry) -> Option<Base> {
    let cad = cad_path(view.cad_link.as_ref()?)?.to_path_buf();
    let url = attached_url(registry)?;
    Some(Base { url, simrobot: view.path.clone(), cad })
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
    /// The threads as read for `base`, when that service has the CAD source open.
    pub(crate) fn open_listed(&self, base: Option<&Base>) -> Option<&read::Listed> {
        let base = base?;
        if !self.read.open(base) {
            return None;
        }
        self.read.listed_for(base)
    }
}

/// Robot mode's ThreadSource over RoboCAD's threads (the CAD anchor): the threads as read while the CAD source is open,
/// each change one RoboCAD call on a job (see the module doc).
pub(crate) struct RobotCadThreads<'a> {
    pub st: &'a mut RobotThreads,
    /// The attached service and the export's CAD source now.
    pub base: Option<Base>,
    /// The CAD source's name (the refusal names it).
    pub file: String,
}
impl RobotCadThreads<'_> {
    /// The service, revision and document id the shown threads were read at, when open.
    fn opened(&self) -> Option<(Base, u64, Option<String>)> {
        let listed = self.st.open_listed(self.base.as_ref())?;
        Some((listed.base.clone(), listed.revision, listed.document_id.clone()))
    }
}
impl ThreadSource for RobotCadThreads<'_> {
    type Anchor = CadAnchor;
    /// Never sent: RoboCAD assigns ids.
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
            return Err(UNDO_IS_ROBOCADS.into());
        }
        let Some((base, revision, document)) = self.opened() else { return Err(not_open(&self.file)) };
        // One change at a time: each is checked against the revision the threads were read at.
        if self.st.in_flight.busy() {
            return Err(BUSY.into());
        }
        let threads = self.threads();
        let (label, thread, request) = request_on(&threads, label, command)?;
        let client = CadClient::new(&base.url).map_err(|e| e.message)?;
        self.st.next += 1;
        let seq = self.st.next;
        let cad = base.cad;
        // A started change runs to its end (its answer is RoboCAD's undo step either way).
        let job = Job::spawn(Pool::Dedicated, seq, "robot-cad-thread-change", move |_| commit_job(&client, &cad, (revision, document), request)).complete_on_drop();
        self.st.commits.push((seq, job));
        self.st.in_flight.submitted(seq, &label, thread);
        Ok(Committed::Pending(seq))
    }
}

/// The change's job: RoboCAD still has the CAD source open, as the same
/// document at the revision the threads were read at, then the one call.
/// (Messages made here never start with "RoboCAD ", so `plain` keeps them.)
fn commit_job(client: &CadClient, cad: &Path, (revision, document): (u64, Option<String>), request: Request) -> Result<Value, String> {
    let health = client.health().map_err(|e| e.to_string())?;
    if !service_has(health.path.as_deref(), cad) {
        return Err(format!("The CAD source {} is no longer open in the RoboCAD at {}: nothing was sent", cad.display(), client.url()));
    }
    if health.document_id != document {
        return Err(REPLACED.to_string());
    }
    if health.revision != revision {
        return Err(moved(revision, health.revision));
    }
    request.send(client).map_err(|e| e.to_string())
}

/// A change's answer, landed (JobResults): the threads are read again; a
/// post's draft ends when RoboCAD took it unchanged; an error stays under
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
pub(crate) fn line(view: &RobotView, registry: &DocumentRegistry, st: &RobotThreads) -> String {
    let Some(status) = view.cad_link.as_ref() else {
        return "The export's CAD source is not known (the robot is loading, or this file has no CAD link).".to_string();
    };
    if cad_path(status).is_none() {
        return format!("{}: its comments cannot be read.", source_line(status));
    }
    let file = source_file(Some(status));
    let Some(base) = base_of(view, registry) else { return lives_in(&file) };
    match st.read.reach(&base) {
        None if st.read.reading() => format!("Reading RoboCAD's comments at {}…", base.url),
        None => lives_in(&file),
        Some(read::Reach::Elsewhere(other)) => format!("RoboCAD at {} has {} open, not {file}. {}", base.url, other.as_deref().unwrap_or("a new document"), lives_in(&file)),
        // `e` is RoboCAD's message or the transport's, without a request line.
        Some(read::Reach::Failed(e)) => format!("RoboCAD at {} could not be read ({e}). {}", base.url, lives_in(&file)),
        Some(read::Reach::Open) => match st.read.listed_for(&base) {
            Some(l) => format!("Comments of {file} as read at RoboCAD revision {}", l.revision),
            None => lives_in(&file),
        },
    }
}

/// Each listed thread's place (by id).
pub(crate) fn placements(listed: &read::Listed, links: &[LinkKeys], note: Option<&str>) -> BTreeMap<String, Placed> {
    listed.threads.iter().map(|t| (t.id.clone(), place(&thread_of(t), links, &listed.parents, note))).collect()
}

/// `robot_state.cad_threads`.
pub(crate) fn state_json(view: &RobotView, registry: &DocumentRegistry, st: &RobotThreads) -> Value {
    let base = base_of(view, registry);
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
        "url": attached_url(registry),
        "source": view.cad_link.as_ref().and_then(cad_path),
        "line": line(view, registry, st),
        "read_at_revision": listed.map(|l| l.revision),
        "reading": st.read.reading(),
        "threads": threads,
    });
    out["filter"] = json!(st.filter);
    out["current"] = json!(st.current);
    out["menu"] = json!(st.menu);
    out["draft"] = json!({"text": st.compose, "editing": st.editing, "author": st.author, "sending": st.sending.as_ref().map(|s| s.0), "error": st.error});
    out["in_flight"] = json!(st.in_flight.busy());
    out["undo"] = json!(UNDO_IS_ROBOCADS);
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
