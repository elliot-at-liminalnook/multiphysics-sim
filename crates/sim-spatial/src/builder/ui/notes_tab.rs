//! The Notes tab: discussion header, messages, the reply composer and the
//! agent card, drawn with the kit's thread panel (`ui_kit::threads`).
use super::*;
use crate::ui_kit::threads;

pub(super) fn discussion_header(body:&mut ChildSpawnerCommands,k:&Kit,b:&Builder){
    use discussion::Action as A;
    let action=|label:&str,a:A,look:Look|k.button(label,BuildAction::Discussion(a),look,true);
    let thread=b.discussion.selected.as_ref().and_then(|id|b.document.discussions.threads.get(id));
    if let Some(t)=thread {
        body.spawn(Node{justify_content:JustifyContent::SpaceBetween,align_items:AlignItems::Center,..default()}).with_children(|r|{
            r.spawn(action("‹ All notes",A::List,Look::Ghost));r.spawn(action("More",A::More,Look::Ghost));
        });
        body.spawn(k.text(&t.title,19.,TEXT,2));
        threads::anchors(body,k,&NotesHost{b},&t.targets);
        body.spawn(wrap()).with_children(|r|{
            r.spawn(action("Show on model",A::Show("context".into()),Look::Ghost));
            if t.resolved{r.spawn(k.text("Resolved", size::DETAIL, OK, 1));}
            if b.discussion.prior.is_some(){r.spawn(action("Restore view",A::Back,Look::Ghost));}
        });
        agent_card(body,k,b,&t.id);
        if b.discussion.more {
            body.spawn((Node{ border_radius: BorderRadius::all(Val::Px(6.)),flex_direction:FlexDirection::Column,row_gap:Val::Px(4.),padding:UiRect::all(Val::Px(8.)),..default()},BackgroundColor(RAISED))).with_children(|menu|{
                for (label,a) in [("Rename note",A::Title),("Add selected parts",A::LinkSelection),("Inspect linked parts",A::Show("parts".into())),(if t.resolved{"Reopen note"}else{"Mark resolved"},A::Resolve),("Reset marker to part origin",A::Pin)]{menu.spawn(action(label,a,Look::Ghost));}
                menu.spawn(action("Delete note",A::Delete,Look::Danger));
            });
        }
    } else {
        let draft=b.input.as_ref().is_some_and(|i|i.purpose==Purpose::Comment);
        body.spawn(Node{justify_content:JustifyContent::SpaceBetween,align_items:AlignItems::Center,..default()}).with_children(|r|{
            r.spawn(k.text(if draft{"New note"}else{"Notes"},20.,TEXT,2));
            if !draft{r.spawn(k.button("+ Add note",BuildAction::SetMode(Mode::Annotate),Look::Primary,true));}
        });
        if draft {
            body.spawn(k.text("Attached to", size::DETAIL, SUBTLE, 0));
            body.spawn(wrap()).with_children(|r|{for path in &b.discussion.draft_targets{r.spawn(action(&format!("↗ {path}"),A::Target(path.clone()),Look::Chip(false)));}});
        } else {
            body.spawn(Node{justify_content:JustifyContent::SpaceBetween,align_items:AlignItems::Center,..default()}).with_children(|r|{
                r.spawn(action(if b.discussion.open_only{"Open notes"}else{"All notes"},A::OpenOnly,Look::Chip(false)));r.spawn(action("More",A::More,Look::Ghost));
            });
            if b.discussion.more{
                body.spawn(action("Note on selected parts",A::New,Look::Ghost));
                body.spawn(action("Change your name",A::Author,Look::Ghost));
                body.spawn(action(if b.discussion.selected_only{"Show all parts"}else{"Only selected parts"},A::SelectedOnly,Look::Ghost));
                body.spawn(k.button("Import existing notes",BuildAction::ImportNotes,Look::Ghost,true));
            }
            body.spawn(k.chip(if b.agent.state.auto_answer {"Auto-answer: on"} else {"Auto-answer: off"}, BuildAction::Agent(agent::Request::Configure{auto_answer:!b.agent.state.auto_answer}), b.agent.state.auto_answer, b.agent.state.ready));
            if let Some(e)=&b.agent.state.error{body.spawn(k.text(e, size::DETAIL, WARN, 0));}
            body.spawn(k.caption(if b.mode==Mode::Annotate{"Click a surface on the model to start a note."}else{"Click a pin on the model to join its conversation."}));
        }
    }
}

