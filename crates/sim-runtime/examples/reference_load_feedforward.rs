//! Reference load feedforward for an explicitly prescribed periodic support policy.
//! This is an actuator-target suggestion, never an applied external body force.
use nalgebra::Vector3;
use serde::Deserialize;
use serde_json::json;
use sim_domain_control::trajectory::{Interpolation, Trajectory, TrajectoryConfig};
use sim_domain_robot::{
    articulated::embedding::{EmbeddedPoint, EmbeddingConfig, RigidEmbedding},
    effective_servo::EffectiveServo,
    motion_capability::{
        constrained_point_forces, weighted_minimum_norm_point_forces, ConstrainedForceConfig,
    },
};
use sim_runtime::{
    session::{Scene, Session},
    tracking::CaptureConfig,
};
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    independent_coordinates: Vec<String>,
    embedding: EmbeddingConfig,
    initial_base_translation_m: [f64; 3],
    #[serde(default)]
    samples: Vec<Vec<f64>>,
    /// Alternative to literal static samples; derivatives use the shared curve.
    reference: Option<Reference>,
    /// Relative vertical support shares, one per marker for every sample.
    support_weights: Vec<Vec<f64>>,
    actuators: BTreeMap<String, BTreeMap<String, f64>>,
    /// Optional weighted minimum-norm force/moment allocation. Otherwise retain
    /// original net-force shares exactly. Neither mode enforces contact forces.
    #[serde(default)]
    wrench_allocation: Option<WrenchAllocation>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WrenchAllocation {
    length_scale_m: f64,
    friction_coefficient: f64,
    #[serde(default)]
    constrained: Option<ConstrainedForceConfig>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    trajectory: TrajectoryConfig,
    sample_times_s: Vec<f64>,
    phase_rate: f64,
    phase_acceleration_per_s: f64,
    /// World linear m/s then angular rad/s, in reduced floating-base order.
    base_velocity: [f64; 6],
    /// World linear m/s^2 then angular rad/s^2.
    base_acceleration: [f64; 6],
}
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 {
        return Err("usage: reference_load_feedforward scene.json markers.json recipe.json".into());
    }
    let read = |p: &str| std::fs::read(p).map_err(|e| e.to_string());
    let scene: Scene = serde_json::from_slice(&read(&args[0])?).map_err(|e| e.to_string())?;
    let markers: CaptureConfig =
        serde_json::from_slice(&read(&args[1])?).map_err(|e| e.to_string())?;
    let recipe: Recipe = serde_json::from_slice(&read(&args[2])?).map_err(|e| e.to_string())?;
    let dynamic = recipe.reference.is_some();
    let (samples, velocities, accelerations) = if let Some(reference) = &recipe.reference {
        if matches!(reference.trajectory.interpolation, Interpolation::Linear) {
            return Err("dynamic feedforward requires a reference with continuous velocity and acceleration".into());
        }
        if !recipe.samples.is_empty()
            || !reference.phase_rate.is_finite()
            || !reference.phase_acceleration_per_s.is_finite()
            || reference
                .base_velocity
                .iter()
                .chain(&reference.base_acceleration)
                .any(|v| !v.is_finite())
        {
            return Err("choose literal static samples or one finite dynamic reference".into());
        }
        let trajectory = Trajectory::new(reference.trajectory.clone())?;
        if trajectory.dimension() != recipe.independent_coordinates.len() {
            return Err("reference dimension must match independent coordinates".into());
        }
        let mut positions = Vec::new();
        let mut velocities = Vec::new();
        let mut accelerations = Vec::new();
        for &time in &reference.sample_times_s {
            let sample = trajectory.sample(time)?;
            let mut velocity = reference.base_velocity.to_vec();
            let mut acceleration = reference.base_acceleration.to_vec();
            for (&rate, &second) in sample.rates.iter().zip(&sample.accelerations) {
                velocity.push(rate * reference.phase_rate);
                acceleration.push(
                    second * reference.phase_rate * reference.phase_rate
                        + rate * reference.phase_acceleration_per_s,
                );
            }
            positions.push(sample.values);
            velocities.push(velocity);
            accelerations.push(acceleration);
        }
        (positions, velocities, accelerations)
    } else {
        let zeros = vec![vec![0.; recipe.independent_coordinates.len() + 6]; recipe.samples.len()];
        (recipe.samples.clone(), zeros.clone(), zeros)
    };
    if samples.is_empty()
        || samples.len() != recipe.support_weights.len()
        || recipe
            .initial_base_translation_m
            .iter()
            .any(|x| !x.is_finite())
        || recipe.support_weights.iter().any(|w| {
            w.len() != markers.markers.len()
                || w.iter().any(|v| !v.is_finite() || *v < 0.0)
                || !w.iter().sum::<f64>().is_finite()
                || w.iter().sum::<f64>() <= 0.0
        })
    {
        return Err(
            "finite coordinates/translation and nonnegative nonempty support shares required"
                .into(),
        );
    }
    let mut session = Session::new(scene, 0)?;
    if markers.coordinate_frame.is_empty()
        || markers.markers.is_empty()
        || markers
            .expected_cad_sha256
            .as_deref()
            .is_none_or(|hash| hash.is_empty())
        || markers.expected_cad_sha256.as_deref()
            != session.robot.art.model.source["cad_sha256"].as_str()
    {
        return Err("registered markers/frame and matching CAD hash required".into());
    }
    // Disable contact only in this analysis object, replacing it with the
    // explicitly prescribed point-force allocation. Runtime scene is unchanged.
    session.robot.art.contact_on = false;
    let art = &session.robot.art;
    let mut seed = session.robot.generalized();
    let map = RigidEmbedding::new(art, &recipe.independent_coordinates, recipe.embedding)?;
    if map.reduced_dimension() != recipe.independent_coordinates.len() + 6 {
        return Err("floating base required".into());
    }
    let base = art.bases[0].state;
    for i in 0..3 {
        seed.states[base + i] += recipe.initial_base_translation_m[i];
    }
    let mut ids = std::collections::BTreeSet::new();
    let points = markers
        .markers
        .iter()
        .map(|m| {
            let found: Vec<_> = art
                .links
                .iter()
                .enumerate()
                .filter(|(_, l)| l.name == m.link)
                .collect();
            if !ids.insert(&m.id) || found.len() != 1 {
                return Err("unique marker IDs and links required".to_owned());
            }
            Ok(EmbeddedPoint {
                link: found[0].0,
                local_point_m: m.local_point_m,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut stiffness = Vec::new();
    let mut motors = Vec::new();
    for name in &recipe.independent_coordinates {
        let p = recipe
            .actuators
            .get(name)
            .ok_or_else(|| format!("missing actuator {name}"))?;
        motors.push(EffectiveServo::new(p).map_err(|e| e.to_string())?);
        stiffness.push(p["stiffness"]);
    }
    let mut frames = Vec::new();
    let mut offsets = Vec::new();
    let mut dynamic_offsets = Vec::new();
    for (index, (q, weights)) in samples.iter().zip(&recipe.support_weights).enumerate() {
        let (static_motion, values) = map.point_jacobians(&seed, q, &points)?;
        let static_required = map
            .prepare_dynamics(&static_motion)?
            .required_reduced_forces(&vec![0.; map.reduced_dimension()])?;
        let required = if dynamic {
            let motion = map.solve(&seed, q, &velocities[index])?;
            map.prepare_dynamics(&motion)?
                .required_reduced_forces(&accelerations[index])?
        } else {
            static_required.clone()
        };
        let total = Vector3::new(required[0], required[1], required[2]);
        let sum = weights.iter().sum::<f64>();
        let reference = Vector3::new(
            seed.states[base],
            seed.states[base + 1],
            seed.states[base + 2],
        );
        let allocate = |load: &nalgebra::DVector<f64>| {
            recipe
                .wrench_allocation
                .as_ref()
                .map(|config| {
                    let positions = values
                        .iter()
                        .map(|(p, _)| [p[0], p[1], p[2]])
                        .collect::<Vec<_>>();
                    let center = [reference[0], reference[1], reference[2]];
                    let required = std::array::from_fn(|i| load[i]);
                    if let Some(constrained) = &config.constrained {
                        constrained_point_forces(
                            &positions,
                            center,
                            required,
                            config.length_scale_m,
                            config.friction_coefficient,
                            weights,
                            constrained,
                        )
                    } else {
                        weighted_minimum_norm_point_forces(
                            &positions,
                            center,
                            required,
                            config.length_scale_m,
                            config.friction_coefficient,
                            weights,
                        )
                    }
                })
                .transpose()
        };
        let allocation = allocate(&required)?;
        let static_allocation = allocate(&static_required)?;
        let forces = if let Some(allocation) = &allocation {
            allocation
                .forces_world_n
                .iter()
                .map(|f| Vector3::from(*f))
                .collect::<Vec<_>>()
        } else {
            weights
                .iter()
                .map(|w| total * (w / sum))
                .collect::<Vec<_>>()
        };
        let torques = (0..q.len())
            .map(|j| {
                required[6 + j]
                    - values
                        .iter()
                        .zip(&forces)
                        .map(|((_, jac), force)| {
                            (0..3).map(|k| jac[(k, j)] * force[k]).sum::<f64>()
                        })
                        .sum::<f64>()
            })
            .collect::<Vec<_>>();
        let residual = Vector3::new(required[3], required[4], required[5])
            - values
                .iter()
                .zip(&forces)
                .map(|((p, _), f)| (p - reference).cross(f))
                .fold(Vector3::zeros(), |a, b| a + b);
        offsets.push(
            torques
                .iter()
                .zip(&stiffness)
                .map(|(tau, k)| tau / k)
                .collect::<Vec<_>>(),
        );
        let static_force = Vector3::new(static_required[0], static_required[1], static_required[2]);
        let static_torques = (0..q.len())
            .map(|j| {
                if let Some(allocation) = &static_allocation {
                    return static_required[6 + j]
                        - values
                            .iter()
                            .zip(&allocation.forces_world_n)
                            .map(|((_, jac), force)| {
                                (0..3).map(|k| jac[(k, j)] * force[k]).sum::<f64>()
                            })
                            .sum::<f64>();
                }
                static_required[6 + j]
                    - values
                        .iter()
                        .zip(weights)
                        .map(|((_, jac), weight)| {
                            (0..3)
                                .map(|k| jac[(k, j)] * static_force[k] * (weight / sum))
                                .sum::<f64>()
                        })
                        .sum::<f64>()
            })
            .collect::<Vec<_>>();
        dynamic_offsets.push(
            torques
                .iter()
                .zip(&static_torques)
                .zip(&stiffness)
                .map(|((tau, base), k)| (tau - base) / k)
                .collect::<Vec<_>>(),
        );
        let mut frame = json!({"coordinates":q,"motor_torques_nm":torques,"reduced_velocity":velocities[index],"reduced_acceleration":accelerations[index],"support_forces_world_n":forces.iter().map(|f|[f[0],f[1],f[2]]).collect::<Vec<_>>(),"unbalanced_base_moment_nm":[residual[0],residual[1],residual[2]]});
        let force_residual = total - forces.iter().fold(Vector3::zeros(), |a, b| a + b);
        frame["unbalanced_base_force_n"] =
            json!([force_residual[0], force_residual[1], force_residual[2]]);
        frame["wrench_allocation"] = json!(allocation);
        frame["static_wrench_allocation"] = json!(static_allocation);
        let capacities = motors
            .iter()
            .zip(&torques)
            .enumerate()
            .map(|(j, (motor, torque))| motor.torque_capacity(velocities[index][6 + j], *torque))
            .collect::<Vec<_>>();
        let margins = capacities
            .iter()
            .zip(&torques)
            .map(|(capacity, torque)| capacity - torque.abs())
            .collect::<Vec<_>>();
        frame["signed_torque_capacity_nm"] = json!(capacities);
        frame["torque_capacity_margin_nm"] = json!(margins);
        if !dynamic {
            frame["static_motor_torques_nm"] = json!(torques);
        }
        frames.push(frame);
    }
    println!("{}",serde_json::to_string(&json!({"cad_sha256":markers.expected_cad_sha256,"coordinate_names":recipe.independent_coordinates,
        "dynamic_reference":dynamic,"weighted_wrench_allocation":recipe.wrench_allocation.is_some(),"target_offsets_rad":offsets,"dynamic_increment_offsets_rad":dynamic_offsets,"frames":frames,"scope":"Shared inverse dynamics at prescribed reference coordinates/velocities/accelerations, under caller-prescribed support availability. Dynamic increments subtract the static allocation at the same position. Default net-force shares retain prior behavior. Optional weighted minimum-norm allocation checks unilateral/friction inequalities; optional constrained optimization enforces those cones and reports convergence, with explicit force regularization. All remaining force and moment are reported and NOT secretly applied to the body. No actual contact, tracking or stability guarantee. Only tau/K offsets may be consumed by the bounded physical servo; signed rate, acceleration and base motion must match the controller use."})).map_err(|e|e.to_string())?);
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1)
    }
}
