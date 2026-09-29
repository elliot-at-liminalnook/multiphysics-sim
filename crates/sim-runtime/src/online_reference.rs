//! Shared adapter pieces for online walking references: resolve the three
//! planar command channels, bind foot markers to CAD links, and place a
//! requested body pose and foot positions through the mechanism's inverse
//! kinematics. Placement solves a private copy; physical state never moves.
use crate::{session::InputChannel, tracking::Marker};
use nalgebra::{Quaternion, UnitQuaternion};
use sim_domain_robot::{
    Articulated, Generalized,
    articulated::embedding::{
        CoordinateInterval, EmbeddedPoint, PlanePlacementConfig, PointPlacement, PointTarget,
        RigidEmbedding,
    },
};

/// Input indices of body-frame forward and lateral speed (m/s) and yaw rate
/// (rad/s), each named exactly once with matching units.
pub(crate) fn planar_command_inputs(
    names: &[String; 3],
    inputs: &[InputChannel],
) -> Result<[usize; 3], String> {
    let mut indices = [0; 3];
    for i in 0..3 {
        let matches = inputs
            .iter()
            .enumerate()
            .filter(|(_, c)| c.name == names[i])
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Err("unique planar command channel required".into());
        }
        let (j, c) = matches[0];
        let kind = if i == 2 {
            sim_core::QuantityKind::AngularVelocity
        } else {
            sim_core::QuantityKind::LinearVelocity
        };
        if c.kind != kind {
            return Err("planar command channel units do not match".into());
        }
        indices[i] = j;
    }
    if indices.iter().collect::<std::collections::BTreeSet<_>>().len() != 3 {
        return Err("distinct planar input channels required".into());
    }
    Ok(indices)
}

/// Each marker's point on its uniquely named CAD link.
pub(crate) fn marker_points(art: &Articulated, markers: &[Marker]) -> Result<Vec<EmbeddedPoint>, String> {
    markers
        .iter()
        .map(|m| {
            let matches = art
                .links
                .iter()
                .enumerate()
                .filter(|(_, l)| l.name == m.link)
                .collect::<Vec<_>>();
            if matches.len() != 1 {
                return Err(format!("unique link required for marker {} ({})", m.id, m.link));
            }
            Ok(EmbeddedPoint { link: matches[0].0, local_point_m: m.local_point_m })
        })
        .collect()
}

/// Warm-started placement of one floating base and its foot points.
pub(crate) struct BodyPlacement {
    pub points: Vec<EmbeddedPoint>,
    seed: Option<Generalized>,
    initial_rotation: Option<UnitQuaternion<f64>>,
}

pub(crate) struct Placed {
    pub fit: PointPlacement,
    pub initial_rotation: UnitQuaternion<f64>,
    pub links: Vec<sim_domain_robot::articulated::LinkKin>,
}

impl BodyPlacement {
    pub fn new(points: Vec<EmbeddedPoint>) -> Self {
        Self { points, seed: None, initial_rotation: None }
    }

    /// The base's rotation when placement started (from `g` on first use).
    pub fn initial_rotation(&self, art: &Articulated, g: &Generalized) -> UnitQuaternion<f64> {
        let base = art.bases[0].state;
        self.initial_rotation.unwrap_or_else(|| {
            UnitQuaternion::from_quaternion(Quaternion::new(
                g.states[base + 3],
                g.states[base + 4],
                g.states[base + 5],
                g.states[base + 6],
            ))
        })
    }

    /// Solve joint coordinates that put the base at `body_world_m` with
    /// `turn * initial rotation` and each foot point at its target. Rejects
    /// fits whose internal link overlap exceeds `penetration_tolerance_m`. Call [`Self::commit`] to warm-start
    /// the next solve from an accepted result.
    pub fn solve(
        &self,
        art: &Articulated,
        map: &RigidEmbedding<'_>,
        g: &Generalized,
        body_world_m: [f64; 3],
        turn: UnitQuaternion<f64>,
        feet_world_m: &[[f64; 3]],
        bounds: &[CoordinateInterval],
        placement: &PlanePlacementConfig,
        penetration_tolerance_m: f64,
        label: &str,
    ) -> Result<Placed, String> {
        if art.bases.len() != 1 || art.bases[0].grounded {
            return Err("online body reference requires one floating base".into());
        }
        let base = art.bases[0].state;
        let initial_rotation = self.initial_rotation(art, g);
        let mut seed = self.seed.clone().unwrap_or_else(|| g.clone());
        seed.states[base..base + 3].copy_from_slice(&body_world_m);
        let q = (turn * initial_rotation).into_inner();
        seed.states[base + 3..base + 7].copy_from_slice(&[q.w, q.i, q.j, q.k]);
        let targets = self
            .points
            .iter()
            .zip(feet_world_m)
            .map(|(point, p)| PointTarget { point: point.clone(), position_world_m: *p })
            .collect::<Vec<_>>();
        let fit = map
            .place_points(&seed, &targets, bounds, placement)
            .map_err(|e| format!("{label} IK: {e}"))?;
        let links = art.evaluate_kinematics_only(&fit.motion.generalized);
        if let Some(c) = art.inter_link_penetrations(&links)?.iter().find(|c| c.penetration_m > penetration_tolerance_m) {
            return Err(format!(
                "{label} reference has internal contact: {} / {}; penetration {:.6} mm at world {:?} m; requested body {:?} m",
                art.links[c.link].name,
                art.links[c.other].name,
                c.penetration_m * 1000.0,
                c.point_m,
                body_world_m,
            ));
        }
        Ok(Placed { fit, initial_rotation, links })
    }

