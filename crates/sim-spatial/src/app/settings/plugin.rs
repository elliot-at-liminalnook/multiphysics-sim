use super::super::ViewerSet;
use super::*;
use crate::jobs::Pool;
use std::time::Duration;
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingsSet {
    Publish,
}
pub struct SettingsPlugin;
impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        crate::app::actions::register::<super::actions::SettingsAction>(app);
        app.add_systems(Update, super::actions::apply.in_set(ViewerSet::Actions));
        app.register_type::<PreferenceGroup>()
            .init_resource::<PreferenceGroup>()
            .init_resource::<SettingsOwner>()
            .configure_sets(Update, SettingsSet::Publish.in_set(ViewerSet::JobResults))
            .add_systems(Startup, start)
            .add_systems(Update, tick.in_set(SettingsSet::Publish));
        // Registration is an actual contract, not a marker derive. The seam
        // uses its group/source mapping and publishes only validated projections.
        let registry = app.world().resource::<AppTypeRegistry>().read();
        let registration = registry
            .get(std::any::TypeId::of::<PreferenceGroup>())
            .expect("registered preference group");
        assert!(registration.data::<ReflectSettingsGroup>().is_some());
    }
}
fn start(mut owner: ResMut<SettingsOwner>) {
    if cfg!(test) {
        owner.ready = true;
        owner.blocked = true;
        owner.diagnostic = Some(
            "Fixture startup: session-only, use explicit isolated paths for disk fixtures".into(),
        );
        return;
    }
    let paths = jobs::Paths::environment();
    owner.load = Some(jobs::load_job(paths.clone()));
    owner.paths = Some(paths);
}
fn tick(
    mut owner: ResMut<SettingsOwner>,
    mut group: ResMut<PreferenceGroup>,
    redraw: Option<MessageWriter<bevy::window::RequestRedraw>>,
) {
    let previous_status = owner.status();
    let previous_diagnostic = owner.diagnostic.clone();
    // Polling handles is not a semantic projection change.
    let state = owner.bypass_change_detection();
    tick_owner(state);
    if previous_status != owner.status() { owner.set_changed(); }
    if previous_diagnostic != owner.diagnostic
        && let Some(error) = &owner.diagnostic
    {
        bevy::log::warn!("viewer preferences: {error}; retry with settings_retry");
    }
    let next = PreferenceGroup {
        recents: serde_json::to_string(&owner.recents).unwrap_or_default(),
        hardware: serde_json::to_string(&owner.hardware).unwrap_or_default(),
        cad: serde_json::to_string(&owner.cad).unwrap_or_default(),
    };
    if group.recents != next.recents || group.hardware != next.hardware || group.cad != next.cad {
        *group = next;
    }
    if (owner.load.is_some() || owner.save.is_some() || owner.canonical.is_some()
        || owner.dirty() && !owner.blocked && owner.normalization_error.is_none())
        && let Some(mut redraw) = redraw
    { redraw.write(bevy::window::RequestRedraw); }
}
fn tick_owner(owner: &mut SettingsOwner) {
    let load = owner.load.as_ref().and_then(Job::poll);
    if let Some(answer) = load {
        land_load(owner, answer);
    }
    if owner.ready {
        if let Some(answer) = owner.canonical.as_ref().and_then(Job::poll) {
            land_canonical(owner, answer);
        }
        if owner.canonical.is_none() && owner.normalization_error.is_none() {
            if let Some((mode, document, now)) = owner.records.front().cloned() {
                owner.canonical = Some(Job::spawn(
                    Pool::Io,
                    owner.revision,
                    "recent path normalization",
                    move |_| Ok((mode, jobs::normalize(&document)?, now)),
                ));
            }
        }
    }
    if let Some(answer) = owner.save.as_ref().and_then(Job::poll) {
        owner.save = None;
        land_save(owner, answer);
    }
    if owner.ready
        && !owner.blocked
        && owner.dirty()
        && owner.save.is_none()
        && owner.canonical.is_none()
        && owner.records.is_empty()
        && Instant::now() >= owner.retry_at
    {
        match (owner.paths.clone(), owner.snapshot()) {
            (Some(paths), Ok(snapshot)) => {
                owner.snapshot_error = None;
                owner.save_revision = Some(owner.revision);
                owner.save = Some(jobs::save_job(
                    paths,
                    snapshot,
                    owner.revision,
                    owner.gate.clone(),
                ))
            }
            (_, Err(error)) => {
                owner.snapshot_error = Some(error.clone());
                owner.diagnostic = Some(error);
                owner.retry_at = Instant::now() + Duration::from_secs(5);
            }
            _ => {}
        }
    }
}

