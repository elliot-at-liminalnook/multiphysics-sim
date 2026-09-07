//! Static support geometry in an explicitly chosen horizontal plane.
//! These diagnostics do not infer contact, friction capacity or dynamic balance.
use serde::Serialize;

/// Privileged runtime observation, not a claim that the CAD robot has force
/// sensors. World -Z gravity is required; internal contacts are excluded.
pub fn ideal_upward_floor_forces(
    art: &sim_domain_robot::Articulated,
    state: &sim_domain_robot::Generalized,
    names: &[String],
) -> Result<Vec<f64>, String> {
    let gravity = art.model.gravity;
    if names.is_empty()
        || names
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != names.len()
        || gravity.iter().any(|v| !v.is_finite())
        || gravity[0] != 0.0
        || gravity[1] != 0.0
        || gravity[2] >= 0.0
    {
        return Err("unique support links and finite world -Z gravity required".into());
    }
    let indices = names
        .iter()
        .map(|name| {
            let found: Vec<_> = art
                .links
                .iter()
                .enumerate()
                .filter(|(_, l)| &l.name == name)
                .map(|(i, _)| i)
                .collect();
            if found.len() != 1 {
                Err(format!("unique support link required: {name}"))
            } else {
                Ok(found[0])
            }
        })
        .collect::<Result<Vec<_>, String>>()?;
    let evaluation = art.evaluate(state);
    let values: Vec<_> = indices
        .iter()
        .map(|i| {
            evaluation
                .contacts
                .iter()
                .filter(|c| c.other.is_none() && c.link == *i)
                .map(|c| c.force.z)
                .sum::<f64>()
        })
        .collect();
    if values.iter().any(|v| !v.is_finite()) {
        return Err("nonfinite ideal support force".into());
    }
    Ok(values)
}

#[derive(Clone, Copy)]
pub struct MassPoint {
    pub mass_kg: f64,
    pub position_m: [f64; 3],
}

pub fn center_of_mass(points: &[MassPoint]) -> Result<[f64; 3], String> {
    if points.is_empty()
        || points.iter().any(|p| {
            !p.mass_kg.is_finite()
                || p.mass_kg <= 0.0
                || p.position_m.iter().any(|v| !v.is_finite())
        })
    {
        return Err("finite positions and positive masses required".into());
    }
    let total: f64 = points.iter().map(|p| p.mass_kg).sum();
    if !total.is_finite() {
        return Err("nonfinite total mass".into());
    }
    let com = std::array::from_fn(|i| {
        points
            .iter()
            .map(|p| p.mass_kg / total * p.position_m[i])
            .sum::<f64>()
    });
    if com.iter().any(|v| !v.is_finite()) {
        return Err("nonfinite center of mass".into());
    }
    Ok(com)
}

#[derive(Debug, Serialize)]
pub struct SupportMargin {
    /// Counterclockwise convex hull in caller's horizontal metre frame.
    pub hull_m: Vec<[f64; 2]>,
    /// Minimum inward signed distance to hull edge supporting lines. Positive
    /// inside, negative outside. Outside magnitude is not Euclidean distance
    /// to the polygon when the nearest point is a corner.
    pub minimum_edge_margin_m: f64,
}

#[derive(Serialize)]
pub struct StaticSupportGeometry {
    pub center_of_mass_world_m: [f64; 3],
    pub total_mass_kg: f64,
    pub assumed_support_marker_ids: Vec<String>,
    pub projected_margin: SupportMargin,
    /// Unique vertical point-load solution only when exactly three supports
    /// are specified. Values correspond to assumed_support_marker_ids and may
    /// be negative (an infeasible request for a unilateral ground support).
    pub vertical_point_forces_n: Option<[f64; 3]>,
    pub support_points_world_xy_m: Vec<[f64; 2]>,
    pub scope: &'static str,
}

