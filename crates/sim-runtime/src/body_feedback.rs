//! Bounded kinematic suggestions for stance-supported body translation.
//! This does not move poses or apply forces. A policy may add the suggested
//! angular correction to servo targets; the shared dynamics execute the result.
use crate::tracking::{Marker, validate_markers};
use nalgebra::{DMatrix, DVector, Vector3};
use serde::{Deserialize, Serialize};
use sim_domain_control::trajectory::{Trajectory, TrajectoryConfig};
use sim_domain_robot::{
    Articulated, Generalized,
    articulated::embedding::{EmbeddedPoint, RigidEmbedding},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BodyFeedbackConfig {
    pub expected_cad_sha256: String,
    /// Explicit export-world frame. Target positions are absolute COM metres.
    pub coordinate_frame: String,
    pub reference_link: String,
    pub support_markers: Vec<Marker>,
    pub position_world_m: TrajectoryConfig,
    /// Velocity-error coefficient in seconds; position-error coefficient is 1.
    pub velocity_damping_s: f64,
    /// Positive regularization for an angular-coordinate Jacobian, m/rad.
    pub damping_m_per_rad: f64,
    pub maximum_correction_rad: f64,
    /// Floor normal force at which a marker gets full least-squares weight.
    /// Zero/unloaded markers contribute no body-support correction.
    pub full_support_force_n: f64,
}

pub struct BodyFeedback {
    config: BodyFeedbackConfig,
    reference: usize,
    points: Vec<EmbeddedPoint>,
    path: Trajectory,
}
#[derive(Debug, Serialize)]
pub struct BodyFeedbackSample {
    pub reference_time_s: f64,
    pub target_position_world_m: [f64; 3],
    pub actual_position_world_m: [f64; 3],
    pub position_error_world_m: [f64; 3],
    pub support_weights: Vec<f64>,
    pub correction_rad: Vec<f64>,
}
impl BodyFeedback {
    pub fn new(art: &Articulated, config: BodyFeedbackConfig) -> Result<Self, String> {
        validate_markers(&config.support_markers)?;
        if config.expected_cad_sha256.is_empty()
            || art.model.source["cad_sha256"].as_str() != Some(&config.expected_cad_sha256)
            || config.coordinate_frame.trim().is_empty()
            || !art.contact_on
            || art.gravity.x != 0.0
            || art.gravity.y != 0.0
            || art.gravity.z >= 0.0
            || !config.velocity_damping_s.is_finite()
            || config.velocity_damping_s < 0.0
            || [
                config.damping_m_per_rad,
                config.maximum_correction_rad,
                config.full_support_force_n,
            ]
            .iter()
            .any(|v| !v.is_finite() || *v <= 0.0)
        {
            return Err("body feedback requires explicit CAD/frame, world-Z floor and finite positive scales".into());
        }
        let find = |name: &str| -> Result<usize, String> {
            let ids: Vec<_> = art
                .links
                .iter()
                .enumerate()
                .filter(|(_, l)| l.name == name)
                .map(|(i, _)| i)
                .collect();
            if ids.len() == 1 {
                Ok(ids[0])
            } else {
                Err(format!("unique feedback link required: {name}"))
            }
        };
        let reference = find(&config.reference_link)?;
        if art.bases.len() != 1 || art.bases[0].link != reference || art.bases[0].grounded {
            return Err("body feedback reference must be the sole floating tree root".into());
        }
        let points = config
            .support_markers
            .iter()
            .map(|m| {
                Ok(EmbeddedPoint {
                    link: find(&m.link)?,
                    local_point_m: m.local_point_m,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        if points.iter().any(|p| p.link == reference)
            || points
                .iter()
                .map(|p| p.link)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != points.len()
        {
            return Err("one support marker per distinct non-root link required".into());
        }
        let path = Trajectory::new(config.position_world_m.clone())?;
        if path.dimension() != 3 || config.position_world_m.keyframes[0].time_s != 0.0 {
            return Err(
                "body reference requires three world metre coordinates starting at zero".into(),
            );
        }
        Ok(Self {
            config,
            reference,
            points,
            path,
        })
    }
    pub fn config(&self) -> &BodyFeedbackConfig {
        &self.config
    }
    pub fn sample(
        &self,
        art: &Articulated,
        map: &RigidEmbedding<'_>,
        g: &Generalized,
        phase: f64,
        advancing: bool,
    ) -> Result<BodyFeedbackSample, String> {
        let reference = self.path.sample(phase)?;
        let dofs = art.dofs().map(|(_, d)| d).collect::<Vec<_>>();
        if map.independent_joint_indices().iter().any(|&i| {
            !matches!(
                dofs[i].kind,
                sim_domain_robot::articulated::DofKind::Revolute
            )
        }) {
            return Err("body feedback correction coordinates must be angular".into());
        }
        let evaluation = art.evaluate(g);
        let body = &evaluation.links[self.reference];
        let error = Vector3::from_column_slice(&reference.values) - body.p;
        let target_velocity = if advancing {
            Vector3::from_column_slice(&reference.rates)
        } else {
            Vector3::zeros()
        };
        let correction = error + self.config.velocity_damping_s * (target_velocity - body.vel);
        let coordinates = map
            .independent_joint_indices()
            .iter()
            .map(|&i| g.q[i])
            .collect::<Vec<_>>();
        let (_, point_values) = map.point_jacobians(g, &coordinates, &self.points)?;
        let weights = self
            .points
            .iter()
            .map(|p| {
                let normal: f64 = evaluation
                    .contacts
                    .iter()
                    .filter(|c| c.link == p.link && c.other.is_none())
                    .map(|c| c.force.z)
                    .sum();
                (normal / self.config.full_support_force_n).clamp(0.0, 1.0)
            })
            .collect::<Vec<_>>();
        let jacobians = point_values.into_iter().map(|(_, j)| j).collect::<Vec<_>>();
        // With planted feet, their displacement relative to a fixed body must
        // oppose the requested body displacement. World axes match the Jacobian.
        let corrections = stance_correction(
            &jacobians,
            &weights,
            -correction,
            self.config.damping_m_per_rad,
            self.config.maximum_correction_rad,
        )?;
        Ok(BodyFeedbackSample {
            reference_time_s: phase,
            target_position_world_m: Vector3::from_column_slice(&reference.values).into(),
            actual_position_world_m: body.p.into(),
            position_error_world_m: error.into(),
            support_weights: weights,
            correction_rad: corrections,
        })
    }
}

/// Weighted damped least squares, with a common scale preserving correction
/// direction when the largest angular coordinate reaches the declared cap.
/// All Jacobians must use the same angular coordinate order and world axes.
pub fn stance_correction(
    jacobians: &[DMatrix<f64>],
    weights: &[f64],
    foot_displacement: Vector3<f64>,
    damping: f64,
    maximum: f64,
) -> Result<Vec<f64>, String> {
    point_correction(
        jacobians,
        weights,
        &vec![foot_displacement; jacobians.len()],
        damping,
        maximum,
    )
}

/// General form: each point has its own desired world displacement.
pub fn point_correction(
    jacobians: &[DMatrix<f64>],
    weights: &[f64],
    displacements: &[Vector3<f64>],
    damping: f64,
    maximum: f64,
) -> Result<Vec<f64>, String> {
    let n = jacobians.first().map_or(0, |j| j.ncols());
    if n == 0
        || weights.len() != jacobians.len()
        || displacements.len() != jacobians.len()
        || !damping.is_finite()
        || damping <= 0.0
        || !maximum.is_finite()
        || maximum <= 0.0
        || displacements
            .iter()
            .any(|d| d.iter().any(|v| !v.is_finite()))
        || weights
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        || jacobians
            .iter()
            .any(|j| j.nrows() != 3 || j.ncols() != n || j.iter().any(|v| !v.is_finite()))
    {
        return Err("invalid stance Jacobian, support weights or correction scales".into());
    }
    let mut normal = DMatrix::identity(n, n) * damping.powi(2);
    let mut rhs = DVector::zeros(n);
    for ((j, &weight), displacement) in jacobians.iter().zip(weights).zip(displacements) {
        normal += j.transpose() * j * weight;
        rhs += j.transpose() * displacement * weight;
    }
    let mut result = normal
        .cholesky()
        .ok_or("stance correction factorization failed")?
        .solve(&rhs);
    if result.iter().any(|x| !x.is_finite()) {
        return Err("nonfinite stance correction".into());
    }
    let largest = result.amax();
    if largest > maximum {
        result *= maximum / largest;
    }
    Ok(result.as_slice().to_vec())
}
