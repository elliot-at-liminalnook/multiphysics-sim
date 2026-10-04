//! The run readouts the browser viewer shows beside a preset run
//! (`web/viewer/viewer.js` showFrame, showTaskObservations,
//! showMotionProgress, the actuation profile, scheduled pushes and learned
//! corrections), from the frames the run thread publishes (`Frame::extra`,
//! the runtime frame without its poses) and the session's load metadata
//! (`Drive::metadata`). Presentation only: every number is a field of the
//! frame, the metadata, the preset's config or the scene, with the browser's
//! units and rounding; nothing is computed beyond its divisions (net travel
//! over elapsed time, a vector's length, degrees from radians). Shown in the
//! inspector's Run section and published as `robot_state.readouts`.
use super::*;

/// Why a readout block is empty, when it is.
const NO_FRAME: &str = "no frame yet: opening builds the session; Run or Step advances it";

fn num(v: &Value) -> Option<f64> {
    v.as_f64().filter(|x| x.is_finite())
}
fn fixed(v: f64, d: usize) -> String {
    // −0 prints as 0 (the browser's `fixed`).
    let r = format!("{:.*}", d, v + 0.0);
    if r.trim_start_matches('-').chars().all(|c| c == '0' || c == '.') { r.trim_start_matches('-').to_string() } else { r }
}
fn vec3(v: &Value, scale: f64, d: usize) -> String {
    v.as_array().map_or("—".into(), |a| a.iter().map(|x| num(x).map_or("—".into(), |x| fixed(x * scale, d))).collect::<Vec<_>>().join(", "))
}
fn norm(v: &Value) -> Option<f64> {
    let a = v.as_array()?;
    Some(a.iter().filter_map(num).map(|x| x * x).sum::<f64>().sqrt())
}

