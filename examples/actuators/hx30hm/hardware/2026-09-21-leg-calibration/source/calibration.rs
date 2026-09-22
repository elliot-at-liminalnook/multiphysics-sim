//! Operator-taught encoder envelopes. No inferred robot geometry or automatic homing.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AxisCalibration {
    pub role: String,
    pub lower: Option<u16>,
    pub upper: Option<u16>,
    pub reference: Option<u16>,
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
            if u <= l + 8 {
                return Err("Bounds need more than eight encoder counts of clearance".into());
            }
        }
        Ok(())
    }
    /// Narrow device envelope on each deliberate jog. No wrapping or accumulated target backlog.
    pub fn jog(&self, position: u16, delta: i16) -> Result<(u16, u16, u16), String> {
        self.validate()?;
        if self.role.is_empty() {
            return Err("Assign the motor role first".into());
        }
        if ![1, 4, 8].contains(&delta.unsigned_abs()) {
            return Err("Jog must be 1, 4 or 8 encoder counts".into());
        }
        let p = position as i32;
        let t = p + delta as i32;
        if !(8..=4087).contains(&p) || !(8..=4087).contains(&t) {
            return Err(
                "Encoder wrap guard: establish another reference before crossing zero".into(),
            );
        }
        // Allow recovery toward the interior after teaching a boundary, but never farther outward.
        if delta < 0 && self.lower.is_some_and(|l| t < (l as i32 + 4)) {
            return Err("Lower working boundary reached".into());
        }
        if delta > 0 && self.upper.is_some_and(|u| t > (u as i32 - 4)) {
            return Err("Upper working boundary reached".into());
        }
        let lo = (p.min(t) - 4).max(self.lower.map_or(0, |v| v as i32).min(p));
        let hi = (p.max(t) + 4).min(self.upper.map_or(4095, |v| v as i32).max(p));
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
        };
        assert!(a.jog(1004, -1).is_err());
        assert!(a.jog(1096, 1).is_err());
        assert!(a.jog(1000, 1).is_ok());
        assert!(a.jog(1100, -1).is_ok());
        assert_eq!(a.jog(1050, 4).unwrap(), (1054, 1046, 1058));
        assert!(a.jog(1050, 9).is_err());
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
        assert!(a.validate().is_err());
    }
}
