//! Adapter from the shared steered contact-phase gait to the CAD mechanism's
//! inverse kinematics. Commands arrive on the same three planar channels as
//! the support-sequence reference (body-frame forward, lateral, yaw rate).
//! Only angular motor targets leave this adapter.
use crate::{
    body_feedback::BodyFeedbackConfig,
    online_reference::{BodyPlacement, OnlineTarget, marker_points, planar_command_inputs},
    point_feedback::PointFeedbackConfig,
    session::InputChannel,
};
use nalgebra::{UnitQuaternion, Vector3};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sim_domain_control::contact_phase::{
    ContactPhaseConfig,
    steered::{PathStart, SteeredGait, SteeringConfig},
};
use sim_domain_robot::{
    Articulated, Generalized,
    articulated::embedding::{CoordinateInterval, PlanePlacementConfig, RigidEmbedding},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SteeredReferenceConfig {
    /// The periodic gait being steered; its foot centers are world
    /// coordinates for the robot's starting placement.
    pub gait: ContactPhaseConfig,
    pub steering: SteeringConfig,
    /// Body-frame vx, vy (m/s) and yaw rate (rad/s) input channels.
    pub command_channels: [String; 3],
    /// Gait time at the first sample (s), e.g. inside an all-feet-down window.
    #[serde(default)]
    pub initial_phase_s: f64,
    pub placement: PlanePlacementConfig,
    pub bounds: Vec<CoordinateInterval>,
    /// Largest accepted internal link overlap of a reference pose (m), as
    /// the contact planner audits it. Zero rejects any overlap.
    #[serde(default)]
    pub penetration_tolerance_m: f64,
}

struct Start {
    time_s: f64,
    base_world_m: [f64; 3],
    heading_rad: f64,
}

pub(crate) struct OnlineSteeredReference {
    config: SteeredReferenceConfig,
    inputs: [usize; 3],
    reference_link: usize,
    placement: BodyPlacement,
    gait: Option<(SteeredGait, Start)>,
}

impl OnlineSteeredReference {
    pub fn new(
        art: &Articulated,
        config: SteeredReferenceConfig,
        body: &BodyFeedbackConfig,
        feet: &PointFeedbackConfig,
        inputs: &[InputChannel],
        limits: &[[f64; 2]],
    ) -> Result<Self, String> {
        if config.gait.feet.len() != feet.markers.len() {
            return Err(format!(
                "steered gait has {} feet but point feedback binds {} markers",
                config.gait.feet.len(),
                feet.markers.len()
            ));
        }
        if !config.initial_phase_s.is_finite()
            || !(config.penetration_tolerance_m >= 0.)
            || config.bounds.len() != limits.len()
            || config.bounds.iter().zip(limits).any(|(b, l)| b.lower < l[0] || b.upper > l[1])
        {
            return Err("steered reference requires a finite initial phase and bounds within the policy envelope".into());
        }
        // Validate the gait and steering bounds now, not at the first sample.
        SteeredGait::new(config.gait.clone(), config.steering.clone(), [0.; 2], PathStart::default())?;
        let reference_link = art
            .links
            .iter()
            .position(|l| l.name == body.reference_link)
            .ok_or_else(|| format!("unknown body reference link {}", body.reference_link))?;
        Ok(Self {
            inputs: planar_command_inputs(&config.command_channels, inputs)?,
            placement: BodyPlacement::new(marker_points(art, &feet.markers)?),
            reference_link,
            config,
            gait: None,
        })
    }
    pub fn config(&self) -> &SteeredReferenceConfig {
        &self.config
    }

    pub fn sample(
        &mut self,
        art: &Articulated,
        map: &RigidEmbedding<'_>,
        g: &Generalized,
        time: f64,
        inputs: &[f64],
    ) -> Result<OnlineTarget, String> {
        if self.gait.is_none() {
            let base = art.bases.first().filter(|b| !b.grounded).ok_or("online body reference requires one floating base")?.state;
            let r = art.evaluate_kinematics_only(g)[self.reference_link].r;
            let start = Start {
                time_s: time,
                base_world_m: [g.states[base], g.states[base + 1], g.states[base + 2]],
                heading_rad: r[(1, 0)].atan2(r[(0, 0)]),
            };
            let gait = SteeredGait::new(
                self.config.gait.clone(),
                self.config.steering.clone(),
                [start.base_world_m[0], start.base_world_m[1]],
                PathStart { time_s: self.config.initial_phase_s, ..Default::default() },
            )?;
            self.gait = Some((gait, start));
        }
        let (gait, start) = self.gait.as_mut().expect("initialized above");
        let clock = time - start.time_s + self.config.initial_phase_s;
        let command = self.inputs.map(|i| inputs[i]);
        // A failed step or placement ends the run (the policy reports it), so
        // the plan need not roll back.
        let sample = gait.step(clock, command)?;
        let body_world_m = std::array::from_fn(|i| start.base_world_m[i] + sample.body_offset_m[i]);
        let feet: Vec<[f64; 3]> = sample.feet.iter().map(|f| f.position_world_m).collect();
        let placed = self.placement.solve(
            art,
            map,
            g,
            body_world_m,
            UnitQuaternion::from_scaled_axis(Vector3::from(sample.body_rotation_vector_rad)),
            &feet,
            &self.config.bounds,
            &self.config.placement,
            self.config.penetration_tolerance_m,
            &format!("steered gait at {clock:.6} s"),
        )?;
        self.placement.commit(&placed);
        let yaw = [start.heading_rad + sample.path_pose[2], sample.path_twist[2]];
        let telemetry = json!({
            "sample": sample,
            "gait_time_s": clock,
            "command": command,
            "horizon_s": gait.horizon_s(),
            "coordinates": placed.fit.coordinates,
            "maximum_marker_error_m": placed.fit.maximum_position_error_m,
        });
        Ok(OnlineTarget {
            coordinates: placed.fit.coordinates.clone(),
            body_world_m,
            body_velocity_m_s: sample.body_velocity_m_s,
            yaw,
            feedback_feet_world_m: feet,
            telemetry,
        })
    }
}
