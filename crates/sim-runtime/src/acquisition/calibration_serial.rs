//! Serial transport for supervised, operator-triggered encoder teaching.
//! Uses the shared HX packet/telemetry contract; never advances simulation.
use super::{
    calibration::{AxisCalibration, EncoderTurns},
    calibration_sweep::{MotionCommand, RangeSweep, SweepInput, SweepSample, SweepTuning},
    servo_bus::{
        HX30HM_PWM_DIRECTION_BIT, PacketBuffer, Telemetry, packet, reply,
        signed_pwm_write_parameters,
    },
};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::fd::AsRawFd,
    os::unix::fs::OpenOptionsExt,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
type R<T> = Result<T, String>;
#[derive(Debug, serde::Serialize)]
pub struct JogSample {
    pub elapsed_ms: u128,
    pub position_raw: i32,
}
#[derive(Debug, serde::Serialize)]
pub struct JogOutcome {
    pub motor_id: u8,
    pub samples: Vec<JogSample>,
    pub telemetry: Telemetry,
    pub start_position_raw: i32,
    pub target_position_raw: i32,
    pub requested_counts: i16,
    pub actual_counts: i32,
    pub drive_pwm: u16,
    pub target_reached: bool,
    pub reason: String,
    pub stop_reply_recoveries: u64,
    pub overshoot_counts: u32,
    pub outside_jog_window: bool,
    pub motion_error: Option<String>,
}
#[derive(Debug, serde::Serialize)]
pub struct SweepOutcome {
    pub telemetry: Telemetry,
    pub motion_error: Option<String>,
}
pub struct CalibrationBus {
    file: File,
    pending: PacketBuffer,
    log: File,
    stop_reply_recoveries: u64,
    encoders: [EncoderTurns; 3],
}
impl CalibrationBus {
    pub fn open(port: &str, log: &Path) -> R<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(if cfg!(target_os = "macos") { 4 } else { 2048 })
            .open(port)
            .map_err(|e| e.to_string())?;
        unsafe extern "C" {
            fn flock(fd: i32, operation: i32) -> i32;
        }
        // Advisory exclusion across calibration instances; TIOCEXCL excludes other serial openers.
        if unsafe { flock(file.as_raw_fd(), 2 | 4) } != 0 {
            return Err("Serial port is already owned".into());
        }
        unsafe extern "C" {
            fn ioctl(fd: i32, request: std::ffi::c_ulong, ...) -> i32;
        }
        #[cfg(target_os = "macos")]
        if unsafe { ioctl(file.as_raw_fd(), 0x2000740d) } != 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        // Configure the existing descriptor through stty's stdin; no second serial opener.
        let copy = file.try_clone().map_err(|e| e.to_string())?;
        if !std::process::Command::new("stty")
            .args([
                "115200", "raw", "-echo", "clocal", "-hupcl", "min", "0", "time", "0",
            ])
            .stdin(copy)
            .status()
            .map_err(|e| e.to_string())?
            .success()
        {
            return Err("Serial setup failed".into());
        }
        Ok(Self {
            file,
            pending: PacketBuffer::default(),
            stop_reply_recoveries: 0,
            encoders: [EncoderTurns::default(); 3],
            log: OpenOptions::new()
                .create(true)
                .append(true)
                .open(log)
                .map_err(|e| e.to_string())?,
        })
    }
    fn txn(&mut self, id: u8, instruction: u8, params: &[u8], width: usize) -> R<Vec<u8>> {
        if !self.pending.pending().is_empty() {
            writeln!(
                self.log,
                "{}",
                serde_json::json!({"event":"unexpected_pending","bytes":self.pending.pending()})
            )
            .map_err(|e| e.to_string())?;
            return Err("Unexpected queued serial data; use Read connected motors to reconnect while stopped".into());
        }
        let tx = packet(id, instruction, params).map_err(str::to_string)?;
        self.log_event(serde_json::json!({"event":"request","tx":tx}))?;
        self.file.write_all(&tx).map_err(|e| e.to_string())?;
        let deadline = Instant::now() + Duration::from_millis(150);
        loop {
            if let Some(rx) = self.pending.next_packet().map_err(str::to_string)? {
                writeln!(self.log,"{}",serde_json::json!({"unix_ms":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis(),"tx":tx,"rx":rx})).map_err(|e|e.to_string())?;
                let r = reply(&rx, id, width).map_err(str::to_string)?;
                if r.error != 0 {
                    return Err(format!("ID {id}: device error {}", r.error));
                }
                return Ok(r.parameters);
            }
            if Instant::now() > deadline {
                self.log_event(serde_json::json!({"event":"reply_timeout","tx":tx,"timeout_ms":150,"pending":self.pending.pending()}))?;
                return Err(format!(
                    "ID {id}: serial reply timeout ({} reply bytes received)",
                    self.pending.pending().len()
                ));
            }
            let mut b = [0; 128];
            match self.file.read(&mut b) {
                Ok(n) => {
                    if n > 0 {
                        writeln!(
                            self.log,
                            "{}",
                            serde_json::json!({"event":"rx_chunk","bytes":&b[..n]})
                        )
                        .map_err(|e| e.to_string())?;
                    }
                    self.pending.push(&b[..n]).map_err(str::to_string)?;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
                Err(e) => return Err(e.to_string()),
            };
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn log_event(&mut self, mut value: serde_json::Value) -> R<()> {
        value["unix_ms"] = serde_json::json!(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        );
        writeln!(self.log, "{value}").map_err(|e| e.to_string())
    }
    pub fn reconnect_stopped(&mut self, id: u8) -> R<Telemetry> {
        self.resync_stopped()?;
        self.stop(id)
    }
    // Recovery is exclusively a STOP and readback path. Never retry drive writes.
    fn resync_stopped(&mut self) -> R<()> {
        let stop = packet(254, 0xa0, &[0]).map_err(str::to_string)?;
        self.file.write_all(&stop).map_err(|e| e.to_string())?;
        writeln!(self.log,"{}",serde_json::json!({"event":"explicit_reconnect","retained_pending":self.pending.pending(),"tx":stop})).map_err(|e|e.to_string())?;
        let start = Instant::now();
        let mut quiet = Instant::now();
        let mut b = [0; 128];
        loop {
            match self.file.read(&mut b) {
                Ok(n) if n > 0 => {
                    quiet = Instant::now();
                    writeln!(
                        self.log,
                        "{}",
                        serde_json::json!({"event":"stopped_reconnect_drain","bytes":&b[..n]})
                    )
                    .map_err(|e| e.to_string())?;
                }
                Ok(_) => (),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
                Err(e) => return Err(e.to_string()),
            }
            if quiet.elapsed() > Duration::from_millis(100) {
                break;
            }
            if start.elapsed() > Duration::from_secs(1) {
                return Err("Serial bus will not become quiet; check signal wiring".into());
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        self.pending = PacketBuffer::default();
        Ok(())
    }
    pub fn read(&mut self, id: u8, reg: u8, n: u8) -> R<Vec<u8>> {
        self.txn(id, 2, &[reg, n], n as usize)
    }
    fn write(&mut self, id: u8, reg: u8, values: &[u8]) -> R<()> {
        let mut p = vec![reg];
        p.extend(values);
        self.txn(id, 3, &p, 0)?;
        if self.read(id, reg, values.len() as u8)? != values {
            return Err("Register write did not verify".into());
        }
        Ok(())
    }
    pub fn reset_turn_tracking(&mut self) {
        self.encoders = [EncoderTurns::default(); 3];
    }
    pub fn feedback(&mut self, id: u8) -> R<Telemetry> {
        if !(1..=3).contains(&id) {
            return Err("Calibration profile only supports IDs 1..3".into());
        }
        let mut t = Telemetry::decode(&self.read(id, 0x38, 15)?).map_err(str::to_string)?;
        t.position_continuous = Some(self.encoders[(id - 1) as usize].observe(t.position_raw)?);
        Ok(t)
    }
    pub fn supervisor(&mut self, params: &[u8]) -> R<Vec<u8>> {
        let s = self.txn(254, 0xa0, params, 13)?;
        if s[0] != 5 || s[8..13] != [60, 90, 126, 208, 7] || s[4] & !7 != 0 || s[5] != 0 {
            return Err("Calibration FPGA profile is not loaded; motion disabled".into());
        }
        Ok(s)
    }
    fn arm(&mut self, id: u8, lo: i32, hi: i32) -> R<()> {
        self.supervisor(&[0])?;
        let anchor = self
            .feedback(id)?
            .position_continuous
            .ok_or("Missing continuous encoder coordinate")?;
        let mut parameters = vec![7, id];
        parameters.extend(anchor.to_le_bytes());
        parameters.extend(lo.to_le_bytes());
        parameters.extend(hi.to_le_bytes());
        self.supervisor(&parameters)?;
        self.feedback(id)?;
        let s = self.supervisor(&[1, id])?;
        if s[1] != 0 || s[4] != (1 << (id - 1)) {
            return Err("FPGA refused target-window arm".into());
        }
        Ok(())
    }
    pub fn stop(&mut self, id: u8) -> R<Telemetry> {
        let mut failures = Vec::new();
        for attempt in 0..3 {
            match self.stop_once(id) {
                Ok(t) => {
                    self.log_event(serde_json::json!({"event":"stop_verified","id":id,"position_raw":t.position_raw,"retries":attempt}))?;
                    return Ok(t);
                }
                Err(e) => {
                    self.log_event(serde_json::json!({"event":"stop_verification_failed","id":id,"attempt":attempt,"error":e}))?;
                    failures.push(e);
                    if attempt < 2 {
                        self.resync_stopped()?;
                        self.stop_reply_recoveries += 1;
                    }
                }
            }
        }
        Err(format!(
            "Stop readback unverified after bounded retries: {}",
            failures.join("; ")
        ))
    }
    fn stop_once(&mut self, id: u8) -> R<Telemetry> {
        // Transmit STOP even if the receive parser is faulted. Never report a physical
        // stop from this write alone; normal checksum/readback checks below still apply.
        if !self.pending.pending().is_empty() {
            let stop = packet(254, 0xa0, &[0]).map_err(str::to_string)?;
            self.file.write_all(&stop).map_err(|e| e.to_string())?;
            writeln!(self.log,"{}",serde_json::json!({"event":"emergency_stop_sent","tx":stop,"unparsed_bytes":self.pending.pending()})).map_err(|e|e.to_string())?;
            return Err("STOP sent; receive stream fault prevents physical verification. Cut motor power, then reconnect.".into());
        }
        self.supervisor(&[0])?;
        // The FPGA repeats PWM=0/torque-off independently. Wait for physical readback.
        let deadline = Instant::now() + Duration::from_millis(700);
        let mut prev = None;
        let mut stationary = 0;
        while Instant::now() < deadline {
            let t = self.feedback(id)?;
            let off = self.read(id, 0x28, 1)? == [0];
            let zero = self.read(id, 0x2c, 2)? == [0, 0];
            if off && zero && t.speed_raw == 0 && prev == Some(t.position_raw) {
                stationary += 1
            } else {
                stationary = 0
            }
            if stationary >= 2 {
                return Ok(t);
            }
            prev = Some(t.position_raw);
            std::thread::sleep(Duration::from_millis(15));
        }
        Err("Stop not verified: cut motor supply power".into())
    }
    fn healthy(t: &Telemetry) -> R<()> {
        if t.position_raw > 4095
            || t.temperature_c >= 55
            || !(9.0..=12.6).contains(&t.voltage_v)
            || t.status != 0
            || t.current_raw >= 500
        {
            return Err("Feedback outside provisional commissioning limits".into());
        }
        Ok(())
    }
    /// Proves local deadlines at ZERO drive. This is not a mechanical stopping-distance test.
    pub fn prove_watchdogs(&mut self, id: u8) -> R<Telemetry> {
        let t = self.stop(id)?;
        Self::healthy(&t)?;
        let position = t.position_continuous.ok_or("Missing encoder coordinate")?;
        let lo = position - 8;
        let hi = position + 8;
        self.arm(id, lo, hi)?;
        std::thread::sleep(Duration::from_millis(350));
        let s = self.supervisor(&[2])?;
        if s[1] != 1 || s[2] != 7 || s[4] != 0 {
            return Err("Independent telemetry-loss watchdog not verified".into());
        }
        self.arm(id, lo, hi)?;
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(350) {
            self.feedback(id)?;
            std::thread::sleep(Duration::from_millis(20));
        }
        let s = self.supervisor(&[2])?;
        if s[1] != 1 || s[2] != 8 || s[4] != 0 {
            return Err("Independent command-loss watchdog not verified".into());
        }
        self.stop(id)
    }
    fn prepare_pwm(&mut self, id: u8, lo: i32, hi: i32, cancel: &AtomicBool) -> R<()> {
        self.arm(id, lo, hi)?;
        self.write(id, 0x28, &[0])?;
        // Torque-off readback disarms; rearm after clearing stored PWM and speed.
        self.write(id, 0x2c, &[0, 0])?;
        self.feedback(id)?;
        self.supervisor(&[1, id])?;
        if self.read(id, 0x21, 1)? != [2] {
            self.write(id, 0x37, &[0])?;
            self.write(id, 0x21, &[2])?;
        }
        self.write(id, 0x37, &[1])?;
        self.feedback(id)?;
        self.supervisor(&[3, id])?;
        if cancel.load(Ordering::SeqCst) {
            return Err("Operator stop before enabling drive".into());
        }
        self.write(id, 0x28, &[1])?;
        Ok(())
    }
    pub fn sweep(
        &mut self,
        id: u8,
        axis: &AxisCalibration,
        clearance: u16,
        tuning: &SweepTuning,
        cancel: &AtomicBool,
        mut controls: impl FnMut() -> R<Option<SweepInput>>,
        mut observe: impl FnMut(&Telemetry, &SweepSample),
    ) -> R<SweepOutcome> {
        self.controlled_motion(
            id,
            axis,
            clearance,
            tuning,
            cancel,
            false,
            || Ok(controls()?.map(|i| (i, MotionCommand::Sweep, axis.clone()))),
            |t, s| {
                observe(t, s);
                Ok(())
            },
        )
    }
    pub fn controlled_motion(
        &mut self,
        id: u8,
        axis: &AxisCalibration,
        clearance: u16,
        tuning: &SweepTuning,
        cancel: &AtomicBool,
        teaching: bool,
        mut controls: impl FnMut() -> R<Option<(SweepInput, MotionCommand, AxisCalibration)>>,
        mut observe: impl FnMut(&Telemetry, &SweepSample) -> R<()>,
    ) -> R<SweepOutcome> {
        let initial = self.stop(id)?;
        Self::healthy(&initial)?;
        let mut sweep = if teaching {
            RangeSweep::teaching(
                axis,
                initial
                    .position_continuous
                    .ok_or("Missing encoder coordinate")?,
                tuning.clone(),
            )?
        } else {
            RangeSweep::new(
                axis,
                initial
                    .position_continuous
                    .ok_or("Missing encoder coordinate")?,
                clearance,
                tuning.clone(),
            )?
        };
        self.log_event(serde_json::json!({"event":"sweep_start","id":id,"axis":axis,"clearance_counts":clearance,"tuning":tuning}))?;
        let run = (|| -> R<()> {
            if controls()?.is_none() || cancel.load(Ordering::SeqCst) {
                return Ok(());
            }
            self.prepare_pwm(id, sweep.raw_lower, sweep.raw_upper, cancel)?;
            let start = Instant::now();
            let mut other_poll = Instant::now();
            loop {
                if cancel.load(Ordering::SeqCst) {
                    break;
                }
                let tick = Instant::now();
                if other_poll.elapsed() > Duration::from_millis(100) {
                    for other in 1..=3 {
                        if other != id {
                            self.feedback(other)?;
                        }
                    }
                    other_poll = Instant::now();
                }
                let t = self.feedback(id)?;
                Self::healthy(&t)?;
                let Some((input, command, bounds)) = controls()? else {
                    break;
                };
                if teaching {
                    sweep.set_taught_bounds(&bounds)?;
                }
                sweep.observe_environment(t.voltage_v, t.temperature_c);
                let sample = sweep.control(
                    start.elapsed().as_secs_f64(),
                    t.position_continuous.ok_or("Missing encoder coordinate")?,
                    input,
                    command,
                )?;
                let status = self.supervisor(&[3, id])?;
                if status[1] != 0 || status[4] != (1 << (id - 1)) {
                    return Err("FPGA stopped continuous traversal".into());
                }
                if cancel.load(Ordering::SeqCst) {
                    break;
                }
                let p = signed_pwm_write_parameters(sample.pwm, HX30HM_PWM_DIRECTION_BIT)
                    .map_err(str::to_string)?;
                self.txn(id, 3, &p, 0)?;
                self.log_event(
                    serde_json::json!({"event":"sweep_sample","id":id,"sample":sample}),
                )?;
                observe(&t, &sample)?;
                if let Some(wait) =
                    Duration::from_secs_f64(tuning.period_s).checked_sub(tick.elapsed())
                {
                    std::thread::sleep(wait);
                }
            }
            Ok(())
        })();
        // No serial read or drive retry while energized. Recovery is solely inside STOP.
        let t = self.stop(id)?;
        self.log_event(serde_json::json!({"event":"sweep_end","id":id,"motion_error":run.as_ref().err(),"telemetry":t}))?;
        Ok(SweepOutcome {
            telemetry: t,
            motion_error: run.err(),
        })
    }
    pub fn jog(
        &mut self,
        id: u8,
        axis: &AxisCalibration,
        delta: i16,
        drive_pwm: u16,
        cancel: &AtomicBool,
    ) -> R<JogOutcome> {
        if drive_pwm > 1000 {
            return Err("PWM must be between 0% and 100% (0..1000 raw)".into());
        }
        let recoveries_before = self.stop_reply_recoveries;
        let t = self.stop(id)?;
        Self::healthy(&t)?;
        let initial = t.position_continuous.ok_or("Missing encoder coordinate")?;
        let (target, lo, hi) = axis.jog(
            t.position_continuous.ok_or("Missing encoder coordinate")?,
            delta,
        )?;
        let observed_start = Instant::now();
        let mut samples = vec![JogSample {
            elapsed_ms: 0,
            position_raw: initial,
        }];
        let run = (|| {
            self.prepare_pwm(id, lo, hi, cancel)?;
            let start = Instant::now();
            loop {
                if cancel.load(Ordering::SeqCst) {
                    break;
                }
                let t = self.feedback(id)?;
                samples.push(JogSample {
                    elapsed_ms: observed_start.elapsed().as_millis(),
                    position_raw: t.position_continuous.ok_or("Missing encoder coordinate")?,
                });
                Self::healthy(&t)?;
                if t.position_continuous.ok_or("Missing encoder coordinate")? < lo
                    || t.position_continuous.ok_or("Missing encoder coordinate")? > hi
                {
                    return Err("Encoder left commanded jog window".into());
                }
                if (delta > 0
                    && t.position_continuous.ok_or("Missing encoder coordinate")? >= target)
                    || (delta < 0
                        && t.position_continuous.ok_or("Missing encoder coordinate")? <= target)
                {
                    break;
                }
                if start.elapsed() > Duration::from_millis(if drive_pwm > 25 { 80 } else { 300 }) {
                    break;
                }
                let s = self.supervisor(&[3, id])?;
                if s[1] != 0 || s[4] != (1 << (id - 1)) {
                    return Err("FPGA stopped the jog".into());
                }
                // Explicit operator-selected duty; never escalates automatically.
                let p = signed_pwm_write_parameters(
                    if delta > 0 {
                        drive_pwm as i16
                    } else {
                        -(drive_pwm as i16)
                    },
                    HX30HM_PWM_DIRECTION_BIT,
                )
                .map_err(str::to_string)?;
                self.txn(id, 3, &p, 0)?;
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(())
        })();
        let stopped = self.stop(id);
        match (run, stopped) {
            (_, Err(e)) => Err(e),
            (run, Ok(t)) => {
                let motion_error = run.err();
                let reached = if delta > 0 {
                    t.position_continuous.ok_or("Missing encoder coordinate")? >= target
                } else {
                    t.position_continuous.ok_or("Missing encoder coordinate")? <= target
                };
                let actual = t.position_continuous.ok_or("Missing encoder coordinate")? as i32
                    - initial as i32;
                let overshoot_counts = if reached {
                    t.position_continuous
                        .ok_or("Missing encoder coordinate")?
                        .abs_diff(target)
                } else {
                    0
                };
                let outside_jog_window =
                    t.position_continuous.ok_or("Missing encoder coordinate")? < lo
                        || t.position_continuous.ok_or("Missing encoder coordinate")? > hi;
                samples.push(JogSample {
                    elapsed_ms: observed_start.elapsed().as_millis(),
                    position_raw: t.position_continuous.ok_or("Missing encoder coordinate")?,
                });
                Ok(JogOutcome {
                    motor_id: id,
                    samples,
                    telemetry: t,
                    start_position_raw: initial,
                    target_position_raw: target,
                    requested_counts: delta,
                    actual_counts: actual,
                    drive_pwm,
                    stop_reply_recoveries: self.stop_reply_recoveries - recoveries_before,
                    overshoot_counts,
                    outside_jog_window,
                    target_reached: reached,
                    reason: if let Some(e) = &motion_error {
                        format!("Jog interrupted: {e}; stopped and verified")
                    } else if cancel.load(Ordering::SeqCst) {
                        "Released or stopped by operator; stopped and verified".into()
                    } else if reached {
                        "Target reached; stopped and verified".into()
                    } else if actual == 0 {
                        "Timed out with no net encoder movement; stopped and verified".into()
                    } else {
                        "Timed out short of target; stopped and verified".into()
                    },
                    motion_error,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixStream;
    fn pair() -> (CalibrationBus, UnixStream) {
        let (a, b) = UnixStream::pair().unwrap();
        a.set_nonblocking(true).unwrap();
        b.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let fd: OwnedFd = a.into();
        let file = File::from(fd);
        (
            CalibrationBus {
                file,
                pending: PacketBuffer::default(),
                stop_reply_recoveries: 0,
                encoders: [EncoderTurns::default(); 3],
                log: File::options().write(true).open("/dev/null").unwrap(),
            },
            b,
        )
    }
    fn request(s: &mut UnixStream) -> Vec<u8> {
        let mut h = [0; 4];
        s.read_exact(&mut h).unwrap();
        let mut r = h.to_vec();
        let mut rest = vec![0; h[3] as usize];
        s.read_exact(&mut rest).unwrap();
        r.extend(rest);
        r
    }
    #[test]
    fn faulted_parser_still_transmits_stop() {
        let (mut bus, mut peer) = pair();
        bus.pending.push(&[12, 34]).unwrap();
        assert!(bus.stop_once(3).unwrap_err().contains("STOP sent"));
        assert_eq!(request(&mut peer), packet(254, 0xa0, &[0]).unwrap());
        assert_eq!(bus.pending.pending(), [12, 34]);
    }
    #[test]
    fn wrong_profile_never_arms() {
        let (mut bus, mut peer) = pair();
        let t = std::thread::spawn(move || {
            assert_eq!(request(&mut peer), packet(254, 0xa0, &[2]).unwrap());
            peer.write_all(
                &packet(254, 0, &[1, 1, 1, 254, 0, 0, 0, 0, 60, 90, 126, 208, 7]).unwrap(),
            )
            .unwrap();
        });
        assert!(bus.supervisor(&[2]).unwrap_err().contains("profile"));
        t.join().unwrap();
    }
    fn exercise_jog(drive: u16, moves: bool, fault: u8) {
        let (mut bus, mut peer) = pair();
        let t = std::thread::spawn(move || {
            let (mut pos, mut torque, mut pwm, mut mode, mut lock, mut armed) =
                (2048u16, 0u8, 0u16, 0u8, 1u8, 0u8);
            let mut writes = Vec::new();
            let (mut driven, mut stopping, mut injected) = (false, false, false);
            loop {
                let mut h = [0; 4];
                if peer.read_exact(&mut h).is_err() {
                    break;
                }
                let mut rest = vec![0; h[3] as usize];
                peer.read_exact(&mut rest).unwrap();
                let id = h[2];
                let inst = rest[0];
                let p = &rest[1..rest.len() - 1];
                let values = if id == 254 {
                    match p[0] {
                        0 => {
                            stopping = driven;
                            armed = 0;
                            torque = 0;
                            pwm = 0
                        }
                        1 => armed = 4,
                        3 => (),
                        7 => {
                            assert_eq!(p.len(), 14);
                        }
                        _ => (),
                    };
                    vec![
                        5,
                        if armed == 0 { 1 } else { 0 },
                        0,
                        254,
                        armed,
                        0,
                        4,
                        0,
                        60,
                        90,
                        126,
                        208,
                        7,
                    ]
                } else if inst == 2 {
                    match p[0] {
                        0x38 => {
                            if torque == 1 && pwm > 0 && moves {
                                pos += 1
                            }
                            let mut v = vec![0; 15];
                            v[0..2].copy_from_slice(&pos.to_le_bytes());
                            v[6] = 120;
                            v[7] = 30;
                            v
                        }
                        0x28 => {
                            if torque == 0 {
                                armed = 0
                            }
                            vec![torque]
                        }
                        0x2c => pwm.to_le_bytes().to_vec(),
                        0x21 => vec![mode],
                        0x37 => vec![lock],
                        _ => panic!("Unexpected register"),
                    }
                } else {
                    assert_eq!(inst, 3);
                    writes.push(p.to_vec());
                    match p[0] {
                        0x28 => {
                            if p[1] != 0 {
                                assert_eq!(pwm, 0);
                                assert_eq!(mode, 2)
                            }
                            torque = p[1]
                        }
                        0x2c => {
                            pwm = u16::from_le_bytes([p[1], p[2]]);
                            assert!(pwm <= drive);
                            driven |= pwm > 0;
                        }
                        0x21 => {
                            assert_eq!(torque, 0);
                            mode = p[1]
                        }
                        0x37 => lock = p[1],
                        _ => panic!("Unexpected write"),
                    };
                    vec![]
                };
                let response = packet(id, 0, &values).unwrap();
                if !injected
                    && ((fault == 1 && stopping && inst == 2 && p[0] == 0x38)
                        || (fault == 2 && inst == 3 && p[0] == 0x2c && pwm > 0))
                {
                    injected = true;
                    peer.write_all(&response[..if fault == 1 { 14 } else { 4 }])
                        .unwrap();
                } else {
                    peer.write_all(&response).unwrap();
                }
            }
            if fault > 0 {
                assert!(injected);
            }
            if fault == 2 {
                assert_eq!(
                    writes
                        .iter()
                        .filter(|p| p[0] == 0x2c && p[1..] != [0, 0])
                        .count(),
                    1,
                    "Never retry active drive after lost ACK"
                );
            }
            assert_eq!(torque, 0);
            assert_eq!(pwm, 0);
            assert!(
                writes
                    .iter()
                    .any(|p| p == &vec![0x2c, drive as u8, (drive >> 8) as u8])
            );
        });
        let axis = AxisCalibration {
            role: "test axis".into(),
            ..Default::default()
        };
        let result = bus.jog(3, &axis, 1, drive, &AtomicBool::new(false));
        if fault == 2 {
            assert!(
                result
                    .unwrap()
                    .motion_error
                    .unwrap()
                    .contains("serial reply timeout")
            );
            drop(bus);
            t.join().unwrap();
            return;
        }
        let result = result.unwrap();
        assert_eq!(result.stop_reply_recoveries, if fault == 1 { 1 } else { 0 });
        assert_eq!(
            result.telemetry.position_raw,
            if moves { 2049 } else { 2048 }
        );
        assert_eq!(result.target_reached, moves);
        if !moves {
            assert!(result.reason.contains("no net encoder movement"));
        }
        drop(bus);
        t.join().unwrap();
    }
    #[test]
    fn low_drive_target_and_verified_stop() {
        exercise_jog(25, true, 0)
    }
    #[test]
    fn ten_percent_target_and_verified_stop() {
        exercise_jog(100, true, 0)
    }
    #[test]
    fn stalled_ten_percent_times_out_without_escalation() {
        exercise_jog(100, false, 0)
    }
    #[test]
    fn arbitrary_percent_and_full_scale_protocol() {
        exercise_jog(378, true, 0);
        exercise_jog(1000, true, 0);
        exercise_jog(0, false, 0);
    }
    #[test]
    fn truncated_reply_during_stop_recovers_without_redrive() {
        exercise_jog(100, true, 1);
    }
    #[test]
    fn active_reply_timeout_cuts_drive_without_repeating_command() {
        exercise_jog(100, true, 2);
    }
    #[test]
    fn persistent_silence_never_reports_stop_verified() {
        let (mut bus, mut peer) = pair();
        let t = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            peer.read_to_end(&mut bytes).unwrap();
            let stop = packet(254, 0xa0, &[0]).unwrap();
            assert_eq!(bytes, stop.repeat(5));
        });
        assert!(
            bus.stop(3)
                .unwrap_err()
                .contains("unverified after bounded retries")
        );
        drop(bus);
        t.join().unwrap();
    }
    #[test]
    fn out_of_range_pwm_rejected_before_serial_io() {
        let (mut bus, _peer) = pair();
        assert!(
            bus.jog(
                3,
                &AxisCalibration::default(),
                1,
                1001,
                &AtomicBool::new(false)
            )
            .unwrap_err()
            .contains("0% and 100%")
        );
    }
    fn exercise_continuous_serial(lose_drive_ack: bool, manual: bool, rollover: bool) {
        let (mut bus, mut peer) = pair();
        let peer_task = std::thread::spawn(move || {
            let (mut torque, mut pwm, mut mode, mut lock, mut armed) = (0u8, 0u16, 2u8, 1u8, 0u8);
            let mut position = if rollover { 4i32 } else { 2048 };
            let (mut enables, mut drive_writes, mut injected) = (0, 0, false);
            loop {
                let mut h = [0; 4];
                if peer.read_exact(&mut h).is_err() {
                    break;
                }
                let mut rest = vec![0; h[3] as usize];
                peer.read_exact(&mut rest).unwrap();
                let (id, inst, p) = (h[2], rest[0], &rest[1..rest.len() - 1]);
                let values = if id == 254 {
                    match p[0] {
                        0 => {
                            armed = 0;
                            torque = 0;
                            pwm = 0;
                        }
                        1 => armed = 4,
                        3 | 7 => (),
                        _ => panic!("Unexpected supervisor opcode"),
                    };
                    vec![
                        5,
                        if armed == 0 { 1 } else { 0 },
                        0,
                        254,
                        armed,
                        0,
                        4,
                        0,
                        60,
                        90,
                        126,
                        208,
                        7,
                    ]
                } else if inst == 2 {
                    match p[0] {
                        0x38 => {
                            let mut v = vec![0; 15];
                            if rollover && id == 3 && torque == 1 && (pwm & 1023) > 0 {
                                position += if pwm & 1024 != 0 { -1 } else { 1 };
                            }
                            v[0..2]
                                .copy_from_slice(&(position.rem_euclid(4096) as u16).to_le_bytes());
                            v[6] = 120;
                            v[7] = 30;
                            v
                        }
                        0x28 => {
                            if torque == 0 {
                                armed = 0;
                            }
                            vec![torque]
                        }
                        0x2c => pwm.to_le_bytes().to_vec(),
                        0x21 => vec![mode],
                        0x37 => vec![lock],
                        _ => panic!("Unexpected register"),
                    }
                } else {
                    assert_eq!(inst, 3);
                    match p[0] {
                        0x28 => {
                            if p[1] == 1 {
                                assert_eq!(pwm, 0);
                                enables += 1;
                            }
                            torque = p[1]
                        }
                        0x2c => {
                            pwm = u16::from_le_bytes([p[1], p[2]]);
                            assert!(pwm & 1023 <= 200);
                            if pwm & 1023 > 0 {
                                drive_writes += 1;
                            }
                        }
                        0x21 => mode = p[1],
                        0x37 => lock = p[1],
                        _ => panic!("Unexpected write"),
                    };
                    vec![]
                };
                let reply = packet(id, 0, &values).unwrap();
                if lose_drive_ack && !injected && inst == 3 && p[0] == 0x2c && pwm & 1023 > 0 {
                    injected = true;
                    peer.write_all(&reply[..4]).unwrap();
                } else {
                    peer.write_all(&reply).unwrap();
                }
            }
            assert_eq!(
                enables, 1,
                "Continuous traversal must not repeatedly rearm/enable"
            );
            assert_eq!((torque, pwm), (0, 0));
            if lose_drive_ack {
                assert_eq!(drive_writes, 1);
                assert!(injected);
            } else {
                assert!(drive_writes >= 3);
            }
        });
        let tuning:SweepTuning=serde_json::from_value(serde_json::from_str::<serde_json::Value>(include_str!("../../../../examples/actuators/hx30hm/hardware/2026-09-21-leg-calibration/server.json")).unwrap()["sweep_tuning"].clone()).unwrap();
        let axis = AxisCalibration {
            role: "test".into(),
            lower: Some(if rollover { -100 } else { 1800 }),
            upper: Some(if rollover { 100 } else { 2300 }),
            ..Default::default()
        };
        let cancel = AtomicBool::new(false);
        let mut samples = Vec::new();
        let mut controls_calls = 0;
        let outcome = bus
            .controlled_motion(
                3,
                &axis,
                64,
                &tuning,
                &cancel,
                manual,
                || {
                    controls_calls += 1;
                    Ok(Some((
                        SweepInput {
                            speed_counts_s: if rollover || controls_calls < 6 {
                                5.
                            } else {
                                2.5
                            },
                            pwm_limit: 200,
                        },
                        if rollover {
                            MotionCommand::Jog(-1)
                        } else if manual {
                            if controls_calls < 8 {
                                MotionCommand::Jog(1)
                            } else {
                                MotionCommand::Hold
                            }
                        } else {
                            MotionCommand::Sweep
                        },
                        axis.clone(),
                    )))
                },
                |_, sample| {
                    samples.push(sample.clone());
                    if samples.len() == if rollover { 100 } else { 12 } {
                        cancel.store(true, Ordering::SeqCst);
                    }
                    Ok(())
                },
            )
            .unwrap();
        if lose_drive_ack {
            assert!(
                outcome
                    .motion_error
                    .unwrap()
                    .contains("serial reply timeout")
            );
        } else {
            assert!(outcome.motion_error.is_none());
            assert_eq!(samples.len(), if rollover { 100 } else { 12 });
            if rollover {
                assert!(
                    samples.iter().any(|s| s.position_continuous < 0),
                    "must actually pass raw zero"
                );
            }
            if manual && !rollover {
                assert!(
                    samples.last().unwrap().holding,
                    "release must hold within the same torque session"
                );
            }
            assert_eq!(
                samples.last().unwrap().requested_speed_counts_s,
                if rollover { 5. } else { 2.5 }
            );
        }
        drop(bus);
        peer_task.join().unwrap();
    }
    #[test]
    fn continuous_session_speed_update_and_operator_stop() {
        exercise_continuous_serial(false, false, false);
    }
    #[test]
    fn continuous_session_lost_reply_cuts_drive_without_retry() {
        exercise_continuous_serial(true, false, false);
    }
    #[test]
    fn teaching_release_holds_without_disabling_or_rearming() {
        exercise_continuous_serial(false, true, false);
    }
    #[test]
    fn serial_motion_crosses_zero_in_one_continuous_session() {
        exercise_continuous_serial(false, true, true);
    }
}
