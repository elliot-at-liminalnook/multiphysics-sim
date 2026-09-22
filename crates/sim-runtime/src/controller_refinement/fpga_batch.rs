//! Offline-capable decoder for bounded, compact FPGA frame captures (A4/v2).
//! Raw bytes are retained once, including unmatched/partial/corrupt traffic.
//! Successful read events reference the exact raw reply ending at `raw_end`.
use super::fpga_events::{Event, Kind};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Frame {
    pub run_id: u32,
    pub frame: u16,
    pub origin_ticks: u64,
    pub mask: u16,
    pub partial: bool,
    pub logging_stride: u8,
    pub diagnostic: bool,
    pub dropped_raw_bytes: u16,
    pub raw: Vec<u8>,
    pub events: Vec<Event>,
}

pub fn is_batch(p: &[u8]) -> bool {
    p.get(2) == Some(&253) && p.get(5..8) == Some(&[2, 0xa4, 1][..])
}
fn u16_at(p: &[u8], i: usize) -> u16 {
    u16::from_le_bytes(p[i..i + 2].try_into().unwrap())
}
fn u24_at(p: &[u8], i: usize) -> u64 {
    u64::from(p[i]) | (u64::from(p[i + 1]) << 8) | (u64::from(p[i + 2]) << 16)
}

impl Frame {
    pub fn decode(p: &[u8]) -> Result<Self, String> {
        if !(30..=255).contains(&p.len())
            || !is_batch(p)
            || p[0..2] != [255, 255]
            || usize::from(p[3]) + 4 != p.len()
            || p[4] != 0
            || p[2..].iter().fold(0u8, |s, b| s.wrapping_add(*b)) != 255
        {
            return Err("Invalid compact frame envelope/checksum".into());
        }
        let mask = u16_at(p, 24);
        let raw_len = usize::from(p[22]);
        let count = usize::from(p[23]);
        if mask == 0
            || mask > 511
            || mask.count_ones() > 3
            || raw_len > 128
            || count > 7
            || p[26] > 15
            || (p[26] & 2 != 0) != (u16_at(p, 27) != 0)
            || 29 + raw_len > p.len() - 1
        {
            return Err("Invalid compact frame bounds/flags".into());
        }
        let mut result = Self {
            run_id: u32::from_le_bytes(p[8..12].try_into().unwrap()),
            frame: u16_at(p, 12),
            origin_ticks: u64::from_le_bytes(p[14..22].try_into().unwrap()),
            mask,
            partial: p[26] & 1 != 0,
            logging_stride: if p[26] & 4 != 0 { 2 } else { 1 },
            diagnostic: p[26] & 8 != 0,
            dropped_raw_bytes: u16_at(p, 27),
            raw: p[29..29 + raw_len].to_vec(),
            events: vec![],
        };
        let mut at = 29 + raw_len;
        let mut previous_raw_end = 0;
        for _ in 0..count {
            if at + 13 > p.len() - 1 {
                return Err("Truncated compact event".into());
            }
            let kind = match p[at] {
                1 => Kind::Telemetry,
                2 => Kind::Control,
                3 => Kind::Audit,
                _ => return Err("Invalid compact event kind".into()),
            };
            let id = p[at + 1];
            let end = usize::from(p[at + 9]);
            let width = p[at + 12];
            if end > raw_len || end < previous_raw_end {
                return Err("Invalid raw reply reference".into());
            }
            let data = if kind == Kind::Control {
                let size = 2 * mask.count_ones() as usize;
                if id != 254 || at + 13 + size > p.len() - 1 {
                    return Err("Truncated scoped PWM".into());
                }
                let mut data = vec![0; 18];
                let mut n = 0;
                for axis in 0..9 {
                    if mask & (1 << axis) != 0 {
                        data[axis * 2..axis * 2 + 2].copy_from_slice(&p[at + 13 + n..at + 15 + n]);
                        n += 2;
                    }
                }
                data
            } else {
                if !(4..=12).contains(&id) || mask & (1 << (id - 4)) == 0 {
                    return Err("Read ID outside compact frame mask".into());
                }
                let length = usize::from(width) + 6;
                if length > end || end - length < previous_raw_end {
                    return Err("Overlapping/truncated raw reply reference".into());
                }
                let reply = &result.raw[end - length..end];
                // Check exact bytes, including the servo checksum. Error replies
                // remain evidence, rather than being replaced by a clean reply.
                if reply[..2] != [255, 255]
                    || reply[2] != id
                    || usize::from(reply[3]) + 4 != length
                    || reply[4] != p[at + 11]
                    || reply[2..].iter().fold(0u8, |s, b| s.wrapping_add(*b)) != 255
                {
                    return Err("Event does not reference a valid matching raw packet".into());
                }
                previous_raw_end = end;
                reply[5..5 + usize::from(width).min(15)].to_vec()
            };
            let request_ticks = result
                .origin_ticks
                .checked_add(u24_at(p, at + 3))
                .ok_or("Timestamp overflow")?;
            let completion_ticks = result
                .origin_ticks
                .checked_add(u24_at(p, at + 6))
                .ok_or("Timestamp overflow")?;
            let event = Event {
                kind,
                run_id: result.run_id,
                frame: result.frame,
                motor_id: id,
                outcome: p[at + 10],
                sequence: p[at + 2],
                request_ticks,
                completion_ticks,
                device_error: p[at + 11],
                reported_width: width,
                data,
            };
            event.validate()?;
            at += 13
                + if kind == Kind::Control {
                    2 * mask.count_ones() as usize
                } else {
                    0
                };
            result.events.push(event);
        }
        if at != p.len() - 1 {
            return Err("Trailing/unaccounted compact event bytes".into());
        }
        if !result.partial
            && (count != 2 * mask.count_ones() as usize + 1 || result.dropped_raw_bytes != 0)
        {
            return Err("Complete compact frame lacks lossless coverage".into());
        }
        Ok(result)
    }
}

