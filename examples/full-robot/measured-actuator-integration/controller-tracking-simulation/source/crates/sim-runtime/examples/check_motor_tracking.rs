//! Independent robustness checks on a frozen gain selection. No retuning here.
use serde_json::json;
use sim_domain_control::fixed_pd::Gains;
use sim_runtime::controller_refinement::{power, tracking};
use std::{fs, path::Path, sync::atomic::AtomicBool};
fn save(path: impl AsRef<Path>, value: &impl serde::Serialize) {
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 2 {
        return Err("check_motor_tracking CAMPAIGN_DIRECTORY".into());
    }
    let dir = Path::new(&args[1]);
    let out = dir.join("robustness");
    fs::create_dir(&out)?;
    let gains: Vec<Gains> = serde_json::from_slice(&fs::read(dir.join("selected-gains.json"))?)?;
    let cancel = AtomicBool::new(false);
    let mut summary = Vec::new();
    let trials: Vec<serde_json::Value> =
        serde_json::from_slice(&fs::read(dir.join("search.json"))?)?;
    for g in gains {
        for name in ["baseline", "id10", "id11", "id12"] {
            for pattern in ["hold", "reversal", "full-gait"] {
                let r: tracking::ResultTrace = serde_json::from_slice(&fs::read(dir.join(
                    format!("{name}-{pattern}-nominal-selected-{}.json", g.limit),
                ))?)?;
                if name == "id10" && pattern == "full-gait" {
                    let replay = tracking::simulate(&r.experiment, &cancel)?;
                    let exact =
                        serde_json::to_vec(&replay.samples)? == serde_json::to_vec(&r.samples)?;
                    summary.push(json!({"kind":"exact-replay","name":r.experiment.name,"samples_identical":exact}));
                    if !exact {
                        return Err("Saved controller trace did not replay exactly".into());
                    }
                }
                let mut e = r.experiment.clone();
                e.model.step_s *= 0.5;
                let fine = tracking::simulate(&e, &cancel)?;
                let max = fine
                    .samples
                    .iter()
                    .zip(&r.samples)
                    .map(|(a, b)| (a.angle_rad - b.angle_rad).abs().to_degrees())
                    .fold(0f64, f64::max);
                let rms_delta = (fine.metrics.rms_degrees - r.metrics.rms_degrees).abs();
                summary.push(json!({"kind":"timestep","name":r.experiment.name,"coarse_step":r.experiment.model.step_s,"fine_step":e.model.step_s,"peak_trace_difference_degrees":max,"rms_difference_degrees":rms_delta,"passes":rms_delta<=0.1,"tolerance_rms_degrees":0.1}));
                save(out.join(format!("{}-fine.json", e.name)), &fine);
            }
        }
        // Per-unit choice uses only the already frozen training search, never validation results.
        for name in ["id10", "id11", "id12"] {
            let selected = trials
                .iter()
                .filter(|t| t["gains"]["limit"].as_u64() == Some(g.limit as u64))
                .filter_map(|t| {
                    let m = t["models"]
                        .as_array()?
                        .iter()
                        .find(|m| m["model"].as_str() == Some(name))?;
                    Some((
                        m["metrics"]["rms_degrees"].as_f64()?
                            + 0.15 * m["metrics"]["peak_degrees"].as_f64()?,
                        t["gains"].clone(),
                    ))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .ok_or("No per-motor training result")?;
            let per_unit: Gains = serde_json::from_value(selected.1)?;
            for pattern in ["hold", "reversal", "chirp", "gait", "full-gait"] {
                let r: tracking::ResultTrace = serde_json::from_slice(&fs::read(dir.join(
                    format!("{name}-{pattern}-nominal-selected-{}.json", g.limit),
                ))?)?;
                let mut e = r.experiment;
                e.gains = per_unit;
                let result = tracking::simulate(&e, &cancel)?;
                summary.push(json!({"kind":"per-unit","model":name,"pattern":pattern,"gains":per_unit,"metrics":result.metrics,"shared_metrics":r.metrics}));
                save(
                    out.join(format!("{name}-{pattern}-per-unit-{}.json", g.limit)),
                    &result,
                );
            }
        }
        for name in ["id10", "id11", "id12"] {
            use sim_domain_control::reference_governor::{Config, State};
            let r: tracking::ResultTrace = serde_json::from_slice(&fs::read(dir.join(format!(
                "{name}-full-gait-nominal-selected-{}.json",
                g.limit
            )))?)?;
            let mut e = r.experiment;
            let original = e.targets_counts.clone();
            let speed: f64 = match g.limit {
                100 => 60.,
                350 => 160.,
                _ => 220.,
            };
            let config = Config {
                period_s: e.period_s,
                maximum_speed_rad_s: speed.to_radians(),
                maximum_acceleration_rad_s2: (speed * 10.).to_radians(),
                response_rate_per_s: 20.,
            };
            let mut state = State::default();
            for (target, desired) in e.targets_counts.iter_mut().zip(&original) {
                state = config.update(state, *desired as f64 * std::f64::consts::TAU / 4096.)?;
                *target = (state.angle_rad * 4096. / std::f64::consts::TAU).round() as i16;
            }
            let result = tracking::simulate(&e, &cancel)?;
            let original_rms = (result
                .samples
                .iter()
                .zip(&original)
                .map(|(s, t)| ((s.encoder_counts - t) as f64).powi(2))
                .sum::<f64>()
                / original.len() as f64)
                .sqrt()
                * 360.
                / 4096.;
            let reference_change_rms = (e
                .targets_counts
                .iter()
                .zip(&original)
                .map(|(a, b)| ((a - b) as f64).powi(2))
                .sum::<f64>()
                / original.len() as f64)
                .sqrt()
                * 360.
                / 4096.;
            summary.push(json!({"kind":"governed-reference","model":name,"limit":g.limit,"config":config,"motor_tracking_governed":result.metrics,"motor_error_to_original_rms_degrees":original_rms,"reference_change_rms_degrees":reference_change_rms}));
            save(
                out.join(format!("{name}-governed-{}.json", g.limit)),
                &json!({"config":config,"original_counts":original,"simulation":result,"warning":"Changes the requested gait. Motor tracking of this new reference is not tracking of the original gait, and does not establish foot placement or balance. Continuous limits apply before encoder quantization."}),
            );
        }
        for cadence in [0.01, 0.1] {
            for label in ["original", "selected"] {
                let r: tracking::ResultTrace = serde_json::from_slice(&fs::read(
                    dir.join(format!("id10-gait-nominal-{label}-{}.json", g.limit)),
                )?)?;
                let mut e = r.experiment;
                e.period_s = cadence;
                if cadence == 0.1 {
                    e.targets_counts = e.targets_counts.into_iter().step_by(10).collect();
                }
                let result = tracking::simulate(&e, &cancel)?;
                summary.push(json!({"kind":"cadence","controller":label,"limit":g.limit,"period_s":cadence,"metrics":result.metrics}));
                save(
                    out.join(format!("cadence-{label}-{}-{cadence}.json", g.limit)),
                    &result,
                );
            }
        }
        for count in [3, 9] {
            for shared_r in [0., 0.35] {
                let mut axes = Vec::new();
                for i in 0..count {
                    let r: tracking::ResultTrace =
                        serde_json::from_slice(&fs::read(dir.join(format!(
                            "id{}-reversal-nominal-selected-{}.json",
                            10 + i % 3,
                            g.limit
                        )))?)?;
                    let mut e = r.experiment;
                    e.targets_counts.truncate(160);
                    e.name = format!("scenario-motor-{i}");
                    axes.push(e);
                }
                let source = power::Setup {
                    source_component: sim_domain_electrical::elements::VOLTAGE_SOURCE.into(),
                    source_parameters: std::collections::BTreeMap::from([("voltage".into(), 11.8)]),
                    auxiliary_current_a: 0.2,
                    evidence: "Hypothetical supply, not measured wiring or current calibration"
                        .into(),
                    limits: Default::default(),
                };
                let result=tracking::simulate_group(&axes,source,shared_r,&vec![0.1;count],"Hypothetical 3/9 motor synchronous reversal; repeat the three provisional motor estimates. Not identities of physical motors 4-9.".into(),&cancel)?;
                let min_v = result
                    .supply_time_voltage_current_power
                    .iter()
                    .map(|s| s[1])
                    .fold(f64::INFINITY, f64::min);
                let max_i = result
                    .supply_time_voltage_current_power
                    .iter()
                    .map(|s| s[2])
                    .fold(0f64, f64::max);
                let max_p = result
                    .supply_time_voltage_current_power
                    .iter()
                    .map(|s| s[3])
                    .fold(0f64, f64::max);
                summary.push(json!({"kind":"shared-supply","motors":count,"limit":g.limit,"shared_resistance_ohm":shared_r,"branch_resistance_ohm":0.1,"min_bus_voltage_v":min_v,"peak_source_current_a_estimated":max_i,"peak_bus_power_w_estimated":max_p,"axes":result.axes.iter().map(|a|&a.metrics).collect::<Vec<_>>()}));
                save(
                    out.join(format!("group-{count}-{}-{shared_r}.json", g.limit)),
                    &result,
                );
                save(out.join("summary.json"), &summary);
                eprintln!("group {count} limit {} R {shared_r}", g.limit);
            }
        }
    }
    save(out.join("summary.json"), &summary);
    Ok(())
}
