use sim_domain_robot::model::{BacklashProvenance, DriveBacklash, JointPhysics};
use sim_runtime::{embedded::{CaptureMode, Config, EmbeddedSession}, session::{Scene, Session}};

fn fixture() -> (Scene, Config) {
    (serde_json::from_str(include_str!("../../../examples/interactive/pendulum.scene.json")).unwrap(),
     serde_json::from_str(include_str!("../../../examples/interactive/pendulum.embedded.json")).unwrap())
}
fn explicit(width: Option<f64>) -> DriveBacklash {
    DriveBacklash { width_rad: width, provenance: if width.is_some() { BacklashProvenance::Estimated } else { BacklashProvenance::Unmeasured },
        reference: "explicit test fixture; not hardware calibration".into(), uncertainty_rad: None }
}

#[test]
fn v4_explicit_value_preserves_legacy_motion_and_ignores_bearing_angle_estimate() {
    let (scene, config) = fixture();
    let mut old = EmbeddedSession::new(scene.clone(), config.clone(), 0, CaptureMode::Full).unwrap();
    old.advance(config.steps).unwrap();
    let mut current = scene.clone(); current.robot.version = 4;
    let joint = &mut current.robot.joints[0];
    joint.physics.drive_backlash = Some(explicit(Some(joint.physics.backlash)));
    joint.physics.backlash = 1.2; // Deliberately large legacy bearing-angle estimate.
    let mut new = EmbeddedSession::new(current, config.clone(), 0, CaptureMode::Full).unwrap();
    new.advance(config.steps).unwrap();
    assert_eq!(old.report().unwrap()["frames"], new.report().unwrap()["frames"]);
    assert!(serde_json::to_value(&scene.robot.joints[0].physics).unwrap().get("drive_backlash").is_none());
}

#[test]
fn unknown_missing_and_invalid_drive_values_fail_both_runtime_hosts() {
    for value in [None, Some(explicit(None)), Some(explicit(Some(-0.01))),
        Some(DriveBacklash { reference: " ".into(), ..explicit(Some(0.0)) })] {
        let (mut scene, config) = fixture(); scene.robot.version = 4;
        scene.robot.joints[0].physics.drive_backlash = value;
        assert!(Session::new(scene.clone(), 0).is_err());
        assert!(EmbeddedSession::new(scene, config, 0, CaptureMode::Latest).is_err());
    }
}

#[test]
fn identified_drive_width_is_used_by_incremental_motor_bank() {
    let (mut scene, config) = fixture(); scene.robot.version=4;
    scene.robot.joints[0].physics.drive_backlash=Some(explicit(None));
    scene.robot.identification.insert(scene.robot.joints[0].name.clone(), sim_domain_robot::model::Identification {
        backlash: Some(0.007), source_log: "reversal-fixture.csv".into(), ..Default::default()
    });
    let expected=scene.robot.motors[0].gearbox.backlash_rad+0.007;
    let run=EmbeddedSession::new(scene,config,0,CaptureMode::Latest).unwrap();
    assert_eq!(run.diagnostic_metadata()["motor_components"][0]["parameters"]["backlash"],expected);
}

#[test]
fn provenance_and_uncertainty_are_validated_and_fits_preserve_bearing_estimate() {
    let mut p=JointPhysics { backlash:0.3, drive_backlash:Some(explicit(Some(0.01))), ..Default::default() };
    p.drive_backlash.as_mut().unwrap().uncertainty_rad=Some(f64::NAN);
    assert!(p.drive_backlash_rad(true).is_err());
    p.set_drive_backlash_rad(0.02,"identified reversal fixture".into());
    assert_eq!(p.backlash,0.3); assert_eq!(p.drive_backlash_rad(true).unwrap(),0.02);
    let d=p.drive_backlash.as_mut().unwrap(); d.provenance=BacklashProvenance::Unmeasured;
    assert!(p.drive_backlash_rad(true).is_err());
}