pub(super) fn land_canonical(owner: &mut SettingsOwner, answer: Result<(ViewerMode, Document, u64), String>) {
    owner.canonical = None;
    match answer {
        Ok((mode, document, now)) => {
            owner.records.pop_front();
            owner.recents.record(mode, &document, now);
            owner.revision += 1;
            owner.normalization_error = None;
            if owner.snapshot_error.is_none() && owner.publication_error.is_none() {
                owner.diagnostic = None;
            }
        }
        Err(e) => {
            // The accepted record remains required, including after failure.
            owner.normalization_error = Some(e.clone());
            owner.diagnostic = Some(e);
        }
    }
}

pub(super) fn land_load(owner: &mut SettingsOwner, answer: Result<jobs::Loaded, String>) {
    owner.load = None;
    owner.drain_epoch += 1;
    owner.ready = false;
    match answer {
        Ok(loaded) => {
            match owner.gate.try_lock() {
                Ok(mut gate) => gate.expected = loaded.previous,
                Err(e) => {
                    owner.blocked = true;
                    owner.diagnostic = Some(format!("Preference publication gate unavailable: {e}"));
                    return;
                }
            }
            owner.raw = loaded.raw;
            // Recents records are queued until the loaded base is ready;
            // unlike replacement groups they merge instead of suppressing it.
            owner.recents = loaded.recents;
            let mut hardware =
                serde_json::to_value(loaded.hardware).expect("validated loaded preferences");
            let current =
                serde_json::to_value(&owner.hardware).expect("validated current preferences");
            for path in &owner.hardware_claims {
                if let Some(value) = current.pointer(path) {
                    apply_claim(&mut hardware, path, value.clone());
                }
            }
            // Validation after merging is still mandatory; malformed merged
            // projections remain a named load failure, never a default success.
            let mut merged: crate::robot::hardware::settings::Settings =
                match serde_json::from_value(hardware) {
                    Ok(value) => value,
                    Err(e) => {
                        owner.blocked = true;
                        owner.diagnostic = Some(e.to_string());
                        return;
                    }
                };
            if let Err(e) = merged.validate() {
                owner.blocked = true;
                owner.diagnostic = Some(e);
                return;
            }
            owner.hardware = merged;
            let mut cad = loaded.cad;
            if owner.cad_claims[0] {
                cad.wall_threshold = owner.cad.wall_threshold;
            }
            if owner.cad_claims[1] {
                cad.fastener = owner.cad.fastener.clone();
            }
            if owner.cad_claims[2] {
                cad.clearance = owner.cad.clearance;
            }
            owner.cad = cad;
            owner.ready = true;
            owner.blocked = false;
            owner.diagnostic = None;
            if owner.paths.as_ref().is_some_and(|p| p.unified.is_none()) {
                owner.blocked = true;
                owner.diagnostic = Some(
                    "No viewer config directory: legacy hardware loaded, preferences session-only"
                        .into(),
                );
            }
            if loaded.migrated || owner.touched.iter().any(|v| *v) {
                owner.revision += 1;
            }
        }
        Err(error) => {
            owner.ready = false;
            owner.blocked = true;
            owner.diagnostic = Some(error);
        }
    }
}

pub(super) fn land_save(owner: &mut SettingsOwner, answer: Result<u64, String>) {
    let captured = owner.save_revision.take();
    // Visibility can advance before its job result lands. An older result must
    // not clear a newer durability failure or authorize the current drain.
    let visible_floor = owner.gate.try_lock().ok().map(|gate| gate.visible_revision);
    match answer {
        Ok(revision) if captured == Some(revision) && revision <= owner.revision
            && revision >= owner.saved_revision
            && visible_floor.is_some_and(|floor| revision >= floor) => {
            // A completion acknowledges only the immutable captured snapshot.
            owner.saved_revision = revision;
            owner.publication_error = None;
            if owner.normalization_error.is_none() && owner.snapshot_error.is_none() {
                owner.diagnostic = None;
            }
        }
        answer => {
            let error = match answer {
                Ok(revision) => format!("Preference publication acknowledgment mismatch: captured {captured:?}, returned {revision}"),
                Err(error) => error,
            };
            owner.publication_error = Some(error.clone());
            owner.diagnostic = Some(error);
            owner.retry_at = Instant::now() + Duration::from_secs(5);
        }
    }
}
