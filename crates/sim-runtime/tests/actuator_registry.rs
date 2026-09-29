//! The accepted actuator registry is the single source of motor values:
//! every accepted family is re-derivable from its recorded evidence, and every
//! live consumer resolves to exactly the accepted families.
use sim_domain_robot::actuator_profile::Family;
use sim_runtime::{
    acquisition::{actuator_promotion::{Sources, derive_family}, characterization::Report},
    actuator_registry::{Registry, joint_limits},
    contact_exploration,
};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn registry() -> Registry {
    Registry::load(&root().join("examples/actuators/hx30hm/accepted/registry.json")).unwrap()
}

#[test]
fn accepted_families_match_their_hashes_and_cover_every_role() {
    let r = registry();
    for role in ["Foot servo output", "Hip servo output", "Worm servo output"] {
        assert!(r.file.roles.contains_key(role), "{role}");
    }
    // Measured families carry an envelope and cite the campaign.
    for name in ["hx30hm-knee-measured", "hx30hm-hip-measured"] {
        let f = &r.families[name];
        assert!(f.envelope.is_some(), "{name} envelope");
        assert!(f.evidence.contains_key("campaign"), "{name} evidence");
        assert!(matches!(f.motor["back_emf_constant"].provenance, sim_domain_robot::actuator_profile::Provenance::Derived));
    }
}

/// A measured family must be exactly what promotion derives from its cited
/// campaign report and base family: no hand edits survive this test.
#[test]
fn measured_families_are_reproducible_from_their_evidence() {
    let r = registry();
    let base = &r.families["hx30hm-provisional"];
    for (name, axis) in [("hx30hm-knee-measured", 1u8), ("hx30hm-hip-measured", 3)] {
        let accepted: &Family = &r.families[name];
        let ev = &accepted.evidence["campaign"];
        let report: Report = serde_json::from_slice(&std::fs::read(root().join(&ev.path)).expect("campaign report is retained")).unwrap();
        let code = &accepted.evidence["promotion"];
        let sources = Sources { report_path: ev.path.clone(), report_sha256: ev.sha256.clone(), scope: ev.scope.clone(), code_path: code.path.clone(), code_sha256: code.sha256.clone() };
        let (family, _) = derive_family(&report, axis, base, &accepted.description, &sources).unwrap();
        assert_eq!(family.content_hash(), accepted.content_hash(), "{name} differs from its derivation; re-promote and re-accept");
    }
}

/// The measured-actuator gait study derives its motor values from the
/// registry at load: families, screen and governor bounds.
#[test]
fn gait_recipe_derives_motor_values_from_the_registry() {
    let config: serde_json::Value = serde_json::from_slice(&std::fs::read(root().join("examples/full-robot/measured-actuator-integration/gait-search-measured-2026-09-23/comparison-config.json")).unwrap()).unwrap();
    let mut recipe: contact_exploration::Recipe = serde_json::from_value(config["recipe"].clone()).unwrap();
    let provenance = recipe.sync_actuators(&root()).unwrap();
    let r = registry();
    r.check(&recipe.experiment.scene.robot).unwrap();
    r.check(&recipe.planning_scene.robot).unwrap();
    let limits = joint_limits(&recipe.experiment.scene.robot).unwrap();
    for (c, screen) in recipe.compiler.robot.independent_coordinates.iter().zip(&recipe.maximum_reference_speed_rad_s) {
        let l = &limits[c.strip_prefix("joint.").unwrap()];
        assert!((screen - l.full_drive_speed_rad_s).abs() < 1e-12, "{c}");
        assert_eq!(recipe.compiler.robot.actuators[c]["no_load_speed"], l.no_load_speed_rad_s);
    }
    // Knee and hip are slower than the 5.51 rad/s catalogue value the earlier studies used.
    let knee = &limits["+X | Foot servo output"];
    assert!(knee.full_drive_speed_rad_s < 5.0 && knee.family == "hx30hm-knee-measured");
    let governor = recipe.template.space.parameters.iter().find(|p| p.name == "governor_speed_rad_s").unwrap();
    let slowest = limits.values().map(|l| l.full_drive_speed_rad_s).fold(f64::INFINITY, f64::min);
    assert!((governor.bounds[1] - 0.8 * slowest).abs() < 1e-12);
    assert!(provenance["registry"]["registry_blake3"].is_string());
    // Idempotent.
    let again = recipe.sync_actuators(&root()).unwrap();
    assert_eq!(again["maximum_reference_speed_rad_s"], provenance["maximum_reference_speed_rad_s"]);
}

/// A stale copy (hand-edited family) is detected.
#[test]
fn stale_family_copy_is_rejected() {
    let config: serde_json::Value = serde_json::from_slice(&std::fs::read(root().join("examples/full-robot/measured-actuator-integration/gait-search-measured-2026-09-23/comparison-config.json")).unwrap()).unwrap();
    let mut recipe: contact_exploration::Recipe = serde_json::from_value(config["recipe"].clone()).unwrap();
    recipe.sync_actuators(&root()).unwrap();
    let mut model = recipe.experiment.scene.robot.clone();
    let profiles = model.actuator_profiles.as_mut().unwrap();
    profiles.families.get_mut("hx30hm-knee-measured").unwrap().motor.get_mut("back_emf_constant").unwrap().value *= 1.01;
    assert!(registry().check(&model).is_err());
}
