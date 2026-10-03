//! Written lifecycle fixtures. Not executed in T45. All disk paths are isolated.
use super::*;
use serde_json::json;
fn paths(name: &str) -> jobs::Paths {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "sim-preference-fixture-{}-{nonce}-{name}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    jobs::Paths {
        unified: Some(root.join("viewer.json")),
        recent: Some(root.join("recent.json")),
        hardware: root.join("hardware.json"),
    }
}
fn write(path: &std::path::Path, v: Value) {
    std::fs::write(path, serde_json::to_vec(&v).unwrap()).unwrap();
}
#[test]
fn migration_preserves_originals_unknowns_and_is_idempotent() {
    let p = paths("migration");
    let recent = json!({"version":1,"modes":{"cad":[{"document":{"url":"http://reference"},"opened":3,"note":"keep"},{"document":{"future":"opaque"},"opened":4}]},"future":true});
    let hardware = json!({"version":1,"mirror":{"bindings":{"4":{"joint":"Foot servo output","future":{"a":1}}}},"unknown":{"nested":[1,2]}});
    write(p.recent.as_ref().unwrap(), recent.clone());
    write(&p.hardware, hardware.clone());
    let loaded = jobs::load(&p).unwrap();
    assert!(loaded.migrated);
    let next = jobs::snapshot(&loaded.raw, &loaded.recents, &loaded.hardware, &loaded.cad).unwrap();
    publication::publish_snapshot(p.unified.as_ref().unwrap(), &next, 1).unwrap();
    let again = jobs::load(&p).unwrap();
    assert!(!again.migrated);
    assert_eq!(again.hardware, loaded.hardware);
    assert_eq!(
        next["preferences"]["hardware"]["mirror"]["bindings"]["4"]["future"],
        json!({"a":1})
    );
    assert_eq!(
        next["preferences"]["recents"]["modes"]["cad"][1]["document"],
        json!({"future":"opaque"})
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(&p.hardware).unwrap()).unwrap(),
        hardware
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(p.recent.unwrap()).unwrap()).unwrap(),
        recent
    );
}
#[test]
fn unified_precedence_and_override_identity_refusal() {
    let mut p = paths("precedence");
    let loaded = jobs::load(&p).unwrap();
    let mut cad = loaded.cad;
    cad.clearance = 0.7;
    let next = jobs::snapshot(&loaded.raw, &loaded.recents, &loaded.hardware, &cad).unwrap();
    publication::publish_snapshot(p.unified.as_ref().unwrap(), &next, 1).unwrap();
    write(&p.hardware, json!({"version":1,"mirror":{"leg":"-Y"}}));
    assert_eq!(jobs::load(&p).unwrap().cad.clearance, 0.7);
    assert_eq!(jobs::load(&p).unwrap().hardware.mirror.leg, "+X");
    p.hardware = p.hardware.with_file_name("other.json");
    assert!(jobs::load(&p).is_err());
}
#[test]
fn future_corrupt_and_unreadable_inputs_fail_closed() {
    let p = paths("future");
    write(&p.hardware, json!({"version":99,"opaque":1}));
    assert!(jobs::load(&p).is_err());
    std::fs::write(&p.hardware, b"{broken").unwrap();
    assert!(jobs::load(&p).is_err());
    let mut q = paths("unreadable");
    q.hardware = q.hardware.parent().unwrap().to_path_buf();
    assert!(jobs::load(&q).is_err());
    write(p.unified.as_ref().unwrap(), json!({"schema":99}));
    assert!(jobs::load(&p).is_err());
}
#[test]
fn session_only_still_reads_exact_hardware_path() {
    let mut p = paths("no-config");
    p.unified = None;
    p.recent = None;
    write(&p.hardware, json!({"mirror":{"leg":"-X"}}));
    assert_eq!(jobs::load(&p).unwrap().hardware.mirror.leg, "-X");
}
#[test]
fn malformed_cad_never_publishes() {
    for bad in [
        json!({"wall_threshold":0}),
        json!({"clearance":6}),
        json!({"fastener":{"size":"M9"}}),
        json!({"fastener":{"depth":-1}}),
    ] {
        let defaults: CadDefaults = serde_json::from_value(bad).unwrap();
        assert!(defaults.validate().is_err());
    }
    let mut owner = SettingsOwner::default();
    let mut bad = owner.cad.clone();
    bad.clearance = f64::NAN;
    assert!(owner.set_cad(bad).is_err());
    assert_eq!(owner.revision, 0);
}
#[test]
fn late_load_and_failed_load_do_not_overwrite_intervening_choices() {
    let p = paths("late");
    let loaded = jobs::load(&p).unwrap();
    let mut owner = SettingsOwner::default();
    let mut cad = owner.cad.clone();
    cad.clearance = 0.9;
    owner.set_cad(cad).unwrap();
    owner.record(ViewerMode::Cad, Document::Url("http://new".into()));
    plugin::land_load(&mut owner, Err("read denied".into()));
    assert!(!owner.ready);
    assert!(owner.blocked);
    assert_eq!(owner.records.len(), 1);
    plugin::land_load(&mut owner, Ok(loaded));
    assert!(owner.ready);
    assert!(!owner.blocked);
    assert_eq!(owner.cad.clearance, 0.9);
    assert_eq!(owner.records.len(), 1);
}
#[test]
fn publication_error_retains_dirty_revision_and_retry_requires_no_edit() {
    let mut owner = SettingsOwner::default();
    owner.ready = true;
    owner.revision = 3;
    plugin::land_save(&mut owner, Err("rename failed".into()));
    owner.retry_at = Instant::now() + std::time::Duration::from_secs(100);
    owner.retry();
    assert!(owner.dirty());
    assert_eq!(owner.saved_revision, 0);
    assert!(owner.retry_at <= Instant::now());
    let p = paths("publication");
    let dir = p.unified.as_ref().unwrap();
    std::fs::create_dir(dir).unwrap();
    assert!(publication::publish_snapshot(dir, &json!({}), 3).is_err());
}
#[test]
fn newer_snapshot_suppresses_older_publication() {
    let p = paths("ordering");
    let gate = Mutex::new(jobs::Publication::default());
    jobs::publish_ordered(&p, &json!({"new":true}), 9, &gate).unwrap();
    assert!(jobs::publish_ordered(&p, &json!({"old":true}), 3, &gate).is_err());
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(p.unified.unwrap()).unwrap()).unwrap(),
        json!({"new":true})
    );
    let mut owner = SettingsOwner::default();
    owner.revision = 10;
    owner.saved_revision = 9;
    assert!(owner.dirty());
}
#[test]
fn unknown_nested_document_metadata_and_removed_bindings_are_retained() {
    let raw = json!({"schema":1,"preferences":{"recents":{"version":1,"modes":{"cad":[{"document":{"url":"x","vendor":1},"opened":1,"note":2}]}},"hardware":{"sync":{"bindings":[{"coordinate":"a","vendor":7},{"coordinate":"b","vendor":8}]}},"cad":{}}});
    let mut r = Recents::default();
    r.record(ViewerMode::Cad, &Document::Url("x".into()), 8);
    let out = jobs::snapshot(&raw, &r, &Settings::default(), &CadDefaults::default()).unwrap();
    assert_eq!(out["preferences"]["recents"]["modes"]["cad"][0]["note"], 2);
    assert_eq!(out["preserved_source"], raw);
    assert_eq!(
        out["preferences"]["hardware"]["sync"]["bindings"],
        json!([])
    );
    assert_eq!(out["retained_rows"]["entries"].as_array().unwrap().len(), 2);
}
#[test]
fn shutdown_does_not_claim_durability_or_persist_active_state() {
    let owner = SettingsOwner::default();
    let status = owner.status();
    assert!(!status["ready"].as_bool().unwrap());
    let serialized = serde_json::to_value(&owner.hardware).unwrap();
    for excluded in [
        "jog",
        "drive",
        "confirmed",
        "travel_windows",
        "measurements",
        "controller",
        "run",
    ] {
        assert!(serialized.get(excluded).is_none());
    }
    // Default owner has no paths: dropping it starts no disk job.
    drop(owner);
}

