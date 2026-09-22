//! Reproducible offline search; never opens a serial device or modifies CAD.
use serde_json::json;
use sim_domain_control::fixed_pd::Gains;
use sim_runtime::controller_refinement::{
    calibration::Family,
    tracking::{self, Experiment},
    trajectory_binding::ReferenceTrace,
};
use std::{fs, path::Path, sync::atomic::AtomicBool};

fn save(path: impl AsRef<Path>, value: &impl serde::Serialize) {
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}
fn pattern(kind: &str, period: f64, seconds: f64, axis: usize, trace: &ReferenceTrace) -> Vec<i16> {
    (0..(seconds / period).round() as usize)
        .map(|i| {
            let t = i as f64 * period;
            let value = match kind {
                "train" => {
                    if t < 0.2 {
                        0.
                    } else if t < 0.6 {
                        40. * (t - 0.2) / 0.4
                    } else if t < 1.0 {
                        40.
                    } else if t < 1.5 {
                        40. - 120. * (t - 1.)
                    } else if t < 2.0 {
                        -20.
                    } else if t < 2.8 {
                        25. * (std::f64::consts::TAU * 2. * (t - 2.)).sin()
                    } else {
                        0.
                    }
                }
                "hold" => {
                    if t < 0.3 {
                        0.
                    } else if t < 0.35 {
                        64. * (t - 0.3) / 0.05
                    } else {
                        64.
                    }
                }
                "reversal" => {
                    if t < 0.2 || t > seconds - 0.7 {
                        0.
                    } else {
                        64. * (std::f64::consts::TAU * 2.7 * (t - 0.2)).sin()
                    }
                }
                "chirp" => {
                    if t > seconds - 0.7 {
                        0.
                    } else {
                        50. * (t / 0.4).min(1.)
                            * (std::f64::consts::TAU * (0.3 * t + 0.6 * t * t)).sin()
                    }
                }
                "gait" | "full-gait" => {
                    let source_t = (t - 0.5).max(0.).min(*trace.times_s.last().unwrap());
                    let b = trace
                        .times_s
                        .partition_point(|v| *v < source_t)
                        .min(trace.times_s.len() - 1);
                    let a = b.saturating_sub(1);
                    let f = if a == b {
                        0.
                    } else {
                        (source_t - trace.times_s[a]) / (trace.times_s[b] - trace.times_s[a])
                    };
                    let angle = trace.targets_rad[a][axis]
                        + f * (trace.targets_rad[b][axis] - trace.targets_rad[a][axis])
                        - trace.targets_rad[0][axis];
                    angle * 4096. / std::f64::consts::TAU * if kind == "gait" { 0.09 } else { 1. }
                }
                _ => unreachable!(),
            };
            value.round() as i16
        })
        .collect()
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if !(5..=6).contains(&args.len()) {
        return Err(
            "tune_motor_tracking BASELINE_FAMILY CANDIDATE_FAMILY REFERENCE_TRACE NEW_OUTPUT_DIR [LIMIT_COUNTS]"
                .into(),
        );
    }
    let baseline: Family = serde_json::from_slice(&fs::read(&args[1])?)?;
    let candidate: Family = serde_json::from_slice(&fs::read(&args[2])?)?;
    let reference: ReferenceTrace = serde_json::from_slice(&fs::read(&args[3])?)?;
    reference.validate()?;
    let out = Path::new(&args[4]);
    let inputs = json!({"baseline":baseline,"candidate":candidate,"reference":reference,
        "format_version":2,"runtime":sim_runtime::physics_context::RuntimeIdentity::current(),
        "search_source_blake3":blake3::hash(include_bytes!("tune_motor_tracking.rs")).to_hex().to_string(),
        "search":"Deterministic coordinate descent over FPGA-supported gains, worst model RMS plus 0.15 peak degrees. Training waveform only. No plant fitting.",
        "model_warning":"Both families are provisional and fail historical physical prediction acceptance. Per-device fits are uncertainty scenarios, not accepted calibrations."});
    if out.exists() {
        let previous: serde_json::Value =
            serde_json::from_slice(&fs::read(out.join("inputs.json"))?)?;
        if previous != inputs || out.join("validation.json").exists() {
            return Err("Can only resume an unfinished search with identical frozen inputs".into());
        }
    } else {
        fs::create_dir(out)?;
        save(out.join("inputs.json"), &inputs);
    }
    let models = vec![
        ("baseline", baseline.shared.clone()),
        ("id10", candidate.model(10)?),
        ("id11", candidate.model(11)?),
        ("id12", candidate.model(12)?),
    ];
    let base = Experiment {
        name: "training".into(),
        model: baseline.shared.clone(),
        gains: Gains {
            kp_q8: 4096,
            kd_q8: 0,
            kv_q8: 4096,
            limit: 100,
        },
        targets_counts: pattern("train", 0.01, 3.4, 0, &reference),
        period_s: 0.01,
        latency_s: 0.002,
        voltage_v: 11.8,
        temperature_c: 45.,
    };
    let cancel = AtomicBool::new(false);
    let mut trials: Vec<serde_json::Value> = if out.join("search.json").exists() {
        serde_json::from_slice(&fs::read(out.join("search.json"))?)?
    } else {
        Vec::new()
    };
    let mut bests = Vec::new();
    let limits = if args.len() == 6 {
        vec![args[5].parse::<u16>()?]
    } else {
        vec![100, 350]
    };
    for limit in limits {
        let mut cache = std::collections::BTreeMap::new();
        for trial in &trials {
            let g: Gains = serde_json::from_value(trial["gains"].clone())?;
            if g.limit == limit {
                cache.insert(
                    (g.kp_q8, g.kd_q8, g.kv_q8),
                    trial["training_score"]
                        .as_f64()
                        .ok_or("Invalid cached score")?,
                );
            }
        }
        let mut evaluate = |g: Gains| -> f64 {
            let key = (g.kp_q8, g.kd_q8, g.kv_q8);
            if let Some(score) = cache.get(&key) {
                return *score;
            }
            let mut worst = 0f64;
            let mut metrics = Vec::new();
            for (name, model) in &models {
                let mut e = base.clone();
                e.gains = g;
                e.model = model.clone();
                // Search at the model's authored timestep; convergence checked later.
                match tracking::simulate(&e, &cancel) {
                    Ok(r) => {
                        worst = worst.max(r.metrics.rms_degrees + 0.15 * r.metrics.peak_degrees);
                        metrics.push(json!({"model":name,"metrics":r.metrics}));
                    }
                    Err(error) => {
                        worst = 1e6;
                        metrics.push(json!({"model":name,"failure":error}));
                    }
                }
            }
            eprintln!(
                "candidate limit={} kp={} kd={} kv={} score={:.4}",
                g.limit, g.kp_q8, g.kd_q8, g.kv_q8, worst
            );
            trials.push(json!({"gains":g,"training_score":worst,"models":metrics}));
            save(out.join("search.json"), &trials);
            cache.insert(key, worst);
            worst
        };
        let original = Gains {
            limit,
            ..base.gains
        };
        let mut best = original;
        let mut score = evaluate(best);
        // Multiple starts reduce coordinate-descent dependence on the original saturated controller.
        for seed in [
            original,
            Gains {
                kp_q8: 1024,
                kd_q8: 2048,
                kv_q8: 0,
                limit,
            },
        ] {
            let mut current = seed;
            let mut current_score = evaluate(current);
            for _ in 0..3 {
                let before = current_score;
                for coordinate in 0..3 {
                    let start = current;
                    for value in [0, 64, 128, 256, 512, 1024, 2048, 4096] {
                        let mut g = start;
                        match coordinate {
                            0 => g.kp_q8 = value,
                            1 => g.kd_q8 = value,
                            _ => g.kv_q8 = value,
                        };
                        let s = evaluate(g);
                        if s < current_score {
                            current = g;
                            current_score = s;
                        }
                    }
                }
                if current_score >= before {
                    break;
                }
            }
            if current_score < score {
                best = current;
                score = current_score;
            }
        }
        bests.push(best);
        save(out.join("selected-gains.json"), &bests);
    }
    let mut summary = Vec::new();
    for best in bests {
        for (model_name, model) in &models {
            for kind in ["hold", "reversal", "chirp", "gait", "full-gait"] {
                for (condition, voltage, temperature, latency, inertia) in [
                    ("nominal", 11.8, 45., 0.002, 1.),
                    ("low-voltage-delay-load", 10.4, 55., 0.008, 5.),
                ] {
                    for (label, gains) in [
                        (
                            "original",
                            Gains {
                                limit: best.limit,
                                ..base.gains
                            },
                        ),
                        ("selected", best),
                    ] {
                        let mut e = base.clone();
                        e.name = format!("{model_name}-{kind}-{condition}-{label}-{}", gains.limit);
                        e.model = model.clone();
                        e.model.conditions.load_inertia *= inertia;
                        e.gains = gains;
                        e.voltage_v = voltage;
                        e.temperature_c = temperature;
                        e.latency_s = latency;
                        let axis = match *model_name {
                            "id11" => 1,
                            "id12" => 2,
                            _ => 0,
                        };
                        e.targets_counts = pattern(
                            kind,
                            0.01,
                            if kind.contains("gait") { 10.5 } else { 4. },
                            axis,
                            &reference,
                        );
                        match tracking::simulate(&e,&cancel) {
                            Ok(r)=>{
                                let file=format!("{}.json",e.name);save(out.join(&file),&r);
                                summary.push(json!({"name":e.name,"model":model_name,"pattern":kind,"condition":condition,"controller":label,"limit":gains.limit,"metrics":r.metrics,"trace":file}));
                            },
                            Err(error)=>summary.push(json!({"name":e.name,"model":model_name,"pattern":kind,"condition":condition,"controller":label,"limit":gains.limit,"failure":error})),
                        }
                        save(out.join("validation.json"), &summary);
                        eprintln!("validated {}", e.name);
                    }
                }
            }
        }
    }
    Ok(())
}