/// Caller selects the assumed supports. No contact is inferred from that list.
/// Uses compiled masses of links not authored as world-ground objects, and COM
/// poses from the matching model. Only vertical gravity is supported here.
pub fn static_support_geometry(
    art: &sim_domain_robot::Articulated,
    poses: &[crate::session::LinkPose],
    supports: &[crate::tracking::Marker],
) -> Result<StaticSupportGeometry, String> {
    crate::tracking::validate_markers(supports)?;
    let g = art.model.gravity;
    if g.iter().any(|v| !v.is_finite()) || g[0] != 0.0 || g[1] != 0.0 || g[2] >= 0.0 {
        return Err("static XY support requires finite gravity along world -Z".into());
    }
    let pose = |name: &str| -> Result<&crate::session::LinkPose, String> {
        let found: Vec<_> = poses.iter().filter(|p| p.name == name).collect();
        if found.len() != 1 || !found[0].valid_rigid_transform() {
            return Err(format!(
                "missing/invalid/ambiguous COM or support pose: {name}"
            ));
        }
        Ok(found[0])
    };
    let mut masses = vec![];
    for link in art.model.links.iter().filter(|l| !l.ground) {
        let compiled: Vec<_> = art.links.iter().filter(|l| l.name == link.name).collect();
        if compiled.len() != 1 {
            return Err("ambiguous compiled mass link".into());
        }
        masses.push(MassPoint {
            mass_kg: compiled[0].mass,
            position_m: pose(&link.name)?.position_m,
        });
    }
    let center = center_of_mass(&masses)?;
    let mut points = vec![];
    for marker in supports {
        if art.links.iter().filter(|l| l.name == marker.link).count() != 1 {
            return Err(format!(
                "missing/ambiguous compiled support link: {}",
                marker.link
            ));
        }
        let p = pose(&marker.link)?;
        let world: [f64; 2] = std::array::from_fn(|i| {
            p.position_m[i]
                + (0..3)
                    .map(|j| p.rotation[i][j] * marker.local_point_m[j])
                    .sum::<f64>()
        });
        points.push(world);
    }
    Ok(StaticSupportGeometry {
        center_of_mass_world_m: center,
        total_mass_kg: masses.iter().map(|p| p.mass_kg).sum(),
        assumed_support_marker_ids: supports.iter().map(|m| m.id.clone()).collect(),
        projected_margin: projected_support_margin([center[0], center[1]], &points)?,
        vertical_point_forces_n: if points.len() == 3 {
            Some(vertical_tripod_forces(
                [center[0], center[1]],
                masses.iter().map(|p| p.mass_kg).sum::<f64>() * (-g[2]),
                &[points[0], points[1], points[2]],
            )?)
        } else {
            None
        },
        support_points_world_xy_m: points,
        scope: "Static COM projection against explicitly assumed point supports in world XY. Supports are not confirmed by this calculation. Excludes authored world-ground objects; ignores foot patch extent, acceleration, friction, torque capacity and uneven terrain wrench feasibility.",
    })
}

/// Static vertical force/moment balance about three noncollinear point supports.
/// Negative reactions are reported, not clipped into an apparently feasible plan.
pub fn vertical_tripod_forces(
    com_xy_m: [f64; 2],
    weight_n: f64,
    supports: &[[f64; 2]; 3],
) -> Result<[f64; 3], String> {
    if !weight_n.is_finite()
        || weight_n <= 0.0
        || com_xy_m
            .iter()
            .chain(supports.iter().flatten())
            .any(|v| !v.is_finite())
    {
        return Err("finite points and positive weight required".into());
    }
    let sub = |a: [f64; 2], b: [f64; 2]| [a[0] - b[0], a[1] - b[1]];
    let b = sub(supports[1], supports[0]);
    let c = sub(supports[2], supports[0]);
    let p = sub(com_xy_m, supports[0]);
    let scale = b[0].abs().max(b[1].abs()).max(c[0].abs()).max(c[1].abs());
    if !scale.is_finite() || scale == 0.0 {
        return Err("distinct noncollinear supports required".into());
    }
    let cross = |a: [f64; 2], b: [f64; 2]| a[0] * b[1] - a[1] * b[0];
    let b = b.map(|v| v / scale);
    let c = c.map(|v| v / scale);
    let p = p.map(|v| v / scale);
    let determinant = cross(b, c);
    if determinant.abs() < 1e-12 {
        return Err("near-collinear vertical support system".into());
    }
    let v = cross(p, c) / determinant;
    let w = cross(b, p) / determinant;
    let forces = [(1.0 - v - w) * weight_n, v * weight_n, w * weight_n];
    if forces.iter().any(|f| !f.is_finite()) {
        return Err("nonfinite vertical support solution".into());
    }
    Ok(forces)
}

#[derive(Debug, Serialize)]
pub struct MinimumSupportShift {
    pub original_forces_n: [f64; 3],
    pub target_com_xy_m: [f64; 2],
    pub shift_xy_m: [f64; 2],
    pub target_forces_n: [f64; 3],
}