#[test]
fn delayed_save_completion_cannot_acknowledge_newer_dirty_choices() {
    let mut owner = SettingsOwner::default();
    owner.revision = 8;
    owner.save_revision = Some(5);
    plugin::land_save(&mut owner, Ok(5));
    assert!(owner.dirty());
    assert_eq!(owner.saved_revision, 5);
    plugin::land_save(&mut owner, Err("directory sync failed".into()));
    assert!(owner.dirty());
    assert!(owner.diagnostic.is_some());
    owner.save_revision = Some(8);
    plugin::land_save(&mut owner, Ok(8));
    assert!(!owner.dirty());
    assert!(owner.diagnostic.is_none());
}
#[test]
fn reflected_group_registration_exposes_actual_source_contract() {
    let mut app = App::new();
    app.register_type::<PreferenceGroup>();
    let types = app.world().resource::<AppTypeRegistry>().read();
    assert!(
        types
            .get(std::any::TypeId::of::<PreferenceGroup>())
            .unwrap()
            .data::<ReflectSettingsGroup>()
            .is_some()
    );
    assert_eq!(
        PreferenceGroup::settings_source(),
        Some("viewer-preferences")
    );
    assert_eq!(PreferenceGroup::settings_group_name(), "preferences");
}

#[test]
fn external_future_or_corrupt_publication_is_never_replaced() {
    let p = paths("external");
    let gate = Mutex::new(jobs::Publication::default());
    write(p.unified.as_ref().unwrap(), json!({"schema":99}));
    assert!(jobs::publish_ordered(&p, &json!({"schema":1}), 1, &gate).is_err());
    std::fs::write(p.unified.as_ref().unwrap(), b"corrupt").unwrap();
    assert!(jobs::publish_ordered(&p, &json!({"schema":1}), 1, &gate).is_err());
    assert_eq!(std::fs::read(p.unified.unwrap()).unwrap(), b"corrupt");
}

