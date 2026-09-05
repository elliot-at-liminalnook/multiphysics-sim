mod common;
use common::*;

fn samples(seed: u64, noise: f64) -> Vec<f64> {
    let mut model = empty_model();
    model.uncertainty.seed = seed;
    model
        .links
        .push(box_link("ground", [0.05; 3], 1., [0., 0., 1.], true));
    model.sensors.push(
        serde_json::from_value(serde_json::json!({
            "name":"imu", "link":"ground", "rate_hz":100., "noise":{"accel":noise}
        }))
        .unwrap(),
    );
    let mut rig = Rig::new(model, &[("contact", 0.)], euler());
    (0..20)
        .map(|_| {
            rig.runtime.advance(0.01, 0.001).unwrap();
            rig.state("imu.ax")
        })
        .collect()
}

#[test]
fn cad_sensor_seed_repeats_and_selects_independent_noise_streams() {
    let a = samples(17, 0.2);
    assert_eq!(a, samples(17, 0.2));
    assert_ne!(a, samples(18, 0.2));
    assert_ne!(a, samples((1_u64 << 53) - 1, 0.2));
    assert_eq!(samples(17, 0.), samples(18, 0.));
}

#[test]
fn multiple_cad_imus_keep_axis_units_and_port_values_in_compiler_order() {
    use sim_core::{PortSchema, QuantityKind};
    let mut model = empty_model();
    model.links.push(box_link("ground", [0.05; 3], 1., [0., 0., 1.], true));
    // Opposite authoring/name order and different values expose channel swaps.
    for (name, offset) in [("zulu", 10.), ("alpha", 0.)] {
        model.sensors.push(serde_json::from_value(serde_json::json!({
            "name":name, "link":"ground", "rate_hz":100.,
            "bias":{"accel":[offset+1.,offset+2.,offset+3.],"gyro":[offset+4.,offset+5.,offset+6.]}
        })).unwrap());
    }
    let gravity = -model.gravity[2];
    let mut rig = Rig::new(model, &[("contact", 0.)], euler());
    rig.runtime.advance(0.03, 0.001).unwrap();
    for (name, offset) in [("zulu", 10.), ("alpha", 0.)] {
        for (k, axis) in ["ax", "ay", "az", "gx", "gy", "gz"].iter().enumerate() {
            let expected = offset + k as f64 + 1. + if k == 2 { gravity } else { 0. };
            let kind = if k < 3 { QuantityKind::LinearAcceleration } else { QuantityKind::AngularVelocity };
            let (port, declaration) = rig.runtime.model.ports.iter().find(|(_, p)| p.name == format!("imu.{name}.{axis}")).unwrap();
            assert_eq!(declaration.schema, PortSchema::SignalOut(kind));
            let value = rig.runtime.get(rig.runtime.signal_id(port));
            assert!((value - expected).abs() < 1e-8, "{name}.{axis}: {value} vs {expected}");
            let state = rig.runtime.state_id(rig.behavior, &format!("{name}.{axis}"));
            assert_eq!(rig.runtime.model.state.iter().find(|(id, _)| *id == state).unwrap().1.quantity, kind);
            assert!((rig.runtime.get(state) - value).abs() < 1e-10);
        }
    }
}

#[test]
fn imported_robot_schema_validates_handles_options_and_exact_model_ports() {
    use std::{collections::BTreeMap, sync::Arc};
    use sim_domain_robot::{register_model, Articulated, Options, ARTICULATED};
    let mut model = empty_model();
    model.links.push(box_link("ground", [0.05; 3], 1., [0., 0., 1.], true));
    let art = Articulated::new(Arc::new(model.clone()), &Options::default()).unwrap();
    let handle = register_model(model);
    let mut parameters = BTreeMap::from_iter(art.port_parameters());
    parameters.insert("model".into(), handle);
    let registry = registry();
    let descriptor = registry.get(&ARTICULATED.into()).unwrap();
    assert!(descriptor.parameters.is_some());
    descriptor.validate_parameters(&parameters).unwrap();
    descriptor.equations.unwrap()(&parameters).unwrap();
    for (name, value) in [("model", handle+0.5), ("planar", 0.5), ("flex.modes", 0.),
        ("loop.cfm.translation", -1.), ("loop.cfm.rotation", -1.), ("loop.cfm", 1e-6),
        ("imu.bad.ax", 1.), ("initial.joint.missing.angle", 1.)] {
        let mut bad = parameters.clone(); bad.insert(name.into(), value);
        let error = descriptor.validate_parameters(&bad).unwrap_err().to_string();
        assert!(error.contains(name), "{error}");
    }
    for modify in [false, true] {
        let mut bad = parameters.clone();
        if modify { bad.insert("imu.missing.ax".into(), 0.); }
        else { bad.remove("contact.ground"); }
        descriptor.validate_parameters(&bad).unwrap();
        assert!(descriptor.equations.unwrap()(&bad).err().unwrap().to_string().contains("model's ports"));
    }
    for handle in [f64::NAN, f64::INFINITY, -1., handle+0.5] {
        assert!(sim_domain_robot::model::model_by_handle(handle).is_none());
    }
}
