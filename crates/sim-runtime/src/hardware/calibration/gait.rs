use super::*;

/// A compiled gait inside the repository examples, never an arbitrary file.
pub(super) fn gait_file(rel: &str) -> R<std::path::PathBuf> {
    let root = repo_root()
        .join("examples")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let file = repo_root()
        .join(rel)
        .canonicalize()
        .map_err(|e| format!("{rel}: {e}"))?;
    if !file.starts_with(&root)
        || file.file_name().and_then(|n| n.to_str()) != Some("compiled.json")
    {
        return Err("Gaits are compiled.json files inside examples/".into());
    }
    Ok(file)
}
/// Completed gait-search trials, best first, with their simulated result.
pub(super) fn gait_catalog() -> R<Value> {
    let base = repo_root().join("examples/full-robot/measured-actuator-integration");
    let mut rows = Vec::new();
    for study in fs::read_dir(&base).map_err(|e| e.to_string())?.flatten() {
        let comparison = study.path().join("comparison");
        let Ok(trials) = fs::read_dir(&comparison) else {
            continue;
        };
        let measured = comparison.join("actuator-provenance.json").exists();
        for t in trials.flatten() {
            let dir = t.path();
            let (Ok(trial), true) = (
                fs::read(dir.join("trial.json")),
                dir.join("compiled.json").exists(),
            ) else {
                continue;
            };
            let Ok(trial) = serde_json::from_slice::<Value>(&trial) else {
                continue;
            };
            let outcome = &trial["observation"]["outcome"];
            if outcome["status"] != "complete" {
                continue;
            }
            // Only gaits that passed the search's own gates (tracking, upright).
            let Ok(evaluation) = fs::read(dir.join("evaluation.json"))
                .map_err(|e| e.to_string())
                .and_then(|b| serde_json::from_slice::<Value>(&b).map_err(|e| e.to_string()))
            else {
                continue;
            };
            if evaluation["rejection_reasons"]
                .as_array()
                .is_none_or(|r| !r.is_empty())
            {
                continue;
            }
            let rel = dir
                .join("compiled.json")
                .strip_prefix(repo_root())
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            let rel = rel.trim_start_matches("./").to_string();
            rows.push(json!({
                "path": rel,
                "study": study.file_name().to_string_lossy(),
                "trial": t.file_name().to_string_lossy(),
                "objective": outcome["objective"],
                "speed_m_s": evaluation["eligible_speed_m_s"],
                "tracking_rms_rad_max": evaluation["tracking_rms_rad"].as_array().map(|v| v.iter().filter_map(|x| x.as_f64()).fold(0f64, f64::max)),
                "measured_actuators": measured,
                "values": trial["observation"]["values"],
            }));
        }
    }
    rows.sort_by(|a, b| {
        (
            b["measured_actuators"].as_bool(),
            b["speed_m_s"].as_f64().unwrap_or(f64::NEG_INFINITY),
        )
            .partial_cmp(&(
                a["measured_actuators"].as_bool(),
                a["speed_m_s"].as_f64().unwrap_or(f64::NEG_INFINITY),
            ))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut rows: Vec<Value> = rows.into_iter().take(60).collect();
    rows.extend(lab_catalog(&base));
    Ok(json!({"gaits": rows}))
}
/// Gait-lab results (`<lab>/results/<name>-<hash>/`): gaits that passed their
/// gates, fastest first, then pose sequences that passed their checks.
pub(super) fn lab_catalog(base: &std::path::Path) -> Vec<Value> {
    let (mut gaits, mut poses) = (Vec::new(), Vec::new());
    for lab in fs::read_dir(base).into_iter().flatten().flatten() {
        let Ok(results) = fs::read_dir(lab.path().join("results")) else {
            continue;
        };
        for r in results.flatten() {
            let dir = r.path();
            let Some(report) = fs::read_to_string(dir.join("report.yaml"))
                .ok()
                .and_then(|t| serde_norway::from_str::<Value>(&t).ok())
            else {
                continue;
            };
            if !dir.join("compiled.json").exists() {
                continue;
            }
            let rel = dir
                .join("compiled.json")
                .strip_prefix(repo_root())
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            let row = |kind: &str, name: &Value| {
                json!({
                    "path": rel.trim_start_matches("./"), "kind": kind, "study": lab.file_name().to_string_lossy(), "trial": name,
                    "speed_m_s": report["speed_m_s"], "measured_actuators": true, "summary": report["summary"],
                })
            };
            match (report["kind"].as_str(), report["status"].as_str()) {
                (Some("pose_sequence"), Some("ready")) => {
                    poses.push(row("pose_sequence", &report["sequence"]))
                }
                (None, Some("passed")) if dir.join("spec-identity.json").exists() => {
                    gaits.push(row("lab_gait", &report["gait"]))
                }
                _ => {}
            }
        }
    }
    gaits.sort_by(|a, b| {
        b["speed_m_s"]
            .as_f64()
            .partial_cmp(&a["speed_m_s"].as_f64())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    gaits.into_iter().chain(poses).collect()
}
/// A trial's compiled gait with the reference governor the simulation ran it
/// through attached (`playback_governor`; the rule is the shared
/// `gait_playback::compiled_with_governor`).
pub(super) fn gait_with_governor(rel: &str) -> R<Value> {
    Ok(crate::gait_playback::compiled_with_governor(&gait_file(rel)?)?.0)
}
/// Per-motor statistics of one leg gait run.
pub(super) fn gait_statistics(
    rows: &[Value],
    ids: &[u8],
    roles: &std::collections::BTreeMap<u8, String>,
    predicted: &std::collections::BTreeMap<u8, f64>,
    pwm_ceiling: f64,
) -> Value {
    let mut out = serde_json::Map::new();
    for id in ids {
        let r: Vec<&Value> = rows
            .iter()
            .filter(|x| x["id"] == *id && x["playing"] == true && x["actual"].is_number())
            .collect();
        let f = |x: &Value, k: &str| x[k].as_f64().unwrap_or(f64::NAN);
        let n = r.len().max(1) as f64;
        let err: Vec<f64> = r.iter().map(|x| f(x, "actual") - f(x, "command")).collect();
        let raw: Vec<f64> = r.iter().map(|x| f(x, "actual") - f(x, "desired")).collect();
        let rms = |v: &[f64]| (v.iter().map(|e| e * e).sum::<f64>() / v.len().max(1) as f64).sqrt();
        let peak = |v: &[f64]| v.iter().fold(0f64, |m, e| m.max(e.abs()));
        let pwm: Vec<f64> = r.iter().map(|x| f(x, "pwm").abs()).collect();
        let governed = r
            .iter()
            .filter(|x| (f(x, "command") - f(x, "desired")).abs() > 20.)
            .count() as f64
            / n;
        let clamped = r.iter().filter(|x| x["clamped"] == true).count() as f64 / n;
        // Measured acceleration over ~50 ms windows (belt-slip screen).
        let mut peak_acc = 0f64;
        for w in r.windows(3) {
            let dt = f(w[2], "wall_s") - f(w[0], "wall_s");
            if dt > 0.03 && dt < 0.2 {
                peak_acc = peak_acc.max(((f(w[2], "velocity") - f(w[0], "velocity")) / dt).abs());
            }
        }
        // Lag: shift (in samples) of the actual trace that best matches the command.
        let (cmd, act): (Vec<f64>, Vec<f64>) =
            r.iter().map(|x| (f(x, "command"), f(x, "actual"))).unzip();
        let lag = (0..20usize)
            .min_by(|a, b| {
                let e = |k: usize| {
                    cmd.iter()
                        .zip(act.iter().skip(k))
                        .map(|(c, a)| (a - c).powi(2))
                        .sum::<f64>()
                        / (cmd.len().saturating_sub(k)).max(1) as f64
                };
                e(*a).total_cmp(&e(*b))
            })
            .unwrap_or(0);
        let period = if r.len() > 1 {
            (f(r[r.len() - 1], "wall_s") - f(r[0], "wall_s")) / (r.len() - 1) as f64
        } else {
            0.
        };
        let volts: Vec<f64> = r
            .iter()
            .map(|x| f(x, "voltage_v"))
            .filter(|v| v.is_finite())
            .collect();
        let temps: Vec<f64> = r
            .iter()
            .map(|x| f(x, "temperature_c"))
            .filter(|v| v.is_finite())
            .collect();
        out.insert(id.to_string(), json!({
            "role": roles[id], "samples": r.len(),
            "tracking_rms_counts": rms(&err), "tracking_peak_counts": peak(&err),
            "tracking_rms_deg": rms(&err) * 360. / 4096., "tracking_peak_deg": peak(&err) * 360. / 4096.,
            "error_vs_raw_gait_rms_counts": rms(&raw),
            "simulated_tracking_rms_counts": predicted.get(id),
            "lag_s": lag as f64 * period,
            "mean_effort": pwm.iter().sum::<f64>() / n / 1000., "saturated_fraction": pwm.iter().filter(|p| **p >= 0.95 * pwm_ceiling).count() as f64 / n,
            "peak_measured_acceleration_counts_s2": peak_acc,
            "governor_limited_fraction": governed, "clamped_fraction": clamped,
            "minimum_voltage_v": volts.iter().cloned().fold(f64::INFINITY, f64::min),
            "maximum_temperature_c": temps.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        }));
    }
    Value::Object(out)
}
/// Recent leg gait runs (summaries only), newest first. Each row says whether
/// the run was simulated and which execution wrote it; a record written
/// before runs were labelled has no `simulated` field and is listed as not
/// simulated (its `execution` is null).
pub(super) fn gait_run_history(cfg: &Config) -> Value {
    let dir = cfg.output.join("gait-runs");
    let mut runs: Vec<(String, Value)> = fs::read_dir(&dir).ok().into_iter().flatten().flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let v: Value = serde_json::from_slice(&fs::read(e.path()).ok()?).ok()?;
            Some((name.clone(), json!({"file": name, "gait": v["gait"], "effort": v["effort"], "speed_scale": v["speed_scale"], "gait_time_s": v["gait_time_s"], "statistics": v["statistics"], "outcome": v["outcome"],
                "simulated": v["simulated"].as_bool().unwrap_or(false), "execution": v["execution"]})))
        }).collect();
    runs.sort_by(|a, b| b.0.cmp(&a.0));
    json!(runs.into_iter().take(12).map(|r| r.1).collect::<Vec<_>>())
}
/// Play a gait on the physical leg. Each bound motor follows the gait's
/// governed reference (the same shared Rust sampler and reference governor
/// the simulation used), tightened to `effort` × that motor's measured
/// capability from the accepted actuator registry, and for the belt to the
/// campaign plan's belt acceleration limit. Motion stays inside the taught
/// poses (the desired reference is clamped before the governor), goes
/// through the shared feedback controller and the FPGA window, needs a live
/// browser lease, and starts from the leg's measured pose. Every period is
/// recorded; per-motor statistics are saved with the run. On a virtual bench
/// the same loop runs against the bench's motor models; the status, log
/// events and record are labelled (`execution`, `simulated`) and the record
/// says what the bench cannot stand for (`virtual_limits`).
#[allow(clippy::too_many_arguments)]
pub(super) fn run_gait(
    app: &App,
    cfg: &Config,
    b: &mut CalibrationBus,
    cal: &Calibration,
    proven: &std::collections::BTreeSet<u8>,
    session: Option<&str>,
    client: &str,
    r: &Request,
    stop_epoch: u64,
    reply: impl FnOnce(&Value),
) -> R<String> {
    use crate::gait_playback::{Gait, GovernedGait, LegBinding};
    const RAD: f64 = std::f64::consts::TAU / 4096.;
    if !r.supported {
        return Err("Confirm the leg is suspended with clear space around every joint".into());
    }
    let effort = if r.effort > 0. {
        r.effort.clamp(0.05, 1.)
    } else {
        0.5
    };
    let compiled = gait_with_governor(&r.gait)?;
    let gait = Gait::from_compiled(&compiled, &r.gait)?;
    if r.bindings.is_empty() {
        return Err("Bind at least one motor to a CAD joint in the leg mirror".into());
    }
    let mut bindings = Vec::new();
    let mut axes = Vec::new();
    for g in &r.bindings {
        let a = usable(cal.axes.get(&g.id).ok_or("Unknown motor")?, session);
        let role = &cfg.roles[&g.id];
        if a.disabled {
            return Err(format!("{role} is disabled"));
        }
        if a.lower.is_none() || a.upper.is_none() {
            return Err(format!("Teach both poses of {role} first"));
        }
        if !proven.contains(&g.id) {
            return Err(format!(
                "{role}: watchdogs not proven; select a motor with hold enabled"
            ));
        }
        let reference = a
            .reference
            .ok_or(format!("{role}: save its sim alignment first"))?;
        if (reference < 0 || reference > 4095) && a.reference_session.as_deref() != session {
            return Err(format!(
                "{role}: its sim alignment is from an earlier encoder session; re-align"
            ));
        }
        // The saved alignment pose wins over the page's: the reference counts
        // were captured at that joint angle.
        let home_rad = a.reference_joint_rad.unwrap_or(g.home_rad);
        let binding = LegBinding {
            id: g.id,
            joint: g.joint.clone(),
            polarity: g.polarity,
            reference_counts: reference as f64,
            home_rad,
        };
        binding.validate()?;
        if gait.index(&g.joint).is_none() {
            return Err(format!("the gait has no joint {}", g.joint));
        }
        bindings.push(binding);
        axes.push(a);
    }
    // The gait must fit the taught poses as mapped: a wrong sign or alignment
    // puts it outside, and every target would be pinned at a pose.
    let window = |a: &AxisCalibration| {
        let (lo, hi) = a.encoder_bounds();
        (
            lo.unwrap().min(hi.unwrap()) as f64 + 6.,
            lo.unwrap().max(hi.unwrap()) as f64 - 6.,
        )
    };
    let mut misfit = Vec::new();
    for (bd, a) in bindings.iter().zip(&axes) {
        let (lo, hi) = window(a);
        let i = gait.index(&bd.joint).unwrap();
        let fit = |polarity: f64| -> R<f64> {
            let flipped = LegBinding {
                polarity,
                ..bd.clone()
            };
            let mut inside = 0;
            for k in 0..200 {
                let q = gait.sample(gait.info.period_s * k as f64 / 200.)?[i];
                inside += usize::from((lo..=hi).contains(&flipped.counts(q)));
            }
            Ok(inside as f64 / 200.)
        };
        let (here, flipped) = (fit(bd.polarity)?, fit(-bd.polarity)?);
        if here < 0.95 {
            let role = &cfg.roles[&bd.id];
            misfit.push(if flipped >= 0.95 {
                format!("{role}: only {:.0}% of the gait fits its taught poses with this direction, {:.0}% with the opposite; its mirror sign is probably reversed (flip +/− in the leg mirror, check with Q)", here * 100., flipped * 100.)
            } else {
                format!("{role}: only {:.0}% of the gait fits its taught poses ({:.0}% reversed); re-save its sim alignment at the CAD home pose or widen its poses", here * 100., flipped * 100.)
            });
        }
    }
    if !misfit.is_empty() {
        return Err(misfit.join("; "));
    }
    // Per-motor limits: effort × measured capability (accepted registry, at the
    // measured supply), belt acceleration from the campaign plan.
    let registry = crate::actuator_registry::Registry::load(
        &repo_root().join("examples/actuators/hx30hm/accepted/registry.json"),
    )?;
    let plan_limits: Value = cfg
        .campaign_plan
        .as_ref()
        .and_then(|p| fs::read(p).ok())
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .map(|v| v["limits"].clone())
        .unwrap_or(Value::Null);
    // A zero or non-finite reading is no supply (family_limits would divide by
    // it and govern every motor to zero speed); such readings are skipped.
    let measured_supply = app.state.lock().unwrap()["samples"]
        .as_object()
        .and_then(|m| {
            m.values()
                .filter_map(|t| t["voltage_v"].as_f64())
                .filter(|v| v.is_finite() && *v > 0.)
                .reduce(f64::min)
        });
    let supply = measured_supply.unwrap_or(12.0);
    let mut governed = GovernedGait::new(gait.clone());
    let mut limits = serde_json::Map::new();
    for (bd, a) in bindings.iter().zip(&axes) {
        let i = gait.index(&bd.joint).unwrap();
        let family_name = registry.role_family(&bd.joint)?.to_string();
        let family = &registry.families[&family_name];
        let (full_speed, measured_acc) = crate::actuator_registry::family_limits(family, supply)?;
        let role = &cfg.roles[&bd.id];
        let belt_acc = plan_limits[role.as_str()]["max_acceleration_counts_s2"]
            .as_f64()
            .map(|c| c * RAD);
        let speed = effort * full_speed;
        let acc = measured_acc
            .map(|m| effort * m)
            .unwrap_or(f64::INFINITY)
            .min(belt_acc.unwrap_or(f64::INFINITY));
        governed.limit(i, speed, acc)?;
        let (lo, hi) = window(a);
        governed.clamp(i, bd.joint_rad(lo), bd.joint_rad(hi));
        let c = governed.config(i).unwrap();
        limits.insert(bd.id.to_string(), json!({"role": role, "family": family_name, "family_hash": family.content_hash(),
            "motor_full_drive_speed_rad_s": full_speed, "motor_measured_acceleration_rad_s2": measured_acc, "belt_acceleration_limit_rad_s2": belt_acc,
            "governor_speed_rad_s": c.maximum_speed_rad_s, "governor_acceleration_rad_s2": c.maximum_acceleration_rad_s2,
            "governor_speed_counts_s": c.maximum_speed_rad_s / RAD, "governor_acceleration_counts_s2": c.maximum_acceleration_rad_s2 / RAD}));
    }
    // Simulated tracking of the same gait (the search's evaluation), for comparison.
    let evaluation: Value = fs::read(gait_file(&r.gait)?.with_file_name("evaluation.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(Value::Null);
    let predicted: std::collections::BTreeMap<u8, f64> = bindings
        .iter()
        .filter_map(|bd| {
            let i = gait.index(&bd.joint)?;
            Some((bd.id, evaluation["tracking_rms_rad"][i].as_f64()? / RAD))
        })
        .collect();
    let ids: Vec<u8> = bindings.iter().map(|b| b.id).collect();
    let speed_scale = if r.speed_scale > 0. {
        r.speed_scale.min(1.)
    } else {
        1.
    };
    let pwm_ceiling = r.drive_pwm.clamp(1, 1000) as f64;
    // Arm before publishing or replying, so a refusal reaches the client.
    app.arm(stop_epoch, false)?;
    {
        let mut s = app.state.lock().unwrap();
        s["busy"] = json!(true);
        s["gait"] = labelled(
            app,
            json!({"running": true, "phase": "approach", "gait": r.gait, "t": 0., "speed_scale": speed_scale, "effort": effort,
            "motor_ids": ids, "bindings": bindings.iter().map(|b| json!(b)).collect::<Vec<_>>(), "limits": limits,
            "gait_governor": gait.info.governor, "targets": {}, "errors": {}, "clamped": 0, "statistics": {}}),
        );
        s["gait_runs"] = gait_run_history(cfg);
        s["message"] = json!(
            "Moving the leg to the gait's first pose through the gait's governor. Z stops drive."
        );
        reply(&s);
    }
    *app.gait.lock().unwrap() = Some(GaitLease {
        owner: client.into(),
        speed_scale,
        playing: true,
        last_seen: Instant::now(),
    });
    let run_started = Instant::now();
    writeln_log(
        cfg,
        labelled(
            app,
            json!({"event": "gait_start", "gait": r.gait, "bindings": bindings, "speed_scale": speed_scale, "effort": effort, "limits": limits, "supply_v": supply, "supply_assumed": measured_supply.is_none()}),
        ),
    )?;
    let started = std::cell::Cell::new(false);
    let clock = std::cell::Cell::new((0f64, Instant::now()));
    let governed = std::cell::RefCell::new(governed);
    let positions = std::cell::RefCell::new(std::collections::BTreeMap::<u8, f64>::new());
    let commands =
        std::cell::RefCell::new(std::collections::BTreeMap::<u8, (f64, f64, f64, bool)>::new());
    let rows = std::cell::RefCell::new(Vec::<Value>::new());
    let clamped = std::cell::Cell::new(0u64);
    let tolerance = cfg.sweep_tuning.hold_deadband_counts.unwrap_or(8.).max(8.) + 4.;
    // Each motor's governed reference starts from its first reading in this session.
    let initialized = std::cell::RefCell::new(std::collections::BTreeSet::<u8>::new());
    let result = b.controlled_motion_multi(&ids, &axes, 20, &cfg.sweep_tuning, &app.cancel, false, r.drive_mode, || {
        if app.cancel.load(Ordering::SeqCst) || app.stop.load(Ordering::SeqCst) {
            return Ok(None);
        }
        let (scale, playing) = {
            let lock = app.gait.lock().unwrap();
            let lease = lock.as_ref().ok_or("Gait controls closed")?;
            if lease.last_seen.elapsed() > Duration::from_millis(1500) {
                return Err("Browser heartbeat lost; gait stopped".into());
            }
            (lease.speed_scale, lease.playing)
        };
        let (mut t, last) = clock.get();
        let now = Instant::now();
        let dt = (now - last).as_secs_f64().clamp(0.001, 0.2);
        if started.get() && playing {
            t += dt * scale;
        }
        clock.set((t, now));
        let mut g = governed.borrow_mut();
        for bd in &bindings {
            let first = if initialized.borrow().contains(&bd.id) { None } else { positions.borrow().get(&bd.id).copied() };
            if let Some(p) = first {
                g.start_from(gait.index(&bd.joint).unwrap(), bd.joint_rad(p));
                initialized.borrow_mut().insert(bd.id);
            }
        }
        if initialized.borrow().len() < bindings.len() {
            // First period: nothing read yet; hold until every motor has a reading.
            return Ok(Some(axes.iter().map(|a| (SweepInput { speed_counts_s: 60., pwm_limit: r.drive_pwm.clamp(1, 1000) }, MotionCommand::Hold, a.clone())).collect()));
        }
        let out = g.step(t, dt, scale)?;
        let desired = g.gait.sample(t)?;
        let desired_start = g.desired(0.)?;
        drop(g);
        // Start the clock once every motor has reached the governed first pose.
        if !started.get() {
            // Per motor: distance still to govern (reference to first pose),
            // and the motor's distance from the reference, in counts.
            let gaps: Vec<(u8, f64, f64)> = bindings.iter().map(|bd| {
                let i = gait.index(&bd.joint).unwrap();
                let goal = bd.counts(out[i].0);
                let first = bd.counts(desired_start[i]);
                (bd.id, (first - goal).abs(), positions.borrow().get(&bd.id).map_or(f64::INFINITY, |p| (p - goal).abs()))
            }).collect();
            if gaps.iter().all(|(_, left, off)| *left < 2. && *off < tolerance) {
                started.set(true);
            } else if run_started.elapsed() > Duration::from_secs(30) {
                return Err(format!("The leg did not reach the gait's first pose within 30 s ({})", gaps.iter().map(|(id, left, off)| format!("{}: reference {left:.0} counts from the first pose, motor {off:.0} counts from the reference", cfg.roles[id])).collect::<Vec<_>>().join("; ")));
            }
        }
        let mut plan = Vec::new();
        let mut shown = serde_json::Map::new();
        for (bd, a) in bindings.iter().zip(&axes) {
            let i = gait.index(&bd.joint).unwrap();
            let (q, v) = out[i];
            let goal = bd.counts(q);
            let raw = bd.counts(desired[i]);
            let (lo, hi) = window(a);
            let is_clamped = raw < lo || raw > hi;
            if is_clamped {
                clamped.set(clamped.get() + 1);
            }
            let velocity = bd.polarity * v / RAD;
            commands.borrow_mut().insert(bd.id, (goal, raw, velocity, is_clamped));
            let speed = (velocity.abs() + 60.).min(cfg.sweep_tuning.maximum_speed_counts_s);
            plan.push((SweepInput { speed_counts_s: speed, pwm_limit: r.drive_pwm.clamp(1, 1000) }, MotionCommand::Track(goal.clamp(lo, hi), velocity), a.clone()));
            shown.insert(bd.id.to_string(), json!(goal));
        }
        let mut s = app.state.lock().unwrap();
        s["gait"]["t"] = json!(t);
        s["gait"]["phase"] = json!(if started.get() { if playing { "playing" } else { "paused" } } else { "approach" });
        s["gait"]["speed_scale"] = json!(scale);
        s["gait"]["targets"] = Value::Object(shown);
        s["gait"]["clamped"] = json!(clamped.get());
        Ok(Some(plan))
    }, |id, t, sample| {
        let p = t.position_continuous.unwrap_or(t.position_raw as i32) as f64;
        positions.borrow_mut().insert(id, p);
        let (command, desired, velocity, is_clamped) = commands.borrow().get(&id).copied().unwrap_or((p, p, 0., false));
        {
            let mut rows = rows.borrow_mut();
            if rows.len() < 60_000 {
                rows.push(json!({"id": id, "wall_s": run_started.elapsed().as_secs_f64(), "gait_s": clock.get().0, "playing": started.get(),
                    "command": command, "desired": desired, "command_velocity": velocity, "clamped": is_clamped,
                    "actual": p, "velocity": sample.velocity_counts_s, "pwm": sample.pwm, "voltage_v": t.voltage_v, "temperature_c": t.temperature_c,
                    "warnings": sample.warnings}));
            }
        }
        let mut s = app.state.lock().unwrap();
        s["samples"][id.to_string()] = json!(t);
        s["gait"]["errors"][id.to_string()] = json!(p - command);
        if !sample.warnings.is_empty() {
            s["gait"]["warnings"] = json!(sample.warnings);
        }
        // Rolling statistics every ~1 s of samples.
        if rows.borrow().len() % 100 == 0 {
            s["gait"]["statistics"] = gait_statistics(&rows.borrow(), &ids, &cfg.roles, &predicted, pwm_ceiling);
        }
        Ok(())
    })?;
    let statistics = gait_statistics(&rows.borrow(), &ids, &cfg.roles, &predicted, pwm_ceiling);
    let outcome = result
        .motion_error
        .clone()
        .unwrap_or_else(|| "stopped".into());
    let dir = cfg.output.join("gait-runs");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut record = labelled(
        app,
        json!({"version": 1, "gait": r.gait, "gait_governor": gait.info.governor, "effort": effort, "speed_scale": speed_scale,
        "pwm_ceiling": pwm_ceiling, "drive_mode": r.drive_mode, "bindings": bindings, "limits": limits, "registry": registry.identity(),
        // The supply the limits were computed at, and whether it was assumed (no usable reading).
        "supply_v": supply, "supply_assumed": measured_supply.is_none(),
        "gait_time_s": clock.get().0, "wall_s": run_started.elapsed().as_secs_f64(), "outcome": outcome, "clamped_targets": clamped.get(),
        "statistics": statistics, "samples": *rows.borrow(),
        "scope": "Suspended leg (no ground contact); tracking error is actual minus the governed command sent to the shared controller."}),
    );
    if app.execution.is_virtual_calibration() {
        record["virtual_limits"] = json!(virtual_gait_limits(measured_supply, supply));
    }
    fs::write(
        dir.join(format!("run-{}.json", stamp())),
        serde_json::to_vec(&record).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    writeln_log(
        cfg,
        labelled(
            app,
            json!({"event": "gait_end", "gait": r.gait, "motion_error": result.motion_error, "clamped_targets": clamped.get(), "statistics": statistics}),
        ),
    )?;
    {
        let mut s = app.state.lock().unwrap();
        s["gait"]["statistics"] = statistics;
        s["gait_runs"] = gait_run_history(cfg);
    }
    match result.motion_error {
        Some(e) => Err(e),
        None => Ok(format!(
            "Gait stopped after {:.1} s of gait time; statistics saved",
            clock.get().0
        )),
    }
}
pub(super) fn writeln_log(cfg: &Config, value: Value) -> R<()> {
    use std::io::Write as _;
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(cfg.output.join("gait.jsonl"))
        .map_err(|e| e.to_string())?;
    let mut value = value;
    value["unix_ms"] = json!(stamp());
    writeln!(f, "{value}").map_err(|e| e.to_string())
}
/// A lesson's lab step (`sim-lab`): one taught, proven motor at a bounded
/// duty for a few seconds, through the campaign's guarded session (travel
/// window with braking margin, sag, temperature, stop). Needs the operator's
/// confirmation that the leg is supported; writes a receipt.
#[allow(clippy::too_many_arguments)]
pub(super) fn run_lab_step(
    app: &App,
    cfg: &Config,
    b: &mut CalibrationBus,
    cal: &Calibration,
    proven: &std::collections::BTreeSet<u8>,
    session: Option<&str>,
    r: &Request,
    stop_epoch: u64,
    reply: impl FnOnce(&Value),
) -> R<Value> {
    use crate::acquisition::{calibration_serial::BusRig, characterization as ch};
    if !r.supported {
        return Err(
            "Confirm the operator checklist (at the bench, leg supported, supervisor running)"
                .into(),
        );
    }
    if app.stop.load(Ordering::SeqCst) {
        return Err("Stop is latched; clear it at the bench first".into());
    }
    let (id, role) = cfg
        .roles
        .iter()
        .find(|(_, role)| {
            role.as_str() == r.role
                || role.starts_with(&format!("{}/", r.role))
                || role.ends_with(&format!("/{}", r.role))
        })
        .map(|(i, role)| (*i, role.clone()))
        .ok_or_else(|| format!("No motor has the role `{}`", r.role))?;
    let a = usable(&cal.axes[&id], session);
    let (lo, hi) = a.encoder_bounds();
    if a.disabled {
        return Err(format!("{role} is disabled"));
    }
    let (Some(lo), Some(hi)) = (lo, hi) else {
        return Err(format!("Teach both poses of {role} first"));
    };
    if !proven.contains(&id) {
        return Err(format!(
            "Prove {role}'s watchdogs first (select it with hold enabled)"
        ));
    }
    // Gates and drive limits from the campaign plan, when configured.
    let plan: Option<ch::Plan> = cfg
        .campaign_plan
        .as_ref()
        .and_then(|p| fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok());
    let gates = plan.as_ref().map(|p| p.gates.clone()).unwrap_or_default();
    let limits: std::collections::BTreeMap<u8, ch::AxisLimits> = plan
        .as_ref()
        .and_then(|p| p.limits.get(&role).cloned())
        .map(|l| [(id, l)].into())
        .unwrap_or_default();
    let axis = ch::Axis {
        id,
        role: role.clone(),
        lower: lo.min(hi) as f64,
        upper: lo.max(hi) as f64,
    };
    app.arm(stop_epoch, false)?;
    {
        let mut s = app.state.lock().unwrap();
        s["busy"] = json!(true);
        s["lab"] = json!({"running": true, "role": role, "duty": r.duty, "seconds": r.seconds});
        s["message"] = json!(format!(
            "Lab step on {role}: duty {:.2} for {:.1} s. Stop ends it.",
            r.duty, r.seconds
        ));
        reply(&s);
    }
    let windows = vec![(id, axis.lower as i32, axis.upper as i32)];
    let step = {
        let mut bus = BusRig::new(b, &windows, cfg.sweep_tuning.period_s, &app.cancel)?;
        let mut limited = ch::LimitedRig::new(&mut bus, limits.clone());
        let result = {
            let mut session = ch::Session::new(&mut limited, gates, vec![axis.clone()])?;
            session.limits = limits;
            ch::lab_step(&mut session, id, r.duty, r.seconds)
        };
        let _ = ch::Rig::stop(&mut limited);
        result?
    };
    const RAD: f64 = std::f64::consts::TAU / 4096.;
    let dir = cfg.output.join("labs");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("lab-{}-{}.json", role.replace('/', "-"), stamp()));
    let steady_rad_s = step.steady_counts_s.map(|c| c * RAD);
    let receipt = json!({"role": role, "motor_id": id, "duty": r.duty, "seconds": r.seconds, "steady_counts_s": step.steady_counts_s, "steady_rad_s": steady_rad_s, "stopped": step.stopped, "samples": step.samples, "window": [axis.lower, axis.upper], "supply_v": step.samples.first().map(|s| s.voltage_v)});
    fs::write(&path, serde_json::to_vec_pretty(&receipt).unwrap()).map_err(|e| e.to_string())?;
    writeln_log(
        cfg,
        json!({"event": "lab_step", "role": role, "duty": r.duty, "seconds": r.seconds, "steady_rad_s": steady_rad_s, "stopped": step.stopped, "receipt": path}),
    )?;
    Ok(
        json!({"role": role, "steady_rad_s": steady_rad_s, "stopped": step.stopped, "seconds": step.seconds, "receipt": path, "supply_v": step.samples.first().map(|s| s.voltage_v),
        "headline": match steady_rad_s { Some(v) => format!("{role} ran at {v:.3} rad/s"), None => format!("{role} did not reach a steady speed") }}),
    )
}
