//! Device-clock evidence for the autonomous experiment bridge.
//! The current commissioned image does not emit this protocol yet. Preserve raw
//! frames even when decoding/validation fails; neither timestamps nor current
//! readings here imply internal sensor sample time or calibrated electrical units.
use super::fpga_upload::{CLOCK_HZ, Upload};
use serde::{Deserialize, Serialize};

pub const STREAM_ID: u8 = 253;
pub const TAG: u8 = 0xa3;
pub const PREFIX_BYTES: usize = 31;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum Kind {
    Start = 0,
    Telemetry = 1,
    Control = 2,
    Audit = 3,
    Terminal = 4,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub kind: Kind,
    pub run_id: u32,
    pub frame: u16,
    pub motor_id: u8,
    pub outcome: u8,
    pub sequence: u8,
    pub request_ticks: u64,
    pub completion_ticks: u64,
    pub device_error: u8,
    /// Full reported payload size, including malformed overlong audit replies.
    pub reported_width: u8,
    /// Stored raw bytes; malformed audits retain at most the first 15 bytes.
    pub data: Vec<u8>,
}

fn word(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap())
}
fn dword(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn qword(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}

pub(crate) fn audit_matches(event: &Event, pwm: &[u16; 9]) -> bool {
    event.kind == Kind::Audit && (4..=12).contains(&event.motor_id)
        && event.reported_width == 6 && event.data.len() >= 6
        && event.device_error == 0 && event.data[0] == 1
        && word(&event.data, 4) == pwm[usize::from(event.motor_id - 4)]
}

impl Event {
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        if !(37..=64).contains(&bytes.len()) {
            return Err("Device event packet size outside protocol bounds".into());
        }
        let frame = crate::acquisition::servo_bus::reply(bytes, STREAM_ID, bytes.len() - 6)?;
        let p = frame.parameters;
        if frame.error != 0 || p[0] != 1 || p[1] != TAG || p[30] as usize != p.len() - PREFIX_BYTES
        {
            return Err("Invalid device event envelope or stored width".into());
        }
        let kind = match p[2] {
            0 => Kind::Start,
            1 => Kind::Telemetry,
            2 => Kind::Control,
            3 => Kind::Audit,
            4 => Kind::Terminal,
            _ => return Err("Unknown device event kind".into()),
        };
        let event = Self {
            kind,
            run_id: dword(&p, 3),
            frame: word(&p, 7),
            motor_id: p[9],
            outcome: p[10],
            sequence: p[11],
            request_ticks: qword(&p, 12),
            completion_ticks: qword(&p, 20),
            device_error: p[28],
            reported_width: p[29],
            data: p[PREFIX_BYTES..].to_vec(),
        };
        event.validate()?;
        Ok(event)
    }

    pub fn validate(&self) -> Result<(), String> {
        let global = matches!(self.kind, Kind::Start | Kind::Control | Kind::Terminal);
        if self.completion_ticks < self.request_ticks
            || (matches!(self.kind, Kind::Telemetry | Kind::Control | Kind::Audit)
                && self.completion_ticks == self.request_ticks)
            || self.frame >= 1200
            || (global && self.motor_id != 254)
            || (!global && !(4..=12).contains(&self.motor_id))
            || (self.kind != Kind::Telemetry && self.sequence != 0)
        {
            return Err("Invalid device event identity or timing".into());
        }
        let expected = match self.kind {
            Kind::Start => 16,
            Kind::Telemetry => 15,
            Kind::Control => 18,
            Kind::Audit => usize::from(self.reported_width.min(15)),
            Kind::Terminal => 27,
        };
        if self.data.len() != expected
            || (self.kind != Kind::Audit && usize::from(self.reported_width) != expected)
            || (self.kind != Kind::Audit && self.device_error != 0)
            || (matches!(self.kind, Kind::Start | Kind::Telemetry | Kind::Control)
                && self.outcome != 0)
            || (self.kind == Kind::Terminal && self.outcome > 8)
            || (self.kind == Kind::Audit
                && (self.reported_width > 58
                    || self.outcome > 1
                    || (self.outcome == 0 && (self.reported_width != 6 || self.device_error != 0))))
        {
            return Err("Invalid device event data shape or outcome".into());
        }
        if self.kind == Kind::Start
            && (self.frame != 0 || self.request_ticks != self.completion_ticks)
        {
            return Err("Invalid START event".into());
        }
        if self.kind == Kind::Terminal {
            let stop = qword(&self.data, 0);
            let stopped = qword(&self.data, 8);
            let interrupted = qword(&self.data, 16);
            let kind = self.data[24];
            let id = self.data[25];
            let present = self.data[26];
            if stop < self.request_ticks
                || stopped < stop
                || stopped != self.completion_ticks
                || present > 1
                || (present == 1
                    && (kind > 2
                        || interrupted < self.request_ticks
                        || interrupted > stop
                        || (kind == 1 && id != 254)
                        || (kind != 1 && !(4..=12).contains(&id))))
                || (present == 0 && (interrupted != 0 || kind != 0 || id != 0))
                || (self.outcome == 0 && present != 0)
            {
                return Err("Invalid terminal stop or interrupted-request evidence".into());
            }
        }
        Ok(())
    }
}

