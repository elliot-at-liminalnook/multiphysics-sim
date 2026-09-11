//! HX-compatible FF FF bus packets and register decoding, independent of serial I/O.
use serde::{Deserialize, Serialize};

pub fn packet(id: u8, instruction: u8, parameters: &[u8]) -> Result<Vec<u8>, &'static str> {
    if id > 254 || parameters.len() > 58 {
        return Err("invalid ID or packet exceeds 64-byte FPGA bridge buffer");
    }
    let mut p = vec![255, 255, id, (parameters.len() + 2) as u8, instruction];
    p.extend(parameters);
    p.push(!p[2..].iter().fold(0u8, |a, b| a.wrapping_add(*b)));
    Ok(p)
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reply {
    pub id: u8,
    pub error: u8,
    pub parameters: Vec<u8>,
}
pub fn reply(bytes: &[u8], expected_id: u8, expected_width: usize) -> Result<Reply, &'static str> {
    if bytes.len() < 6 || bytes[..2] != [255, 255] || bytes[3] as usize + 4 != bytes.len() {
        return Err("bad framing or length");
    }
    if bytes[2] != expected_id {
        return Err("foreign reply ID");
    }
    if bytes[2..].iter().fold(0u8, |a, b| a.wrapping_add(*b)) != 255 {
        return Err("bad checksum");
    }
    if bytes.len() != expected_width + 6 {
        return Err("unexpected payload width");
    }
    Ok(Reply {
        id: bytes[2],
        error: bytes[4],
        parameters: bytes[5..bytes.len() - 1].to_vec(),
    })
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Telemetry {
    pub position_raw: u16,
    pub position_rad: f64,
    pub speed_raw: u16,
    pub speed_rad_s: f64,
    pub load_raw: u16,
    pub voltage_raw: u8,
    pub voltage_v: f64,
    pub temperature_c: u8,
    pub status: u8,
    pub moving: u8,
    pub current_raw: u16,
    pub current_a_uncalibrated: f64,
}
impl Telemetry {
    /// A 15-byte read of 0x38..0x46. Voltage at 0x3e is ONE byte; 0x3f is temperature.
    /// Voltage scale 0.1 V/count; current's 0.001 A/count remains uncalibrated.
    pub fn decode(p: &[u8]) -> Result<Self, &'static str> {
        if p.len() != 15 {
            return Err("expected 15 register bytes");
        }
        let word = |i| u16::from_le_bytes([p[i], p[i + 1]]);
        let pos = word(0);
        let speed = word(2);
        let current = word(13);
        let signed_speed = (speed & 0x7fff) as f64 * if speed & 0x8000 != 0 { -1. } else { 1. };
        Ok(Self {
            position_raw: pos,
            position_rad: pos as f64 * std::f64::consts::TAU / 4096.,
            speed_raw: speed,
            speed_rad_s: signed_speed * std::f64::consts::TAU / 4096.,
            load_raw: word(4),
            voltage_raw: p[6],
            voltage_v: p[6] as f64 * 0.1,
            temperature_c: p[7],
            status: p[9],
            moving: p[10],
            current_raw: current,
            current_a_uncalibrated: current as f64 * 0.001,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn documented_position_reply() {
        let b = [255, 255, 1, 4, 0, 0x18, 5, 0xdd];
        assert_eq!(reply(&b, 1, 2).unwrap().parameters, [0x18, 5]);
        assert!(reply(&b, 2, 2).is_err());
        let mut bad = b;
        bad[6] = 6;
        assert!(reply(&bad, 1, 2).is_err());
        assert!(reply(&b[..7], 1, 2).is_err());
        assert!(packet(1, 3, &[0; 59]).is_err());
        assert_eq!(
            packet(1, 2, &[0x38, 2]).unwrap(),
            [255, 255, 1, 4, 2, 0x38, 2, 0xbe]
        );
    }
    #[test]
    fn voltage_is_not_temperature() {
        let mut b = [0; 15];
        b[0] = 0;
        b[1] = 8;
        b[2] = 1;
        b[3] = 0x80;
        b[6] = 104;
        b[7] = 49;
        let t = Telemetry::decode(&b).unwrap();
        assert!((t.voltage_v - 10.4).abs() < 1e-10);
        assert_eq!(t.temperature_c, 49);
        assert!(t.speed_rad_s < 0.);
        assert!((t.position_rad - std::f64::consts::PI).abs() < 1e-10);
    }
}
