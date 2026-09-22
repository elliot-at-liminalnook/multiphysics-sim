//! Rate study through the existing Rust actuator bench and integer controller.
//! Same piecewise-linear reference, plant, drive cap, duration, and score grid.
use serde_json::json;
use sim_domain_control::fixed_pd::Gains;
use sim_runtime::controller_refinement::tracking;
use std::{fs, path::Path, sync::atomic::AtomicBool};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("compare_control_cadence ARCHIVED_CAMPAIGN NEW_OUTPUT".into());
    }
    let input = Path::new(&args[1]);
    let out = Path::new(&args[2]);
    fs::create_dir(out)?;
    let mut results = Vec::new();
    let cancel = AtomicBool::new(false);
    for id in [10, 11, 12] {
        for pattern in ["gait", "reversal", "full-gait"] {
            let bytes =
                fs::read(input.join(format!("id{id}-{pattern}-nominal-selected-1000.json")))?;
            let archived: tracking::ResultTrace = serde_json::from_slice(&bytes)?;
            let source_hash = blake3::hash(&bytes).to_hex().to_string();
            for family in ["fixed-original", "period-compensated"] {
                for ratio in [1usize, 2, 4] {
                    let mut e = archived.experiment.clone();
                    if (e.period_s - 0.01).abs() > 1e-12 {
                        return Err("Expected 100 Hz source reference".into());
                    }
                    let reference = e.targets_counts.clone();
                    e.name = format!("id{id}-{pattern}-{family}-{}hz", 100 * ratio);
                    e.period_s = 0.01 / ratio as f64;
                    // Explicit common assumption, not a claim about internal sensor age.
                    e.latency_s = 0.00125;
                    e.gains = if family == "fixed-original" {
                        Gains {
                            kp_q8: 4096,
                            kd_q8: 0,
                            kv_q8: 4096,
                            limit: 1000,
                        }
                    } else {
                        Gains {
                            kp_q8: 4096,
                            kd_q8: (1024 * ratio) as u16,
                            kv_q8: (1024 * ratio) as u16,
                            limit: 1000,
                        }
                    };
                    e.targets_counts = (0..reference.len() * ratio)
                        .map(|i| {
                            let j = i / ratio;
                            let t = (i % ratio) as f64 / ratio as f64;
                            (reference[j] as f64 * (1. - t)
                                + reference[(j + 1).min(reference.len() - 1)] as f64 * t)
                                .round() as i16
                        })
                        .collect();
                    let r = tracking::simulate(&e, &cancel)?;
                    let score = |trace: &tracking::ResultTrace| {
                        let sum = trace
                            .samples
                            .iter()
                            .step_by(ratio)
                            .zip(&reference)
                            .map(|(s, t)| {
                                (s.angle_rad.to_degrees() - *t as f64 * 360. / 4096.).powi(2)
                            })
                            .sum::<f64>();
                        (sum / reference.len() as f64).sqrt()
                    };
                    let mut row = json!({"id":id,"pattern":pattern,"controller_family":family,"rate_hz":100*ratio,
                "source_blake3":source_hash,"gains":e.gains,"sample_to_apply_assumption_s":e.latency_s,
                "common_100hz_grid_rms_degrees":score(&r),"metrics_at_control_rate":r.metrics,
                "simulation_only":true});
                    // Selected comparisons verify the integration timestep separately.
                    if family == "period-compensated" && pattern == "gait" {
                        let mut fine = e.clone();
                        fine.model.step_s *= 0.5;
                        let f = tracking::simulate(&fine, &cancel)?;
                        let delta = (score(&r) - score(&f)).abs();
                        row["timestep_rms_delta_degrees"] = json!(delta);
                        row["timestep_tolerance_degrees"] = json!(0.1);
                        if delta > 0.1 {
                            return Err(format!("Timestep sensitivity: {} {delta}", e.name).into());
                        }
                    }
                    fs::write(
                        out.join(format!("{}.json", e.name)),
                        serde_json::to_vec(&r)?,
                    )?;
                    results.push(row);
                    fs::write(
                        out.join("summary.json"),
                        serde_json::to_vec_pretty(&results)?,
                    )?;
                    eprintln!("{} common-grid RMS {:.4}", e.name, score(&r));
                }
            }
        }
    }
    Ok(())
}