/// Raw device packets remain the durable source, including failed captures.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capture {
    pub upload: Upload,
    pub packets: Vec<Vec<u8>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Review {
    pub run_id: u32,
    pub source_plan_blake3: String,
    pub capture_blake3: String,
    pub clock_hz: u32,
    pub completion_scope: String,
    /// Complete protocol coverage and stop-pair transmission, NOT observed
    /// mechanical stationarity, model accuracy, or permission to score a fit.
    pub completed: bool,
    pub terminal_result: Option<u8>,
    pub completed_frames: usize,
    pub events: Vec<Event>,
}

impl Capture {
    /// Convert audited device windows to the shared simulation input format.
    /// Remains incomplete until an acquisition adapter verifies physical stopping.
    pub fn unverified_recording(&self) -> Result<super::fpga::Recording, String> {
        recording_from_review(&self.upload.source_plan, self.upload.homes, self.review()?)
    }

    /// Validate coverage, ordering, fixed device cadence, readback and source
    /// identity. A valid incomplete/failed capture remains unscored, not success.
    pub fn review(&self) -> Result<Review,String> {
        self.upload.validate()?;
        review_execution(&self.upload.source_plan, self.upload.homes, self.upload.plan_crc32, &self.packets)
    }
}

/// Shared event audit for sealed or streamed references. Callers must validate
/// the correspondence between the supplied references and accepted transport.
pub fn review_execution(plan:&super::fpga::Plan, homes:[u16;9], start_crc:u32, packets:&[Vec<u8>]) -> Result<Review,String> {
    plan.validate()?;
    review_validated_execution(plan, homes, start_crc, packets)
}

/// Same arithmetic, timing, coverage and stop checks, with explicitly offline
/// cadence bounds. Does not authorize acquisition or certify physical stopping.
pub fn review_offline_rate_study(plan:&super::fpga::Plan, homes:[u16;9], start_crc:u32, packets:&[Vec<u8>]) -> Result<Review,String> {
    plan.validate_offline_rate_study()?;
    let mut review=review_validated_execution(plan,homes,start_crc,packets)?;
    review.completion_scope=format!("Offline rate study only. {}",review.completion_scope);
    Ok(review)
}

