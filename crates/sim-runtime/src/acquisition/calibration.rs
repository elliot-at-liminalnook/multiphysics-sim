//! Operator-taught encoder envelopes. No inferred robot geometry or automatic homing.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AxisCalibration {
    pub role: String,
    pub lower: Option<i32>,
    pub upper: Option<i32>,
    pub reference: Option<i32>,
    /// Explicit mounting direction before both named part poses are taught.
    #[serde(default)]
    pub reverse: bool,
    #[serde(default)]
    pub coordinate_session: Option<String>,
    /// Tracking session of a multi-turn `reference` (outside one encoder
    /// turn); it is not meaningful in any other session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_session: Option<String>,
    /// CAD joint angle (rad) the leg mirror showed when `reference` was saved
    /// (the alignment pose). Absent means the joint's CAD home.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_joint_rad: Option<f64>,
    /// Operator-disabled: the motor may not be selected or driven.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
    /// Gains fitted to this motor by an identification run; replaces the
    /// shared provisional gains for this axis.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tuning: Option<MotorTuning>,
}
/// Per-motor feedback gains from a measured open-loop response.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotorTuning {
    pub pid: sim_domain_control::pwm_feedback::Pid,
    /// Duty added in the direction of the position error to overcome friction.
    pub friction_duty: f64,
    /// Measured response the gains were designed from.
    pub gain_counts_s_per_duty: f64,
    pub time_constant_s: f64,
    pub loop_delay_s: f64,
    pub breakaway_duty: [f64; 2],
    /// Identification record (samples and fits) in the measurements folder.
    pub record: String,
    pub method: String,
}
impl AxisCalibration {
    pub fn validate(&self) -> Result<(), String> {
        if self.role.len() > 100 {
            return Err("Unknown joint role".into());
        }
        if [self.lower, self.upper, self.reference]
            .into_iter()
            .flatten()
            .any(|v| v.unsigned_abs() > 8_000_000)
        {
            return Err("Continuous encoder coordinate exceeds supported numeric range".into());
        }
        if self.reference_joint_rad.is_some_and(|q| !q.is_finite() || q.abs() > 100.) {
            return Err("Alignment joint angle must be a finite angle".into());
        }
        if let (Some(l), Some(u)) = (self.lower, self.upper) {
            if u.abs_diff(l) <= 8 {
                return Err("Bounds need more than eight encoder counts of clearance".into());
            }
        }
        if let Some(t) = &self.tuning {
            t.pid.validate()?;
            if !(t.friction_duty.is_finite() && (0. ..1.).contains(&t.friction_duty))
                || !(t.gain_counts_s_per_duty.is_finite() && t.gain_counts_s_per_duty > 0.)
                || !(t.time_constant_s.is_finite() && t.time_constant_s > 0.)
                || !(t.loop_delay_s.is_finite() && t.loop_delay_s >= 0.)
                || t.breakaway_duty.iter().any(|d| !(d.is_finite() && (0. ..=1.).contains(d)))
            {
                return Err(format!("Motor tuning for {} is not a valid measured response", self.role));
            }
        }
        Ok(())
    }
    /// Upper/lower name part poses, independent of the motor mounting orientation.
    pub fn reversed(&self) -> bool {
        match (self.lower, self.upper) {
            (Some(l), Some(u)) => u < l,
            _ => self.reverse,
        }
    }
    pub fn clear(&mut self, boundary: &str) -> Result<(), String> {
        self.reverse = self.reversed();
        match boundary {
            "lower" => self.lower = None,
            "upper" => self.upper = None,
            "both" => {
                self.lower = None;
                self.upper = None;
            }
            "reference" => {
                self.reference = None;
                self.reference_session = None;
                self.reference_joint_rad = None;
            }
            _ => return Err("Choose lower, upper, both, or reference".into()),
        }
        if self.lower.is_none() && self.upper.is_none() {
            self.coordinate_session = None;
        }
        self.validate()
    }
    pub fn encoder_bounds(&self) -> (Option<i32>, Option<i32>) {
        if self.reversed() {
            (self.upper, self.lower)
        } else {
            (self.lower, self.upper)
        }
    }
    /// Explicit mechanical envelope in continuous encoder coordinates; no target backlog.
    pub fn jog(&self, position: i32, delta: i16) -> Result<(i32, i32, i32), String> {
        self.validate()?;
        if self.role.is_empty() {
            return Err("Assign the motor role first".into());
        }
        if !(1..=4095).contains(&delta.unsigned_abs()) {
            return Err("Step must be 1..4095 encoder counts".into());
        }
        let p = position as i32;
        let t = p
            .checked_add(delta as i32)
            .ok_or("Continuous coordinate overflow")?;
        if p.unsigned_abs() > 8_000_000 || t.unsigned_abs() > 8_000_000 {
            return Err("Continuous coordinate overflow".into());
        }
        let (lower, upper) = self.encoder_bounds();
        // Allow recovery toward the interior after teaching a boundary, but never farther outward.
        if delta < 0 && lower.is_some_and(|l| t < (l as i32 + 4)) {
            return Err("Working boundary reached toward decreasing encoder counts".into());
        }
        if delta > 0 && upper.is_some_and(|u| t > (u as i32 - 4)) {
            return Err("Working boundary reached toward increasing encoder counts".into());
        }
        let lo = (p.min(t) - 4).max(lower.map_or(-8_000_000, |v| v as i32).min(p));
        let hi = (p.max(t) + 4).min(upper.map_or(8_000_000, |v| v as i32).max(p));
        Ok((t, lo, hi))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Calibration {
    pub schema_version: u32,
    pub fixture: String,
    pub units: String,
    pub provenance: String,
    pub axes: std::collections::BTreeMap<u8, AxisCalibration>,
}
impl Default for Calibration {
    fn default() -> Self {
        Self{schema_version:1,fixture:"Actuator fixture; CAD binding pending".into(),units:"HX motor encoder counts; 4096 counts/revolution; not joint degrees".into(),provenance:"Operator-taught, provisional. Four-count inward working margin. Requires repeatability and physical clearance validation; no automatic homing or coupled-joint envelope.".into(),axes:Default::default()}
    }
}
impl Calibration {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 || self.axes.is_empty() || self.axes.keys().any(|id| *id > 253)
        {
            return Err("Unsupported calibration schema or motor IDs".into());
        }
        for a in self.axes.values() {
            a.validate()?;
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn boundaries_and_recovery() {
        let a = AxisCalibration {
            role: "worm".into(),
            lower: Some(1000),
            upper: Some(1100),
            reference: None,
            reverse: false,
            coordinate_session: None,
            reference_session: None,
            reference_joint_rad: None,
            disabled: false,
            tuning: None,
        };
        assert!(a.jog(1004, -1).is_err());
        assert!(a.jog(1096, 1).is_err());
        assert!(a.jog(1000, 1).is_ok());
        assert!(a.jog(1100, -1).is_ok());
        assert_eq!(a.jog(1050, 4).unwrap(), (1054, 1046, 1058));
        assert_eq!(a.jog(1050, 27).unwrap(), (1077, 1046, 1081));
        assert!(a.jog(1050, 0).is_err());
    }
    #[test]
    fn partial_reverse_bound_and_arbitrary_step() {
        let a = AxisCalibration {
            role: "reversed worm".into(),
            lower: Some(3000),
            reverse: true,
            ..Default::default()
        };
        assert!(a.jog(2996, 1).is_err());
        assert_eq!(a.jog(3000, -1024).unwrap(), (1976, 1972, 3000));
        assert!(a.jog(3000, -4095).is_ok());
    }
    #[test]
    fn never_wrap_or_guess_mapping() {
        let mut a = AxisCalibration::default();
        assert!(a.jog(2048, 1).is_err());
        a.role = "knee".into();
        assert!(a.jog(2, -4).is_ok());
        assert!(a.jog(4090, 8).is_ok());
        a.lower = Some(200);
        a.upper = Some(100);
        assert!(a.validate().is_ok());
        assert!(a.reversed());
        assert_eq!(a.encoder_bounds(), (Some(100), Some(200)));
        assert!(a.jog(104, -1).is_err());
        assert!(a.jog(196, 1).is_err());
        assert!(a.jog(100, 1).is_ok());
        assert!(a.jog(200, -1).is_ok());
        a.upper = Some(196);
        assert!(a.validate().is_err());
    }
    #[test]
    fn alignment_angle_is_kept_with_its_reference_and_cleared_with_it() {
        let text = r#"{"role":"knee","lower":2629,"upper":3387,"reference":2900,"reference_joint_rad":-1.248}"#;
        let mut a: AxisCalibration = serde_json::from_str(text).unwrap();
        assert_eq!(a.reference_joint_rad, Some(-1.248));
        a.validate().unwrap();
        let back: AxisCalibration = serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
        assert_eq!((back.reference, back.reference_joint_rad), (Some(2900), Some(-1.248)));
        // Older saves have no angle (CAD home) and serialize without one.
        let old: AxisCalibration = serde_json::from_str(r#"{"role":"knee","lower":null,"upper":null,"reference":2722}"#).unwrap();
        assert_eq!(old.reference_joint_rad, None);
        assert!(!serde_json::to_string(&old).unwrap().contains("reference_joint_rad"));
        a.reference_joint_rad = Some(f64::NAN);
        assert!(a.validate().is_err());
        a.reference_joint_rad = Some(-1.248);
        a.clear("lower").unwrap();
        assert_eq!(a.reference_joint_rad, Some(-1.248));
        a.clear("reference").unwrap();
        assert_eq!((a.reference, a.reference_joint_rad), (None, None));
    }
    #[test]
    fn clearing_reversed_bounds_retains_mounting_and_reference() {
        let mut a = AxisCalibration {
            role: "worm".into(),
            lower: Some(3000),
            upper: Some(1000),
            reference: Some(2000),
            reverse: false,
            coordinate_session: None,
            reference_session: None,
            reference_joint_rad: None,
            disabled: false,
            tuning: None,
        };
        a.clear("lower").unwrap();
        assert!(a.reverse);
        assert_eq!(a.encoder_bounds(), (Some(1000), None));
        a.clear("both").unwrap();
        assert_eq!(a.reference, Some(2000));
        assert!(a.reversed());
    }
}

/// Nearest-turn tracking for a sampled cyclic encoder. It cannot recover turns lost while disconnected.
#[derive(Clone, Copy, Debug, Default)]
pub struct EncoderTurns {
    last_raw: Option<u16>,
    pub counts: i32,
}
impl EncoderTurns {
    pub fn observe(&mut self, raw: u16) -> Result<i32, String> {
        if raw > 4095 {
            return Err("Invalid 12-bit encoder sample".into());
        }
        if let Some(last) = self.last_raw {
            let delta = (raw as i32 - last as i32 + 2048).rem_euclid(4096) - 2048;
            if delta.abs() == 2048 {
                return Err(
                    "Ambiguous half-turn encoder jump; reference must be re-established".into(),
                );
            }
            self.counts = self
                .counts
                .checked_add(delta)
                .filter(|p| p.abs() <= 8_000_000)
                .ok_or("Continuous encoder coordinate overflow")?;
        } else {
            self.counts = raw as i32;
        }
        self.last_raw = Some(raw);
        Ok(self.counts)
    }
}
#[cfg(test)]
mod rollover_tests {
    use super::*;
    #[test]
    fn wraps_in_both_directions_without_a_jump() {
        let mut e = EncoderTurns::default();
        for (raw, expected) in [(4, 4), (0, 0), (4095, -1), (4090, -6), (2, 2)] {
            assert_eq!(e.observe(raw).unwrap(), expected);
        }
        for _ in 0..3 {
            for raw in [1000, 2000, 3000, 4000, 2] {
                e.observe(raw).unwrap();
            }
        }
        assert_eq!(e.counts, 12290);
    }
    #[test]
    fn ambiguous_jump_is_not_guessed() {
        let mut e = EncoderTurns::default();
        e.observe(4).unwrap();
        assert!(e.observe(2052).is_err());
    }
}