#[test]
fn shutdown_snapshot_includes_pending_recents_and_unrelated_dirty_defaults() {
    let p = paths("shutdown");
    let loaded = jobs::load(&p).unwrap();
    let mut cad = loaded.cad;
    cad.clearance = 0.8;
    let queue = VecDeque::from([(ViewerMode::Cad, Document::Url("http://pending".into()), 12)]);
    let snapshot =
        jobs::shutdown_snapshot(&loaded.raw, loaded.recents, &loaded.hardware, &cad, queue)
            .unwrap();
    assert_eq!(snapshot["preferences"]["cad"]["clearance"], 0.8);
    assert_eq!(
        snapshot["preferences"]["recents"]["modes"]["cad"][0]["document"]["url"],
        "http://pending"
    );
}
#[test]
fn accepted_recent_intent_is_dirty_before_canonicalization() {
    let mut owner = SettingsOwner::default();
    owner.record(ViewerMode::Cad, Document::Url("http://new".into()));
    assert!(owner.dirty());
    assert_eq!(owner.status()["pending_records"], 1);
}

#[test]
fn pure_path_overrides_preserve_distinct_legacy_semantics() {
    use std::path::PathBuf;
    let config = super::super::recent::config_dir_from(
        |key| match key {
            "SIM_SPATIAL_CONFIG_DIR" => Some(PathBuf::from("explicit")),
            "XDG_CONFIG_HOME" => Some(PathBuf::from("xdg")),
            "HOME" => Some(PathBuf::from("home")),
            _ => None,
        },
        true,
    );
    assert_eq!(config, Some(PathBuf::from("explicit")));
    assert_eq!(
        super::super::recent::config_dir_from(
            |key| (key == "XDG_CONFIG_HOME").then(|| PathBuf::from("xdg")),
            true
        ),
        Some(PathBuf::from("xdg/sim-spatial"))
    );
    assert_eq!(
        super::super::recent::config_dir_from(
            |key| (key == "HOME").then(|| PathBuf::from("home")),
            true
        ),
        Some(PathBuf::from(
            "home/Library/Application Support/sim-spatial"
        ))
    );
    assert!(super::super::recent::config_dir_from(|_| None, true).is_none());
    assert_eq!(
        crate::robot::hardware::settings::path_from(Some(PathBuf::from("override.json")), None),
        PathBuf::from("override.json")
    );
    assert_eq!(
        crate::robot::hardware::settings::path_from(None, None),
        PathBuf::from(".config/sim-spatial/hardware-preferences.json")
    );
}