fn review_validated_execution(plan:&super::fpga::Plan, homes:[u16;9], start_crc:u32, packets:&[Vec<u8>]) -> Result<Review,String> {
        let max_packets =
            plan.targets.len() * (plan.ids.len() * 2 + 1) + 2;
        if packets.len() > max_packets {
            return Err("Device capture exceeds source event count".into());
        }
        let events = super::fpga_batch::decode_events(packets)?;
        if events.len() > max_packets {
            return Err("Expanded device capture exceeds source event count".into());
        }
        let start = events.first().ok_or("Device capture has no START")?;
        if start.kind != Kind::Start {
            return Err("Device capture does not begin with START".into());
        }
        let mask = plan
            .ids
            .iter()
            .fold(0u16, |m, id| m | 1 << (id - 4));
        let frames = plan.targets.len();
        let period = (plan.period_s * f64::from(CLOCK_HZ)).round() as u64;
        if dword(&start.data, 0) != start_crc
            || word(&start.data, 4) != mask
            || usize::from(word(&start.data, 6)) != frames
            || u64::from(dword(&start.data, 8)) != period
            || dword(&start.data, 12) != CLOCK_HZ
        {
            return Err("START differs from sealed source experiment".into());
        }
        let ids: Vec<_> = (4..=12).filter(|id| mask & (1 << (id - 4)) != 0).collect();
        let per_frame = ids.len() * 2 + 1;
        let mut next = 0usize;
        let mut last_done = start.completion_ticks;
        let mut sequences = [None; 9];
        let mut pwm = [0u16; 9];
        let mut positions = [0u16; 9];
        let mut previous = [0u16; 9];
        let mut failed_audit = false;
        let mut terminal = None;
        for event in events.iter().skip(1) {
            if event.run_id != start.run_id || terminal.is_some() || event.kind == Kind::Start {
                return Err("Mixed, restarted or post-terminal device capture".into());
            }
            if event.kind == Kind::Terminal {
                let stop = qword(&event.data, 0);
                if event.request_ticks != start.request_ticks
                    || stop < last_done
                    || usize::from(event.frame) >= frames
                {
                    return Err("Terminal identity/timing differs from run".into());
                }
                if event.outcome == 0 {
                    let scheduled_stop = start
                        .request_ticks
                        .checked_add(period * frames as u64)
                        .ok_or("Device clock overflow")?;
                    if next != frames * per_frame
                        || failed_audit
                        || stop != scheduled_stop
                        || usize::from(event.frame) != frames - 1
                    {
                        return Err(
                            "Successful terminal lacks complete fixed-cadence evidence".into()
                        );
                    }
                }
                if failed_audit && event.outcome != 7 && event.outcome != 2 && event.outcome != 4 {
                    return Err("Failed audit has an inconsistent terminal result".into());
                }
                if event.outcome == 7 && !failed_audit {
                    return Err(
                        "Audit-failure terminal is missing its failed readback event".into(),
                    );
                }
                if event.outcome == 3 {
                    let deadline = start
                        .request_ticks
                        .checked_add(period * (u64::from(event.frame) + 1))
                        .ok_or("Device clock overflow")?;
                    if stop != deadline {
                        return Err("Deadline terminal moved the fixed frame boundary".into());
                    }
                }
                terminal = Some(event.outcome);
                continue;
            }
            if failed_audit || next >= frames * per_frame {
                return Err("Device run continued after failure or final frame".into());
            }
            let frame = next / per_frame;
            let offset = next % per_frame;
            let (kind, id) = if offset < ids.len() {
                (Kind::Telemetry, ids[offset])
            } else if offset == ids.len() {
                (Kind::Control, 254)
            } else {
                (Kind::Audit, ids[offset - ids.len() - 1])
            };
            let begin = start
                .request_ticks
                .checked_add(period * frame as u64)
                .ok_or("Device clock overflow")?;
            let end = begin.checked_add(period).ok_or("Device clock overflow")?;
            if event.kind != kind
                || event.motor_id != id
                || usize::from(event.frame) != frame
                || event.request_ticks < begin
                || event.request_ticks < last_done
                || event.completion_ticks >= end
            {
                return Err("Missing, reordered, overlapping or late device transaction".into());
            }
            if kind == Kind::Telemetry {
                let axis = usize::from(id - 4);
                if sequences[axis]
                    .is_some_and(|previous: u8| event.sequence != previous.wrapping_add(1))
                {
                    return Err("Missing or duplicate supervised telemetry sequence".into());
                }
                sequences[axis] = Some(event.sequence);
                let telemetry = crate::acquisition::servo_bus::Telemetry::decode(&event.data)?;
                if telemetry.position_raw > 4095 || telemetry.status != 0 {
                    return Err("Accepted telemetry has invalid encoder/status data".into());
                }
                positions[axis] = telemetry.position_raw;
            } else if kind == Kind::Control {
                for (axis, duty) in pwm.iter_mut().enumerate() {
                    *duty = word(&event.data, axis * 2);
                    let magnitude = *duty & !1024;
                    if magnitude > plan.gains.limit
                        || (mask & (1 << axis) == 0 && *duty != 0)
                        || *duty == 1024
                    {
                        return Err(
                            "Transmitted PWM outside source limits or noncanonical zero".into()
                        );
                    }
                    if mask & (1 << axis) != 0 {
                        let target = plan.targets[frame][axis];
                        let prior_target = if frame == 0 {
                            0
                        } else {
                            plan.targets[frame - 1][axis]
                        };
                        let calculated = sim_domain_control::fixed_pd::step(
                            plan.gains,
                            (i32::from(homes[axis]) + i32::from(target)) as u16,
                            positions[axis],
                            if frame == 0 {
                                positions[axis]
                            } else {
                                previous[axis]
                            },
                            target - prior_target,
                        )?;
                        let encoded =
                            calculated.unsigned_abs() | if calculated < 0 { 1024 } else { 0 };
                        if *duty != encoded {
                            return Err("Transmitted PWM differs from shared controller on recorded feedback".into());
                        }
                        previous[axis] = positions[axis];
                    }
                }
            } else if kind == Kind::Audit {
                let matches = audit_matches(event, &pwm);
                if (event.outcome == 0) != matches {
                    return Err("Audit outcome disagrees with actual torque/PWM readback".into());
                }
                failed_audit = !matches;
            }
            next += 1;
            last_done = event.completion_ticks;
        }
        Ok(Review {
            run_id: start.run_id,
            source_plan_blake3: blake3::hash(&serde_json::to_vec(plan).map_err(|e|e.to_string())?).to_hex().to_string(),
            capture_blake3: blake3::hash(&serde_json::to_vec(&(plan,homes,start_crc,packets)).map_err(|e| e.to_string())?).to_hex().to_string(),
            clock_hz: CLOCK_HZ,
            completion_scope: "Protocol coverage and stop-pair wire completion only. Mechanical stopping and model accuracy require separate evidence. Device-clock transaction windows are not internal sensor sample timestamps. Servo current is raw and uncalibrated.".into(),
            completed: terminal == Some(0),
            terminal_result: terminal,
            completed_frames: (next - usize::from(failed_audit)) / per_frame,
            events,
        })
    }

