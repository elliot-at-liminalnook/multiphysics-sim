//! Discussion handlers: inspector actions, hover highlighting, starting a
//! draft on a clicked surface, and submitting drafts.
use super::*;

pub(in crate::builder) fn act(b: &mut Builder, scene: &mut SpatialScene, o: &mut Orbit, action: Action) {
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
pub(in crate::builder) fn hover(
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
    // Written only on a change: an unconditional write marks the scene changed every
    // frame, and the scene systems that skip an unchanged scene would then run each frame.
    let hover = selection(&scene, &paths);
    if scene.note_pointer_hover != hover {
        scene.note_pointer_hover = hover;
    }
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
            .to_matrix()
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

pub(in crate::builder) fn submit(b: &mut Builder, scene: &mut SpatialScene, o: &mut Orbit) {
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
