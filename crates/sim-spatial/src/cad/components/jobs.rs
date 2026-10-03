//! One durable server-job lifecycle. Cancel intents remain state, not mode-
//! gated messages. Even a cancel during POST waits for the returned job id
//! before DELETE. No completion mutates a mismatched native document.
use super::ComponentsState;
use crate::app::actions::Act;
use crate::cad::CadAction;
use crate::cad::CadDocument;
use crate::jobs::{Job, Pool};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use sim_runtime::cad_client::{
    CadClient, CadError, ComponentJobState, ComponentJobStatus, ComponentOperation,
    ComponentStarted,
};
use std::time::{Duration, Instant};

/// What the start job answers: the job IDs RoboCAD listed before the POST
/// (lost-answer recovery may adopt only a job that is not among them), and
/// the POST's own outcome. The job's own error means the listing failed and
/// no POST was sent.
pub(crate) type Started = (Vec<String>, Result<ComponentStarted, String>);
/// How long after a start's answer was lost a listing that still shows no
/// matching new job ends the wait. The POST waits `EDIT_TIMEOUT`, longer
/// than RoboCAD's own 120 s run-on-main deadline, so by then the service has
/// begun or cancelled it; this margin covers a start it had begun.
const NOT_LISTED_AFTER: Duration = Duration::from_secs(30);
/// Marks a status or cancel error for a job id the service does not have
/// (HTTP 404): it restarted or was replaced, so the job can no longer end.
const GONE: &str = "RoboCAD no longer has this job (its service restarted or was replaced)";
/// Marks a connect failure: nothing was sent to the service.
const UNREACHABLE: &str = "the service that runs this job cannot be reached";

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) struct Identity {
    pub generation: u64,
    pub document_id: String,
}
impl Identity {
    pub fn of(doc: &CadDocument) -> Result<Self, String> {
        let document_id = doc
            .doc_key
            .as_ref()
            .and_then(|(id, _)| id.clone())
            .ok_or("components.document_id: the authoritative document has not been read")?;
        Ok(Self {
            generation: doc.generation,
            document_id,
        })
    }
    fn matches(&self, doc: &CadDocument) -> bool {
        Self::of(doc).is_ok_and(|id| id == *self)
    }
}
pub(crate) struct Active {
    pub identity: Identity,
    pub expected_revision: u64,
    pub client: CadClient,
    pub operation: ComponentOperation,
    pub start: Option<Job<Started>>,
    pub status: Option<ComponentJobStatus>,
    poll: Option<Job<ComponentJobStatus>>,
    cancel: Option<Job<ComponentJobStatus>>,
    pub cancel_requested: bool,
    cancel_sent: bool,
    last: Option<Instant>,
    uncertain: Option<String>,
    /// When the start's answer was lost (`uncertain` set).
    uncertain_at: Option<Instant>,
    recovery: Option<Job<Vec<ComponentJobStatus>>>,
    /// Job IDs listed before the POST; never adopted by recovery.
    known: Vec<String>,
    /// The retained draft this request was built from (drafts are never
    /// removed, so the index stays valid); None for a typed direct operation.
    pub draft: Option<usize>,
}
impl Active {
    pub fn new(
        identity: Identity,
        expected_revision: u64,
        client: CadClient,
        operation: ComponentOperation,
        start: Job<Started>,
        draft: Option<usize>,
    ) -> Self {
        Self {
            identity,
            expected_revision,
            client,
            operation,
            start: Some(start),
            status: None,
            poll: None,
            cancel: None,
            cancel_requested: false,
            cancel_sent: false,
            last: None,
            uncertain: None,
            uncertain_at: None,
            recovery: None,
            known: Vec::new(),
            draft,
        }
    }
}

pub(crate) fn key(st: &ComponentsState) -> String {
    format!("{}", st.revision)
}

