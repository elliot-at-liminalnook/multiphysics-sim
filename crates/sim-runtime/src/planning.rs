//! Geometric marker paths compiled to shared motor-coordinate trajectories.
//! This is a local inverse-kinematics building block, not a walking controller.
use crate::{
    session::LinkPose,
    tracking::{validate_markers, CaptureConfig},
};
use nalgebra::Vector3;
use serde::{Deserialize, Serialize};
use sim_domain_control::trajectory::{Interpolation, Keyframe, Trajectory, TrajectoryConfig};
use sim_domain_robot::articulated::embedding::{
    CoordinateInterval, EmbeddedMotion, EmbeddedPoint, EmbeddingConfig, PlanePlacementConfig,
    PointTarget, RigidEmbedding,
};
use sim_domain_robot::{Articulated, Generalized};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarkerMotionConfig {
    /// Optional static vertical point-support requirements during declared phases.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub support_requirements: Vec<MarkerSupportRequirement>,
    pub expected_cad_sha256: String,
    /// Exact CAD motor-coordinate order; no implicit name/index mapping.
    pub independent_coordinates: Vec<String>,
    pub initial_coordinates: Vec<f64>,
    #[serde(default)]
    pub initial_base_translation_m: Option<[f64; 3]>,
    /// Explicit search intervals, not inferred physical travel limits.
    pub bounds: Vec<CoordinateInterval>,
    #[serde(default)]
    pub embedding: EmbeddingConfig,
    #[serde(default)]
    pub placement: PlanePlacementConfig,
    pub sample_period_s: f64,
    /// Maximum geometric error allowed between compiled command knots.
    pub maximum_interpolation_error_m: f64,
    /// Flattened xyz displacements in export-world axes, in marker order.
    /// Zero at time zero; positions are relative to the initial closed pose.
    pub displacements_world_m: TrajectoryConfig,
    /// Optional desired floating-base translation relative to the initial pose.
    /// The compiler derives motor references with world marker targets held;
    /// actual execution must produce this motion through forces, not teleporting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_displacements_world_m: Option<TrajectoryConfig>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarkerSupportRequirement {
    /// Phase boundaries must lie on the inspected knot/midpoint grid.
    pub start_s: f64,
    pub end_s: f64,
    pub marker_ids: [String; 3],
    pub minimum_forces_n: [f64; 3],
}

#[derive(Serialize)]
pub struct MarkerSupportCheck {
    pub marker_ids: [String; 3],
    pub minimum_forces_n: [f64; 3],
    pub predicted_vertical_forces_n: [f64; 3],
}

#[derive(Serialize)]
pub struct MarkerPlanFrame {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub support_load_checks: Vec<MarkerSupportCheck>,
    pub time_s: f64,
    pub poses: Vec<LinkPose>,
    pub joint_positions: Vec<f64>,
    pub marker_positions_world_m: Vec<[f64; 3]>,
    pub maximum_marker_error_m: f64,
    pub minimum_scaled_singular_value: f64,
    pub maximum_scaled_closure_error: f64,
}

#[derive(Serialize)]
pub struct MarkerMotionPlan {
    pub version: u32,
    pub completed: bool,
    pub source: serde_json::Value,
    pub embedding: EmbeddingConfig,
    pub independent_coordinates: Vec<String>,
    pub initial_coordinates: Vec<f64>,
    pub markers: CaptureConfig,
    pub config: MarkerMotionConfig,
    /// Piecewise-linear joint references consumed by the existing servo path.
    pub trajectory: TrajectoryConfig,
    /// Command knots and interval midpoints, including collision checks.
    pub frames: Vec<MarkerPlanFrame>,
    pub maximum_marker_error_m: f64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extra_inspection_times_s: Vec<f64>,
    pub scope: &'static str,
}

pub fn plan_marker_motion(
    art: &Articulated,
    seed: &Generalized,
    markers: &CaptureConfig,
    config: &MarkerMotionConfig,
) -> Result<MarkerMotionPlan, String> {
    plan_marker_motion_with_inspections(art, seed, markers, config, &[])
}