/// System discussions drawn with the shared thread panel.
struct NotesHost<'a> { b: &'a Builder }
impl threads::Host<sim_system::display::Target> for NotesHost<'_> {
    type Action = BuildAction;
    fn open(&self, thread: &str) -> BuildAction { BuildAction::Discussion(discussion::Action::Open(thread.into())) }
    fn menu(&self, comment: &str) -> Option<BuildAction> { Some(BuildAction::Discussion(discussion::Action::CommentMore(comment.into()))) }
    fn edit(&self, comment: &str) -> Option<BuildAction> { Some(BuildAction::Discussion(discussion::Action::Edit(comment.into()))) }
    fn delete(&self, comment: &str) -> Option<BuildAction> { Some(BuildAction::Discussion(discussion::Action::DeleteComment(comment.into()))) }
    fn anchor(&self, t: &sim_system::display::Target) -> Option<BuildAction> { Some(BuildAction::Discussion(discussion::Action::Target(t.path.clone()))) }
    fn anchor_text(&self, t: &sim_system::display::Target) -> String { t.path.clone() }
    fn link(&self, c: &sim_system::display::Comment, link: &sim_markdown::Link) -> Option<BuildAction> {
        if let Some(path)=link.target.strip_prefix("part:").or_else(||link.target.strip_prefix("group:")){
            c.links.iter().find(|t|!t.missing&&(t.path==path||t.label==path.rsplit('/').next().unwrap_or(path))).map(|t|BuildAction::Discussion(discussion::Action::Target(t.path.clone())))
        }else if link.target.starts_with("https://")||link.target.starts_with("http://")||sim_markdown::source_location(&link.target).is_ok(){Some(BuildAction::OpenReference(link.target.clone()))}else{None}
    }
    fn badge(&self, thread: &str) -> Option<String> { self.b.agent_badge(thread) }
}

/// The open thread's messages, the draft prompt, or the list (`selected`:
/// the builder's selected names, for "Only selected parts").
pub(super) fn discussion_content(body:&mut ChildSpawnerCommands,k:&Kit,b:&Builder,selected:&BTreeSet<String>){
    let host=NotesHost{b};
    if let Some(t)=b.discussion.selected.as_ref().and_then(|id|b.document.discussions.threads.get(id)){
        threads::messages(body,k,&host,t,b.discussion.comment_menu.as_deref());
    }else if b.input.as_ref().is_some_and(|i|i.purpose==Purpose::Comment){
        body.spawn(k.text("What would you like to discuss?",14.,SUBTLE,0));
    }else{
        let paths:Vec<_>=selected.iter().map(|n|b.full_path(n)).collect();
        let shown=b.document.discussions.threads.values().filter(|t|(!b.discussion.open_only||!t.resolved)&&(!b.discussion.selected_only||t.targets.iter().any(|r|paths.iter().any(|p|p==&r.path||r.path.starts_with(&format!("{p}/"))))));
        let count=threads::list(body,k,&host,shown);
        if count==0{body.spawn(k.text("No notes here yet", size::TITLE, TEXT, 1));body.spawn(k.text("Add a note, then click the part you want to talk about.", size::ITEM, SUBTLE, 0));}
    }
}