pub(crate) fn tick(
    mut st: ResMut<ComponentsState>,
    mut doc: Option<ResMut<CadDocument>>,
    mut actions: MessageWriter<Act<CadAction>>,
) {
    if let Some((identity, target)) = &st.selection_after {
        if let Some(d) = doc.as_deref().filter(|d| identity.matches(d)) {
            if d.stale.is_none()
                && d.doc
                    .as_ref()
                    .is_some_and(|s| s.nodes.iter().any(|n| n.id == *target))
            {
                actions.write(Act::ui(CadAction::CadSelect {
                    ids: vec![target.clone()],
                    items: Vec::new(),
                    extend: false,
                    toggle: false,
                    picked_at: None,
                }));
                st.selection_after = None;
            }
        } else {
            st.selection_after = None;
        }
    }
    if let Some((identity, result)) = st
        .listing
        .as_ref()
        .and_then(|(id, j)| j.poll().map(|r| (id.clone(), r)))
    {
        st.listing = None;
        if doc.as_deref().is_some_and(|d| identity.matches(d)) {
            match result {
                Ok(list) => {
                    st.folder = list.path;
                    st.files = list.files.into_iter().map(|f| f.path).collect();
                    st.library_identity = Some(identity);
                }
                Err(e) => {
                    st.error = Some(e);
                    st.files.clear();
                    // A missing folder remains a named error until explicit
                    // refresh, rather than spawning a read every frame.
                    st.library_identity = Some(identity);
                }
            }
            st.touch();
        }
    }
    if let Some((identity, revision, result)) = st
        .reads
        .as_ref()
        .and_then(|(id, at, j)| j.poll().map(|r| (id.clone(), *at, r)))
    {
        st.reads = None;
        if doc
            .as_deref()
            .is_some_and(|d| identity.matches(d) && revision == d.shown_revision())
        {
            match result {
                Ok((c, recipes)) => {
                    st.catalogue = Some((identity, c));
                    st.recipes = Some(recipes);
                    st.catalogue_revision = revision;
                }
                Err(e) => st.error = Some(format!("components.catalogue: {e}")),
            }
            st.touch();
        }
    }
    let Some(mut active) = st.active.take() else {
        read(&mut st, doc.as_deref());
        return;
    };
    let mut ended = false;
    let op = active.operation.op_name();
    // A displaced identity (another connection or document) is cancelled
    // using its captured client, never through the replacement's service.
    let displaced = !doc.as_deref().is_some_and(|d| active.identity.matches(d));
    if let Some(result) = active.start.as_ref().and_then(Job::poll) {
        active.start = None;
        match result {
            Err(e) => {
                // The listing before the POST failed: nothing was sent.
                let error = format!("components.{op}: nothing was sent: {e}");
                report(&mut st, &active, doc.as_deref_mut(), error);
                ended = true;
            }
            Ok((known, Ok(started))) => {
                active.known = known;
                let status = started.job;
                if status.document_id != active.identity.document_id
                    || status.revision != active.expected_revision
                {
                    st.error =
                        Some("components.start: server answered for another document".into());
                    active.cancel_requested = true;
                }
                active.status = Some(status);
            }
            Ok((known, Err(e))) => {
                active.known = known;
                let error = format!("components.{op}: {e}");
                if ambiguous_start(&e) {
                    st.error = Some(error);
                    active.uncertain = Some(e);
                    active.uncertain_at = Some(Instant::now());
                } else {
                    // A refusal (e.g. RoboCAD's revision guard): the draft
                    // that was sent keeps the reason and stays editable.
                    report(&mut st, &active, doc.as_deref_mut(), error);
                    ended = true;
                }
            }
        }
        st.touch();
    }
    if let Some(result) = active.recovery.as_ref().and_then(Job::poll) {
        active.recovery = None;
        let waited = active
            .uncertain_at
            .is_some_and(|t| t.elapsed() >= NOT_LISTED_AFTER);
        match result {
            Ok(list) => match recover(&mut active, list) {
                Ok(true) => {}
                Ok(false) if waited => {
                    let error = format!(
                        "components.{op}: RoboCAD still lists no job for this request {} s after its answer was lost; nothing was retried. Check the model before applying the retained draft again",
                        NOT_LISTED_AFTER.as_secs()
                    );
                    report(&mut st, &active, doc.as_deref_mut(), error);
                    active.uncertain = None;
                    ended = true;
                }
                Ok(false) => {
                    st.error = Some(format!(
                        "components.{op}: outcome uncertain; RoboCAD lists no matching new job yet; reading again (no POST is retried)"
                    ))
                }
                Err(error) => st.error = Some(error),
            },
            Err(e) if waited && displaced => {
                let error = format!(
                    "components.{op}: the service that received this request can no longer be read ({e}); whether it changed the model is not known here"
                );
                report(&mut st, &active, doc.as_deref_mut(), error);
                active.uncertain = None;
                ended = true;
            }
            Err(e) => {
                st.error = Some(format!(
                    "components.start: outcome remains uncertain; recovering by read only: {e}"
                ))
            }
        }
        st.touch();
    }
    if active.uncertain.is_some()
        && active.recovery.is_none()
        && active
            .last
            .is_none_or(|t| t.elapsed() >= Duration::from_millis(500))
    {
        let client = active.client.clone();
        active.recovery = Some(Job::spawn(
            Pool::Dedicated,
            active.identity.generation,
            "cad component start recovery",
            move |_| client.component_jobs().map_err(|e| e.to_string()),
        ));
        active.last = Some(Instant::now());
    }
    // Cancellation always precedes a new GET: GET can commit a ready result.
    if active.cancel_requested
        && !active.cancel_sent
        && active
            .last
            .is_none_or(|t| t.elapsed() >= Duration::from_millis(250))
        && let Some(status) = &active.status
    {
        let id = status.id.clone();
        let client = active.client.clone();
        active.cancel = Some(Job::spawn(
            Pool::Dedicated,
            active.identity.generation,
            "cad component cancel",
            move |_| {
                client
                    .cancel_component_job(&id)
                    .map_err(|e| job_error(&id, ".cancel", &e))
            },
        ));
        active.cancel_sent = true;
        active.last = Some(Instant::now());
    }
    if let Some(result) = active.cancel.as_ref().and_then(Job::poll) {
        active.cancel = None;
        match result {
            Ok(status) => accept_cancel(&mut active, status),
            // The job cannot end on a service that no longer has it (unless
            // its terminal status already arrived, reported below).
            Err(e)
                if !terminal(&active)
                    && (e.contains(GONE) || (displaced && e.contains(UNREACHABLE))) =>
            {
                report(&mut st, &active, doc.as_deref_mut(), lost(op, &e));
                ended = true;
            }
            Err(e) => {
                st.error = Some(e);
                active.cancel_sent = false;
            }
        }
        st.touch();
    }
    if let Some(result) = active.poll.as_ref().and_then(Job::poll) {
        active.poll = None;
        match result {
            Ok(status) => accept_status(&mut active, status),
            Err(e) if !terminal(&active) && e.contains(GONE) => {
                report(&mut st, &active, doc.as_deref_mut(), lost(op, &e));
                ended = true;
            }
            Err(e) => st.error = Some(e),
        }
        st.touch();
    }
    if !ended
        && let Some(status) = &active.status
        && status.state.terminal()
    {
        ended = true;
        if status.document_id != active.identity.document_id
            || status.revision != active.expected_revision
        {
            st.error = Some("components.job: completion belongs to another document".into());
        } else if let Some(d) = doc.as_deref_mut().filter(|d| active.identity.matches(d)) {
            match status.state {
                ComponentJobState::Applied => {
                    crate::cad::sync::refresh(d, true);
                    st.catalogue = None;
                    let definition = status
                        .result
                        .get("definition_id")
                        .and_then(serde_json::Value::as_str)
                        .or_else(|| {
                            status.result.as_str().filter(|_| {
                                matches!(
                                    active.operation,
                                    ComponentOperation::Parametric { .. }
                                        | ComponentOperation::Create { .. }
                                        | ComponentOperation::Family { .. }
                                        | ComponentOperation::Import { .. }
                                )
                            })
                        });
                    if let Some(id) = definition {
                        st.selected = Some(id.into());
                    }
                    let target = status
                        .result
                        .get("instance_id")
                        .and_then(serde_json::Value::as_str)
                        .or_else(|| {
                            status.result.as_str().filter(|_| {
                                matches!(
                                    active.operation,
                                    ComponentOperation::Place { .. }
                                        | ComponentOperation::Detach { .. }
                                        | ComponentOperation::LinkFamily { .. }
                                )
                            })
                        });
                    if let Some(target) = target {
                        st.selection_after = Some((active.identity.clone(), target.into()));
                    }
                    // The applied form closes, as RoboCAD's dialog does; it
                    // stays in the list but is no longer offered to resume.
                    if let Some(i) = active.draft {
                        if let Some(draft) = st.drafts.get_mut(i) {
                            draft.applied = true;
                            draft.error = None;
                        }
                        if st.current == Some(i) {
                            st.current = None;
                            st.focus = None;
                        }
                    }
                    let done = if matches!(active.operation, ComponentOperation::Export { .. }) {
                        "Saved to the component library."
                    } else {
                        "Component updated. Undo restores the previous version."
                    };
                    // DELETE cannot undo a commit RoboCAD had already made.
                    d.show(Ok(if active.cancel_requested {
                        format!("Cancel arrived after RoboCAD had applied it. {done}")
                    } else {
                        done.into()
                    }));
                }
                ComponentJobState::Failed => {
                    let error = status.error.as_deref().unwrap_or("Component rebuild failed");
                    // RoboCAD's message already names its diagnostics file
                    // when it has one (component_jobs.py ComponentJob.run).
                    let log = status
                        .log_path
                        .as_ref()
                        .filter(|p| !error.contains(p.as_str()))
                        .map_or(String::new(), |p| format!("; diagnostics: {p}"));
                    let error = format!("components.{op}: {error}{log}");
                    report(&mut st, &active, Some(d), error);
                }
                ComponentJobState::Cancelled => {
                    d.show(Ok("Rebuild cancelled. Model unchanged.".into()))
                }
                _ => {}
            }
        }
        st.history.push(status.clone());
        st.touch();
    }
    if ended {
        // Only this request sets the busy guard, and only one runs at a time,
        // so it is cleared on whichever document is open now: a reconnect
        // keeps the same document resource under a new generation.
        if let Some(d) = doc.as_deref_mut() {
            d.component_busy = None;
            d.touch();
        }
    } else {
        if displaced {
            active.cancel_requested = true;
        }
        if !active.cancel_requested
            && active.poll.is_none()
            && active.start.is_none()
            && active
                .last
                .is_none_or(|t| t.elapsed() >= Duration::from_millis(250))
            && let Some(status) = &active.status
        {
            let id = status.id.clone();
            let client = active.client.clone();
            active.poll = Some(Job::spawn(
                Pool::Dedicated,
                active.identity.generation,
                "cad component status",
                move |_| client.component_job(&id).map_err(|e| job_error(&id, "", &e)),
            ));
            active.last = Some(Instant::now());
        }
        st.active = Some(active);
    }
    read(&mut st, doc.as_deref());
}

