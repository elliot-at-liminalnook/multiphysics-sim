//! Finite-horizon diagnostic references through shared CAD dynamics and servos.
use sim_domain_control::trajectory::{Interpolation, Keyframe, Trajectory, TrajectoryConfig};
use sim_domain_robot::effective_servo::EffectiveServo;
use sim_runtime::{
    contact_implicit::{ContactImplicitConfig, ContactImplicitPlanner},
    session::{Scene, Session},
    tracking::CaptureConfig,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 4 {
        return Err("usage: compile_contact_implicit scene markers recipe result".into());
    }
    let scene: Scene = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let markers: CaptureConfig = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let recipe: serde_json::Value = serde_json::from_slice(&std::fs::read(&args[2])?)?;
    let result: serde_json::Value = serde_json::from_slice(&std::fs::read(&args[3])?)?;
    let config: ContactImplicitConfig = serde_json::from_value(recipe["config"].clone())?;
    let positions: Vec<Vec<f64>> = serde_json::from_value(result["positions"].clone())?;
    if config.periodic_horizontal_translation || config.initial_velocity.iter().any(|v| *v != 0.) {
        return Err("diagnostic runtime initialization requires a fixed at-rest initial state; periodic orbit startup needs a separate transition".into());
    }
    let mut session = Session::new(scene, 0)?;
    session.robot.art.contact_on = false;
    let seed = session.robot.generalized();
    let planner = ContactImplicitPlanner::new(&session.robot.art, &seed, &markers, config.clone())?;
    let report = planner.evaluate(&positions)?;
    let geometry = planner.audit_geometry(&positions, 5)?;
    if !report.within_planning_tolerances
        || geometry.iter().any(|g| {
            g.maximum_inter_link_penetration_m > 0.
                || g.floor_clearances
                    .iter()
                    .any(|f| f.minimum_clearance_m < -config.maximum_point_penetration_m)
        })
    {
        return Err(
            "declared planning tolerances and sampled geometry gates must pass before compilation"
                .into(),
        );
    }
    let trajectory = TrajectoryConfig {
        interpolation: Interpolation::Linear,
        keyframes: positions
            .iter()
            .enumerate()
            .map(|(k, q)| Keyframe {
                time_s: k as f64 * config.step_s,
                values: q[6..].to_vec(),
            })
            .collect(),
    };
    let curve = Trajectory::new(trajectory.clone())?;
    let motors = config
        .independent_coordinates
        .iter()
        .map(|name| EffectiveServo::new(&config.actuators[name]))
        .collect::<Result<Vec<_>, _>>()?;
    let mut velocity_offsets = vec![];
    let mut torque_offsets = vec![];
    for (k, frame) in report.frames.iter().enumerate() {
        let sample = curve.sample((k as f64 + 0.5) * config.step_s)?;
        let mut velocity = vec![];
        let mut torque = vec![];
        for (j, motor) in motors.iter().enumerate() {
            if (sample.rates[j] - frame.velocity[6 + j]).abs() > 1e-10 {
                return Err("joint interpolation rate mismatch".into());
            }
            velocity.push(motor.reference_target(0., sample.rates[j], 0.)?);
            torque.push(motor.reference_target(0., 0., frame.motor_torques_nm[j])?);
        }
        velocity_offsets.push(velocity);
        torque_offsets.push(torque);
    }
    let base = session.robot.art.bases[0].state;
    let translation: Vec<_> = (0..3)
        .map(|i| positions[0][i] - seed.states[base + i])
        .collect();
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "expected_cad_sha256":config.expected_cad_sha256,"independent_coordinates":config.independent_coordinates,
            "step_s":config.step_s,"duration_s":(positions.len()-1)as f64*config.step_s,
            "trajectory":trajectory,"velocity_offsets":velocity_offsets,"torque_offsets":torque_offsets,
            "initial_coordinates":positions[0][6..],"initial_base_translation_m":translation,
            "initial_base_rotation_vector_rad":positions[0][3..6],"planning":report,"geometry":geometry,
            "scope":"Finite-horizon diagnostic only: piecewise linear joint positions, interval-held planned inverse torque and matching velocity feedforward through unchanged effective servo gains/envelopes. Coarse implicit endpoint dynamics do not certify interpolation or detailed runtime tracking. No loop, endpoint hold, stopping, steering or browser gait claim."
        }))?
    );
    Ok(())
}