#[test]
fn late_publication_merges_untouched_fields_and_equal_value_claims() {
    let p = paths("field-merge");
    write(
        &p.hardware,
        json!({"calibration":{"hold_others":true},"mirror":{"leg":"-Y"}}),
    );
    let mut loaded = jobs::load(&p).unwrap();
    loaded.cad.wall_threshold = Some(3.0);
    loaded.cad.clearance = 0.8;
    let mut owner = SettingsOwner::default();
    let mut cad = owner.cad.clone();
    cad.clearance = 0.4;
    owner.set_cad(cad).unwrap();
    owner.claim_hardware("/mirror/leg".into());
    plugin::land_load(&mut owner, Ok(loaded));
    assert_eq!(owner.cad.clearance, 0.4);
    assert_eq!(owner.cad.wall_threshold, Some(3.0));
    assert_eq!(owner.hardware.mirror.leg, "+X");
    assert_eq!(owner.hardware.calibration.hold_others, Some(true));
}

#[test]
fn early_visibility_choice_does_not_claim_materialized_binding_defaults() {
    let p = paths("materialized");
    write(
        &p.hardware,
        json!({"mirror":{"bindings":{"1":{"joint":"Foot servo output","polarity":-1,"align":"mid"}}}}),
    );
    let loaded = jobs::load(&p).unwrap();
    let mut owner = SettingsOwner::default();
    let mut materialized = owner.hardware.clone();
    materialized.mirror.enabled = false;
    materialized.mirror.bindings.insert(
        1,
        crate::robot::hardware::settings::MirrorBinding {
            joint: "Hip servo output".into(),
            polarity: 1,
            align: crate::robot::hardware::actions::Align::Home,
        },
    );
    owner.set_hardware_claimed(materialized, vec!["/mirror/enabled".into()]);
    plugin::land_load(&mut owner, Ok(loaded));
    assert!(!owner.hardware.mirror.enabled);
    assert_eq!(
        owner.hardware.mirror.bindings[&1].joint,
        "Foot servo output"
    );
    assert_eq!(owner.hardware.mirror.bindings[&1].polarity, -1);
}

#[test]
fn removed_rows_stay_inactive_and_later_metadata_survives_existing_archive() {
    let p = paths("removed-rows");
    write(
        &p.hardware,
        json!({"mirror":{"bindings":{"4":{"vendor":{"first":1}},"5":{"vendor":{"retained":2}}}},"sync":{"bindings":[{"coordinate":"a","vendor":7},{"coordinate":"b","vendor":8}]}}),
    );
    let loaded = jobs::load(&p).unwrap();
    let mut current = loaded.hardware.clone();
    current.mirror.bindings.remove(&4);
    current.sync.bindings.reverse();
    let first = jobs::snapshot(&loaded.raw, &loaded.recents, &current, &loaded.cad).unwrap();
    assert!(
        first["preferences"]["hardware"]["mirror"]["bindings"]
            .get("4")
            .is_none()
    );
    assert_eq!(
        first["preferences"]["hardware"]["mirror"]["bindings"]["5"]["vendor"]["retained"],
        2
    );
    assert_eq!(
        first["preferences"]["hardware"]["sync"]["bindings"][0]["vendor"],
        8
    );
    publication::publish_snapshot(p.unified.as_ref().unwrap(), &first, 1).unwrap();
    assert!(
        !jobs::load(&p)
            .unwrap()
            .hardware
            .mirror
            .bindings
            .contains_key(&4)
    );
    let original = first["preserved_source"].clone();
    // A later startup sees new vendor metadata absent from immutable migration evidence.
    let mut later = first;
    later["preferences"]["hardware"]["mirror"]["bindings"]["5"]["vendor"]["later"] = json!(3);
    later["preferences"]["hardware"]["sync"]["bindings"][0]["later"] = json!(9);
    write(p.unified.as_ref().unwrap(), later);
    let loaded = jobs::load(&p).unwrap();
    let mut current = loaded.hardware.clone();
    current.mirror.bindings.clear();
    current.sync.bindings.clear();
    let out = jobs::snapshot(&loaded.raw, &loaded.recents, &current, &loaded.cad).unwrap();
    assert_eq!(out["preserved_source"], original);
    assert_eq!(
        out["preferences"]["hardware"]["mirror"]["bindings"],
        json!({})
    );
    assert_eq!(
        out["preferences"]["hardware"]["sync"]["bindings"],
        json!([])
    );
    let entries = out["retained_rows"]["entries"].as_array().unwrap();
    assert!(entries.iter().any(|e| e["collection"] == "mirror"
        && e["identity"] == "5"
        && e["value"]["vendor"]["later"] == 3));
    assert!(entries.iter().any(|e| e["collection"] == "sync"
        && e["identity"] == "coordinate:b"
        && e["value"]["later"] == 9));
    publication::publish_snapshot(p.unified.as_ref().unwrap(), &out, 2).unwrap();
    let loaded = jobs::load(&p).unwrap();
    assert!(loaded.hardware.mirror.bindings.is_empty());
    assert!(loaded.hardware.sync.bindings.is_empty());
    assert_eq!(
        jobs::snapshot(&loaded.raw, &loaded.recents, &loaded.hardware, &loaded.cad).unwrap(),
        out
    );
}

