use serde::{Deserialize, Serialize};
use sim_core::{
    BehaviorRegistry, Channel, Contract, Coupler, QuantityKind as Q,
    primitive::{Descriptor, Field},
};
use sim_script::{RhaiController, Sources};
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    angle: f64,
}
fn registry() -> BehaviorRegistry {
    let mut r = BehaviorRegistry::default();
    r.register_primitive(
        Descriptor::new(
            "test.double",
            "Test callback",
            vec![Field::quantity("angle", Q::Angle, "scalar")],
            vec![Field::quantity("angle", Q::Angle, "scalar")],
            &[],
        ),
        |i: Input| {
            Ok(Input {
                angle: i.angle * 2.,
            })
        },
    )
    .unwrap();
    r
}
#[test]
fn native_registry_function_is_available_in_captured_rhai_controller() {
    let r = registry();
    let source = Sources::single(
        "primitive.rhai",
        r#"fn control(t,s,a,state) { let v=primitive_call("test.double",1,#{angle:s.angle}); a.target=v.angle; #{commands:a,state:state} }"#,
    );
    let mut c = RhaiController::with_seed_and_registry(source, Default::default(), 1, &r).unwrap();
    c.open(&Contract {
        element: "test".into(),
        period: 0.01,
        sensors: vec![Channel {
            name: "angle".into(),
            kind: Q::Angle,
        }],
        actuators: vec![Channel {
            name: "target".into(),
            kind: Q::Angle,
        }],
    })
    .unwrap();
    let mut output = [0.];
    c.sample(0., &[0.25], &mut output).unwrap();
    assert_eq!(output, [0.5]);
    let catalogue = sim_script::primitive_catalogue(&r);
    assert_eq!(catalogue[0]["inputs"][0]["unit"], "rad");
}
