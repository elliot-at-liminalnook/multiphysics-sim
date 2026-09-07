use nalgebra::{Matrix3, Rotation3, Vector3};
use sim_domain_robot::articulated::{ContactPoint, LinkKin};
use sim_runtime::{
    session::{Scene, Session},
    task_observation::{TaskObservationConfig, TaskObserver, point_in_moving_frame},
};

fn kin(p: Vector3<f64>, r: Matrix3<f64>, vel: Vector3<f64>, w: Vector3<f64>) -> LinkKin {
    LinkKin {
        p,
        r,
        vel,
        w,
        acc: Vector3::zeros(),
        alpha: Vector3::zeros(),
    }
}
#[test]
fn rotating_frame_velocity_matches_an_independent_time_derivative() {
    let world = |t: f64| {
        let body = kin(
            Vector3::new(2.0 + 0.4 * t, -0.2, 1.0),
            Rotation3::from_axis_angle(&Vector3::z_axis(), 0.3 + 0.7 * t).into_inner(),
            Vector3::new(0.4, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 0.7),
        );
        let link = kin(
            Vector3::new(3.0 + 0.2 * t, 0.5 - 0.1 * t, 0.7),
            Rotation3::from_axis_angle(&Vector3::y_axis(), -0.4 + 1.2 * t).into_inner(),
            Vector3::new(0.2, -0.1, 0.0),
            Vector3::new(0.0, 1.2, 0.0),
        );
        (body, link)
    };
    let local = Vector3::new(0.2, -0.3, 0.1);
    let (body, link) = world(0.0);
    let (_, actual) = point_in_moving_frame(&link, local, &body);
    let position = |t| {
        let (b, l) = world(t);
        b.r.transpose() * (l.p + l.r * local - b.p)
    };
    let numeric = (position(1e-6) - position(-1e-6)) / (2e-6);
    assert!((numeric - actual).norm() < 1e-9);
    // A point fixed to the rotating body must have zero relative velocity.
    assert!(point_in_moving_frame(&body, local, &body).1.norm() < 1e-14);
}

#[test]
fn named_task_channels_preserve_frames_units_and_exclude_internal_contacts() {
    let mut scene: Scene = serde_json::from_str(include_str!(
        "../../../examples/interactive/pendulum.scene.json"
    ))
    .unwrap();
    scene.robot.source["cad_sha256"] = serde_json::json!("synthetic-observation-test");
    scene.options.contact = true;
    let session = Session::new(scene, 0).unwrap();
    let art = &session.robot.art;
    let config:TaskObservationConfig=serde_json::from_value(serde_json::json!({"observation_source":"ideal_rigid_body_diagnostics","expected_cad_sha256":"synthetic-observation-test","reference_link":"ground","markers":[{"id":"tip","link":"pendulum","local_point_m":[1.0,0.0,0.0]}],"floor_forces":true})).unwrap();
    let observer = TaskObserver::new(art, config.clone()).unwrap();
    let mut links = vec![
        kin(
            Vector3::zeros(),
            Matrix3::identity(),
            Vector3::zeros(),
            Vector3::zeros()
        );
        art.links.len()
    ];
    let bi = art.links.iter().position(|l| l.name == "ground").unwrap();
    let mi = art.links.iter().position(|l| l.name == "pendulum").unwrap();
    links[bi].r =
        Rotation3::from_axis_angle(&Vector3::y_axis(), std::f64::consts::FRAC_PI_2).into_inner();
    links[mi].p = Vector3::new(1.0, 0.0, 0.0);
    let contact = |other, force| ContactPoint {
        link: mi,
        other,
        point: Vector3::zeros(),
        force,
        penetration: 0.001,
    };
    let values = observer
        .observe_evaluation(
            &links,
            &[
                contact(None, Vector3::new(1.0, 2.0, 3.0)),
                contact(None, Vector3::new(2.0, 0.0, 1.0)),
                contact(Some(bi), Vector3::new(100.0, 200.0, 300.0)),
            ],
        )
        .unwrap();
    let lookup = |n: &str| {
        values[observer
            .channels()
            .iter()
            .position(|c| c.name == n)
            .unwrap()]
    };
    assert!((lookup("body.gravity_direction.x") - 1.0).abs() < 1e-14);
    assert!((lookup("marker.tip.position.z") - 2.0).abs() < 1e-14);
    assert_eq!(lookup("marker.tip.floor_force_world.x"), 3.0);
    assert_eq!(lookup("marker.tip.floor_force_world.z"), 4.0);
    assert_eq!(
        observer
            .channels()
            .iter()
            .find(|c| c.name == "marker.tip.position.x")
            .unwrap()
            .unit(),
        "m"
    );
    let mut bad = config.clone();
    bad.expected_cad_sha256 = "other".into();
    assert!(TaskObserver::new(art, bad).is_err());
    let mut bad = config.clone();
    bad.reference_link = "missing".into();
    assert!(TaskObserver::new(art, bad).is_err());
    let mut bad = config;
    bad.markers[0].local_point_m[0] = f64::NAN;
    assert!(TaskObserver::new(art, bad).is_err());
}
