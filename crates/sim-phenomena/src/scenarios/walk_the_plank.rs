//! 35. Walk the plank — `multibody` `contact` `seam` `environment`.
//!
//! A planar point-foot biped on stepping stones, after Dai et al.'s "Walk
//! the PLANC": a reduced-order stepping planner (the linear inverted
//! pendulum, LIP) supplies foot targets and step timing that are
//! consistent with the terrain, a PD loop tracks the joint targets, and a
//! learner — through the seam's environment mode — is meant to refine the
//! planner's targets with a Control Lyapunov Function reward on the LIP
//! coordinates. The plate exercises the pieces without learning: the
//! terrain curriculum, the planner walking the level-0 course, the CLF
//! reference it leaves behind, the environment's snapshot determinism,
//! and the planner's brittleness to a perception error — the motivation
//! for guiding learning with it rather than trusting it.
//!
//! Training: `clients/python/examples/planc/train.py` against the
//! `sim-gym` server (`cargo build --release -p sim-phenomena --bin sim-gym`).

use crate::Report;
use crate::world::{damped_runtime, registry};
use sim_compile::{Runtime, RuntimeSnapshot};
use sim_core::{BehaviorId, BehaviorRegistry, FnCoupler, ModelWorld, StateId};
use sim_couple::{Environment, Frame, Spaces};
use sim_domain_control::external::EXTERNAL;
use sim_domain_multibody::chain::CHAIN;
use sim_domain_multibody::contact as ct;
use sim_domain_sensing as sense;
use std::sync::{Arc, Mutex};

// ------------------------------------------------------------------ terrain

/// The four courses of the paper, from the curriculum level `d ∈ [0, 1]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Course {
    /// Flat stones with gaps growing with the level.
    Flat,
    /// Stones whose heights vary by up to ±0.2·d m.
    Varying,
    StairsUp,
    StairsDown,
}

impl Course {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "flat" => Some(Self::Flat),
            "varying" => Some(Self::Varying),
            "stairs-up" => Some(Self::StairsUp),
            "stairs-down" => Some(Self::StairsDown),
            _ => None,
        }
    }
}

/// Horizontal patches `(x0, x1, y)`: a start platform, the stones, an end
/// platform. Between them there is nothing to stand on.
#[derive(Clone, Debug, PartialEq)]
pub struct Terrain {
    pub patches: Vec<(f64, f64, f64)>,
}

/// splitmix64: a seed is the whole episode.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^= z >> 31;
        (z >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next()
    }
}

impl Terrain {
    pub const START: (f64, f64) = (-1.2, 0.3);
    pub const STONES: usize = 8;
    pub const END_LENGTH: f64 = 1.5;

    /// The paper's ranges: gap `[0.3, 0.3 + 0.4d]`, stone length
    /// `[0.13, 0.3]`, height variation `[−0.2d, 0.2d]`; stairs of depth
    /// 0.28 m rising or falling `0.05 + 0.1d` per step.
    pub fn generate(course: Course, seed: u64, level: f64) -> Self {
        let d = level.clamp(0.0, 1.0);
        let mut rng = Rng(seed.wrapping_mul(0x2545F4914F6CDD1D).wrapping_add(0x1234));
        let mut patches = vec![(Self::START.0, Self::START.1, 0.0)];
        let (mut x, mut y) = (Self::START.1, 0.0);
        for _ in 0..Self::STONES {
            let (gap, length, height) = match course {
                Course::Flat => (rng.range(0.3, 0.3 + 0.4 * d), rng.range(0.13, 0.3), 0.0),
                Course::Varying => (rng.range(0.3, 0.3 + 0.4 * d), rng.range(0.13, 0.3), rng.range(-0.2 * d, 0.2 * d)),
                Course::StairsUp => (0.02, 0.28, y + 0.05 + 0.1 * d),
                Course::StairsDown => (0.02, 0.28, y - 0.05 - 0.1 * d),
            };
            x += gap;
            patches.push((x, x + length, height));
            x += length;
            y = height;
        }
        let gap = match course {
            Course::Flat | Course::Varying => 0.3,
            _ => 0.02,
        };
        patches.push((x + gap, x + gap + Self::END_LENGTH, y));
        Self { patches }
    }

    /// Ground under `x`, if any.
    pub fn height_at(&self, x: f64) -> Option<f64> {
        self.patches.iter().filter(|(x0, x1, _)| x >= *x0 && x <= *x1).map(|p| p.2).fold(None, |m, y| Some(m.map_or(y, |m: f64| m.max(y))))
    }

    /// Footholds: a stone's centre; on a platform, strides of `stride`
    /// ending a foot's length before its edge.
    pub fn targets(&self, stride: f64) -> Vec<(f64, f64)> {
        let mut out = Vec::new();
        for (x0, x1, y) in &self.patches {
            if x1 - x0 <= 0.45 {
                out.push((0.5 * (x0 + x1), *y));
            } else {
                let last = x1 - 0.08;
                let mut x = x0 + 0.12;
                while x < last - 0.5 * stride {
                    out.push((x, *y));
                    x += stride;
                }
                out.push((last, *y));
            }
        }
        out
    }

    pub fn end(&self) -> f64 {
        self.patches.last().map(|p| p.0).unwrap_or(0.0)
    }

