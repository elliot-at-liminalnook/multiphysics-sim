use super::*;

/// Campaign directory and the receipts to reuse. Arms first: a command a STOP
/// interrupted creates nothing, so it can never shadow the real interrupted
/// campaign. Resume picks the newest unfinished/resumable directory holding
/// at least one receipt; empty ones are ignored, and none is ever deleted.
pub(super) fn campaign_directory(
    app: &App,
    root: &std::path::Path,
    resume: bool,
    stop_epoch: u64,
) -> R<(
    PathBuf,
    Vec<crate::acquisition::characterization::StageResult>,
)> {
    use crate::acquisition::characterization as ch;
    app.arm(stop_epoch, false)?;
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let receipt = |p: &std::path::Path| {
        p.extension().is_some_and(|x| x == "json")
            && !p.to_string_lossy().ends_with(".execution.json")
    };
    let previous = fs::read_dir(root)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && (!p.join("report.json").exists()
                    || fs::read(p.join("resume-pending.json"))
                        .ok()
                        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                        .is_some_and(|v| v["resumable"] == json!(true)))
        })
        .filter(|p| {
            fs::read_dir(p.join("receipts"))
                .ok()
                .into_iter()
                .flatten()
                .flatten()
                .any(|e| receipt(&e.path()))
        })
        .max();
    match (resume, previous) {
        (true, Some(dir)) => {
            let history = dir.join(format!("attempt-before-resume-{}", stamp()));
            fs::create_dir(&history).map_err(|e| e.to_string())?;
            for name in [
                "report.json",
                "summary.json",
                "promotion.json",
                "resume-pending.json",
            ] {
                let previous = dir.join(name);
                if previous.exists() {
                    fs::copy(&previous, history.join(name)).map_err(|e| e.to_string())?;
                }
            }
            let mut receipts: Vec<ch::StageResult> = Vec::new();
            let mut names: Vec<_> = fs::read_dir(dir.join("receipts"))
                .map_err(|e| e.to_string())?
                .flatten()
                .map(|e| e.path())
                .filter(|p| !p.to_string_lossy().ends_with(".execution.json"))
                .collect();
            names.sort();
            for n in names {
                receipts.push(
                    serde_json::from_slice(&fs::read(&n).map_err(|e| e.to_string())?)
                        .map_err(|e| e.to_string())?,
                );
            }
            Ok((dir, receipts))
        }
        (true, None) => Err("No interrupted campaign with receipts to resume".into()),
        (false, _) => Ok((root.join(format!("campaign-{}", stamp())), Vec::new())),
    }
}
/// The characterization campaign on the connected leg (PLAN.md). Axes are the
/// enabled, watchdog-proven motors with both poses taught; each is rehearsed
/// with its tuned model (untuned axes are refused). Stage results are written
/// as receipts the moment they finish; a resumed campaign reuses them.
#[allow(clippy::too_many_arguments)]
pub(super) fn run_campaign(
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
    use crate::acquisition::{
        calibration_serial::BusRig, characterization as ch, virtual_bench::MotorModel,
    };
    if !r.supported {
        return Err("Confirm the leg is suspended with clear space around every joint".into());
    }
    let plan_path = cfg
        .campaign_plan
        .clone()
        .ok_or("No campaign_plan in the server configuration")?;
    let mut plan: ch::Plan = serde_json::from_slice(
        &fs::read(&plan_path).map_err(|e| format!("{}: {e}", plan_path.display()))?,
    )
    .map_err(|e| e.to_string())?;
    // Axes from taught poses; untaught, disabled, unproven or untuned motors are left out.
    let (mut axes, mut models, mut skipped) =
        (Vec::new(), std::collections::BTreeMap::new(), Vec::new());
    for (id, role) in &cfg.roles {
        let a = usable(&cal.axes[id], session);
        let (lo, hi) = a.encoder_bounds();
        let why = if a.disabled {
            Some("disabled")
        } else if lo.is_none() || hi.is_none() {
            Some("poses not taught")
        } else if !proven.contains(id) {
            Some("watchdogs not proven; select a motor with hold enabled")
        } else if a.tuning.is_none() {
            Some("not tuned; the campaign rehearses with the tuned model")
        } else {
            None
        };
        if let Some(w) = why {
            skipped.push(format!("{role} ({w})"));
            continue;
        }
        let (lo, hi) = (lo.unwrap().min(hi.unwrap()), lo.unwrap().max(hi.unwrap()));
        let t = a.tuning.as_ref().unwrap();
        axes.push(ch::Axis {
            id: *id,
            role: role.clone(),
            lower: lo as f64,
            upper: hi as f64,
        });
        models.insert(
            *id,
            MotorModel {
                speed_gain: t.gain_counts_s_per_duty,
                lag_s: t.time_constant_s.max(0.01),
                breakaway_duty: 0.5 * (t.breakaway_duty[0] + t.breakaway_duty[1]),
                moving_friction_duty: t.friction_duty,
                ..Default::default()
            },
        );
    }
    if axes.is_empty() {
        return Err(format!(
            "No motor is ready for the campaign: {}",
            skipped.join(", ")
        ));
    }
    plan.axes = axes.clone();
    let (dir, resume) =
        campaign_directory(app, &cfg.output.join("campaigns"), r.resume, stop_epoch)?;
    fs::create_dir_all(dir.join("receipts")).map_err(|e| e.to_string())?;
    let plan_bytes = serde_json::to_vec_pretty(&plan).map_err(|e| e.to_string())?;
    if r.resume && fs::read(dir.join("plan.json")).map_err(|e| e.to_string())? != plan_bytes {
        return Err("Campaign plan or taught axes changed; start a new campaign instead of reusing receipts".into());
    }
    if !r.resume {
        fs::write(dir.join("plan.json"), &plan_bytes).map_err(|e| e.to_string())?;
    }
    fs::write(dir.join("resume-pending.json"), serde_json::to_vec_pretty(&json!({"execution":app.execution,"simulated":app.execution.is_virtual_calibration(),"resumable":true})).unwrap()).map_err(|e| e.to_string())?;
    {
        let mut s = app.state.lock().unwrap();
        s["busy"] = json!(true);
        s["campaign"] = json!({"execution":app.execution,"simulated":app.execution.is_virtual_calibration(),"running": true, "axes": axes, "skipped": skipped, "stage": "Starting", "completed": resume.iter().filter(|stage| stage.completed).count(), "receipt_count":resume.len(), "directory": dir, "log": []});
        s["message"] = json!(format!(
            "Characterization campaign on {}. Stop ends it; completed stages are kept.",
            axes.iter()
                .map(|a| a.role.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
        reply(&s);
    }
    let windows: Vec<(u8, i32, i32)> = axes
        .iter()
        .map(|a| (a.id, a.lower as i32, a.upper as i32))
        .collect();
    let started = Instant::now();
    let receipt_error = std::cell::RefCell::new(None::<String>);
    let report = {
        let mut rig = BusRig::new(b, &windows, cfg.sweep_tuning.period_s, &app.cancel)?;
        let mut receipt_index = resume.len();
        let mut completed = resume.iter().filter(|stage| stage.completed).count();
        let result = ch::run_with(
            &plan,
            &mut rig,
            Some(ch::per_axis_predictor(models.clone())),
            // No CAD scene in this server: multi-axis combinations stay within
            // taught poses, which were reached together only if taught so.
            &|_| Ok(()),
            &mut |m| {
                let mut s = app.state.lock().unwrap();
                s["campaign"]["stage"] = json!(m);
                if let Some(log) = s["campaign"]["log"].as_array_mut() {
                    log.push(json!(m));
                }
            },
            &resume,
            &mut |stage| {
                let next_index = receipt_index + 1;
                let path = dir
                    .join("receipts")
                    .join(format!("{next_index:03}-{}-{}.json", stage.stage, stage.id));
                if let Err(error) = fs::write(&path, serde_json::to_vec_pretty(stage).unwrap()) {
                    *receipt_error.borrow_mut() = Some(format!("{}: {error}", path.display()));
                    app.latch_stop();
                    return;
                }
                let provenance = path.with_extension("execution.json");
                if let Err(error) = fs::write(
                    &provenance,
                    serde_json::to_vec_pretty(&app.execution).unwrap(),
                ) {
                    *receipt_error.borrow_mut() =
                        Some(format!("{}: {error}", provenance.display()));
                    app.latch_stop();
                    return;
                }
                receipt_index = next_index;
                let mut s = app.state.lock().unwrap();
                if stage.completed {
                    completed += 1;
                }
                s["campaign"]["completed"] = json!(completed);
                s["campaign"]["receipt_count"] = json!(receipt_index);
                s["campaign"]["last"] = json!({"stage": stage.stage, "axis": stage.id, "completed": stage.completed, "abort": stage.abort});
            },
        );
        let _ = ch::Rig::stop(&mut rig);
        result
    };
    if let Some(error) = receipt_error.into_inner() {
        return Err(error);
    }
    let report = report?;
    let fitted_replay: Vec<Value> = report.fitted.iter().map(|(id, fits)| {
        let prior = &models[id];
        json!({"axis": id, "tuned_model_rms_counts": ch::replay_error(&report.samples, *id, prior, cfg.sweep_tuning.period_s),
               "fitted_model_rms_counts": ch::replay_error(&report.samples, *id, &ch::fitted_model(prior, fits), cfg.sweep_tuning.period_s)})
    }).collect();
    let coordinates: std::collections::BTreeMap<u8, Vec<String>> = cfg
        .roles
        .iter()
        .map(|(id, role)| {
            let joint = if role.contains("knee") {
                "Foot"
            } else if role.contains("worm") {
                "Worm"
            } else {
                "Hip"
            };
            (
                *id,
                ["-Y", "+X", "+Y", "-X"]
                    .iter()
                    .map(|leg| format!("joint.{leg} | {joint} servo output"))
                    .collect(),
            )
        })
        .collect();
    let promotion = ch::promotion(
        &report,
        &json!({"source": "tuned per-motor models"}),
        &coordinates,
        &format!(
            "{} campaign on {} ({})",
            if app.execution.is_virtual_calibration() {
                "SIMULATED virtual calibration"
            } else {
                "Physical hardware"
            },
            cfg.fixture,
            dir.display()
        ),
    );
    let ranking = ch::select_tests(&report, &plan.sensitivity);
    let aborted: Vec<Value> = report
        .stages
        .iter()
        .filter(|s| !s.completed)
        .map(|s| json!({"stage": s.stage, "axis": s.id, "abort": s.abort}))
        .collect();
    let summary = json!({
        "execution":app.execution,"simulated":app.execution.is_virtual_calibration(),
        "resumable": !aborted.is_empty() || app.cancel.load(Ordering::SeqCst),
        "directory": dir, "wall_s": started.elapsed().as_secs_f64(), "axes": axes, "skipped": skipped,
        "aborted": aborted, "replay": fitted_replay, "ranking": ranking.iter().take(8).collect::<Vec<_>>(),
        "headline": format!("{} stages, {} stopped by a gate", report.stages.len(), aborted.len()),
    });
    for (name, mut value) in [
        ("report.json", serde_json::to_value(&report).unwrap()),
        ("promotion.json", promotion),
        ("summary.json", summary.clone()),
    ] {
        value["execution"] = json!(app.execution);
        value["simulated"] = json!(app.execution.is_virtual_calibration());
        fs::write(dir.join(name), serde_json::to_vec_pretty(&value).unwrap())
            .map_err(|e| e.to_string())?;
    }
    if summary["resumable"] == json!(false) {
        fs::write(dir.join("resume-pending.json"), serde_json::to_vec_pretty(&json!({"execution":app.execution,"simulated":app.execution.is_virtual_calibration(),"resumable":false})).unwrap()).map_err(|e| e.to_string())?;
    }
    Ok(summary)
}
