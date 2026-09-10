//! CAD closure/Jacobian inspection followed by conditional rate-budget analysis.
//! No robot topology, motor constants or physical defaults are supplied here.
use nalgebra::{DMatrix, DVector};
use serde::Deserialize;
use serde_json::json;
use sim_domain_control::trajectory::{Interpolation, Keyframe, Trajectory, TrajectoryConfig};
use sim_domain_robot::{
    articulated::embedding::RigidEmbedding,
    effective_servo::EffectiveServo,
    motion_capability::{directional_rate_bound, minimum_norm_point_forces},
};
use sim_runtime::{
    configuration_inspection::{ConfigurationInspection, inspect_configurations},
    session::{Scene, Session},
    tracking::CaptureConfig,
};
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferenceCycle {
    period_s: f64,
    stride_m: f64,
    /// Uniformly timed coordinates, including the repeated final sample.
    samples: Vec<Vec<f64>>,
    #[serde(default)]
    interpolation: Interpolation,
    /// Optional rate-only nonuniform traversal bracket; no acceleration model.
    #[serde(default)]
    retiming_subdivisions_per_segment: Option<usize>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    inspection: ConfigurationInspection,
    marker_coordinates: BTreeMap<String, Vec<String>>,
    actuators: BTreeMap<String, BTreeMap<String, f64>>,
    directions_world: Vec<[f64; 3]>,
    duty_factor: f64,
    support_groups: Vec<Vec<String>>,
    support_friction_coefficient: f64,
    reference_cycle: ReferenceCycle,
    provenance: serde_json::Value,
}
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 {
        return Err("usage: analyze_motion_capability scene.json markers.json recipe.json".into());
    }
    let read = |p: &str| std::fs::read(p).map_err(|e| e.to_string());
    let scene: Scene = serde_json::from_slice(&read(&args[0])?).map_err(|e| e.to_string())?;
    let markers: CaptureConfig =
        serde_json::from_slice(&read(&args[1])?).map_err(|e| e.to_string())?;
    let recipe: Recipe = serde_json::from_slice(&read(&args[2])?).map_err(|e| e.to_string())?;
    let coords = &recipe.inspection.independent_coordinates;
    if !(0.0..1.0).contains(&recipe.duty_factor)
        || recipe.duty_factor == 0.0
        || recipe.directions_world.is_empty()
        || recipe.directions_world.iter().any(|d| {
            d.iter().any(|v| !v.is_finite())
                || (DVector::from_column_slice(d).norm() - 1.0).abs() > 1e-9
        })
    {
        return Err(
            "unit directions and a duty factor strictly between zero and one required".into(),
        );
    }
    let mut power = 0.0;
    let mut budgets = Vec::new();
    for name in coords {
        let p = recipe
            .actuators
            .get(name)
            .ok_or_else(|| format!("missing actuator {name}"))?;
        let motor = EffectiveServo::new(p).map_err(|e| e.to_string())?;
        power += motor.peak_motoring_power_w();
        budgets.push(p["no_load_speed"]);
    }
    let cycle = &recipe.reference_cycle;
    if !cycle.period_s.is_finite()
        || cycle.period_s <= 0.0
        || !cycle.stride_m.is_finite()
        || cycle.stride_m <= 0.0
        || cycle.samples.len() < 2
        || cycle
            .samples
            .iter()
            .any(|s| s.len() != coords.len() || s.iter().any(|x| !x.is_finite()))
        || cycle.samples[0]
            .iter()
            .zip(cycle.samples.last().unwrap())
            .any(|(a, b)| (a - b).abs() > 1e-7)
    {
        return Err(
            "finite periodic coordinate samples and positive period/stride required".into(),
        );
    }
    let dt = cycle.period_s / (cycle.samples.len() - 1) as f64;
    let trajectory = Trajectory::new(TrajectoryConfig {
        interpolation: cycle.interpolation,
        keyframes: cycle.samples.iter().enumerate().map(|(i,values)| Keyframe {time_s:i as f64 * dt,values:values.clone()}).collect(),
    })?;
    let peak_rates = trajectory.maximum_absolute_rates()?;
    let cycle_bounds: Vec<_> = coords.iter().enumerate().map(|(j,name)| {
        let peak = peak_rates[j];
        let speed = if peak > 0.0 { Some(cycle.stride_m / cycle.period_s * budgets[j] / peak) } else { None };
        json!({"coordinate":name,"peak_reference_rate_rad_s":peak,"time_scaled_rate_budget_speed_m_s":speed})
    }).collect();
    let cycle_limit = cycle_bounds
        .iter()
        .filter_map(|r| r["time_scaled_rate_budget_speed_m_s"].as_f64())
        .fold(f64::INFINITY, f64::min);
    let retiming = cycle.retiming_subdivisions_per_segment.map(|subdivisions| {
        let bounds = trajectory.rate_traversal_bounds(&budgets, subdivisions)?;
        let speed = |duration:f64| if duration>0. {Some(cycle.stride_m/duration)} else {None};
        let mut limiting_duration = vec![0.;coords.len()];
        for cell in &bounds.cells {limiting_duration[cell.limiting_coordinate]+=cell.duration_upper_s;}
        Ok::<_,String>(json!({"subdivisions_per_segment":subdivisions,
            "speed_lower_m_s":speed(bounds.duration_upper_s),"speed_upper_m_s":speed(bounds.duration_lower_s),
            "uniform_to_piecewise_speed_ratio":if bounds.duration_upper_s>0. {Some(bounds.uniform_duration_s/bounds.duration_upper_s)}else{None},
            "limiting_coordinates":coords.iter().zip(limiting_duration).map(|(name,duration)|json!({"coordinate":name,"allocated_upper_duration_s":duration})).collect::<Vec<_>>(),
            "bounds":bounds,
            "scope":"Rate-only minimum cycle time bracket for the same joint path. Integral max_i |dq_i/ds|/budget_i ds; lower duration from per-cell endpoint displacements, upper duration from exact polynomial rate extrema and piecewise constant phase rates. Speed bounds invert those durations using the declared stride. Uses f64, not outward-rounded interval arithmetic. Omits acceleration, torque, phase-rate continuity, dwell, support/contact and stability; the upper construction is not a dynamically feasible controller. No-load budgets are not hard backdrive limits or a global physical speed ceiling."}))
    }).transpose()?;
    let mut session = Session::new(scene, 0)?;
    let rows = inspect_configurations(
        &session.robot.art,
        &session.robot.generalized(),
        &markers,
        &recipe.inspection,
    )?;
    // Analysis-only external point-force allocation replaces contact. The
    // running scene is unchanged; gravity, masses and passive mechanics remain.
    session.robot.art.contact_on = false;
    let art = &session.robot.art;
    let seed = session.robot.generalized();
    let map = RigidEmbedding::new(art, coords, recipe.inspection.embedding.clone())?;
    if map.reduced_dimension() != coords.len() + 6 {
        return Err("support audit requires a floating base".into());
    }
    let mut result = Vec::new();
    for row in rows {
        let valid = row.error.is_none()
            && row.authored_limit_violations.is_empty()
            && row.sampled_penetrations.is_empty();
        let mut directions = Vec::new();
        let mut support = Vec::new();
        if valid {
            let motion = map.solve(&seed, &row.coordinates, &vec![0.0; map.reduced_dimension()])?;
            let loads = map
                .prepare_dynamics(&motion)?
                .required_reduced_forces(&vec![0.0; map.reduced_dimension()])?;
            let base = art.bases[0].state;
            let reference = std::array::from_fn(|i| motion.generalized.states[base + i]);
            for group in &recipe.support_groups {
                if group.is_empty()
                    || group
                        .iter()
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                        != group.len()
                {
                    return Err("nonempty unique support markers required".into());
                }
                let selected = group
                    .iter()
                    .map(|id| {
                        row.markers
                            .iter()
                            .find(|m| &m.id == id)
                            .ok_or_else(|| format!("unknown support marker {id}"))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let points = selected
                    .iter()
                    .map(|m| m.position_world_m)
                    .collect::<Vec<_>>();
                let allocation = minimum_norm_point_forces(
                    &points,
                    reference,
                    std::array::from_fn(|i| loads[i]),
                    recipe.inspection.embedding.length_scale_m,
                    recipe.support_friction_coefficient,
                )?;
                let torques = (0..coords.len())
                    .map(|j| {
                        loads[6 + j]
                            - selected
                                .iter()
                                .zip(&allocation.forces_world_n)
                                .map(|(m, f)| (0..3).map(|k| m.jacobian[k][j] * f[k]).sum::<f64>())
                                .sum::<f64>()
                    })
                    .collect::<Vec<_>>();
                let max_stall_fraction = torques
                    .iter()
                    .enumerate()
                    .map(|(j, t)| t.abs() / recipe.actuators[&coords[j]]["stall_torque"])
                    .fold(0.0_f64, f64::max);
                let height_range = points
                    .iter()
                    .map(|p| p[2])
                    .fold(f64::NEG_INFINITY, f64::max)
                    - points.iter().map(|p| p[2]).fold(f64::INFINITY, f64::min);
                support.push(json!({"markers":group,"allocation":allocation,"required_motor_torques_nm":torques,
                    "maximum_static_stall_fraction":max_stall_fraction,"contact_height_range_m":height_range}));
            }
            for direction in &recipe.directions_world {
                let mut feet = Vec::new();
                let mut limit = f64::INFINITY;
                let mut rates = vec![0.0; coords.len()];
                for marker in &row.markers {
                    let names = recipe
                        .marker_coordinates
                        .get(&marker.id)
                        .ok_or_else(|| format!("missing marker binding {}", marker.id))?;
                    let indices = names
                        .iter()
                        .map(|n| {
                            coords
                                .iter()
                                .position(|c| c == n)
                                .ok_or_else(|| format!("unknown coordinate {n}"))
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    if indices.len() != 3
                        || indices
                            .iter()
                            .collect::<std::collections::BTreeSet<_>>()
                            .len()
                            != 3
                    {
                        return Err("each marker needs three unique independent coordinates".into());
                    }
                    if marker.jacobian.iter().any(|r| {
                        r.iter()
                            .enumerate()
                            .any(|(j, v)| !indices.contains(&j) && v.abs() > 1e-12)
                    }) {
                        return Err("marker moves under omitted independent coordinates".into());
                    }
                    let j = DMatrix::from_fn(3, 3, |r, c| marker.jacobian[r][indices[c]]);
                    let bound = directional_rate_bound(
                        &j,
                        &DVector::from_column_slice(direction),
                        &indices.iter().map(|i| budgets[*i]).collect::<Vec<_>>(),
                    )?;
                    for (k, i) in indices.iter().enumerate() {
                        rates[*i] = -bound.coordinate_rates_per_unit[k];
                    }
                    limit = limit.min(bound.maximum_task_speed);
                    feet.push(json!({"marker":marker.id,"coordinate_names":names,
                        "limiting_coordinate":names[bound.limiting_coordinate],"bound":bound}));
                }
                let loaded = support.iter().map(|s| {
                    let torques = s["required_motor_torques_nm"].as_array().unwrap();
                    let group = s["markers"].as_array().unwrap();
                    let mut bound = f64::INFINITY;
                    for marker in group {
                        for name in &recipe.marker_coordinates[marker.as_str().unwrap()] {
                            let j = coords.iter().position(|c|c==name).unwrap();
                            let fraction = torques[j].as_f64().unwrap().abs()/recipe.actuators[name]["stall_torque"];
                            let budget = if fraction>1.0 {0.0} else if rates[j]*torques[j].as_f64().unwrap()>0.0 {budgets[j]*(1.0-fraction)} else {budgets[j]};
                            if rates[j].abs()>1e-12 { bound = bound.min(budget/rates[j].abs()); }
                        }
                    }
                    json!({"support_markers":group,"constant_static_torque_stance_rate_budget_m_s":bound,
                        "balanced_candidate":s["allocation"]["scaled_wrench_residual_norm_n"].as_f64().unwrap()<1e-8 && s["allocation"]["unilateral_friction_satisfied"]==true && s["maximum_static_stall_fraction"].as_f64().unwrap()<=1.0})
                }).collect::<Vec<_>>();
                // Frozen-Jacobian screening only. A periodic foot travels v*beta*T
                // relative to the base in each direction. A quintic WORLD return
                // of v*T has peak relative velocity v*(1.875/(1-beta)-1).
                directions.push(json!({"unit_direction_world":direction,"feet":feet,"static_load_screens":loaded,
                    "frozen_pose_stance_rate_budget_m_s":limit,
                    "frozen_pose_ideal_return_rate_budget_m_s":limit.min(limit*(1.0-recipe.duty_factor)/recipe.duty_factor),
                    "frozen_pose_quintic_return_rate_budget_m_s":limit.min(limit/(1.875/(1.0-recipe.duty_factor)-1.0))}));
            }
        }
        result.push(json!({"id":row.id,"sampled_geometry_valid":valid,"error":row.error,
            "authored_limit_violations":row.authored_limit_violations,"sampled_penetrations":row.sampled_penetrations,
            "directions":directions,"static_support":support}));
    }
    println!("{}",serde_json::to_string_pretty(&json!({"version":1,"provenance":recipe.provenance,
        "cad_sha256":markers.expected_cad_sha256,"sum_peak_positive_motor_power_w":power,
        "reference_cycle_rate_budget_speed_m_s":cycle_limit,"reference_cycle_interpolation":cycle.interpolation,"reference_cycle_coordinates":cycle_bounds,"reference_cycle_retiming":retiming,"rows":result,
        "scope":"Conditional shaft-rate budgets from the shared CAD closure Jacobian and effective actuator model. No-load speed is a design budget, not a hard backdrive limit. Frozen-pose return bounds omit varying geometry, lifting, acceleration, torque, friction and stability. Static support uses explicit point forces with contact disabled only for the audit: minimum-norm allocation is a candidate, not an optimized feasibility proof; contact height differences are reported, not resolved. Constant-static-torque rate screens omit velocity and acceleration loads. The declared polynomial joint-reference rate bound includes interior extrema and applies only to uniform time scaling of that reference. None is a global attainable speed proof."})).map_err(|e|e.to_string())?);
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1)
    }
}
