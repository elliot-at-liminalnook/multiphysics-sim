//! Compare slipping contact with unloaded swing using the shared residual.
use sim_domain_control::contact_slip::{ContactSlip, ContactSlipConfig};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ContactSlipConfig {
        duration_s: 1.,
        displacement_m: 0.2,
        load_threshold_n: 1.,
    };
    let component = ContactSlip::new(config.clone())?;
    let mut cases = Vec::new();
    for (name, observations) in [
        (
            "constant_loaded_sliding",
            [(0.5, 20., [0.2, 0.]), (0.5, 20., [0.2, 0.])],
        ),
        (
            "stationary_contact_and_unloaded_swing",
            [(0.5, 20., [0., 0.]), (0.5, 0., [1., 0.])],
        ),
    ] {
        let mut squared = 0.;
        let mut mean_path = 0.;
        for (dt, force, velocity) in observations {
            let residual = component.sample(dt, force, force, velocity)?;
            squared += residual.iter().map(|v| v * v).sum::<f64>();
            mean_path += component.mean_path_sample(dt, force, force, velocity)?;
        }
        cases.push(serde_json::json!({"case":name,"sampled_slip_upper_bound":squared.sqrt(),
            "continuous_mean_slip_upper_bound":mean_path/config.displacement_m}));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "config":config,"cases":cases,
            "scope":"Declared contact observations illustrate the residual only; no robot simulation or gait acceptance is claimed."
        }))?
    );
    Ok(())
}
