//! M6: every example system's realtime profile has a published measurement
//! taken on exactly this model, remeasuring reproduces it, and the errors
//! stay within the published bounds. The browser check
//! (`node web/system-realtime-check.mjs`) repeats the measurement in Chromium.
use sim_runtime::realtime_fidelity;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn published_realtime_measurements_hold() {
    let (registry, _) = sim_runtime::registry_with_parts(&root().join("library/parts"));
    for path in ["examples/systems-builder/worm-drive/winch.system.json", "examples/systems-builder/parts-from-parts/winch.system.json", "examples/systems-builder/parts-from-parts/leg.system.json"] {
        let doc: sim_system::SystemDocument = serde_json::from_slice(&std::fs::read(root().join(path)).unwrap()).unwrap();
        let profile = doc.realtime.as_ref().unwrap_or_else(|| panic!("{path}: no realtime profile"));
        let published = profile.measured.as_ref().unwrap_or_else(|| panic!("{path}: no published measurement (sim-system realtime FILE --publish)"));
        assert_eq!(published.content_hash, realtime_fidelity::measured_hash(&doc), "{path}: the measurement is for another version of the model; remeasure");
        let fresh = realtime_fidelity::measure(&doc, &registry).unwrap().measurement;
        for (k, e) in &fresh.errors {
            assert!((e - published.errors[k]).abs() < 1e-9, "{path}: {k} error {e} vs published {}", published.errors[k]);
        }
        realtime_fidelity::within_bound(&doc, &fresh).unwrap_or_else(|e| panic!("{path}: {e}"));
        assert!(fresh.realtime_speed > fresh.detailed_speed, "{path}: the realtime profile should be faster");
        eprintln!("{path}: errors {:?}; realtime {:.1}× vs detailed {:.1}× ({})", fresh.errors, fresh.realtime_speed, fresh.detailed_speed, fresh.host);
    }
}
