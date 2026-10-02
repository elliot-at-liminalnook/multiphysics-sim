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
    let previous_diagnostic = owner.diagnostic.clone();
    let load = owner.load.as_ref().and_then(Job::poll);
    if let Some(answer) = load {
        land_load(&mut owner, answer);
    }
    if owner.ready {
        if let Some(answer) = owner.canonical.as_ref().and_then(Job::poll) {
            owner.canonical = None;
            owner.records.pop_front();
            match answer {
                Ok((mode, document, now)) => {
                    owner.recents.record(mode, &document, now);
                    owner.revision += 1;
                }
                Err(e) => owner.diagnostic = Some(e),
            }
        }
        if owner.canonical.is_none() {
            if let Some((mode, document, now)) = owner.records.front().cloned() {
                owner.canonical = Some(Job::spawn(
                    Pool::Io,
                    owner.revision,
                    "recent path normalization",
                    move |_| Ok((mode, super::super::recent::absolute(&document), now)),
                ));
            }
        }
    }
    if let Some(answer) = owner.save.as_ref().and_then(Job::poll) {
        owner.save = None;
        land_save(&mut owner, answer);
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
                owner.save = Some(jobs::save_job(
                    paths,
                    snapshot,
                    owner.revision,
                    owner.gate.clone(),
                ))
            }
            (_, Err(error)) => {
                owner.diagnostic = Some(error);
                owner.retry_at = Instant::now() + Duration::from_secs(5);
            }
            _ => {}
        }
    }
    if previous_diagnostic != owner.diagnostic
        && let Some(error) = &owner.diagnostic
    {
        bevy::log::warn!("viewer preferences: {error}; retry with settings_retry");
    }
    // This registered settings resource is the durable group's published,
    // validated representation. Consumers use the typed owner projections.
    let next = PreferenceGroup {
        recents: serde_json::to_string(&owner.recents).unwrap_or_default(),
        hardware: serde_json::to_string(&owner.hardware).unwrap_or_default(),
        cad: serde_json::to_string(&owner.cad).unwrap_or_default(),
    };
    if group.recents != next.recents || group.hardware != next.hardware || group.cad != next.cad {
        *group = next;
    }
    if (owner.load.is_some()
        || owner.save.is_some()
        || owner.canonical.is_some()
        || owner.dirty() && !owner.blocked)
        && let Some(mut redraw) = redraw
    {
        redraw.write(bevy::window::RequestRedraw);
    }
}

pub(super) fn land_load(owner: &mut SettingsOwner, answer: Result<jobs::Loaded, String>) {
    owner.load = None;
    match answer {
        Ok(loaded) => {
            if let Ok(mut gate) = owner.gate.lock() {
                gate.expected = loaded.previous;
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
    match answer {
        Ok(revision) => {
            owner.saved_revision = owner.saved_revision.max(revision);
            owner.diagnostic = None;
        }
        Err(error) => {
            owner.diagnostic = Some(error);
            owner.retry_at = Instant::now() + Duration::from_secs(5);
        }
    }
}