    pub fn max_gap(&self) -> f64 {
        self.patches.windows(2).map(|w| w[1].0 - w[0].1).fold(0.0, f64::max)
    }
}

// -------------------------------------------------------------------- biped

#[derive(Clone, Copy, Debug)]
pub struct Biped {
    pub torso_mass: f64,
    pub torso_inertia: f64,
    /// Hip below the torso's centre of mass (body frame).
    pub hip_y: f64,
    pub thigh: (f64, f64),
    pub shank: (f64, f64),
    pub friction: f64,
    pub gravity: f64,
    /// The PD loop's period (the seam's sample) and the policy's.
    pub pd_period: f64,
    pub policy_period: f64,
    pub torque_limit: f64,
    pub servo_bandwidth: f64,
    pub kp: f64,
    pub kd: f64,
    /// Hip height the planner keeps above the stance foot.
    pub hip_height: f64,
    pub step_height: f64,
    /// Torso pitch regulation through the stance hip: `(gain, rate gain)`.
    pub torso_gain: (f64, f64),
    pub step_time: (f64, f64),
    /// Stride on a platform, and how far past the foothold the capture
    /// point is let run before touchdown (the next step's speed).
    pub stride: f64,
    pub margin: f64,
    /// The PD loop lags a swing target that moves backwards through the
    /// body frame by about this long: the foot is aimed short by `lead·ẋ`.
    pub lead: f64,
}

impl Default for Biped {
    fn default() -> Self {
        Self {
            torso_mass: 30.0,
            torso_inertia: 2.0,
            hip_y: -0.1,
            thigh: (0.4, 3.0),
            shank: (0.4, 1.5),
            friction: 0.9,
            gravity: 9.81,
            pd_period: 2.0e-3,
            policy_period: 0.02,
            torque_limit: 250.0,
            servo_bandwidth: 100.0,
            kp: 1200.0,
            kd: 30.0,
            hip_height: 0.68,
            step_height: 0.10,
            torso_gain: (300.0, 30.0),
            step_time: (0.25, 0.9),
            stride: 0.35,
            margin: 0.10,
            lead: 0.03,
        }
    }
}

const LEGS: [&str; 2] = ["l", "r"];
const SENSES: [[&str; 4]; 2] = [
    ["sense.l.hip.angle", "sense.l.hip.speed", "sense.l.knee.angle", "sense.l.knee.speed"],
    ["sense.r.hip.angle", "sense.r.hip.speed", "sense.r.knee.angle", "sense.r.knee.speed"],
];
const ACTS: [[&str; 2]; 2] = [["act.l.hip.torque", "act.l.knee.torque"], ["act.r.hip.torque", "act.r.knee.torque"]];

/// Parameter names are `&'static str`; the terrain's are minted per patch
/// index and kept for the process.
fn intern(name: String) -> &'static str {
    static NAMES: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    let mut names = NAMES.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(n) = names.iter().find(|n| **n == name) {
        return n;
    }
    let leaked: &'static str = Box::leak(name.into_boxed_str());
    names.push(leaked);
    leaked
}

/// Two-link inverse kinematics in the body frame: foot at `(x, y)` from the
/// hip, knee bent backwards.
pub fn inverse(x: f64, y: f64, l1: f64, l2: f64) -> (f64, f64) {
    let r2 = x * x + y * y;
    let c = ((r2 - l1 * l1 - l2 * l2) / (2.0 * l1 * l2)).clamp(-1.0, 1.0);
    let knee = -c.acos();
    let hip = y.atan2(x) - (l2 * knee.sin()).atan2(l1 + l2 * knee.cos());
    (hip, knee)
}

pub struct Plant {
    pub runtime: Runtime,
    pub seam: BehaviorId,
    /// x, y, θ, vx, vy, ω of the torso.
    pub torso: [StateId; 6],
    /// Per leg: hip angle, knee angle (node across values).
    pub joints: [[StateId; 2]; 2],
    pub joint_speeds: [[StateId; 2]; 2],
    pub feet: [[StateId; 2]; 2],
    /// Per leg: the tip's vertical contact force.
    pub foot_force: [StateId; 2],
    /// The PD loop's joint targets, read at every seam sample.
    pub targets: Arc<Mutex<[f64; 4]>>,
    pub terrain: Terrain,
}

impl Biped {
    /// Hip and knee angles standing with the foot `dx` ahead of the hip.
    pub fn standing(&self, dx: f64) -> (f64, f64) {
        inverse(dx, -self.hip_height, self.thigh.0, self.shank.0)
    }

