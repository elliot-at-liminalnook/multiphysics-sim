//! Builder discussions: shared persisted commands, local drafts and reversible inspection.
use super::*;
use serde::{Deserialize, Serialize};
use sim_system::display::{Comment, Target, Thread};

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
    pub prior: Option<(sim_inspect::annotations::PhysicalView, SelectionTarget)>,
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
pub(super) fn stamp() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}
fn uid(prefix: &str) -> String {
    format!(
        "{prefix}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    )
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
impl Builder {
    fn targets(&self, paths: Vec<String>) -> Result<Vec<Target>, String> {
        paths
            .iter()
            .map(|p| sim_system::display::bind(&self.document, p).map_err(|e| e.to_string()))
            .collect()
    }
    pub(crate) fn discussion_request(
        &mut self,
        request: Request,
        expected: Option<u64>,
        scene: &mut SpatialScene,
        orbit: &mut Orbit,
    ) -> Result<serde_json::Value, String> {
        use serde_json::json;
        if expected.is_some_and(|r| r != self.document.revision) {
            return Err("stale discussion revision; read system_discussions again".into());
        }
        let mut commands = vec![];
        let mut changed = None;
        match request {
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
                if let Some((v, s)) = self.discussion.prior.take() {
                    restore(scene, orbit, v);
                    let _ = scene.set_selection(s);
                }
                self.panel_dirty = true;
                return Ok(json!({"restored":true}));
            }
            Request::InspectTarget { target } => {
                self.inspect(scene, orbit, &[target], true)?;
                return Ok(json!({"selection":scene.selection}));
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
                    "parts" => self.inspect(scene, orbit, &paths, true)?,
                    "context" => {
                        self.inspect(scene, orbit, &paths, false)?;
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
                return Ok(json!({"selection":scene.selection,"mode":mode}));
            }
            Request::Create {
                title,
                targets,
                body,
                author,
                pin_m,
            } => {
                let id = uid("thread");
                let links = self.inline_links(&body)?;
                let t = Thread {
                    id: id.clone(),
                    title,
                    targets: self.targets(targets)?,
                    resolved: false,
                    comments: vec![Comment {
                        id: uid("comment"),
                        author,
                        body,
                        created_at: stamp(),
                        edited_at: None,
                        links,
                    }],
                    pin_m,
                    view: Some(view(scene, orbit)),
                };
                changed = Some(id);
                commands.push(SystemCommand::PutThread { thread: t });
            }
            Request::Reply {
                id,
                body,
                author,
                links,
            } => {
                let mut refs = self.targets(links)?;
                refs.extend(self.inline_links(&body)?);
                commands.push(SystemCommand::AddComment {
                    thread: id.clone(),
                    comment: Comment {
                        id: uid("comment"),
                        author,
                        body,
                        created_at: stamp(),
                        edited_at: None,
                        links: refs,
                    },
                });
                changed = Some(id);
            }
            Request::Delete { id } => commands.push(SystemCommand::DeleteThread { id }),
            Request::ImportLegacy => {
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
                    commands.push(SystemCommand::PutThread {
                        thread: Thread {
                            id,
                            title: note.label.clone(),
                            targets,
                            resolved: false,
                            comments: vec![Comment {
                                id: uid("legacy-comment"),
                                author: "Legacy note".into(),
                                body: if note.text.is_empty() {
                                    note.label.clone()
                                } else {
                                    note.text.clone()
                                },
                                created_at: stamp(),
                                edited_at: None,
                                links: self.targets(links)?,
                            }],
                            pin_m: None,
                            view: None,
                        },
                    });
                }
            }
            edit => {
                let id = match &edit {
                    Request::EditComment { id, .. }
                    | Request::DeleteComment { id, .. }
                    | Request::Resolve { id, .. }
                    | Request::Link { id, .. }
                    | Request::Pin { id, .. }
                    | Request::Title { id, .. } => id,
                    _ => unreachable!(),
                };
                let mut t = self
                    .document
                    .discussions
                    .threads
                    .get(id)
                    .cloned()
                    .ok_or("unknown thread")?;
                match edit {
                    Request::EditComment { comment, body, .. } => {
                        let links = self.inline_links(&body)?;
                        let c = t
                            .comments
                            .iter_mut()
                            .find(|c| c.id == comment)
                            .ok_or("unknown comment")?;
                        c.body = body;
                        c.links = links;
                        c.edited_at = Some(stamp());
                    }
                    Request::DeleteComment { comment, .. } => {
                        let old = t.comments.len();
                        t.comments.retain(|c| c.id != comment);
                        if old == t.comments.len() {
                            return Err("unknown comment".into());
                        }
                    }
                    Request::Resolve { resolved, .. } => t.resolved = resolved,
                    Request::Link { targets, .. } => {
                        for target in self.targets(targets)? {
                            if !t.targets.iter().any(|r| r.lineage == target.lineage) {
                                t.targets.push(target);
                            }
                        }
                    }
                    Request::Pin { pin_m, .. } => t.pin_m = pin_m,
                    Request::Title { title, .. } => t.title = title,
                    _ => unreachable!(),
                }
                changed = Some(t.id.clone());
                commands.push(SystemCommand::PutThread { thread: t });
            }
        }
        if !commands.is_empty() {
            self.apply("Edit discussion", commands)?;
        }
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
    fn inspect(
        &mut self,
        scene: &mut SpatialScene,
        o: &mut Orbit,
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
            self.discussion.prior = Some((view(scene, o), scene.selection.clone()));
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
        scene.set_selection(target).map_err(|e| e.to_string())?;
        Ok(())
    }
}

mod handlers;
pub(super) use handlers::{act, hover, submit};
pub(crate) use handlers::begin_surface;

#[cfg(test)]
mod tests;
