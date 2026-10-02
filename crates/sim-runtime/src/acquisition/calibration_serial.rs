//! Serial transport for supervised, operator-triggered encoder teaching.
//! Uses the shared HX packet/telemetry contract; never advances simulation.
use super::{
    calibration::{AxisCalibration, EncoderTurns},
    calibration_sweep::{DriveMode, MotionCommand, RangeSweep, SweepInput, SweepSample, SweepTuning},
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
/// Coasting allowance for identification steps (measured lag ≈ 60–70 ms plus
/// one to two control periods).
const COAST_S: f64 = 0.15;
/// Write parameters (register first) for one control period in `mode`.
/// PWM: the host feedback loop's duty. Servo position: the host reference as a
/// goal with a speed limit a little above the reference speed, clamped inside
/// the armed window. Servo speed: reference speed plus a position trim, zero
/// once a hold is within its tolerance.
pub fn servo_command(mode: DriveMode, s: &SweepSample, (lo, hi): (i32, i32), hold_tolerance: f64) -> Vec<u8> {
    match mode {
        DriveMode::Pwm => {
            let p = signed_pwm_write_parameters(s.pwm, HX30HM_PWM_DIRECTION_BIT).unwrap_or([0x2c, 0, 0]);
            p.to_vec()
        }
        DriveMode::ServoPosition => {
            let goal = (s.target_raw.round() as i32).clamp(lo, hi).rem_euclid(4096) as u16;
            let speed = (s.target_velocity_counts_s.abs() * 1.3 + 150.).clamp(150., 3000.) as u16;
            let [g0, g1] = goal.to_le_bytes();
            let [v0, v1] = speed.to_le_bytes();
            vec![0x2a, g0, g1, 0, 0, v0, v1]
        }
        DriveMode::ServoSpeed => {
            let error = s.target_raw - s.position_continuous as f64;
            let settled = s.holding || (s.target_velocity_counts_s.abs() < 1. && error.abs() <= hold_tolerance);
            let mut speed = if settled { 0. } else { (s.target_velocity_counts_s + 4. * error).clamp(-3000., 3000.) };
            // Never ask for motion further out past a window edge (the FPGA would drop it).
            if (speed > 0. && s.position_continuous >= hi) || (speed < 0. && s.position_continuous <= lo) {
                speed = 0.;
            }
            let raw = (speed.abs().round() as u16).min(0x7fff) | if speed < 0. { 0x8000 } else { 0 };
            let [v0, v1] = raw.to_le_bytes();
            vec![0x2e, v0, v1]
        }
    }
}
impl CalibrationBus {
    /// Connect only to the virtual bench's Unix socket. The returned descriptor
    /// is a socket, never a serial file; handshake failure has no fallback.
    pub fn open_virtual(socket: &Path, expected_bench: Option<&str>, log: &Path) -> R<(Self, String)> {
        use std::os::{fd::OwnedFd, unix::net::UnixStream};
        let mut stream = UnixStream::connect(socket).map_err(|e| format!("Virtual bench connect: {e}"))?;
        stream.set_read_timeout(Some(Duration::from_secs(2))).map_err(|e| e.to_string())?;
        stream.set_write_timeout(Some(Duration::from_secs(2))).map_err(|e| e.to_string())?;
        stream.write_all(b"HX-VIRTUAL-CALIBRATION/1\n").map_err(|e| e.to_string())?;
        let mut response = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let remaining = deadline.checked_duration_since(Instant::now()).ok_or("Virtual handshake deadline exceeded")?;
            stream.set_read_timeout(Some(remaining.max(Duration::from_millis(1)))).map_err(|e| e.to_string())?;
            let mut byte = [0];
            stream.read_exact(&mut byte).map_err(|e| format!("Virtual bench handshake: {e}"))?;
            if byte[0] == b'\n' { break; }
            if response.len() >= 512 { return Err("Virtual bench handshake too large".into()); }
            response.push(byte[0]);
        }
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Handshake { schema_version: u32, kind: String, bench_instance: String }
        let h: Handshake = serde_json::from_slice(&response).map_err(|e| e.to_string())?;
        if h.schema_version != 1 || h.kind != "virtual_calibration"
            || !crate::hardware_client::calibration::valid_instance(&h.bench_instance)
            || expected_bench.is_some_and(|expected| expected != h.bench_instance) {
            return Err("Virtual bench identity mismatch; no transport opened".into());
        }
        stream.set_read_timeout(None).map_err(|e| e.to_string())?;
        stream.set_write_timeout(None).map_err(|e| e.to_string())?;
        stream.set_nonblocking(true).map_err(|e| e.to_string())?;
        let descriptor: OwnedFd = stream.into();
        let bus = Self {
            file: File::from(descriptor), pending: PacketBuffer::default(),
            stop_reply_recoveries: 0, encoders: [EncoderTurns::default(); 3],
            log: OpenOptions::new().create(true).append(true).open(log).map_err(|e| e.to_string())?,
        };
        Ok((bus, h.bench_instance))
    }
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
    /// Forget one axis's turn count after its readback was actually lost.
    pub fn reset_turn_tracking_for(&mut self, id: u8) {
        if let Some(e) = self.encoders.get_mut(id.wrapping_sub(1) as usize) {
            *e = EncoderTurns::default();
        }
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
        // Profile 5 arms one axis at a time; profile 6 may arm several together.
        if !matches!(s[0], 5..=8) || s[8..13] != [60, 90, 126, 208, 7] || s[4] & !7 != 0 || s[5] != 0 {
            return Err("Calibration FPGA profile is not loaded; motion disabled".into());
        }
        Ok(s)
    }
    fn arm(&mut self, id: u8, lo: i32, hi: i32) -> R<()> {
        self.arm_with(id, lo, hi, true)
    }
    /// `stop_first` latches every axis before arming this one; an additional
    /// axis (profile 6) is armed alongside those already armed.
    fn arm_with(&mut self, id: u8, lo: i32, hi: i32, stop_first: bool) -> R<()> {
        let before = if stop_first {
            self.supervisor(&[0])?;
            0
        } else {
            self.supervisor(&[2])?[4]
        };
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
        if s[1] != 0 || s[4] != (before | (1 << (id - 1))) {
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
        if t.position_raw > 4095 || t.temperature_c >= 55 || !(9.0..=12.6).contains(&t.voltage_v) {
            return Err(format!(
                "Feedback outside commissioning limits: {} °C, {:.1} V, encoder {}",
                t.temperature_c, t.voltage_v, t.position_raw
            ));
        }
        Ok(())
    }
    /// Servo-reported conditions that are logged and shown, not stop causes.
    fn advisories(t: &Telemetry) -> Vec<String> {
        let mut out = Vec::new();
        if t.status != 0 {
            out.push(format!("Servo status flags 0x{:02x}", t.status));
        }
        if t.current_raw >= 500 {
            out.push(format!("High servo current reading {} (uncalibrated)", t.current_raw));
        }
        out
    }
    /// Proves local deadlines at ZERO drive. This is not a mechanical stopping-distance test.
    pub fn prove_watchdogs(&mut self, id: u8) -> R<Telemetry> {
        let t = self.stop(id)?;
        Self::healthy(&t)?;
        let position = t.position_continuous.ok_or("Missing encoder coordinate")?;
        let lo = position - 8;
        let hi = position + 8;
        // Wait for each FPGA deadline to trip on its own, whatever the loaded
        // profile's timeout; status reads neither refresh feedback nor renew commands.
        let tripped = |s: &[u8], reason: u8| s[1] == 1 && s[2] == reason && s[4] == 0;
        self.arm(id, lo, hi)?;
        let start = Instant::now();
        let mut s = self.supervisor(&[2])?;
        while !tripped(&s, 7) && start.elapsed() < Duration::from_millis(1200) {
            std::thread::sleep(Duration::from_millis(50));
            s = self.supervisor(&[2])?;
        }
        if !tripped(&s, 7) {
            return Err("Independent telemetry-loss watchdog not verified".into());
        }
        self.arm(id, lo, hi)?;
        let start = Instant::now();
        let mut s = self.supervisor(&[2])?;
        while !tripped(&s, 8) && start.elapsed() < Duration::from_millis(1500) {
            self.feedback(id)?;
            std::thread::sleep(Duration::from_millis(20));
            s = self.supervisor(&[2])?;
        }
        if !tripped(&s, 8) {
            return Err("Independent command-loss watchdog not verified".into());
        }
        self.stop(id)
    }
    fn prepare_pwm(&mut self, id: u8, lo: i32, hi: i32, cancel: &AtomicBool) -> R<()> {
        self.prepare_pwm_with(id, lo, hi, cancel, true)
    }
    fn prepare_pwm_with(&mut self, id: u8, lo: i32, hi: i32, cancel: &AtomicBool, stop_first: bool) -> R<()> {
        self.prepare_drive(id, lo, hi, cancel, stop_first, DriveMode::Pwm)
    }
    /// Arm, select the servo control mode, park its goal on the current pose
    /// (so enabling torque does not move it) and enable torque.
    fn prepare_drive(&mut self, id: u8, lo: i32, hi: i32, cancel: &AtomicBool, stop_first: bool, mode: DriveMode) -> R<()> {
        self.arm_with(id, lo, hi, stop_first)?;
        self.write(id, 0x28, &[0])?;
        // Torque-off readback disarms; rearm after clearing stored PWM and speed.
        self.write(id, 0x2c, &[0, 0])?;
        self.feedback(id)?;
        self.supervisor(&[1, id])?;
        if self.read(id, 0x21, 1)? != [mode.register()] {
            self.write(id, 0x37, &[0])?;
            self.write(id, 0x21, &[mode.register()])?;
        }
        self.write(id, 0x37, &[1])?;
        if mode != DriveMode::Pwm && self.read(id, 0x21, 1)? != [mode.register()] {
            return Err(format!("Servo {id} did not accept control mode {mode:?}; it may need a power cycle"));
        }
        let here = self.feedback(id)?;
        match mode {
            DriveMode::ServoPosition => {
                let raw = here.position_raw.to_le_bytes();
                self.write(id, 0x2a, &[raw[0], raw[1], 0, 0, 0x90, 0x01])?;
            }
            DriveMode::ServoSpeed => self.write(id, 0x2e, &[0, 0])?,
            DriveMode::Pwm => {}
        }
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
    /// Several axes energized in one session (FPGA profile 6). Each axis has
    /// its own feedback controller, taught bounds and FPGA window; every
    /// period reads all of them, renews their leases together and writes each
    /// PWM. Any fault or lost reply stops every axis; no drive is retried.
    /// `controls` returns one entry per axis, in `ids` order.
    pub fn controlled_motion_multi(
        &mut self,
        ids: &[u8],
        axes: &[AxisCalibration],
        clearance: u16,
        tuning: &SweepTuning,
        cancel: &AtomicBool,
        teaching: bool,
        mode: DriveMode,
        mut controls: impl FnMut() -> R<Option<Vec<(SweepInput, MotionCommand, AxisCalibration)>>>,
        mut observe: impl FnMut(u8, &Telemetry, &SweepSample) -> R<()>,
    ) -> R<SweepOutcome> {
        let valid = !ids.is_empty() && ids.len() == axes.len() && ids.iter().all(|id| (1..=3).contains(id));
        let mask: u16 = if valid { ids.iter().map(|id| 1u16 << (id - 1)).fold(0, |a, b| a | b) } else { 0 };
        if !valid || mask.count_ones() as usize != ids.len() {
            return Err("Multi-axis motion needs distinct IDs 1..3 with one axis record each".into());
        }
        if ids.len() > 1 && self.supervisor(&[2])?[0] < 6 {
            return Err("The loaded FPGA calibration profile arms one motor at a time; load profile 6 to hold or sweep several".into());
        }
        let mut sweeps = Vec::new();
        for (id, axis) in ids.iter().zip(axes) {
            let initial = self.stop(*id)?;
            Self::healthy(&initial)?;
            let position = initial.position_continuous.ok_or("Missing encoder coordinate")?;
            sweeps.push(if teaching {
                RangeSweep::teaching(axis, position, tuning.clone())?
            } else {
                RangeSweep::new(axis, position, clearance, tuning.clone())?
            });
        }
        self.log_event(serde_json::json!({"event":"sweep_start","ids":ids,"axes":axes,"clearance_counts":clearance,"tuning":tuning,"drive_mode":mode}))?;
        let bounds: Vec<(i32, i32)> = sweeps.iter().map(|s| (s.raw_lower, s.raw_upper)).collect();
        let renew = [5, (mask & 0xff) as u8, (mask >> 8) as u8];
        let run = (|| -> R<()> {
            if controls()?.is_none() || cancel.load(Ordering::SeqCst) {
                return Ok(());
            }
            for (k, id) in ids.iter().enumerate() {
                self.prepare_drive(*id, sweeps[k].raw_lower, sweeps[k].raw_upper, cancel, k == 0, mode)?;
                // Keep already-energized axes inside their feedback and command deadlines.
                for earlier in &ids[..k] {
                    self.feedback(*earlier)?;
                }
                if k > 0 {
                    self.supervisor(&renew)?;
                }
            }
            let start = Instant::now();
            let mut other_poll = Instant::now();
            let (mut cycle, mut held) = (0u64, Vec::<bool>::new());
            loop {
                if cancel.load(Ordering::SeqCst) {
                    break;
                }
                let tick = Instant::now();
                if other_poll.elapsed() > Duration::from_millis(100) {
                    for other in (1..=3).filter(|o| !ids.contains(o)) {
                        self.feedback(other)?;
                    }
                    other_poll = Instant::now();
                }
                // The moving axis is serviced every period. Axes that are only
                // holding are read and driven every third period: that keeps the
                // moving axis's loop near its tuned period instead of slowing it
                // with every held axis (their FPGA deadlines are still met).
                cycle += 1;
                let serviced: Vec<bool> = (0..ids.len())
                    .map(|k| k == 0 || cycle % 3 == 0 || held.get(k).is_some_and(|h| !*h))
                    .collect();
                let mut readings = Vec::new();
                for (id, on) in ids.iter().zip(&serviced) {
                    if *on {
                        let t = self.feedback(*id)?;
                        Self::healthy(&t)?;
                        readings.push(Some(t));
                    } else {
                        readings.push(None);
                    }
                }
                let Some(plan) = controls()? else {
                    break;
                };
                if plan.len() != ids.len() {
                    return Err("Motion plan does not cover every energized axis".into());
                }
                held = plan.iter().map(|(_, c, _)| *c == MotionCommand::Hold).collect();
                let time = start.elapsed().as_secs_f64();
                let mut samples = Vec::new();
                for (k, ((input, command, bounds), t)) in plan.into_iter().zip(&readings).enumerate() {
                    let Some(t) = t else {
                        samples.push(None);
                        continue;
                    };
                    if teaching {
                        sweeps[k].set_taught_bounds(&bounds)?;
                    }
                    sweeps[k].observe_environment(t.voltage_v, t.temperature_c);
                    samples.push(Some(sweeps[k].control(time, t.position_continuous.ok_or("Missing encoder coordinate")?, input, command)?));
                }
                let status = if ids.len() == 1 { self.supervisor(&[3, ids[0]])? } else { self.supervisor(&renew)? };
                if status[1] != 0 || status[4] as u16 != mask {
                    return Err("FPGA stopped continuous traversal".into());
                }
                if cancel.load(Ordering::SeqCst) {
                    break;
                }
                for (k, ((id, sample), t)) in ids.iter().zip(samples.iter_mut()).zip(&readings).enumerate() {
                    let (Some(sample), Some(t)) = (sample.as_mut(), t) else { continue };
                    sample.warnings.extend(Self::advisories(t));
                    let sample = &*sample;
                    let command = servo_command(mode, sample, bounds[k], tuning.hold_deadband_counts.unwrap_or(0.));
                    self.txn(*id, 3, &command, 0)?;
                    self.log_event(serde_json::json!({"event":"sweep_sample","id":id,"sample":sample,"command":command}))?;
                    observe(*id, t, sample)?;
                }
                if let Some(wait) = Duration::from_secs_f64(tuning.period_s).checked_sub(tick.elapsed()) {
                    std::thread::sleep(wait);
                }
            }
            Ok(())
        })();
        // One STOP latches every axis; each axis's torque-off readback is then verified.
        let mut stopped = Vec::new();
        let mut stop_error = None;
        for id in ids {
            match self.stop(*id) {
                Ok(t) => stopped.push(t),
                Err(e) => stop_error = Some(e),
            }
        }
        self.log_event(serde_json::json!({"event":"sweep_end","ids":ids,"motion_error":run.as_ref().err(),"stop_error":stop_error}))?;
        if let Some(e) = stop_error {
            return Err(e);
        }
        Ok(SweepOutcome { telemetry: stopped.swap_remove(0), motion_error: run.err() })
    }
    /// Open-loop response of one motor for gain design: a slow PWM ramp each
    /// way to find breakaway, then short constant-duty steps alternating toward
    /// the start pose. Travel stays within `travel` counts of the start; the
    /// FPGA window is set just outside that. Returns the raw record; gains are
    /// designed by `motor_identification::design`.
    pub fn identify(
        &mut self,
        id: u8,
        travel: i32,
        duty_ceiling: f64,
        period_s: f64,
        cancel: &AtomicBool,
        mut progress: impl FnMut(&str),
    ) -> R<serde_json::Value> {
        use super::motor_identification::StepTrace;
        if !(20..=1000).contains(&travel) || !(0.05..=1.0).contains(&duty_ceiling) {
            return Err("Identification travel or duty ceiling out of range".into());
        }
        let initial = self.stop(id)?;
        Self::healthy(&initial)?;
        let start = initial.position_continuous.ok_or("Missing encoder coordinate")?;
        let (lo, hi) = (start - travel - 60, start + travel + 60);
        self.log_event(serde_json::json!({"event":"identify_start","id":id,"start":start,"travel":travel,"duty_ceiling":duty_ceiling}))?;
        let clock = Instant::now();
        let mut periods = Vec::new();
        let mut breakaway = [duty_ceiling; 2];
        let mut steps: Vec<StepTrace> = Vec::new();
        let run = (|| -> R<()> {
            self.prepare_pwm(id, lo, hi, cancel)?;
            let mut last_tick: Option<Instant> = None;
            // One control period: read, renew, drive. Returns (time, position).
            let mut cycle = |bus: &mut Self, duty: f64| -> R<(f64, i32)> {
                if cancel.load(Ordering::SeqCst) {
                    return Err("Operator stop during tuning".into());
                }
                let tick = Instant::now();
                if let Some(prev) = last_tick {
                    periods.push(tick.duration_since(prev).as_secs_f64());
                }
                last_tick = Some(tick);
                let t = bus.feedback(id)?;
                Self::healthy(&t)?;
                let position = t.position_continuous.ok_or("Missing encoder coordinate")?;
                let status = bus.supervisor(&[3, id])?;
                if status[1] != 0 || status[4] & (1 << (id - 1)) == 0 {
                    return Err("FPGA stopped the tuning run".into());
                }
                let raw = (duty * 1000.).round() as i16;
                let p = signed_pwm_write_parameters(raw, HX30HM_PWM_DIRECTION_BIT).map_err(str::to_string)?;
                bus.txn(id, 3, &p, 0)?;
                bus.log_event(serde_json::json!({"event":"identify_sample","id":id,"t":clock.elapsed().as_secs_f64(),"position":position,"pwm":raw}))?;
                if let Some(wait) = Duration::from_secs_f64(period_s).checked_sub(tick.elapsed()) {
                    std::thread::sleep(wait);
                }
                Ok((clock.elapsed().as_secs_f64(), position))
            };
            let settle = |bus: &mut Self, cycle: &mut dyn FnMut(&mut Self, f64) -> R<(f64, i32)>| -> R<i32> {
                let (mut last, mut same, begun) = (i32::MIN, 0, Instant::now());
                loop {
                    let (_, p) = cycle(bus, 0.)?;
                    same = if p == last { same + 1 } else { 0 };
                    last = p;
                    if same >= 3 || begun.elapsed() > Duration::from_millis(800) {
                        return Ok(p);
                    }
                }
            };
            // Breakaway: ramp 0.4% duty per period until the axis moves 3 counts.
            for (k, sign) in [(1usize, 1f64), (0, -1f64)] {
                progress(&format!("Finding friction ({})", if sign > 0. { "increasing" } else { "decreasing" }));
                let from = settle(self, &mut cycle)?;
                let mut duty = 0.;
                loop {
                    duty += 0.004;
                    if duty > duty_ceiling.min(0.6) {
                        break;
                    }
                    let (_, p) = cycle(self, sign * duty)?;
                    if (p - from).abs() >= 3 || (p - start).abs() >= travel {
                        breakaway[k] = duty;
                        break;
                    }
                }
            }
            // Constant-duty steps, each heading back toward the start pose.
            let duties: Vec<f64> = [0.15, 0.3, 0.5]
                .into_iter()
                .filter(|d| *d <= duty_ceiling + 1e-9 && *d > breakaway[0].max(breakaway[1]) + 0.03)
                .collect();
            let duties = if duties.is_empty() { vec![duty_ceiling] } else { duties };
            for duty in duties {
                for _ in 0..2 {
                    let from = settle(self, &mut cycle)?;
                    let sign = if from >= start { -1. } else { 1. };
                    progress(&format!("Step response at {:.0}% {}", duty * 100., if sign > 0. { "up" } else { "down" }));
                    let mut samples: Vec<(f64, i32)> = Vec::new();
                    let begun = clock.elapsed().as_secs_f64();
                    loop {
                        let (t, p) = cycle(self, sign * duty)?;
                        samples.push((t - begun, p));
                        // Stop early enough that coasting (lag plus loop delay)
                        // ends inside the travel budget, not past it.
                        let speed = match samples.as_slice() {
                            [.., (t0, p0), (t1, p1)] if t1 > t0 => (p1 - p0) as f64 / (t1 - t0),
                            _ => 0.,
                        };
                        let coasting = speed * sign * COAST_S;
                        let beyond = (p - start) as f64 * sign + coasting.max(0.) >= travel as f64;
                        if beyond || t - begun >= 0.6 {
                            break;
                        }
                    }
                    steps.push(StepTrace { duty: sign * duty, samples });
                }
            }
            // Return to the start pose at a gentle duty so tuning leaves the
            // axis where the operator confirmed there was room.
            let mut from = settle(self, &mut cycle)?;
            let back = (breakaway[0].max(breakaway[1]) + 0.05).min(duty_ceiling);
            for _ in 0..3 {
                if (from - start).abs() <= 20 {
                    break;
                }
                let sign = if from > start { -1. } else { 1. };
                let mut last: Option<(f64, i32)> = None;
                for _ in 0..200 {
                    let (t, p) = cycle(self, sign * back)?;
                    let speed = last.filter(|(t0, _)| t > *t0).map_or(0., |(t0, p0)| (p - p0) as f64 / (t - t0));
                    last = Some((t, p));
                    if (start - p) as f64 * sign - (speed * sign * COAST_S).max(0.) <= 10. {
                        break;
                    }
                }
                from = settle(self, &mut cycle)?;
            }
            Ok(())
        })();
        let stopped = self.stop(id);
        self.log_event(serde_json::json!({"event":"identify_end","id":id,"error":run.as_ref().err()}))?;
        run?;
        let t = stopped?;
        let period = super::motor_identification::median_of(&periods).unwrap_or(period_s);
        Ok(serde_json::json!({
            "schema_version": 1,
            "motor_id": id,
            "start_counts": start,
            "end_counts": t.position_continuous,
            "travel_counts": travel,
            "duty_ceiling": duty_ceiling,
            "loop_period_s": period,
            "breakaway_duty": breakaway,
            "steps": steps,
        }))
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

/// The characterization campaign's [`Rig`](super::characterization::Rig) on
/// the real bus, in real time. Every axis the campaign drives is armed in the
/// FPGA with its campaign window (outward drive blocked there), energized in
/// the servo mode it needs, and kept inside its feedback and command
/// deadlines every period. `tick` waits out the period; the operator's cancel
/// flag makes it fail, and `stop` latches everything with verified readback.
/// Loads are attached by the operator, never by the rig.
pub struct BusRig<'a> {
    bus: &'a mut CalibrationBus,
    windows: std::collections::BTreeMap<u8, (i32, i32)>,
    energized: std::collections::BTreeMap<u8, DriveMode>,
    read_this_period: std::collections::BTreeSet<u8>,
    period_s: f64,
    start: Instant,
    tick_start: Instant,
    cancel: &'a AtomicBool,
}
impl<'a> BusRig<'a> {
    /// `windows`: per axis id, the FPGA window in continuous counts (the
    /// campaign's saved poses; the campaign's own inset keeps motion inside).
    pub fn new(bus: &'a mut CalibrationBus, windows: &[(u8, i32, i32)], period_s: f64, cancel: &'a AtomicBool) -> R<Self> {
        if windows.iter().any(|(id, lo, hi)| !(1..=3).contains(id) || lo >= hi) {
            return Err("Campaign windows need IDs 1..3 with lower < upper".into());
        }
        if windows.len() > 1 && bus.supervisor(&[2])?[0] < 6 {
            return Err("The loaded FPGA calibration profile arms one motor at a time; load profile 6 or later for a campaign".into());
        }
        for (id, _, _) in windows {
            Self::healthy(&bus.stop(*id)?)?;
        }
        bus.log_event(serde_json::json!({"event":"campaign_rig_start","windows":windows,"period_s":period_s}))?;
        Ok(Self {
            bus,
            windows: windows.iter().map(|(id, lo, hi)| (*id, (*lo, *hi))).collect(),
            energized: Default::default(),
            read_this_period: Default::default(),
            period_s,
            start: Instant::now(),
            tick_start: Instant::now(),
            cancel,
        })
    }
    fn healthy(t: &Telemetry) -> R<()> {
        CalibrationBus::healthy(t)
    }
    fn mask(&self) -> u16 {
        self.energized.keys().map(|id| 1u16 << (id - 1)).fold(0, |a, b| a | b)
    }
    /// Energize `id` in `mode` (arming it alongside those already armed).
    fn ensure(&mut self, id: u8, mode: DriveMode) -> R<()> {
        if self.energized.get(&id) == Some(&mode) {
            return Ok(());
        }
        let (lo, hi) = *self.windows.get(&id).ok_or_else(|| format!("Axis {id} is not in the campaign"))?;
        let first = self.energized.is_empty() || (self.energized.len() == 1 && self.energized.contains_key(&id));
        if first {
            self.energized.clear();
        }
        self.bus.prepare_drive(id, lo, hi, self.cancel, first, mode)?;
        self.energized.insert(id, mode);
        // Keep the others inside their deadlines while this one was armed.
        let others: Vec<u8> = self.energized.keys().copied().filter(|o| *o != id).collect();
        for other in others {
            self.bus.feedback(other)?;
        }
        if self.energized.len() > 1 {
            let m = self.mask();
            self.bus.supervisor(&[5, (m & 0xff) as u8, (m >> 8) as u8])?;
        }
        Ok(())
    }
}
impl super::characterization::Rig for BusRig<'_> {
    fn now(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }
    fn period(&self) -> f64 {
        self.period_s
    }
    fn read(&mut self, id: u8) -> R<super::characterization::Reading> {
        let t = self.bus.feedback(id)?;
        Self::healthy(&t)?;
        self.read_this_period.insert(id);
        Ok(super::characterization::Reading {
            t: self.now(),
            position: t.position_continuous.ok_or("Missing encoder coordinate")? as f64,
            speed: t.speed_rad_s * 4096. / std::f64::consts::TAU,
            voltage_v: t.voltage_v,
            temperature_c: t.temperature_c as f64,
            // No supply current sensor or joint-side angle on the leg yet (PLAN.md prerequisites).
            supply_current_a: None,
            joint: None,
        })
    }
    fn drive(&mut self, id: u8, duty: f64) -> R<()> {
        self.ensure(id, DriveMode::Pwm)?;
        let pwm = (duty.clamp(-1., 1.) * 1000.).round() as i16;
        let p = signed_pwm_write_parameters(pwm, HX30HM_PWM_DIRECTION_BIT).map_err(str::to_string)?;
        self.bus.txn(id, 3, &p, 0)?;
        Ok(())
    }
    fn goal(&mut self, id: u8, counts: f64, speed: f64) -> R<()> {
        let mode = *self.energized.get(&id).unwrap_or(&DriveMode::ServoPosition);
        if mode != DriveMode::ServoPosition {
            return Err(format!("Axis {id} is not in servo position mode"));
        }
        self.ensure(id, mode)?;
        let (lo, hi) = self.windows[&id];
        let goal = (counts.round() as i32).clamp(lo, hi).rem_euclid(4096) as u16;
        let [g0, g1] = goal.to_le_bytes();
        let [v0, v1] = (speed.abs().clamp(1., 3000.) as u16).to_le_bytes();
        self.bus.txn(id, 3, &[0x2a, g0, g1, 0, 0, v0, v1], 0)?;
        Ok(())
    }
    fn set_mode(&mut self, id: u8, mode: super::virtual_bench::ServoMode) -> R<()> {
        use super::virtual_bench::ServoMode;
        let mode = match mode {
            ServoMode::Pwm => DriveMode::Pwm,
            ServoMode::Position => DriveMode::ServoPosition,
            ServoMode::Speed => DriveMode::ServoSpeed,
        };
        self.ensure(id, mode)
    }
    fn tick(&mut self) -> R<()> {
        if self.cancel.load(Ordering::SeqCst) {
            return Err("cancelled".into());
        }
        // Every energized axis is read each period (feedback deadline) and the
        // supervisor renews their commands; a latch anywhere ends the test.
        let unread: Vec<u8> = self.energized.keys().copied().filter(|id| !self.read_this_period.contains(id)).collect();
        for id in unread {
            Self::healthy(&self.bus.feedback(id)?)?;
        }
        self.read_this_period.clear();
        if !self.energized.is_empty() {
            let m = self.mask();
            let status = if self.energized.len() == 1 {
                self.bus.supervisor(&[3, *self.energized.keys().next().unwrap()])?
            } else {
                self.bus.supervisor(&[5, (m & 0xff) as u8, (m >> 8) as u8])?
            };
            if status[1] != 0 || status[4] as u16 != m {
                return Err(format!("FPGA latched (reason {}, axis {})", status[2], status[3]));
            }
        }
        if let Some(wait) = Duration::from_secs_f64(self.period_s).checked_sub(self.tick_start.elapsed()) {
            std::thread::sleep(wait);
        }
        self.tick_start = Instant::now();
        Ok(())
    }
    fn stop(&mut self) -> R<()> {
        // Verify every axis's torque-off readback even if an earlier one fails.
        let ids: Vec<u8> = self.windows.keys().copied().collect();
        self.energized.clear();
        let mut first_error = None;
        for id in ids {
            if let Err(e) = self.bus.stop(id) {
                first_error.get_or_insert(e);
            }
        }
        first_error.map_or(Ok(()), Err)
    }
    fn attach_load(&mut self, id: u8, duty: f64) -> R<()> {
        Err(format!("Attach or remove the known load on axis {id} by hand ({duty:.3} duty equivalent), then resume the campaign"))
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
    /// Host bus wired to the simulated bench (FPGA policy + servos) running in
    /// real time on a thread. The bench is shared so tests can read true state.
    fn bench_pair(
        bench: crate::acquisition::virtual_bench::Bench,
    ) -> (CalibrationBus, std::sync::Arc<std::sync::Mutex<crate::acquisition::virtual_bench::Bench>>) {
        let (bus, mut peer) = pair();
        let shared = std::sync::Arc::new(std::sync::Mutex::new(bench));
        let bench = shared.clone();
        std::thread::spawn(move || {
            peer.set_read_timeout(Some(Duration::from_millis(1))).unwrap();
            let (mut pending, mut clock) = (Vec::<u8>::new(), Instant::now());
            loop {
                let mut buf = [0u8; 256];
                match peer.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => pending.extend_from_slice(&buf[..n]),
                    Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                    Err(_) => break,
                }
                let dt = clock.elapsed().as_secs_f64();
                clock = Instant::now();
                bench.lock().unwrap().advance(dt);
                while pending.len() >= 4 && pending.len() >= pending[3] as usize + 4 {
                    let total = pending[3] as usize + 4;
                    let frame: Vec<u8> = pending.drain(..total).collect();
                    let reply = bench.lock().unwrap().handle(frame[2], frame[4], &frame[5..total - 1]);
                    std::thread::sleep(Duration::from_millis(3));
                    if !reply.is_empty() && peer.write_all(&reply).is_err() {
                        return;
                    }
                }
            }
        });
        (bus, shared)
    }
    #[test]
    fn campaign_runs_on_the_bus_rig_in_real_time() {
        use crate::acquisition::characterization::{self as ch, Plan};
        let (mut bus, bench) = bench_pair(worm_bench());
        // Axis 2 only; a short plan: braking, steps at two levels (the gain is
        // their slope; one level would fold breakaway into it), a one-duty ladder.
        let plan: Plan = serde_json::from_value(serde_json::json!({
            "axes": [{"id": 2, "role": "worm", "lower": 500, "upper": 1700}],
            "c_speeds_counts_s": [150], "d_duties": [0.2, 0.35], "g_duties": [0.3]
        })).unwrap();
        let cancel = AtomicBool::new(false);
        let mut rig = BusRig::new(&mut bus, &[(2, 500, 1700)], 0.02, &cancel).unwrap();
        let mut log = Vec::new();
        let report = ch::run(&plan, &mut rig, None, &|_| Ok(()), &mut |m| log.push(m.to_string())).unwrap();
        assert!(report.stages.iter().all(|s| s.completed), "{:?}", report.stages.iter().map(|s| (&s.stage, &s.abort)).collect::<Vec<_>>());
        assert_eq!(report.stages.len(), 3, "{log:?}");
        // The FPGA never latched and every sample stayed inside the window.
        assert!(report.samples.iter().all(|x| (500. ..=1700.).contains(&x.position)));
        let b = bench.lock().unwrap();
        assert!(!b.servos[1].torque, "stopped with torque off");
        let gain = ch::fit_axis(&report.stages, 2).into_iter().find(|f| f.name == "speed_gain").map(|f| f.value);
        eprintln!("bus campaign speed gain {gain:?} (bench 3290)");
        assert!(gain.is_some_and(|g| (g - 3290.).abs() < 700.), "speed gain {gain:?} over the bus");
        // Cancelling makes the next period fail and the rig stops everything.
        drop(b);
        cancel.store(true, Ordering::SeqCst);
        assert_eq!(ch::Rig::tick(&mut rig).unwrap_err(), "cancelled");
        ch::Rig::stop(&mut rig).unwrap();
        assert!(ch::Rig::attach_load(&mut rig, 2, 0.05).is_err(), "loads are attached by hand");
    }
    fn worm_bench() -> crate::acquisition::virtual_bench::Bench {
        use crate::acquisition::virtual_bench::{Bench, MotorModel};
        let worm = MotorModel { speed_gain: 3290., breakaway_duty: 0.066, moving_friction_duty: 0.045, ..Default::default() };
        Bench::new([3100., 1100., 1500.], [MotorModel::default(), worm, MotorModel::default()])
    }
    fn bench_tuning() -> SweepTuning {
        serde_json::from_value(serde_json::from_str::<serde_json::Value>(include_str!("../../../../examples/actuators/hx30hm/hardware/2026-09-21-leg-calibration/server.json")).unwrap()["sweep_tuning"].clone()).unwrap()
    }
    /// One held jog on the simulated worm: 1.2 s at 300 counts/s, then 1 s of
    /// hold. Returns (true speed spread while cruising, mean cruise speed,
    /// position change during the hold, drive reversals while jogging).
    fn bench_jog(mode: DriveMode, tuned: Option<crate::acquisition::calibration::MotorTuning>) -> (f64, f64, f64, usize) {
        let (mut bus, bench) = bench_pair(worm_bench());
        if let Ok(path) = std::env::var("BENCH_LOG") {
            bus.log = File::create(format!("{path}-{mode:?}.jsonl")).unwrap();
        }
        let axis = AxisCalibration { role: "worm".into(), lower: Some(0), upper: Some(3000), tuning: tuned, ..Default::default() };
        let cancel = AtomicBool::new(false);
        let input = SweepInput { speed_counts_s: 300., pwm_limit: 1000 };
        let start = Instant::now();
        let (mut speeds, mut hold_positions, mut reversals, mut last) = (Vec::new(), Vec::new(), 0, 0i16);
        let outcome = bus.controlled_motion_multi(&[2], &[axis.clone()], 64, &bench_tuning(), &cancel, true, mode, || {
            let t = start.elapsed().as_secs_f64();
            let command = if t < 1.2 { MotionCommand::Jog(1) } else { MotionCommand::Hold };
            Ok((t < 2.4).then(|| vec![(input, command, axis.clone())]))
        }, |_, _, sample| {
            let t = start.elapsed().as_secs_f64();
            let truth = bench.lock().unwrap().servos[1].clone();
            if (0.6..1.2).contains(&t) { speeds.push(truth.speed); }
            if t > 1.6 { hold_positions.push(truth.position); }
            if t < 1.2 && mode == DriveMode::Pwm && sample.pwm != 0 {
                if last != 0 && sample.pwm.signum() != last.signum() { reversals += 1; }
                last = sample.pwm;
            }
            Ok(())
        }).unwrap();
        assert!(outcome.motion_error.is_none(), "{:?}", outcome.motion_error);
        let mean = speeds.iter().sum::<f64>() / speeds.len().max(1) as f64;
        let spread = (speeds.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / speeds.len().max(1) as f64).sqrt();
        let drift = hold_positions.iter().cloned().fold(f64::MIN, f64::max) - hold_positions.iter().cloned().fold(f64::MAX, f64::min);
        (spread, mean, drift, reversals)
    }
    #[test]
    fn simulated_bench_jogs_smoothly_in_every_drive_mode() {
        use crate::acquisition::motor_identification::{design, fit_step, StepTrace};
        // Identify the simulated worm exactly as the panel's Tune button does.
        let (mut bus, _bench) = bench_pair(worm_bench());
        let record = bus.identify(2, 200, 1.0, bench_tuning().period_s, &AtomicBool::new(false), |_| {}).unwrap();
        drop(bus);
        let steps: Vec<StepTrace> = serde_json::from_value(record["steps"].clone()).unwrap();
        let fits: Vec<_> = steps.iter().filter_map(fit_step).collect();
        let tuned = design(&fits, serde_json::from_value(record["breakaway_duty"].clone()).unwrap(), record["loop_period_s"].as_f64().unwrap(), 0.08, "bench").unwrap();
        // Tuning stays within its travel budget (coasting included) and returns to the start.
        let start = record["start_counts"].as_i64().unwrap() as i32;
        let widest = steps.iter().flat_map(|s| s.samples.iter()).map(|(_, p)| (p - start).abs()).max().unwrap();
        assert!(widest <= 200 + 40, "tuning travelled {widest} counts from the start (budget 200)");
        let end = record["end_counts"].as_i64().unwrap() as i32;
        assert!((end - start).abs() <= 30, "tuning returned to the start: {start} -> {end}");
        assert!((tuned.gain_counts_s_per_duty - 3290.).abs() < 700., "identified gain {}", tuned.gain_counts_s_per_duty);
        let report = |name: &str, (spread, mean, drift, rev): (f64, f64, f64, usize)| {
            eprintln!("{name:>28}: cruise {mean:6.1} ± {spread:5.1} counts/s, hold drift {drift:5.1} counts, drive reversals {rev}");
            (spread, mean, drift, rev)
        };
        let untuned = report("PWM, shared gains", bench_jog(DriveMode::Pwm, None));
        let pwm = report("PWM, tuned + feed-forward", bench_jog(DriveMode::Pwm, Some(tuned.clone())));
        let position = report("servo position loop", bench_jog(DriveMode::ServoPosition, None));
        let speed = report("servo speed loop", bench_jog(DriveMode::ServoSpeed, None));
        for (name, (spread, mean, drift, _)) in [("tuned PWM", pwm), ("servo position", position), ("servo speed", speed)] {
            assert!((mean - 300.).abs() < 45., "{name} cruises near the requested speed: {mean}");
            assert!(spread < 40., "{name} cruises smoothly: ±{spread}");
            assert!(drift < 20., "{name} holds still after release: {drift}");
        }
        assert!(pwm.0 < untuned.0, "feed-forward is smoother than the shared loop");
        assert_eq!(pwm.3, 0, "tuned PWM never reverses drive while jogging one way");
    }
    #[test]
    fn servo_commands_stay_inside_the_armed_window() {
        let axis = AxisCalibration { role: "t".into(), lower: Some(1000), upper: Some(3000), ..Default::default() };
        let mut c = RangeSweep::teaching(&axis, 2000, bench_tuning()).unwrap();
        let base = c.control(0.03, 2000, SweepInput { speed_counts_s: 100., pwm_limit: 1000 }, MotionCommand::Hold).unwrap();
        let (lo, hi) = (c.raw_lower, c.raw_upper);
        let with = |target: f64, velocity: f64, position: i32, holding: bool| {
            let mut s = base.clone();
            (s.target_raw, s.target_velocity_counts_s, s.position_continuous, s.holding) = (target, velocity, position, holding);
            s
        };
        // PWM passes the host loop's duty through unchanged.
        let mut p = with(2000., 0., 2000, true);
        p.pwm = -120;
        assert_eq!(servo_command(DriveMode::Pwm, &p, (lo, hi), 16.), signed_pwm_write_parameters(-120, HX30HM_PWM_DIRECTION_BIT).unwrap().to_vec());
        // Position goals are clamped into the window, then sent as raw counts.
        let goal = |cmd: Vec<u8>| u16::from_le_bytes([cmd[1], cmd[2]]);
        assert_eq!(goal(servo_command(DriveMode::ServoPosition, &with(3500., 200., 2990, false), (lo, hi), 16.)), hi as u16);
        let far = servo_command(DriveMode::ServoPosition, &with(2500., 400., 2400, false), (lo, hi), 16.);
        assert_eq!((far[0], goal(far.clone())), (0x2a, 2500));
        assert!(u16::from_le_bytes([far[5], far[6]]) >= 400, "speed limit covers the reference speed");
        // Speed: reference plus trim; zero when settled or pushing past an edge.
        let speed = |cmd: Vec<u8>| { let v = u16::from_le_bytes([cmd[1], cmd[2]]); (v & 0x7fff) as i32 * if v & 0x8000 != 0 { -1 } else { 1 } };
        assert_eq!(speed(servo_command(DriveMode::ServoSpeed, &with(2010., 300., 2000, false), (lo, hi), 16.)), 340);
        assert_eq!(speed(servo_command(DriveMode::ServoSpeed, &with(1990., -300., 2000, false), (lo, hi), 16.)), -340);
        assert_eq!(speed(servo_command(DriveMode::ServoSpeed, &with(2005., 0., 2000, false), (lo, hi), 16.)), 0);
        assert_eq!(speed(servo_command(DriveMode::ServoSpeed, &with(hi as f64 + 5., 200., hi, false), (lo, hi), 16.)), 0);
        assert_eq!(speed(servo_command(DriveMode::ServoSpeed, &with(hi as f64 - 50., -200., hi, false), (lo, hi), 16.)), -400);
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
    /// Fake profile-`version` bridge with three servos: tracks each axis's
    /// torque, PWM and arm bit, and the FPGA's armed mask.
    fn multi_axis_peer(mut peer: UnixStream, version: u8) -> std::thread::JoinHandle<([u32; 3], [u32; 3], u8, [u8; 3])> {
        std::thread::spawn(move || {
            let (mut torque, mut pwm, mut mode, mut lock) = ([0u8; 3], [0u16; 3], [2u8; 3], [1u8; 3]);
            let (mut armed, mut enables, mut drives, mut max_mask) = (0u8, [0u32; 3], [0u32; 3], 0u8);
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
                            torque = [0; 3];
                            pwm = [0; 3];
                        }
                        1 => {
                            let bit = 1 << (p[1] - 1);
                            if version >= 6 || armed == 0 {
                                armed |= bit;
                            }
                        }
                        2 | 3 | 7 => (),
                        5 => assert_eq!(p[1] & !armed, 0, "renewing an unarmed axis"),
                        _ => panic!("Unexpected supervisor opcode"),
                    };
                    max_mask = max_mask.max(armed);
                    vec![version, u8::from(armed == 0), 0, 254, armed, 0, 7, 0, 60, 90, 126, 208, 7]
                } else if inst == 2 {
                    let k = (id - 1) as usize;
                    match p[0] {
                        0x38 => {
                            let mut v = vec![0; 15];
                            v[0..2].copy_from_slice(&2048u16.to_le_bytes());
                            v[6] = 120;
                            v[7] = 30;
                            v
                        }
                        0x28 => {
                            if torque[k] == 0 {
                                armed &= !(1 << k);
                            }
                            vec![torque[k]]
                        }
                        0x2c => pwm[k].to_le_bytes().to_vec(),
                        0x21 => vec![mode[k]],
                        0x37 => vec![lock[k]],
                        _ => panic!("Unexpected register"),
                    }
                } else {
                    let k = (id - 1) as usize;
                    assert_eq!(inst, 3);
                    match p[0] {
                        0x28 => {
                            if p[1] == 1 {
                                assert_eq!(pwm[k], 0);
                                assert!(armed & (1 << k) != 0, "torque enabled on an unarmed axis");
                                enables[k] += 1;
                            }
                            torque[k] = p[1]
                        }
                        0x2c => {
                            pwm[k] = u16::from_le_bytes([p[1], p[2]]);
                            if pwm[k] & 1023 > 0 {
                                assert!(armed & (1 << k) != 0, "drive on an unarmed axis");
                                drives[k] += 1;
                            }
                        }
                        0x21 => mode[k] = p[1],
                        0x37 => lock[k] = p[1],
                        _ => panic!("Unexpected write"),
                    };
                    vec![]
                };
                peer.write_all(&packet(id, 0, &values).unwrap()).unwrap();
            }
            (enables, drives, max_mask, torque)
        })
    }
    fn two_axis_sweep(version: u8) -> (R<SweepOutcome>, ([u32; 3], [u32; 3], u8, [u8; 3])) {
        let (mut bus, peer) = pair();
        let task = multi_axis_peer(peer, version);
        let tuning:SweepTuning=serde_json::from_value(serde_json::from_str::<serde_json::Value>(include_str!("../../../../examples/actuators/hx30hm/hardware/2026-09-21-leg-calibration/server.json")).unwrap()["sweep_tuning"].clone()).unwrap();
        let axis = AxisCalibration { role: "test".into(), lower: Some(1800), upper: Some(2300), ..Default::default() };
        let cancel = AtomicBool::new(false);
        let mut calls = 0;
        let input = SweepInput { speed_counts_s: 50., pwm_limit: 1000 };
        let outcome = bus.controlled_motion_multi(&[2, 3], &[axis.clone(), axis.clone()], 64, &tuning, &cancel, false, DriveMode::Pwm, || {
            calls += 1;
            Ok((calls < 12).then(|| vec![(input, MotionCommand::Sweep, axis.clone()), (input, MotionCommand::Hold, axis.clone())]))
        }, |_, _, _| Ok(()));
        drop(bus);
        (outcome, task.join().unwrap())
    }
    #[test]
    fn profile_six_energizes_two_axes_in_one_session_and_stops_both() {
        let (outcome, (enables, drives, max_mask, torque)) = two_axis_sweep(6);
        let outcome = outcome.unwrap();
        assert!(outcome.motion_error.is_none(), "{:?}", outcome.motion_error);
        assert_eq!(max_mask, 0b110, "motors 2 and 3 armed together");
        assert_eq!(enables, [0, 1, 1], "each axis enabled exactly once");
        assert!(drives[1] >= 3, "the swept axis is driven: {drives:?}");
        assert_eq!(torque, [0; 3], "every axis ends torque-off");
    }
    #[test]
    fn profile_five_refuses_several_axes_before_any_drive() {
        let (outcome, (enables, drives, _, _)) = two_axis_sweep(5);
        assert!(outcome.unwrap_err().contains("one motor at a time"));
        assert_eq!((enables, drives), ([0; 3], [0; 3]));
    }
    /// Fake bridge whose deadlines trip in wall time, like the FPGA profile 7
    /// (feedback 400 ms, command 600 ms).
    #[test]
    fn watchdog_proof_waits_for_the_loaded_profiles_deadlines() {
        let (mut bus, mut peer) = pair();
        let task = std::thread::spawn(move || {
            let (mut armed, mut latched, mut reason) = (0u8, true, 1u8);
            let (mut fed, mut renewed) = (Instant::now(), Instant::now());
            loop {
                let mut h = [0; 4];
                if peer.read_exact(&mut h).is_err() {
                    break;
                }
                let mut rest = vec![0; h[3] as usize];
                peer.read_exact(&mut rest).unwrap();
                let (id, inst, p) = (h[2], rest[0], &rest[1..rest.len() - 1]);
                if armed != 0 && fed.elapsed() > Duration::from_millis(400) {
                    (armed, latched, reason) = (0, true, 7);
                } else if armed != 0 && renewed.elapsed() > Duration::from_millis(600) {
                    (armed, latched, reason) = (0, true, 8);
                }
                let values = if id == 254 {
                    match p[0] {
                        0 => (armed, latched, reason) = (0, true, 10),
                        1 => {
                            (armed, latched, reason) = (1 << (p[1] - 1), false, 0);
                            (fed, renewed) = (Instant::now(), Instant::now());
                        }
                        2 | 7 => (),
                        _ => panic!("Unexpected supervisor opcode"),
                    }
                    vec![7, u8::from(latched), reason, 254, armed, 0, 7, 0, 60, 90, 126, 208, 7]
                } else {
                    assert_eq!(inst, 2, "no writes during a zero-drive proof");
                    match p[0] {
                        0x38 => {
                            fed = Instant::now();
                            let mut v = vec![0; 15];
                            v[0..2].copy_from_slice(&2048u16.to_le_bytes());
                            (v[6], v[7]) = (120, 30);
                            v
                        }
                        0x28 => vec![0],
                        0x2c => vec![0, 0],
                        _ => panic!("Unexpected register"),
                    }
                };
                peer.write_all(&packet(id, 0, &values).unwrap()).unwrap();
            }
        });
        let started = Instant::now();
        bus.prove_watchdogs(2).unwrap();
        assert!(started.elapsed() > Duration::from_millis(1000), "both deadlines were actually waited for");
        drop(bus);
        task.join().unwrap();
    }
    /// Fake profile-7 bridge with one moving motor: speed = 1500 counts/s per
    /// duty beyond 8% friction, first-order lag 50 ms, integrated in wall time.
    #[test]
    fn identification_measures_the_motor_and_designs_gains() {
        let (mut bus, mut peer) = pair();
        let task = std::thread::spawn(move || {
            let (mut armed, mut torque, mut pwm) = (0u8, 0u8, 0i32);
            let (mut position, mut speed, mut last) = (2048f64, 0f64, Instant::now());
            let mut max_excursion = 0f64;
            loop {
                let mut h = [0; 4];
                if peer.read_exact(&mut h).is_err() {
                    break;
                }
                let mut rest = vec![0; h[3] as usize];
                peer.read_exact(&mut rest).unwrap();
                let (id, inst, p) = (h[2], rest[0], &rest[1..rest.len() - 1]);
                let dt = last.elapsed().as_secs_f64();
                last = Instant::now();
                let duty = if torque == 1 { pwm as f64 / 1000. } else { 0. };
                let target = (duty.abs() - 0.08).max(0.) * duty.signum() * 1500.;
                speed += (target - speed) * (dt / 0.05).min(1.);
                position += speed * dt;
                max_excursion = max_excursion.max((position - 2048.).abs());
                let values = if id == 254 {
                    match p[0] {
                        0 => (armed, torque, pwm) = (0, 0, 0),
                        1 => armed |= 1 << (p[1] - 1),
                        2 | 3 | 7 => (),
                        _ => panic!("Unexpected supervisor opcode"),
                    }
                    vec![7, u8::from(armed == 0), 0, 254, armed, 0, 7, 0, 60, 90, 126, 208, 7]
                } else if inst == 2 {
                    match p[0] {
                        0x38 => {
                            let mut v = vec![0; 15];
                            v[0..2].copy_from_slice(&((position.round() as i32).rem_euclid(4096) as u16).to_le_bytes());
                            (v[6], v[7]) = (120, 30);
                            v
                        }
                        0x28 => {
                            if torque == 0 {
                                armed = 0;
                            }
                            vec![torque]
                        }
                        0x2c => ((pwm.unsigned_abs() as u16) | if pwm < 0 { 1024 } else { 0 }).to_le_bytes().to_vec(),
                        0x21 => vec![2],
                        0x37 => vec![1],
                        _ => panic!("Unexpected register"),
                    }
                } else {
                    match p[0] {
                        0x28 => torque = p[1],
                        0x2c => {
                            let raw = u16::from_le_bytes([p[1], p[2]]);
                            pwm = (raw & 1023) as i32 * if raw & 1024 != 0 { -1 } else { 1 };
                        }
                        _ => (),
                    }
                    vec![]
                };
                peer.write_all(&packet(id, 0, &values).unwrap()).unwrap();
            }
            max_excursion
        });
        let cancel = AtomicBool::new(false);
        let record = bus.identify(1, 200, 0.6, 0.02, &cancel, |_| {}).unwrap();
        drop(bus);
        let excursion = task.join().unwrap();
        assert!(excursion < 200. + 60., "stayed inside the travel budget and FPGA window: {excursion}");
        let steps: Vec<super::super::motor_identification::StepTrace> = serde_json::from_value(record["steps"].clone()).unwrap();
        let fits: Vec<_> = steps.iter().filter_map(super::super::motor_identification::fit_step).collect();
        assert!(fits.len() >= 3, "{} usable steps of {}", fits.len(), steps.len());
        let breakaway: [f64; 2] = serde_json::from_value(record["breakaway_duty"].clone()).unwrap();
        assert!(breakaway.iter().all(|b| (0.06..0.2).contains(b)), "{breakaway:?}");
        let tuning = super::super::motor_identification::design(&fits, breakaway, record["loop_period_s"].as_f64().unwrap(), 0.08, "x").unwrap();
        assert!((tuning.gain_counts_s_per_duty - 1500.).abs() < 450., "{}", tuning.gain_counts_s_per_duty);
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
    #[test]
    fn virtual_socket_handshake_pins_identity_and_cannot_open_serial_paths() {
        use std::os::unix::{fs::FileTypeExt, net::UnixListener};
        // Short absolute base: macOS $TMPDIR exceeds the 104-byte sun_path limit.
        let base = std::path::PathBuf::from(format!("/tmp/vc-{}-{}", std::process::id(), &crate::hardware_client::new_client_id()[..8]));
        std::fs::create_dir(&base).unwrap();
        // Removed even when an assertion below fails.
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
        let _cleanup = Cleanup(base.clone());
        let socket = base.join("bench.sock");
        let log = base.join("serial.jsonl");
        let instance = crate::hardware_client::new_client_id();
        for expected in [Some(instance.as_str()), Some("ffffffff-ffff-ffff-ffff-ffffffffffff")] {
            let listener = UnixListener::bind(&socket).unwrap();
            let peer_instance = instance.clone();
            let server = std::thread::spawn(move || {
                let mut peer = listener.accept().unwrap().0;
                peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                let mut greeting = [0; b"HX-VIRTUAL-CALIBRATION/1\n".len()];
                peer.read_exact(&mut greeting).unwrap();
                assert_eq!(&greeting, b"HX-VIRTUAL-CALIBRATION/1\n");
                writeln!(peer,"{}",serde_json::json!({"schema_version":1,"kind":"virtual_calibration","bench_instance":peer_instance})).unwrap();
            });
            let opened = CalibrationBus::open_virtual(&socket, expected, &log);
            if expected == Some(instance.as_str()) {
                let (bus, actual) = opened.unwrap();
                assert_eq!(actual, instance);
                assert!(bus.file.metadata().unwrap().file_type().is_socket());
            } else { assert!(opened.is_err()); }
            server.join().unwrap();
            std::fs::remove_file(&socket).unwrap();
        }
        // UnixStream::connect refuses regular device/file paths; there is no
        // OpenOptions(serial), stty, or physical fallback in this consumer.
        assert!(CalibrationBus::open_virtual(Path::new("/dev/null"),None,&log).is_err());
    }

}
