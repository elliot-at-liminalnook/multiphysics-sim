//! Offline example: synthetic stopping observations, then a clearance-limited speed query.
//! Run: cargo run -p sim-domain-control --example adaptive_braking
//! This example opens no hardware and is not a fitted actuator model.
use sim_domain_control::adaptive_braking::{Config, Model};
fn main() -> Result<(), String> {
    let c = Config {
        bootstrap_speed_rad_s: 0.1,
        fallback_deceleration_rad_s2: 0.2,
        reaction_time_s: 0.2,
        position_uncertainty_rad: 0.001,
        boundary_margin_rad: 0.03,
        learning_inset_fraction: 0.25,
        minimum_stops: 3,
        braking_safety_factor: 0.5,
        trial_speed_growth: 1.25,
        learning_start_fraction: 0.25,
        maximum_evidence_age_s: 120.,
    };
    let mut model = Model::default();
    let mut time = 0.;
    for direction in [-1., 1.] {
        for _ in 0..3 {
            let (mut position, mut speed) = (5., 0.4f64);
            time += 0.02;
            model.constrain_observed_speed(speed)?;
            model.observe(&c, time, position, direction * speed, false, 0.2, [0., 10.])?;
            for _ in 0..200 {
                time += 0.02;
                model.observe(&c, time, position, direction * speed, true, 0.2, [0., 10.])?;
                speed = (speed - 0.02 * if direction > 0. { 0.8 } else { 0.4 }).max(0.);
                position += direction * speed * 0.02;
            }
        }
    }
    for position in [5., 9.95] {
        let e = model.envelope(&c, time, position, 0.4, [0., 10.], 1, 2., false)?;
        println!(
            "angle={position:.2} rad: permitted={:.3} rad/s; predicted stop={:.3} rad; brake={}",
            e.permitted_speed_rad_s, e.stopping_distance_rad, e.brake_now
        );
    }
    Ok(())
}
