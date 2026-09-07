//! Ideal world-marker feedback for teacher policies, through ordinary servo targets.
use crate::{
    body_feedback::point_correction,
    tracking::{Marker, validate_markers},
};
use nalgebra::Vector3;
use serde::{Deserialize, Serialize};
use sim_domain_control::trajectory::{Trajectory, TrajectoryConfig};
use sim_domain_robot::{
    Articulated, Generalized,
    articulated::embedding::{EmbeddedPoint, RigidEmbedding},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PointFeedbackConfig {
    pub expected_cad_sha256: String,
    pub coordinate_frame: String,
    pub markers: Vec<Marker>,
    /// Absolute world XYZ metres, concatenated in marker order.
    pub position_world_m: TrajectoryConfig,
    /// One dimensionless gain per marker, in [0,1]. Multiplies displacement
    /// before solving, so activation smoothly attenuates even isolated joints.
    pub activation: TrajectoryConfig,
    pub damping_m_per_rad: f64,
    pub maximum_correction_rad: f64,
}
pub struct PointFeedback {
    config: PointFeedbackConfig,
    points: Vec<EmbeddedPoint>,
    path: Trajectory,
    activation: Trajectory,
}
#[derive(Debug, Serialize)]
pub struct PointFeedbackSample {
    pub reference_time_s: f64,
    pub target_positions_world_m: Vec<[f64; 3]>,
    pub actual_positions_world_m: Vec<[f64; 3]>,
    pub position_errors_world_m: Vec<[f64; 3]>,
    pub activation: Vec<f64>,
    pub correction_rad: Vec<f64>,
}
impl PointFeedback {
    pub fn new(art: &Articulated, config: PointFeedbackConfig) -> Result<Self, String> {
        validate_markers(&config.markers)?;
        if config.expected_cad_sha256.is_empty()
            || art.model.source["cad_sha256"].as_str() != Some(&config.expected_cad_sha256)
            || config.coordinate_frame.trim().is_empty()
            || [config.damping_m_per_rad, config.maximum_correction_rad]
                .iter()
                .any(|x| !x.is_finite() || *x <= 0.0)
        {
            return Err(
                "point feedback requires explicit CAD/frame and positive finite scales".into(),
            );
        }
        let points = config
            .markers
            .iter()
            .map(|m| {
                let indices = art
                    .links
                    .iter()
                    .enumerate()
                    .filter(|(_, l)| l.name == m.link)
                    .map(|(i, _)| i)
                    .collect::<Vec<_>>();
                if indices.len() != 1 {
                    return Err(format!("unique feedback link required: {}", m.link));
                }
                Ok(EmbeddedPoint {
                    link: indices[0],
                    local_point_m: m.local_point_m,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let path = Trajectory::new(config.position_world_m.clone())?;
        let activation = Trajectory::new(config.activation.clone())?;
        if path.dimension() != 3 * points.len()
            || activation.dimension() != points.len()
            || config.position_world_m.keyframes[0].time_s != 0.0
            || config.activation.keyframes[0].time_s != 0.0
        {
            return Err(
                "point paths require XYZ and activation per marker, starting at zero".into(),
            );
        }
        activation.validate_value_bounds(&vec![(Some(0.0), Some(1.0)); points.len()])?;
        Ok(Self {
            config,
            points,
            path,
            activation,
        })
    }
    pub fn config(&self) -> &PointFeedbackConfig {
        &self.config
    }
    pub fn sample(
        &self,
        art: &Articulated,
        map: &RigidEmbedding<'_>,
        g: &Generalized,
        phase: f64,
    ) -> Result<PointFeedbackSample, String> {
        let dofs = art.dofs().map(|(_, d)| d).collect::<Vec<_>>();
        if map.independent_joint_indices().iter().any(|&i| {
            !matches!(
                dofs[i].kind,
                sim_domain_robot::articulated::DofKind::Revolute
            )
        }) {
            return Err("point feedback correction coordinates must be angular".into());
        }
        let reference = self.path.sample(phase)?;
        let activation = self.activation.sample(phase)?.values;
        let coordinates = map
            .independent_joint_indices()
            .iter()
            .map(|&i| g.q[i])
            .collect::<Vec<_>>();
        let (_, values) = map.point_jacobians(g, &coordinates, &self.points)?;
        // Evaluate actual committed poses, rather than silently substituting the
        // embedding's projected pose for the observed state.
        let links = art.evaluate_kinematics_only(g);
        let actual = self
            .points
            .iter()
            .map(|p| links[p.link].p + links[p.link].r * Vector3::from(p.local_point_m))
            .collect::<Vec<_>>();
        let targets = reference
            .values
            .chunks_exact(3)
            .map(Vector3::from_column_slice)
            .collect::<Vec<_>>();
        let errors = targets
            .iter()
            .zip(&actual)
            .map(|(t, a)| t - a)
            .collect::<Vec<_>>();
        let displacements = errors
            .iter()
            .zip(&activation)
            .map(|(e, a)| e * *a)
            .collect::<Vec<_>>();
        let jacobians = values.into_iter().map(|(_, j)| j).collect::<Vec<_>>();
        // Inactive points impose no least-squares objective on shared joints.
        let weights = activation.clone();
        let correction_rad = point_correction(
            &jacobians,
            &weights,
            &displacements,
            self.config.damping_m_per_rad,
            self.config.maximum_correction_rad,
        )?;
        Ok(PointFeedbackSample {
            reference_time_s: phase,
            target_positions_world_m: targets.into_iter().map(Into::into).collect(),
            actual_positions_world_m: actual.into_iter().map(Into::into).collect(),
            position_errors_world_m: errors.into_iter().map(Into::into).collect(),
            activation,
            correction_rad,
        })
    }
}