/// Nearest COM projection satisfying the three specified minimum vertical loads.
/// This is a planning target under a static point-support model, not an applied
/// translation, controller action, or dynamic/contact feasibility certificate.
pub fn minimum_support_shift(
    com: [f64; 2],
    weight: f64,
    supports: &[[f64; 2]; 3],
    minimum: [f64; 3],
) -> Result<MinimumSupportShift, String> {
    let original = vertical_tripod_forces(com, weight, supports)?;
    let total: f64 = minimum.iter().sum();
    if minimum.iter().any(|v| !v.is_finite() || *v < 0.0) || !total.is_finite() || total > weight {
        return Err(
            "finite nonnegative minimum loads whose sum does not exceed weight required".into(),
        );
    }
    let anchor: [f64; 2] = std::array::from_fn(|k| {
        supports[0][k]
            + (0..3)
                .map(|i| minimum[i] / weight * (supports[i][k] - supports[0][k]))
                .sum::<f64>()
    });
    let remaining = 1.0 - total / weight;
    let vertices: [[f64; 2]; 3] =
        supports.map(|p| std::array::from_fn(|k| anchor[k] + remaining * (p[k] - supports[0][k])));
    let mut target = com;
    if !original.iter().zip(minimum).all(|(f, m)| *f >= m) {
        let mut best = f64::INFINITY;
        for i in 0..3 {
            let a = vertices[i];
            let b = vertices[(i + 1) % 3];
            let delta = [b[0] - a[0], b[1] - a[1]];
            let length2 = delta[0] * delta[0] + delta[1] * delta[1];
            let t = if length2 > 0.0 {
                (((com[0] - a[0]) * delta[0] + (com[1] - a[1]) * delta[1]) / length2)
                    .clamp(0.0, 1.0)
            } else {
                0.0
            };
            let p = [a[0] + t * delta[0], a[1] + t * delta[1]];
            let distance = (p[0] - com[0]).hypot(p[1] - com[1]);
            if distance < best {
                best = distance;
                target = p;
            }
        }
    }
    let forces = vertical_tripod_forces(target, weight, supports)?;
    if forces
        .iter()
        .zip(minimum)
        .any(|(f, m)| *f < m - 1e-10 * weight)
    {
        return Err("minimum-load projection failed its force audit".into());
    }
    Ok(MinimumSupportShift {
        original_forces_n: original,
        target_com_xy_m: target,
        shift_xy_m: [target[0] - com[0], target[1] - com[1]],
        target_forces_n: forces,
    })
}

