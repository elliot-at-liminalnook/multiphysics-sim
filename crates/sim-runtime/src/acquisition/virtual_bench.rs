//! Simulated HX-30HM calibration bench: the FPGA calibration supervisor
//! (profile 7 policy) in front of three servos with a friction/lag motor
//! model. It speaks the same half-duplex packets as the real bridge, so the
//! calibration host, server and browser can be exercised without hardware.
//!
//! The motor model is fitted to this fixture's identification runs (about
//! 3200 counts/s per unit duty, 60 ms lag, 5–7% breakaway, 4–5% moving
//! friction). The servo's internal position and speed loops are generic
//! stand-ins with plausible bandwidth, not identified firmware behaviour.
use super::servo_bus::packet;

const COUNTS: f64 = 4096.;

/// Servo control mode register 0x21.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServoMode {
    Position,
    Speed,
    Pwm,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotorModel {
    /// Counts/s per unit duty beyond moving friction.
    pub speed_gain: f64,
    pub lag_s: f64,
    pub breakaway_duty: f64,
    pub moving_friction_duty: f64,
    /// Constant load (e.g. gravity) as an equivalent duty, positive = increasing counts.
    pub load_duty: f64,
    /// Gravity-like load varying with position: `amplitude · cos((counts − zero)·2π/4096)`.
    pub gravity_duty: f64,
    pub gravity_zero_counts: f64,
    /// Transmission between motor encoder and joint: total backlash (counts)
    /// and compliance (joint deflection in counts per unit load duty).
    pub backlash_counts: f64,
    pub compliance_counts_per_duty: f64,
    /// Current at full duty and stall (A); winding resistance (Ω).
    pub stall_current_a: f64,
    pub winding_resistance_ohm: f64,
    /// Winding thermal capacity (J/K) and resistance to ambient (K/W).
    pub heat_capacity_j_k: f64,
    pub thermal_resistance_k_w: f64,
}
impl Default for MotorModel {
    fn default() -> Self {
        Self {
            speed_gain: 3200., lag_s: 0.06, breakaway_duty: 0.06, moving_friction_duty: 0.045, load_duty: 0.,
            gravity_duty: 0., gravity_zero_counts: 2048., backlash_counts: 0., compliance_counts_per_duty: 0.,
            stall_current_a: 3.0, winding_resistance_ohm: 3.7, heat_capacity_j_k: 12., thermal_resistance_k_w: 9.,
        }
    }
}
impl MotorModel {
    /// External load (duty-equivalent) at a motor position.
    pub fn load_at(&self, position: f64) -> f64 {
        self.load_duty + self.gravity_duty * ((position - self.gravity_zero_counts) * std::f64::consts::TAU / COUNTS).cos()
    }
}

