//! Compile a dense-audited contact plan into ordinary sampled motor references.
//! All kinematics, inverse loads and interpolation use shared Rust components.
use serde::Deserialize;
use serde_json::json;
use sim_domain_control::{
    contact_phase::{ContactPhaseConfig, ContactPhaseMotion},
    trajectory::{Interpolation, Keyframe, Trajectory, TrajectoryConfig},
};
use sim_runtime::{
    contact_planning::{ContactPlanRecipe, ContactPlanner, JointContactMotion},
    session::{Scene, Session},
    tracking::CaptureConfig,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    robot: ContactPlanRecipe,
    motion: ContactPhaseConfig,
    /// Optional independently optimized forward and reverse xyz force templates.
    /// Static pause loads continue to use the explicitly audited static allocator.
    #[serde(default)]
    force_templates: Option<[Vec<TrajectoryConfig>; 2]>,
    /// Maximum sampled joint interpolation errors: rad, rad/s, rad/s^2.
    maximum_reference_errors: [f64; 3],
    minimum_pause_window_s: f64,
    #[serde(default)]
    require_reverse_feasible: bool,
    /// Permit measured closed-loop diagnostics of an imperfect inverse-load/contact
    /// reference. Never changes or passes its physical audit, interpolation
    /// gates, pause checks, or the runtime actuator/contact model.
    #[serde(default)]
    diagnostic_allow_failed_reference_audit: bool,
}
fn curve(values: Vec<Vec<f64>>, period: f64) -> Result<TrajectoryConfig, String> {
    let n = values.len();
    let mut keyframes = values
        .into_iter()
        .enumerate()
        .map(|(i, values)| Keyframe {
            time_s: period * i as f64 / n as f64,
            values,
        })
        .collect::<Vec<_>>();
    keyframes.push(Keyframe {
        time_s: period,
        values: keyframes[0].values.clone(),
    });
    let config = TrajectoryConfig {
        interpolation: Interpolation::PeriodicCubicBSpline,
        keyframes,
    };
    Trajectory::new(config.clone())?;
    Ok(config)
}
fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        return Err("usage: compile_contact_reference scene.json markers.json recipe.json".into());
    }
    let read = |p: &str| std::fs::read(p).map_err(|e| e.to_string());
    let scene: Scene = serde_json::from_slice(&read(&args[0])?).map_err(|e| e.to_string())?;
    let markers: CaptureConfig =
        serde_json::from_slice(&read(&args[1])?).map_err(|e| e.to_string())?;
    let recipe: Recipe = serde_json::from_slice(&read(&args[2])?).map_err(|e| e.to_string())?;
    if recipe.robot.phase_rate != 1.0
        || recipe.robot.phase_acceleration_per_s != 0.0
        || !recipe.robot.additional_clocks.is_empty()
        || recipe.robot.uniform_samples < 64
        || recipe
            .maximum_reference_errors
            .iter()
            .any(|v| !v.is_finite() || *v <= 0.0)
        || !recipe.minimum_pause_window_s.is_finite()
        || recipe.minimum_pause_window_s < 2.0 * scene.period_s
    {
        return Err("unit-rate periodic plan, dense sampling, positive errors and at least two policy periods per pause window required".into());
    }
    let mut session = Session::new(scene, 0)?;
    session.robot.art.contact_on = false;
    let seed = session.robot.generalized();
    let planner = ContactPlanner::new(&session.robot.art, &seed, &markers, recipe.robot.clone())?;
    let evaluate = |planner: &ContactPlanner<'_>, clock: usize| {
        if let Some(templates) = &recipe.force_templates {
            let joint = planner.evaluate_joint(&JointContactMotion {
                motion: recipe.motion.clone(),
                force_templates: vec![templates[clock].clone()],
                force_timing: None,
            })?;
            let mut report = joint.motion_report;
            // Preserve cone and zero inter-link-overlap gates in compilation.
            report.sampled_feasible = joint.sampled_feasible;
            Ok::<_, String>(report)
        } else {
            planner.evaluate(&recipe.motion)
        }
    };
    let nominal = evaluate(&planner, 0)?;
    if !nominal.sampled_feasible && !recipe.diagnostic_allow_failed_reference_audit {
        return Err("nominal contact plan fails declared dense physical tolerances".into());
    }
    let mut static_recipe = recipe.robot.clone();
    static_recipe.phase_rate = 0.0;
    let static_planner = ContactPlanner::new(&session.robot.art, &seed, &markers, static_recipe)?;
    let stationary = static_planner.evaluate(&recipe.motion)?;
    let mut reverse_recipe = recipe.robot.clone();
    reverse_recipe.phase_rate = -1.0;
    let reverse = evaluate(
        &ContactPlanner::new(&session.robot.art, &seed, &markers, reverse_recipe)?,
        1,
    )?;
    if recipe.require_reverse_feasible
        && !reverse.sampled_feasible
        && !recipe.diagnostic_allow_failed_reference_audit
    {
        return Err("reverse contact plan fails declared dense physical tolerances".into());
    }
    let n = recipe.robot.uniform_samples;
    let period = recipe.motion.period_s;
    let offset = period / (2.0 * n as f64);
    let trajectory = curve(
        nominal.frames[..n]
            .iter()
            .map(|f| f.coordinates.clone())
            .collect(),
        period,
    )?;
    let reference = Trajectory::new(trajectory.clone())?;
    let mut errors = [0.0_f64; 3];
    for frame in &nominal.frames {
        let sample = reference.sample((frame.time_s - offset).rem_euclid(period))?;
        for j in 0..frame.coordinates.len() {
            errors[0] = errors[0].max((sample.values[j] - frame.coordinates[j]).abs());
            errors[1] = errors[1].max((sample.rates[j] - frame.reduced_velocity[6 + j]).abs());
            errors[2] =
                errors[2].max((sample.accelerations[j] - frame.reduced_acceleration[6 + j]).abs());
        }
    }
    if errors
        .iter()
        .zip(recipe.maximum_reference_errors)
        .any(|(e, b)| *e > b)
    {
        return Err(format!(
            "sampled reference interpolation errors {errors:?} exceed declared tolerances"
        ));
    }
    let stiffness = recipe
        .robot
        .independent_coordinates
        .iter()
        .map(|name| recipe.robot.actuators[name]["stiffness"])
        .collect::<Vec<_>>();
    let static_ff = curve(
        stationary.frames[..n]
            .iter()
            .map(|f| {
                f.motor_torques_nm
                    .iter()
                    .zip(&stiffness)
                    .map(|(t, k)| t / k)
                    .collect()
            })
            .collect(),
        period,
    )?;
    let dynamic_ff = curve(
        nominal.frames[..n]
            .iter()
            .zip(&reverse.frames[..n])
            .zip(&stationary.frames[..n])
            .map(|((a, b), zero)| {
                a.motor_torques_nm
                    .iter()
                    .zip(&b.motor_torques_nm)
                    .zip(&zero.motor_torques_nm)
                    .zip(&stiffness)
                    .map(|(((a, b), zero), k)| (0.5 * (a + b) - zero) / k)
                    .collect()
            })
            .collect(),
        period,
    )?;
    let velocity_ff = curve(
        nominal.frames[..n]
            .iter()
            .zip(&reverse.frames[..n])
            .map(|(a, b)| {
                a.motor_torques_nm
                    .iter()
                    .zip(&b.motor_torques_nm)
                    .zip(&stiffness)
                    .map(|((a, b), k)| 0.5 * (a - b) / k)
                    .collect()
            })
            .collect(),
        period,
    )?;
    let motion = ContactPhaseMotion::new(recipe.motion.clone())?;
    let mut pause_windows = Vec::new();
    for interval in motion.contact_intervals() {
        let duration = (interval.end_phase - interval.start_phase) * period;
        if duration < recipe.minimum_pause_window_s {
            continue;
        }
        let sample = motion.sample((interval.start_phase + interval.end_phase) * 0.5 * period)?;
        if sample.feet.iter().all(|f| f.in_contact) {
            pause_windows.push([interval.start_phase * period, interval.end_phase * period]);
        }
    }
    if pause_windows.is_empty() {
        return Err("this pause-based controller needs a sufficiently long all-stance interval; other contact schedules need a different stopping controller".into());
    }
    let mut pause_static_samples = 0;
    for f in &stationary.frames {
        if pause_windows.iter().any(|w| {
            let t = if f.time_s < w[0] {
                f.time_s + period
            } else {
                f.time_s
            };
            t >= w[0] && t <= w[1]
        }) {
            pause_static_samples += 1;
            if f.wrench_residual[..3]
                .iter()
                .any(|v| v.abs() > recipe.robot.force_tolerance_n)
                || f.wrench_residual[3..]
                    .iter()
                    .any(|v| v.abs() > recipe.robot.moment_tolerance_nm)
                || f.torque_capacity_margin_nm
                    .iter()
                    .any(|v| *v < -recipe.robot.torque_tolerance_nm)
            {
                return Err("all-stance pause window fails sampled static load feasibility".into());
            }
        }
    }
    if pause_static_samples == 0 {
        return Err("pause window lacks static samples".into());
    }
    let window = pause_windows
        .iter()
        .max_by(|a, b| (a[1] - a[0]).total_cmp(&(b[1] - b[0])))
        .unwrap();
    let initial_phase = ((window[0] + window[1]) * 0.5).rem_euclid(period);
    let initial = reference.sample((initial_phase - offset).rem_euclid(period))?;
    let body = motion.sample(initial_phase)?.body;
    let initial_base_translation: Vec<_> = (0..3)
        .map(|i| recipe.robot.initial_base_translation_m[i] + body.values[i])
        .collect();
    let mut output = json!({
        "motion":recipe.motion,"trajectory":trajectory,"static_feedforward":static_ff,
        "dynamic_feedforward":dynamic_ff,"velocity_feedforward":velocity_ff,
        "diagnostic_allow_failed_reference_audit":recipe.diagnostic_allow_failed_reference_audit,
        "required_load_audits_passed":nominal.sampled_feasible
            && (!recipe.require_reverse_feasible || reverse.sampled_feasible),
        "reverse_load_audit":{"required":recipe.require_reverse_feasible,"sampled_feasible":reverse.sampled_feasible,
            "maximum_force_error_n":reverse.maximum_force_error_n,"maximum_moment_error_nm":reverse.maximum_moment_error_nm,
            "minimum_torque_margin_nm":reverse.minimum_torque_margin_nm,"maximum_penetration_m":reverse.maximum_penetration_m},
        "feedforward_scope":"Static + phase_rate * odd load + phase_rate squared * even dynamic load reconstructs the three sampled inverse-load cases at rates 0/+1/-1 before interpolation. Intermediate rates, acceleration and yaw remain approximations, not inverse-dynamics certificates.",
        "phase_offset_s":offset,"pause_windows_s":pause_windows,
        "initial_phase_s":initial_phase,"initial_coordinates":initial.values,
        "initial_base_translation_m":initial_base_translation,
        "initial_base_rotation_vector_rad":&body.values[3..6],
        "nominal_speed_m_s":nominal.speed_m_s,"maximum_reference_errors":errors,
        "pause_static_samples":pause_static_samples,
        "nominal_physical_summary":{"sampled_feasible":nominal.sampled_feasible,"force_n":nominal.maximum_force_error_n,"moment_nm":nominal.maximum_moment_error_nm,
            "minimum_torque_margin_nm":nominal.minimum_torque_margin_nm,"penetration_m":nominal.maximum_penetration_m},
        "recipe":recipe.robot,
        "scope":"Reference compiled into periodic B-spline motor targets with explicit nominal/reverse load audit outcomes. Diagnostic permission to execute a failed physical reference never makes that audit pass. Interpolation errors are sampled, not continuous certificates. Odd/even feedforward blending during transitions is a controller approximation. Pause windows pass sampled static load checks. Detailed contact physics and closed-loop WASD validation remain required."
    });
    if let Some(templates) = recipe.force_templates {
        output["joint_force_reference"] = json!({
            "forward_reverse_templates": templates,
            "scope": "Forward/reverse motor feedforward uses the independently supplied force trajectories. Static pause loads use the unchanged constrained allocator. Force-cone and zero sampled inter-link-overlap checks participate in the nominal/reverse audit; any diagnostic audit failure remains a failure. Runtime contact forces remain generated by the simulator, not injected from this reference."
        });
    }
    println!(
        "{}",
        serde_json::to_string(&output).map_err(|e| e.to_string())?
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