/// Expand transport records into the existing shared timing/controller auditor.
/// A truncated raw capture remains decodable above but cannot pass this audit.
pub fn decode_events(packets: &[Vec<u8>]) -> Result<Vec<Event>, String> {
    let mut events = Vec::new();
    let mut start: Option<Event> = None;
    let mut next_frame = 0u16;
    let mut partial_frame = None;
    let mut terminal_seen = false;
    for p in packets {
        if terminal_seen {
            return Err("Post-terminal capture packet".into());
        }
        if is_batch(p) {
            let frame = Frame::decode(p)?;
            if frame.logging_stride != 1 {
                return Err("Decimated logging requires sampled capture review; full controller audit is unavailable".into());
            }
            if frame.dropped_raw_bytes != 0 {
                return Err("Raw capture overflow; evidence is incomplete".into());
            }
            let origin = start.as_ref().ok_or("Batch before START")?;
            let period = u32::from_le_bytes(origin.data[8..12].try_into().unwrap()) as u64;
            let expected = origin
                .request_ticks
                .checked_add(u64::from(frame.frame) * period)
                .ok_or("Frame clock overflow")?;
            if partial_frame.is_some()
                || frame.run_id != origin.run_id
                || frame.frame != next_frame
                || frame.mask != u16_at(&origin.data, 4)
                || frame.frame >= u16_at(&origin.data, 6)
                || frame.origin_ticks != expected
            {
                return Err("Batch identity, ordering, or clock differs from START".into());
            }
            if frame.partial {
                partial_frame = Some(frame.frame);
            } else {
                next_frame += 1;
            }
            events.extend(frame.events);
        } else {
            let event = Event::decode(p)?;
            if event.kind == Kind::Start {
                if start.is_some() {
                    return Err("Repeated START".into());
                }
                start = Some(event.clone());
            }
            if let Some(frame) = partial_frame {
                if event.kind != Kind::Terminal || event.outcome == 0 || event.frame != frame {
                    return Err("Partial batch requires a matching failed terminal".into());
                }
            }
            terminal_seen = event.kind == Kind::Terminal;
            events.push(event);
        }
    }
    Ok(events)
}

/// Intentionally sampled evidence. This is NOT a complete controller arithmetic
/// audit: the omitted cycle's encoder input is unavailable. Safety still executes
/// every cycle on the FPGA. Callers must explicitly select this review contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampledCapture {
    pub logging_stride: u8,
    pub complete_controller_evidence: bool,
    pub start: Event,
    pub terminal: Event,
    pub frames: Vec<Frame>,
    pub intentionally_unlogged_frames: Vec<u16>,
}

