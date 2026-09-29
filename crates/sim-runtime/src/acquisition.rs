//! Acquisition records retain device-clock windows and transport evidence.
//! They are irregular measurements, never implicitly resampled simulator states.
pub mod servo_bus;
pub mod calibration;
#[cfg(unix)]
pub mod calibration_serial;
pub mod servo_safety;
pub mod actuator_sweep;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClockWindow {
    pub clock_hz: u32,
    pub request_tick: u64,
    pub completion_tick: u64,
}
impl ClockWindow {
    pub fn validate(&self) -> Result<(), String> {
        if self.clock_hz == 0 || self.completion_tick < self.request_tick {
            return Err("invalid device clock or reversed acquisition window".into());
        }
        Ok(())
    }
    pub fn start_s(&self) -> f64 {
        self.request_tick as f64 / self.clock_hz as f64
    }
    pub fn end_s(&self) -> f64 {
        self.completion_tick as f64 / self.clock_hz as f64
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BusTransaction {
    pub version: u8,
    pub transport_sequence: u8,
    pub transaction_id: u32,
    pub device_dropped_total: u32,
    pub window: ClockWindow,
    pub device_id: u8,
    pub instruction: u8,
    /// 0: checksum-valid reply (inspect device_error), 1: bad reply, 2: timeout.
    pub outcome: u8,
    pub device_error: u8,
    pub control_flags: u8,
    pub request: Vec<u8>,
    pub reply: Vec<u8>,
}
impl BusTransaction {
    /// Version 1 FPGA transaction envelope. Operation 0x83 is distinct from
    /// diagnostic cache snapshots (0x81), which have no acquisition windows.
    pub fn from_frame(frame: &LinkFrame) -> Result<Self, String> {
        let p = &frame.payload;
        if frame.operation != 0x83 || p.len() != 56 || p[0] != 1 {
            return Err("unsupported transaction envelope operation/version/length".into());
        }
        if p[1] > 2 || p[4] > 8 || p[5] > 8 || p[7] != 0 || p[53..].iter().any(|b| *b != 0) {
            return Err("invalid transaction envelope fields".into());
        }
        if p[1] != 0 && (p[5] != 0 || p[40..49].iter().any(|b| *b != 0)) {
            return Err("failed transaction contains stale reply data".into());
        }
        let u32at = |i| u32::from_le_bytes(p[i..i + 4].try_into().unwrap());
        let u64at = |i| u64::from_le_bytes(p[i..i + 8].try_into().unwrap());
        let window = ClockWindow {
            clock_hz: u32at(49),
            request_tick: u64at(16),
            completion_tick: u64at(24),
        };
        window.validate()?;
        Ok(Self {
            version: 1,
            transport_sequence: frame.sequence,
            transaction_id: u32at(8),
            device_dropped_total: u32at(12),
            window,
            device_id: p[2],
            instruction: p[3],
            outcome: p[1],
            device_error: p[48],
            control_flags: p[6],
            request: p[32..32 + p[4] as usize].to_vec(),
            reply: p[40..40 + p[5] as usize].to_vec(),
        })
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LinkFrame {
    pub sequence: u8,
    pub operation: u8,
    pub payload: Vec<u8>,
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct DecodeStatistics {
    pub frames: u64,
    pub crc_errors: u64,
    pub invalid_lengths: u64,
    pub discarded_bytes: u64,
    pub partial_frames_discarded: u64,
}
/// Incremental AA55 / u8 length / sequence / operation / data / CRC-8(0x07).
/// Garbage, short reads and corrupted packets never produce measurements.
#[derive(Default)]
pub struct FrameDecoder {
    buffer: Vec<u8>,
    pub statistics: DecodeStatistics,
}
impl FrameDecoder {
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<LinkFrame> {
        self.buffer.extend_from_slice(bytes);
        let mut frames = vec![];
        loop {
            if self.buffer.len() < 2 {
                break;
            }
            if self.buffer[..2] != [0xaa, 0x55] {
                self.buffer.remove(0);
                self.statistics.discarded_bytes += 1;
                continue;
            }
            if self.buffer.len() < 3 {
                break;
            }
            let len = self.buffer[2] as usize;
            if len > 64 {
                self.statistics.invalid_lengths += 1;
                self.buffer.remove(0);
                continue;
            }
            let total = len + 6;
            if self.buffer.len() < total {
                break;
            }
            if crc8(&self.buffer[2..total - 1]) != self.buffer[total - 1] {
                self.statistics.crc_errors += 1;
                self.buffer.remove(0);
                continue;
            }
            frames.push(LinkFrame {
                sequence: self.buffer[3],
                operation: self.buffer[4],
                payload: self.buffer[5..5 + len].to_vec(),
            });
            self.statistics.frames += 1;
            self.buffer.drain(..total);
        }
        frames
    }
    /// The host calls this on a declared inter-frame timeout or end of input.
    pub fn discard_partial(&mut self) {
        if !self.buffer.is_empty() {
            self.statistics.partial_frames_discarded += 1;
            self.statistics.discarded_bytes += self.buffer.len() as u64;
            self.buffer.clear();
        }
    }
}
pub fn crc8(bytes: &[u8]) -> u8 {
    let mut crc = 0u8;
    for byte in bytes {
        crc ^= byte;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x07
            } else {
                crc << 1
            };
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;
    fn packet(seq: u8) -> Vec<u8> {
        let mut p = vec![0xaa, 0x55, 2, seq, 0x81, 0x38, 0x12];
        p.push(crc8(&p[2..]));
        p
    }
    #[test]
    fn framing_survives_fragmentation_noise_and_corruption() {
        let good = packet(255);
        let mut bad = packet(4);
        bad[5] ^= 1;
        let input = [vec![0x11, 0xaa, 0x55, 255], bad, good.clone(), packet(0)].concat();
        for chunk in 1..=input.len() {
            let mut d = FrameDecoder::default();
            let mut frames = vec![];
            for b in input.chunks(chunk) {
                frames.extend(d.feed(b));
            }
            assert_eq!(
                frames.iter().map(|f| f.sequence).collect::<Vec<_>>(),
                [255, 0]
            );
            assert_eq!(d.statistics.crc_errors, 1);
            assert_eq!(d.statistics.invalid_lengths, 1);
        }
    }
    #[test]
    fn incomplete_input_and_timeout_do_not_invent_samples() {
        let mut d = FrameDecoder::default();
        assert!(d.feed(&[0xaa, 0x55, 56, 0, 0x83]).is_empty());
        d.discard_partial();
        assert_eq!(d.statistics.partial_frames_discarded, 1);
        assert_eq!(d.feed(&packet(2)).len(), 1);
    }
    #[test]
    fn windows_and_failed_payloads_are_validated() {
        let mut p = vec![0u8; 56];
        p[0] = 1;
        p[4] = 2;
        p[5] = 2;
        p[32] = 0x38;
        p[33] = 2;
        p[49..53].copy_from_slice(&50_000_000u32.to_le_bytes());
        p[16..24].copy_from_slice(&100u64.to_le_bytes());
        p[24..32].copy_from_slice(&200u64.to_le_bytes());
        let mut f = LinkFrame {
            sequence: 0,
            operation: 0x83,
            payload: p,
        };
        let t = BusTransaction::from_frame(&f).unwrap();
        assert_eq!(t.window.end_s(), 4e-6);
        f.payload[1] = 2;
        assert!(BusTransaction::from_frame(&f).is_err());
        f.payload[5] = 0;
        assert!(BusTransaction::from_frame(&f).is_ok());
        f.payload[24..32].copy_from_slice(&50u64.to_le_bytes());
        assert!(BusTransaction::from_frame(&f).is_err());
    }
}

pub mod calibration_sweep;
pub mod motor_identification;
pub mod virtual_bench;
pub mod actuator_promotion;
pub mod characterization;
