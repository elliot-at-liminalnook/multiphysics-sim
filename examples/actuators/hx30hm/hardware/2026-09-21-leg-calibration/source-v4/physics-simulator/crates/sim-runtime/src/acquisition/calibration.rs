//! Operator-taught encoder envelopes. No inferred robot geometry or automatic homing.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AxisCalibration {
    pub role: String,
    pub lower: Option<u16>,
    pub upper: Option<u16>,
    pub reference: Option<u16>,
    /// Explicit mounting direction before both named part poses are taught.
    #[serde(default)]
    pub reverse: bool,
}
impl AxisCalibration {
    pub fn validate(&self) -> Result<(), String> {
        if self.role.len() > 100 {
            return Err("Unknown joint role".into());
        }
        if [self.lower, self.upper, self.reference]
            .into_iter()
            .flatten()
            .any(|v| v > 4095)
        {
            return Err("Encoder must be within 0..4095".into());
        }
        if let (Some(l), Some(u)) = (self.lower, self.upper) {
            if u.abs_diff(l) <= 8 {
                return Err("Bounds need more than eight encoder counts of clearance".into());
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
    pub fn encoder_bounds(&self) -> (Option<u16>, Option<u16>) {
        if self.reversed() {
            (self.upper, self.lower)
        } else {
            (self.lower, self.upper)
        }
    }
    /// Explicit target envelope per jog. No wrapping or accumulated target backlog.
    pub fn jog(&self, position: u16, delta: i16) -> Result<(u16, u16, u16), String> {
        self.validate()?;
        if self.role.is_empty() {
            return Err("Assign the motor role first".into());
        }
        if !(1..=4095).contains(&delta.unsigned_abs()) {
            return Err("Step must be 1..4095 encoder counts".into());
        }
        let p = position as i32;
        let t = p + delta as i32;
        if !(8..=4087).contains(&p) || !(8..=4087).contains(&t) {
            return Err(
                "Encoder wrap guard: establish another reference before crossing zero".into(),
            );
        }
        let (lower, upper) = self.encoder_bounds();
        // Allow recovery toward the interior after teaching a boundary, but never farther outward.
        if delta < 0 && lower.is_some_and(|l| t < (l as i32 + 4)) {
            return Err("Working boundary reached toward decreasing encoder counts".into());
        }
        if delta > 0 && upper.is_some_and(|u| t > (u as i32 - 4)) {
            return Err("Working boundary reached toward increasing encoder counts".into());
        }
        let lo = (p.min(t) - 4).max(lower.map_or(0, |v| v as i32).min(p));
        let hi = (p.max(t) + 4).min(upper.map_or(4095, |v| v as i32).max(p));
        Ok((t as u16, lo as u16, hi as u16))
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
        assert!(a.jog(3000, -4095).is_err());
    }
    #[test]
    fn never_wrap_or_guess_mapping() {
        let mut a = AxisCalibration::default();
        assert!(a.jog(2048, 1).is_err());
        a.role = "knee".into();
        assert!(a.jog(2, -4).is_err());
        assert!(a.jog(4090, 8).is_err());
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
}
