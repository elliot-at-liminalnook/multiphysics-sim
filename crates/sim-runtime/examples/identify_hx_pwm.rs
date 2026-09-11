//! Offline encoder-trajectory identification. Never opens a serial device.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sim_solve::{
    least_squares::{LeastSquaresConfig, VariableBound},
    pulse_response::{PulseObservation, PulseResponse, fit_pulses},
};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};
type E = Box<dyn std::error::Error>;
const QUANTUM: f64 = std::f64::consts::TAU / 4096.;
#[derive(Clone)]
struct Row {
    id: u8,
    stage: usize,
    request: f64,
    completion: f64,
    command: f64,
    position: f64,
    speed: f64,
    voltage: f64,
    temperature: f64,
}
#[derive(Clone, Serialize)]
struct Trial {
    run: String,
    id: u8,
    stage: usize,
    kind: String,
    observation: PulseObservation,
    windows: Vec<[f64; 2]>,
    voltage_range_v: [f64; 2],
    temperature_range_c: [f64; 2],
    on_host_window_s: [f64; 2],
    off_host_window_s: [f64; 2],
}
fn json_file(p: impl AsRef<Path>) -> Result<Value, E> {
    Ok(serde_json::from_slice(&fs::read(p)?)?)
}
fn load(root: &Path, name: &str, kind: &str) -> Result<Vec<Trial>, E> {
    let p = root.join(name);
    let run = json_file(p.join("run.json"))?;
    let pre = json_file(p.join("preflight.json"))?;
    if run["completed"] != true {
        return Err(format!("incomplete run {name}").into());
    }
    let content = fs::read_to_string(p.join("pwm.csv"))?;
    let mut lines = content.lines();
    let header: Vec<_> = lines
        .next()
        .ok_or("missing CSV header")?
        .split(',')
        .collect();
    let mut groups = BTreeMap::<(u8, usize), Vec<Row>>::new();
    for line in lines {
        let values: Vec<_> = line.split(',').collect();
        let n = |key: &str| -> Result<f64, E> {
            Ok(values[*header
                .iter()
                .position(|h| *h == key)
                .as_ref()
                .ok_or("missing column")?]
            .parse()?)
        };
        if n("status")? != 0. {
            return Err("faulted observation in identification input".into());
        }
        let row = Row {
            id: n("id")? as u8,
            stage: n("stage")? as usize,
            request: n("request_s")?,
            completion: n("completion_s")?,
            command: n("command_start_s")?,
            position: n("position_raw")?,
            speed: n("speed_rad_s")?,
            voltage: n("voltage_v")?,
            temperature: n("temperature_c")?,
        };
        if row.request > row.completion {
            return Err("reversed observation window".into());
        }
        groups.entry((row.id, row.stage)).or_default().push(row);
    }
    let transactions: Vec<Value> = fs::read_to_string(p.join("transactions.jsonl"))?
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    let mut trials = vec![];
    for ((id, stage), rows) in groups {
        let home = pre["servos"][id.to_string()]["telemetry"]["position_raw"]
            .as_f64()
            .ok_or("missing home")?;
        let drive = run["plan"]["drive_counts"][stage]
            .as_i64()
            .ok_or("missing drive")? as f64
            / 1000.;
        let start = rows[0].command;
        let command = transactions
            .iter()
            .position(|t| {
                t["id"] == id
                    && t["instruction"] == 3
                    && t["tx"]
                        .as_array()
                        .is_some_and(|a| a.len() == 9 && a[5] == 44 && (a[6] != 0 || a[7] != 0))
                    && t["request_host_ns"]
                        .as_f64()
                        .is_some_and(|ns| ns * 1e-9 >= start - 1e-8 && ns * 1e-9 < start + 0.01)
            })
            .ok_or("no matching PWM command")?;
        let off = transactions[command + 1..]
            .iter()
            .find(|t| {
                t["id"] == id
                    && t["instruction"] == 3
                    && t["tx"]
                        .as_array()
                        .is_some_and(|a| a.len() == 9 && a[5] == 44 && a[6] == 0 && a[7] == 0)
            })
            .ok_or("missing zero command")?;
        for transaction in [&transactions[command], off] {
            let tx: Vec<u8> = serde_json::from_value(transaction["tx"].clone())?;
            let rx: Vec<u8> = serde_json::from_value(transaction["rx"].clone())?;
            if tx[2..].iter().fold(0u8, |a, b| a.wrapping_add(*b)) != 255
                || sim_runtime::acquisition::servo_bus::reply(&rx, id, 0)?.error != 0
            {
                return Err("unverified command transaction".into());
            }
        }
        let tx: Vec<u8> = serde_json::from_value(transactions[command]["tx"].clone())?;
        let expected = sim_runtime::acquisition::servo_bus::signed_pwm_write_parameters(
            (drive * 1000.).round() as i16,
            10,
        )?;
        if tx[5..8] != expected {
            return Err("command encoding disagrees with selected drive".into());
        }
        let window = |t: &Value| -> Result<[f64; 2], E> {
            Ok([
                t["request_host_ns"].as_f64().ok_or("missing request")? * 1e-9,
                t["completion_host_ns"]
                    .as_f64()
                    .ok_or("missing completion")?
                    * 1e-9,
            ])
        };
        let on_window = window(&transactions[command])?;
        let off_window = window(off)?;
        let on = (on_window[0] + on_window[1]) / 2.;
        let off = (off_window[0] + off_window[1]) / 2.;
        let peak = rows.iter().map(|r| r.speed.abs()).fold(0., f64::max);
        let retained: Vec<_> = rows
            .iter()
            .filter(|r| r.request < off + 0.12)
            .chain(rows.last())
            .collect();
        let samples = retained
            .iter()
            .map(|r| {
                [
                    (r.request + r.completion) / 2. - on,
                    (r.position - home) * QUANTUM,
                    QUANTUM + peak * (r.completion - r.request) / 2.,
                ]
            })
            .collect();
        let windows = retained
            .iter()
            .map(|r| [r.request - on, r.completion - on])
            .collect();
        let range = |get: fn(&Row) -> f64| -> [f64; 2] {
            [
                rows.iter().map(get).fold(f64::INFINITY, f64::min),
                rows.iter().map(get).fold(f64::NEG_INFINITY, f64::max),
            ]
        };
        trials.push(Trial {
            run: name.into(),
            id,
            stage,
            kind: kind.into(),
            observation: PulseObservation {
                input: drive,
                duration_s: off - on,
                samples,
            },
            windows,
            voltage_range_v: range(|r| r.voltage),
            temperature_range_c: range(|r| r.temperature),
            on_host_window_s: on_window,
            off_host_window_s: off_window,
        });
    }
    // Rebase later pulses on their preceding complete stage's final position.
    let mut positions = BTreeMap::<u8, f64>::new();
    for t in &mut trials {
        let initial = pre["servos"][t.id.to_string()]["telemetry"]["position_raw"]
            .as_f64()
            .unwrap()
            * QUANTUM;
        let prior = *positions.entry(t.id).or_insert(initial);
        for s in &mut t.observation.samples {
            s[1] += initial - prior;
        }
        positions.insert(t.id, prior + t.observation.samples.last().unwrap()[1]);
    }
    Ok(trials)
}
#[derive(Serialize, Deserialize)]
struct Metrics {
    samples: usize,
    rmse_encoder_counts: f64,
    maximum_error_encoder_counts: f64,
    final_displacement_error_counts: f64,
}
fn metrics(model: &PulseResponse, p: &PulseObservation) -> Result<Metrics, E> {
    let errors: Vec<f64> = p
        .samples
        .iter()
        .map(|s| Ok((model.predict(p.input, p.duration_s, s[0])?[0] - s[1]) / QUANTUM))
        .collect::<Result<_, E>>()?;
    Ok(Metrics {
        samples: errors.len(),
        rmse_encoder_counts: (errors.iter().map(|e| e * e).sum::<f64>() / errors.len() as f64)
            .sqrt(),
        maximum_error_encoder_counts: errors.iter().map(|e| e.abs()).fold(0., f64::max),
        final_displacement_error_counts: *errors.last().unwrap(),
    })
}
fn main() -> Result<(), E> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: identify_hx_pwm HARDWARE_ROOT NEW_OUTPUT".into());
    }
    let root = PathBuf::from(&args[0]);
    let out = PathBuf::from(&args[1]);
    fs::create_dir(&out)?;
    let names = [
        ("pwm-pilot-id12-v1", "pilot"),
        ("pwm-individual-ids4-11-v1", "pilot"),
        ("pwm-direction-bit10-id12-v1", "short_direction"),
        ("pwm-direction-bit10-ids4-11-v1", "short_direction"),
    ];
    let mut trials = vec![];
    let mut inputs = BTreeMap::new();
    for (name, kind) in names {
        trials.extend(load(&root, name, kind)?);
        for file in [
            "run.json",
            "preflight.json",
            "pwm.csv",
            "transactions.jsonl",
        ] {
            let path = root.join(name).join(file);
            inputs.insert(
                path.to_string_lossy().to_string(),
                blake3::hash(&fs::read(path)?).to_hex().to_string(),
            );
        }
    }
    fs::write(
        out.join("observations.json"),
        serde_json::to_vec_pretty(&trials)?,
    )?;
    let bounds: Vec<_> = [
        (0., 20.),
        (0., 0.08),
        (0.001, 0.2),
        (0.001, 0.2),
        (0., 0.04),
    ]
    .into_iter()
    .map(|(lower, upper)| VariableBound { lower, upper })
    .collect();
    let config = LeastSquaresConfig {
        maximum_iterations: 100,
        maximum_evaluations: 1200,
        difference_step: 1e-5,
        initial_damping: 0.001,
        gradient_tolerance: 1e-7,
    };
    let mut starts = vec![];
    for deadband in [0., 0.02, 0.05] {
        for tau in [0.01, 0.04, 0.1] {
            for delay in [0., 0.012, 0.03] {
                starts.push(vec![5., deadband, tau, 0.02, delay]);
            }
        }
    }
    let mut results = vec![];
    let mut predictions = fs::File::create(out.join("predictions.csv"))?;
    writeln!(
        predictions,
        "id,run,stage,split,drive,time_s,observed_displacement_rad,predicted_displacement_rad,predicted_speed_rad_s,weight_scale_rad"
    )?;
    for id in 4..=12 {
        let rows: Vec<_> = trials.iter().filter(|t| t.id == id).collect();
        let training = |t: &&Trial| t.kind == "pilot" && [0, 2, 4].contains(&t.stage);
        let train: Vec<_> = rows
            .iter()
            .copied()
            .filter(training)
            .map(|t| t.observation.clone())
            .collect();
        if train.len() != 3 {
            return Err("unexpected training partition".into());
        }
        let mut fit = fit_pulses(&train, &bounds, &starts, &config)?;
        let mut bound_refinement = Value::Null;
        if fit.termination != sim_solve::least_squares::Termination::Stationary {
            let mut fixed = bounds.clone();
            for (v, b) in fit.values.iter().zip(&mut fixed) {
                if (v - b.lower).abs() < 1e-5 * (b.upper - b.lower) {
                    b.upper = b.lower;
                } else if (v - b.upper).abs() < 1e-5 * (b.upper - b.lower) {
                    b.lower = b.upper;
                }
            }
            let seed: Vec<_> = fit
                .values
                .iter()
                .zip(&fixed)
                .map(|(v, b)| v.clamp(b.lower, b.upper))
                .collect();
            let conditional = fit_pulses(&train, &fixed, &[seed], &config)?;
            // Reopen all original bounds: conditional stationarity alone is
            // not a full constrained-optimum check.
            let reopened = fit_pulses(&train, &bounds, &[conditional.values.clone()], &config)?;
            bound_refinement = json!({"original_termination":fit.termination,"original_cost":fit.cost,
                "conditional":conditional,"reopened_termination":reopened.termination,"reopened_cost":reopened.cost});
            if reopened.cost <= fit.cost {
                fit = reopened;
            }
        }
        let model = PulseResponse::from_values(&fit.values)?;
        let mut evaluations = vec![];
        for t in &rows {
            let split = if training(t) {
                "train"
            } else if t.kind == "pilot" {
                "held_out_drive"
            } else {
                "held_out_duration_direction"
            };
            evaluations.push(json!({"run":t.run,"stage":t.stage,"split":split,"drive":t.observation.input,"duration_s":t.observation.duration_s,"metrics":metrics(&model,&t.observation)?,"voltage_range_v":t.voltage_range_v,"temperature_range_c":t.temperature_range_c}));
            for s in &t.observation.samples {
                let prediction =
                    model.predict(t.observation.input, t.observation.duration_s, s[0])?;
                writeln!(
                    predictions,
                    "{id},{},{},{split},{},{},{},{},{},{}",
                    t.run,
                    t.stage,
                    t.observation.input,
                    s[0],
                    s[1],
                    prediction[0],
                    prediction[1],
                    s[2]
                )?;
            }
        }
        let mut timing_sensitivity = vec![];
        for boundary in 0..2 {
            let perturbed: Vec<_> = rows
                .iter()
                .copied()
                .filter(training)
                .map(|t| {
                    let mut p = t.observation.clone();
                    for (s, w) in p.samples.iter_mut().zip(&t.windows) {
                        s[0] = w[boundary];
                    }
                    p
                })
                .collect();
            let f = fit_pulses(&perturbed, &bounds, &[fit.values.clone()], &config)?;
            timing_sensitivity.push(json!({"observation_assignment":if boundary==0{"request"}else{"completion"},"model":PulseResponse::from_values(&f.values)?,"cost":f.cost}));
        }
        let active_bounds: Vec<_> = fit
            .values
            .iter()
            .zip(&bounds)
            .enumerate()
            .filter(|(_, (v, b))| {
                (**v - b.lower).abs() < 1e-5 * (b.upper - b.lower)
                    || (**v - b.upper).abs() < 1e-5 * (b.upper - b.lower)
            })
            .map(|(i, _)| i)
            .collect();
        eprintln!(
            "ID {id}: gain {:.3}, deadband {:.4}, on/release lag {:.1}/{:.1} ms; cost {:.3}",
            model.gain,
            model.deadband,
            1000. * model.drive_tau_s,
            1000. * model.release_tau_s,
            fit.cost
        );
        results.push(json!({"id":id,"model":model,"fit":fit,"bound_refinement":bound_refinement,"active_parameter_bounds_indices":active_bounds,"evaluations":evaluations,"timing_sensitivity":timing_sensitivity}));
        fs::write(
            out.join("results.json"),
            serde_json::to_vec_pretty(
                &json!({"schema_version":1,"models":results,"bounds":bounds,"optimizer":config,"initial_points":starts,"source_blake3":blake3::hash(include_bytes!("identify_hx_pwm.rs")).to_hex().to_string(),"input_blake3":inputs,
                    "interpretation":"Empirical encoder-displacement fit only. Fits gain, deadband, on/release lags and combined delay; does not identify physical R/L/Ke/Kt/inertia/friction separately. No prediction beyond observed 2.5..20% drive is validated.",
                    "weights":"One encoder count plus observed peak speed times half the request/reply window; descriptive scale, not a calibrated noise standard deviation. Each pulse has equal aggregate weight.",
                    "split":"Train positive pilot 25/100/200. Hold out pilot 50/150 and separate +/-100 short pulses. Timing perturbation covers serialization window only, not unknown internal sample age.",
                    "acceptance":"Predeclared held-out encoder RMSE <=3 counts AND final displacement error <=5 counts per pulse; otherwise flag this model as insufficient. Not a physical calibration confidence interval."
                }),
            )?,
        )?;
    }
    Ok(())
}
