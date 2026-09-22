//! Check runtime observation bindings for a captured CAD system, without stepping.
use sim_inspect::{
    Availability, SampleValue,
    runtime::{FrameStamp, RuntimeInspection},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: inspect_scene_observations scene.json")?;
    let bytes = std::fs::read(&path)?;
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let scene: sim_runtime::session::Scene = serde_json::from_slice(&bytes)?;
    let registry = sim_runtime::registry();
    let robot = sim_runtime::PhysicalRobot::build(scene.robot, &registry, &scene.options)?;
    let snapshot_before = serde_json::to_value(robot.runtime.snapshot())?;
    let inspection = RuntimeInspection::new(
        &robot.runtime,
        &registry,
        &hash,
        1,
        &robot.composition.identities,
    )?;
    let subscription = inspection.subscribe(
        inspection
            .description
            .observables
            .keys()
            .map(String::as_str),
    )?;
    let frame = subscription.sample(
        &robot.runtime,
        FrameStamp {
            run_id: "initial-observation-check",
            generation: 0,
            sequence: 0,
            step: 0,
        },
    )?;
    assert_eq!(
        snapshot_before,
        serde_json::to_value(robot.runtime.snapshot())?
    );
    let unavailable: Vec<_> = inspection
        .description
        .observables
        .values()
        .filter(|d| matches!(d.availability, Availability::Unavailable { .. }))
        .collect();
    let counts = |predicate: fn(&SampleValue) -> bool| {
        frame.values.values().filter(|v| predicate(v)).count()
    };
    let binding_failures = unavailable
        .iter()
        .filter(|d| {
            !matches!(
                d.location,
                sim_inspect::ObservationLocation::Diagnostic { .. }
            )
        })
        .count();
    let report = serde_json::json!({
        "source": path,
        "source_blake3": hash,
        "description_id": inspection.description.id,
        "definition_fingerprint": robot.runtime.definitions.fingerprint(),
        "components": inspection.description.components.len(),
        "ports": inspection.description.ports.len(),
        "nets": inspection.description.nets.len(),
        "observables": subscription.len(),
        "unavailable_bindings": unavailable,
        "binding_failures": binding_failures,
        "initial_endpoint_samples": counts(|v| matches!(v, SampleValue::Committed { .. })),
        "initial_stage_samples": counts(|v| matches!(v, SampleValue::AcceptedStage { .. })),
        "initial_unavailable_samples": counts(|v| matches!(v, SampleValue::Unavailable { .. })),
        "snapshot_unchanged": true,
        "stepped": false,
    });
    println!("{}", serde_json::to_string_pretty(&report)?);
    if binding_failures != 0 {
        return Err(
            format!("{binding_failures} state/port bindings are unavailable; see report").into(),
        );
    }
    Ok(())
}