    /// The biped on `terrain`, feet `±stance` about the start platform's
    /// `x`, the PD loop attached through the seam.
    pub fn model(&self, registry: &BehaviorRegistry, terrain: &Terrain, x: f64, stance: f64) -> Plant {
        let mut m = ModelWorld::default();
        let ground = terrain.height_at(x).unwrap_or(0.0);
        let body = m
            .part(registry, "torso", ct::PLANAR_RIGID_BODY, [
                ("mass", self.torso_mass), ("inertia", self.torso_inertia), ("gravity", self.gravity),
                ("initial.x", x), ("initial.y", ground + self.hip_height - self.hip_y - 0.001),
            ])
            .unwrap();
        let mut seam_params: Vec<(&'static str, f64)> = vec![("period", self.pd_period)];
        for k in 0..2 {
            seam_params.extend(SENSES[k].iter().map(|n| (*n, 0.0)));
            seam_params.extend(ACTS[k].iter().map(|n| (*n, 0.0)));
        }
        let seam = m.part(registry, "controller", EXTERNAL, seam_params).unwrap();
        let mut body_ports = vec![body.port("frame")];
        let mut joints = Vec::new();
        let mut speeds = Vec::new();
        let mut feet = Vec::new();
        let mut forces = Vec::new();
        for (k, leg) in LEGS.iter().enumerate() {
            let dx = if k == 0 { -stance } else { stance };
            let (hip0, knee0) = self.standing(dx);
            let chain = m
                .part(registry, leg, CHAIN, [
                    ("gravity", self.gravity), ("ax", 0.0), ("ay", self.hip_y),
                    ("joint.hip", 0.0), ("joint.knee", 1.0),
                    ("link0.length", self.thigh.0), ("link0.mass", self.thigh.1 * std::env::var("SIM_PLANK_LEGMASS").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0)),
                    ("link1.length", self.shank.0), ("link1.mass", self.shank.1 * std::env::var("SIM_PLANK_LEGMASS").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0)),
                    ("initial.joint.hip.angle", hip0), ("initial.joint.knee.angle", knee0),
                ])
                .unwrap();
            body_ports.push(chain.port("base"));
            // Two point feet a hand apart must out-stiffen gravity's tipping
            // moment on a 40 kg body: the contacts are ten times a paw's.
            let knob = |name: &str, d: f64| std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
            let mut contact: Vec<(&'static str, f64)> = vec![("friction", self.friction), ("stiffness", knob("SIM_PLANK_STIFF", 2.0e5)), ("damping", knob("SIM_PLANK_DAMP", 2.0e3)), ("regularisation", knob("SIM_PLANK_REG", 1.0e-3)), ("edge", 0.01), ("patches", terrain.patches.len() as f64)];
            for (i, (x0, x1, y)) in terrain.patches.iter().enumerate() {
                contact.push((intern(format!("patch{i}.x0")), *x0));
                contact.push((intern(format!("patch{i}.x1")), *x1));
                contact.push((intern(format!("patch{i}.y")), *y));
            }
            let foot = m.part(registry, &format!("{leg}.foot"), ct::POINT_TERRAIN_COMPLIANT, contact).unwrap();
            m.connect([chain.port("tip"), foot.port("frame")]);
            let mut pair = Vec::new();
            for (j, joint) in ["hip", "knee"].iter().enumerate() {
                let servo = m.part(registry, &format!("{leg}.{joint}.servo"), sense::SERVO, [("bandwidth", self.servo_bandwidth), ("torque_limit", self.torque_limit)]).unwrap();
                let encoder = m.part(registry, &format!("{leg}.{joint}.encoder"), sense::ENCODER, []).unwrap();
                let tacho = m.part(registry, &format!("{leg}.{joint}.tacho"), sense::TACHOMETER, []).unwrap();
                let port = if j == 0 { "joint.hip" } else { "joint.knee" };
                m.connect([chain.port(port), servo.port("shaft"), encoder.port("shaft"), tacho.port("shaft")]);
                m.connect([encoder.port("angle"), seam.port(SENSES[k][2 * j])]);
                m.connect([tacho.port("speed"), seam.port(SENSES[k][2 * j + 1])]);
                m.connect([seam.port(ACTS[k][j]), servo.port("command")]);
                m.connect([servo.port("current")]);
                pair.push(chain.port(port));
            }
            joints.push([pair[0], pair[1]]);
            speeds.push([(chain.behavior, "hip.speed"), (chain.behavior, "knee.speed")]);
            feet.push((chain.behavior, "tip.x", "tip.y"));
            forces.push((chain.behavior, "tip.fy"));
        }
        m.connect(body_ports);
        let mut runtime = damped_runtime(m, registry);
        let torso = ["x", "y", "theta", "vx", "vy", "omega"].map(|n| runtime.state_id(body.behavior, n));
        let joints = [0, 1].map(|k| [runtime.across_id(joints[k][0]), runtime.across_id(joints[k][1])]);
        let joint_speeds = [0, 1].map(|k| [runtime.state_id(speeds[k][0].0, speeds[k][0].1), runtime.state_id(speeds[k][1].0, speeds[k][1].1)]);
        let feet = [0, 1].map(|k| [runtime.state_id(feet[k].0, feet[k].1), runtime.state_id(feet[k].0, feet[k].2)]);
        let foot_force = [0, 1].map(|k| runtime.state_id(forces[k].0, forces[k].1));
        // The PD loop on the seam: joint targets held by the policy.
        let (hl, kl) = self.standing(-stance);
        let (hr, kr) = self.standing(stance);
        let targets = Arc::new(Mutex::new([hl, kl, hr, kr]));
        let contract = runtime.contract(seam.behavior);
        // The contract names channels without their `sense.`/`act.` family.
        let index = |names: &[sim_core::Channel], name: &str| {
            let bare = name.trim_start_matches("sense.").trim_start_matches("act.");
            names.iter().position(|c| c.name == bare).unwrap_or_else(|| panic!("seam has no channel `{bare}`; it has {:?}", names.iter().map(|c| c.name.as_str()).collect::<Vec<_>>()))
        };
        let angle = [0, 1].map(|k| [index(&contract.sensors, SENSES[k][0]), index(&contract.sensors, SENSES[k][2])]);
        let speed = [0, 1].map(|k| [index(&contract.sensors, SENSES[k][1]), index(&contract.sensors, SENSES[k][3])]);
        let torque = [0, 1].map(|k| [index(&contract.actuators, ACTS[k][0]), index(&contract.actuators, ACTS[k][1])]);
        let (kp, kd, limit) = (self.kp, self.kd, self.torque_limit);
        let held = targets.clone();
        runtime
            .attach(
                seam.behavior,
                Box::new(FnCoupler(move |_t: f64, s: &[f64], a: &mut [f64]| {
                    let target = *held.lock().unwrap_or_else(|p| p.into_inner());
                    for k in 0..2 {
                        for j in 0..2 {
                            let q = target[2 * k + j];
                            a[torque[k][j]] = (kp * (q - s[angle[k][j]]) - kd * s[speed[k][j]]).clamp(-limit, limit);
                        }
                    }
                })),
            )
            .expect("seam");
        Plant { runtime, seam: seam.behavior, torso, joints, joint_speeds, feet, foot_force, targets, terrain: terrain.clone() }
    }
}

