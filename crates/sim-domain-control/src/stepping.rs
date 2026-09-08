//! Sampled support-phase sequencing and planar foothold references.
//! Outputs are desired positions, never physical poses or applied forces.
use serde::{Deserialize, Serialize};

/// Optional velocity-indexed posture schedule. Interpolate between sorted knots;
/// clamp outside their span. Sample only when a new foot transfer starts.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandPosture {
    pub forward_speed_m_s: f64,
    pub support_offsets_m: Vec<[f64; 3]>,
    pub stance_offsets_m: Vec<[f64; 2]>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepSequenceConfig {
    pub period_s: f64,
    pub initial_hold_s: f64,
    /// Shift, raise, lower, return, settle. All durations lie on the sample grid.
    pub phase_durations_s: [f64; 5],
    pub maximum_wait_s: f64,
    pub qualification_s: f64,
    pub lift_m: f64,
    pub order: Vec<usize>,
    /// Desired support displacement in heading axes for each foot; Z is crouch.
    pub support_offsets_m: Vec<[f64; 3]>,
    /// Persistent change from the initial stance, applied when each foot first moves.
    pub stance_offsets_m: Vec<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub command_postures: Vec<CommandPosture>,
    /// Replan a crawl cycle at the next transfer after a translational reversal.
    #[serde(default, skip_serializing_if = "is_false")]
    pub restart_order_on_translation_reversal: bool,
    /// Reconsider the command after the support shift, before lifting a foot.
    /// A stop cancels the unstarted swing and recenters with all feet planted.
    /// Non-reversing direction changes preserve the completed support shift and
    /// retarget only the unstarted swing and subsequent body return. Translation
    /// reversals retain the committed transfer before selecting a new stance.
    /// The caller still checks
    /// inverse kinematics, clearance and support of every reference.
    #[serde(default, skip_serializing_if = "is_false")]
    pub update_command_before_lift: bool,
    /// Fraction [0,1] of the commanded planar body advance performed during
    /// raise/lower, with one smooth profile across both phases. The support
    /// shift is retained until landing. Zero preserves sequential body return.
    /// These are references only; callers must still validate support and IK.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub swing_body_advance_fraction: f64,
    /// Spread horizontal foot travel smoothly across both raise and lower.
    /// Vertical clearance retains the separate raise/lower trajectory. False
    /// completes horizontal travel during raise, as in the original crawl.
    #[serde(default, skip_serializing_if = "is_false")]
    pub whole_swing_horizontal_motion: bool,
    pub maximum_speed_m_s: f64,
    pub maximum_yaw_rate_rad_s: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StepPhase {
    Hold,
    Idle,
    Shift,
    Raise,
    Lower,
    Return,
    Settle,
    Recenter,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StepReference {
    pub sample: u64,
    pub phase: StepPhase,
    pub foot: Option<usize>,
    pub step: usize,
    pub progress: f64,
    pub waiting: bool,
    pub body_world_m: [f64; 3],
    pub yaw_rad: f64,
    pub feet_world_m: Vec<[f64; 3]>,
    pub latched_twist: [f64; 3],
}
#[derive(Clone, Debug)]
pub struct StepSequence {
    config: StepSequenceConfig,
    ticks: [u64; 5],
    initial_ticks: u64,
    wait_ticks: u64,
    qualification_ticks: u64,
    sample: u64,
    phase_tick: u64,
    waited: u64,
    qualified: u64,
    phase: StepPhase,
    step: usize,
    order_slot: usize,
    last_translation: [f64; 2],
    center: [f64; 3], // world x,y,yaw
    center_z: f64,
    home: Vec<[f64; 3]>, // heading-local XYZ relative to initial center
    feet: Vec<[f64; 3]>,
    next_center: [f64; 3],
    shift_body: [f64; 3],
    swing_start: [f64; 3],
    swing_end: [f64; 3],
    command: [f64; 3],
}
fn is_false(value: &bool) -> bool {
    !*value
}
fn is_zero(value: &f64) -> bool {
    *value == 0.
}
fn rotate(yaw: f64, v: [f64; 2]) -> [f64; 2] {
    let (s, c) = yaw.sin_cos();
    [c * v[0] - s * v[1], s * v[0] + c * v[1]]
}
fn smooth(t: f64) -> f64 {
    let t = t.clamp(0., 1.);
    t * t * t * (10. + t * (-15. + 6. * t))
}
fn lerp<const N: usize>(a: [f64; N], b: [f64; N], s: f64) -> [f64; N] {
    std::array::from_fn(|i| a[i] + s * (b[i] - a[i]))
}
/// Integrate a constant body-frame planar twist exactly, including zero turn.
/// This integrates a command reference, not the robot's dynamics.
pub fn advance_planar(pose: [f64; 3], twist: [f64; 3], dt: f64) -> [f64; 3] {
    let angle = twist[2] * dt;
    let (a, b) = if angle.abs() < 1e-6 {
        (
            dt * (1. - angle * angle / 6.),
            dt * (angle / 2. - angle * angle * angle / 24.),
        )
    } else {
        (angle.sin() / twist[2], (1. - angle.cos()) / twist[2])
    };
    let delta = rotate(
        pose[2],
        [a * twist[0] - b * twist[1], b * twist[0] + a * twist[1]],
    );
    [pose[0] + delta[0], pose[1] + delta[1], pose[2] + angle]
}
impl StepSequence {
    pub fn new(
        config: StepSequenceConfig,
        body: [f64; 3],
        yaw: f64,
        feet: Vec<[f64; 3]>,
    ) -> Result<Self, String> {
        let grid = |s: f64| -> Result<u64, String> {
            let t = s / config.period_s;
            if !s.is_finite() || s < 0. || !t.is_finite() || t > 1e8 || (t - t.round()).abs() > 1e-8
            {
                return Err("step durations must lie on the finite sample grid".into());
            }
            Ok(t.round() as u64)
        };
        if !config.swing_body_advance_fraction.is_finite()
            || !(0.0..=1.0).contains(&config.swing_body_advance_fraction)
        {
            return Err("swing body advance fraction must be finite and in [0,1]".into());
        }
        if !config.period_s.is_finite()
            || config.period_s <= 0.
            || feet.len() < 3
            || feet.len() > 64
            || config.order.len() != feet.len()
            || config.support_offsets_m.len() != feet.len()
            || config.stance_offsets_m.len() != feet.len()
            || config
                .order
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                != (0..feet.len()).collect()
            || body
                .iter()
                .chain(std::iter::once(&yaw))
                .chain(feet.iter().flatten())
                .chain(config.support_offsets_m.iter().flatten())
                .chain(config.stance_offsets_m.iter().flatten())
                .any(|v| !v.is_finite())
            || [
                config.lift_m,
                config.maximum_speed_m_s,
                config.maximum_yaw_rate_rad_s,
            ]
            .iter()
            .any(|v| !v.is_finite() || *v <= 0.)
        {
            return Err("finite stepping frame, positive scales, and a permutation of distinct feet required".into());
        }
        for (i, posture) in config.command_postures.iter().enumerate() {
            if !posture.forward_speed_m_s.is_finite()
                || posture.forward_speed_m_s.abs() > config.maximum_speed_m_s
                || posture.support_offsets_m.len() != feet.len()
                || posture.stance_offsets_m.len() != feet.len()
                || posture
                    .support_offsets_m
                    .iter()
                    .flatten()
                    .chain(posture.stance_offsets_m.iter().flatten())
                    .any(|v| !v.is_finite())
                || (i > 0
                    && posture.forward_speed_m_s
                        <= config.command_postures[i - 1].forward_speed_m_s)
            {
                return Err("posture knots require strictly sorted bounded speeds and finite per-foot offsets".into());
            }
        }
        let ticks = config
            .phase_durations_s
            .map(grid)
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        if ticks.contains(&0) {
            return Err("positive step phase durations required".into());
        }
        let initial_ticks = grid(config.initial_hold_s)?;
        let wait_ticks = grid(config.maximum_wait_s)?;
        let qualification_ticks = grid(config.qualification_s)?.max(1);
        if wait_ticks == 0 || qualification_ticks > wait_ticks {
            return Err("bounded positive qualification/wait required".into());
        }
        let home = feet
            .iter()
            .map(|f| {
                let d = rotate(-yaw, [f[0] - body[0], f[1] - body[1]]);
                [d[0], d[1], f[2] - body[2]]
            })
            .collect();
        Ok(Self {
            config,
            ticks: ticks.try_into().unwrap(),
            initial_ticks,
            wait_ticks,
            qualification_ticks,
            sample: 0,
            phase_tick: 0,
            waited: 0,
            qualified: 0,
            phase: StepPhase::Hold,
            step: 0,
            order_slot: 0,
            last_translation: [0.; 2],
            center: [body[0], body[1], yaw],
            center_z: body[2],
            home,
            feet,
            next_center: [body[0], body[1], yaw],
            shift_body: body,
            swing_start: [0.; 3],
            swing_end: [0.; 3],
            command: [0.; 3],
        })
    }
    pub fn config(&self) -> &StepSequenceConfig {
        &self.config
    }
    pub fn next_foot(&self) -> usize {
        self.config.order[self.order_slot]
    }
    fn posture(&self, speed: f64, foot: usize) -> ([f64; 3], [f64; 2]) {
        let knots = &self.config.command_postures;
        if knots.is_empty() {
            return (
                self.config.support_offsets_m[foot],
                self.config.stance_offsets_m[foot],
            );
        }
        let hi = knots
            .partition_point(|p| p.forward_speed_m_s < speed)
            .min(knots.len() - 1);
        let lo = hi.saturating_sub(1);
        let a = &knots[lo];
        let b = &knots[hi];
        let weight = if hi == lo {
            0.
        } else {
            ((speed - a.forward_speed_m_s) / (b.forward_speed_m_s - a.forward_speed_m_s))
                .clamp(0., 1.)
        };
        (
            lerp(a.support_offsets_m[foot], b.support_offsets_m[foot], weight),
            lerp(a.stance_offsets_m[foot], b.stance_offsets_m[foot], weight),
        )
    }
    fn start(&mut self, command: [f64; 3]) {
        if self.config.restart_order_on_translation_reversal
            && self.last_translation[0] * command[0] + self.last_translation[1] * command[1]
                < -1e-24
        {
            self.order_slot = 0;
        }
        if command[0].hypot(command[1]) > 1e-12 {
            self.last_translation = [command[0], command[1]];
        }
        self.command = command;
        let foot = self.next_foot();
        self.plan_landing(command);
        let (shift, _) = self.posture(command[0], foot);
        let xy = rotate(self.center[2], [shift[0], shift[1]]);
        self.shift_body = [
            self.center[0] + xy[0],
            self.center[1] + xy[1],
            self.center_z + shift[2],
        ];
        self.phase = StepPhase::Shift;
        self.phase_tick = 0;
    }
    fn plan_landing(&mut self, command: [f64; 3]) {
        let foot = self.next_foot();
        let slot = self.order_slot;
        let seconds = self.config.phase_durations_s.iter().sum::<f64>();
        self.next_center = advance_planar(self.center, command, seconds);
        let landing = advance_planar(
            self.center,
            command,
            seconds * (self.config.order.len() - slot) as f64,
        );
        let (_, stance) = self.posture(command[0], foot);
        let offset = rotate(
            landing[2],
            [
                self.home[foot][0] + stance[0],
                self.home[foot][1] + stance[1],
            ],
        );
        self.swing_start = self.feet[foot];
        self.swing_end = [
            landing[0] + offset[0],
            landing[1] + offset[1],
            self.center_z + self.home[foot][2],
        ];
    }
    /// Exactly one call per controller sample. Readiness comes from the caller's
    /// declared observations. A failed call leaves the sequence unchanged.
    pub fn sample(
        &mut self,
        time_s: f64,
        command: [f64; 3],
        support_ready: bool,
        landed: bool,
    ) -> Result<StepReference, String> {
        if !time_s.is_finite()
            || (time_s - self.sample as f64 * self.config.period_s).abs() > 1e-8
            || command.iter().any(|v| !v.is_finite())
            || command[0].hypot(command[1]) > self.config.maximum_speed_m_s + 1e-12
            || command[2].abs() > self.config.maximum_yaw_rate_rad_s + 1e-12
        {
            return Err("stepping sample must match its clock and bounded planar command".into());
        }
        let mut next = self.clone();
        let result = next.sample_inner(command, support_ready, landed)?;
        *self = next;
        Ok(result)
    }
    fn sample_inner(
        &mut self,
        command: [f64; 3],
        support_ready: bool,
        landed: bool,
    ) -> Result<StepReference, String> {
        use StepPhase::*;
        let enabled = command.iter().any(|v| v.abs() > 1e-12);
        let duration = match self.phase {
            Hold => self.initial_ticks,
            Idle => 0,
            Shift => self.ticks[0],
            Raise => self.ticks[1],
            Lower => self.ticks[2],
            Return => self.ticks[3],
            Settle => self.ticks[4],
            Recenter => self.ticks[3] + self.ticks[4],
        };
        let condition = match self.phase {
            Shift => support_ready,
            Lower | Recenter => landed,
            _ => true,
        };
        let guarded = matches!(self.phase, Shift | Lower | Recenter);
        self.qualified = if condition {
            self.qualified.saturating_add(1)
        } else {
            0
        };
        let waiting =
            self.phase_tick >= duration && guarded && self.qualified < self.qualification_ticks;
        if waiting {
            self.waited += 1;
            if self.waited > self.wait_ticks {
                return Err(format!(
                    "step {} {:?} readiness timed out",
                    self.step, self.phase
                ));
            }
        } else if self.phase_tick >= duration {
            self.waited = 0;
            self.qualified = 0;
            self.phase_tick = 0;
            match self.phase {
                Hold | Idle => {
                    if enabled {
                        self.start(command)
                    } else {
                        self.phase = Idle
                    }
                }
                Shift => {
                    if self.config.update_command_before_lift {
                        if !enabled {
                            self.command = [0.; 3];
                            self.phase = Recenter;
                        } else if self.last_translation[0] * command[0]
                            + self.last_translation[1] * command[1]
                            >= -1e-24
                        {
                            if command[0].hypot(command[1]) > 1e-12 {
                                self.last_translation = [command[0], command[1]];
                            }
                            self.command = command;
                            self.plan_landing(command);
                            self.phase = Raise;
                        } else {
                            // The completed support shift was chosen for the
                            // old direction. Reversal needs the next transfer's
                            // stance/order selection, rather than a new landing
                            // imposed on that committed support arrangement.
                            self.phase = Raise;
                        }
                    } else {
                        self.phase = Raise;
                    }
                }
                Raise => self.phase = Lower,
                Lower => {
                    let foot = self.next_foot();
                    self.feet[foot] = self.swing_end;
                    self.phase = Return
                }
                Return => {
                    self.center = self.next_center;
                    self.phase = Settle
                }
                Settle => {
                    self.step += 1;
                    self.order_slot = (self.order_slot + 1) % self.config.order.len();
                    if enabled {
                        self.start(command)
                    } else {
                        self.phase = Idle
                    }
                }
                Recenter => {
                    // No foot transfer took place: preserve its index and order.
                    if enabled {
                        self.start(command)
                    } else {
                        self.phase = Idle
                    }
                }
            }
        }
        let mut body = [self.center[0], self.center[1], self.center_z];
        let mut yaw = self.center[2];
        let mut feet = self.feet.clone();
        if matches!(self.phase, Idle) {
            self.command = [0.; 3];
        }
        let mut progress = 0.;
        let foot = self.next_foot();
        match self.phase {
            Shift => {
                progress = self.phase_tick as f64 / self.ticks[0] as f64;
                body = lerp(body, self.shift_body, smooth(progress));
            }
            Raise | Lower => {
                body = self.shift_body;
                let up = matches!(self.phase, Raise);
                let duration = if up { self.ticks[1] } else { self.ticks[2] };
                progress = self.phase_tick as f64 / duration as f64;
                if self.config.swing_body_advance_fraction > 0. {
                    let elapsed = self.phase_tick + if up { 0 } else { self.ticks[1] };
                    let fraction = self.config.swing_body_advance_fraction
                        * smooth(elapsed as f64 / (self.ticks[1] + self.ticks[2]) as f64);
                    for axis in 0..2 {
                        body[axis] += fraction * (self.next_center[axis] - self.center[axis]);
                    }
                    yaw += fraction * (self.next_center[2] - self.center[2]);
                }
                let mut peak = self.swing_end;
                peak[2] += self.config.lift_m;
                feet[foot] = if up {
                    lerp(self.swing_start, peak, smooth(progress))
                } else {
                    lerp(peak, self.swing_end, smooth(progress))
                };
                if self.config.whole_swing_horizontal_motion {
                    let elapsed = self.phase_tick + if up { 0 } else { self.ticks[1] };
                    let fraction = smooth(elapsed as f64 / (self.ticks[1] + self.ticks[2]) as f64);
                    for axis in 0..2 {
                        feet[foot][axis] = self.swing_start[axis]
                            + fraction * (self.swing_end[axis] - self.swing_start[axis]);
                    }
                }
            }
            Return => {
                progress = self.phase_tick as f64 / self.ticks[3] as f64;
                let s = smooth(progress);
                body = lerp(
                    self.shift_body,
                    [self.next_center[0], self.next_center[1], self.center_z],
                    s,
                );
                yaw = self.center[2] + s * (self.next_center[2] - self.center[2]);
                if self.config.swing_body_advance_fraction > 0. {
                    let fraction = self.config.swing_body_advance_fraction * (1. - s);
                    for axis in 0..2 {
                        body[axis] += fraction * (self.next_center[axis] - self.center[axis]);
                    }
                    yaw += fraction * (self.next_center[2] - self.center[2]);
                }
            }
            Settle => progress = self.phase_tick as f64 / self.ticks[4] as f64,
            Recenter => {
                progress = self.phase_tick as f64 / (self.ticks[3] + self.ticks[4]) as f64;
                body = lerp(
                    self.shift_body,
                    body,
                    smooth(self.phase_tick as f64 / self.ticks[3] as f64),
                );
            }
            Hold | Idle => {}
        }
        let reference = StepReference {
            sample: self.sample,
            phase: self.phase.clone(),
            foot: if matches!(self.phase, Hold | Idle | Recenter) {
                None
            } else {
                Some(foot)
            },
            step: self.step,
            progress: progress.min(1.),
            waiting,
            body_world_m: body,
            yaw_rad: yaw,
            feet_world_m: feet,
            latched_twist: self.command,
        };
        self.sample += 1;
        self.phase_tick = self.phase_tick.saturating_add(1);
        Ok(reference)
    }
}