pub fn decode_sampled_capture(
    packets: &[Vec<u8>],
    logging_stride: u8,
) -> Result<SampledCapture, String> {
    if logging_stride != 2 || packets.len() < 2 {
        return Err("Expected explicit stride-two capture with boundaries".into());
    }
    let start = Event::decode(&packets[0])?;
    let terminal = Event::decode(packets.last().unwrap())?;
    if start.kind != Kind::Start
        || terminal.kind != Kind::Terminal
        || start.run_id != terminal.run_id
    {
        return Err("Missing/mismatched sampled capture boundaries".into());
    }
    let total = u16_at(&start.data, 6);
    let period = u32::from_le_bytes(start.data[8..12].try_into().unwrap()) as u64;
    let mask = u16_at(&start.data, 4);
    if total == 0 || total > 1200 || period == 0 || terminal.frame >= total {
        return Err("Invalid sampled run bounds".into());
    }
    if terminal.outcome == 0 && terminal.frame + 1 != total {
        return Err("Early successful terminal".into());
    }
    let mut frames = Vec::new();
    let mut expected = 0u16;
    let mut last = None;
    let mut partial_seen = false;
    for p in &packets[1..packets.len() - 1] {
        let f = Frame::decode(p)?;
        let diagnostic_odd =
            f.frame % 2 == 1 && (f.partial || f.diagnostic) && f.frame + 1 == expected;
        let origin = start
            .request_ticks
            .checked_add(u64::from(f.frame) * period)
            .ok_or("Clock overflow")?;
        if partial_seen
            || f.run_id != start.run_id
            || f.mask != mask
            || f.logging_stride != logging_stride
            || f.frame >= total
            || f.frame > terminal.frame
            || f.origin_ticks != origin
            || last.is_some_and(|n| f.frame <= n)
            || (f.frame != expected && !diagnostic_odd)
            || f.dropped_raw_bytes != 0
        {
            return Err("Sampled frame identity, stride, ordering, or coverage violation".into());
        }
        if f.partial && (terminal.outcome == 0 || terminal.frame != f.frame) {
            return Err("Partial sampled evidence needs matching failed terminal".into());
        }
        let ids: Vec<_> = (4u8..=12)
            .filter(|id| mask & (1 << (id - 4)) != 0)
            .collect();
        let mut previous = origin;
        let mut pwm = [0u16; 9];
        for (i, e) in f.events.iter().enumerate() {
            let (kind, id) = if i < ids.len() {
                (Kind::Telemetry, ids[i])
            } else if i == ids.len() {
                (Kind::Control, 254)
            } else {
                (
                    Kind::Audit,
                    *ids.get(i - ids.len() - 1).ok_or("Excess sampled event")?,
                )
            };
            if e.kind != kind
                || e.motor_id != id
                || e.request_ticks < previous
                || e.completion_ticks >= origin + period
            {
                return Err("Sampled transaction ordering/timing violation".into());
            }
            if e.outcome != 0 && !f.diagnostic {
                return Err("Unmarked diagnostic event".into());
            }
            if e.kind == Kind::Control {
                for (axis, duty) in pwm.iter_mut().enumerate() {
                    *duty = u16_at(&e.data, axis * 2);
                }
            } else if e.kind == Kind::Audit
                && ((e.outcome == 0) != super::fpga_events::audit_matches(e, &pwm))
            {
                return Err("Sampled audit outcome disagrees with torque/PWM readback".into());
            }
            previous = e.completion_ticks;
        }
        partial_seen = f.partial;
        if f.frame == expected {
            expected = expected.checked_add(2).ok_or("Frame overflow")?;
        }
        last = Some(f.frame);
        frames.push(f);
    }
    // Even frames must be present up to the last fully completed cycle. A failed
    // terminal may precede any measurable event in its final cycle.
    let completed_exclusive = if terminal.outcome == 0 {
        total
    } else {
        terminal.frame
    };
    for frame in (0..completed_exclusive).step_by(2) {
        if !frames.iter().any(|f| f.frame == frame && !f.partial) {
            return Err("Missing scheduled sampled frame".into());
        }
    }
    if terminal.completion_ticks
        < frames.last().map_or(start.completion_ticks, |f| {
            f.events
                .last()
                .map_or(f.origin_ticks, |e| e.completion_ticks)
        })
    {
        return Err("Terminal predates sampled evidence".into());
    }
    let intentionally_unlogged_frames = (0..completed_exclusive)
        .filter(|n| n % 2 == 1 && !frames.iter().any(|f| f.frame == *n))
        .collect();
    Ok(SampledCapture {
        logging_stride,
        complete_controller_evidence: false,
        start,
        terminal,
        frames,
        intentionally_unlogged_frames,
    })
}