// ------------------------------------------------------------------ planner

/// What the planner and the reward read from the plant.
#[derive(Clone, Copy, Debug, Default)]
pub struct Body {
    pub t: f64,
    pub torso: [f64; 6],
    pub joints: [[f64; 2]; 2],
    pub joint_speeds: [[f64; 2]; 2],
    pub feet: [[f64; 2]; 2],
    pub foot_force: [f64; 2],
}

impl Plant {
    pub fn body(&self) -> Body {
        let rt = &self.runtime;
        Body {
            t: rt.time,
            torso: self.torso.map(|id| rt.get(id)),
            joints: self.joints.map(|j| j.map(|id| rt.get(id))),
            joint_speeds: self.joint_speeds.map(|j| j.map(|id| rt.get(id))),
            feet: self.feet.map(|f| f.map(|id| rt.get(id))),
            foot_force: self.foot_force.map(|id| rt.get(id)),
        }
    }
}

/// The LIP stepping planner: from the stance foot and the torso's state it
/// picks the next foothold (the next stone centre, or a stride ahead on a
/// platform), the step duration the LIP needs to carry the centre of mass
/// half-way to it, and swing-foot and stance-leg targets for the PD loop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Planner {
    pub stance: usize,
    pub step_start: f64,
    pub duration: f64,
    pub lift: [f64; 2],
    pub target: [f64; 2],
    /// LIP coefficients of the step's reference: `p_ref(τ) = a·cosh(ωτ) + b·sinh(ωτ)`.
    pub a: f64,
    pub b: f64,
    pub steps: usize,
    /// Perception error: stones appear this much nearer than they are.
    pub perception_offset: f64,
}

impl Planner {
    pub fn omega(biped: &Biped) -> f64 {
        (biped.gravity / biped.hip_height).sqrt()
    }

    /// Start the first step with the trailing foot as stance.
    pub fn start(biped: &Biped, body: &Body, terrain: &Terrain, perception_offset: f64) -> Self {
        let stance = if body.feet[0][0] <= body.feet[1][0] { 0 } else { 1 };
        let mut planner = Self { stance, step_start: body.t, duration: 0.4, lift: body.feet[1 - stance], target: body.feet[1 - stance], a: 0.0, b: 0.0, steps: 0, perception_offset };
        planner.begin(biped, body, terrain);
        planner
    }

    pub fn hip(biped: &Biped, torso: &[f64; 6]) -> [f64; 2] {
        let (c, s) = (torso[2].cos(), torso[2].sin());
        [torso[0] - s * biped.hip_y, torso[1] + c * biped.hip_y]
    }

    /// Plan the step that begins now.
    fn begin(&mut self, biped: &Biped, body: &Body, terrain: &Terrain) {
        let stance_x = body.feet[self.stance][0];
        let min_advance = 0.15;
        let seen: Vec<(f64, f64)> = terrain.targets(biped.stride).iter().map(|(x, y)| (x - self.perception_offset, *y)).collect();
        let target = seen.iter().copied().find(|(x, _)| *x > stance_x + min_advance).unwrap_or((stance_x + biped.stride, body.feet[self.stance][1]));
        self.lift = body.feet[1 - self.stance];
        self.target = [target.0, target.1];
        self.step_start = body.t;
        let omega = Self::omega(biped);
        let hip = Self::hip(biped, &body.torso);
        self.a = hip[0] - stance_x;
        self.b = body.torso[3] / omega;
        // Step timing from the divergent component of motion ξ = p + ṗ/ω,
        // which grows as ξ₀·e^{ωt}: touch down when ξ has passed the
        // foothold by `margin`, which is the speed the next step inherits.
        let l = target.0 - stance_x;
        let (lo, hi) = biped.step_time;
        let xi = self.a + self.b;
        self.duration = if xi > 1.0e-4 { (((l + biped.margin) / xi).ln() / omega).clamp(lo, hi) } else { hi };
        self.steps += 1;
        if std::env::var_os("SIM_PLANK_TRACE").is_some() {
            eprintln!("step {} at t={:.2}: stance {} at x={:.3}, hip {:.3} v={:.2}, p0={:.3} ξ0={:.3}, target x={:.3} (L={:.3}), T={:.2}", self.steps, body.t, self.stance, stance_x, hip[0], body.torso[3], self.a, xi, target.0, l, self.duration);
        }
    }

