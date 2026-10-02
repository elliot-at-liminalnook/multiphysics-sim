//! Consumer ordering and expected-state authority; filesystem stages are shared.
use super::{jobs::{Paths, SCHEMA}, *};
use sim_runtime::publication::{self, Hooks, NoHooks, Outcome, Policy};
#[cfg(test)]
use std::path::Path;

#[derive(Default)]
pub(super) struct Publication {
    /// Highest confirmed revision (not merely destination-visible).
    pub revision: u64,
    pub expected: Option<Value>,
    /// A visible unconfirmed replacement also excludes older writers.
    pub visible_revision: u64,
    pending_confirmation: bool,
}
pub(super) fn publish_ordered(
    paths: &Paths, snapshot: &Value, revision: u64, gate: &Mutex<Publication>,
) -> Result<u64, String> {
    publish_ordered_with(paths, snapshot, revision, gate, &NoHooks)
}
pub(super) fn publish_ordered_with(
    paths: &Paths, snapshot: &Value, revision: u64, gate: &Mutex<Publication>, hooks: &dyn Hooks,
) -> Result<u64, String> {
    let mut state = gate.lock().map_err(|_| "preference publication gate poisoned")?;
    if state.visible_revision.max(state.revision) > revision {
        return Err(format!("Preference publication superseded: requested {revision}, visible {}", state.visible_revision));
    }
    let path = paths.unified.as_ref().ok_or("No viewer config directory; preferences session-only")?;
    let current = super::jobs::read(path)?;
    if let Some(current) = &current { super::jobs::version(current, "schema", SCHEMA)?; }
    if current != state.expected {
        return Err(format!("{}: changed since load/publication; preserving file, restart after reviewing it", path.display()));
    }
    if revision == state.visible_revision && state.visible_revision != 0 && state.expected.as_ref() != Some(snapshot) {
        return Err("Preference revision reused for a different snapshot; preserving visible file".into());
    }
    // The read/compare is a consumer check, not cross-process compare-and-swap.
    // An external writer can still race between this comparison and publication.
    let outcome = if state.pending_confirmation && state.expected.as_ref() == Some(snapshot) {
        publication::confirm_existing_with(path, hooks)
    } else {
        let bytes = serde_json::to_vec_pretty(snapshot).map_err(|e| e.to_string())?;
        publication::publish_with(path, &bytes, Policy::Replace, hooks)
    };
    if !matches!(&outcome, Outcome::Unpublished(_)) {
        state.expected = Some(snapshot.clone());
        state.visible_revision = revision;
        state.pending_confirmation = true;
    }
    outcome.into_result()?;
    state.revision = revision;
    state.pending_confirmation = false;
    Ok(revision)
}
/// Isolated fixture setup uses the production filesystem implementation.
#[cfg(test)]
pub(super) fn publish_snapshot(path: &Path, snapshot: &Value, _revision: u64) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(snapshot).map_err(|e| e.to_string())?;
    publication::publish(path, &bytes, Policy::Replace).into_result()
}
