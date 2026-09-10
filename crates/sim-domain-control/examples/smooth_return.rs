use sim_domain_control::smooth_return::SmoothReturn;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let curve = SmoothReturn::new(0.25)?;
    let samples = [0., 0.125, 0.25, 0.5, 0.75, 0.875, 1.].map(|phase|
        serde_json::json!({"phase":phase,"progress_and_phase_derivatives":curve.sample(phase).unwrap()}));
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({"ramp_fraction":0.25,
        "samples":samples,"peak_phase_rate":4./3.,"peak_phase_acceleration":8.,
        "scope":"Normalized geometric C2 return. Actual displacement/duration and actuator/contact constraints belong to the caller."}))?);
    Ok(())
}
