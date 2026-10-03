//! The Comments controls (`cad:threads:<id>`, one segment: `filter-open`,
//! `thread-<id>`, `part-<node>`, `edit_message-<id>`, `delete_message-<id>`
//! and the plain ones), each with its label, action and why it is disabled
//! now: what the dock draws and `system_ui` lists. RoboCAD disables the
//! list, the filter and the thread actions while a draft is open
//! (`update_send`, ui/comments.py:235-240).
use super::ops::not_drafting;
use super::{Filter, ThreadsArgs, ThreadsOp, command_action, draft_gone, isolation, may_replace_pin, read};
use crate::cad::actions::{CadAction, Cx};
use crate::cad::document::CadDocument;
use crate::cad::panel::Control;
use crate::cad::selection::CadItems;
use sim_runtime::cad_client::{AnchorStatus, CadThread, SelectionItem};

/// The action the composer's post sends now (Post annotation, Save edit or Reply).
pub(crate) fn submit_action(doc: &CadDocument) -> Option<(CadAction, &'static str)> {
    let st = &doc.threads;
    let body = Some(st.compose.clone());
    let author = Some(st.author.clone());
    if st.pending.is_some() {
        return Some((ThreadsArgs { op: ThreadsOp::Create, body, author, ..ThreadsArgs::default() }.action(), "Post annotation"));
    }
    if let Some(comment) = &st.editing {
        return Some((ThreadsArgs { op: ThreadsOp::Edit, comment: Some(comment.clone()), body, ..ThreadsArgs::default() }.action(), "Save edit"));
    }
    st.current.as_ref().map(|t| (ThreadsArgs { op: ThreadsOp::Reply, thread: Some(t.clone()), body, author, ..ThreadsArgs::default() }.action(), "Reply"))
}

/// The threads the list shows (filter, Selected parts only), with RoboCAD's
/// number (the position in the whole list, comments.py:254).
pub(crate) fn shown_threads<'d>(doc: &'d CadDocument, selection: &[SelectionItem]) -> Vec<(usize, &'d CadThread)> {
    let st = &doc.threads;
    let selected = selection.nodes();
    let touches = |t: &CadThread| t.anchor.node_id.as_ref().is_some_and(|n| selected.contains(n)) || t.linked_parts.iter().any(|p| selected.contains(&p.part.node_id));
    read::listed(doc).unwrap_or(&[]).iter().enumerate().map(|(i, t)| (i + 1, t)).filter(|(_, t)| st.filter.keeps(t) && (!st.selected_only || touches(t))).collect()
}

/// RoboCAD's attachment line (comments.py:283); "Attachment unknown" when
/// RoboCAD's answer had no state this viewer knows.
pub(crate) fn attachment(state: AnchorStatus) -> &'static str {
    match state {
        AnchorStatus::Evidence => "Captured experiment",
        AnchorStatus::Attached => "Attached to surface",
        AnchorStatus::Missing => "Part deleted — reattach this annotation",
        AnchorStatus::NeedsReview => "Geometry changed — check and reattach this pin",
        // Not RoboCAD's: a missing or unrecognised `anchor_status` (cad_client reads it as Unknown).
        AnchorStatus::Unknown => "Attachment unknown",
    }
}