/// A refusal or failure of this request, named in the dock, on the draft it
/// was built from (not whichever form is open now) and in the status line of
/// the document it was sent for.
fn report(st: &mut ComponentsState, active: &Active, doc: Option<&mut CadDocument>, error: String) {
    st.error = Some(error.clone());
    if let Some(draft) = active.draft.and_then(|i| st.drafts.get_mut(i)) {
        draft.error = Some(error.clone());
    }
    if let Some(d) = doc.filter(|d| active.identity.matches(d)) {
        d.show(Err(error));
    }
}

fn terminal(active: &Active) -> bool {
    active.status.as_ref().is_some_and(|s| s.state.terminal())
}

/// A status or cancel error, marked when the job can no longer end there.
fn job_error(id: &str, what: &str, e: &CadError) -> String {
    if e.not_found() {
        format!("components.job.{id}{what}: {GONE}: {e}")
    } else if e.status.is_none() && e.message.contains(": connect: ") {
        format!("components.job.{id}{what}: {UNREACHABLE}: {e}")
    } else {
        format!("components.job.{id}{what}: {e}")
    }
}

fn lost(op: &str, error: &str) -> String {
    format!(
        "components.{op}: {error}; whether it changed the model is not known here: check the model and its undo history before applying the retained draft again"
    )
}