/// Build a recording only from a reviewed execution; physical stop is separate.
pub fn recording_from_review(plan: &super::fpga::Plan, homes:[u16;9], review:Review) -> Result<super::fpga::Recording,String> {
    use super::fpga::{Frame,Observation,Recording};
        let origin = review.events[0].request_ticks;
        let seconds = |ticks| (ticks - origin) as f64 / f64::from(CLOCK_HZ);
        let ids = &plan.ids;
        let mut frames = Vec::new();
        for (tick, events) in review.events[1..]
            .chunks(ids.len() * 2 + 1)
            .take(review.completed_frames)
            .enumerate()
        {
            let observations = events[..ids.len()]
                .iter()
                .map(|e| {
                    Ok(Observation {
                        id: e.motor_id,
                        request_s: seconds(e.request_ticks),
                        completion_s: seconds(e.completion_ticks),
                        telemetry: crate::acquisition::servo_bus::Telemetry::decode(&e.data)?,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            let control = &events[ids.len()];
            let mut pwm_readback = [0; 9];
            let mut torque_readback = [0; 9];
            for audit in &events[ids.len() + 1..] {
                let axis = usize::from(audit.motor_id - 4);
                let raw = word(&audit.data, 4);
                pwm_readback[axis] = (raw & 1023) as i16 * if raw & 1024 != 0 { -1 } else { 1 };
                torque_readback[axis] = audit.data[0];
            }
            frames.push(Frame {
                tick,
                observations,
                command_request_s: seconds(control.request_ticks),
                command_receipt_s: seconds(control.completion_ticks),
                pwm_readback,
                torque_readback: Some(torque_readback),
                arithmetic_matches: true,
            });
        }
        let terminal = review.events.last().filter(|e| e.kind == Kind::Terminal);
        Ok(Recording {
            version: 1,
            plan: plan.clone(),
            home: homes,
            frames,
            completed: false,
            failure: Some("Physical stopping has not been verified".into()),
            stop_verified: false,
            stop_request_s: terminal.map_or(0., |e| seconds(qword(&e.data, 0))),
            stop_receipt_s: terminal.map_or(0., |e| seconds(e.completion_ticks)),
            transactions_origin_host_s: 0.,
            sources: std::collections::BTreeMap::from([
                ("device_capture".into(), review.capture_blake3),
                (
                    "bitstream".into(),
                    plan.bitstream_blake3.clone(),
                ),
                (
                    "controller_ir".into(),
                    sim_domain_control::fixed_pd::implementation_identity(),
                ),
            ]),
            initial: serde_json::json!({"timing":"Device-clock UART transaction windows; not internal sensor timestamps", "clock_hz":CLOCK_HZ,"run_id":review.run_id}),
            recovery: serde_json::json!({}),
        })
    }
