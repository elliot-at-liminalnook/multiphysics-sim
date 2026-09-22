use serde::{Deserialize, Serialize};
use sim_core::{
    BehaviorRegistry, QuantityKind as Q,
    definitions::DefinitionId,
    primitive::{Descriptor, Field},
};
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Input {
    value: f64,
}
fn double(i: Input) -> Result<Input, String> {
    if !i.value.is_finite() {
        return Err("nonfinite".into());
    }
    Ok(Input {
        value: i.value * 2.,
    })
}
fn descriptor() -> Descriptor {
    Descriptor::new(
        "example.double",
        "Double an angle",
        vec![Field::quantity("value", Q::Angle, "scalar")],
        vec![Field::quantity("value", Q::Angle, "scalar")],
        &["example only"],
    )
}
#[test]
fn versioned_discovery_calls_and_failed_registration_are_transactional() {
    let mut r = BehaviorRegistry::default();
    r.register_primitive(descriptor(), double).unwrap();
    assert!(r.register_primitive(descriptor(), double).is_err());
    let id = DefinitionId::new("example.double", 1);
    assert_eq!(
        r.call_primitive(&id, serde_json::json!({"value":2}))
            .unwrap(),
        serde_json::json!({"value":4.})
    );
    assert!(
        r.call_primitive(&id, serde_json::json!({"value":2,"typo":0}))
            .is_err()
    );
    assert!(
        r.call_primitive(
            &DefinitionId::new("example.double", 2),
            serde_json::json!({"value":2})
        )
        .is_err()
    );
    let mut bad = descriptor();
    bad.id.name = "bad".into();
    bad.inputs[0].unit = "m".into();
    assert!(r.register_primitive(bad, double).is_err());
    let mut unknown = descriptor();
    unknown.id.name = "unknown".into();
    unknown.inputs[0] = Field::quantity(
        "value",
        Q::named("unregistered.quantity", 1, "rad"),
        "scalar",
    );
    assert!(r.register_primitive(unknown, double).is_err());
    let mut stale = descriptor();
    stale.id.name = "stale".into();
    stale.inputs[0] = Field::quantity("value", Q::named("sim.quantity.Angle", 1, "m"), "scalar");
    assert!(r.register_primitive(stale, double).is_err());
    assert_eq!(r.primitive_descriptors().count(), 1);
    assert_eq!(
        r.clone()
            .call_primitive(&id, serde_json::json!({"value":2}))
            .unwrap(),
        serde_json::json!({"value":4.})
    );
}
