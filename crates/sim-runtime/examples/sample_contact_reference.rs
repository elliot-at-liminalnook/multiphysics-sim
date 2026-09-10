//! Sample existing references as numerical initial guesses, never contact constraints.
use serde_json::json;
use sim_domain_control::{
    contact_phase::{ContactPhaseConfig, ContactPhaseMotion},
    trajectory::{Trajectory, TrajectoryConfig},
};
use sim_domain_robot::math::{V, rotation_vector_motion};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    if a.len() != 4 {
        return Err(
            "usage: sample_contact_reference compiled.json start_s step_s intervals".into(),
        );
    }
    let j: serde_json::Value = serde_json::from_slice(&std::fs::read(&a[0])?)?;
    let start: f64 = a[1].parse()?;
    let step: f64 = a[2].parse()?;
    let count: usize = a[3].parse()?;
    if !start.is_finite() || !step.is_finite() || step <= 0. || count < 2 || count > 1000 {
        return Err("finite start, positive step and 2..1000 intervals required".into());
    }
    let config: ContactPhaseConfig = serde_json::from_value(j["motion"].clone())?;
    let motion = ContactPhaseMotion::new(config.clone())?;
    let trajectory = Trajectory::new(serde_json::from_value::<TrajectoryConfig>(
        j["trajectory"].clone(),
    )?)?;
    let offset = j["phase_offset_s"].as_f64().ok_or("missing offset")?;
    let origin: Vec<f64> =
        serde_json::from_value(j["recipe"]["initial_base_translation_m"].clone())?;
    if origin.len() != 3 {
        return Err("three base origin values required".into());
    }
    let mut positions = vec![];
    let mut initial_velocity: Vec<f64> = vec![];
    for k in 0..=count {
        let t = start + k as f64 * step;
        let body = motion.sample(t)?.body;
        let joints = trajectory.sample((t - offset).rem_euclid(config.period_s))?;
        let mut q = body.values.clone();
        for i in 0..3 {
            q[i] += origin[i];
        }
        q.extend(&joints.values);
        positions.push(q);
        if k == 0 {
            let angular = rotation_vector_motion(
                V::from_column_slice(&body.values[3..6]),
                V::from_column_slice(&body.rates[3..6]),
                V::zeros(),
            )?
            .0;
            initial_velocity.extend(&body.rates[..3]);
            initial_velocity.extend(angular.iter());
            initial_velocity.extend(&joints.rates);
        }
    }
    println!(
        "{}",
        serde_json::to_string(
            &json!({"positions":positions,"initial_velocity":initial_velocity,"step_s":step,
        "source":a[0],"start_s":start,"scope":"Shared Rust reference sampling used only to initialize a position-only search. Foot support schedules are not exported or enforced."})
        )?
    );
    Ok(())
}
