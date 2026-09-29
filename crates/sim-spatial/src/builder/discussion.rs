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
pub(super) enum Action {
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
    sim_inspect::annotations::PhysicalView {
        focus: o.focus.to_array(),
        radius: o.radius,
        yaw: o.yaw,
        pitch: o.pitch,
        exploded: scene.state.exploded,
        connections: scene.state.connections,
        hidden: scene.state.hidden.clone(),
    }
}
fn restore(scene: &mut SpatialScene, o: &mut Orbit, v: sim_inspect::annotations::PhysicalView) {
    o.focus = Vec3::from_array(v.focus);
    o.radius = v.radius;
    o.yaw = v.yaw;
    o.pitch = v.pitch;
    o.home = false;
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
            o.focus = center;
            o.radius = scene
                .spatial
                .parts
                .iter()
                .filter(|p| details.components.contains(&p.component))
                .map(|p| (Vec3::from_array(p.position) - center).length() + 0.04)
                .fold(0.04, f32::max)
                * 3.;
            o.home = false;
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

pub(super) fn act(b: &mut Builder, scene: &mut SpatialScene, o: &mut Orbit, action: Action) {
    if matches!(action, Action::Reply)
        && b.input
            .as_ref()
            .is_some_and(|i| i.purpose == Purpose::Comment)
    {
        return;
    }
    if matches!(action, Action::Submit) {
        if b.input
            .as_ref()
            .is_some_and(|i| i.purpose == Purpose::CommentAuthor)
        {
            b.commit_input();
        } else {
            submit(b, scene, o);
        }
        return;
    }
    if matches!(action, Action::CancelDraft) {
        b.input = None;
        b.discussion.editing = None;
        b.discussion.original_body = None;
        b.discussion.error = None;
        if b.discussion
            .selected
            .as_ref()
            .is_some_and(|id| !b.document.discussions.threads.contains_key(id))
        {
            b.discussion.selected = None;
        }
        b.panel_dirty = true;
        return;
    }
    let selected = b.discussion.selected.clone();
    if b.input.is_some() && !matches!(action, Action::Target(_) | Action::Back) {
        b.status = "Finish or cancel the current draft first.".into();
        b.action_error = Some(b.status.clone());
        return;
    }
    let request = match action {
        Action::Submit | Action::CancelDraft => unreachable!(),
        Action::List => {
            b.discussion.selected = None;
            b.discussion.more = false;
            b.discussion.error = None;
            b.discussion.comment_menu = None;
            b.discussion.reset_scroll = true;
            None
        }
        Action::More => {
            b.discussion.more = !b.discussion.more;
            None
        }
        Action::CommentMore(id) => {
            b.discussion.comment_menu = if b.discussion.comment_menu.as_ref() == Some(&id) {
                None
            } else {
                Some(id)
            };
            None
        }
        Action::New => {
            b.discussion.reset_scroll = true;
            b.discussion.draft_targets = b.selected.iter().map(|n| b.full_path(n)).collect();
            if b.discussion.draft_targets.is_empty() {
                b.status = "Select parts or a group first.".into();
                return;
            }
            b.discussion.selected = None;
            b.discussion.editing = None;
            b.discussion.draft_pin = Some([0.; 3]);
            b.start_input(Purpose::Comment, String::new());
            None
        }
        Action::Open(id) => {
            b.discussion.more = false;
            b.discussion.error = None;
            b.discussion.comment_menu = None;
            b.discussion.reset_scroll = true;
            b.tab = Tab::Discussions;
            b.discussion.selected_only = false;
            b.discussion.selected = Some(id);
            None
        }
        Action::Reply => {
            b.discussion.editing = None;
            b.start_input(Purpose::Comment, String::new());
            None
        }
        Action::Edit(id) => {
            if let Some(c) = selected
                .as_ref()
                .and_then(|id| b.document.discussions.threads.get(id))
                .and_then(|t| t.comments.iter().find(|c| c.id == id))
            {
                let body = c.body.clone();
                b.discussion.original_body = Some(body.clone());
                b.discussion.editing = Some(id);
                b.discussion.comment_menu = None;
                b.start_input(Purpose::Comment, body);
            }
            None
        }
        Action::Author => {
            b.start_input(Purpose::CommentAuthor, b.discussion.author.clone());
            None
        }
        Action::Title => {
            if let Some(t) = selected
                .as_ref()
                .and_then(|id| b.document.discussions.threads.get(id))
            {
                b.start_input(Purpose::ThreadTitle, t.title.clone());
            }
            None
        }
        Action::OpenOnly => {
            b.discussion.open_only = !b.discussion.open_only;
            None
        }
        Action::SelectedOnly => {
            b.discussion.selected_only = !b.discussion.selected_only;
            None
        }
        Action::Delete => selected.map(|id| Request::Delete { id }),
        Action::DeleteComment(comment) => selected.map(|id| Request::DeleteComment { id, comment }),
        Action::Resolve => selected.map(|id| Request::Resolve {
            resolved: !b.document.discussions.threads[&id].resolved,
            id,
        }),
        Action::LinkSelection => selected.map(|id| Request::Link {
            id,
            targets: b.selected.iter().map(|n| b.full_path(n)).collect(),
        }),
        Action::Pin => selected.map(|id| Request::Pin {
            id,
            pin_m: Some([0.; 3]),
        }),
        Action::Show(mode) => selected.map(|id| Request::Show { id, mode }),
        Action::Target(target) => Some(Request::InspectTarget { target }),
        Action::Back => Some(Request::Back),
    };
    if let Some(r) = request {
        let result = b.discussion_request(r, None, scene, o);
        b.report(result);
    }
    b.panel_dirty = true;
}
pub(super) fn hover(
    mut scene: ResMut<SpatialScene>,
    b: Res<Builder>,
    actions: Query<(&Interaction, &BuildAction)>,
) {
    let paths = actions
        .iter()
        .filter(|(i, _)| matches!(**i, Interaction::Hovered | Interaction::Pressed))
        .flat_map(|(_, a)| match a {
            BuildAction::Discussion(Action::Target(p)) => vec![p.clone()],
            BuildAction::Discussion(Action::Open(id)) => b
                .document
                .discussions
                .threads
                .get(id)
                .map(|t| {
                    t.targets
                        .iter()
                        .chain(t.comments.iter().flat_map(|c| &c.links))
                        .filter(|r| !r.missing)
                        .map(|r| r.path.clone())
                        .collect()
                })
                .unwrap_or_default(),
            _ => vec![],
        })
        .collect::<Vec<_>>();
    scene.note_pointer_hover = selection(&scene, &paths);
}

/// Start a local draft at the actual clicked surface, expressed in its part frame.
pub(crate) fn begin_surface(b: &mut Builder, scene: &SpatialScene, index: usize, world: Vec3) {
    if b.input.is_some() {
        b.status = "Finish or cancel the current draft first.".into();
        return;
    }
    let path = scene.spatial.parts[index].component.clone();
    let transform = animation::part_transform(scene, index);
    b.discussion.draft_pin = Some(
        transform
            .compute_matrix()
            .inverse()
            .transform_point3(world)
            .to_array(),
    );
    b.discussion.draft_targets = vec![path];
    b.discussion.selected_only = false;
    b.discussion.reset_scroll = true;
    b.discussion.selected = None;
    b.discussion.editing = None;
    b.mode = Mode::Select;
    b.tab = Tab::Discussions;
    b.connect_from = None;
    b.start_input(Purpose::Comment, String::new());
    b.status = "Comment attached to this surface. Enter posts; Escape cancels.".into();
    b.panel_dirty = true;
}

pub(super) fn submit(b: &mut Builder, scene: &mut SpatialScene, o: &mut Orbit) {
    b.discussion.error = None;
    let Some(input) = b.input.as_ref() else {
        return;
    };
    if let (Some(id), Some(comment), Some(original)) = (
        &b.discussion.selected,
        &b.discussion.editing,
        &b.discussion.original_body,
    ) {
        if b.document
            .discussions
            .threads
            .get(id)
            .and_then(|t| t.comments.iter().find(|c| &c.id == comment))
            .is_none_or(|c| &c.body != original)
        {
            b.status="This reply changed in another editor. Your draft is retained; copy it before cancelling and reopening the latest reply.".into();
            b.action_error = Some(b.status.clone());
            b.discussion.error=Some("This message changed elsewhere. Your draft is safe. Cancel and reopen the latest message before editing.".into());
            b.panel_dirty = true;
            return;
        }
    }
    let body = input.buffer.trim().to_string();
    let request = if input.purpose == Purpose::ThreadTitle {
        let Some(id) = b.discussion.selected.clone() else {
            return;
        };
        Request::Title { id, title: body }
    } else if let Some(id) = b.discussion.selected.clone() {
        if let Some(comment) = b.discussion.editing.clone() {
            Request::EditComment { id, comment, body }
        } else {
            Request::Reply {
                id,
                body,
                author: b.discussion.author.clone(),
                links: vec![],
            }
        }
    } else {
        Request::Create {
            title: body
                .lines()
                .next()
                .unwrap_or("Discussion")
                .chars()
                .take(80)
                .collect(),
            body,
            author: b.discussion.author.clone(),
            targets: b.discussion.draft_targets.clone(),
            pin_m: b.discussion.draft_pin.or(Some([0.; 3])),
        }
    };
    match b.discussion_request(request, None, scene, o) {
        Ok(value) => {
            b.discussion.selected = value["thread"]["id"]
                .as_str()
                .map(str::to_string)
                .or(b.discussion.selected.clone());
            b.input = None;
            b.discussion.editing = None;
        }
        Err(e) => {
            b.action_error = Some(e.clone());
            b.discussion.error = Some(e.clone());
            b.status = e;
        }
    }
    b.panel_dirty = true;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rest_comments_preserve_the_draft_and_its_target_and_reject_conflicting_edits() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dir = std::env::temp_dir().join(format!("builder-discussion-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("review.system.json");
        std::fs::copy(
            root.join("examples/systems-builder/worm-drive/winch.system.json"),
            &path,
        )
        .unwrap();
        let registry = sim_runtime::registry_with_parts(&root.join("library/parts")).0;
        let mut b = Builder::open(path, root.join("library/systems"), registry.clone()).unwrap();
        let compiled = system_builder::compile(
            &b.document,
            &registry,
            system_builder::config_for(&b.document),
        )
        .unwrap();
        let spatial = compiled
            .spatial
            .clone()
            .unwrap_or_else(|| compiled.flat.spatial(&compiled.description.id, "Review"));
        let mut scene = SpatialScene::for_builder(compiled.description, spatial).unwrap();
        let mut orbit = Orbit {
            focus: Vec3::ZERO,
            radius: 0.5,
            yaw: 0.5,
            pitch: 0.5,
            home: false,
            ..Default::default()
        };
        b.scene_dirty = false;
        b.discussion.draft_targets = vec!["motor".into()];
        b.start_input(Purpose::Comment, "My unsent motor note".into());
        b.discussion_request(
            Request::Create {
                title: "Other discussion".into(),
                targets: vec!["gearbox".into()],
                body: "REST comment".into(),
                author: "Codex".into(),
                pin_m: None,
            },
            None,
            &mut scene,
            &mut orbit,
        )
        .unwrap();
        assert_eq!(b.input.as_ref().unwrap().buffer, "My unsent motor note");
        assert_eq!(
            b.discussion.selected, None,
            "REST must not retarget the draft"
        );
        assert!(
            !b.scene_dirty,
            "a comment must not rebuild the running model"
        );
        submit(&mut b, &mut scene, &mut orbit);
        assert!(b.input.is_none());
        let id = b.discussion.selected.clone().unwrap();
        let t = &b.document.discussions.threads[&id];
        assert_eq!(t.targets[0].path, "motor");
        let c = t.comments[0].id.clone();
        act(&mut b, &mut scene, &mut orbit, Action::Edit(c.clone()));
        b.input.as_mut().unwrap().buffer = "My local edit".into();
        b.discussion_request(
            Request::EditComment {
                id: id.clone(),
                comment: c,
                body: "External edit".into(),
            },
            None,
            &mut scene,
            &mut orbit,
        )
        .unwrap();
        submit(&mut b, &mut scene, &mut orbit);
        assert_eq!(b.input.as_ref().unwrap().buffer, "My local edit");
        assert_eq!(
            b.document.discussions.threads[&id].comments[0].body,
            "External edit"
        );
        assert!(b.status.contains("draft is retained"));
        b.input = None;
        let part = scene
            .spatial
            .parts
            .iter()
            .position(|p| p.component == "motor")
            .unwrap();
        let local = Vec3::new(0.003, 0.002, 0.001);
        let world = animation::part_transform(&scene, part).transform_point(local);
        begin_surface(&mut b, &scene, part, world);
        assert_eq!(b.tab, Tab::Discussions);
        assert_eq!(b.discussion.draft_targets, vec!["motor"]);
        assert!((Vec3::from_array(b.discussion.draft_pin.unwrap()) - local).length() < 1e-6);
        b.input.as_mut().unwrap().buffer = "Surface-specific comment".into();
        act(&mut b, &mut scene, &mut orbit, Action::Open(id.clone()));
        assert!(
            b.discussion.selected.is_none(),
            "a pin click cannot discard or retarget a draft"
        );
        submit(&mut b, &mut scene, &mut orbit);
        let surface_id = b.discussion.selected.clone().unwrap();
        assert!(
            (Vec3::from_array(b.document.discussions.threads[&surface_id].pin_m.unwrap()) - local)
                .length()
                < 1e-6
        );
        b.tab = Tab::Library;
        act(&mut b, &mut scene, &mut orbit, Action::Open(id.clone()));
        assert_eq!(b.tab, Tab::Discussions);
        assert_eq!(b.discussion.selected, Some(id));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