pub(super) fn discussion_composer(body:&mut ChildSpawnerCommands,k:&Kit,b:&Builder){
    use discussion::Action as A;
    let input=b.input.as_ref().filter(|i|matches!(i.purpose,Purpose::Comment|Purpose::CommentAuthor|Purpose::ThreadTitle));
    let special=input.is_some_and(|i|i.purpose!=Purpose::Comment);
    let label=if input.is_some_and(|i|i.purpose==Purpose::CommentAuthor){"Your name"}else if input.is_some_and(|i|i.purpose==Purpose::ThreadTitle){"Note title"}else if b.discussion.editing.is_some(){"Edit message"}else if b.discussion.selected.is_none(){"Write a note"}else{"Reply"};
    let submit=if special||b.discussion.editing.is_some(){"Save"}else if b.discussion.selected.is_none(){"Post note"}else{"Post reply"};
    // A persistent footer keeps the reply field in reach while messages scroll.
    threads::composer(body,k,threads::Composer{
        identity:format!("builder-composer:{:?}:{:?}:{label}",b.discussion.selected,b.discussion.editing),
        label,draft:input.map(|i|i.buffer.as_str()),placeholder:"Write a reply…",min_height:if special{36.}else{76.},
        focus:BuildAction::Discussion(A::Reply),submit:BuildAction::Discussion(A::Submit),submit_label:submit,cancel:BuildAction::Discussion(A::CancelDraft),
        author:Some((b.discussion.author.as_str(),BuildAction::Discussion(A::Author))),error:b.discussion.error.as_deref(),
    });
}

fn agent_card(body:&mut ChildSpawnerCommands,k:&Kit,b:&Builder,id:&str){
    use agent::Request as A;
    use sim_agent::Status;
    let button=|label:&str,a:A|k.button(label,BuildAction::Agent(a),Look::Ghost,b.agent.state.ready);
    let run=b.agent.state.latest(id);
    body.spawn((Node{ border_radius: BorderRadius::all(Val::Px(6.)),flex_direction:FlexDirection::Column,row_gap:Val::Px(5.),padding:UiRect::all(Val::Px(9.)),flex_shrink:0.,min_width:Val::Px(0.),max_width:Val::Percent(100.),overflow:Overflow::clip(),..default()},BackgroundColor(RAISED))).with_children(|card|{
        card.spawn(Node{justify_content:JustifyContent::SpaceBetween,align_items:AlignItems::Center,..default()}).with_children(|row|{
            row.spawn(k.text("Codex · Astra / High", size::SMALL, TEXT, 1));
            if let Some(r)=run.filter(|r|r.status.active()) {
                row.spawn(button("Stop",A::Cancel{run:r.id.clone()}));
            }else{
                row.spawn(button("Ask Codex",A::Ask{discussion:id.into(),question:None,request_id:None}));
            }
        });
        if let Some(error)=&b.agent.state.error{card.spawn(k.text(error, size::DETAIL, WARN, 0));}
        if let Some(r)=run{
            card.spawn(k.text(if r.status.active(){format!("{} · {}s",r.activity,sim_agent::now().saturating_sub(r.created_at))}else{r.activity.clone()}, size::CAPTION, if r.status==Status::Failed{WARN}else{SUBTLE}, 0));
            if let Some(error)=&r.error{card.spawn(k.text(error, size::DETAIL, WARN, 0));}
            card.spawn(wrap()).with_children(|row|{
                row.spawn(button(if b.agent.expanded{"Hide activity"}else{"Show activity"},A::Activity));
                if matches!(r.status,Status::Failed|Status::Cancelled){row.spawn(button("Retry",A::Retry{run:r.id.clone()}));}
            });
            if let Some(count)=r.input.context["context_summary"]["scope"]["included_instances"].as_u64(){
                let resolved=r.input.context["context_summary"]["resolved"].as_bool().unwrap_or(false);
                card.spawn(k.text(format!("Context: {count} parts & groups · {}",if resolved{"model resolved"}else{"source only; model has errors"}), 10.5, SUBTLE, 0));
            }
            if b.agent.expanded{
                card.spawn(k.text(format!("Source revision {} · started {}",r.input.revision,sim_system::display::relative_time(&r.created_at.to_string())),10.,FAINT,0));
                for e in b.agent.state.events.iter().filter(|e|e.run==r.id).rev().take(3){
                    let mut message=e.message.chars().take(150).collect::<String>();if e.message.chars().count()>150{message.push('…');}
                    let mut text=k.text(message, size::DETAIL, SUBTLE, 0);text.3=TextLayout::linebreak(bevy::text::LineBreak::AnyCharacter);
                    card.spawn((text,Node{min_width:Val::Px(0.),max_width:Val::Percent(100.),..default()}));
                }
            }
        }else{card.spawn(k.text("Ask about this note and its linked parts.", size::CAPTION, SUBTLE, 0));}
    });
}
