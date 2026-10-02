//! Automatic intent is queued state with a dock epoch, separate from an actual
//! submitted request receipt. Closing only releases queued automatic markers.
use super::*;
use std::time::{Duration, Instant};
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Queued {
    pub epoch: u64,
    pub stamp: Stamp,
}
pub(crate) fn clear_queued(st: &mut ExperimentsState) {
    if let Some(queued) = st.queued_automatic.take() {
        if st.active.as_ref().is_some_and(|active| {
            active.stamp == queued.stamp
                && matches!(
                    active.operation,
                    ExperimentsOp::Run | ExperimentsOp::Preflight | ExperimentsOp::CandidateRun
                )
        }) {
            return;
        }
        if let Some(d) = st.drafts.get_mut(queued.stamp.draft_index) {
            if d.stamp == queued.stamp && d.submitted_sequence == Some(queued.stamp.sequence) {
                d.submitted_sequence = None;
            }
        }
    }
}
pub(crate) fn dock(st: &mut ExperimentsState, open: bool) {
    if st.open != open {
        st.automatic_epoch = st.automatic_epoch.wrapping_add(1);
        clear_queued(st);
    }
    st.open = open;
    if open {
        if let Some(i) = st.current {
            if let Some(d) = st.drafts.get_mut(i).filter(|d| d.auto) {
                d.edited = Instant::now();
            }
        }
    }
    if !open {
        st.request_cancel();
    }
    st.focus = None;
    st.focus_index = None;
}
pub(crate) fn configure(st: &mut ExperimentsState, enabled: Option<bool>) -> Result<(), String> {
    let index = st.current.ok_or("Open a draft first")?;
    clear_queued(st);
    st.automatic_epoch = st.automatic_epoch.wrapping_add(1);
    st.drafts[index].auto = enabled.unwrap_or(!st.drafts[index].auto);
    st.drafts[index].edited = Instant::now();
    Ok(())
}
pub(crate) fn validate(st: &ExperimentsState, a: &ExperimentsArgs) -> Result<(), String> {
    let Some(epoch) = a.automatic_epoch else {
        return Ok(());
    };
    if a.op != ExperimentsOp::Run || !st.open || epoch != st.automatic_epoch {
        return Err("experiments.automatic: dock closed or rerun belongs to an earlier dock lifetime; nothing started".into());
    }
    let queued = st
        .queued_automatic
        .as_ref()
        .ok_or("experiments.automatic: queued rerun was cancelled; nothing started")?;
    if queued.epoch != epoch
        || a.draft_index != Some(queued.stamp.draft_index)
        || a.draft_sequence != Some(queued.stamp.sequence)
        || !queued.stamp.draft_matches(st)
        || !st.draft().is_some_and(|d| d.auto)
    {
        return Err("experiments.automatic: draft or automatic setting changed since enqueue; nothing started".into());
    }
    Ok(())
}
pub(crate) fn rebase(st: &mut ExperimentsState, doc: &CadDocument) {
    if !st.open || st.busy() || doc.stale.is_some() || doc.commit_refusal(None).is_some() {
        return;
    }
    if let Some(mut d) = st
        .draft()
        .filter(|d| {
            d.auto && d.stamp.document_matches(doc) && d.stamp.revision != doc.shown_revision()
        })
        .cloned()
    {
        clear_queued(st);
        d.stamp.revision = doc.shown_revision();
        d.stamp.draft_index = st.drafts.len();
        d.stamp.sequence += 1;
        d.edited = Instant::now();
        d.submitted_sequence = None;
        st.current = Some(st.drafts.len());
        st.focus = None;
        st.focus_index = None;
        st.drafts.push(d);
        st.touch();
    }
}
pub(crate) fn enqueue(st: &mut ExperimentsState, doc: &CadDocument) -> Option<ExperimentsArgs> {
    if st.queued_automatic.as_ref().is_some_and(|q| {
        q.epoch != st.automatic_epoch
            || !q.stamp.draft_matches(st)
            || !st.draft().is_some_and(|d| d.auto)
    }) {
        clear_queued(st);
    }
    if !st.open || st.busy() || st.linked.is_some() || st.queued_automatic.is_some() {
        return None;
    }
    let d = st.draft().filter(|d| {
        d.auto
            && d.edited.elapsed() >= Duration::from_millis(750)
            && d.submitted_sequence != Some(d.stamp.sequence)
            && d.stamp.document_matches(doc)
            && doc.shown_revision() == d.stamp.revision
    })?;
    let stamp = d.stamp.clone();
    let epoch = st.automatic_epoch;
    let args = ExperimentsArgs {
        automatic_epoch: Some(epoch),
        draft_index: Some(stamp.draft_index),
        draft_sequence: Some(stamp.sequence),
        ..ExperimentsArgs::of(ExperimentsOp::Run)
    };
    st.drafts[stamp.draft_index].submitted_sequence = Some(stamp.sequence);
    st.queued_automatic = Some(Queued { epoch, stamp });
    Some(args)
}
/// Called only after a request job has been installed. Its submitted sequence
/// now belongs to a real request, so closing cannot clear that receipt.
pub(crate) fn submitted(st: &mut ExperimentsState, a: &ExperimentsArgs) {
    if a.automatic_epoch.is_some()
        || st.queued_automatic.as_ref().is_some_and(|queued| {
            st.active.as_ref().is_some_and(|active| {
                active.stamp == queued.stamp
                    && matches!(
                        active.operation,
                        ExperimentsOp::Run | ExperimentsOp::Preflight | ExperimentsOp::CandidateRun
                    )
            })
        })
    {
        st.queued_automatic = None;
    }
}
