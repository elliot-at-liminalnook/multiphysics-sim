//! Compare aligned samples from integrate_embedding. Diagnostic only: no claim
//! of a converged reference or hardware accuracy. Optional accepted-stage
//! contact traces support method-consistent impulse comparisons.
use serde_json::{Value, json};
use sim_runtime::{
    session::EpisodeFrame,
    tracking::{CaptureConfig, sample_markers},
};
use std::collections::BTreeMap;
fn read(path: &str) -> Result<Value, String> {
    serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
fn number(v: &Value) -> Result<f64, String> {
    v.as_f64()
        .filter(|v| v.is_finite())
        .ok_or("missing/nonfinite number".into())
}
fn array(v: &Value) -> Result<&Vec<Value>, String> {
    v.as_array().ok_or("missing array".into())
}
fn distance(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).powi(2))
        .sum::<f64>()
        .sqrt()
}
fn contact_comparison(
    a: &Value,
    b: &Value,
    frames: &[Value],
    end: f64,
) -> Result<Option<Value>, String> {
    use sim_domain_robot::articulated::embedding::EmbeddedContactStep;
    use sim_runtime::contact_audit::{
        compare_impulse_reports, embedded_contact_impulses, embedded_floor_force_ratios,
    };
    if a["contact_steps"].is_null() || b["contact_steps"].is_null() {
        return Ok(None);
    }
    let ca: Vec<EmbeddedContactStep> =
        serde_json::from_value(a["contact_steps"].clone()).map_err(|e| e.to_string())?;
    let cb: Vec<EmbeddedContactStep> =
        serde_json::from_value(b["contact_steps"].clone()).map_err(|e| e.to_string())?;
    let total = compare_impulse_reports(
        &embedded_contact_impulses(0.0, end, &ca)?,
        &embedded_contact_impulses(0.0, end, &cb)?,
    )?;
    let mut windows = Vec::new();
    for pair in frames.windows(2) {
        let (start, finish) = (number(&pair[0]["time_s"])?, number(&pair[1]["time_s"])?);
        let tolerance = 128.0 * f64::EPSILON * finish.abs();
        let report = |steps: &[EmbeddedContactStep]| {
            let window: Vec<_> = steps
                .iter()
                .filter(|s| {
                    s.start_time_s + s.step_s > start + tolerance
                        && s.start_time_s < finish - tolerance
                })
                .cloned()
                .collect();
            embedded_contact_impulses(start, finish, &window)
        };
        windows.push(json!({"start_s":start,"end_s":finish,"differences":compare_impulse_reports(&report(&ca)?,&report(&cb)?)?}));
    }
    Ok(Some(json!({"total":total,"windows":windows,
            "floor_force_ratio_diagnostic":{
                "minimum_normal_force_n":0.1,
                "candidate":embedded_floor_force_ratios(0.0,end,&ca,0.1)?,
                "reference":embedded_floor_force_ratios(0.0,end,&cb,0.1)?,
                "scope":"Patch resultant |F_xy|/F_z for current world-Z floor law, excluding internal contacts and loads below the stated reporting cutoff. Not a calibrated coefficient, torsional capacity or an acceptance gate."
            },
            "scope":"Accepted BE endpoint-force quadrature with complete coverage. Not an exact continuous impulse; no torsional impulse, energy or calibrated accuracy gate."})))
}

#[cfg(test)]
fn compare(a: &Value, b: &Value, markers: &CaptureConfig) -> Result<Value, String> {
    compare_with_reduction(a,b,markers,false)
}

