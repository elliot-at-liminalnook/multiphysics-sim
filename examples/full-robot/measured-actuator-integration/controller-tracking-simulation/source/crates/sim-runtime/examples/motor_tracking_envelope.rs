//! Trackability versus reference constraints. Never hides the original request error.
use serde_json::json;
use sim_domain_control::reference_governor::{Config, State};
use sim_runtime::controller_refinement::tracking;
use std::{fs, path::Path, sync::atomic::AtomicBool};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 2 {
        return Err("motor_tracking_envelope FULL_DRIVE_CAMPAIGN".into());
    }
    let dir = Path::new(&args[1]);
    let out = dir.join("reference-envelope");
    fs::create_dir(&out)?;
    let mut summary = Vec::new();
    let cancel = AtomicBool::new(false);
    for name in ["id10", "id11", "id12"] {
        for speed in [20f64, 40., 80., 140., 220.] {
            let source: tracking::ResultTrace = serde_json::from_slice(&fs::read(
                dir.join(format!("{name}-full-gait-nominal-selected-1000.json")),
            )?)?;
            let mut e = source.experiment;
            let original = e.targets_counts.clone();
            let config = Config {
                period_s: e.period_s,
                maximum_speed_rad_s: speed.to_radians(),
                maximum_acceleration_rad_s2: (speed * 5.).to_radians(),
                response_rate_per_s: 20.,
            };
            let mut state = State::default();
            let mut continuous = Vec::new();
            for (target, desired) in e.targets_counts.iter_mut().zip(&original) {
                state = config.update(state, *desired as f64 * std::f64::consts::TAU / 4096.)?;
                *target = (state.angle_rad * 4096. / std::f64::consts::TAU).round() as i16;
                continuous.push(state);
            }
            let r = tracking::simulate(&e, &cancel)?;
            let original_rms = (r
                .samples
                .iter()
                .zip(&original)
                .map(|(s, t)| ((s.encoder_counts - t) as f64).powi(2))
                .sum::<f64>()
                / original.len() as f64)
                .sqrt()
                * 360.
                / 4096.;
            let file = format!("{name}-{speed}.json");
            summary.push(json!({"model":name,"maximum_command_speed_degrees_s":speed,"maximum_command_acceleration_degrees_s2":speed*5.,"motor_tracking_command":r.metrics,"motor_error_to_original_rms_degrees":original_rms,"trace":file}));
            fs::write(
                out.join(file),
                serde_json::to_vec_pretty(
                    &json!({"config":config,"original_counts":original,"continuous_governor_states":continuous,"simulation":r}),
                )?,
            )?;
            fs::write(
                out.join("summary.json"),
                serde_json::to_vec_pretty(&summary)?,
            )?;
            eprintln!("{name} speed {speed} RMS {:.3}", r.metrics.rms_degrees);
        }
    }
    Ok(())
}
