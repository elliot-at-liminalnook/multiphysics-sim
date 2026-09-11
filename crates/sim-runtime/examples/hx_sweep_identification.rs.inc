// Adapter from supervised acquisition records to the shared empirical fitter.
// Baselines come from verified telemetry immediately before each command.
fn load_sweep(root: &Path, name: &str) -> Result<Vec<Trial>, E> {
    use sim_runtime::acquisition::servo_bus::{Telemetry, reply};
    let p = root.join(name);
    let run = json_file(p.join("run.json"))?;
    if run["result"]["fpga_stop"]["Ok"]["latched"] != true
        || !run["result"]["recovery"].as_object().is_some_and(|r| {
            !r.is_empty()
                && r.values()
                    .all(|s| s["final_state"]["Ok"]["telemetry"]["speed_raw"] == 0)
        })
    {
        return Err("sweep requires completed cleanup and measured final stops".into());
    }
    let checkpoint = json_file(p.join("checkpoint.json"))?;
    let outcomes = checkpoint["outcomes_this_run"]
        .as_array()
        .ok_or("missing completed outcomes")?;
    let transactions: Vec<Value> = fs::read_to_string(p.join("transactions.jsonl"))?
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    let content = fs::read_to_string(p.join("sweep.csv"))?;
    let mut lines = content.lines();
    let header: Vec<_> = lines
        .next()
        .ok_or("missing sweep header")?
        .split(',')
        .collect();
    let mut groups = BTreeMap::<(usize, u8), Vec<Row>>::new();
    for line in lines {
        let values: Vec<_> = line.split(',').collect();
        if values.len() != header.len() {
            return Err("partial sweep row".into());
        }
        let n = |key: &str| -> Result<f64, E> {
            let x: f64 = values[header
                .iter()
                .position(|h| *h == key)
                .ok_or("missing sweep column")?]
            .parse()?;
            if !x.is_finite() {
                return Err("nonfinite sweep observation".into());
            }
            Ok(x)
        };
        let stage = n("trial")? as usize;
        // Only fully completed, stationary-verified pulse trials enter the fit.
        if !outcomes
            .iter()
            .any(|o| o["trial"]["index"] == stage && o["stationary_verified"] == true)
        {
            continue;
        }
        if n("status")? != 0. {
            return Err("faulted completed sweep observation".into());
        }
        let row = Row {
            id: n("id")? as u8,
            stage,
            request: n("request_s")?,
            completion: n("completion_s")?,
            command: 0.,
            // Continuous sweeps reset the unwrapped origin to the raw
            // stationary baseline at each trial. Never fit the wrap jump as
            // physical motion; legacy bounded sweeps retain their raw column.
            position: n(if header.contains(&"position_unwrapped_counts") {
                "position_unwrapped_counts"
            } else {
                "position_raw"
            })?,
            speed: n("speed_rad_s")?,
            voltage: n("voltage_v")?,
            temperature: n("temperature_c")?,
        };
        if row.request > row.completion {
            return Err("reversed sample window".into());
        }
        groups.entry((stage, row.id)).or_default().push(row);
    }
    let mut result = Vec::new();
    for ((stage, id), rows) in groups {
        let o = outcomes
            .iter()
            .find(|o| o["trial"]["index"] == stage)
            .ok_or("no outcome")?;
        if o["trial"]["segments"]
            .as_array()
            .is_some_and(|s| !s.is_empty())
        {
            return Err("step fitter does not accept arbitrary waveforms".into());
        }
        let number = |v: &Value| -> Result<f64, E> {
            let x = v.as_f64().ok_or("missing timing")?;
            if !x.is_finite() {
                return Err("nonfinite timing".into());
            }
            Ok(x)
        };
        let on_window = [
            number(&o["segment_events"][0]["request_s"])?,
            number(&o["segment_events"][0]["receipt_s"])?,
        ];
        let off_window = [
            number(&o["zero_request_s"])?,
            number(&o["zero_bridge_receipt_s"])?,
        ];
        let on = (on_window[0] + on_window[1]) / 2.;
        let off = (off_window[0] + off_window[1]) / 2.;
        if on_window[0] > on_window[1] || off_window[0] > off_window[1] || off <= on {
            return Err("invalid pulse windows".into());
        }
        let before = transactions
            .iter()
            .rev()
            .find(|t| {
                t["id"] == id
                    && t["instruction"] == 2
                    && t["tx"][5] == 56
                    && t["tx"][6] == 15
                    && t["completion_host_ns"]
                        .as_f64()
                        .is_some_and(|s| s * 1e-9 <= on_window[0])
            })
            .ok_or("no measured pre-pulse baseline")?;
        let bytes: Vec<u8> = serde_json::from_value(before["rx"].clone())?;
        let response = reply(&bytes, id, 15)?;
        let baseline = Telemetry::decode(&response.parameters)?;
        if response.error != 0 || baseline.speed_raw != 0 || baseline.status != 0 {
            return Err("baseline not healthy and stationary".into());
        }
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
                    (r.position - f64::from(baseline.position_raw)) * QUANTUM,
                    QUANTUM + peak * (r.completion - r.request) / 2.,
                ]
            })
            .collect();
        let windows = retained
            .iter()
            .map(|r| [r.request - on, r.completion - on])
            .collect();
        let range = |get: fn(&Row) -> f64| {
            [
                rows.iter().map(get).fold(f64::INFINITY, f64::min),
                rows.iter().map(get).fold(f64::NEG_INFINITY, f64::max),
            ]
        };
        result.push(Trial {
            run: name.into(),
            id,
            stage,
            kind: if o["trial"]["ids"].as_array().ok_or("missing IDs")?.len() > 1 {
                "sweep_concurrent"
            } else {
                "sweep_individual"
            }
            .into(),
            observation: PulseObservation {
                input: number(&o["trial"]["drive"])? / 1000.,
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
    Ok(result)
}
