use nalgebra::{Matrix3, UnitQuaternion, Vector3};
use sim_runtime::{
    embedded::{CaptureMode, Config, EmbeddedSession},
    session::Scene,
};

#[test]
fn explicit_initial_rotation_preserves_mechanism_and_replays() {
    let mut scene: Scene = serde_json::from_str(include_str!(
        "../../../examples/interactive/pendulum.scene.json"
    ))
    .unwrap();
    let mut config: Config = serde_json::from_str(include_str!(
        "../../../examples/interactive/pendulum.embedded.json"
    ))
    .unwrap();
    config.initial_base_rotation_vector_rad = Some([0.2, -0.3, 0.1]);
    assert!(
        EmbeddedSession::new(scene.clone(), config.clone(), 0, CaptureMode::Latest)
            .err()
            .unwrap()
            .contains("initial rotation requires one floating base")
    );
    for link in &mut scene.robot.links {
        link.ground = false;
    }
    config.motors = None;
    config.audit_contact_steps = false;
    config.applied_generalized_loads = vec![0.0; 7];
    config.initial_coordinates = Some(vec![0.25]);
    let mut original = config.clone();
    original.initial_base_rotation_vector_rad = None;
    let a = EmbeddedSession::new(scene.clone(), original, 0, CaptureMode::Latest).unwrap();
    config.initial_base_translation_m = Some([0.1, 0.2, 0.3]);
    let b = EmbeddedSession::new(scene.clone(), config.clone(), 0, CaptureMode::Latest).unwrap();
    let af = a.frame().unwrap();
    let bf = b.frame().unwrap();
    let vector = |v: &serde_json::Value| {
        Vector3::new(
            v[0].as_f64().unwrap(),
            v[1].as_f64().unwrap(),
            v[2].as_f64().unwrap(),
        )
    };
    let matrix = |v: &serde_json::Value| Matrix3::from_fn(|i, j| v[i][j].as_f64().unwrap());
    let origin = vector(&af["poses"][0]["position_m"]);
    let rotation =
        UnitQuaternion::from_scaled_axis(Vector3::new(0.2, -0.3, 0.1)).to_rotation_matrix();
    for (p, q) in af["poses"]
        .as_array()
        .unwrap()
        .iter()
        .zip(bf["poses"].as_array().unwrap())
    {
        let expected =
            origin + Vector3::new(0.1, 0.2, 0.3) + rotation * (vector(&p["position_m"]) - origin);
        assert!((vector(&q["position_m"]) - expected).norm() < 1e-11);
        assert!(
            (matrix(&q["rotation"]) - rotation.matrix() * matrix(&p["rotation"])).norm() < 1e-11
        );
    }
    assert_eq!(af["joint_positions"], bf["joint_positions"]);
    let (replay, steps) =
        EmbeddedSession::prepare_replay(b.recording(), CaptureMode::Latest).unwrap();
    assert_eq!(steps, 0);
    assert_eq!(replay.frame().unwrap(), bf);
    config.initial_base_rotation_vector_rad = Some([f64::NAN, 0.0, 0.0]);
    assert!(EmbeddedSession::new(scene, config, 0, CaptureMode::Latest).is_err());
}