    pub fn commit(&mut self, placed: &Placed) {
        self.initial_rotation = Some(placed.initial_rotation);
        self.seed = Some(placed.fit.motion.generalized.clone());
    }
}

/// What an online walking reference hands the policy each sample.
pub(crate) struct OnlineTarget {
    /// Motor-coordinate reference (rad), in policy actuator order.
    pub coordinates: Vec<f64>,
    pub body_world_m: [f64; 3],
    pub body_velocity_m_s: [f64; 3],
    /// Reference-link heading about world Z (rad) and its rate (rad/s).
    pub yaw: [f64; 2],
    /// Foot targets for point feedback, world metres.
    pub feedback_feet_world_m: Vec<[f64; 3]>,
    pub telemetry: serde_json::Value,
}

/// The configured online reference: the quasi-static support sequence or a
/// steered contact-phase gait. Both use the same command channels, CAD
/// placement and feedback bindings.
pub(crate) enum OnlineReference {
    Step(crate::step_reference::OnlineStepReference),
    Steered(crate::steered_reference::OnlineSteeredReference),
}

impl OnlineReference {
    /// Telemetry and metadata key.
    pub fn key(&self) -> &'static str {
        match self {
            Self::Step(_) => "step_reference",
            Self::Steered(_) => "steered_gait",
        }
    }
    pub fn sample(
        &mut self,
        art: &Articulated,
        map: &RigidEmbedding<'_>,
        g: &Generalized,
        time: f64,
        inputs: &[f64],
    ) -> Result<OnlineTarget, String> {
        match self {
            Self::Step(r) => {
                let p = r.sample(art, map, g, time, inputs)?;
                Ok(OnlineTarget {
                    coordinates: p.coordinates.clone(),
                    body_world_m: p.reference.body_world_m,
                    body_velocity_m_s: [0.; 3],
                    yaw: [p.reference.yaw_rad, 0.],
                    feedback_feet_world_m: p.feedback_feet_world_m.clone(),
                    telemetry: serde_json::json!({"reference":p.reference,"coordinates":p.coordinates,"maximum_marker_error_m":p.maximum_marker_error_m,"static_support":p.support,"feedback_feet_world_m":p.feedback_feet_world_m,"preload_extension_m":p.preload_extension_m,"measured_support_force_n":p.measured_support_force_n}),
                })
            }
            Self::Steered(r) => r.sample(art, map, g, time, inputs),
        }
    }
    pub fn metadata(&self) -> serde_json::Value {
        match self {
            Self::Step(r) => {
                let scope = if r.config().sequence.update_command_before_lift {
                    "Online support sequence and bounded CAD inverse kinematics. Non-reversing commands are reconsidered before lift-off after support qualification. Stops cancel unstarted swings and recenter without moving planted foot references; airborne swings finish landing. Translation reversals retain the committed transfer before selecting a new stance. Every reference still passes CAD geometry/placement checks. Ideal floor loads qualify lift, landing and recenter transitions."
                } else {
                    "Online support sequence and bounded CAD inverse kinematics. Commands latch at foot-transfer boundaries; geometric references never mutate physical state. Ideal floor loads qualify lift/landing transitions."
                };
                serde_json::json!({"config":r.config(),"scope":scope})
            }
            Self::Steered(r) => serde_json::json!({"config":r.config(),"scope":"Steered contact-phase gait with bounded CAD inverse kinematics. The body path is fixed one commitment horizon ahead and follows the rate-limited command after that delay; footholds are the gait's centers carried by the mid-stance path pose, final before lift-off. Timing, swing shape and body oscillation are the gait's. Geometric reference only: no support-force qualification or inverse-load feedforward; balance and contact are the physics engine's."}),
        }
    }
}
