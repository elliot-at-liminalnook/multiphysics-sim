use serde_json::json;
use sim_runtime::{
    embedded::{CaptureMode, Config, EmbeddedSession},
    session::Scene,
};

fn fixture() -> (Scene, Config, f64) {
    let mut scene: Scene = serde_json::from_str(include_str!(
        "../../../examples/interactive/pendulum.scene.json"
    ))
    .unwrap();
    // Float the whole fixture. A vertical force passes through both COMs,
    // producing pure translation; the unused motor keeps the session seam.
    for l in &mut scene.robot.links {
        l.ground = false;
        l.com[0] = 0.0;
        l.com[1] = 0.0;
        for i in 0..3 {
            for j in 0..3 {
                if i != j {
                    l.inertia[i][j] = 0.0;
                }
            }
        }
    }
    scene.robot.gravity = [0.0; 3];
    scene.robot.sensors.clear();
    scene.robot.cables.clear();
    scene.robot.battery = None;
    scene.controller = None;
    scene.options.contact = false;
    scene.options.flex = false;
    let model = serde_json::to_value(&scene.robot).unwrap();
    let mass = model["links"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["mass"].as_f64().unwrap())
        .sum();
    let config: Config=serde_json::from_value(json!({
        "step_s":0.01,"steps":10,"report_every":1,"applied_generalized_loads":[0,0,0,0,0,0,0],
        "world_loads":{"version":1,"base_link":"ground","provenance":"analytic impulse fixture",
            "maximum_force_n":0.02,"maximum_moment_nm":0.000001,
            "pulses":[{"name":"push","start_s":0.02,"duration_s":0.04,"force_world_n":[0,0,0.02],"moment_world_nm":[0,0,0]}]}
    })).unwrap();
    (scene, config, mass)
}

#[test]
fn physical_impulse_matches_momentum_and_replays_across_host_chunks() {
    let (scene, config, mass) = fixture();
    for implicit in [false, true] {
        let mut c = config.clone();
        if implicit {
            c.implicit = Some(Default::default());
        }
        let source = serde_json::to_value(&scene.robot).unwrap();
        let mut session = EmbeddedSession::new(scene.clone(), c, 9, CaptureMode::Full).unwrap();
        let initial_frame = session.frame().unwrap();
        let initial = initial_frame["poses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "ground")
            .unwrap()["position_m"][2]
            .as_f64()
            .unwrap();
        session.advance(10).unwrap();
        let frame = session.frame().unwrap();
        let pose = frame["poses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "ground")
            .unwrap();
        assert!(
            (mass * pose["velocity_m_s"][2].as_f64().unwrap() - 0.02 * 0.04).abs() < 1e-10,
            "implicit={implicit} mass={mass} pose={pose}"
        );
        if !implicit {
            let expected = initial + 0.02 / mass * (0.5 * 0.04 * 0.04 + 0.04 * 0.04);
            assert!((pose["position_m"][2].as_f64().unwrap() - expected).abs() < 1e-10);
        }
        assert_eq!(
            frame["environment_load"]["force_world_n"],
            json!([0.0, 0.0, 0.0])
        );
        let record = session.recording();
        assert_eq!(serde_json::to_value(&record.scene.robot).unwrap(), source);
        let (mut replay, n) = EmbeddedSession::prepare_replay(record, CaptureMode::Latest).unwrap();
        replay.advance(3).unwrap();
        replay.advance(n - 3).unwrap();
        assert_eq!(frame, replay.frame().unwrap());
    }
}
