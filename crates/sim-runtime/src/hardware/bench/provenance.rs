//! Acquisition source identity and exact retained sources. This is a conservative
//! shared implementation identity, not a compiler/dependency-lock or FPGA-image proof.
//! Standalone test/fixture sources and compatibility executable wrappers are excluded.
use serde_json::{Value, json};
use std::{fs, path::Path};

pub(super) const SCHEMA: &str = "sim-runtime-acquisition-source-v1";
type Error = Box<dyn std::error::Error>;

// Explicit named compile-time inputs. Keep this list current when splitting or
// adding production behavior. Sorting is performed explicitly by the hashing operation.
const SOURCES: &[(&str, &[u8])] = &[
    (
        "crates/sim-domain-control/src/adaptive_braking.rs",
        include_bytes!("../../../../sim-domain-control/src/adaptive_braking.rs"),
    ),
    (
        "crates/sim-domain-control/src/angle_integral.rs",
        include_bytes!("../../../../sim-domain-control/src/angle_integral.rs"),
    ),
    (
        "crates/sim-domain-control/src/command_lease.rs",
        include_bytes!("../../../../sim-domain-control/src/command_lease.rs"),
    ),
    (
        "crates/sim-domain-control/src/contact_phase/sequence.rs",
        include_bytes!("../../../../sim-domain-control/src/contact_phase/sequence.rs"),
    ),
    (
        "crates/sim-domain-control/src/contact_phase/steered.rs",
        include_bytes!("../../../../sim-domain-control/src/contact_phase/steered.rs"),
    ),
    (
        "crates/sim-domain-control/src/contact_phase.rs",
        include_bytes!("../../../../sim-domain-control/src/contact_phase.rs"),
    ),
    (
        "crates/sim-domain-control/src/contact_slip.rs",
        include_bytes!("../../../../sim-domain-control/src/contact_slip.rs"),
    ),
    (
        "crates/sim-domain-control/src/displacement.rs",
        include_bytes!("../../../../sim-domain-control/src/displacement.rs"),
    ),
    (
        "crates/sim-domain-control/src/distillation.rs",
        include_bytes!("../../../../sim-domain-control/src/distillation.rs"),
    ),
    (
        "crates/sim-domain-control/src/drive/geometry.rs",
        include_bytes!("../../../../sim-domain-control/src/drive/geometry.rs"),
    ),
    (
        "crates/sim-domain-control/src/drive/kinematics.rs",
        include_bytes!("../../../../sim-domain-control/src/drive/kinematics.rs"),
    ),
    (
        "crates/sim-domain-control/src/drive/mod.rs",
        include_bytes!("../../../../sim-domain-control/src/drive/mod.rs"),
    ),
    (
        "crates/sim-domain-control/src/drive/profile.rs",
        include_bytes!("../../../../sim-domain-control/src/drive/profile.rs"),
    ),
    (
        "crates/sim-domain-control/src/drive/steered.rs",
        include_bytes!("../../../../sim-domain-control/src/drive/steered.rs"),
    ),
    (
        "crates/sim-domain-control/src/elements.rs",
        include_bytes!("../../../../sim-domain-control/src/elements.rs"),
    ),
    (
        "crates/sim-domain-control/src/gait_script.rs",
        include_bytes!("../../../../sim-domain-control/src/gait_script.rs"),
    ),
    (
        "crates/sim-domain-control/src/heading.rs",
        include_bytes!("../../../../sim-domain-control/src/heading.rs"),
    ),
    (
        "crates/sim-domain-control/src/lib.rs",
        include_bytes!("../../../../sim-domain-control/src/lib.rs"),
    ),
    (
        "crates/sim-domain-control/src/load_damping.rs",
        include_bytes!("../../../../sim-domain-control/src/load_damping.rs"),
    ),
    (
        "crates/sim-domain-control/src/maneuver_script.rs",
        include_bytes!("../../../../sim-domain-control/src/maneuver_script.rs"),
    ),
    (
        "crates/sim-domain-control/src/motion_clock.rs",
        include_bytes!("../../../../sim-domain-control/src/motion_clock.rs"),
    ),
    (
        "crates/sim-domain-control/src/motion_parameters.rs",
        include_bytes!("../../../../sim-domain-control/src/motion_parameters.rs"),
    ),
    (
        "crates/sim-domain-control/src/motion_primitives.rs",
        include_bytes!("../../../../sim-domain-control/src/motion_primitives.rs"),
    ),
    (
        "crates/sim-domain-control/src/neural.rs",
        include_bytes!("../../../../sim-domain-control/src/neural.rs"),
    ),
    (
        "crates/sim-domain-control/src/notes.rs",
        include_bytes!("../../../../sim-domain-control/src/notes.rs"),
    ),
    (
        "crates/sim-domain-control/src/optimization.rs",
        include_bytes!("../../../../sim-domain-control/src/optimization.rs"),
    ),
    (
        "crates/sim-domain-control/src/periodic_drift.rs",
        include_bytes!("../../../../sim-domain-control/src/periodic_drift.rs"),
    ),
    (
        "crates/sim-domain-control/src/planar.rs",
        include_bytes!("../../../../sim-domain-control/src/planar.rs"),
    ),
    (
        "crates/sim-domain-control/src/planar_prediction.rs",
        include_bytes!("../../../../sim-domain-control/src/planar_prediction.rs"),
    ),
    (
        "crates/sim-domain-control/src/policy_search.rs",
        include_bytes!("../../../../sim-domain-control/src/policy_search.rs"),
    ),
    (
        "crates/sim-domain-control/src/pose_script.rs",
        include_bytes!("../../../../sim-domain-control/src/pose_script.rs"),
    ),
    (
        "crates/sim-domain-control/src/ppo.rs",
        include_bytes!("../../../../sim-domain-control/src/ppo.rs"),
    ),
    (
        "crates/sim-domain-control/src/pwm.rs",
        include_bytes!("../../../../sim-domain-control/src/pwm.rs"),
    ),
    (
        "crates/sim-domain-control/src/sampled_fixed_pd.rs",
        include_bytes!("../../../../sim-domain-control/src/sampled_fixed_pd.rs"),
    ),
    (
        "crates/sim-domain-control/src/smooth_return.rs",
        include_bytes!("../../../../sim-domain-control/src/smooth_return.rs"),
    ),
    (
        "crates/sim-domain-control/src/stepping.rs",
        include_bytes!("../../../../sim-domain-control/src/stepping.rs"),
    ),
    (
        "crates/sim-domain-control/src/support_preload.rs",
        include_bytes!("../../../../sim-domain-control/src/support_preload.rs"),
    ),
    (
        "crates/sim-domain-control/src/trajectory.rs",
        include_bytes!("../../../../sim-domain-control/src/trajectory.rs"),
    ),
    (
        "crates/sim-script/src/lib.rs",
        include_bytes!("../../../../sim-script/src/lib.rs"),
    ),
    (
        "crates/sim-script/src/expr.rs",
        include_bytes!("../../../../sim-script/src/expr.rs"),
    ),
    (
        "crates/sim-script/src/pacing.rs",
        include_bytes!("../../../../sim-script/src/pacing.rs"),
    ),
    (
        "crates/sim-script/src/presentation.rs",
        include_bytes!("../../../../sim-script/src/presentation.rs"),
    ),
    (
        "crates/sim-script/src/drive.rs",
        include_bytes!("../../../../sim-script/src/drive.rs"),
    ),
    (
        "crates/sim-domain-control/src/fixed_pd.rs",
        include_bytes!("../../../../sim-domain-control/src/fixed_pd.rs"),
    ),
    (
        "crates/sim-domain-control/src/fixed_pd/law.rs",
        include_bytes!("../../../../sim-domain-control/src/fixed_pd/law.rs"),
    ),
    (
        "crates/sim-domain-control/src/pulse.rs",
        include_bytes!("../../../../sim-domain-control/src/pulse.rs"),
    ),
    (
        "crates/sim-domain-control/src/pwm_feedback.rs",
        include_bytes!("../../../../sim-domain-control/src/pwm_feedback.rs"),
    ),
    (
        "crates/sim-domain-control/src/reference_governor.rs",
        include_bytes!("../../../../sim-domain-control/src/reference_governor.rs"),
    ),
    (
        "crates/sim-domain-robot/src/motor.rs",
        include_bytes!("../../../../sim-domain-robot/src/motor.rs"),
    ),
    (
        "crates/sim-runtime/src/acquisition/actuator_promotion.rs",
        include_bytes!("../../acquisition/actuator_promotion.rs"),
    ),
    (
        "crates/sim-runtime/src/acquisition/actuator_sweep.rs",
        include_bytes!("../../acquisition/actuator_sweep.rs"),
    ),
    (
        "crates/sim-runtime/src/acquisition/calibration.rs",
        include_bytes!("../../acquisition/calibration.rs"),
    ),
    (
        "crates/sim-runtime/src/acquisition/calibration_serial.rs",
        include_bytes!("../../acquisition/calibration_serial.rs"),
    ),
    (
        "crates/sim-runtime/src/acquisition/calibration_sweep.rs",
        include_bytes!("../../acquisition/calibration_sweep.rs"),
    ),
    (
        "crates/sim-runtime/src/acquisition/characterization.rs",
        include_bytes!("../../acquisition/characterization.rs"),
    ),
    (
        "crates/sim-runtime/src/acquisition/motor_identification.rs",
        include_bytes!("../../acquisition/motor_identification.rs"),
    ),
    (
        "crates/sim-runtime/src/acquisition/servo_bus.rs",
        include_bytes!("../../acquisition/servo_bus.rs"),
    ),
    (
        "crates/sim-runtime/src/acquisition/servo_safety.rs",
        include_bytes!("../../acquisition/servo_safety.rs"),
    ),
    (
        "crates/sim-runtime/src/acquisition/virtual_bench.rs",
        include_bytes!("../../acquisition/virtual_bench.rs"),
    ),
    (
        "crates/sim-runtime/src/actuator_bench.rs",
        include_bytes!("../../actuator_bench.rs"),
    ),
    (
        "crates/sim-runtime/src/actuator_bench/group.rs",
        include_bytes!("../../actuator_bench/group.rs"),
    ),
    (
        "crates/sim-runtime/src/bench.rs",
        include_bytes!("../../bench.rs"),
    ),
    (
        "crates/sim-runtime/src/controller_refinement/authoring.rs",
        include_bytes!("../../controller_refinement/authoring.rs"),
    ),
    (
        "crates/sim-runtime/src/controller_refinement/cad.rs",
        include_bytes!("../../controller_refinement/cad.rs"),
    ),
    (
        "crates/sim-runtime/src/controller_refinement/calibration.rs",
        include_bytes!("../../controller_refinement/calibration.rs"),
    ),
    (
        "crates/sim-runtime/src/controller_refinement/calibration_data.rs",
        include_bytes!("../../controller_refinement/calibration_data.rs"),
    ),
    (
        "crates/sim-runtime/src/controller_refinement/context.rs",
        include_bytes!("../../controller_refinement/context.rs"),
    ),
    (
        "crates/sim-runtime/src/controller_refinement/control.rs",
        include_bytes!("../../controller_refinement/control.rs"),
    ),
    (
        "crates/sim-runtime/src/controller_refinement/electrical_measurements.rs",
        include_bytes!("../../controller_refinement/electrical_measurements.rs"),
    ),
    (
        "crates/sim-runtime/src/controller_refinement/evidence.rs",
        include_bytes!("../../controller_refinement/evidence.rs"),
    ),
    (
        "crates/sim-runtime/src/controller_refinement/fpga.rs",
        include_bytes!("../../controller_refinement/fpga.rs"),
    ),
    (
        "crates/sim-runtime/src/controller_refinement/fpga_batch.rs",
        include_bytes!("../../controller_refinement/fpga_batch.rs"),
    ),
    (
        "crates/sim-runtime/src/controller_refinement/fpga_design.rs",
        include_bytes!("../../controller_refinement/fpga_design.rs"),
    ),
    (
        "crates/sim-runtime/src/controller_refinement/fpga_events.rs",
        include_bytes!("../../controller_refinement/fpga_events.rs"),
    ),
    (
        "crates/sim-runtime/src/controller_refinement/fpga_group.rs",
        include_bytes!("../../controller_refinement/fpga_group.rs"),
    ),
    (
        "crates/sim-runtime/src/controller_refinement/fpga_review.rs",
        include_bytes!("../../controller_refinement/fpga_review.rs"),
    ),
    (
        "crates/sim-runtime/src/controller_refinement/fpga_upload.rs",
        include_bytes!("../../controller_refinement/fpga_upload.rs"),
    ),
    (
        "crates/sim-runtime/src/controller_refinement/fpga_voltage.rs",
        include_bytes!("../../controller_refinement/fpga_voltage.rs"),
    ),
    (
        "crates/sim-runtime/src/controller_refinement/live_reference.rs",
        include_bytes!("../../controller_refinement/live_reference.rs"),
    ),
    (
        "crates/sim-runtime/src/controller_refinement/live_stream.rs",
        include_bytes!("../../controller_refinement/live_stream.rs"),
    ),
    (
        "crates/sim-runtime/src/controller_refinement/mod.rs",
        include_bytes!("../../controller_refinement/mod.rs"),
    ),
    (
        "crates/sim-runtime/src/controller_refinement/motor_response.rs",
        include_bytes!("../../controller_refinement/motor_response.rs"),
    ),
    (
        "crates/sim-runtime/src/controller_refinement/power.rs",
        include_bytes!("../../controller_refinement/power.rs"),
    ),
    (
        "crates/sim-runtime/src/controller_refinement/recording.rs",
        include_bytes!("../../controller_refinement/recording.rs"),
    ),
    (
        "crates/sim-runtime/src/controller_refinement/sweep_review.rs",
        include_bytes!("../../controller_refinement/sweep_review.rs"),
    ),
    (
        "crates/sim-runtime/src/controller_refinement/tracking.rs",
        include_bytes!("../../controller_refinement/tracking.rs"),
    ),
    (
        "crates/sim-runtime/src/controller_refinement/trajectory_binding.rs",
        include_bytes!("../../controller_refinement/trajectory_binding.rs"),
    ),
    (
        "crates/sim-runtime/src/controller_refinement/transients.rs",
        include_bytes!("../../controller_refinement/transients.rs"),
    ),
    (
        "crates/sim-runtime/src/controller_refinement/workspace.rs",
        include_bytes!("../../controller_refinement/workspace.rs"),
    ),
    (
        "crates/sim-runtime/src/hardware/bench/acquisition.rs",
        include_bytes!("acquisition.rs"),
    ),
    (
        "crates/sim-runtime/src/hardware/bench/hx_controller.rs.inc",
        include_bytes!("hx_controller.rs.inc"),
    ),
    (
        "crates/sim-runtime/src/hardware/bench/hx_device.rs.inc",
        include_bytes!("hx_device.rs.inc"),
    ),
    (
        "crates/sim-runtime/src/hardware/bench/hx_fpga.rs.inc",
        include_bytes!("hx_fpga.rs.inc"),
    ),
    (
        "crates/sim-runtime/src/hardware/bench/hx_safety_probe.rs.inc",
        include_bytes!("hx_safety_probe.rs.inc"),
    ),
    (
        "crates/sim-runtime/src/hardware/bench/hx_sweep_support.rs.inc",
        include_bytes!("hx_sweep_support.rs.inc"),
    ),
    (
        "crates/sim-runtime/src/hardware/bench/mod.rs",
        include_bytes!("mod.rs"),
    ),
    (
        "crates/sim-runtime/src/hardware/bench/motion.rs.inc",
        include_bytes!("motion.rs.inc"),
    ),
    (
        "crates/sim-runtime/src/hardware/bench/provenance.rs",
        include_bytes!("provenance.rs"),
    ),
    (
        "crates/sim-runtime/src/hardware/bench/virtual_run.rs",
        include_bytes!("virtual_run.rs"),
    ),
    (
        "crates/sim-runtime/src/hardware/calibration/campaign.rs",
        include_bytes!("../calibration/campaign.rs"),
    ),
    (
        "crates/sim-runtime/src/hardware/calibration/gait.rs",
        include_bytes!("../calibration/gait.rs"),
    ),
    (
        "crates/sim-runtime/src/hardware/calibration/mod.rs",
        include_bytes!("../calibration/mod.rs"),
    ),
    (
        "crates/sim-runtime/src/hardware/calibration/session.rs",
        include_bytes!("../calibration/session.rs"),
    ),
    (
        "crates/sim-runtime/src/hardware/calibration/worker.rs",
        include_bytes!("../calibration/worker.rs"),
    ),
    (
        "crates/sim-runtime/src/hardware/local.rs",
        include_bytes!("../local.rs"),
    ),
    (
        "crates/sim-runtime/src/hardware/mod.rs",
        include_bytes!("../mod.rs"),
    ),
    (
        "crates/sim-runtime/src/hardware/ownership.rs",
        include_bytes!("../ownership.rs"),
    ),
    (
        "crates/sim-runtime/src/hardware/protocol/bench.rs",
        include_bytes!("../protocol/bench.rs"),
    ),
    (
        "crates/sim-runtime/src/hardware/protocol/calibration.rs",
        include_bytes!("../protocol/calibration.rs"),
    ),
    (
        "crates/sim-runtime/src/hardware/protocol/mod.rs",
        include_bytes!("../protocol/mod.rs"),
    ),
    (
        "crates/sim-runtime/src/publication.rs",
        include_bytes!("../../publication.rs"),
    ),
];