#[derive(Clone, Debug)]
pub struct Servo {
    pub model: MotorModel,
    /// Continuous position (counts, not wrapped) and speed (counts/s).
    pub position: f64,
    pub speed: f64,
    pub torque: bool,
    pub mode: ServoMode,
    pub nvs_locked: bool,
    /// Signed PWM duty in thousandths (PWM mode).
    pub pwm: i32,
    pub goal_position: i32,
    /// Signed speed goal (counts/s); in position mode the speed limit.
    pub goal_speed: i32,
    pub voltage_dv: u8,
    pub temperature_c: u8,
    integral: f64,
    /// Winding temperature (°C), joint-side angle (counts) and drawn current (A).
    pub winding_c: f64,
    pub joint: f64,
    pub current_a: f64,
    /// Supply voltage relative to nominal (sag), set by the bench each step.
    pub supply_scale: f64,
    /// Extra external load attached by a test (duty-equivalent), e.g. a known weight.
    pub attached_load_duty: f64,
}
impl Servo {
    pub fn new(position: f64, model: MotorModel) -> Self {
        Self {
            model,
            position,
            speed: 0.,
            torque: false,
            mode: ServoMode::Pwm,
            nvs_locked: true,
            pwm: 0,
            goal_position: position as i32,
            goal_speed: 0,
            voltage_dv: 118,
            temperature_c: 31,
            integral: 0.,
            winding_c: 31.,
            joint: position,
            current_a: 0.,
            supply_scale: 1.,
            attached_load_duty: 0.,
        }
    }
    /// Duty the servo applies this instant (its own firmware loop in position/speed modes).
    fn applied_duty(&mut self, dt: f64) -> f64 {
        if !self.torque {
            self.integral = 0.;
            return 0.;
        }
        match self.mode {
            ServoMode::Pwm => self.pwm as f64 / 1000.,
            ServoMode::Speed => {
                let error = self.goal_speed as f64 - self.speed;
                self.integral = (self.integral + error * dt).clamp(-200., 200.);
                (error / self.model.speed_gain * 0.8 + self.integral / self.model.speed_gain * 6.
                    + self.goal_speed as f64 / self.model.speed_gain)
                    .clamp(-1., 1.)
            }
            ServoMode::Position => {
                // Firmware-style position loop with a speed limit.
                let raw = self.position.rem_euclid(COUNTS);
                let mut error = self.goal_position as f64 - raw;
                if error > COUNTS / 2. {
                    error -= COUNTS;
                } else if error < -COUNTS / 2. {
                    error += COUNTS;
                }
                let limit = if self.goal_speed == 0 { 3000. } else { self.goal_speed.unsigned_abs() as f64 };
                let desired = (error * 12.).clamp(-limit, limit);
                ((desired - self.speed) * 0.0008 + desired / self.model.speed_gain).clamp(-1., 1.)
            }
        }
    }
    pub fn step(&mut self, dt: f64) {
        let drive = self.applied_duty(dt) * self.supply_scale;
        let load = self.model.load_at(self.position) + self.attached_load_duty;
        let duty = drive + load;
        let m = &self.model;
        let stuck = self.speed.abs() < 3.;
        let target = if stuck && duty.abs() < m.breakaway_duty {
            0.
        } else {
            (duty.abs() - m.moving_friction_duty).max(0.) * duty.signum() * m.speed_gain
        };
        self.speed += (target - self.speed) * (dt / m.lag_s).min(1.);
        if stuck && target == 0. {
            self.speed = 0.;
        }
        self.position += self.speed * dt;
        // Current: drive minus back-EMF share, at stall current per unit duty.
        self.current_a = (drive - self.speed / m.speed_gain) * m.stall_current_a;
        let heat = self.current_a * self.current_a * m.winding_resistance_ohm;
        self.winding_c += (heat - (self.winding_c - 25.) / m.thermal_resistance_k_w) / m.heat_capacity_j_k * dt;
        self.temperature_c = self.winding_c.round().clamp(0., 255.) as u8;
        // Joint side: follows the motor through backlash, deflected by load.
        let target = self.position - m.compliance_counts_per_duty * load;
        let half = m.backlash_counts * 0.5;
        if self.joint < target - half {
            self.joint = target - half;
        } else if self.joint > target + half {
            self.joint = target + half;
        }
    }
    fn telemetry(&self) -> Vec<u8> {
        let mut v = vec![0; 15];
        let pos = self.position.round().rem_euclid(COUNTS) as u16;
        v[0..2].copy_from_slice(&pos.to_le_bytes());
        let speed = (self.speed.abs().round().min(32767.) as u16) | if self.speed < 0. { 0x8000 } else { 0 };
        v[2..4].copy_from_slice(&speed.to_le_bytes());
        v[6] = self.voltage_dv;
        v[7] = self.temperature_c;
        v[10] = u8::from(self.speed.abs() > 3.);
        v
    }
}

/// FPGA calibration supervisor (profile 7) plus three servos, IDs 1..3.
pub struct Bench {
    pub servos: [Servo; 3],
    pub latched: bool,
    pub reason: u8,
    pub fault_id: u8,
    pub armed: u8,
    window: [Option<(i64, i64)>; 3],
    /// FPGA's continuous count per axis (anchored by operation 7).
    continuous: [i64; 3],
    last_raw: [Option<u16>; 3],
    feedback_age: [f64; 3],
    command_age: [f64; 3],
    pub feedback_timeout_s: f64,
    pub command_timeout_s: f64,
    /// Seconds per servo transaction on the wire (reply latency).
    pub transaction_s: f64,
    pub time: f64,
    /// Shared supply: open-circuit voltage and internal resistance.
    pub supply_open_v: f64,
    pub supply_resistance_ohm: f64,
    /// Present supply voltage and total drawn current (a virtual supply sensor).
    pub supply_v: f64,
    pub supply_current_a: f64,
}