    /// Phase of the current step in `[0, 1]`.
    pub fn phase(&self, t: f64) -> f64 {
        ((t - self.step_start) / self.duration).clamp(0.0, 1.0)
    }

    /// The LIP reference `(p, ṗ)` relative to the stance foot at `t`.
    pub fn reference(&self, biped: &Biped, t: f64) -> (f64, f64) {
        let omega = Self::omega(biped);
        let tau = (t - self.step_start).max(0.0);
        let (ch, sh) = ((omega * tau).cosh(), (omega * tau).sinh());
        (self.a * ch + self.b * sh, omega * (self.a * sh + self.b * ch))
    }

    /// The measured LIP state `(p, ṗ)`: hip relative to the stance foot.
    pub fn measured(&self, biped: &Biped, body: &Body) -> (f64, f64) {
        let hip = Self::hip(biped, &body.torso);
        (hip[0] - body.feet[self.stance][0], body.torso[3])
    }

    /// Joint targets `[l.hip, l.knee, r.hip, r.knee]` for the PD loop.
    pub fn targets(&self, biped: &Biped, body: &Body) -> [f64; 4] {
        let s = self.phase(body.t);
        let hip = Self::hip(biped, &body.torso);
        let theta = body.torso[2];
        let (c, sn) = (theta.cos(), theta.sin());
        let to_body = |world: [f64; 2]| {
            let (dx, dy) = (world[0] - hip[0], world[1] - hip[1]);
            [c * dx + sn * dy, -sn * dx + c * dy]
        };
        let aim = self.target[0] - biped.lead * body.torso[3].max(0.0);
        let swing_world = [
            self.lift[0] + (aim - self.lift[0]) * (s - (std::f64::consts::TAU * s).sin() / std::f64::consts::TAU),
            self.lift[1] + (self.target[1] - self.lift[1]) * s + biped.step_height * (std::f64::consts::PI * s).sin(),
        ];
        let stance_foot = body.feet[self.stance];
        // The stance leg keeps the hip at its height above the foot; the
        // hip target carries the torso's pitch regulation.
        let stance_rel = to_body([stance_foot[0], hip[1] - biped.hip_height]);
        let swing_rel = to_body(swing_world);
        let (sh, sk) = inverse(stance_rel[0], stance_rel[1], biped.thigh.0, biped.shank.0);
        let (wh, wk) = inverse(swing_rel[0], swing_rel[1], biped.thigh.0, biped.shank.0);
        let pitch = (biped.torso_gain.0 * theta + biped.torso_gain.1 * body.torso[5]) / biped.kp;
        let mut out = [0.0; 4];
        out[2 * self.stance] = sh + pitch;
        out[2 * self.stance + 1] = sk;
        out[2 * (1 - self.stance)] = wh;
        out[2 * (1 - self.stance) + 1] = wk;
        out
    }

    /// Advance the plan: switch stance at touchdown or at the end of the
    /// step; otherwise re-time the step from the measured capture point,
    /// so losses at the last touchdown shorten or stretch this step
    /// instead of surprising the next.
    pub fn update(&mut self, biped: &Biped, body: &Body, terrain: &Terrain) -> bool {
        let s = self.phase(body.t);
        let landed = s >= 0.6 && body.foot_force[1 - self.stance] > 0.2 * biped.torso_mass * biped.gravity;
        if s >= 1.0 || landed {
            self.stance = 1 - self.stance;
            self.begin(biped, body, terrain);
            return true;
        }
        let omega = Self::omega(biped);
        let (p, v) = self.measured(biped, body);
        let xi = p + v / omega;
        let l = self.target[0] - body.feet[self.stance][0];
        let elapsed = body.t - self.step_start;
        let (lo, hi) = biped.step_time;
        if xi > 1.0e-4 {
            let remaining = ((l + biped.margin) / xi).ln() / omega;
            self.duration = (elapsed + remaining.max(0.0)).clamp(lo, hi).max(elapsed + 0.02);
        }
        false
    }

    pub fn as_vec(&self) -> Vec<f64> {
        vec![self.stance as f64, self.step_start, self.duration, self.lift[0], self.lift[1], self.target[0], self.target[1], self.a, self.b, self.steps as f64, self.perception_offset]
    }

    pub fn from_slice(v: &[f64]) -> Option<Self> {
        if v.len() < 11 {
            return None;
        }
        Some(Self { stance: v[0] as usize, step_start: v[1], duration: v[2], lift: [v[3], v[4]], target: [v[5], v[6]], a: v[7], b: v[8], steps: v[9] as usize, perception_offset: v[10] })
    }
}

/// The CLF on the LIP error `e = (p − p_ref, (ṗ − ṗ_ref)/ω)`: `V = eᵀe`.
pub fn lyapunov(biped: &Biped, measured: (f64, f64), reference: (f64, f64)) -> f64 {
    let omega = Planner::omega(biped);
    let (ep, ev) = (measured.0 - reference.0, (measured.1 - reference.1) / omega);
    ep * ep + ev * ev
}

// -------------------------------------------------------------- environment

