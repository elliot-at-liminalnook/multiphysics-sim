//! Named ideal rigid-body observations for teacher policies and diagnostics.
//! No sensor is inferred. Marker offsets are relative to exported link COMs.
use crate::tracking::{Marker, validate_markers};
use nalgebra::Vector3;
use serde::{Deserialize, Serialize};
use sim_core::{Channel, QuantityKind};
use sim_domain_robot::{
    Articulated, Generalized,
    articulated::{ContactPoint, LinkKin},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskObservationSource {
    IdealRigidBodyDiagnostics,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskObservationConfig {
    pub observation_source: TaskObservationSource,
    pub expected_cad_sha256: String,
    /// Exact CAD link whose COM and axes define the moving reference frame.
    pub reference_link: String,
    pub markers: Vec<Marker>,
    /// Include floor-force resultants on each marker's link, in world axes.
    /// These are privileged force values, not inferred contact sensors.
    pub floor_forces: bool,
    /// Opt-in ideal heading of reference-link X projected onto world XY.
    /// Requires world-Z gravity; this is not an inferred hardware orientation sensor.
    #[serde(default, skip_serializing_if = "is_false")]
    pub heading_world_z: bool,
}
fn is_false(value: &bool) -> bool { !value }

pub struct TaskObserver {
    config: TaskObservationConfig,
    reference: usize,
    marker_links: Vec<usize>,
    gravity_world: Vector3<f64>,
    channels: Vec<Channel>,
}
impl TaskObserver {
    pub fn new(art: &Articulated, config: TaskObservationConfig) -> Result<Self, String> {
        validate_markers(&config.markers)?;
        if config.expected_cad_sha256.is_empty()
            || art.model.source["cad_sha256"].as_str() != Some(&config.expected_cad_sha256)
            || !art.gravity.iter().all(|v| v.is_finite())
            || art.gravity.norm() <= 0.0
            || (config.floor_forces && !art.contact_on)
            || (config.heading_world_z && (art.gravity.x != 0. || art.gravity.y != 0. || art.gravity.z >= 0.))
        {
            return Err("task observations require matching CAD provenance, finite nonzero gravity, enabled requested contact physics and world-Z gravity for requested heading".into());
        }
        let index = |name: &str| -> Result<usize, String> {
            let matches: Vec<_> = art
                .links
                .iter()
                .enumerate()
                .filter(|(_, l)| l.name == name)
                .map(|(i, _)| i)
                .collect();
            if matches.len() == 1 {
                Ok(matches[0])
            } else {
                Err(format!("unique task observation link required: {name}"))
            }
        };
        let reference = index(&config.reference_link)?;
        let marker_links = config
            .markers
            .iter()
            .map(|m| index(&m.link))
            .collect::<Result<_, _>>()?;
        let mut channels = vec![];
        let mut vector = |name: &str, kind: QuantityKind| {
            channels.extend(["x", "y", "z"].map(|axis| Channel {
                name: format!("{name}.{axis}"),
                kind,
            }));
        };
        vector("body.gravity_direction", QuantityKind::Dimensionless);
        vector("body.linear_velocity", QuantityKind::LinearVelocity);
        vector("body.angular_velocity", QuantityKind::AngularVelocity);
        for marker in &config.markers {
            vector(
                &format!("marker.{}.position", marker.id),
                QuantityKind::Length,
            );
            vector(
                &format!("marker.{}.velocity", marker.id),
                QuantityKind::LinearVelocity,
            );
            if config.floor_forces {
                vector(
                    &format!("marker.{}.floor_force_world", marker.id),
                    QuantityKind::Force,
                );
            }
        }
        if config.heading_world_z {
            channels.push(Channel { name: "body.heading_world_z".into(), kind: QuantityKind::Angle });
        }
        Ok(Self {
            config,
            reference,
            marker_links,
            gravity_world: art.gravity.normalize(),
            channels,
        })
    }
    pub fn channels(&self) -> &[Channel] {
        &self.channels
    }
    pub fn config(&self) -> &TaskObservationConfig {
        &self.config
    }
    pub fn observe(&self, art: &Articulated, state: &Generalized) -> Result<Vec<f64>, String> {
        if self.config.floor_forces {
            let e = art.evaluate(state);
            self.observe_evaluation(&e.links, &e.contacts)
        } else {
            self.observe_evaluation(&art.evaluate_kinematics_only(state), &[])
        }
    }
    /// Reuse a current evaluation if the caller already has one. This never
    /// updates contact memory and never identifies marker motion as slipping.
    pub fn observe_evaluation(
        &self,
        links: &[LinkKin],
        contacts: &[ContactPoint],
    ) -> Result<Vec<f64>, String> {
        let body = links
            .get(self.reference)
            .ok_or("missing task reference state")?;
        let to_body = body.r.transpose();
        let mut values = vec![];
        values.extend((to_body * self.gravity_world).iter());
        values.extend((to_body * body.vel).iter());
        values.extend((to_body * body.w).iter());
        for (marker, &index) in self.config.markers.iter().zip(&self.marker_links) {
            let link = links.get(index).ok_or("missing task marker state")?;
            let (position, velocity) =
                point_in_moving_frame(link, Vector3::from(marker.local_point_m), body);
            values.extend(position.iter());
            values.extend(velocity.iter());
            if self.config.floor_forces {
                let force = contacts
                    .iter()
                    .filter(|c| c.link == index && c.other.is_none())
                    .fold(Vector3::zeros(), |f, c| f + c.force);
                values.extend(force.iter());
            }
        }
        if self.config.heading_world_z {
            let (x, y) = (body.r[(0, 0)], body.r[(1, 0)]);
            if x == 0. && y == 0. {
                return Err("world-Z heading is undefined for a vertical reference X axis".into());
            }
            values.push(y.atan2(x));
        }
        if values.iter().any(|v: &f64| !v.is_finite()) {
            return Err("nonfinite task observation".into());
        }
        Ok(values)
    }
}

/// Derivative of R_body^T * (p_marker - p_body), including rotating-frame term.
pub fn point_in_moving_frame(
    link: &LinkKin,
    local: Vector3<f64>,
    body: &LinkKin,
) -> (Vector3<f64>, Vector3<f64>) {
    let offset = link.r * local;
    let delta = link.p + offset - body.p;
    let velocity_world = link.vel + link.w.cross(&offset);
    (
        body.r.transpose() * delta,
        body.r.transpose() * (velocity_world - body.vel - body.w.cross(&delta)),
    )
}
