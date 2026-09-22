//! Offline compiler for the prototype device-timed trajectory protocol.
//!
//! Produces CONFIG/ROW/SEAL packets only. The current commissioned bridge does
//! not implement this protocol. No serial I/O, ARM, START or lease renewal occurs.
use super::fpga::Plan;
use crate::acquisition::servo_bus::packet;
use serde::{Deserialize, Serialize};

pub const CLOCK_HZ: u32 = 50_000_000;
pub const PROTOCOL_VERSION: u8 = 1;
pub const INSTRUCTION: u8 = 0xa2;
pub const HEADER_BYTES: usize = 34;
pub const ROW_BYTES: usize = 36;

/// Retains the source experiment and measured home positions, not just packets.
/// Call `validate` after loading an artifact before using any encoded fields.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Upload {
    pub protocol_version: u8,
    pub clock_hz: u32,
    pub source_plan: Plan,
    pub source_plan_blake3: String,
    pub homes: [u16; 9],
    pub plan_crc32: u32,
    pub packets: Vec<Vec<u8>>,
}

/// CRC-32/ISO-HDLC (IEEE), matching zlib and the FPGA byte scanner.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ if crc & 1 != 0 { 0xedb88320 } else { 0 };
        }
    }
    !crc
}

impl Upload {
    pub fn compile(plan: &Plan, homes: [u16; 9]) -> Result<Self, String> {
        plan.validate()?;
        if plan.targets.len() > 256
            || plan.bitstream_path.is_empty()
            || !plan.bitstream_blake3.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err("Invalid trajectory capacity or firmware provenance".into());
        }
        let exact_ticks = plan.period_s * f64::from(CLOCK_HZ);
        let period = exact_ticks.round() as u32;
        // Permit floating point representation error, not a changed schedule.
        if (exact_ticks - f64::from(period)).abs() > 1e-6
            || !(2_000_000..=7_500_000).contains(&period)
            || u64::from(period) * plan.targets.len() as u64 > 600_000_000
        {
            return Err("Trajectory period must be exact at 50 MHz and duration bounded".into());
        }
        let mask = plan.ids.iter().fold(0u16, |m, id| m | (1 << (id - 4)));
        let mut canonical_homes = [0u16; 9];
        for &id in &plan.ids {
            let i = usize::from(id - 4);
            if !(600..=3495).contains(&homes[i]) {
                return Err(format!("ID {id} home outside commissioned travel range"));
            }
            canonical_homes[i] = homes[i];
        }
        let mut header = Vec::with_capacity(HEADER_BYTES);
        header.extend(mask.to_le_bytes());
        header.extend((plan.targets.len() as u16).to_le_bytes());
        header.extend(period.to_le_bytes());
        for value in [
            plan.gains.kp_q8,
            plan.gains.kd_q8,
            plan.gains.kv_q8,
            plan.gains.limit,
        ] {
            header.extend(value.to_le_bytes());
        }
        for home in canonical_homes {
            header.extend(home.to_le_bytes());
        }
        let mut config = vec![0, PROTOCOL_VERSION];
        config.extend(&header);
        let mut packets = vec![packet(254, INSTRUCTION, &config)?];
        let mut bytes = header;
        for (index, targets) in plan.targets.iter().enumerate() {
            let mut payload = vec![1];
            payload.extend((index as u16).to_le_bytes());
            for i in 0..9 {
                let (target, delta) = if mask & (1 << i) != 0 {
                    let target = i32::from(canonical_homes[i]) + i32::from(targets[i]);
                    if !(0..=4095).contains(&target) {
                        return Err("Encoder boundary crossed".into());
                    }
                    (
                        target as u16,
                        targets[i]
                            - if index == 0 {
                                0
                            } else {
                                plan.targets[index - 1][i]
                            },
                    )
                } else {
                    (0, 0)
                };
                payload.extend(target.to_le_bytes());
                payload.extend(delta.to_le_bytes());
            }
            bytes.extend(&payload[3..]);
            packets.push(packet(254, INSTRUCTION, &payload)?);
        }
        let plan_crc32 = crc32(&bytes);
        let mut seal = vec![2];
        seal.extend(plan_crc32.to_le_bytes());
        packets.push(packet(254, INSTRUCTION, &seal)?);
        Ok(Self {
            protocol_version: PROTOCOL_VERSION,
            clock_hz: CLOCK_HZ,
            source_plan: plan.clone(),
            source_plan_blake3: blake3::hash(&serde_json::to_vec(plan).map_err(|e| e.to_string())?)
                .to_hex()
                .to_string(),
            homes,
            plan_crc32,
            packets,
        })
    }

    pub fn validate(&self) -> Result<(), String> {
        let expected = Self::compile(&self.source_plan, self.homes)?;
        if self.protocol_version != expected.protocol_version
            || self.clock_hz != expected.clock_hz
            || self.source_plan_blake3 != expected.source_plan_blake3
            || self.plan_crc32 != expected.plan_crc32
            || self.packets != expected.packets
        {
            return Err("Trajectory artifact differs from its source experiment".into());
        }
        Ok(())
    }

    /// Canonical header followed by rows; useful for independent RTL verification.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        let mut bytes = self.packets[0][7..7 + HEADER_BYTES].to_vec();
        for packet in &self.packets[1..self.packets.len() - 1] {
            bytes.extend(&packet[8..8 + ROW_BYTES]);
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn standard_crc_check() {
        assert_eq!(super::crc32(b"123456789"), 0xcbf43926);
        assert_eq!(super::crc32(b""), 0);
    }
}
