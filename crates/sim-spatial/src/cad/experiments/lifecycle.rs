//! Durable pending server work: cancellation survives closing the dock and
//! losing CadDocument. Unknown POST outcomes retain ownership and are never
//! retried; discovery is a GET matched to a unique request label and stamp.
pub(crate) use super::reads::{compare, read_candidate, read_selected, refresh};
use super::*;
use crate::jobs::{Job, Pool};
use sim_runtime::cad_client::{CadClient, experiments::ExperimentRecord};
use std::time::{Duration, Instant};
pub(crate) struct Active {
    pub stamp: Stamp,
    pub expected_revision: u64,
    pub client: CadClient,
    pub label: String,
    pub mutation: bool,
    pub cancel_requested: bool,
    pub uncertain: Option<String>,
    pub status: Option<ExperimentRecord>,
    pub start: Option<Job<Value>>,
    pub poll: Option<Job<ExperimentRecord>>,
    pub cancel: Option<Job<ExperimentRecord>>,
    pub discovery: Option<Job<Value>>,
    pub last: Option<Instant>,
    pub operation: ExperimentsOp,
}
impl Active {
    pub(crate) fn json(&self) -> Value {
        json!({"stamp":self.stamp,"operation":self.operation,"label":self.label,"cancel_requested":self.cancel_requested,"uncertain":self.uncertain,"status":self.status,"starting":self.start.is_some(),"nonpublishing":!self.mutation})
    }
}
pub(crate) struct Read {
    pub stamp: Stamp,
    pub kind: &'static str,
    pub restore: bool,
    pub job: Job<Value>,
}
pub(crate) struct Linked {
    pub stamp: Stamp,
    pub job: Job<BTreeMap<String, Value>>,
}
pub(crate) fn stamp(doc: &CadDocument, st: &ExperimentsState) -> Result<Stamp, String> {
    let i = st.current.unwrap_or(st.drafts.len());
    Stamp::of(doc, i, st.draft().map_or(0, |d| d.stamp.sequence))
}
pub(crate) fn value<T: Serialize>(v: T) -> Result<Value, String> {
    serde_json::to_value(v).map_err(|e| e.to_string())
}
pub(crate) fn start(
    st: &mut ExperimentsState,
    doc: &CadDocument,
    a: &ExperimentsArgs,
) -> Result<(), String> {
    if st.busy() {
        return Err(
            "experiments.start: pending operation has not reached a terminal outcome".into(),
        );
    }
    let d = form::guard_for(doc, st, true)?;
    if a.draft_index.is_some_and(|i| Some(i) != st.current)
        || a.draft_sequence.is_some_and(|seq| seq != d.stamp.sequence)
    {
        return Err("experiments.draft: switched or edited since control was drawn".into());
    }
    if !d.linked.is_empty()
        && (st.linked.is_some() || d.linked.keys().any(|key| !d.restored.contains_key(key)))
    {
        return Err("experiments.link: waiting for linked source read".into());
    }
    let s = d.stamp.clone();
    let mut request = form::request(doc, st, a.op == ExperimentsOp::Preflight)?;
    // A unique local request label makes an ambiguous creation discoverable
    // without issuing another POST or confusing two equal source drafts.
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    request.label = format!(
        "{} [native:{}:{}:{}]",
        request.label, s.generation, s.sequence, nonce
    );
    let label = request.label.clone();
    let client = doc.client.clone().ok_or("Not connected to RoboCAD")?;
    let sending = client.clone();
    let candidate = if a.op == ExperimentsOp::CandidateRun {
        let id = st.candidate.clone().ok_or("Select a candidate first")?;
        let record = st
            .candidates
            .iter()
            .find(|r| r["id"].as_str() == Some(id.as_str()))
            .ok_or("candidate.id: not listed")?;
        request.expected_revision = record["revision"]
            .as_u64()
            .ok_or("candidate.revision: missing")?;
        Some(id)
    } else {
        None
    };
    let expected_revision = request.expected_revision;
    let job = Job::spawn(
        Pool::Dedicated,
        s.generation,
        "captured experiment start",
        move |_| {
            value(
                if let Some(id) = candidate {
                    sending.candidate_experiment(&id, &request)
                } else {
                    sending.start_experiment(&request)
                }
                .map_err(|e| e.to_string())?,
            )
        },
    );
    let i = st.current.ok_or("Open draft first")?;
    st.drafts[i].submitted_sequence = Some(s.sequence);
    st.active = Some(Active {
        expected_revision,
        stamp: s,
        client,
        label,
        mutation: false,
        cancel_requested: false,
        uncertain: None,
        status: None,
        start: Some(job),
        poll: None,
        cancel: None,
        discovery: None,
        last: None,
        operation: a.op,
    });
    st.error = None;
    Ok(())
}
/// Non-source mutation (staged candidate creation/refusal); source commits use
/// the existing actions::edit_at owner below, including REST continuation.
pub(crate) fn mutate(
    st: &mut ExperimentsState,
    doc: &CadDocument,
    a: &ExperimentsArgs,
) -> Result<(), String> {
    if st.busy() {
        return Err("experiments.operation: previous outcome pending".into());
    }
    let d = form::guard(doc, st)?;
    if a.draft_index.is_some_and(|i| Some(i) != st.current)
        || a.draft_sequence
            .is_some_and(|sequence| sequence != d.stamp.sequence)
    {
        return Err("candidate.draft: editor changed; original retained".into());
    }
    let s = d.stamp.clone();
    let client = doc.client.clone().ok_or("Not connected to RoboCAD")?;
    let sending = client.clone();
    let operation = a.op;
    let candidate = st.candidate.clone();
    let label = if operation == ExperimentsOp::CandidateCreate {
        format!(
            "{} [candidate-native:{}:{}]",
            d.get("label"),
            s.generation,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos()
        )
    } else {
        candidate.clone().unwrap_or_default()
    };
    let request = if operation == ExperimentsOp::CandidateCreate {
        let mut request = form::candidate_request(doc, st)?;
        request.label = label.clone();
        Some(request)
    } else {
        None
    };
    let job = Job::spawn(
        Pool::Dedicated,
        s.generation,
        "candidate authoring",
        move |_| match operation {
            ExperimentsOp::CandidateCreate => value(
                sending
                    .create_candidate(&request.ok_or("candidate.request: missing")?)
                    .map_err(|e| e.to_string())?,
            ),
            ExperimentsOp::CandidateDiscard => value(
                sending
                    .discard_candidate(&candidate.ok_or("candidate.id: select first")?)
                    .map_err(|e| e.to_string())?,
            ),
            _ => Err("source changes must use shared CAD edit handler".into()),
        },
    );
    st.active = Some(Active {
        expected_revision: s.revision,
        stamp: s,
        client,
        label,
        mutation: false,
        cancel_requested: false,
        uncertain: None,
        status: None,
        start: Some(job),
        poll: None,
        cancel: None,
        discovery: None,
        last: None,
        operation,
    });
    Ok(())
}
pub(crate) fn ambiguous(error: &str) -> bool {
    error.contains("may still apply") || error.contains("applied it, but")
}
pub(crate) fn discover(st: &mut ExperimentsState) -> Result<(), String> {
    let a = st.active.as_mut().ok_or("No ambiguous operation pending")?;
    if a.uncertain.is_none() {
        return Err("experiments.discovery: no unknown POST outcome".into());
    }
    if a.discovery.is_some() {
        return Err("experiments.discovery: already reading".into());
    }
    let client = a.client.clone();
    let operation = a.operation;
    let id = a.label.clone();
    a.discovery = Some(Job::spawn(
        Pool::Dedicated,
        a.stamp.generation,
        "discover outcome without publication",
        move |_| match operation {
            ExperimentsOp::CandidateCreate => {
                value(client.candidates().map_err(|e| e.to_string())?)
            }
            ExperimentsOp::CandidateDiscard => {
                value(client.candidate(&id).map_err(|e| e.to_string())?)
            }
            _ => value(client.experiments().map_err(|e| e.to_string())?),
        },
    ));
    Ok(())
}
pub(crate) fn recover(a: &mut Active, records: Vec<ExperimentRecord>) -> Result<(), String> {
    let mut matches = records.into_iter().filter(|r| {
        r.label == a.label
            && r.document_id.as_deref() == Some(a.stamp.document_id.as_str())
            && r.revision == Some(a.expected_revision)
    });
    let first = matches
        .next()
        .ok_or("experiments.outcome: no matching record yet; keep pending, no retry")?;
    if matches.next().is_some() {
        return Err(
            "experiments.outcome: multiple matching records; inspect history, no retry".into(),
        );
    }
    a.status = Some(first);
    a.uncertain = None;
    Ok(())
}
pub(crate) fn recover_candidate(a: &mut Active, rows: Value) -> Result<Value, String> {
    if a.operation == ExperimentsOp::CandidateDiscard {
        if rows["id"].as_str() == Some(a.label.as_str())
            && rows["document_id"].as_str() == Some(a.stamp.document_id.as_str())
            && rows["state"] == "discarded"
        {
            a.uncertain = None;
            return Ok(rows);
        }
        return Err("candidate.refusal: outcome still unknown; record retained, no retry".into());
    }
    let matches = rows
        .as_array()
        .ok_or("candidate.discovery: expected list")?
        .iter()
        .filter(|r| {
            r["label"].as_str() == Some(a.label.as_str())
                && r["document_id"].as_str() == Some(a.stamp.document_id.as_str())
                && r["base_revision"].as_u64() == Some(a.stamp.revision)
        })
        .cloned()
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(format!(
            "candidate.discovery: {} matching records; outcome retained, no retry",
            matches.len()
        ));
    }
    a.uncertain = None;
    Ok(matches[0].clone())
}
pub(crate) fn accept_status(a: &mut Active, status: ExperimentRecord) {
    if a.status.as_ref().is_some_and(ExperimentRecord::terminal) {
        return;
    }
    if status.document_id.as_deref() != Some(a.stamp.document_id.as_str())
        || status.revision != Some(a.expected_revision)
    {
        a.cancel_requested = true;
        return;
    }
    if a.status
        .as_ref()
        .is_some_and(|previous| previous.id != status.id || previous.updated_at > status.updated_at)
    {
        return;
    }
    a.status = Some(status);
}
pub(crate) fn tick(
    mut st: ResMut<ExperimentsState>,
    doc: Option<Res<CadDocument>>,
    mut actions: MessageWriter<crate::app::actions::Act<CadAction>>,
) {
    if let Some(mut a) = st.active.take() {
        if !doc.as_deref().is_some_and(|d| a.stamp.document_matches(d)) {
            a.cancel_requested = true;
        }
        let mut ended = false;
        if let Some(result) = a.start.as_ref().and_then(Job::poll) {
            a.start = None;
            match result {
                Ok(answer) => {
                    if matches!(
                        a.operation,
                        ExperimentsOp::Run | ExperimentsOp::Preflight | ExperimentsOp::CandidateRun
                    ) {
                        match serde_json::from_value::<ExperimentRecord>(answer) {
                            Ok(r)
                                if r.document_id.as_deref()
                                    == Some(a.stamp.document_id.as_str())
                                    && r.revision == Some(a.expected_revision)
                                    && r.label == a.label =>
                            {
                                a.status = Some(r)
                            }
                            Ok(_) => {
                                a.uncertain=Some("experiments.start: mismatched captured identity; discover by unique label, no retry".into());
                                st.error = a.uncertain.clone();
                            }
                            Err(e) => {
                                a.uncertain = Some(format!("answer decoded incorrectly: {e}"));
                                st.error = a.uncertain.clone();
                            }
                        }
                    } else {
                        st.candidates.retain(|r| r["id"] != answer["id"]);
                        st.candidates.insert(0, answer.clone());
                        if a.stamp.draft_matches(&st)
                            && doc.as_deref().is_some_and(|d| a.stamp.document_matches(d))
                        {
                            st.candidate = answer["id"].as_str().map(str::to_string);
                            st.diagnostics = Some(answer);
                        }
                        ended = true;
                    }
                }
                Err(e) => {
                    if ambiguous(&e) {
                        a.uncertain = Some(e.clone());
                    } else {
                        ended = true;
                    }
                    st.error = Some(e);
                }
            }
            st.touch();
        }
        if let Some(result) = a.discovery.as_ref().and_then(Job::poll) {
            a.discovery = None;
            match result {
                Ok(rows)
                    if matches!(
                        a.operation,
                        ExperimentsOp::CandidateCreate | ExperimentsOp::CandidateDiscard
                    ) =>
                {
                    match recover_candidate(&mut a, rows) {
                        Ok(record) => {
                            st.candidates.retain(|r| r["id"] != record["id"]);
                            st.candidates.insert(0, record);
                            ended = true;
                        }
                        Err(e) => st.error = Some(e),
                    }
                }
                Ok(rows) => match serde_json::from_value(rows)
                    .map_err(|e| e.to_string())
                    .and_then(|records| recover(&mut a, records))
                {
                    Ok(()) => {}
                    Err(e) => st.error = Some(e),
                },
                Err(e) => st.error = Some(e),
            }
            st.touch();
        }
        if let Some(result) = a.cancel.as_ref().and_then(Job::poll) {
            a.cancel = None;
            match result {
                Ok(r) => accept_status(&mut a, r),
                Err(e) => st.error = Some(e),
            }
            st.touch();
        }
        if let Some(result) = a.poll.as_ref().and_then(Job::poll) {
            a.poll = None;
            match result {
                Ok(r) => accept_status(&mut a, r),
                Err(e) => st.error = Some(e),
            }
            st.touch();
        }
        if let Some(status) = &a.status {
            if status.terminal() {
                ended = true;
                let row = serde_json::to_value(status).unwrap_or(Value::Null);
                st.history
                    .retain(|r| r["id"].as_str() != Some(status.id.as_str()));
                st.history.insert(0, row);
                if a.stamp.draft_matches(&st)
                    && doc.as_deref().is_some_and(|d| a.stamp.document_matches(d))
                {
                    st.selected = Some(status.id.clone());
                }
                if status.preflight
                    && status.state == "completed"
                    && a.stamp.draft_matches(&st)
                    && doc.as_deref().is_some_and(|d| {
                        a.stamp.document_matches(d) && d.shown_revision() == a.stamp.revision
                    })
                {
                    st.completed_check = Some((
                        a.stamp.generation,
                        a.stamp.document_id.clone(),
                        a.stamp.revision,
                        status.id.clone(),
                    ));
                }
            } else if a
                .last
                .is_none_or(|t| t.elapsed() >= Duration::from_millis(500))
            {
                let id = status.id.clone();
                let client = a.client.clone();
                if a.cancel_requested && a.cancel.is_none() {
                    a.cancel = Some(Job::spawn(
                        Pool::Dedicated,
                        a.stamp.generation,
                        "cancel captured experiment",
                        move |_| client.cancel_experiment(&id).map_err(|e| e.to_string()),
                    ));
                } else if !a.cancel_requested && a.poll.is_none() {
                    a.poll = Some(Job::spawn(
                        Pool::Dedicated,
                        a.stamp.generation,
                        "captured experiment status",
                        move |_| client.experiment(&id).map_err(|e| e.to_string()),
                    ));
                }
                a.last = Some(Instant::now());
            }
        }
        if !ended {
            st.active = Some(a);
        }
        st.touch();
    }
    let Some(doc) = doc.as_deref() else {
        return;
    };
    let current_identity = doc
        .doc_key
        .as_ref()
        .and_then(|(id, _)| id.clone())
        .map(|id| (doc.generation, id));
    if st.read_identity.is_some() && st.read_identity != current_identity {
        st.read_identity = current_identity;
        st.last_read = None;
        st.catalogue = None;
        st.candidates.clear();
        st.selected = None;
        st.candidate = None;
        st.baseline = None;
        st.completed_check = None;
        st.inputs = None;
        st.diagnostics = None;
        st.comparison = None;
        st.focus = None;
        st.focus_index = None;
        st.touch();
    }
    if let Some((s, kind, restore, result)) = st.read.as_ref().and_then(|r| {
        r.job
            .poll()
            .map(|v| (r.stamp.clone(), r.kind, r.restore, v))
    }) {
        st.read = None;
        if s.document_matches(doc) {
            match result {
                Err(e) => st.error = Some(e),
                Ok(v) => match kind {
                    "list" => {
                        st.history = v["runs"].as_array().cloned().unwrap_or_default();
                        st.candidates = v["candidates"].as_array().cloned().unwrap_or_default();
                        st.catalogue = (!v["catalogue"].is_null()).then(|| v["catalogue"].clone());
                        st.error = v["catalogue_error"].as_str().map(str::to_string);
                        st.read_identity = Some((s.generation, s.document_id.clone()));
                    }
                    "selected" => {
                        if s.draft_matches(&st) || st.current.is_none() {
                            st.diagnostics = Some(v["diagnostics"].clone());
                            if restore
                                && (doc.shown_revision() != s.revision
                                    || doc.commit_refusal(Some(s.revision)).is_some())
                            {
                                st.error=Some("experiments.restore: document revised during read; captured inputs retained on service".into());
                            } else if restore {
                                if let Err(e) = form::restored(&mut st, s, v["inputs"].clone()) {
                                    st.error = Some(e);
                                }
                            } else {
                                st.inputs = Some(v["inputs"].clone());
                            }
                        } else {
                            st.error=Some("experiments.inputs: draft changed during read; captured input remains on service".into());
                        }
                    }
                    "compare" => st.comparison = Some(v),
                    "candidate" => st.diagnostics = Some(v),
                    _ => {}
                },
            }
        }
        st.touch();
    }
    if let Some((s, result)) = st
        .linked
        .as_ref()
        .and_then(|r| r.job.poll().map(|v| (r.stamp.clone(), v)))
    {
        st.linked = None;
        if s.document_matches(doc) && s.draft_matches(&st) {
            match result {
                Ok(files) => {
                    let d = &mut st.drafts[s.draft_index];
                    for (key, bundle) in files {
                        if d.restored.get(&key) != Some(&bundle) {
                            let entry = bundle["entry"].as_str().unwrap_or("");
                            let text = bundle["files"][entry].as_str().unwrap_or("").to_string();
                            d.fields.insert(key.clone(), text);
                            d.restored.insert(key, bundle);
                            d.stamp.sequence += 1;
                            d.edited = Instant::now();
                        }
                    }
                }
                Err(e) => st.error = Some(e),
            }
        }
        st.touch();
    }
    if st.open
        && st.read.is_none()
        && (st.last_read.is_none()
            || (st.read_identity.as_ref().is_some_and(|(generation, id)| {
                *generation != doc.generation
                    || doc.doc_key.as_ref().and_then(|(id, _)| id.as_deref()) != Some(id.as_str())
            }) && st
                .last_read
                .is_some_and(|t| t.elapsed() >= Duration::from_millis(500))))
    {
        let _ = refresh(&mut st, doc);
    }
    if st.linked.is_none()
        && st
            .last_read
            .is_none_or(|t| t.elapsed() >= Duration::from_millis(500))
        && let Some(d) = st
            .draft()
            .filter(|d| !d.linked.is_empty() && d.stamp.document_matches(doc))
    {
        let s = d.stamp.clone();
        let paths = d.linked.clone();
        let client = doc.client.clone();
        st.linked = Some(Linked {
            stamp: s.clone(),
            job: Job::spawn(
                Pool::Io,
                s.generation,
                "linked experiment sources",
                move |_| {
                    let client = client.ok_or("Not connected to RoboCAD")?;
                    paths
                        .into_iter()
                        .map(|(key, path)| {
                            client
                                .experiment_linked_sources(&path)
                                .map(|bundle| (key, bundle))
                                .map_err(|e| format!("experiments.link.{path}: {e}"))
                        })
                        .collect()
                },
            ),
        });
        st.last_read = Some(Instant::now());
    }
    // CAD-only edits capture a new retained draft rather than silently
    // changing a source-mutation guard on the existing authored draft.
    if !st.busy() && doc.stale.is_none() && doc.commit_refusal(None).is_none() {
        if let Some(mut d) = st
            .draft()
            .filter(|d| {
                d.auto && d.stamp.document_matches(doc) && d.stamp.revision != doc.shown_revision()
            })
            .cloned()
        {
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
    if !st.busy()
        && st.linked.is_none()
        && let Some(d) = st.draft().filter(|d| {
            d.auto
                && d.edited.elapsed() >= Duration::from_millis(750)
                && d.submitted_sequence != Some(d.stamp.sequence)
                && d.stamp.document_matches(doc)
                && doc.shown_revision() == d.stamp.revision
        })
    {
        actions.write(crate::app::actions::Act::ui(
            ExperimentsArgs {
                draft_index: st.current,
                draft_sequence: Some(d.stamp.sequence),
                ..ExperimentsArgs::of(ExperimentsOp::Run)
            }
            .action(),
        ));
        // Record queued sequence to avoid enqueueing each frame before Actions.
        let sequence = d.stamp.sequence;
        if let Some(i) = st.current {
            st.drafts[i].submitted_sequence = Some(sequence);
        }
    }
}