#[test]
fn malformed_future_or_overfull_removal_archive_blocks_loading_and_snapshot() {
    let p = paths("archive-refusal");
    let loaded = jobs::load(&p).unwrap();
    let base = jobs::snapshot(&loaded.raw, &loaded.recents, &loaded.hardware, &loaded.cad).unwrap();
    let row = json!({"collection":"mirror","identity":"4","value":{}});
    for archive in [
        json!({"version":2,"entries":[]}),
        json!({"version":1,"entries":[{"collection":"mirror","identity":4,"value":{}}]}),
        json!({"version":1,"entries":vec![row;257]}),
        json!({"version":1,"entries":[{"collection":"sync","identity":"coordinate:a","value":{"vendor":"x".repeat(1024*1024)}}]}),
    ] {
        let mut raw = base.clone();
        raw["retained_rows"] = archive;
        write(p.unified.as_ref().unwrap(), raw.clone());
        assert!(jobs::load(&p).err().unwrap().contains("retained_rows"));
        assert!(jobs::snapshot(&raw, &loaded.recents, &loaded.hardware, &loaded.cad).is_err());
        assert_eq!(
            serde_json::from_slice::<Value>(&std::fs::read(p.unified.as_ref().unwrap()).unwrap())
                .unwrap(),
            raw
        );
    }
}

#[test]
fn removing_another_row_at_archive_capacity_refuses_instead_of_evicting_evidence() {
    let p = paths("archive-growth");
    write(
        &p.hardware,
        json!({"mirror":{"bindings":{"4":{"vendor":"new removal"}}}}),
    );
    let loaded = jobs::load(&p).unwrap();
    let mut raw = loaded.raw.clone();
    raw["retained_rows"] = json!({"version":1,"entries":(0..256).map(|i| json!({"collection":"mirror","identity":i.to_string(),"value":{"vendor":i}})).collect::<Vec<_>>()});
    let before = raw.clone();
    let mut owner = SettingsOwner::default();
    owner.revision = 2;
    owner.saved_revision = 1;
    assert!(
        jobs::snapshot(&raw, &loaded.recents, &Settings::default(), &loaded.cad)
            .err()
            .unwrap()
            .contains("capacity exceeded")
    );
    assert_eq!(raw, before);
    assert!(owner.dirty());
}

#[test]
fn duplicate_coordinates_consume_rows_once_and_archive_removed_later_metadata() {
    let p = paths("duplicate-sync-rows");
    write(
        &p.hardware,
        json!({"sync":{"bindings":[{"coordinate":"a","motor_id":4,"vendor":"first"},{"coordinate":"a","motor_id":5,"vendor":"second"}]}}),
    );
    let loaded = jobs::load(&p).unwrap();
    let first =
        jobs::snapshot(&loaded.raw, &loaded.recents, &loaded.hardware, &loaded.cad).unwrap();
    assert_eq!(
        first["preferences"]["hardware"]["sync"]["bindings"][0]["vendor"],
        "first"
    );
    assert_eq!(
        first["preferences"]["hardware"]["sync"]["bindings"][1]["vendor"],
        "second"
    );
    let original = first["preserved_source"].clone();
    let mut later = first;
    later["preferences"]["hardware"]["sync"]["bindings"][1]["new_vendor"] = json!(42);
    write(p.unified.as_ref().unwrap(), later);
    let loaded = jobs::load(&p).unwrap();
    let mut current = loaded.hardware.clone();
    current.sync.bindings.pop();
    let out = jobs::snapshot(&loaded.raw, &loaded.recents, &current, &loaded.cad).unwrap();
    assert_eq!(out["preserved_source"], original);
    let rows = out["preferences"]["hardware"]["sync"]["bindings"]
        .as_array()
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["vendor"], "first");
    assert!(rows[0].get("new_vendor").is_none());
    assert!(
        out["retained_rows"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["identity"] == "coordinate:a"
                && e["value"]["vendor"] == "second"
                && e["value"]["new_vendor"] == 42)
    );
    publication::publish_snapshot(p.unified.as_ref().unwrap(), &out, 2).unwrap();
    let loaded = jobs::load(&p).unwrap();
    assert_eq!(loaded.hardware.sync.bindings.len(), 1);
    assert_eq!(
        jobs::snapshot(&loaded.raw, &loaded.recents, &loaded.hardware, &loaded.cad).unwrap(),
        out
    );
}

