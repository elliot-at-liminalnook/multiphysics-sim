//! System discussions: the builder's thread adapter for
//! `crate::annotations` (`sim_system::display` threads, saved inside the
//! system document by the builder's own save path, undone with the system's
//! undo), the `system_discussions` request, local drafts and reversible
//! inspection of what a thread is about.
use super::*;
use crate::annotations::{self, Committed, ThreadOp, ThreadSource};
use serde::{Deserialize, Serialize};
use sim_annotate::ThreadCommand;
use sim_system::display::{Target, Thread};

#[derive(Default)]
pub(super) struct Editor {
    pub selected: Option<String>,
    pub author: String,
    pub open_only: bool,
    pub selected_only: bool,
    pub draft_targets: Vec<String>,
    pub draft_pin: Option<[f32; 3]>,
    pub markers: Vec<super::markers::MarkerInfo>,
    pub more: bool,
    pub comment_menu: Option<String>,
    pub reset_scroll: bool,
    pub error: Option<String>,
    pub editing: Option<String>,
    pub original_body: Option<String>,
    /// What an inspection replaced, for Back: the camera, the builder's
    /// selected names and what the scene showed.
    pub prior: Option<(sim_inspect::annotations::PhysicalView, BTreeSet<String>, SelectionTarget)>,
}
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Action {
    List,
    More,
    CommentMore(String),
    Submit,
    CancelDraft,
    New,
    Open(String),
    Reply,
    Edit(String),
    DeleteComment(String),
    Delete,
    Resolve,
    LinkSelection,
    Author,
    Title,
    OpenOnly,
    SelectedOnly,
    Show(String),
    Back,
    Target(String),
    Pin,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    List {
        #[serde(default)]
        target: Option<String>,
        #[serde(default)]
        resolved: Option<bool>,
        #[serde(default)]
        author: Option<String>,
    },
    Get {
        id: String,
    },
    Create {
        title: String,
        targets: Vec<String>,
        body: String,
        author: String,
        #[serde(default)]
        pin_m: Option<[f32; 3]>,
    },
    Reply {
        id: String,
        body: String,
        author: String,
        #[serde(default)]
        links: Vec<String>,
    },
    EditComment {
        id: String,
        comment: String,
        body: String,
    },
    DeleteComment {
        id: String,
        comment: String,
    },
    Resolve {
        id: String,
        resolved: bool,
    },
    Delete {
        id: String,
    },
    Link {
        id: String,
        targets: Vec<String>,
    },
    Pin {
        id: String,
        pin_m: Option<[f32; 3]>,
    },
    Title {
        id: String,
        title: String,
    },
    Show {
        id: String,
        #[serde(default = "context")]
        mode: String,
    },
    Highlight {
        #[serde(default)]
        targets: Vec<String>,
    },
    InspectTarget {
        target: String,
    },
    Back,
    ImportLegacy,
}
fn context() -> String {
    "context".into()
}
fn view(scene: &SpatialScene, o: &Orbit) -> sim_inspect::annotations::PhysicalView {
    // The view's heading now: in the trackball the stored yaw/pitch are stale,
    // and the saved view is a turntable pose (`restore` cuts to it).
    let (yaw, pitch) = o.turntable();
    sim_inspect::annotations::PhysicalView {
        focus: o.focus.to_array(),
        radius: o.radius,
        yaw,
        pitch,
        exploded: scene.state.exploded,
        connections: scene.state.connections,
        hidden: scene.state.hidden.clone(),
    }
}
fn restore(scene: &mut SpatialScene, o: &mut Orbit, v: sim_inspect::annotations::PhysicalView) {
    // A cut: also ends a glide (which would overwrite it) and the trackball.
    // A restored view stands still: a spin would turn away from it.
    o.interrupt();
    o.glide_to(crate::camera::Pose { focus: Vec3::from_array(v.focus), radius: v.radius, yaw: v.yaw, pitch: v.pitch }, 0.0);
    scene.state.hidden = v.hidden;
    scene.state.exploded = v.exploded;
    scene.state.connections = v.connections;
}
pub fn selection(scene: &SpatialScene, paths: &[String]) -> SelectionTarget {
    let ids = scene
        .description
        .components
        .keys()
        .filter(|id| {
            paths
                .iter()
                .any(|p| *id == p || id.starts_with(&format!("{p}/")))
        })
        .cloned()
        .collect::<BTreeSet<_>>();
    if ids.is_empty() {
        SelectionTarget::None
    } else {
        SelectionTarget::Components { ids }
    }
}
/// System discussions as a thread source: the builder's document holds
/// them, each edit is one system command through the builder's store (one
/// undo step), and an anchor is an instance by lineage.
pub(in crate::builder) struct SystemThreads<'a> {
    pub b: &'a mut Builder,
}
impl ThreadSource for SystemThreads<'_> {
    type Anchor = Target;
    const THREAD_ID: &'static str = "thread";
    const COMMENT_ID: &'static str = "comment";
    fn threads(&self) -> BTreeMap<String, Thread> {
        self.b.document.discussions.threads.clone()
    }
    fn thread(&self, id: &str) -> Option<Thread> {
        self.b.document.discussions.threads.get(id).cloned()
    }
    /// The same instance, by identity rather than by name.
    fn same(a: &Target, b: &Target) -> bool {
        a.lineage == b.lineage
    }
    /// `sim_system`'s thread commands validate with the same shared
    /// function (and bind lineage first); their errors keep their wording.
    fn validate(&self, _thread: &Thread) -> Result<(), String> {
        Ok(())
    }
    fn commit(&mut self, label: &str, command: ThreadCommand<Target>) -> Result<Committed, String> {
        let command = match command {
            ThreadCommand::PutThread { thread } => SystemCommand::PutThread { thread },
            ThreadCommand::DeleteThread { id } => SystemCommand::DeleteThread { id },
            ThreadCommand::AddComment { thread, comment } => SystemCommand::AddComment { thread, comment },
            ThreadCommand::Undo | ThreadCommand::Redo => return Err("system discussions are undone with the system's Undo".into()),
            edit => {
                let thread = annotations::subject(&edit).and_then(|id| self.thread(id)).ok_or("unknown thread")?;
                SystemCommand::PutThread { thread: annotations::edited(thread, edit)? }
            }
        };
        self.b.apply(label, vec![command])?;
        Ok(Committed::Done)
    }
}