pub fn projected_support_margin(
    point: [f64; 2],
    supports: &[[f64; 2]],
) -> Result<SupportMargin, String> {
    if point
        .iter()
        .chain(supports.iter().flatten())
        .any(|v| !v.is_finite())
    {
        return Err("finite support geometry required".into());
    }
    let mut points = supports.to_vec();
    points.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    points.dedup();
    let cross = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    };
    let mut lower: Vec<[f64; 2]> = vec![];
    let mut upper: Vec<[f64; 2]> = vec![];
    for (chain, values) in [
        (&mut lower, points.clone()),
        (&mut upper, points.iter().rev().copied().collect()),
    ] {
        for p in values {
            while chain.len() >= 2
                && cross(chain[chain.len() - 2], chain[chain.len() - 1], p) <= 0.0
            {
                chain.pop();
            }
            chain.push(p);
        }
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    if lower.len() < 3 {
        return Err("at least three noncollinear point supports required".into());
    }
    let mut margin = f64::INFINITY;
    for i in 0..lower.len() {
        let a = lower[i];
        let b = lower[(i + 1) % lower.len()];
        let d = cross(a, b, point) / (b[0] - a[0]).hypot(b[1] - a[1]);
        if !d.is_finite() {
            return Err("nonfinite support edge distance".into());
        }
        margin = margin.min(d);
    }
    Ok(SupportMargin {
        hull_m: lower,
        minimum_edge_margin_m: margin,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tripod_balance_and_minimum_load_projection_have_analytic_solutions() {
        let points = [[0.0, 0.0], [2.0, 0.0], [0.0, 2.0]];
        assert_eq!(
            vertical_tripod_forces([0.5, 0.5], 100.0, &points).unwrap(),
            [50.0, 25.0, 25.0]
        );
        assert_eq!(
            vertical_tripod_forces([2.0, 2.0], 100.0, &points).unwrap(),
            [-100.0, 100.0, 100.0]
        );
        let plan = minimum_support_shift([0.5, 0.5], 100.0, &points, [20.0, 20.0, 40.0]).unwrap();
        assert!((plan.shift_xy_m[0]).abs() < 1e-12);
        assert!((plan.shift_xy_m[1] - 0.3).abs() < 1e-12);
        for (a, b) in plan.target_forces_n.iter().zip([35.0, 25.0, 40.0]) {
            assert!((a - b).abs() < 1e-12);
        }
        let fixed = minimum_support_shift([0.5, 0.5], 100.0, &points, [10.0, 20.0, 70.0]).unwrap();
        assert!((fixed.target_com_xy_m[0] - 0.4).abs() < 1e-12);
        assert!((fixed.target_com_xy_m[1] - 1.4).abs() < 1e-12);
        let shifted = points.map(|p| [p[0] + 100.0, p[1] - 20.0]);
        let same =
            minimum_support_shift([100.5, -19.5], 100.0, &shifted, [20.0, 20.0, 40.0]).unwrap();
        assert!((same.shift_xy_m[1] - plan.shift_xy_m[1]).abs() < 1e-12);
        assert!(minimum_support_shift([0.0, 0.0], 100.0, &points, [40.0; 3]).is_err());
        assert!(
            vertical_tripod_forces([0.0, 0.0], 100.0, &[[0.0, 0.0], [1.0, 1.0], [2.0, 2.0]])
                .is_err()
        );
    }
    #[test]
    fn ideal_floor_observation_uses_named_supports_and_known_static_load() {
        use sim_core::Behavior;
        let model = serde_json::from_value(serde_json::json!({"gravity":[0,0,-9.81],
            "world":{"floor_z":0,"floor_stiffness":1000,"floor_damping":10},
            "links":[{"name":"loaded","com":[0,0,-0.002]},
                {"name":"airborne","com":[1,0,0.01]}]}))
        .unwrap();
        let mut art = sim_domain_robot::Articulated::new(
            std::sync::Arc::new(model),
            &sim_domain_robot::Options {
                contact: true,
                flex: false,
                ..Default::default()
            },
        )
        .unwrap();
        for link in &mut art.links {
            link.contact = vec![nalgebra::Vector3::zeros()];
        }
        let state = art.generalized(
            art.states().iter().map(|s| s.initial).collect(),
            vec![0.0; art.state_count],
            &[],
            vec![],
        );
        let names = vec!["airborne".into(), "loaded".into()];
        let forces = ideal_upward_floor_forces(&art, &state, &names).unwrap();
        assert!((forces[0]).abs() < 1e-12);
        assert!((forces[1] - 2.0).abs() < 1e-12);
        assert!(ideal_upward_floor_forces(&art, &state, &["missing".into()]).is_err());
        assert!(
            ideal_upward_floor_forces(&art, &state, &["loaded".into(), "loaded".into()]).is_err()
        );
    }
    #[test]
    fn triangle_margin_has_known_sign_distance_and_handles_unordered_duplicates() {
        let supports = [[0.0, 0.0], [2.0, 0.0], [0.0, 2.0], [0.1, 0.1], [2.0, 0.0]];
        let inside = projected_support_margin([0.5, 0.5], &supports).unwrap();
        assert_eq!(inside.hull_m.len(), 3);
        assert!((inside.minimum_edge_margin_m - 0.5).abs() < 1e-14);
        assert_eq!(
            projected_support_margin([1.0, 1.0], &supports)
                .unwrap()
                .minimum_edge_margin_m,
            0.0
        );
        assert!(
            (projected_support_margin([2.0, 2.0], &supports)
                .unwrap()
                .minimum_edge_margin_m
                + 2.0_f64.sqrt())
            .abs()
                < 1e-14
        );
        let translated = supports.map(|p| [p[0] + 10.0, p[1] - 20.0]);
        assert!(
            (projected_support_margin([10.5, -19.5], &translated)
                .unwrap()
                .minimum_edge_margin_m
                - 0.5)
                .abs()
                < 1e-14
        );
        assert!(
            projected_support_margin([0.0, 0.0], &[[0.0, 0.0], [1.0, 1.0], [2.0, 2.0]]).is_err()
        );
        assert!(projected_support_margin([f64::NAN, 0.0], &supports).is_err());
        assert!(projected_support_margin([0.0, 0.0], &[]).is_err());
    }
    #[test]
    fn mass_center_uses_declared_weights_and_rejects_missing_or_invalid_mass() {
        let a = MassPoint {
            mass_kg: 1.0,
            position_m: [0.0, 2.0, 0.0],
        };
        let b = MassPoint {
            mass_kg: 3.0,
            position_m: [4.0, 2.0, -4.0],
        };
        assert_eq!(center_of_mass(&[a, b]).unwrap(), [3.0, 2.0, -3.0]);
        assert!(center_of_mass(&[]).is_err());
        assert!(center_of_mass(&[MassPoint { mass_kg: 0.0, ..a }]).is_err());
        assert!(
            center_of_mass(&[MassPoint {
                mass_kg: f64::NAN,
                ..a
            }])
            .is_err()
        );
    }
}