/// Inspect additional exact reference phases without changing the compiled
/// command knots. Uses the same closure solve, prescribed base motion, authored
/// limits and collision/support checks as the initial plan.
pub fn plan_marker_motion_with_inspections(
    art: &Articulated,
    seed: &Generalized,
    markers: &CaptureConfig,
    config: &MarkerMotionConfig,
    extra_times: &[f64],
) -> Result<MarkerMotionPlan, String> {
    validate_markers(&markers.markers)?;
    let hash = art.model.source.get("cad_sha256").and_then(|v| v.as_str());
    if config.expected_cad_sha256.is_empty()
        || hash != Some(config.expected_cad_sha256.as_str())
        || markers.expected_cad_sha256.as_deref() != hash
        || markers.coordinate_frame.trim().is_empty()
    {
        return Err("planner and markers require the same explicit CAD hash/world frame".into());
    }
    if !art.contact_on {
        return Err("marker planning requires enabled collision inspection".into());
    }
    let motor_coordinates = art
        .model
        .motors
        .iter()
        .map(|m| {
            let joint = m.joint.as_ref().ok_or("motor missing joint")?;
            let names: Vec<_> = art
                .dofs()
                .filter(|(j, _)| &j.name == joint)
                .map(|(_, d)| d.name.clone())
                .collect();
            if names.len() != 1 {
                return Err(format!("motor {} needs one coordinate", m.name));
            }
            Ok(names[0].clone())
        })
        .collect::<Result<Vec<_>, String>>()?;
    if motor_coordinates.is_empty() || config.independent_coordinates != motor_coordinates {
        return Err("planner coordinates must match exact CAD motor order".into());
    }
    let path = Trajectory::new(config.displacements_world_m.clone())?;
    let knots = &config.displacements_world_m.keyframes;
    let duration = knots.last().unwrap().time_s;
    if extra_times
        .iter()
        .any(|t| !t.is_finite() || *t < 0.0 || *t > duration)
    {
        return Err("extra inspection times must be finite and within the plan duration".into());
    }
    for requirement in &config.support_requirements {
        if !requirement.start_s.is_finite()
            || !requirement.end_s.is_finite()
            || requirement.start_s < 0.0
            || requirement.end_s < requirement.start_s
            || requirement.end_s > duration
            || requirement
                .marker_ids
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != 3
            || requirement
                .marker_ids
                .iter()
                .any(|id| markers.markers.iter().filter(|m| &m.id == id).count() != 1)
            || requirement
                .minimum_forces_n
                .iter()
                .any(|f| !f.is_finite() || *f < 0.0)
            || [requirement.start_s, requirement.end_s].iter().any(|t| {
                let tick = 2.0 * t / config.sample_period_s;
                !tick.is_finite() || (tick - tick.round()).abs() > 1e-8
            })
        {
            return Err("support requirements need ordered in-range knot/midpoint times, three unique known markers and nonnegative finite loads".into());
        }
    }
    let base_path = config
        .base_displacements_world_m
        .clone()
        .map(Trajectory::new)
        .transpose()?;
    if let Some(base) = &config.base_displacements_world_m {
        if art.bases.len() != 1
            || art.bases[0].grounded
            || base_path.as_ref().unwrap().dimension() != 3
            || base.keyframes[0].time_s != 0.0
            || base.keyframes[0].values.iter().any(|v| *v != 0.0)
            || base.keyframes.last().unwrap().time_s != duration
            || base.keyframes.iter().any(|k| {
                let n = k.time_s / config.sample_period_s;
                !n.is_finite() || (n - n.round()).abs() > 1e-8
            })
        {
            return Err("base path requires one floating base, zero initial xyz, matching duration and knot grid".into());
        }
    }
    let intervals = duration / config.sample_period_s;
    if path.dimension() != markers.markers.len() * 3
        || knots[0].time_s != 0.0
        || knots[0].values.iter().any(|v| *v != 0.0)
        || !config.sample_period_s.is_finite()
        || config.sample_period_s <= 0.0
        || !intervals.is_finite()
        || !(1.0..=10000.0).contains(&intervals)
        || (intervals - intervals.round()).abs() > 1e-8
        || knots.iter().any(|k| {
            let n = k.time_s / config.sample_period_s;
            (n - n.round()).abs() > 1e-8
        })
        || !config.maximum_interpolation_error_m.is_finite()
        || config.maximum_interpolation_error_m <= 0.0
    {
        return Err(
            "invalid marker path dimension, zero origin, time grid or interpolation tolerance"
                .into(),
        );
    }
    let map = RigidEmbedding::new(art, &motor_coordinates, config.embedding.clone())?;
    let points = markers
        .markers
        .iter()
        .map(|m| {
            let links: Vec<_> = art
                .links
                .iter()
                .enumerate()
                .filter(|(_, l)| l.name == m.link)
                .map(|(i, _)| i)
                .collect();
            if links.len() != 1 {
                return Err(format!("missing/ambiguous marker link {}", m.link));
            }
            Ok(EmbeddedPoint {
                link: links[0],
                local_point_m: m.local_point_m,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut seed = seed.clone();
    if let Some(delta) = config.initial_base_translation_m {
        if art.bases.len() != 1 || art.bases[0].grounded || delta.iter().any(|v| !v.is_finite()) {
            return Err("initial translation requires one floating base and finite metres".into());
        }
        for (k, d) in delta.iter().enumerate() {
            seed.states[art.bases[0].state + k] += d;
        }
    }
    let zero = vec![0.0; map.reduced_dimension()];
    let set_base = |g: &mut Generalized, t: f64| -> Result<(), String> {
        if let Some(path) = &base_path {
            let base = art.bases[0].state;
            let offset = path.sample(t)?.values;
            for k in 0..3 {
                g.states[base + k] = seed.states[base + k] + offset[k];
            }
        }
        Ok(())
    };
    let initial = map.solve(&seed, &config.initial_coordinates, &zero)?;
    let world_points = |g: &Generalized| -> Vec<Vector3<f64>> {
        let poses = art.poses(g);
        points
            .iter()
            .map(|p| {
                let (r, x) = &poses[p.link];
                x + r * Vector3::from(p.local_point_m)
            })
            .collect()
    };
    let origins = world_points(&initial.generalized);
    let targets = |t: f64| -> Result<Vec<PointTarget>, String> {
        let offset = path.sample(t)?.values;
        Ok(points
            .iter()
            .zip(&origins)
            .enumerate()
            .map(|(i, (point, p))| PointTarget {
                point: point.clone(),
                position_world_m: std::array::from_fn(|k| p[k] + offset[3 * i + k]),
            })
            .collect())
    };
    let inspect = |t: f64, motion: &EmbeddedMotion| -> Result<MarkerPlanFrame, String> {
        for (i, (_, d)) in art.dofs().enumerate() {
            if d.lower.is_some_and(|lo| motion.generalized.q[i] < lo)
                || d.upper.is_some_and(|hi| motion.generalized.q[i] > hi)
            {
                return Err(format!("authored limit {} violated at {t}s", d.name));
            }
        }
        if let Some(c) = art
            .evaluate(&motion.generalized)
            .contacts
            .iter()
            .find(|c| c.other.is_some() && c.penetration > 0.0)
        {
            return Err(format!(
                "internal contact at {t}s: {} / {}, penetration {} m",
                art.links[c.link].name,
                art.links[c.other.unwrap()].name,
                c.penetration
            ));
        }
        let actual = world_points(&motion.generalized);
        let target = targets(t)?;
        let error = actual
            .iter()
            .zip(&target)
            .map(|(a, b)| (a - Vector3::from(b.position_world_m)).norm())
            .fold(0.0_f64, f64::max);
        if !error.is_finite() || error > config.maximum_interpolation_error_m {
            return Err(format!(
                "marker path error {error} m at {t}s exceeds interpolation allowance"
            ));
        }
        let poses: Vec<LinkPose> = art
            .poses(&motion.generalized)
            .iter()
            .zip(&art.links)
            .map(|((r, p), l)| LinkPose {
                name: l.name.clone(),
                position_m: (*p).into(),
                rotation: std::array::from_fn(|i| std::array::from_fn(|j| r[(i, j)])),
            })
            .collect();
        let mut support_load_checks = vec![];
        for requirement in config
            .support_requirements
            .iter()
            .filter(|r| t >= r.start_s && t <= r.end_s)
        {
            let selected: Vec<_> = requirement
                .marker_ids
                .iter()
                .map(|id| {
                    markers
                        .markers
                        .iter()
                        .find(|m| &m.id == id)
                        .unwrap()
                        .clone()
                })
                .collect();
            let support = crate::support::static_support_geometry(art, &poses, &selected)?;
            let forces = support
                .vertical_point_forces_n
                .ok_or("missing tripod load solution")?;
            if forces
                .iter()
                .zip(requirement.minimum_forces_n)
                .any(|(f, min)| *f < min)
            {
                return Err(format!(
                    "static vertical support minimum failed at {t}s: markers {:?}, predicted {:?} N, required {:?} N",
                    requirement.marker_ids, forces, requirement.minimum_forces_n
                ));
            }
            support_load_checks.push(MarkerSupportCheck {
                marker_ids: requirement.marker_ids.clone(),
                minimum_forces_n: requirement.minimum_forces_n,
                predicted_vertical_forces_n: forces,
            });
        }
        Ok(MarkerPlanFrame {
            support_load_checks,
            time_s: t,
            poses,
            joint_positions: motion.generalized.q.clone(),
            marker_positions_world_m: actual.into_iter().map(Into::into).collect(),
            maximum_marker_error_m: error,
            minimum_scaled_singular_value: motion.minimum_scaled_singular_value,
            maximum_scaled_closure_error: motion.maximum_scaled_position_error,
        })
    };
    let mut motion = initial;
    let mut commands = Vec::new();
    let mut frames = Vec::new();
    for i in 0..=intervals.round() as usize {
        let t = duration * i as f64 / intervals.round();
        set_base(&mut motion.generalized, t)?;
        let fit = map
            .place_points(
                &motion.generalized,
                &targets(t)?,
                &config.bounds,
                &config.placement,
            )
            .map_err(|e| format!("marker placement at {t}s: {e}"))?;
        frames.push(inspect(t, &fit.motion)?);
        commands.push(Keyframe {
            time_s: t,
            values: fit.coordinates,
        });
        motion = fit.motion;
    }
    let trajectory = TrajectoryConfig {
        interpolation: Interpolation::Linear,
        keyframes: commands,
    };
    let command = Trajectory::new(trajectory.clone())?;
    for pair in trajectory.keyframes.windows(2) {
        let t = (pair[0].time_s + pair[1].time_s) * 0.5;
        // Continue from a nearby solved branch; midpoint commands use exactly
        // the same linear sampler as motor execution, not a separate animation.
        let mut g = map.solve(&seed, &pair[0].values, &zero)?.generalized;
        set_base(&mut g, t)?;
        let mid = map.solve(&g, &command.sample(t)?.values, &zero)?;
        frames.push(inspect(t, &mid)?);
    }
    for &t in extra_times {
        if frames.iter().any(|f| (f.time_s - t).abs() <= 1e-10) {
            continue;
        }
        let previous = trajectory
            .keyframes
            .iter()
            .rev()
            .find(|k| k.time_s <= t)
            .unwrap();
        let mut g = map.solve(&seed, &previous.values, &zero)?.generalized;
        set_base(&mut g, t)?;
        let sample = map.solve(&g, &command.sample(t)?.values, &zero)?;
        frames.push(inspect(t, &sample)?);
    }
    frames.sort_by(|a, b| a.time_s.total_cmp(&b.time_s));
    let maximum_marker_error_m = frames
        .iter()
        .map(|f| f.maximum_marker_error_m)
        .fold(0.0_f64, f64::max);
    Ok(MarkerMotionPlan {
        version: 1,
        completed: true,
        source: art.model.source.clone(),
        embedding: config.embedding.clone(),
        independent_coordinates: motor_coordinates,
        initial_coordinates: config.initial_coordinates.clone(),
        markers: markers.clone(),
        config: config.clone(),
        trajectory,
        frames,
        maximum_marker_error_m,
        extra_inspection_times_s: extra_times.to_vec(),
        scope: "Local geometric reference with prescribed base translation, original linkage closure, authored limits and sampled internal collision checks. Knots, midpoints and requested inspection times only, not continuous clearance, balance, loaded tracking, actuator speed/torque feasibility or a walking policy. Execution applies motor commands; it must achieve base motion physically.",
    })
}
