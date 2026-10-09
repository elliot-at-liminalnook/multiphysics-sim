//! Nonpublishing authoritative reads, each bound to the captured document/draft stamp.
use super::lifecycle::{Read, stamp, value};
use super::*;
use crate::jobs::{Job, Pool};
use std::time::Instant;
pub(crate) fn refresh(st: &mut ExperimentsState, doc: &CadDocument) -> Result<(), String> {
    if st.read.is_some() {
        return Err("experiments.read: previous read pending".into());
    }
    let s = stamp(doc, st)?;
    let client = crate::cad::lab::service(doc)?;
    st.read = Some(Read {
        stamp: s.clone(),
        kind: "list",
        restore: false,
        job: Job::spawn(
            Pool::Dedicated,
            s.generation,
            "experiment history catalogue",
            move |_| {
                let runs = client.experiments().map_err(|e| e.to_string())?;
                let candidates = client.candidates().map_err(|e| e.to_string())?;
                // Catalogue executable errors remain visible without hiding recorded runs.
                let catalogue = client.experiment_catalogue().map_err(|e| e.to_string());
                Ok(
                    json!({"runs":runs,"candidates":candidates,"catalogue":catalogue.as_ref().ok(),"catalogue_error":catalogue.err()}),
                )
            },
        ),
    });
    st.last_read = Some(Instant::now());
    Ok(())
}
pub(crate) fn read_selected(
    st: &mut ExperimentsState,
    doc: &CadDocument,
    restore: bool,
) -> Result<(), String> {
    if st.read.is_some() {
        return Err("experiments.read: previous read pending".into());
    }
    let id = st.selected.clone().ok_or("Select a captured run first")?;
    let s = stamp(doc, st)?;
    let client = crate::cad::lab::service(doc)?;
    st.read = Some(Read {
        stamp: s.clone(),
        kind: "selected",
        restore,
        job: Job::spawn(
            Pool::Dedicated,
            s.generation,
            "captured run inputs diagnostics",
            move |_| {
                Ok(
                    json!({"inputs":client.experiment_inputs(&id).map_err(|e|e.to_string())?,"diagnostics":client.experiment_diagnostics(&id).map_err(|e|e.to_string())?,"id":id}),
                )
            },
        ),
    });
    Ok(())
}
pub(crate) fn read_candidate(st: &mut ExperimentsState, doc: &CadDocument) -> Result<(), String> {
    if st.read.is_some() {
        return Err("experiments.read: previous read pending".into());
    }
    let id = st.candidate.clone().ok_or("Select a candidate first")?;
    let s = stamp(doc, st)?;
    let client = crate::cad::lab::service(doc)?;
    st.read = Some(Read {
        stamp: s.clone(),
        kind: "candidate",
        restore: false,
        job: Job::spawn(
            Pool::Dedicated,
            s.generation,
            "candidate change review",
            move |_| value(client.candidate(&id).map_err(|e| e.to_string())?),
        ),
    });
    Ok(())
}
pub(crate) fn compare(st: &mut ExperimentsState, doc: &CadDocument) -> Result<(), String> {
    if st.read.is_some() {
        return Err("experiments.read: previous read pending".into());
    }
    let baseline = st.baseline.clone().ok_or("Set a baseline first")?;
    let id = st.selected.clone().ok_or("Select a run first")?;
    let s = stamp(doc, st)?;
    let client = crate::cad::lab::service(doc)?;
    st.read = Some(Read {
        stamp: s.clone(),
        kind: "compare",
        restore: false,
        job: Job::spawn(
            Pool::Dedicated,
            s.generation,
            "captured baseline comparison",
            move |_| {
                client
                    .experiment_compare(&baseline, &id)
                    .map_err(|e| e.to_string())
            },
        ),
    });
    Ok(())
}
