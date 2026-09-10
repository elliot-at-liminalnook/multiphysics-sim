//! Batch kinematic capability inspection through the shared closure and collision model.
//! A collision-free sampled pose is not a certified trajectory or loaded equilibrium.
use crate::{session::LinkPose, tracking::CaptureConfig};
use serde::{Deserialize, Serialize};
use sim_domain_robot::{articulated::embedding::{EmbeddingConfig, EmbeddedPoint, RigidEmbedding}, Articulated, Generalized};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigurationSample {
    pub id: String,
    /// Independent joint coordinates, radians for revolute and metres for prismatic DOFs.
    pub coordinates: Vec<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigurationInspection {
    /// Record link transforms for an independent CAD/B-rep inspection.
    #[serde(default)]
    pub record_poses: bool,
    pub independent_coordinates: Vec<String>,
    pub embedding: EmbeddingConfig,
    pub samples: Vec<ConfigurationSample>,
}

#[derive(Debug, Serialize)]
pub struct InspectedMarker {
    pub id: String,
    pub position_world_m: [f64; 3],
    /// Three rows, one column per independent coordinate; base held fixed.
    pub jacobian: Vec<Vec<f64>>,
}

#[derive(Debug, Serialize)]
pub struct InspectedConfiguration {
    pub id: String,
    pub coordinates: Vec<f64>,
    pub error: Option<String>,
    pub authored_limit_violations: Vec<String>,
    pub markers: Vec<InspectedMarker>,
    pub sampled_penetrations: Vec<sim_domain_robot::articulated::InterLinkPenetration>,
    pub minimum_scaled_singular_value: Option<f64>,
    pub maximum_scaled_closure_error: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poses: Option<Vec<LinkPose>>,
}

/// Each row solves from the same caller-provided seed, so failures and row order
/// cannot silently change another row's branch. Authored exclusions remain active.
pub fn inspect_configurations(art: &Articulated, seed: &Generalized, markers: &CaptureConfig,
    config: &ConfigurationInspection) -> Result<Vec<InspectedConfiguration>, String> {
    let hash = art.model.source.get("cad_sha256").and_then(|v| v.as_str());
    if markers.coordinate_frame.is_empty() || markers.markers.is_empty()
        || hash.is_none() || markers.expected_cad_sha256.as_deref() != hash {
        return Err("nonempty registered frame/markers and matching CAD hash required".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    let points = markers.markers.iter().map(|m| {
        let found: Vec<_> = art.links.iter().enumerate().filter(|(_, l)| l.name == m.link).collect();
        if !ids.insert(&m.id) || found.len() != 1 || m.local_point_m.iter().any(|x| !x.is_finite()) {
            return Err(format!("invalid or ambiguous marker {}", m.id));
        }
        Ok(EmbeddedPoint { link: found[0].0, local_point_m: m.local_point_m })
    }).collect::<Result<Vec<_>, String>>()?;
    let map = RigidEmbedding::new(art, &config.independent_coordinates, config.embedding.clone())?;
    let mut ids = std::collections::BTreeSet::new();
    if config.samples.is_empty() || config.samples.iter().any(|s| s.id.is_empty() || !ids.insert(&s.id)
        || s.coordinates.len() != map.independent_joint_indices().len() || s.coordinates.iter().any(|v| !v.is_finite())) {
        return Err("nonempty unique sample IDs and finite dimension-matched coordinates required".into());
    }
    Ok(config.samples.iter().map(|s| {
        let mut row = InspectedConfiguration { id:s.id.clone(), coordinates:s.coordinates.clone(),
            error:None, authored_limit_violations:vec![], markers:vec![], sampled_penetrations:vec![],
            minimum_scaled_singular_value:None, maximum_scaled_closure_error:None, poses:None };
        let result = (|| -> Result<(), String> {
            let (motion, values) = map.point_jacobians(seed, &s.coordinates, &points)?;
            row.minimum_scaled_singular_value = Some(motion.minimum_scaled_singular_value);
            row.maximum_scaled_closure_error = Some(motion.maximum_scaled_position_error);
            for (i, (_, d)) in art.dofs().enumerate() {
                if d.lower.is_some_and(|lo| motion.generalized.q[i] < lo)
                    || d.upper.is_some_and(|hi| motion.generalized.q[i] > hi) {
                    row.authored_limit_violations.push(d.name.clone());
                }
            }
            row.markers = values.iter().zip(&markers.markers).map(|((p,j),m)| InspectedMarker {
                id:m.id.clone(), position_world_m:(*p).into(),
                jacobian:(0..3).map(|r| (0..j.ncols()).map(|c| j[(r,c)]).collect()).collect(),
            }).collect();
            let poses: Vec<_> = art.poses(&motion.generalized).iter().zip(&art.links).map(|((r,p),l)| LinkPose {
                name:l.name.clone(), position_m:(*p).into(),
                rotation:std::array::from_fn(|i| std::array::from_fn(|j| r[(i,j)])),
            }).collect();
            row.sampled_penetrations = crate::contact_audit::sampled_inter_link_penetrations(art,&poses)?;
            if config.record_poses { row.poses = Some(poses); }
            Ok(())
        })();
        if let Err(e) = result { row.error = Some(e); }
        row
    }).collect())
}