/// The biped as a learner's environment: joint targets in, proprioception
/// and the next stones out, the planner's references and the LIP
/// coordinates as privileged channels.
pub struct PlankEnv {
    pub biped: Biped,
    pub course: Course,
    pub registry: BehaviorRegistry,
    pub plant: Option<Plant>,
    pub planner: Option<Planner>,
    pub seed: u64,
    pub level: f64,
    pub perception_offset: f64,
    pub done: bool,
    pub success: bool,
}

pub const OBS: [&str; 21] = [
    "l.hip.angle", "l.hip.speed", "l.knee.angle", "l.knee.speed",
    "r.hip.angle", "r.hip.speed", "r.knee.angle", "r.knee.speed",
    "torso.pitch", "torso.rate", "torso.vx", "torso.vy",
    "stone0.x0", "stone0.x1", "stone0.y", "stone1.x0", "stone1.x1", "stone1.y", "stone2.x0", "stone2.x1", "stone2.y",
];
pub const PRIV: [&str; 22] = [
    "torso.x", "torso.y", "l.foot.x", "l.foot.y", "r.foot.x", "r.foot.y", "l.foot.fy", "r.foot.fy",
    "stance", "phase", "ref.l.hip", "ref.l.knee", "ref.r.hip", "ref.r.knee",
    "ref.p", "ref.pdot", "lip.p", "lip.pdot", "clf", "steps", "success", "failed",
];
pub const ACT: [&str; 4] = ["l.hip", "l.knee", "r.hip", "r.knee"];

impl PlankEnv {
    pub fn new(biped: Biped, course: Course) -> Self {
        Self { biped, course, registry: registry(), plant: None, planner: None, seed: 0, level: 0.0, perception_offset: 0.0, done: false, success: false }
    }

    fn frame(&self, terrain: bool, failed: bool) -> Frame {
        let plant = self.plant.as_ref().expect("reset first");
        let planner = self.planner.as_ref().expect("reset first");
        let body = plant.body();
        let mut obs = Vec::with_capacity(OBS.len());
        for k in 0..2 {
            obs.extend([body.joints[k][0], body.joint_speeds[k][0], body.joints[k][1], body.joint_speeds[k][1]]);
        }
        obs.extend([body.torso[2], body.torso[5], body.torso[3], body.torso[4]]);
        let ahead: Vec<&(f64, f64, f64)> = plant.terrain.patches.iter().filter(|(_, x1, _)| *x1 > body.feet[planner.stance][0] + 0.05).take(3).collect();
        for k in 0..3 {
            match ahead.get(k) {
                Some((x0, x1, y)) => obs.extend([x0 - body.torso[0], x1 - body.torso[0], y - body.torso[1]]),
                None => obs.extend([5.0, 6.0, 0.0]),
            }
        }
        let targets = planner.targets(&self.biped, &body);
        let reference = planner.reference(&self.biped, body.t);
        let measured = planner.measured(&self.biped, &body);
        let privileged = vec![
            body.torso[0], body.torso[1], body.feet[0][0], body.feet[0][1], body.feet[1][0], body.feet[1][1], body.foot_force[0], body.foot_force[1],
            planner.stance as f64, planner.phase(body.t), targets[0], targets[1], targets[2], targets[3],
            reference.0, reference.1, measured.0, measured.1, lyapunov(&self.biped, measured, reference),
            planner.steps as f64, self.success as u8 as f64, failed as u8 as f64,
        ];
        Frame { obs, privileged, t: body.t, done: self.done, terrain: terrain.then(|| plant.terrain.patches.clone()) }
    }

    /// Fallen, through a gap, or across the end platform.
    fn judge(&mut self) {
        let plant = self.plant.as_ref().expect("reset first");
        let body = plant.body();
        let ground = plant.terrain.height_at(body.torso[0]).or_else(|| plant.terrain.height_at(body.feet[0][0])).or_else(|| plant.terrain.height_at(body.feet[1][0])).unwrap_or(f64::NEG_INFINITY);
        let lowest = plant.terrain.patches.iter().map(|p| p.2).fold(f64::INFINITY, f64::min);
        let fallen = body.torso[2].abs() > 0.9 || body.torso[1] - ground < 0.4 || body.feet.iter().any(|f| f[1] < lowest - 0.25) || body.torso[1] < lowest - 0.2;
        self.success = body.torso[0] > plant.terrain.end() + 0.35;
        self.done = fallen || self.success;
    }

    /// Holding the nominal posture with the pitch damped: what standing
    /// still amounts to without ankles.
    pub fn hold_action(&self) -> [f64; 4] {
        let plant = self.plant.as_ref().expect("reset first");
        let body = plant.body();
        let damping = self.biped.torso_gain.1 * body.torso[5] / self.biped.kp;
        let (hl, kl) = self.biped.standing(-0.08);
        let (hr, kr) = self.biped.standing(0.08);
        [hl + 0.5 * damping, kl, hr + 0.5 * damping, kr]
    }

    /// The planner's own joint targets: the model-based baseline.
    pub fn planner_action(&self) -> [f64; 4] {
        let plant = self.plant.as_ref().expect("reset first");
        self.planner.as_ref().expect("reset first").targets(&self.biped, &plant.body())
    }
}

impl Environment for PlankEnv {
    fn spaces(&self) -> Spaces {
        Spaces { period: self.biped.policy_period, obs: OBS.iter().map(|s| s.to_string()).collect(), privileged: PRIV.iter().map(|s| s.to_string()).collect(), act: ACT.iter().map(|s| s.to_string()).collect() }
    }