/// Every Comments control with its state now (`cad:threads:<id>`): what
/// the dock draws and `system_ui` lists.
pub(crate) fn controls_of(doc: &CadDocument, selection: &[SelectionItem]) -> Vec<Control> {
    let st = &doc.threads;
    let mut out = Vec::new();
    let mut c = |id: String, label: &str, action: CadAction, ready: Result<(), String>| out.push(Control { id: format!("cad:threads:{id}"), label: label.to_string(), action, ready });
    let draft = not_drafting(doc);
    let edits = doc.commit_refusal_for(None, true).map_or(Ok(()), Err);
    let current = st.current.as_deref().and_then(|id| read::thread(doc, id));
    let id = current.map(|t| t.id.as_str());
    let need = |more: Result<(), String>| -> Result<(), String> {
        if current.is_none() {
            return Err("Open a comment thread first".into());
        }
        draft.clone().and(more)
    };
    c("open".into(), "Comments panel", ThreadsArgs { op: ThreadsOp::Dock, open: Some(true), ..ThreadsArgs::default() }.action(), Ok(()));
    c("close".into(), "Close", ThreadsArgs { op: ThreadsOp::Dock, open: Some(false), ..ThreadsArgs::default() }.action(), Ok(()));
    // A stale pin is re-placed by Annotate, its text kept (`threads::draft_gone`).
    c("annotate".into(), "＋ Annotate model", ThreadsArgs::of(ThreadsOp::Annotate).action(), if may_replace_pin(doc) { Ok(()) } else { draft.clone() });
    for f in Filter::ALL {
        c(format!("filter-{}", f.name()), f.label(), ThreadsArgs { op: ThreadsOp::Filter, filter: Some(f), ..ThreadsArgs::default() }.action(), draft.clone());
    }
    c("selected_only".into(), "Selected parts only", ThreadsArgs::of(ThreadsOp::SelectedOnly).action(), draft.clone());
    for (n, t) in shown_threads(doc, selection) {
        let ready = if st.drafting() && id != Some(t.id.as_str()) { Err("Post or cancel your current draft before opening another thread".to_string()) } else { Ok(()) };
        let number = if t.resolved() { "✓".to_string() } else { n.to_string() };
        c(format!("thread-{}", t.id), &format!("{number} · {}", t.node_name), ThreadsArgs::thread(ThreadsOp::Open, Some(&t.id)), ready);
    }
    let show_ready = current.map_or(Ok(()), |t| {
        if t.anchor_status == AnchorStatus::Evidence {
            if t.evidence.as_ref().and_then(|e| e["run_id"].as_str()).is_some() && doc.client.is_some() { Ok(()) }
            else { Err("Captured evidence requires a run_id and a connected service".into()) }
        } else if t.anchor.node_id.is_none() { Err("This annotation has no model pin".into()) }
        else { Ok(()) }
    });
    let fit = current.map(|t| isolation::fit_nodes(doc, t)).unwrap_or_default();
    let no_parts = if fit.is_empty() { Err("This annotation has no available linked parts".to_string()) } else { Ok(()) };
    c("show".into(), "Show on model", ThreadsArgs::thread(ThreadsOp::Show, id), need(show_ready));
    c("fit".into(), "Fit in view", ThreadsArgs::thread(ThreadsOp::Fit, id), need(no_parts.clone()));
    c("reattach".into(), "Reattach…", ThreadsArgs::thread(ThreadsOp::Reattach, id), need(edits.clone()));
    let resolved = current.is_some_and(CadThread::resolved);
    c("resolve".into(), if resolved { "Reopen" } else { "Resolve" }, CadAction::CadThreads(ThreadsArgs { op: ThreadsOp::Resolve, thread: id.map(str::to_string), resolved: Some(!resolved), ..ThreadsArgs::default() }), need(edits.clone()));
    let nothing_selected = if selection.is_empty() { Err("Select parts in the outliner or viewport first".to_string()) } else { Ok(()) };
    c("link_selected".into(), "Link selected parts", ThreadsArgs::thread(ThreadsOp::Link, id), need(nothing_selected.clone().and(edits.clone())));
    let part = st.part.clone().filter(|p| current.is_some_and(|t| t.linked_parts.iter().any(|l| l.part.node_id == *p)));
    c("label".into(), "Rename part label…", CadAction::CadThreads(ThreadsArgs { op: ThreadsOp::Label, thread: id.map(str::to_string), node: part.clone(), ..ThreadsArgs::default() }), need(if part.is_none() { Err("Choose a part in Parts in this discussion first".into()) } else { Ok(()) }));
    c("show_parts".into(), "Show only linked parts", ThreadsArgs::thread(ThreadsOp::ShowParts, id), need(no_parts));
    c("return".into(), "Return to assembly", ThreadsArgs::of(ThreadsOp::Return).action(), if st.isolation.is_some() { Ok(()) } else { Err("No linked parts are shown alone".into()) });
    c("insert_link".into(), "Insert part link from selection", ThreadsArgs::of(ThreadsOp::InsertLink).action(), nothing_selected);
    if let Some(t) = current {
        for p in &t.linked_parts {
            let label = p.part.label.clone().filter(|l| !l.is_empty()).unwrap_or_else(|| p.name.clone());
            c(format!("part-{}", p.part.node_id), &label, CadAction::CadThreads(ThreadsArgs { op: ThreadsOp::Part, node: Some(p.part.node_id.clone()), ..ThreadsArgs::default() }), if p.available { Ok(()) } else { Err("this part was deleted".into()) });
        }
        let last = t.comments.len() <= 1;
        for m in &t.comments {
            c(format!("edit_message-{}", m.id), "Edit message", CadAction::CadThreads(ThreadsArgs { op: ThreadsOp::EditMessage, comment: Some(m.id.clone()), ..ThreadsArgs::default() }), draft.clone());
            let ready = draft.clone().and(edits.clone()).and(if last { Err("delete the thread to remove its last comment".into()) } else { Ok(()) });
            c(format!("delete_message-{}", m.id), "Delete message", CadAction::CadThreads(ThreadsArgs { op: ThreadsOp::Delete, comment: Some(m.id.clone()), ..ThreadsArgs::default() }), ready);
        }
    }
    c("delete_thread".into(), "Delete thread", ThreadsArgs::thread(ThreadsOp::DeleteThread, id), need(edits.clone()));
    c("pins".into(), "Toggle comment pins", command_action("view.comment_pins").unwrap_or(CadAction::State), Ok(()));
    if let Some((action, label)) = submit_action(doc) {
        let ready = match draft_gone(doc) {
            _ if st.compose.trim().is_empty() => Err("Write a message first".to_string()),
            Some(why) => Err(why),
            None => edits.clone(),
        };
        c("post".into(), label, action, ready);
    }
    c("discard".into(), "Cancel", ThreadsArgs::of(ThreadsOp::Discard).action(), if st.drafting() { Ok(()) } else { Err("No draft to cancel".into()) });
    c("cancel".into(), "Cancel Annotate", ThreadsArgs::of(ThreadsOp::Cancel).action(), if st.tool.is_some() { Ok(()) } else { Err("Annotate is not active".into()) });
    out
}

/// cad-organize's `system_ui` controls for the threads (`cad:threads:*`).
pub(in crate::cad) fn controls(cx: &Cx) -> Vec<(String, String, CadAction, Result<(), String>)> {
    controls_of(cx.doc, &cx.shared.items()).into_iter().map(|c| (c.id, c.label, c.action, c.ready)).collect()
}