impl Bench {
    pub fn new(start: [f64; 3], models: [MotorModel; 3]) -> Self {
        let [m0, m1, m2] = models;
        Self {
            servos: [Servo::new(start[0], m0), Servo::new(start[1], m1), Servo::new(start[2], m2)],
            latched: true,
            reason: 1,
            fault_id: 254,
            armed: 0,
            window: [None; 3],
            continuous: [0; 3],
            last_raw: [None; 3],
            feedback_age: [f64::INFINITY; 3],
            command_age: [f64::INFINITY; 3],
            feedback_timeout_s: 0.4,
            command_timeout_s: 0.6,
            transaction_s: 0.004,
            time: 0.,
            supply_open_v: 11.8,
            supply_resistance_ohm: 0.25,
            supply_v: 11.8,
            supply_current_a: 0.,
        }
    }
    /// Advance physics and supervisor deadlines.
    pub fn advance(&mut self, dt: f64) {
        let steps = (dt / 0.001).ceil().max(1.) as usize;
        let h = dt / steps as f64;
        for _ in 0..steps {
            let scale = self.supply_v / self.supply_open_v;
            for s in &mut self.servos {
                s.supply_scale = scale;
                s.step(h);
            }
            self.supply_current_a = self.servos.iter().map(|s| s.current_a.abs()).sum();
            self.supply_v = self.supply_open_v - self.supply_resistance_ohm * self.supply_current_a;
            let dv = (self.supply_v * 10.).round().clamp(0., 255.) as u8;
            for s in &mut self.servos {
                s.voltage_dv = dv;
            }
        }
        self.time += dt;
        for k in 0..3 {
            self.feedback_age[k] += dt;
            self.command_age[k] += dt;
            if self.armed & (1 << k) != 0 && !self.latched {
                if self.feedback_age[k] > self.feedback_timeout_s {
                    self.trip(7, k as u8 + 1);
                } else if self.command_age[k] > self.command_timeout_s {
                    self.trip(8, k as u8 + 1);
                }
            }
        }
    }
    fn trip(&mut self, reason: u8, id: u8) {
        self.latched = true;
        self.reason = reason;
        self.fault_id = id;
        self.armed = 0;
        self.stop_all();
    }
    /// The FPGA's STOP broadcast: zero drive and torque off everywhere.
    fn stop_all(&mut self) {
        for s in &mut self.servos {
            s.pwm = 0;
            s.goal_speed = 0;
            s.torque = false;
        }
    }
    fn status(&self) -> Vec<u8> {
        vec![7, u8::from(self.latched), self.reason, self.fault_id, self.armed, 0, 7, 0, 60, 90, 126, 208, 7]
    }
    fn raw(&self, k: usize) -> u16 {
        self.servos[k].position.round().rem_euclid(COUNTS) as u16
    }
    fn observe_raw(&mut self, k: usize) {
        let raw = self.raw(k);
        if let Some(last) = self.last_raw[k] {
            let mut d = raw as i64 - last as i64;
            if d > 2048 {
                d -= 4096;
            } else if d < -2048 {
                d += 4096;
            }
            self.continuous[k] += d;
        } else {
            self.continuous[k] = raw as i64;
        }
        self.last_raw[k] = Some(raw);
        self.feedback_age[k] = 0.;
    }
    /// Would this write drive the axis further outside its window?
    fn outward(&self, k: usize, direction: f64) -> bool {
        match self.window[k] {
            Some((lo, hi)) => (direction > 0. && self.continuous[k] >= hi) || (direction < 0. && self.continuous[k] <= lo),
            None => true,
        }
    }
    /// Handle one host packet; returns the reply bytes (empty when the
    /// supervisor blocks it, as the real bridge forwards nothing).
    pub fn handle(&mut self, id: u8, instruction: u8, p: &[u8]) -> Vec<u8> {
        if id == 254 && instruction == 0xa0 {
            self.local(p);
            return packet(254, 0, &self.status()).unwrap();
        }
        if !(1..=3).contains(&id) {
            return vec![];
        }
        let k = (id - 1) as usize;
        let armed = self.armed & (1 << k) != 0 && !self.latched;
        let reply = |values: &[u8]| packet(id, 0, values).unwrap();
        match instruction {
            2 => {
                let (address, n) = (p[0], p[1] as usize);
                let s = &self.servos[k];
                let values: Vec<u8> = match address {
                    0x38 if n == 15 => {
                        let t = s.telemetry();
                        self.observe_raw(k);
                        t
                    }
                    0x28 => {
                        if !s.torque && self.armed & (1 << k) != 0 {
                            // Solicited torque-off readback disarms (FPGA rule).
                            self.armed &= !(1 << k);
                        }
                        vec![u8::from(self.servos[k].torque)]
                    }
                    0x2c => {
                        let v = s.pwm.unsigned_abs() as u16 | if s.pwm < 0 { 1024 } else { 0 };
                        v.to_le_bytes()[..n.min(2)].to_vec()
                    }
                    0x21 => vec![match s.mode { ServoMode::Position => 0, ServoMode::Speed => 1, ServoMode::Pwm => 2 }],
                    0x37 => vec![u8::from(s.nvs_locked)],
                    0x2a | 0x2e => {
                        // [goal position, run time, goal speed], read from the requested start.
                        let speed = s.goal_speed.unsigned_abs() as u16 | if s.goal_speed < 0 { 0x8000 } else { 0 };
                        let mut block = Vec::new();
                        block.extend((s.goal_position as u16).to_le_bytes());
                        block.extend([0, 0]);
                        block.extend(speed.to_le_bytes());
                        let from = if address == 0x2a { 0 } else { 4 };
                        block.into_iter().skip(from).chain(std::iter::repeat(0)).take(n).collect()
                    }
                    _ => vec![0; n],
                };
                reply(&values)
            }
            3 => {
                let address = p[0];
                let data = &p[1..];
                let word = |i: usize| u16::from_le_bytes([data[i], data.get(i + 1).copied().unwrap_or(0)]);
                match address {
                    0x28 => {
                        if data[0] != 0 && !armed {
                            return vec![];
                        }
                        self.servos[k].torque = data[0] != 0;
                    }
                    0x2c => {
                        let raw = word(0);
                        let duty = (raw & 1023) as i32 * if raw & 1024 != 0 { -1 } else { 1 };
                        if raw & !2047 != 0 || (raw & 1023) > 1000 {
                            return vec![];
                        }
                        if duty != 0 && (!armed || self.outward(k, duty as f64)) {
                            return vec![];
                        }
                        self.servos[k].pwm = duty;
                    }
                    0x21 => {
                        if self.servos[k].nvs_locked || data[0] > 2 {
                            return vec![];
                        }
                        self.servos[k].mode = match data[0] { 0 => ServoMode::Position, 1 => ServoMode::Speed, _ => ServoMode::Pwm };
                    }
                    0x37 => self.servos[k].nvs_locked = data[0] != 0,
                    0x2a => {
                        if !armed {
                            return vec![];
                        }
                        let goal = word(0) as i32 & 4095;
                        let mut d = goal as i64 - self.raw(k) as i64;
                        if d > 2048 {
                            d -= 4096;
                        } else if d < -2048 {
                            d += 4096;
                        }
                        if let Some((lo, hi)) = self.window[k] {
                            let target = self.continuous[k] + d;
                            if target < lo || target > hi {
                                return vec![];
                            }
                        }
                        self.servos[k].goal_position = goal;
                        if data.len() >= 6 {
                            let v = word(4);
                            self.servos[k].goal_speed = (v & 0x7fff) as i32;
                        }
                    }
                    0x2e => {
                        let raw = word(0);
                        let speed = (raw & 0x7fff) as i32 * if raw & 0x8000 != 0 { -1 } else { 1 };
                        if speed != 0 && (!armed || self.outward(k, speed as f64)) {
                            return vec![];
                        }
                        self.servos[k].goal_speed = speed;
                    }
                    _ => return vec![],
                }
                reply(&[])
            }
            _ => vec![],
        }
    }
    fn local(&mut self, p: &[u8]) {
        let op = p.first().copied().unwrap_or(255);
        let axis = |id: u8| (1..=3).contains(&id).then(|| (id - 1) as usize);
        match op {
            0 => {
                self.latched = true;
                self.reason = 10;
                self.fault_id = 254;
                self.armed = 0;
                self.stop_all();
            }
            1 => {
                if let Some(k) = p.get(1).copied().and_then(axis) {
                    let fresh = self.feedback_age[k] < self.feedback_timeout_s;
                    let inside = self.window[k].is_some_and(|(lo, hi)| (lo..=hi).contains(&self.continuous[k]));
                    if fresh && inside && self.armed & (1 << k) == 0 {
                        self.latched = false;
                        self.reason = 0;
                        self.fault_id = 254;
                        self.armed |= 1 << k;
                        self.command_age[k] = 0.;
                    }
                }
            }
            3 => {
                if let Some(k) = p.get(1).copied().and_then(axis) {
                    if self.armed & (1 << k) != 0 && !self.latched {
                        self.command_age[k] = 0.;
                    }
                }
            }
            5 => {
                let mask = p.get(1).copied().unwrap_or(0);
                if !self.latched && mask != 0 && mask & !self.armed == 0 {
                    for k in 0..3 {
                        if mask & (1 << k) != 0 {
                            self.command_age[k] = 0.;
                        }
                    }
                }
            }
            7 if p.len() == 14 => {
                if let Some(k) = axis(p[1]) {
                    if self.armed & (1 << k) == 0 {
                        let i32_at = |i: usize| i32::from_le_bytes([p[i], p[i + 1], p[i + 2], p[i + 3]]) as i64;
                        let (anchor, lo, hi) = (i32_at(2), i32_at(6), i32_at(10));
                        let fresh = self.feedback_age[k] < self.feedback_timeout_s;
                        if fresh && anchor.rem_euclid(4096) == self.raw(k) as i64 && lo < hi && (lo..=hi).contains(&anchor) {
                            self.window[k] = Some((lo, hi));
                            self.continuous[k] = anchor;
                        } else {
                            self.window[k] = None;
                        }
                    }
                }
            }
            4 => {
                self.latched = true;
                self.reason = 10;
                self.armed = 0;
                self.stop_all();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pwm_step_matches_the_identified_response() {
        let mut s = Servo::new(2000., MotorModel::default());
        s.torque = true;
        s.pwm = 300;
        for _ in 0..500 {
            s.step(0.001);
        }
        let expected = (0.3 - 0.045) * 3200.;
        assert!((s.speed - expected).abs() < 20., "{} vs {expected}", s.speed);
        s.pwm = 40;
        s.speed = 0.;
        let before = s.position;
        for _ in 0..200 {
            s.step(0.001);
        }
        assert_eq!(s.position, before, "below breakaway the axis stays stuck");
    }

    #[test]
    fn position_and_speed_modes_reach_their_goals() {
        let mut s = Servo::new(1000., MotorModel::default());
        s.torque = true;
        s.mode = ServoMode::Position;
        s.goal_position = 1400;
        s.goal_speed = 600;
        for _ in 0..3000 {
            s.step(0.001);
        }
        assert!((s.position - 1400.).abs() < 12., "{}", s.position);
        s.mode = ServoMode::Speed;
        s.goal_speed = -300;
        for _ in 0..1500 {
            s.step(0.001);
        }
        assert!((s.speed + 300.).abs() < 30., "{}", s.speed);
    }

    #[test]
    fn supervisor_blocks_unarmed_drive_and_trips_on_silence() {
        let mut b = Bench::new([2048., 1000., 3000.], Default::default());
        assert!(b.handle(1, 3, &[0x2c, 100, 0]).is_empty(), "unarmed PWM is not forwarded");
        b.handle(1, 2, &[0x38, 15]);
        let anchor = 2048i32;
        let mut op7 = vec![7, 1];
        for v in [anchor, anchor - 100, anchor + 100] {
            op7.extend(v.to_le_bytes());
        }
        b.handle(254, 0xa0, &op7);
        b.handle(254, 0xa0, &[1, 1]);
        assert_eq!(b.armed, 1);
        assert!(!b.handle(1, 3, &[0x28, 1]).is_empty());
        assert!(!b.handle(1, 3, &[0x2c, 100, 0]).is_empty());
        b.advance(0.5);
        assert!(b.latched && b.reason == 7, "feedback deadline trips");
        assert!(!b.servos[0].torque);
    }
}