impl Builder {
    fn targets(&self, paths: Vec<String>) -> Result<Vec<Target>, String> {
        paths
            .iter()
            .map(|p| sim_system::display::bind(&self.document, p).map_err(|e| e.to_string()))
            .collect()
    }
    /// `system_discussions` (and the Notes tab's actions): reads answer at
    /// once; edits are `ThreadOp`s through the annotations service.
    pub(crate) fn discussion_request(
        &mut self,
        request: Request,
        expected: Option<u64>,
        scene: &mut SpatialScene,
        orbit: &mut Orbit,
        pick: &mut Picked,
    ) -> Result<serde_json::Value, String> {
        use serde_json::json;
        if expected.is_some_and(|r| r != self.document.revision) {
            return Err("stale discussion revision; read system_discussions again".into());
        }
        let op = match request {
            Request::List {
                target,
                resolved,
                author,
            } => {
                return Ok(
                    json!({"revision":self.document.revision,"threads":self.document.discussions.threads.values().filter(|t|resolved.is_none_or(|s|s==t.resolved)&&target.as_ref().is_none_or(|p|t.targets.iter().chain(t.comments.iter().flat_map(|c|&c.links)).any(|r|&r.path==p))&&author.as_ref().is_none_or(|a|t.comments.iter().any(|c|&c.author==a))).collect::<Vec<_>>(),"timestamps":"Unix seconds (UTC)","semantics":"display annotations only; not physics inputs"}),
                );
            }
            Request::Get { id } => {
                return self
                    .document
                    .discussions
                    .threads
                    .get(&id)
                    .map(|t| json!({"revision":self.document.revision,"thread":t}))
                    .ok_or("unknown thread".into());
            }
            Request::Highlight { targets } => {
                scene.note_hover = selection(scene, &targets);
                return Ok(json!({"highlighted":scene.note_hover}));
            }
            Request::Back => {
                if let Some((v, names, shown)) = self.discussion.prior.take() {
                    restore(scene, orbit, v);
                    if pick.document().is_some() {
                        let _ = pick.set(names);
                    }
                    let _ = scene.set_selection(shown);
                    // The exact parts restored stay shown: `picked::track`
                    // would re-project whole instances over them.
                    self.seen_selection = Some(pick.selection.changed);
                }
                self.panel_dirty = true;
                return Ok(json!({"restored":true}));
            }
            Request::InspectTarget { target } => {
                self.inspect(scene, orbit, pick, &[target], true)?;
                return Ok(json!({"selection":scene.shown}));
            }
            Request::Show { id, mode } => {
                let t = self
                    .document
                    .discussions
                    .threads
                    .get(&id)
                    .cloned()
                    .ok_or("unknown thread")?;
                let paths = t
                    .targets
                    .iter()
                    .filter(|t| !t.missing)
                    .map(|t| t.path.clone())
                    .collect::<Vec<_>>();
                match mode.as_str() {
                    "highlight" => scene.note_hover = selection(scene, &paths),
                    "parts" => self.inspect(scene, orbit, pick, &paths, true)?,
                    "context" => {
                        self.inspect(scene, orbit, pick, &paths, false)?;
                        if let Some(v) = t.view {
                            restore(scene, orbit, v);
                        }
                    }
                    _ => return Err("mode must be context, parts or highlight".into()),
                }
                if self.input.is_none() {
                    self.discussion.selected = Some(id);
                    self.tab = Tab::Discussions;
                }
                self.panel_dirty = true;
                return Ok(json!({"selection":scene.shown,"mode":mode}));
            }
            Request::ImportLegacy => {
                self.import_notes(scene)?;
                self.panel_dirty = true;
                return Ok(json!({"revision":self.document.revision,"thread":null}));
            }
            Request::Create {
                title,
                targets,
                body,
                author,
                pin_m,
            } => {
                let links = self.inline_links(&body)?;
                ThreadOp::Create { title, targets: self.targets(targets)?, body, author, links, pin_m, view: Some(view(scene, orbit)) }
            }
            Request::Reply {
                id,
                body,
                author,
                links,
            } => {
                let mut refs = self.targets(links)?;
                refs.extend(self.inline_links(&body)?);
                ThreadOp::Reply { thread: id, body, author, links: refs }
            }
            Request::Delete { id } => ThreadOp::Delete { thread: id },
            Request::EditComment { id, comment, body } => {
                let links = self.inline_links(&body)?;
                ThreadOp::EditComment { thread: id, comment, body, links: Some(links) }
            }
            Request::DeleteComment { id, comment } => ThreadOp::DeleteComment { thread: id, comment },
            Request::Resolve { id, resolved } => ThreadOp::Resolve { thread: id, resolved },
            Request::Link { id, targets } => ThreadOp::Link { thread: id, targets: self.targets(targets)? },
            Request::Pin { id, pin_m } => ThreadOp::Pin { thread: id, pin_m },
            Request::Title { id, title } => ThreadOp::Retitle { thread: id, title },
        };
        let changed = annotations::apply(&mut SystemThreads { b: self }, "Edit discussion", op)?.thread;
        if self.input.is_none() {
            if let Some(id) = &changed {
                self.discussion.selected = Some(id.clone());
            }
        }
        self.panel_dirty = true;
        Ok(
            json!({"revision":self.document.revision,"thread":changed.as_ref().and_then(|id|self.document.discussions.threads.get(id))}),
        )
    }
    /// "Import existing notes": each Inspect note of the scene's sidecar not
    /// imported yet becomes a thread on its parts (one undo step).
    fn import_notes(&mut self, scene: &SpatialScene) -> Result<(), String> {
        let mut commands = vec![];
        for note in scene.note_document().notes.values() {
            let id = format!("legacy-{}", note.id);
            if self.document.discussions.threads.contains_key(&id) {
                continue;
            }
            let details = note
                .targets
                .resolve(&scene.description)
                .map_err(|e| e.to_string())?;
            let targets = self.targets(details.components.into_iter().collect())?;
            let links = note
                .links
                .iter()
                .filter_map(|l| match &l.target {
                    sim_inspect::annotations::LinkTarget::Selection { target } => {
                        target.resolve(&scene.description).ok()
                    }
                    _ => None,
                })
                .flat_map(|d| d.components)
                .collect();
            let comment = annotations::comment(
                "legacy-comment",
                "Legacy note".into(),
                if note.text.is_empty() { note.label.clone() } else { note.text.clone() },
                self.targets(links)?,
            );
            commands.push(SystemCommand::PutThread {
                thread: Thread { id, title: note.label.clone(), targets, resolved: false, comments: vec![comment], pin_m: None, view: None },
            });
        }
        if !commands.is_empty() {
            self.apply("Edit discussion", commands)?;
        }
        Ok(())
    }
    fn inline_links(&self, body: &str) -> Result<Vec<Target>, String> {
        let mut paths = vec![];
        for prefix in ["](part:", "](group:"] {
            let mut rest = body;
            while let Some((_, tail)) = rest.split_once(prefix) {
                if let Some((path, next)) = tail.split_once(')') {
                    paths.push(path.into());
                    rest = next;
                } else {
                    break;
                }
            }
        }
        self.targets(paths)
    }
    /// Frame (and with `isolate`, show only) the parts under `paths`: the
    /// builder's selection becomes the instances at its level that hold them
    /// (the shared selection), the scene shows exactly those parts, and the
    /// first inspection keeps what it replaced for Back.
    fn inspect(
        &mut self,
        scene: &mut SpatialScene,
        o: &mut Orbit,
        pick: &mut Picked,
        paths: &[String],
        isolate: bool,
    ) -> Result<(), String> {
        let target = selection(scene, paths);
        let details = target
            .resolve(&scene.description)
            .map_err(|e| e.to_string())?;
        if details.components.is_empty() {
            return Err("linked parts are missing".into());
        }
        if self.discussion.prior.is_none() {
            self.discussion.prior = Some((view(scene, o), pick.names(), scene.shown.clone()));
        }
        let mut center = Vec3::ZERO;
        let mut count = 0.;
        for p in &scene.spatial.parts {
            if details.components.contains(&p.component) {
                center += Vec3::from_array(p.position);
                count += 1.;
            }
        }
        if count > 0. {
            center /= count;
            let radius = scene
                .spatial
                .parts
                .iter()
                .filter(|p| details.components.contains(&p.component))
                .map(|p| (Vec3::from_array(p.position) - center).length() + 0.04)
                .fold(0.04, f32::max)
                * 3.;
            // A cut from the direction the view is settling on (the trackball's, when it is on).
            o.glide_frame(center, radius, 0.0);
        }
        if isolate {
            scene.state.hidden = scene
                .description
                .components
                .keys()
                .filter(|id| !details.components.contains(*id))
                .cloned()
                .collect();
        }
        if pick.document().is_some() {
            let names: BTreeSet<String> = paths.iter().filter_map(|p| self.instance_for_component(p)).collect();
            if !names.is_empty() {
                pick.set(names)?;
            }
        }
        scene.set_selection(target).map_err(|e| e.to_string())?;
        // The exact linked parts stay shown: `picked::track` would
        // re-project whole instances over them.
        self.seen_selection = Some(pick.selection.changed);
        self.panel_dirty = true;
        Ok(())
    }
}

mod handlers;
pub(super) use handlers::{act, hover, submit};
pub(crate) use handlers::begin_surface;

#[cfg(test)]
mod tests;
