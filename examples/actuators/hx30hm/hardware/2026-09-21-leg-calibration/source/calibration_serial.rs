//! Serial transport for supervised, operator-triggered encoder teaching.
//! Uses the shared HX packet/telemetry contract; never advances simulation.
use super::{
    calibration::AxisCalibration,
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
pub struct CalibrationBus {
    file: File,
    pending: PacketBuffer,
    log: File,
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
        // stty opens the port, so perform termios before requesting exclusion below instead.
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
                return Err(format!("ID {id}: serial reply timeout"));
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
    pub fn reconnect_stopped(&mut self, id: u8) -> R<Telemetry> {
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
        self.stop(id)
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
    pub fn feedback(&mut self, id: u8) -> R<Telemetry> {
        if !(1..=3).contains(&id) {
            return Err("Calibration profile only supports IDs 1..3".into());
        }
        Telemetry::decode(&self.read(id, 0x38, 15)?).map_err(str::to_string)
    }
    pub fn supervisor(&mut self, params: &[u8]) -> R<Vec<u8>> {
        let s = self.txn(254, 0xa0, params, 13)?;
        if s[0] != 2 || s[8..13] != [60, 90, 126, 208, 7] || s[4] & !7 != 0 || s[5] != 0 {
            return Err("Calibration FPGA profile is not loaded; motion disabled".into());
        }
        Ok(s)
    }
    fn arm(&mut self, id: u8, lo: u16, hi: u16) -> R<()> {
        self.supervisor(&[0])?;
        let [ll, lh] = lo.to_le_bytes();
        let [hl, hh] = hi.to_le_bytes();
        self.supervisor(&[6, id, ll, lh, hl, hh])?;
        self.feedback(id)?;
        let s = self.supervisor(&[1, id])?;
        if s[1] != 0 || s[4] != (1 << (id - 1)) {
            return Err("FPGA refused narrow-window arm".into());
        }
        Ok(())
    }
    pub fn stop(&mut self, id: u8) -> R<Telemetry> {
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
        let lo = t.position_raw.saturating_sub(8);
        let hi = (t.position_raw + 8).min(4095);
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
    pub fn jog(
        &mut self,
        id: u8,
        axis: &AxisCalibration,
        delta: i16,
        cancel: &AtomicBool,
    ) -> R<Telemetry> {
        let t = self.stop(id)?;
        Self::healthy(&t)?;
        let (target, lo, hi) = axis.jog(t.position_raw, delta)?;
        let run = (|| {
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
                return Err("Jog cancelled before enabling drive".into());
            }
            self.write(id, 0x28, &[1])?;
            let start = Instant::now();
            loop {
                if cancel.load(Ordering::SeqCst) {
                    return Err("Operator stop".into());
                }
                let t = self.feedback(id)?;
                Self::healthy(&t)?;
                if t.position_raw < lo || t.position_raw > hi {
                    return Err("Encoder left narrow jog window".into());
                }
                if (delta > 0 && t.position_raw >= target)
                    || (delta < 0 && t.position_raw <= target)
                {
                    break;
                }
                if start.elapsed() > Duration::from_millis(300) {
                    break;
                }
                let s = self.supervisor(&[3, id])?;
                if s[1] != 0 || s[4] != (1 << (id - 1)) {
                    return Err("FPGA stopped the jog".into());
                }
                // Deliberate low duty; never escalates when a joint does not move.
                let p = signed_pwm_write_parameters(
                    if delta > 0 { 25 } else { -25 },
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
            (Err(e), _) => Err(e),
            (Ok(()), Ok(t)) => Ok(t),
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
        assert!(bus.stop(3).unwrap_err().contains("STOP sent"));
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
    #[test]
    fn fresh_jog_is_bounded_and_finishes_with_verified_torque_off() {
        let (mut bus, mut peer) = pair();
        let t = std::thread::spawn(move || {
            let (mut pos, mut torque, mut pwm, mut mode, mut lock, mut armed) =
                (2048u16, 0u8, 0u16, 0u8, 1u8, 0u8);
            let mut writes = Vec::new();
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
                            armed = 0;
                            torque = 0;
                            pwm = 0
                        }
                        1 => armed = 4,
                        3 => (),
                        6 => {
                            assert!(
                                u16::from_le_bytes([p[4], p[5]]) - u16::from_le_bytes([p[2], p[3]])
                                    <= 32
                            )
                        }
                        _ => (),
                    };
                    vec![
                        2,
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
                            if torque == 1 && pwm > 0 {
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
                            assert!(pwm <= 25)
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
                peer.write_all(&packet(id, 0, &values).unwrap()).unwrap();
            }
            assert_eq!(torque, 0);
            assert_eq!(pwm, 0);
            assert!(writes.iter().any(|p| p == &vec![0x2c, 25, 0]));
        });
        let axis = AxisCalibration {
            role: "test axis".into(),
            ..Default::default()
        };
        let result = bus.jog(3, &axis, 1, &AtomicBool::new(false)).unwrap();
        assert_eq!(result.position_raw, 2049);
        drop(bus);
        t.join().unwrap();
    }
}
