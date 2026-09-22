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

/// Receipt from the upload-enabled bridge, distinct from the A0 safety status.
/// Capability 1 means upload/status only, not autonomous execution.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub operation: u8,
    pub rejected: bool,
    pub valid: bool,
    pub busy: bool,
    pub mask: u16,
    pub frames: u16,
    pub written: u16,
    pub period_ticks: u32,
    pub crc32: u32,
    pub clock_hz: u32,
    pub capabilities: u8,
}
impl Receipt {
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let frame = crate::acquisition::servo_bus::reply(bytes, 254, 25)?;
        let p = &frame.parameters;
        if frame.error != 0
            || p[0] != PROTOCOL_VERSION
            || p[1] != INSTRUCTION
            || p[3..6].iter().any(|b| *b > 1)
        {
            return Err("Invalid trajectory receipt envelope".into());
        }
        let word = |n| u16::from_le_bytes([p[n], p[n + 1]]);
        let dword = |n| u32::from_le_bytes([p[n], p[n + 1], p[n + 2], p[n + 3]]);
        let value = Self {
            operation: p[2],
            rejected: p[3] != 0,
            valid: p[4] != 0,
            busy: p[5] != 0,
            mask: word(6),
            frames: word(8),
            written: word(10),
            period_ticks: dword(12),
            crc32: dword(16),
            clock_hz: dword(20),
            capabilities: p[24],
        };
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.clock_hz != CLOCK_HZ
            || self.capabilities != 1
            || self.mask > 511
            || self.frames > 256
            || self.written > self.frames
            || (self.valid
                && (self.rejected
                    || self.busy
                    || self.mask == 0
                    || self.frames < 2
                    || self.written != self.frames
                    || !(2_000_000..=7_500_000).contains(&self.period_ticks)
                    || u64::from(self.frames) * u64::from(self.period_ticks) > 600_000_000))
        {
            return Err("Invalid trajectory receipt state or unsupported FPGA capability".into());
        }
        Ok(())
    }
    /// Check one ordered CONFIG/ROW/SEAL result. No pipelined assumptions or START.
    pub fn check_for(&self, upload: &Upload, packet_index: usize) -> Result<(), String> {
        self.validate()?;
        upload.validate()?;
        let packet = upload
            .packets
            .get(packet_index)
            .ok_or("Unknown upload packet index")?;
        if self.rejected {
            return Err(format!(
                "FPGA rejected trajectory operation {}",
                self.operation
            ));
        }
        let header = &upload.packets[0][7..41];
        let mask = u16::from_le_bytes([header[0], header[1]]);
        let frames = u16::from_le_bytes([header[2], header[3]]);
        let period = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
        let sealed = packet[5] == 2;
        let written = if sealed { frames } else { packet_index as u16 };
        if self.operation != packet[5]
            || self.busy
            || self.valid != sealed
            || self.mask != mask
            || self.frames != frames
            || self.period_ticks != period
            || self.written != written
            || self.crc32 != if sealed { upload.plan_crc32 } else { 0 }
        {
            return Err(
                "FPGA receipt does not acknowledge the expected trajectory operation".into(),
            );
        }
        Ok(())
    }
}

/// Read capability, progress and last upload rejection without modifying the plan.
pub fn status_request() -> Vec<u8> {
    packet(254, INSTRUCTION, &[4]).expect("fixed trajectory status packet")
}

/// Retained upload exchange; neither a motor recording nor evidence of motion.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transfer {
    pub source_plan_blake3: String,
    pub plan_crc32: u32,
    pub capability_reply: Option<Vec<u8>>,
    pub replies: Vec<Vec<u8>>,
    pub completed: bool,
    pub cancelled: bool,
    pub failure: Option<String>,
}
impl Transfer {
    pub fn validate_capture(&self, upload: &Upload) -> Result<(), String> {
        upload.validate()?;
        if self.source_plan_blake3 != upload.source_plan_blake3
            || self.plan_crc32 != upload.plan_crc32
            || self.replies.len() > upload.packets.len()
            || self.replies.iter().any(|p| p.len() > 64)
            || self.capability_reply.as_ref().is_some_and(|p| p.len() > 64)
            || (self.cancelled && self.completed)
            || (!self.completed && self.failure.as_ref().is_none_or(|e| e.is_empty()))
        {
            return Err("Invalid retained trajectory upload".into());
        }
        if self.completed {
            if self.failure.is_some() || self.replies.len() != upload.packets.len() {
                return Err("Incomplete trajectory upload cannot be accepted".into());
            }
            let status = Receipt::decode(
                self.capability_reply
                    .as_ref()
                    .ok_or("Missing capability reply")?,
            )?;
            if status.operation != 4 {
                return Err("Missing trajectory capability query".into());
            }
            for (index, reply) in self.replies.iter().enumerate() {
                Receipt::decode(reply)?.check_for(upload, index)?;
            }
        }
        Ok(())
    }
}

/// Exchange one packet at a time, verifying capability and every receipt before
/// proceeding. The adapter supplies raw framed replies and logs transport timing.
/// A rejection, timeout or cancellation leaves an explicit incomplete artifact.
/// It never sends motor commands, ARM, START, automatic retries or lease renewals.
pub fn transfer(
    upload: &Upload,
    cancel: &std::sync::atomic::AtomicBool,
    mut exchange: impl FnMut(&[u8]) -> Result<Vec<u8>, String>,
) -> Result<Transfer, String> {
    use std::sync::atomic::Ordering;
    upload.validate()?;
    let mut report = Transfer {
        source_plan_blake3: upload.source_plan_blake3.clone(),
        plan_crc32: upload.plan_crc32,
        capability_reply: None,
        replies: vec![],
        completed: false,
        cancelled: false,
        failure: None,
    };
    let result = (|| -> Result<(), String> {
        if cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            return Err("Cancelled".into());
        }
        let capability = exchange(&status_request())?;
        report.capability_reply = Some(capability);
        let status = Receipt::decode(report.capability_reply.as_ref().unwrap())?;
        if status.operation != 4 {
            return Err("Expected trajectory capability reply".into());
        }
        for (index, packet) in upload.packets.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                report.cancelled = true;
                return Err("Cancelled".into());
            }
            let reply = exchange(packet)?;
            report.replies.push(reply);
            Receipt::decode(report.replies.last().unwrap())?.check_for(upload, index)?;
        }
        Ok(())
    })();
    match result {
        Ok(()) => report.completed = true,
        Err(error) => report.failure = Some(error),
    }
    Ok(report)
}
