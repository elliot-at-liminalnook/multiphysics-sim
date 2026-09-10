//! Restore an evaluable joint reference through a checked parameter segment.
use super::*;
use sim_solve::domain_restore::{DomainPoint, DomainRestoration, bisect_evaluation_domain};
#[derive(Debug, Serialize)]
pub struct JointDomainRestoration {
    pub reference_body_refinements: usize,
    pub restoration: DomainRestoration<(JointContactMotion, JointContactReport)>,
    pub scope: &'static str,
}
impl ContactPlanner<'_> {
    /// Interpolate specified decisions toward a target, retaining its fixed fields.
    /// Reference body controls may be exactly refined to the target resolution.
    /// Acceptance means successful CAD evaluation plus the original geometry
    /// gates; force balance and motor limits remain separately reported failures.
    pub fn restore_joint_evaluation_domain(
        &self,
        reference: &JointContactMotion,
        target: &JointContactMotion,
        variables: &[JointContactVariable],
        iterations: usize,
    ) -> Result<JointDomainRestoration, String> {
        // Validate even when no refinement is needed: decoding retimes controls.
        Trajectory::new(reference.motion.body.clone())?;
        Trajectory::new(target.motion.body.clone())?;
        if reference.motion.body.keyframes.len() < 2 || target.motion.body.keyframes.len() < 2 {
            return Err("restoration requires at least two body keyframes per endpoint".into());
        }
        let mut reference = reference.clone();
        let mut refinements = 0;
        while reference.motion.body.keyframes.len() < target.motion.body.keyframes.len() {
            reference.motion.body =
                Trajectory::new(reference.motion.body.clone())?.refined_periodic_config()?;
            refinements += 1;
        }
        if reference.motion.body.keyframes.len() != target.motion.body.keyframes.len() {
            return Err("reference body cannot be exactly refined to target layout".into());
        }
        let from = joint_values(&reference, variables, self.recipe.direction_world)?;
        let to = joint_values(target, variables, self.recipe.direction_world)?;
        if variables
            .iter()
            .zip(from.iter().zip(&to))
            .any(|(v, (a, b))| {
                !v.bound.lower.is_finite()
                    || !v.bound.upper.is_finite()
                    || v.bound.lower > v.bound.upper
                    || !a.is_finite()
                    || !b.is_finite()
                    || *a < v.bound.lower
                    || *a > v.bound.upper
                    || *b < v.bound.lower
                    || *b > v.bound.upper
            })
        {
            return Err(
                "restoration endpoints must remain within every original variable bound".into(),
            );
        }
        let restoration = bisect_evaluation_domain(&from, &to, iterations, |values| {
            let candidate =
                match decode_joint_values(target, variables, self.recipe.direction_world, values) {
                    Ok(c) => c,
                    Err(e) => return Ok(DomainPoint::Rejected(e)),
                };
            let report = match self.evaluate_joint_uncached(&candidate) {
                Ok(r) => r,
                Err(e) => return Ok(DomainPoint::Rejected(e)),
            };
            for frame in &report.motion_report.frames {
                if frame.maximum_inter_link_penetration_m > 0.
                    || frame.maximum_floor_penetration_m > self.recipe.penetration_tolerance_m
                {
                    return Ok(DomainPoint::Rejected(format!(
                        "geometry gate at time {} s, clock {}: inter-link {} m, floor {} m",
                        frame.time_s,
                        frame.clock.phase_rate,
                        frame.maximum_inter_link_penetration_m,
                        frame.maximum_floor_penetration_m
                    )));
                }
            }
            Ok(DomainPoint::Accepted((candidate, report)))
        })?;
        Ok(JointDomainRestoration {
            reference_body_refinements: refinements,
            restoration,
            scope: "Local restoration of the CAD evaluation/geometry domain along explicit bounded decisions. Exact reference body refinement, original target fixed fields and physical limits retained. Does not restore force/motor feasibility or certify a continuous parameter path, global speed limit or runtime gait.",
        })
    }
}
