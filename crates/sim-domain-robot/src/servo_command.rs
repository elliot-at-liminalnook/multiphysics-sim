//! Explicit command bounds for the existing effective-servo reference law.
use super::EffectiveServo;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServoCommandLimits {
    /// Command bounds in motor coordinate order; supplied by CAD/runtime configuration.
    pub bounds_rad: Vec<[f64; 2]>,
    /// Numerical inequality scaling only. Bounds have no added acceptance tolerance.
    pub residual_scale_rad: f64,
}
#[derive(Clone, Debug, Serialize)]
pub struct ServoCommandCheck {
    pub targets_rad: Vec<f64>,
    /// Upper then lower signed inequality for each command; <= 0 is admissible.
    pub inequalities: Vec<f64>,
    pub maximum_violation_rad: f64,
}
impl ServoCommandLimits {
    pub fn validate(&self, count: usize) -> Result<(), String> {
        if count == 0
            || self.bounds_rad.len() != count
            || !self.residual_scale_rad.is_finite()
            || self.residual_scale_rad <= 0.
            || self
                .bounds_rad
                .iter()
                .any(|b| b.iter().any(|x| !x.is_finite()) || b[0] > b[1])
        {
            return Err(
                "matched finite ordered servo-command bounds and positive numerical scale required"
                    .into(),
            );
        }
        Ok(())
    }
    /// Nominal zero-tracking-error command; feedback, interpolation and clock
    /// transitions need separate runtime validation. Uses the actual servo law.
    pub fn evaluate(
        &self,
        motors: &[EffectiveServo],
        angles: &[f64],
        speeds: &[f64],
        torques: &[f64],
    ) -> Result<ServoCommandCheck, String> {
        self.validate(motors.len())?;
        if [angles.len(), speeds.len(), torques.len()]
            .iter()
            .any(|n| *n != motors.len())
        {
            return Err(
                "matched motor reference angle, speed and torque dimensions required".into(),
            );
        }
        let mut targets_rad = Vec::with_capacity(motors.len());
        let mut inequalities = Vec::with_capacity(2 * motors.len());
        let mut maximum_violation_rad = 0.0_f64;
        for (i, motor) in motors.iter().enumerate() {
            let target = motor.reference_target(angles[i], speeds[i], torques[i])?;
            let [lower, upper] = self.bounds_rad[i];
            let violations = [target - upper, lower - target];
            maximum_violation_rad = maximum_violation_rad.max(violations[0]).max(violations[1]);
            targets_rad.push(target);
            inequalities.extend(violations.map(|v| v / self.residual_scale_rad));
        }
        if inequalities.iter().any(|x| !x.is_finite()) {
            return Err("nonfinite servo-command inequality".into());
        }
        Ok(ServoCommandCheck {
            targets_rad,
            inequalities,
            maximum_violation_rad,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn feedforward_can_exceed_command_bounds_while_pose_and_torque_remain_admissible() {
        let motor = EffectiveServo::new(&std::collections::BTreeMap::from([
            ("stiffness".into(), 10.),
            ("damping".into(), 0.1),
            ("stall_torque".into(), 3.),
            ("no_load_speed".into(), 2.),
        ]))
        .unwrap();
        let limits = ServoCommandLimits {
            bounds_rad: vec![[-0.1, 0.1]],
            residual_scale_rad: 0.01,
        };
        let check = limits
            .evaluate(std::slice::from_ref(&motor), &[0.09], &[0.2], &[0.1])
            .unwrap();
        assert!(motor.torque_capacity(0.2, 0.1) > 0.1);
        assert!((check.targets_rad[0] - 0.102).abs() < 1e-14);
        assert!((check.maximum_violation_rad - 0.002).abs() < 1e-14);
        assert!((check.inequalities[0] - 0.2).abs() < 1e-12);
        let repaired = limits
            .evaluate(std::slice::from_ref(&motor), &[0.09], &[0.2], &[0.05])
            .unwrap();
        assert_eq!(repaired.maximum_violation_rad, 0.);
        assert!(repaired.inequalities.iter().all(|x| *x <= 0.));
        assert!(
            limits
                .evaluate(std::slice::from_ref(&motor), &[f64::NAN], &[0.], &[0.])
                .is_err()
        );
        assert!(limits.evaluate(&[motor], &[], &[0.], &[0.]).is_err());
        assert!(
            ServoCommandLimits {
                bounds_rad: vec![[1., -1.]],
                residual_scale_rad: 1.
            }
            .validate(1)
            .is_err()
        );
    }
}
