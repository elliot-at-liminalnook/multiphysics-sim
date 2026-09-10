use sim_core::{Channel, Contract, Coupler, QuantityKind};
use sim_script::{RhaiController, Sources};

#[test]
fn rhai_uses_shared_displacement_with_transactional_validation() {
    let source = Sources::single(
        "progress.rhai",
        r#"
fn control(t,s,a,state) {
    let axes=if s.invalid>0.0 {"unknown"} else {"xz"};
    let p=net_displacement([1,2,3],[s.x,99,s.z],axes);
    a.distance=p.distance_m;
    #{commands:a,state:state}
}"#,
    );
    let mut controller = RhaiController::new(source, Default::default()).unwrap();
    controller
        .open(&Contract {
            element: "test".into(),
            period: 0.02,
            sensors: vec![
                Channel {
                    name: "x".into(),
                    kind: QuantityKind::Length,
                },
                Channel {
                    name: "z".into(),
                    kind: QuantityKind::Length,
                },
                Channel {
                    name: "invalid".into(),
                    kind: QuantityKind::Dimensionless,
                },
            ],
            actuators: vec![Channel {
                name: "distance".into(),
                kind: QuantityKind::Length,
            }],
        })
        .unwrap();
    let mut out = [0.];
    controller.sample(0., &[4., 7., 0.], &mut out).unwrap();
    assert_eq!(out, [5.]);
    assert!(controller.sample(0.02, &[4., 7., 1.], &mut out).is_err());
    assert_eq!(out, [5.]);
    controller.sample(0.02, &[1., 3., 0.], &mut out).unwrap();
    assert_eq!(out, [0.]);
}