    fn reset(&mut self, seed: u64, level: f64) -> Result<Frame, String> {
        let terrain = Terrain::generate(self.course, seed, level);
        let plant = self.biped.model(&self.registry, &terrain, 0.0, 0.08);
        let body = plant.body();
        self.planner = Some(Planner::start(&self.biped, &body, &terrain, self.perception_offset));
        self.plant = Some(plant);
        self.seed = seed;
        self.level = level;
        self.done = false;
        self.success = false;
        Ok(self.frame(true, false))
    }

    fn step(&mut self, action: &[f64]) -> Result<Frame, String> {
        if action.len() != ACT.len() {
            return Err(format!("expected {} actions, got {}", ACT.len(), action.len()));
        }
        let biped = self.biped;
        let plant = self.plant.as_mut().ok_or("reset first")?;
        *plant.targets.lock().unwrap_or_else(|p| p.into_inner()) = [action[0], action[1], action[2], action[3]];
        let failed = plant.runtime.advance(biped.policy_period, biped.pd_period).is_err();
        let terrain = plant.terrain.clone();
        let body = plant.body();
        if let Some(planner) = self.planner.as_mut() {
            planner.update(&biped, &body, &terrain);
        }
        self.judge();
        if failed {
            self.done = true;
        }
        Ok(self.frame(false, failed))
    }

    fn snapshot(&self) -> Vec<f64> {
        let Some(plant) = self.plant.as_ref() else { return Vec::new() };
        let snap = plant.runtime.snapshot();
        let mut out = vec![self.seed as f64, self.level, self.done as u8 as f64, self.success as u8 as f64, snap.time];
        out.extend(self.planner.map(|p| p.as_vec()).unwrap_or_default());
        let targets = *plant.targets.lock().unwrap_or_else(|p| p.into_inner());
        out.extend(targets);
        out.push(snap.islands.len() as f64);
        for island in &snap.islands {
            out.push(island.time);
            out.push(island.state.len() as f64);
            out.extend(&island.state);
            out.extend(&island.previous_rate);
        }
        out
    }

    fn restore(&mut self, snapshot: &[f64]) -> Result<Frame, String> {
        if snapshot.len() < 5 {
            return Err("snapshot too short".into());
        }
        let (seed, level) = (snapshot[0] as u64, snapshot[1]);
        if self.plant.is_none() || seed != self.seed || level != self.level {
            self.reset(seed, level)?;
        }
        self.done = snapshot[2] != 0.0;
        self.success = snapshot[3] != 0.0;
        let time = snapshot[4];
        let mut at = 5;
        self.planner = Some(Planner::from_slice(&snapshot[at..]).ok_or("snapshot lacks the planner")?);
        at += 11;
        let plant = self.plant.as_mut().ok_or("reset first")?;
        *plant.targets.lock().unwrap_or_else(|p| p.into_inner()) = [snapshot[at], snapshot[at + 1], snapshot[at + 2], snapshot[at + 3]];
        at += 4;
        let count = snapshot[at] as usize;
        at += 1;
        let mut islands = Vec::with_capacity(count);
        for _ in 0..count {
            let t = snapshot[at];
            let n = snapshot[at + 1] as usize;
            at += 2;
            let state = snapshot.get(at..at + n).ok_or("snapshot truncated")?.to_vec();
            let previous_rate = snapshot.get(at + n..at + 2 * n).ok_or("snapshot truncated")?.to_vec();
            at += 2 * n;
            islands.push(sim_dynamics::Snapshot { time: t, state, previous_rate });
        }
        plant.runtime.restore(&RuntimeSnapshot { time, islands }).map_err(|e| e.to_string())?;
        Ok(self.frame(false, false))
    }
}

// -------------------------------------------------------------------- plate

/// Walk `env` with the planner's own targets for up to `steps` policy
/// periods; returns the frames' torso x, the CLF values, and whether it
/// reached the end.
pub fn planner_walk(env: &mut PlankEnv, seed: u64, level: f64, steps: usize) -> (Vec<f64>, Vec<f64>, Vec<f64>, bool) {
    env.reset(seed, level).expect("reset");
    let (mut time, mut x, mut clf) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..steps {
        let action = env.planner_action();
        let frame = env.step(&action).expect("step");
        time.push(frame.t);
        x.push(frame.privileged[0]);
        clf.push(frame.privileged[18]);
        if frame.done {
            break;
        }
    }
    (time, x, clf, env.success)
}