fn read(st: &mut ComponentsState, doc: Option<&CadDocument>) {
    let Some(doc) = doc.filter(|d| d.connected()) else {
        return;
    };
    let Ok(id) = Identity::of(doc) else { return };
    if st.library_identity.as_ref() != Some(&id)
        && st.listing.is_none()
        && let Some(client) = doc.client.clone()
    {
        st.listing = Some((
            id.clone(),
            Job::spawn(
                Pool::Dedicated,
                id.generation,
                "cad component saved library",
                move |_| client.component_library(None).map_err(|e| e.to_string()),
            ),
        ));
    }
    if st
        .read_at
        .is_some_and(|t| t.elapsed() < Duration::from_millis(500))
    {
        return;
    }
    if st.reads.is_some()
        || st.active.is_some()
        || st.catalogue.as_ref().is_some_and(|(at, _)| *at == id)
            && st.catalogue_revision == doc.shown_revision()
    {
        return;
    }
    let Some(client) = doc.client.clone() else {
        return;
    };
    st.read_at = Some(Instant::now());
    st.reads = Some((
        id.clone(),
        doc.shown_revision(),
        Job::spawn(
            Pool::Dedicated,
            id.generation,
            "cad components catalogue",
            move |_| {
                Ok((
                    client.components().map_err(|e| e.to_string())?,
                    client.component_recipes().map_err(|e| e.to_string())?,
                ))
            },
        ),
    ));
}