fn motor_metadata(d: &Value) -> Result<(Value,Value,sim_domain_robot::motor::MotorDynamics),String> {
    use sim_domain_robot::motor::MotorDynamics;
    let mut options=d["scene_options"].as_object().ok_or("missing scene options")?.clone();
    let mode:MotorDynamics=serde_json::from_value(options.remove("motor_dynamics").unwrap_or(json!("detailed"))).map_err(|e|e.to_string())?;
    let expected=mode.parameter_flags();
    let mut components=array(&d["motor_components"] )?.clone();
    for component in &mut components {
        let parameters=component["parameters"].as_object_mut().ok_or("missing motor parameters")?;
        for key in ["dynamics.quasistatic_winding","dynamics.quasistatic_rotor"] {
            let actual=match parameters.remove(key) {None=>0.0,Some(v)=>number(&v)?};
            if actual!=expected.get(key).copied().unwrap_or(0.0) {
                return Err(format!("motor dynamics option/parameter disagreement: {key}"));
            }
        }
    }
    Ok((Value::Object(options),Value::Array(components),mode))
}

fn compare_with_reduction(a: &Value, b: &Value, markers: &CaptureConfig, allow_motor_reduction:bool) -> Result<Value, String> {
    for d in [a, b] {
        if d["completed"] != true || !d["error"].is_null() || number(&d["simulated_s"])? <= 0.0 {
            return Err("both diagnostic runs must complete".into());
        }
        if let Some(hash) = &markers.expected_cad_sha256 {
            if d["source"]["cad_sha256"].as_str() != Some(hash) {
                return Err("marker CAD source mismatch".into());
            }
        }
    }
    for key in [
        "source",
        "independent_coordinates",
        "motor_state_layout",
        "motor_experiment",
        "applied_generalized_loads",
    ] {
        if a.get(key).is_none() || b.get(key).is_none() || a[key] != b[key] {
            return Err(format!("different experiment metadata: {key}"));
        }
    }
    let (options_a,motors_a,mode_a)=motor_metadata(a)?;
    let (options_b,motors_b,mode_b)=motor_metadata(b)?;
    if options_a!=options_b {return Err("different experiment metadata: scene_options".into());}
    if motors_a!=motors_b {return Err("different experiment metadata: motor_components".into());}
    if mode_a!=mode_b && !allow_motor_reduction {
        return Err("different motor dynamics require explicit --motor-reduction comparison".into());
    }
    if a["world"]!=b["world"] {return Err("different experiment metadata: world".into());}
    if a["policy_experiment"] != b["policy_experiment"] {
        return Err("different experiment metadata: policy_experiment".into());
    }
    if a["motion_gate"] != b["motion_gate"] {
        return Err("different experiment metadata: motion_gate".into());
    }
    // Exact closure derivative and block-factor implementations may be experimental variables;
    // all other embedding scales, tolerances and rank methods must match.
    let embedding = |d: &Value| -> Result<(Value, bool, bool, bool), String> {
        let mut config = d["embedding"].as_object().ok_or("missing embedding configuration")?.clone();
        let direct = match config.remove("direct_closure_jacobian") {
            None => false,
            Some(Value::Bool(value)) => value,
            _ => return Err("invalid direct_closure_jacobian option".into()),
        };
        let blocked = match config.remove("block_dependent_factorization") {
            None => false,
            Some(Value::Bool(value)) => value,
            _ => return Err("invalid block_dependent_factorization option".into()),
        };
        let analytic = match config.remove("analytic_mechanism_positions") {
            None => false,
            Some(Value::Bool(value)) => value,
            _ => return Err("invalid analytic_mechanism_positions option".into()),
        };
        Ok((Value::Object(config), direct, blocked, analytic))
    };
    let (embedding_a, direct_a, blocked_a, analytic_a) = embedding(a)?;
    let (embedding_b, direct_b, blocked_b, analytic_b) = embedding(b)?;
    if embedding_a != embedding_b {
        return Err("different experiment metadata: embedding".into());
    }
    // Legacy captures omit optional banks. Empty, null and absent are equivalent
    // only when neither capture has that bank/layout configured.
    for key in [
        "driver_components",
        "servo_components",
        "servo_state_layout",
    ] {
        let configured = |d: &Value| {
            d.get(key)
                .filter(|v| !v.is_null() && v.as_array().is_none_or(|a| !a.is_empty()))
                .cloned()
        };
        if configured(a) != configured(b) {
            return Err(format!("different experiment metadata: {key}"));
        }
    }
    if a["control_guard_offset"] != b["control_guard_offset"] {
        return Err("different experiment metadata: control_guard_offset".into());
    }
    if a["initial_coordinates"] != b["initial_coordinates"] {
        return Err("different experiment metadata: initial_coordinates".into());
    }
    if a["initial_base_translation_m"] != b["initial_base_translation_m"] {
        return Err("different experiment metadata: initial_base_translation_m".into());
    }
    let (af, bf) = (array(&a["frames"])?, array(&b["frames"])?);
    if af.len() != bf.len() || af.len() < 2 {
        return Err("equal nonempty sample grids required".into());
    }
    let mut marker_errors: BTreeMap<String, (f64, f64, f64)> = BTreeMap::new();
    let mut maxima: BTreeMap<&str, f64> = BTreeMap::new();
    let mut last_time = -1.0;
    let mut max_gap = 0.0_f64;
    let mut contact_mode_mismatches = 0;
    let mut sample_differences = Vec::new();
    for (x, y) in af.iter().zip(bf) {
        let t = number(&x["time_s"])?;
        if (t - number(&y["time_s"])?).abs() > 1e-12 || t <= last_time {
            return Err("unaligned/nonmonotonic sample times".into());
        }
        if last_time >= 0.0 {
            max_gap = max_gap.max(t - last_time);
        } else if t != 0.0 {
            return Err("initial sample required".into());
        }
        last_time = t;
        let sample = |f: &Value| -> Result<_, String> {
            let frame = EpisodeFrame {
                time_s: t,
                done: false,
                poses: serde_json::from_value(f["poses"].clone()).map_err(|e| e.to_string())?,
                joint_positions: vec![],
                telemetry: Default::default(),
                contacts: vec![],
                error: None,
            };
            sample_markers(&frame, &markers.markers)
        };
        let (sx, sy) = (sample(x)?, sample(y)?);
        let mut point_errors = BTreeMap::new();
        for (id, p) in &sx.points {
            let e = distance(&p.position_m, &sy.points[id].position_m);
            point_errors.insert(id.clone(), e);
            let v = marker_errors.entry(id.clone()).or_default();
            if e > v.0 {
                v.0 = e;
                v.2 = t;
            }
            v.1 += e * e;
        }
        for key in ["joint_positions", "joint_velocities", "motor_states"] {
            let (xs, ys) = (array(&x[key])?, array(&y[key])?);
            if xs.len() != ys.len() {
                return Err(format!("different {key} dimensions"));
            }
            for (u, v) in xs.iter().zip(ys) {
                let e = (number(u)? - number(v)?).abs();
                let m = maxima.entry(key).or_default();
                *m = m.max(e);
            }
        }
        for key in ["servo_commands", "servo_states"] {
            match (x.get(key), y.get(key)) {
                (None, None) => (),
                (Some(xs), Some(ys)) => {
                    let (xs, ys) = (array(xs)?, array(ys)?);
                    if xs.len() != ys.len() {
                        return Err(format!("different {key} dimensions"));
                    }
                    for (u, v) in xs.iter().zip(ys) {
                        let error = (number(u)? - number(v)?).abs();
                        let maximum = maxima.entry(key).or_default();
                        *maximum = maximum.max(error);
                    }
                }
                _ => return Err(format!("different {key} telemetry layout")),
            }
        }
        let (xm, ym) = (array(&x["motor_readings"])?, array(&y["motor_readings"])?);
        if xm.len() != ym.len() {
            return Err("different motor count".into());
        }
        for (u, v) in xm.iter().zip(ym) {
            for key in [
                "current_a",
                "gear_speed_rad_s",
                "shaft_torque_nm",
                "heating_w",
            ] {
                let e = (number(&u[key])? - number(&v[key])?).abs();
                let m = maxima.entry(key).or_default();
                *m = m.max(e);
            }
        }
        match (x.get("driver_readings"), y.get("driver_readings")) {
            (None, None) => (),
            (Some(dx), Some(dy)) => {
                let (dx, dy) = (array(dx)?, array(dy)?);
                if dx.len() != dy.len() {
                    return Err("different driver telemetry dimensions".into());
                }
                for (u, v) in dx.iter().zip(dy) {
                    for key in ["motor_voltage_v", "supply_current_a", "power_difference_w"] {
                        let error = (number(&u[key])? - number(&v[key])?).abs();
                        let maximum = maxima.entry(key).or_default();
                        *maximum = maximum.max(error);
                    }
                }
            }
            _ => return Err("different driver telemetry layout".into()),
        }
        // Compare active body-pair sets; point identities may change at events.
        let pairs = |f: &Value| -> Result<_, String> {
            Ok(array(&f["contacts"])?
                .iter()
                .map(|c| format!("{}:{}", c["link"], c["other"]))
                .collect::<std::collections::BTreeSet<_>>())
        };
        let (candidate_pairs, reference_pairs) = (pairs(x)?, pairs(y)?);
        contact_mode_mismatches += usize::from(candidate_pairs != reference_pairs);
        sample_differences.push(json!({"time_s":t,"marker_errors_m":point_errors,"candidate_contact_pairs":candidate_pairs,"reference_contact_pairs":reference_pairs}));
    }
    if (last_time - number(&a["simulated_s"])?).abs() > 1e-12
        || (last_time - number(&b["simulated_s"])?).abs() > 1e-12
    {
        return Err("terminal sample required".into());
    }
    let impulse_comparison = contact_comparison(a, b, af, last_time)?;
    let events = |d: &Value| -> Result<_, String> {
        let mut by_guard: BTreeMap<u64, Vec<f64>> = BTreeMap::new();
        for step in array(&d["hybrid_steps"])? {
            for event in array(&step["events"])? {
                by_guard
                    .entry(event["guard"].as_u64().ok_or("invalid guard")?)
                    .or_default()
                    .push(number(&event["time"])?);
            }
        }
        Ok(by_guard)
    };
    let (ae, be) = (events(a)?, events(b)?);
    let guards: std::collections::BTreeSet<_> = ae.keys().chain(be.keys()).collect();
    let event_report:Vec<_>=guards.into_iter().map(|g| {
        let (x,y)=(ae.get(g).map(Vec::as_slice).unwrap_or(&[]),be.get(g).map(Vec::as_slice).unwrap_or(&[]));
        json!({"guard":g,"candidate_count":x.len(),"reference_count":y.len(),"maximum_ordered_time_difference_s":if x.len()==y.len() {Some(x.iter().zip(y).map(|(a,b)|(a-b).abs()).fold(0.0_f64,f64::max))} else {None}})
    }).collect();
    Ok(
        json!({"scope":"Aligned sampled diagnostic only; motor_states and joint arrays contain mixed units. Event occurrence matching is diagnostic, not proof of identical modes. Optional accepted-stage impulse comparison is diagnostic. No energy, hardware or learning gate is established.",
        "contact_impulse_comparison":impulse_comparison,"samples":af.len(),"sample_differences":sample_differences,"max_sample_gap_s":max_gap,"duration_s":last_time,
        "exact_frames":a["frames"]==b["frames"],"exact_terminal_frame":a["terminal_frame"]==b["terminal_frame"],
        "exact_event_schedule":ae==be,"contact_pair_mismatch_samples":contact_mode_mismatches,
        "markers":marker_errors.into_iter().map(|(id,(max,sum,t))|json!({"id":id,"maximum_error_m":max,"rms_error_m":(sum/af.len() as f64).sqrt(),"worst_time_s":t})).collect::<Vec<_>>(),
        "maximum_sampled_differences":maxima,"events":event_report,
        "motor_dynamics_comparison":{"candidate":mode_a,"reference":mode_b,"physical_reduction_acknowledged":allow_motor_reduction,"scope":"Only declared winding/rotor storage-term flags may differ; all other captured motor parameters, world, controller and experiment metadata must match. Physical reduction comparisons do not establish numerical equivalence or calibration."},
        "analytic_mechanism_positions":{"candidate":analytic_a,"reference":analytic_b},
        "block_dependent_factorization":{"candidate":blocked_a,"reference":blocked_b},"direct_closure_jacobian":{"candidate":direct_a,"reference":direct_b},
        "candidate_stepping_wall_s":a["stepping_wall_s"],"reference_stepping_wall_s":b["stepping_wall_s"]}),
    )
}
fn main() {
    let run = || -> Result<Value, String> {
        let args: Vec<_> = std::env::args().skip(1).collect();
        if args.len() != 3 && !(args.len()==4 && args[3]=="--motor-reduction") {
            return Err(
                "usage: compare_embedding candidate.json reference.json markers.json [--motor-reduction]".into(),
            );
        }
        compare_with_reduction(
            &read(&args[0])?,
            &read(&args[1])?,
            &serde_json::from_value(read(&args[2])?).map_err(|e| e.to_string())?,
            args.len()==4,
        )
    };
    match run() {
        Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap()),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(offset: f64) -> Value {
        let frame = |t: f64| json!({"time_s":t,"poses":[{"name":"foot","position_m":[offset*t,0.0,0.0],"rotation":[[0.0,-1.0,0.0],[1.0,0.0,0.0],[0.0,0.0,1.0]]}],"motor_readings":[],"contacts":[],"joint_positions":[],"joint_velocities":[],"motor_states":[]});
        json!({"completed":true,"error":null,"source":{"cad_sha256":"fixture"},"scene_options":{},"independent_coordinates":[],"motor_components":[],"motor_state_layout":[],"motor_experiment":null,"applied_generalized_loads":[],"embedding":{},"simulated_s":1.0,"frames":[frame(0.0),frame(1.0)],"hybrid_steps":[{"events":[{"guard":0,"time":0.5}]}],"terminal_frame":frame(1.0)})
    }
    #[test]
    fn reduction_comparison_requires_matching_explicit_flags_and_preserves_other_checks() {
        let cfg:CaptureConfig=serde_json::from_value(json!({"experiment_id":"reduction","coordinate_frame":"world","markers":[{"id":"tip","link":"foot","local_point_m":[0.0,0.0,0.0]}]})).unwrap();
        let mut detailed=fixture(0.0);
        detailed["motor_components"]=json!([{"dof":"motor","parameters":{"resistance":2.0,"inductance":0.004}}]);
        detailed["world"]=json!({"floor_height":0.0});
        let mut reduced=detailed.clone();
        reduced["scene_options"]["motor_dynamics"]=json!("quasistatic_winding");
        reduced["motor_components"][0]["parameters"]["dynamics.quasistatic_winding"]=json!(1.0);
        assert!(compare(&reduced,&detailed,&cfg).is_err());
        let report=compare_with_reduction(&reduced,&detailed,&cfg,true).unwrap();
        assert_eq!(report["motor_dynamics_comparison"]["physical_reduction_acknowledged"],true);
        for key in ["resistance","inductance"] {
            let mut invalid=reduced.clone();invalid["motor_components"][0]["parameters"][key]=json!(10.0);
            assert!(compare_with_reduction(&invalid,&detailed,&cfg,true).is_err());
        }
        let mut invalid=reduced.clone();invalid["world"]["floor_height"]=json!(0.1);
        assert!(compare_with_reduction(&invalid,&detailed,&cfg,true).is_err());
        invalid=reduced.clone();invalid["motor_components"][0]["parameters"].as_object_mut().unwrap().remove("dynamics.quasistatic_winding");
        assert!(compare_with_reduction(&invalid,&detailed,&cfg,true).is_err());
    }
    #[test]
    fn samples_use_rotated_markers_and_reject_unaligned_or_incomplete_runs() {
        let cfg:CaptureConfig=serde_json::from_value(json!({"experiment_id":"fixture","coordinate_frame":"world","expected_cad_sha256":"fixture","markers":[{"id":"tip","link":"foot","local_point_m":[1.0,0.0,0.0]}]})).unwrap();
        let a = fixture(0.003);
        let b = fixture(0.0);
        let report = compare(&a, &b, &cfg).unwrap();
        assert!((number(&report["markers"][0]["maximum_error_m"]).unwrap() - 0.003).abs() < 1e-15);
        assert_eq!(report["exact_frames"], false);
        assert_eq!(report["exact_event_schedule"], true);
        let mut rotated = b.clone();
        rotated["frames"][1]["poses"][0]["rotation"] =
            json!([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
        let rotation_report = compare(&b, &rotated, &cfg).unwrap();
        assert!(
            (number(&rotation_report["markers"][0]["maximum_error_m"]).unwrap() - 2.0_f64.sqrt())
                .abs()
                < 1e-15
        );
        let mut bad = b.clone();
        bad["frames"][1]["time_s"] = json!(0.9);
        assert!(compare(&a, &bad, &cfg).is_err());
        bad = b.clone();
        bad["completed"] = json!(false);
        assert!(compare(&a, &bad, &cfg).is_err());
        bad = b.clone();
        bad["source"]["cad_sha256"] = json!("other");
        assert!(compare(&a, &bad, &cfg).is_err());
        bad = b.clone();
        bad["policy_experiment"] = json!({"controller":{"inputs":[{"initial":0.5}]}});
        assert!(compare(&a, &bad, &cfg).unwrap_err().contains("policy_experiment"));
        bad = b.clone();
        bad["initial_coordinates"] = json!([0.3]);
        assert!(compare(&a, &bad, &cfg).is_err());
        bad = b.clone();
        bad["initial_base_translation_m"] = json!([0.0, 0.0, -0.008]);
        assert!(compare(&a, &bad, &cfg).is_err());
        bad = b.clone();
        bad["embedding"]["direct_closure_jacobian"] = json!(true);
        assert_eq!(compare(&a, &bad, &cfg).unwrap()["direct_closure_jacobian"]["reference"], json!(true));
        bad["embedding"]["block_dependent_factorization"] = json!(true);
        assert_eq!(compare(&a, &bad, &cfg).unwrap()["block_dependent_factorization"]["reference"], json!(true));
        bad["embedding"]["analytic_mechanism_positions"] = json!(true);
        assert_eq!(compare(&a, &bad, &cfg).unwrap()["analytic_mechanism_positions"]["reference"], json!(true));
        bad["embedding"]["analytic_mechanism_positions"] = json!("yes");
        assert!(compare(&a, &bad, &cfg).is_err());
        bad["embedding"]["analytic_mechanism_positions"] = json!(true);
        bad["embedding"]["length_scale_m"] = json!(0.2);
        assert!(compare(&a, &bad, &cfg).is_err());
        bad = b.clone();
        bad["frames"][1]["joint_positions"] = json!([0.0]);
        assert!(compare(&a, &bad, &cfg).is_err());
        bad = b.clone();
        bad["frames"][1]["poses"][0]["name"] = json!("missing");
        assert!(compare(&a, &bad, &cfg).is_err());
        let mut driven = a.clone();
        driven["driver_components"] = json!([{"dof":"shaft","parameters":{"on_resistance":0.1}}]);
        for frame in driven["frames"].as_array_mut().unwrap() {
            frame["driver_readings"] =
                json!([{"motor_voltage_v":0.0,"supply_current_a":0.0,"power_difference_w":0.0}]);
        }
        let reference = driven.clone();
        driven["frames"][1]["driver_readings"][0]["motor_voltage_v"] = json!(2.0);
        assert_eq!(
            compare(&driven, &reference, &cfg).unwrap()["maximum_sampled_differences"]["motor_voltage_v"],
            json!(2.0)
        );
        driven["driver_components"][0]["parameters"]["on_resistance"] = json!(0.2);
        assert!(compare(&driven, &reference, &cfg).is_err());
    }
    #[test]
    fn servo_comparison_checks_feedback_state_and_clock_identity() {
        let cfg: CaptureConfig = serde_json::from_value(
            json!({"experiment_id":"servo","coordinate_frame":"world","markers":[{"id":"tip","link":"foot","local_point_m":[0.0,0.0,0.0]}]}),
        )
        .unwrap();
        let mut a = fixture(0.0);
        a["servo_components"] = json!([{"dof":"shaft","parameters":{"rate":50.0}}]);
        a["servo_state_layout"] = json!([[0, 5, 0]]);
        a["control_guard_offset"] = json!(3);
        for frame in a["frames"].as_array_mut().unwrap() {
            frame["servo_commands"] = json!([0.0]);
            frame["servo_states"] = json!([0.0, 0.0, 0.0, 0.0, 1.02]);
        }
        let mut b = a.clone();
        b["frames"][1]["servo_commands"][0] = json!(0.25);
        b["frames"][1]["servo_states"][0] = json!(0.25);
        let report = compare(&a, &b, &cfg).unwrap();
        assert_eq!(
            report["maximum_sampled_differences"]["servo_commands"],
            json!(0.25)
        );
        assert_eq!(
            report["maximum_sampled_differences"]["servo_states"],
            json!(0.25)
        );
        for key in [
            "servo_components",
            "servo_state_layout",
            "control_guard_offset",
        ] {
            let mut wrong = b.clone();
            wrong[key] = Value::Null;
            assert!(compare(&a, &wrong, &cfg).unwrap_err().contains(key));
        }
        b["frames"][1]["servo_states"] = json!([]);
        assert!(compare(&a, &b, &cfg).unwrap_err().contains("dimensions"));
        b = a.clone();
        b["frames"][0]
            .as_object_mut()
            .unwrap()
            .remove("servo_commands");
        assert!(
            compare(&a, &b, &cfg)
                .unwrap_err()
                .contains("telemetry layout")
        );
        let legacy = fixture(0.0);
        let mut empty = legacy.clone();
        empty["servo_components"] = json!([]);
        empty["servo_state_layout"] = Value::Null;
        assert!(compare(&legacy, &empty, &cfg).is_ok());
    }
    #[test]
    fn contact_windows_reject_missing_stages_and_include_zero_contact_pairs() {
        let mut a = fixture(0.0);
        let mut b = fixture(0.0);
        let step = |start: f64, force: f64| json!({"start_time_s":start,"step_s":0.5,"contacts":[{"link":0,"other":null,"force_n":[0.0,0.0,force],"point_m":[0.0,0.0,0.0],"penetration_m":0.001}]});
        a["contact_steps"] = json!([step(0.0, 2.0), step(0.5, 4.0)]);
        b["contact_steps"] = json!([{"start_time_s":0.0,"step_s":1.0,"contacts":[]}]);
        let frames = array(&a["frames"]).unwrap();
        let report = contact_comparison(&a, &b, frames, 1.0).unwrap().unwrap();
        assert_eq!(report["total"][0]["candidate_ns"], json!([0.0, 0.0, 3.0]));
        assert_eq!(report["total"][0]["difference_norm_ns"], json!(3.0));
        let mut missing = a.clone();
        missing["contact_steps"].as_array_mut().unwrap().remove(0);
        assert!(contact_comparison(&missing, &b, frames, 1.0).is_err());
        b["contact_steps"] = Value::Null;
        assert!(contact_comparison(&a, &b, frames, 1.0).unwrap().is_none());
    }
}