pub fn run() -> Report {
    let mut report = Report::new("walk-the-plank");
    let biped = Biped::default();

    // The curriculum knob.
    let easy = Terrain::generate(Course::Flat, 7, 0.0);
    let hard = Terrain::generate(Course::Flat, 7, 1.0);
    let varied = Terrain::generate(Course::Varying, 7, 1.0);
    report.measure("level 0: widest gap (m)", easy.max_gap());
    report.measure("level 1: widest gap (m)", hard.max_gap());
    report.measure("level 1, varying: height spread (m)", varied.patches.iter().map(|p| p.2).fold(f64::NEG_INFINITY, f64::max) - varied.patches.iter().map(|p| p.2).fold(f64::INFINITY, f64::min));
    report.close("level 0 keeps every gap at 0.3 m", easy.max_gap(), 0.3, 1.0e-9);
    report.holds("level 1 opens gaps past 0.5 m and never past 0.7 m", hard.max_gap() > 0.5 && hard.max_gap() <= 0.7 + 1.0e-9);
    report.holds("stairs climb one rise per stone", {
        let stairs = Terrain::generate(Course::StairsUp, 1, 0.5);
        stairs.patches.windows(2).skip(1).take(Terrain::STONES - 1).all(|w| (w[1].2 - w[0].2 - 0.1).abs() < 1.0e-9)
    });

    // Standing still. A point-foot biped has no ankle: holding a posture,
    // its weight lands on the feet within a few tenths of a second, and
    // then it topples like the inverted pendulum it is — the reason for a
    // stepping planner in the first place.
    let mut env = PlankEnv::new(biped, Course::Flat);
    let first = env.reset(1, 0.0).expect("reset");
    let weight = (biped.torso_mass + 2.0 * (biped.thigh.1 + biped.shank.1)) * biped.gravity;
    let mut carried = 0.0;
    let mut toppled_at = f64::NAN;
    for k in 0..100 {
        let action = env.hold_action();
        let frame = env.step(&action).expect("hold");
        if k == 14 {
            carried = frame.privileged[6] + frame.privileged[7];
        }
        if frame.done {
            toppled_at = frame.t;
            break;
        }
    }
    report.measure("holding still: weight on the feet at 0.3 s (N)", carried);
    report.measure("holding still: topples at (s)", toppled_at);
    report.within("holding still: the feet carry the biped's weight at 0.3 s", carried, weight, 0.05);
    report.holds("holding still on point feet topples within 2 s (no ankle: it must step)", toppled_at.is_finite() && toppled_at < 2.0);
    let _ = first;

    // The planner walks the level-0 course.
    let (time, x, clf, success) = planner_walk(&mut env, 3, 0.0, 600);
    let steps_taken = env.planner.map(|p| p.steps).unwrap_or(0);
    report.series("torso x along the level-0 course (m)", &time, &x, 300);
    report.series("CLF on the LIP error along the walk", &time, &clf, 300);
    report.measure("level 0: distance walked (m)", x.last().copied().unwrap_or(0.0));
    report.measure("level 0: steps taken", steps_taken as f64);
    report.measure("level 0: time to the end (s)", time.last().copied().unwrap_or(0.0));
    report.holds("the LIP planner walks the level-0 stones to the end platform", success);
    // The CLF the learner is rewarded on: small along a good walk, large
    // where a walk goes wrong.
    let mean_clf = clf.iter().sum::<f64>() / clf.len().max(1) as f64;
    report.measure("mean CLF along the level-0 walk (m²)", mean_clf);
    report.below("the LIP error stays small along the planner's walk (a usable teacher)", mean_clf, 0.05);

    // The curriculum: the planner alone, over eight seeds per level. Its
    // success falls with the level — the room a learner has to earn.
    let mut rates = Vec::new();
    for level in [0.0, 0.6] {
        let mut wins = 0;
        for seed in 1..=4 {
            let (_, _, _, ok) = planner_walk(&mut env, seed, level, 400);
            wins += ok as usize;
        }
        rates.push(wins as f64 / 4.0);
        report.measure(&format!("planner success rate at level {level}"), rates[rates.len() - 1]);
    }
    report.series("planner success rate vs level", &[0.0, 0.6], &rates, 2);
    report.close("level 0: the planner alone crosses every course", rates[0], 1.0, 1.0e-9);
    report.holds("harder courses defeat the planner alone (level 0.6 below level 0)", rates[1] < rates[0]);

    // Snapshot determinism: the environment resumes bit for bit.
    env.reset(3, 0.0).expect("reset");
    for _ in 0..20 {
        let action = env.planner_action();
        env.step(&action).expect("step");
    }
    let saved = env.snapshot();
    let mut first_run = Vec::new();
    for _ in 0..40 {
        let action = env.planner_action();
        first_run.push(env.step(&action).expect("step").privileged[0]);
    }
    env.restore(&saved).expect("restore");
    let mut second_run = Vec::new();
    for _ in 0..40 {
        let action = env.planner_action();
        second_run.push(env.step(&action).expect("step").privileged[0]);
    }
    let divergence = first_run.iter().zip(&second_run).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
    report.measure("snapshot: replay divergence in torso x (m)", divergence);
    report.below("a restored snapshot replays the same trajectory", divergence, 1.0e-9);

    // Falsifier: the planner trusts its perception. Stones reported 12 cm
    // nearer than they are put a foot in the gap.
    let mut misled = PlankEnv::new(biped, Course::Flat);
    misled.perception_offset = 0.12;
    let (_, x_misled, clf_misled, success_misled) = planner_walk(&mut misled, 3, 0.0, 600);
    report.measure("misled planner: distance before the fall (m)", x_misled.last().copied().unwrap_or(0.0));
    let peak_misled = clf_misled.iter().copied().fold(0.0, f64::max);
    report.measure("misled planner: peak CLF (m²)", peak_misled);
    report.holds("falsifier: a 12 cm perception error brings the planner down", !success_misled);
    // The planner re-times each step from the measured state, so the LIP
    // error stays modest even as a foot goes into the gap: the peak is a
    // few times a good walk's mean, not an order of magnitude.
    report.above("the CLF rises on the misled walk (its peak, twice the good walk's mean)", peak_misled, 2.0 * mean_clf);
    report
}