/// Drive bindings (`robot::drive_input`): absent means the defaults; set
/// bindings round-trip whole; a reset removes the group again.
#[test]
fn drive_bindings_round_trip_and_reset_removes_the_group() {
    use crate::robot::drive_input::BindingsFile;
    let p = paths("drive-bindings");
    let loaded = jobs::load(&p).unwrap();
    assert_eq!(loaded.drive_bindings, None);
    let mut b = BindingsFile::default();
    b.gamepad.deadzone = 0.2;
    let base = jobs::snapshot(&loaded.raw, &loaded.recents, &loaded.hardware, &loaded.cad).unwrap();
    let next = jobs::with_drive_bindings(base, Some(&b)).unwrap();
    publication::publish_snapshot(p.unified.as_ref().unwrap(), &next, 1).unwrap();
    let again = jobs::load(&p).unwrap();
    assert_eq!(again.drive_bindings, Some(b));
    let base = jobs::snapshot(&again.raw, &again.recents, &again.hardware, &again.cad).unwrap();
    assert!(base["preferences"].get("drive_bindings").is_some(), "an unchanged snapshot keeps the stored group");
    let reset = jobs::with_drive_bindings(base, None).unwrap();
    assert!(reset["preferences"].get("drive_bindings").is_none());
}

/// A stored binding that fails validation protects the whole file, naming the field.
#[test]
fn malformed_drive_bindings_block_loading_naming_the_field() {
    use crate::robot::drive_input::BindingsFile;
    let p = paths("drive-bindings-bad");
    let loaded = jobs::load(&p).unwrap();
    let mut next = jobs::snapshot(&loaded.raw, &loaded.recents, &loaded.hardware, &loaded.cad).unwrap();
    let mut bad = serde_json::to_value(BindingsFile::default()).unwrap();
    bad["keyboard"]["axes"][1]["key"] = json!("KeyNope");
    next["preferences"]["drive_bindings"] = bad;
    publication::publish_snapshot(p.unified.as_ref().unwrap(), &next, 1).unwrap();
    let e = jobs::load(&p).err().unwrap();
    assert!(e.contains("drive_bindings.keyboard.axes[1].key") && e.contains("saves blocked"), "{e}");
}

/// Bindings set before the load lands own their group against it, and the
/// owner is dirty so they are written.
#[test]
fn drive_bindings_set_before_the_load_survive_it() {
    use crate::robot::drive_input::BindingsFile;
    let p = paths("drive-bindings-claim");
    let mut owner = SettingsOwner::default();
    let mut b = BindingsFile::default();
    b.gamepad.deadzone = 0.3;
    owner.set_drive_bindings(Some(b.clone())).unwrap();
    plugin::land_load(&mut owner, Ok(jobs::load(&p).unwrap()));
    assert_eq!(owner.drive_bindings, Some(b));
    assert!(owner.dirty());
    // A refused value changes nothing.
    let mut bad = BindingsFile::default();
    bad.gamepad.deadzone = 1.0;
    assert!(owner.set_drive_bindings(Some(bad)).unwrap_err().contains("drive_bindings.gamepad.deadzone"));
    assert_eq!(owner.drive_bindings.as_ref().map(|b| b.gamepad.deadzone), Some(0.3));
}
