//! M7: fitting a library part to measured data through the shared runtime.
//! The fitter recovers known parameters from synthetic data, the real knee
//! servo fit reproduces the campaign's independently derived motor constant,
//! and the published part carries the fitted values as measured, with
//! uncertainty and the data's hash.
use sim_runtime::part_fit::{self, Condition, Unknown};
use sim_system::{ParameterBinding, SystemDocument};
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn read<T: serde::de::DeserializeOwned>(p: &str) -> T {
    serde_json::from_slice(&std::fs::read(root().join(p)).unwrap()).unwrap()
}
const SPEC: &str = "examples/actuators/hx30hm/accepted/hx30hm-knee-gearmotor.fit.json";

fn spec() -> (SystemDocument, Vec<Unknown>, Vec<Condition>, f64, serde_json::Value) {
    let spec: serde_json::Value = read(SPEC);
    let model: SystemDocument = read(spec["model"].as_str().unwrap());
    (model, serde_json::from_value(spec["unknowns"].clone()).unwrap(), serde_json::from_value(spec["conditions"].clone()).unwrap(), spec["duration"].as_f64().unwrap(), spec)
}

#[test]
fn the_fitter_recovers_known_parameters() {
    let registry = sim_runtime::registry();
    let (model, unknowns, conditions, duration, _) = spec();
    let truth = [0.0105, 0.08, -0.05];
    let predicted = part_fit::predict(&model, &registry, &unknowns, &truth, &conditions, duration).unwrap();
    // Measurement noise of about 1 % (deterministic).
    let synthetic: Vec<Condition> = conditions.iter().zip(&predicted).enumerate().map(|(i, (c, y))| Condition { measured: y * (1. + 0.01 * ((i as f64 * 2.3).sin())), ..c.clone() }).collect();
    let fit = part_fit::fit(&model, &registry, &unknowns, &synthetic, duration).unwrap();
    for (k, ((name, v), (_, sd))) in fit.values.iter().zip(&fit.uncertainties).enumerate() {
        eprintln!("{name}: {v:.5} ± {sd:.5} (truth {})", truth[k]);
        assert!((v - truth[k]).abs() <= 3. * sd + 1e-9, "{name} {v} vs {} ± {sd}", truth[k]);
    }
}

#[test]
fn knee_servo_fit_matches_the_campaign_and_is_published_as_measured() {
    let registry = sim_runtime::registry();
    let (model, unknowns, conditions, duration, spec) = spec();
    let data_path = spec["data"]["path"].as_str().unwrap();
    let hash = blake3::hash(&std::fs::read(root().join(data_path)).unwrap()).to_hex().to_string();
    assert_eq!(spec["data"]["blake3"].as_str(), Some(hash.as_str()), "the dataset changed since the spec was written");
    let fit = part_fit::fit(&model, &registry, &unknowns, &conditions, duration).unwrap();
    let (k, sd) = (fit.values[0].1, fit.uncertainties[0].1);
    let reference = &spec["compare"]["k"];
    let (r, rsd) = (reference["value"].as_f64().unwrap(), reference["uncertainty"].as_f64().unwrap());
    eprintln!("k = {k:.6} ± {sd:.6}; campaign {r:.6} ± {rsd:.6}; rms {:.4} rad/s", fit.rms);
    assert!((k - r).abs() <= rsd, "fitted k disagrees with the campaign's derivation");
    assert!(fit.rms < 0.05, "residuals {} rad/s", fit.rms);

    // The library part carries the promoted values as measured, citing the data.
    let lib: serde_json::Value = read("library/systems/hx30hm_knee_gearmotor.definition.json");
    let motor = &lib["definitions"]["hx30hm_knee_gearmotor"]["instances"]["motor"]["parameters"];
    let kt: ParameterBinding = serde_json::from_value(motor["torque_constant"].clone()).unwrap();
    match kt {
        ParameterBinding::Value { value, provenance: Some(sim_inspect::Provenance::Measured { source }), uncertainty: Some(u), .. } => {
            assert_eq!(source.artifact_hash, hash);
            assert_eq!(source.path, data_path);
            assert!((value - k).abs() < 1e-6 && (u - sd).abs() < 1e-6, "published {value} ± {u} vs fitted {k} ± {sd}");
        }
        other => panic!("torque_constant is not a measured value: {other:?}"),
    }
    // What was not fitted stays an estimate.
    let r: ParameterBinding = serde_json::from_value(motor["resistance"].clone()).unwrap();
    assert!(matches!(r, ParameterBinding::Value { provenance: Some(sim_inspect::Provenance::Estimated { .. }), .. }));
}