/// A completed server status is monotonic: an older GET cannot resurrect
/// running work after DELETE acknowledged cancellation.
fn ambiguous_start(error: &str) -> bool {
    error.contains("may still apply it")
        || error.contains("applied it, but its answer could not be read")
}

fn accept_status(active: &mut Active, status: ComponentJobStatus) {
    if active.status.as_ref().is_some_and(|s| s.state.terminal()) {
        return;
    }
    if status.document_id != active.identity.document_id
        || status.revision != active.expected_revision
    {
        active.cancel_requested = true;
        return;
    }
    active.status = Some(status);
}

// DELETE acknowledges the request, not necessarily worker termination. Repeat
// DELETE until terminal so a ready result never slips through a publishing GET.
fn accept_cancel(active: &mut Active, status: ComponentJobStatus) {
    accept_status(active, status);
    if active.status.as_ref().is_none_or(|s| !s.state.terminal()) {
        active.cancel_sent = false;
    }
}

/// Adopts the one new job matching the captured operation, document and
/// revision (true); false while none is listed. A job listed before the POST
/// (an earlier failed or cancelled try at the same revision) is never ours.
fn recover(active: &mut Active, list: Vec<ComponentJobStatus>) -> Result<bool, String> {
    let matching = list
        .into_iter()
        .filter(|s| {
            s.document_id == active.identity.document_id
                && s.revision == active.expected_revision
                && s.operation == active.operation.op_name()
                && !active.known.contains(&s.id)
        })
        .collect::<Vec<_>>();
    if matching.is_empty() {
        return Ok(false);
    }
    if matching.len() != 1 {
        return Err(format!(
            "components.start: unresolved request ({} matching jobs); no POST is retried",
            matching.len()
        ));
    }
    active.status = matching.into_iter().next();
    active.uncertain = None;
    active.uncertain_at = None;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn status(state: ComponentJobState) -> ComponentJobStatus {
        ComponentJobStatus {
            id: "job-a".into(),
            state,
            stage: "Receiving geometry".into(),
            done: 2,
            total: 3,
            error: None,
            log_path: None,
            result: json!(null),
            document_id: "doc-a".into(),
            revision: 4,
            operation: "import_component".into(),
        }
    }
    fn fake() -> Active {
        Active {
            identity: Identity {
                generation: 3,
                document_id: "doc-a".into(),
            },
            expected_revision: 4,
            client: CadClient::new("http://127.0.0.1:1").unwrap(),
            operation: ComponentOperation::Import {
                path: "/library/leg.rcomp".into(),
            },
            start: None,
            status: Some(status(ComponentJobState::Running)),
            poll: None,
            cancel: None,
            cancel_requested: false,
            cancel_sent: false,
            last: None,
            uncertain: None,
            uncertain_at: None,
            recovery: None,
            known: Vec::new(),
            draft: None,
        }
    }
    #[test]
    fn ambiguous_start_is_retained_until_read_only_recovery() {
        assert!(ambiguous_start(
            "RoboCAD may still apply it; refresh before retrying"
        ));
        assert!(ambiguous_start(
            "RoboCAD applied it, but its answer could not be read"
        ));
        assert!(!ambiguous_start("HTTP 409 revision conflict"));
    }
    #[test]
    fn recovering_start_keeps_pending_cancel_and_refuses_ambiguous_matches() {
        let mut active = fake();
        active.status = None;
        active.uncertain = Some("answer lost".into());
        active.cancel_requested = true;
        let mut other = status(ComponentJobState::Applied);
        other.document_id = "doc-b".into();
        assert!(recover(&mut active, vec![other, status(ComponentJobState::Ready)]).unwrap());
        assert!(active.cancel_requested);
        assert!(active.uncertain.is_none());
        assert_eq!(active.status.as_ref().unwrap().id, "job-a");
        active.status = None;
        active.uncertain = Some("answer lost".into());
        assert!(
            recover(
                &mut active,
                vec![
                    status(ComponentJobState::Ready),
                    status(ComponentJobState::Running)
                ]
            )
            .unwrap_err()
            .contains("2 matching")
        );
        assert!(active.uncertain.is_some());
        assert!(active.status.is_none());
        // A job listed before the POST (an earlier cancelled try at the same
        // revision) is never adopted: nothing new is listed yet.
        active.known = vec!["job-a".into()];
        assert!(!recover(&mut active, vec![status(ComponentJobState::Cancelled)]).unwrap());
        assert!(active.uncertain.is_some());
        assert!(active.status.is_none());
    }
    #[test]
    fn running_cancel_acknowledgement_keeps_cancellation_polling_durable() {
        let mut active = fake();
        active.cancel_requested = true;
        active.cancel_sent = true;
        accept_cancel(&mut active, status(ComponentJobState::Running));
        assert!(active.cancel_requested);
        assert!(!active.cancel_sent);
        active.cancel_sent = true;
        accept_cancel(&mut active, status(ComponentJobState::Cancelled));
        assert!(active.status.as_ref().unwrap().state.terminal());
    }
    #[test]
    fn cancellation_acknowledgment_is_not_undone_by_older_poll() {
        let mut active = fake();
        active.cancel_requested = true;
        accept_status(&mut active, status(ComponentJobState::Cancelled));
        accept_status(&mut active, status(ComponentJobState::Ready));
        assert_eq!(active.status.unwrap().state, ComponentJobState::Cancelled);
    }
    #[test]
    fn wrong_start_revision_and_document_request_cancellation_without_adoption() {
        let mut active = fake();
        let mut other = status(ComponentJobState::Applied);
        other.revision = 5;
        accept_status(&mut active, other);
        assert!(active.cancel_requested);
        assert_eq!(
            active.status.as_ref().unwrap().state,
            ComponentJobState::Running
        );
        let mut other = status(ComponentJobState::Applied);
        other.document_id = "doc-b".into();
        accept_status(&mut active, other);
        assert_eq!(active.status.unwrap().document_id, "doc-a");
    }
}
