//! Pending connections are retained user intent, never rebased onto new source.
use super::*;
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(super) struct PendingPort {
    pub endpoint: CadEndpoint,
    pub generation: u64,
    pub document_id: String,
    pub revision: u64,
}
pub(super) fn snapshot_at<'a>(
    doc: &CadDocument,
    st: &'a CadCompositionState,
    revision: u64,
) -> Result<&'a Snapshot, String> {
    let id = doc.doc.as_ref().and_then(|d| d.document_id.as_deref());
    let snapshot = st
        .snapshot
        .as_ref()
        .ok_or("composition.port: current displayed graph required")?;
    if revision != doc.shown_revision()
        || snapshot.graph.revision != revision
        || st.snapshot_key != Some((doc.generation, revision))
        || id.is_none()
        || snapshot.graph.document_id.as_deref() != id
    {
        return Err("composition.port.stamp: displayed document or revision changed; pending intent retained".into());
    }
    Ok(snapshot)
}
pub(super) fn validate_pending(
    doc: &CadDocument,
    st: &CadCompositionState,
    supplied: Option<u64>,
) -> Result<PendingPort, String> {
    not_submitted(st)?;
    let pending = st
        .pending_port
        .as_ref()
        .ok_or("composition.port: choose a port first")?;
    if supplied != Some(pending.revision)
        || pending.generation != doc.generation
        || doc.doc.as_ref().and_then(|d| d.document_id.as_deref())
            != Some(pending.document_id.as_str())
    {
        return Err("composition.port.stamp: first pick belongs to another document or revision; cancel connection to discard retained intent".into());
    }
    snapshot_at(doc, st, pending.revision)?;
    Ok(pending.clone())
}
fn endpoint_exists(snapshot: &Snapshot, endpoint: &CadEndpoint) -> Result<(), String> {
    let adapted = adapter::adapt(
        &snapshot.graph.graph,
        snapshot.graph.revision,
        &snapshot.types,
        snapshot
            .imports
            .as_ref()
            .filter(|i| !i.metadata_stale)
            .map(|i| i.imported.as_slice())
            .unwrap_or(&[]),
        &snapshot.recipes,
    )?;
    if !adapted
        .description
        .ports
        .values()
        .any(|p| p.component == endpoint.component_id && p.name == endpoint.port)
    {
        return Err(format!(
            "composition.port.{}.{}: unknown port",
            endpoint.component_id, endpoint.port
        ));
    }
    Ok(())
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct SubmittedPort {
    pub generation: u64,
    pub seq: u64,
    pub pending: PendingPort,
}
fn not_submitted(st: &CadCompositionState) -> Result<(), String> {
    if st.submitted_port.is_some() {
        return Err("composition.port: submitted source edit is awaiting acknowledgment".into());
    }
    Ok(())
}
fn dispatched(doc: &CadDocument, st: &mut CadCompositionState, previous_seq: u64) {
    if doc.edit_seq != previous_seq {
        if let Some(pending) = st.pending_port.clone() {
            st.submitted_port = Some(SubmittedPort {
                generation: doc.generation,
                seq: doc.edit_seq,
                pending,
            });
        }
    }
}
pub(super) fn answered(
    st: &mut CadCompositionState,
    generation: u64,
    seq: u64,
    result: Result<(), String>,
) {
    if !st
        .submitted_port
        .as_ref()
        .is_some_and(|sent| sent.generation == generation && sent.seq == seq)
    {
        return;
    }
    let sent = st.submitted_port.take().unwrap();
    match result {
        Ok(()) => {
            if st.pending_port.as_ref() == Some(&sent.pending) {
                st.pending_port = None;
            }
        }
        Err(error) => {
            st.error = Some(format!(
                "composition.port: source edit failed; retained intent if not cancelled: {error}"
            ));
        }
    }
    st.revision += 1;
}
pub(super) fn cancel(st: &mut CadCompositionState) -> Value {
    st.pending_port = None;
    let submitted = st.submitted_port.is_some();
    st.error = Some(
        if submitted {
            "composition.port: local connection cancelled; submitted source edit continues"
        } else {
            "composition.port: connection cancelled; source unchanged"
        }
        .into(),
    );
    json!({"cancelled":true,"submitted_edit_continues":submitted,"source_changed": if submitted {Value::Null} else {Value::Bool(false)}})
}
pub(super) fn pick(
    doc: &mut CadDocument,
    st: &mut CadCompositionState,
    call: &mut Call,
    a: &CadCompositionArgs,
) -> Result<Value, String> {
    not_submitted(st)?;
    let endpoint = a.port.clone().ok_or("composition.port: required")?;
    let revision = a
        .revision
        .ok_or("composition.revision: displayed source revision required")?;
    endpoint_exists(snapshot_at(doc, st, revision)?, &endpoint)?;
    if st.pending_port.is_some() {
        let first = validate_pending(doc, st, Some(revision))?;
        if first.endpoint == endpoint {
            return Err("composition.port: choose another port, or Leave port open".into());
        }
        let previous_seq = doc.edit_seq;
        let result = mutation(
            doc,
            st,
            call,
            GraphCommand::Connect {
                ports: vec![first.endpoint, endpoint],
            },
            first.revision,
        );
        if result.is_ok() {
            dispatched(doc, st, previous_seq);
        }
        return result;
    }
    let document_id = doc
        .doc
        .as_ref()
        .and_then(|d| d.document_id.clone())
        .ok_or("composition.document_id: required")?;
    st.pending_port = Some(PendingPort {
        endpoint,
        generation: doc.generation,
        document_id,
        revision,
    });
    Ok(json!({"connecting":st.pending_port}))
}
pub(super) fn open_command(
    doc: &CadDocument,
    st: &CadCompositionState,
    supplied: Option<u64>,
) -> Result<(GraphCommand, u64), String> {
    let pending = validate_pending(doc, st, supplied)?;
    let snapshot = snapshot_at(doc, st, pending.revision)?;
    endpoint_exists(snapshot, &pending.endpoint)?;
    if snapshot
        .graph
        .graph
        .connections
        .values()
        .any(|n| n.ports.contains(&pending.endpoint))
    {
        return Err(
            "composition.port: already connected; remove its connection before declaring it open"
                .into(),
        );
    }
    Ok((
        GraphCommand::Connect {
            ports: vec![pending.endpoint],
        },
        pending.revision,
    ))
}
pub(super) fn leave_open(
    doc: &mut CadDocument,
    st: &mut CadCompositionState,
    call: &mut Call,
    revision: Option<u64>,
) -> Result<Value, String> {
    not_submitted(st)?;
    let (command, revision) = open_command(doc, st, revision)?;
    let previous_seq = doc.edit_seq;
    let result = mutation(doc, st, call, command, revision);
    if result.is_ok() {
        dispatched(doc, st, previous_seq);
    }
    result
}
