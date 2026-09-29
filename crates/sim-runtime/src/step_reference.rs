//! Adapter from shared support-sequence references to the CAD mechanism's IK.
//! Planning copies are private; only angular motor targets leave this adapter.
use crate::{
    body_feedback::BodyFeedbackConfig,
    online_reference::{BodyPlacement, marker_points, planar_command_inputs},
    point_feedback::PointFeedbackConfig,
    session::InputChannel,
};
use nalgebra::{UnitQuaternion, Vector3};
use serde::{Deserialize, Serialize};
use sim_domain_control::{
    stepping::{StepPhase, StepReference, StepSequence, StepSequenceConfig},
    support_preload::{SupportPreload, SupportPreloadConfig},
};
use sim_domain_robot::{
    Articulated, Generalized,
    articulated::embedding::{CoordinateInterval, PlanePlacementConfig, RigidEmbedding},
};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepReferenceConfig {
    pub sequence: StepSequenceConfig,
    /// Body-frame vx, vy (m/s), yaw rate (rad/s), explicitly named input channels.
    pub command_channels: [String; 3],
    pub initial_yaw_rad: f64,
    pub minimum_support_force_n: f64,
    /// Planner-only static load screen; actual lift/landing qualification uses
    /// minimum_support_force_n. None preserves the same threshold for both.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_planned_support_force_n: Option<f64>,
    pub placement: PlanePlacementConfig,
    pub bounds: Vec<CoordinateInterval>,
    /// Optional privileged force feedback. Extends only position targets, never physical poses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub support_preload: Option<SupportPreloadConfig>,
}
pub(crate) struct OnlineStepReference {
    config: StepReferenceConfig,
    sequence: StepSequence,
    inputs: [usize; 3],
    placement: BodyPlacement,
    links: Vec<String>,
    markers: Vec<crate::tracking::Marker>,
    preload: Option<SupportPreload>,
    extensions: Vec<f64>,
}
pub(crate) struct PlannedStep {
    pub reference: StepReference,
    pub coordinates: Vec<f64>,
    pub maximum_marker_error_m: f64,
    pub support: Option<crate::support::StaticSupportGeometry>,
    pub feedback_feet_world_m: Vec<[f64; 3]>,
    pub preload_extension_m: Vec<f64>,
    pub measured_support_force_n: Vec<f64>,
}
impl OnlineStepReference {
    pub fn new(
        art: &Articulated,
        config: StepReferenceConfig,
        body: &BodyFeedbackConfig,
        feet: &PointFeedbackConfig,
        inputs: &[InputChannel],
        limits: &[[f64; 2]],
        period: f64,
    ) -> Result<Self, String> {
        if config.sequence.period_s != period
            || !config.minimum_support_force_n.is_finite()
            || config.minimum_support_force_n <= 0.
            || config
                .minimum_planned_support_force_n
                .is_some_and(|f| !f.is_finite() || f < 0.)
            || !config.initial_yaw_rad.is_finite()
            || config.bounds.len() != limits.len()
            || config
                .bounds
                .iter()
                .zip(limits)
                .any(|(b, l)| b.lower < l[0] || b.upper > l[1])
        {
            return Err("online step reference requires matching sample period, finite frame/load and bounds within the policy envelope".into());
        }
        if body.support_markers.len() != feet.markers.len()
            || body
                .support_markers
                .iter()
                .zip(&feet.markers)
                .any(|(a, b)| {
                    a.id != b.id || a.link != b.link || a.local_point_m != b.local_point_m
                })
        {
            return Err("body and foot feedback must use the same ordered support markers".into());
        }
        let indices = planar_command_inputs(&config.command_channels, inputs)?;
        let points = marker_points(art, &feet.markers)?;
        let origin = &body.position_world_m.keyframes[0].values;
        let feet_origin = feet.position_world_m.keyframes[0]
            .values
            .chunks_exact(3)
            .map(|v| [v[0], v[1], v[2]])
            .collect();
        let sequence = StepSequence::new(
            config.sequence.clone(),
            [origin[0], origin[1], origin[2]],
            config.initial_yaw_rad,
            feet_origin,
        )?;
        let preload = config
            .support_preload
            .clone()
            .map(|c| {
                if c.period_s != period {
                    return Err("support preload must match controller period".into());
                }
                SupportPreload::new(c)
            })
            .transpose()?;
        Ok(Self {
            preload,
            extensions: vec![0.; feet.markers.len()],
            config,
            sequence,
            inputs: indices,
            placement: BodyPlacement::new(points),
            links: feet.markers.iter().map(|m| m.link.clone()).collect(),
            markers: feet.markers.clone(),
        })
    }
    pub fn config(&self) -> &StepReferenceConfig {
        &self.config
    }
    pub fn sample(
        &mut self,
        art: &Articulated,
        map: &RigidEmbedding<'_>,
        g: &Generalized,
        time: f64,
        inputs: &[f64],
    ) -> Result<PlannedStep, String> {
        if art.bases.len() != 1 || art.bases[0].grounded {
            return Err("online body reference requires one floating base".into());
        }
        if art.omit_inter_link_contact {
            let observed = art.evaluate_kinematics_only(g);
            if let Some(c) = art.inter_link_penetrations(&observed)?.first() {
                return Err(format!("observed inter-link overlap outside reduced-contact operating envelope: {} / {}; penetration {:.6} mm at world {:?} m, time {:.6} s",
                    art.links[c.link].name, art.links[c.other].name, c.penetration_m * 1000.0, c.point_m, time));
            }
        }
        let forces = crate::support::ideal_upward_floor_forces(art, g, &self.links)?;
        let foot = self.sequence.next_foot();
        let minimum = self.config.minimum_support_force_n;
        let mut sequence = self.sequence.clone();
        let reference = sequence.sample(
            time,
            self.inputs.map(|i| inputs[i]),
            forces
                .iter()
                .enumerate()
                .all(|(i, f)| i == foot || *f >= minimum),
            forces.iter().all(|f| *f >= minimum),
        )?;
        let turn = UnitQuaternion::from_axis_angle(
            &Vector3::z_axis(),
            reference.yaw_rad - self.config.initial_yaw_rad,
        );
        let placed = self.placement.solve(
            art,
            map,
            g,
            reference.body_world_m,
            turn,
            &reference.feet_world_m,
            &self.config.bounds,
            &self.config.placement,
            0.,
            &format!("step {} {:?} at {time:.6} s", reference.step, reference.phase),
        )?;
        let links = &placed.links;
        let support = if matches!(
            reference.phase,
            sim_domain_control::stepping::StepPhase::Raise
                | sim_domain_control::stepping::StepPhase::Lower
        ) {
            let poses = links
                .iter()
                .zip(&art.links)
                .map(|(k, l)| crate::session::LinkPose {
                    name: l.name.clone(),
                    position_m: k.p.into(),
                    rotation: std::array::from_fn(|i| std::array::from_fn(|j| k.r[(i, j)])),
                })
                .collect::<Vec<_>>();
            let markers = self
                .markers
                .iter()
                .enumerate()
                .filter(|(i, _)| Some(*i) != reference.foot)
                .map(|(_, m)| m.clone())
                .collect::<Vec<_>>();
            let support = crate::support::static_support_geometry(art, &poses, &markers)?;
            if support.projected_margin.minimum_edge_margin_m < 0.
                || support.vertical_point_forces_n.is_some_and(|loads| {
                    loads.iter().any(|f| {
                        *f < self
                            .config
                            .minimum_planned_support_force_n
                            .unwrap_or(self.config.minimum_support_force_n)
                    })
                })
            {
                return Err(format!(
                    "step {} {:?} reference lacks static support: {:?}",
                    reference.step, reference.phase, support.vertical_point_forces_n
                ));
            }
            Some(support)
        } else {
            None
        };
        // Readiness controls transition timing, but it never substitutes for IK
        // or collision checks. Dynamic support remains the physics engine's job.
        let mut extensions = self.extensions.clone();
        let mut feedback_feet_world_m = reference.feet_world_m.clone();
        if let Some(preload) = &self.preload {
            if let Some(loads) = support.as_ref().and_then(|s| s.vertical_point_forces_n) {
                let mut support_index = 0;
                for i in 0..extensions.len() {
                    if Some(i) == reference.foot {
                        extensions[i] = 0.;
                        continue;
                    }
                    if reference.foot.is_some_and(|foot| forces[foot] <= 0.1)
                        && (reference.phase == StepPhase::Raise || reference.progress < 0.8)
                    {
                        extensions[i] = preload.update(
                            extensions[i],
                            loads[support_index],
                            forces[i].max(0.),
                            true,
                        )?;
                    }
                    // Integrate only after the swing foot unloads. Release
                    // over the final fifth of lowering before landing qualification.
                    let p = ((reference.progress - 0.8) / 0.2).clamp(0., 1.);
                    let weight = if reference.phase == StepPhase::Lower {
                        1. - sim_domain_control::smooth_return::rest_to_rest(p)[0]
                    } else {
                        1.
                    };
                    feedback_feet_world_m[i][2] -= extensions[i] * weight;
                    support_index += 1;
                }
            } else {
                extensions.fill(0.);
            }
        }
        self.extensions = extensions.clone();
        self.placement.commit(&placed);
        self.sequence = sequence;
        Ok(PlannedStep {
            reference,
            coordinates: placed.fit.coordinates.clone(),
            maximum_marker_error_m: placed.fit.maximum_position_error_m,
            support,
            feedback_feet_world_m,
            preload_extension_m: extensions,
            measured_support_force_n: forces,
        })
    }
}