/// Hash schema, source count, then sorted (name length/name, byte length/bytes).
/// All lengths/counts are unsigned 64-bit little endian, avoiding concatenation
/// ambiguity. A production constituent change necessarily changes the hash input.
pub(super) fn fingerprint() -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&(SCHEMA.len() as u64).to_le_bytes());
    hasher.update(SCHEMA.as_bytes());
    hasher.update(&(SOURCES.len() as u64).to_le_bytes());
    let mut ordered = SOURCES.to_vec();
    ordered.sort_by_key(|(name, _)| *name);
    for (name, bytes) in ordered {
        hasher.update(&(name.len() as u64).to_le_bytes());
        hasher.update(name.as_bytes());
        hasher.update(&(bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    }
    hasher.finalize().to_hex().to_string()
}

/// Immutable publication; retries verify exact existing bytes and synchronize
/// them rather than overwriting historical sources. Any failure refuses motion.
fn retain_bytes(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    if path.try_exists()? {
        if fs::read(path)? != bytes {
            return Err(format!("Retained acquisition source differs: {}", path.display()).into());
        }
        crate::publication::confirm_existing(path).into_result()?;
    } else {
        crate::publication::publish(path, bytes, crate::publication::Policy::ImmutableNew)
            .into_result()?;
    }
    Ok(())
}

/// Retain exact split sources and their manifest before transport/acquisition
/// effects. Publication synchronizes each file and directory ancestry. Success
/// is OS durability acknowledgment, not a hardware/storage survival guarantee.
pub(super) fn retain(out: &Path) -> Result<Value, Error> {
    let mut ordered = SOURCES.to_vec();
    ordered.sort_by_key(|(name, _)| *name);
    let mut constituents = Vec::with_capacity(ordered.len());
    for (name, bytes) in ordered {
        let artifact = format!("sources/{name}");
        retain_bytes(&out.join(&artifact), bytes)?;
        constituents.push(json!({
            "name": name,
            "blake3": blake3::hash(bytes).to_hex().to_string(),
            "byte_length": bytes.len(),
            "artifact": artifact,
        }));
    }
    let manifest = json!({
        "schema": SCHEMA,
        "algorithm": "blake3",
        "framing": "u64-le schema length/schema, u64-le source count, sorted u64-le name length/name and u64-le byte length/bytes",
        "composite_blake3": fingerprint(),
        "manifest_artifact": "source-manifest.json",
        "constituents": constituents,
    });
    retain_bytes(
        &out.join("source-manifest.json"),
        &serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(manifest)
}