/// `robot_state.readouts`: each block the browser shows, as structured JSON
/// with its text line(s); a block absent from the frame is null.
pub(super) fn readouts(view: &RobotView) -> Value {
    let Some(run) = view.run.as_ref() else { return Value::Null };
    let frame = run.shown_frame();
    let x = frame.and_then(|f| f.extra.as_deref());
    let drive = run.preset_drive();
    let meta = drive.map(|d| &d.metadata);
    let preset = run.preset();
    let mut out = serde_json::Map::new();
    out.insert("execution".into(), json!(execution(run)));
    out.insert("sim_time_s".into(), json!(frame.map(|f| f.time)));
    out.insert("performance".into(), json!(run.rtf().map(|r| format!("{}× live", fixed(r, 2)))));
    let contacts = x.and_then(|x| x["contacts"].as_array()).map(Vec::len).or_else(|| frame.and_then(|f| f.overlays.contacts.as_ref()).map(Vec::len));
    out.insert("contact_count".into(), json!(contacts));
    if let Some(x) = x {
        let learning = &x["learning"];
        // Travel speed: the learning environment's net travel since reset over elapsed sim time.
        if let (Some(speed), Some(t)) = (learning.get("speed").filter(|s| s.is_object()), frame.map(|f| f.time)) {
            let net = num(&speed["net_distance_m"]).unwrap_or(0.0);
            let fallen = speed["fallen"] == true;
            let rate = if t > 0.0 { net / t } else { 0.0 };
            out.insert("travel".into(), json!({"net_distance_m": net, "mean_speed_m_s": rate, "fallen": fallen,
                "text": format!("Actual net travel: {} m/s · {} m since reset{}. Mean over elapsed simulation time.", fixed(rate, 3), fixed(net, 3), if fallen { " · FALL DETECTED" } else { "" })}));
        }
        if let Some(w) = learning.get("walking").filter(|w| w.is_object()) {
            let body = norm(&w["body_error_world_m"]).map_or("—".into(), |e| fixed(e * 1000.0, 2));
            let heading = w["heading"]["error_rad"].as_f64().map_or(String::new(), |e| format!(" · heading {}°", fixed(e.to_degrees(), 3)));
            out.insert("walking".into(), json!({"value": w, "text": format!("{} qualified · {} failed · body error {body} mm{heading}", w["qualified_steps"], w["failed_steps"])}));
        }
        if learning.is_object() {
            let state = if learning["terminated"] == true { "Task bound reached" } else if learning["truncated"] == true { "Time limit reached" } else { "Episode in progress" };
            let w = &learning["walking"];
            let walking = if w.is_object() {
                let outcome = if w["outcome"].is_object() { format!(" Last step {}: {}.", w["outcome"]["step"], if w["outcome"]["passed"] == true { "qualified" } else { "failed" }) } else { String::new() };
                format!(" Qualified steps: {}; failed: {}. Body reference error: {} mm.{outcome}", w["qualified_steps"], w["failed_steps"], norm(&w["body_error_world_m"]).map_or("—".into(), |e| fixed(e * 1000.0, 2)))
            } else {
                String::new()
            };
            let heading = w["heading"].as_object().map_or(String::new(), |h| format!(" Heading error: {}°; heading score: {}.", h.get("error_rad").and_then(Value::as_f64).map_or("—".into(), |e| fixed(e.to_degrees(), 3)), h.get("reward").and_then(Value::as_f64).map_or("—".into(), |r| fixed(r, 6))));
            let reasons: Vec<&str> = learning["termination_reasons"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            let observations = learning["observations"].as_array().map_or(0, Vec::len);
            let text = format!("Learning environment · {state}. Last {} ms score: {}. {observations} ideal observations; not hardware sensor readings.{walking}{heading}{}",
                num(&learning["elapsed_s"]).map_or("—".into(), |e| fixed(e * 1000.0, 0)), num(&learning["reward"]).map_or("—".into(), |r| fixed(r, 6)), if reasons.is_empty() { String::new() } else { format!(" {}", reasons.join("; ")) });
            out.insert("learning".into(), json!({"state": state, "reward": learning["reward"], "elapsed_s": learning["elapsed_s"], "observations": observations, "termination_reasons": reasons, "text": text}));
        }
        if let Some(m) = motion_progress(x, meta) {
            out.insert("motion_progress".into(), m);
        }
        if let Some(l) = x.get("environment_load").filter(|l| l.is_object()) {
            let active = l["force_world_n"].as_array().into_iter().chain(l["moment_world_nm"].as_array()).flatten().any(|v| num(v).is_some_and(|v| v != 0.0));
            out.insert("world_load".into(), json!({"active": active, "value": l, "text": format!("{} · world force [{}] N · moment [{}] N·m about the body's center of mass.", if active { "Push active" } else { "Push inactive" }, vec3(&l["force_world_n"], 1.0, 3), vec3(&l["moment_world_nm"], 1.0, 3))}));
        }
        if let Some(r) = x["policy"]["neural_residual"].as_object() {
            out.insert("neural_residuals".into(), json!(r));
        }
        out.insert("joint_readings".into(), joint_readings(x, meta, frame.map(|f| f.inputs.as_slice()).unwrap_or(&[])));
        if let Some(t) = task_observations(x, meta) {
            out.insert("task_observations".into(), t);
        }
    }
    if let Some(p) = preset {
        if let Some(a) = actuation(p, view.model.as_ref()) {
            out.insert("actuation_profile".into(), json!(a));
        }
        if let Some(c) = p.config.as_ref() {
            let config = json!(c);
            if config["world_loads"].is_object() {
                out.insert("scheduled_pushes".into(), config["world_loads"].clone());
            }
            if let Some(outputs) = config["policy"]["neural_residual"]["outputs"].as_array() {
                out.insert("neural_residual_outputs".into(), json!(outputs.iter().filter_map(|o| o["target"].as_str()).collect::<Vec<_>>()));
            }
        }
    }
    out.insert("rule".into(), json!("the browser viewer's readouts (web/viewer/viewer.js showFrame and its helpers) from the run thread's latest frame and the session's load metadata; presentation only, with the browser's units and rounding"));
    if frame.is_none() {
        out.insert("reason".into(), json!(NO_FRAME));
    }
    Value::Object(out)
}

/// The execution line (`viewer.js` showFrame's execution-state).
fn execution(run: &RunController) -> String {
    let f = run.shown_frame();
    let x = f.and_then(|f| f.extra.as_deref());
    if run.error().is_some() || x.is_some_and(|x| x["error"].is_string()) {
        return "Experiment stopped with an error".into();
    }
    let learning = x.map(|x| &x["learning"]);
    if learning.is_some_and(|l| l["terminated"] == true) {
        return "Task bound reached".into();
    }
    if learning.is_some_and(|l| l["truncated"] == true) {
        return "Episode time limit reached".into();
    }
    if run.phase() == run::Phase::Ended || x.is_some_and(|x| x["done"] == true) {
        return "Experiment complete".into();
    }
    match run.phase() {
        run::Phase::Running => {
            let done = f.and_then(|f| f.completed_steps).map_or(String::new(), |n| n.to_string());
            let of = run.preset().map_or(String::new(), |p| format!(" / {} physics steps", p.requested_steps()));
            format!("Running · {done}{of}")
        }
        run::Phase::Building => "Building the session…".into(),
        run::Phase::Idle => "Not built yet".into(),
        _ => "Paused".into(),
    }
}

/// The step-reference controller's phase, else the planned motion's (`showMotionProgress`).
fn motion_progress(x: &Value, meta: Option<&Value>) -> Option<Value> {
    let contract = meta.map(|m| &m["policy_contract"]);
    if let Some(step) = x["policy"]["step_reference"]["reference"].as_object() {
        let phase = step.get("phase").and_then(Value::as_str).unwrap_or("");
        let waiting = step.get("waiting") == Some(&json!(true));
        let foot = step.get("foot").and_then(Value::as_u64).and_then(|f| contract?["point_feedback"]["config"]["markers"].get(f as usize)?["id"].as_str().map(str::to_string));
        let prelift = contract.is_some_and(|c| c["step_reference"]["config"]["sequence"]["update_command_before_lift"] == true);
        let title = if x["done"] == true { "Episode ended · reset to continue".to_string() } else {
            match phase {
                "idle" => "Standing · ready for a command".into(),
                "hold" => "Settling initial stance".into(),
                "recenter" => format!("Returning to standing · lift canceled{}", if waiting { " · waiting for all feet to support the body" } else { "" }),
                p => format!("{} · {p}{}", foot.as_deref().unwrap_or("Foot"), if waiting { " · waiting for support" } else { "" }),
            }
        };
        let twist = step.get("latched_twist").cloned().unwrap_or(Value::Null);
        let detail = format!("Completed transfers: {}. Current transfer: {}%. Latched speed: {} mm/s · turn: {}°/s. {}",
            step.get("step").cloned().unwrap_or(Value::Null), step.get("progress").and_then(Value::as_f64).map_or("—".into(), |p| fixed(p * 100.0, 0)),
            twist.get(0).and_then(Value::as_f64).map_or("—".into(), |v| fixed(v * 1000.0, 2)), twist.get(2).and_then(Value::as_f64).map_or("—".into(), |w| fixed(w.to_degrees(), 3)),
            if prelift { "New requests are checked again before lift-off. Airborne steps and reversals finish the current transfer." } else { "Changes apply at the next transfer." });
        return Some(json!({"source": "policy.step_reference.reference", "title": title, "text": detail, "value": step}));
    }
    let p = x["motion_progress"].as_object()?;
    let labels = [("initial", "Preparing motion"), ("following_reference", "Following the plan"), ("waiting_for_condition", "Waiting for sustained foot support"), ("condition_qualified", "Foot support qualified"), ("complete", "Planned motion complete"), ("timed_out", "Support checkpoint timed out")];
    let phase = p.get("phase").and_then(Value::as_str).unwrap_or("");
    let title = labels.iter().find(|(k, _)| *k == phase).map_or(phase.to_string(), |(_, l)| l.to_string());
    let g = |k: &str| p.get(k).and_then(Value::as_f64);
    let ms = |k: &str| g(k).map_or("—".into(), |v| fixed(v * 1000.0, 0));
    let text = format!("Plan: {} / {} s. Support observed for {} / {} ms. Waiting: {} / {} ms.", g("reference_time_s").map_or("—".into(), |v| fixed(v, 3)), g("duration_s").map_or("—".into(), |v| fixed(v, 3)), ms("qualified_duration_s"), ms("required_qualification_s"), ms("paused_duration_s"), ms("maximum_pause_s"));
    Some(json!({"source": "motion_progress", "title": title, "text": text, "value": p}))
}

/// Plan → motor target → actual per coordinate (`viewer.js` readings), degrees.
fn joint_readings(x: &Value, meta: Option<&Value>, _held: &[f64]) -> Value {
    let names = meta.and_then(|m| m["coordinate_names"].as_array());
    let indices = meta.and_then(|m| m["joint_indices"].as_array());
    let positions = x["joint_positions"].as_array();
    let rows: Vec<Value> = match x["servo_targets_rad"].as_array() {
        Some(targets) => targets.iter().enumerate().map(|(i, t)| {
            let name = names.and_then(|n| n.get(i)).and_then(Value::as_str).map_or(format!("Coordinate {}", i + 1), |n| n.trim_start_matches("joint.").to_string());
            let actual = indices.and_then(|ix| ix.get(i)).and_then(Value::as_u64).and_then(|j| positions?.get(j as usize)).and_then(num);
            json!({"name": name, "reference": x["reference_targets_rad"].get(i).and_then(num), "target": num(t), "actual": actual})
        }).collect(),
        None => positions.into_iter().flatten().enumerate().map(|(i, a)| json!({"name": format!("Joint {}", i + 1), "reference": null, "target": x["telemetry"]["actuators"].get(i).and_then(num), "actual": num(a)})).collect(),
    };
    let label = if x["reference_targets_rad"].is_array() { "Plan → motor target → actual" } else { "Requested → actual" };
    let lines: Vec<String> = rows.iter().map(|r| {
        let deg = |k: &str| r[k].as_f64().map(|v| fixed(v.to_degrees(), 1));
        format!("{}: {}{} → {}°", r["name"].as_str().unwrap_or(""), deg("reference").map_or(String::new(), |v| format!("{v} → ")), deg("target").unwrap_or_else(|| "—".into()), deg("actual").unwrap_or_else(|| "—".into()))
    }).collect();
    json!({"label": label, "unit": "degrees", "rows": rows, "lines": lines})
}

/// The task observations panel (`showTaskObservations`): point and body
/// feedback, body vectors and each marker's position, velocity and support force.
fn task_observations(x: &Value, meta: Option<&Value>) -> Option<Value> {
    let contract = &meta?["policy_contract"];
    let config = &contract["task_observations"]["config"];
    let feedback_config = &contract["body_feedback"]["config"];
    let point_config = &contract["point_feedback"]["config"];
    if config.is_null() && feedback_config.is_null() && point_config.is_null() {
        return None;
    }
    let policy = &x["policy"];
    let Some(obs) = policy["observations"].as_object() else {
        return Some(json!({"lines": ["Waiting for the first controller sample."]}));
    };
    let mut lines = vec![format!("Controller sample: {} s · vectors shown as x, y, z", num(&policy["time_s"]).map_or("—".into(), |t| fixed(t, 3)))];
    let vector = |name: &str, scale: f64| ["x", "y", "z"].iter().map(|a| obs.get(&format!("{name}.{a}")).and_then(Value::as_f64).map_or("—".into(), |v| fixed(v * scale, 2))).collect::<Vec<_>>().join(", ");
    let mm = |v: &Value| vec3(v, 1000.0, 2);
    if let Some(points) = policy["point_feedback"].as_object() {
        lines.push(format!("Foot / point position feedback — Reference time: {} s · ideal world observations", points.get("reference_time_s").and_then(Value::as_f64).map_or("—".into(), |t| fixed(t, 3))));
        for (i, m) in point_config["markers"].as_array().into_iter().flatten().enumerate() {
            let at = |k: &str| points.get(k).and_then(|a| a.get(i)).cloned().unwrap_or(Value::Null);
            lines.push(format!("{} world tracking — target (mm): {} · actual (mm): {} · error (mm): {} · activation: {}%", m["id"].as_str().unwrap_or("?"), mm(&at("target_positions_world_m")), mm(&at("actual_positions_world_m")), mm(&at("position_errors_world_m")), at("activation").as_f64().map_or("—".into(), |a| fixed(a * 100.0, 0))));
        }
        let largest = points.get("correction_rad").and_then(Value::as_array).map(|c| c.iter().filter_map(num).fold(0.0_f64, |m, v| m.max(v.abs())));
        lines.push(format!("Bounded point suggestion — Largest before policy gain: {}°", largest.map_or("—".into(), |l| fixed(l.to_degrees(), 3))));
    }
    if let Some(f) = policy["body_feedback"].as_object() {
        let g = |k: &str| f.get(k).cloned().unwrap_or(Value::Null);
        lines.push(format!("Body position feedback — Reference time: {} s · world target (mm): {} · actual (mm): {} · error (mm): {}", g("reference_time_s").as_f64().map_or("—".into(), |t| fixed(t, 3)), mm(&g("target_position_world_m")), mm(&g("actual_position_world_m")), mm(&g("position_error_world_m"))));
        let weights: Vec<String> = feedback_config["support_markers"].as_array().into_iter().flatten().enumerate().map(|(i, m)| format!("{}: {}%", m["id"].as_str().unwrap_or("?"), g("support_weights").get(i).and_then(Value::as_f64).map_or("—".into(), |w| fixed(w * 100.0, 0)))).collect();
        lines.push(format!("Support used for correction — {}", weights.join(" · ")));
        let largest = g("correction_rad").as_array().map(|c| c.iter().filter_map(num).fold(0.0_f64, |m, v| m.max(v.abs())));
        lines.push(format!("Bounded joint suggestion — Largest correction before policy gain: {}°", largest.map_or("—".into(), |l| fixed(l.to_degrees(), 2))));
    }
    if config.is_object() {
        lines.push(format!("Body — Gravity direction: {} · velocity (m/s): {} · angular velocity (rad/s): {}", vector("body.gravity_direction", 1.0), vector("body.linear_velocity", 1.0), vector("body.angular_velocity", 1.0)));
        for m in config["markers"].as_array().into_iter().flatten() {
            let id = m["id"].as_str().unwrap_or("?");
            let prefix = format!("marker.{id}");
            let support = if config["floor_forces"] == true { format!(" · upward support (N): {}", obs.get(&format!("{prefix}.floor_force_world.z")).and_then(Value::as_f64).map_or("—".into(), |v| fixed(v, 2))) } else { String::new() };
            lines.push(format!("{id} — Position (mm): {} · relative velocity (mm/s): {}{support}", vector(&format!("{prefix}.position"), 1000.0), vector(&format!("{prefix}.velocity"), 1000.0)));
        }
    }
    Some(json!({"lines": lines}))
}

/// The actuation profile line (cad_fixed_pd presets with CAD-bound motor families).
fn actuation(p: &crate::robot::preset::PresetRun, model: Option<&PhysicalModel>) -> Option<String> {
    let config = json!(p.config.as_ref()?);
    if config["motors"]["controller"] != "cad_fixed_pd" {
        return None;
    }
    let profiles = json!(model?.actuator_profiles.as_ref()?);
    let families: Vec<&Value> = profiles["families"].as_object()?.values().collect();
    if families.is_empty() {
        return None;
    }
    let mut rates: Vec<String> = families.iter().filter_map(|f| num(&f["controller"]["period"]["value"])).map(|period| fixed(1.0 / period, 0)).collect();
    rates.dedup();
    let profile_hz = num(&config["step_s"]).zip(num(&config["report_every"])).map_or("—".into(), |(s, r)| fixed(1.0 / (s * r), 0));
    Some(format!("Motor feedback/PWM: {} Hz · native measurement profile: {profile_hz} Hz · motion policy: {} Hz. {} CAD-bound motor parameter sets; provisional calibration.", rates.join(" / "), fixed(1.0 / p.scene.period_s, 0), families.len()))
}

/// The Run section's text: every readout block in the browser's order.
pub(super) fn text(view: &RobotView) -> String {
    let r = readouts(view);
    if r.is_null() {
        return "Run readouts: the robot has not loaded.".into();
    }
    let mut t = format!("RUN\n{}\n", r["execution"].as_str().unwrap_or(""));
    t += &format!("sim time {} s · {} · contacts {}\n", r["sim_time_s"].as_f64().map_or("—".into(), |x| fixed(x, 3)), r["performance"].as_str().unwrap_or("waiting for physics"), r["contact_count"].as_u64().map_or("—".into(), |n| n.to_string()));
    if let Some(reason) = r["reason"].as_str() {
        t += &format!("{reason}\n");
    }
    for key in ["actuation_profile"] {
        if let Some(s) = r[key].as_str() {
            t += &format!("{s}\n");
        }
    }
    if let Some(m) = r["motion_progress"].as_object() {
        t += &format!("\nMOTION PROGRESS — {}\n{}\n", m["title"].as_str().unwrap_or(""), m["text"].as_str().unwrap_or(""));
    }
    for (key, head) in [("travel", "TRAVEL"), ("walking", "WALKING"), ("learning", "LEARNING")] {
        if let Some(s) = r[key]["text"].as_str() {
            t += &format!("\n{head}\n{s}\n");
        }
    }
    if let Some(p) = r["scheduled_pushes"].as_object() {
        t += &format!("\nSCHEDULED BODY PUSHES — {} · experimental disturbance. Maximum combined force {} N; moment {} N·m.\n", p.get("base_link").and_then(Value::as_str).unwrap_or("?"), p.get("maximum_force_n").cloned().unwrap_or(Value::Null), p.get("maximum_moment_nm").cloned().unwrap_or(Value::Null));
        for pulse in p.get("pulses").and_then(Value::as_array).into_iter().flatten() {
            let (s, d) = (pulse["start_s"].as_f64().unwrap_or(0.0), pulse["duration_s"].as_f64().unwrap_or(0.0));
            t += &format!("{}: {}–{} s of simulation time.\n", pulse["name"].as_str().unwrap_or("?"), fixed(s, 2), fixed(s + d, 2));
        }
        t += &format!("{}\n", r["world_load"]["text"].as_str().unwrap_or("Push inactive"));
    }
    if let Some(outputs) = r["neural_residual_outputs"].as_array() {
        t += "\nLEARNED MOTOR CORRECTIONS — Rust network outputs added to baseline feedback (controlled by the policy; WASD requests the motion task)\n";
        for o in outputs.iter().filter_map(Value::as_str) {
            let v = r["neural_residuals"][o].as_f64().unwrap_or(0.0);
            t += &format!("{}: {} rad\n", o.trim_end_matches(".target"), fixed(v, 6));
        }
    }
    if let Some(lines) = r["joint_readings"]["lines"].as_array().filter(|l| !l.is_empty()) {
        t += &format!("\nREADINGS — {} (degrees)\n", r["joint_readings"]["label"].as_str().unwrap_or(""));
        for l in lines.iter().filter_map(Value::as_str) {
            t += &format!("{l}\n");
        }
    }
    if let Some(lines) = r["task_observations"]["lines"].as_array() {
        t += "\nTASK OBSERVATIONS\n";
        for l in lines.iter().filter_map(Value::as_str) {
            t += &format!("{l}\n");
        }
    }
    t
}
