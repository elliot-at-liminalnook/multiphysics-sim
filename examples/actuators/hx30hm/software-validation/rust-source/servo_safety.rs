//! Versioned protocol for the FPGA HX bench supervisor. Limits reported here
//! are bench policy, not calibrated actuator properties or supply measurements.
use super::servo_bus::packet;
use serde::{Deserialize, Serialize};
pub const BRIDGE_ID: u8 = 254;
pub const INSTRUCTION: u8 = 0xa0;
pub const STATUS_WIDTH: usize = 13;
#[derive(Clone, Copy, Debug)]
pub enum Command {
    Stop,
    Arm(u8),
    Status,
    Heartbeat(u8),
    Disarm(u8),
}
impl Command {
    pub fn parameters(self) -> Result<Vec<u8>, &'static str> {
        let (op, id) = match self {
            Self::Stop => (0, None),
            Self::Arm(id) => (1, Some(id)),
            Self::Status => (2, None),
            Self::Heartbeat(id) => (3, Some(id)),
            Self::Disarm(id) => (4, Some(id)),
        };
        if id.is_some_and(|id| !(4..=12).contains(&id)) {
            return Err("FPGA profile supports IDs 4..12");
        }
        Ok(id.map_or_else(|| vec![op], |id| vec![op, id]))
    }
    pub fn packet(self) -> Result<Vec<u8>, &'static str> {
        packet(BRIDGE_ID, INSTRUCTION, &self.parameters()?)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Status {
    pub version: u8,
    pub latched: bool,
    pub reason: u8,
    pub fault_id: u8,
    pub armed_mask: u16,
    pub fresh_mask: u16,
    pub temperature_max_c: u8,
    pub voltage_min_raw: u8,
    pub voltage_max_raw: u8,
    pub current_max_raw_uncalibrated: u16,
}
impl Status {
    pub fn decode(p: &[u8]) -> Result<Self, &'static str> {
        if p.len() != STATUS_WIDTH || p[0] != 1 || p[1] > 1 || p[2] > 11 {
            return Err("unsupported FPGA safety status");
        }
        let armed = u16::from_le_bytes([p[4], p[5]]);
        let fresh = u16::from_le_bytes([p[6], p[7]]);
        if (armed | fresh) & !0x1ff != 0 || (p[1] == 1 && armed != 0) || p[9] > p[10] {
            return Err("inconsistent FPGA safety status");
        }
        Ok(Self {
            version: p[0],
            latched: p[1] != 0,
            reason: p[2],
            fault_id: p[3],
            armed_mask: armed,
            fresh_mask: fresh,
            temperature_max_c: p[8],
            voltage_min_raw: p[9],
            voltage_max_raw: p[10],
            current_max_raw_uncalibrated: u16::from_le_bytes([p[11], p[12]]),
        })
    }
    pub fn require_bench_profile(&self) -> Result<(), &'static str> {
        if self.temperature_max_c != 60
            || self.voltage_min_raw != 90
            || self.voltage_max_raw != 126
            || self.current_max_raw_uncalibrated != 2000
        {
            return Err("unexpected FPGA safety thresholds");
        }
        Ok(())
    }
    pub fn require_armed(&self, id: u8) -> Result<(), String> {
        if !(4..=12).contains(&id)
            || self.latched
            || self.armed_mask & (1 << (id - 4)) == 0
            || self.fresh_mask & (1 << (id - 4)) == 0
        {
            return Err(format!(
                "ID {id}: FPGA not armed/fresh; fault {} on ID {}",
                self.reason, self.fault_id
            ));
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_wire_protocol_and_rejects_invalid_profiles() {
        assert_eq!(
            Command::Arm(12).packet().unwrap(),
            [255, 255, 254, 4, 160, 1, 12, 80]
        );
        assert!(Command::Arm(3).packet().is_err());
        let mut bytes = [1, 0, 0, 254, 0, 1, 0, 1, 60, 90, 126, 208, 7];
        let s = Status::decode(&bytes).unwrap();
        s.require_bench_profile().unwrap();
        s.require_armed(12).unwrap();
        assert!(s.require_armed(4).is_err());
        bytes[1] = 1;
        assert!(Status::decode(&bytes).is_err());
        bytes[4] = 0;
        bytes[5] = 0;
        let s = Status::decode(&bytes).unwrap();
        assert!(s.require_armed(12).is_err());
        bytes[10] = 127;
        assert!(
            Status::decode(&bytes)
                .unwrap()
                .require_bench_profile()
                .is_err()
        );
    }
}
