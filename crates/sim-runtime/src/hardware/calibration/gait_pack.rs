//! Gait pack for the robot-link ESP32: the leg's gaits as the Rust player
//! would command them, precomputed for a host that cannot run the governor.
//!
//! Everything comes from the shared sources the calibration panel uses: the
//! gait catalog and `compiled_with_governor`, the leg's taught calibration
//! record, the accepted actuator registry ([`govern_leg`]), and the CAD scene
//! through the kinematic mirror. Nothing is re-derived here; this module only
//! samples those definitions and writes them down.
//!
//! The pack is one file the ESP32 stores in flash (`POST /api/gaits`):
//!
//! ```text
//! "RLGP" | u32 version=3 | u32 json_len | u32 bin_len | sha256(json ‖ bin) [32]
//! json   — everything the page draws (gaits with their frames as motor angles,
//!          plots, the leg figure: CAD meshes and per-link transform tables)
//! bin    — first every commandable motor's taught travel, which the ESP32 arms,
//!          jogs and teaches against without a gait:
//!   "AXES" | u8 count | u8 0 | u16 0
//!   count × { u8 id, u8 flags, u16 0, i32 travel_lo, i32 travel_hi, i32 turn_margin_counts }
//!          (flags: 1 multi-turn, 2 reversed: the motor's lower pose is its high-count end)
//!          then one plan per playable gait, at the offset its JSON entry names:
//!   "PLAN" | f32 period_s | u16 samples | u8 axes | u8 0
//!   axes ×    { u8 id, u8 drive, u8 flags, u8 0, i32 window_lo, i32 window_hi,
//!               f32 governor_period_s, f32 max_speed_counts_s,
//!               f32 max_acceleration_counts_s2, f32 response_rate_per_s,
//!               f32 hold_tolerance_counts, i32 turn_margin_counts }
//!   samples × axes × f32 desired_counts      (one gait cycle, evenly spaced)
//!   u16 check_steps | u16 0 | f32 check_dt_s
//!   check_steps × axes × f32 governed_counts (the Rust governor's output)
//! ```
//!
//! All integers and floats are little-endian. A plan is the gait's desired
//! reference for the motors the ESP32 can command, already mapped to encoder
//! counts by each motor's binding and clamped inside its taught window (six
//! counts in, as on the panel), plus each motor's reference governor (the
//! gait's own, tightened to the registry limits by [`govern_leg`]). The ESP32
//! runs that governor live, as [`GovernedGait`] does on the panel; a stored
//! cycle of governed output would not repeat seamlessly (a saturated governor
//! need not settle on the gait's period). The governor law is linear in the
//! angle, so it runs in counts with speed and acceleration scaled by the
//! binding. The check sequence is [`GovernedGait`] stepped from the first
//! desired value at rest: the ESP32's tests replay it to prove the C port of
//! the law matches. `window_lo/hi` are the taught poses the ESP32 sends to
//! the FPGA.
//!
//! `drive` is the panel's [`DriveMode`] register value: 0 servo position for
//! a single-turn motor, 1 servo speed for a multi-turn one (flags bit 0),
//! whose travel is longer than the encoder's one turn so position mode
//! cannot reach it. A multi-turn motor's poses are encoder counts plus whole
//! turns, as `EncoderTurns` counted them while they were taught: every
//! reading is congruent to its pose modulo 4096, so they hold across
//! sessions once the motor's current turn is known. The ESP32 learns that
//! turn from the operator, who picks which of the places the reading allows
//! (inside the taught travel widened by `turn_margin_counts`) matches the
//! real leg, and forgets it whenever a reading is missed.
use super::gait::{gait_misfit, gait_with_governor, govern_leg, taught_window};
use super::{Config, repo_root};
use crate::acquisition::calibration::{AxisCalibration, Calibration};
use crate::acquisition::calibration_sweep::DriveMode;
use crate::gait_playback::{Gait, GovernedGait, LegBinding};
use crate::kinematic_mirror::KinematicMirror;
use crate::session::Scene;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

type R<T> = Result<T, String>;
const RAD: f64 = std::f64::consts::TAU / 4096.;
pub const PACK_MAGIC: &[u8; 4] = b"RLGP";
pub const PLAN_MAGIC: &[u8; 4] = b"PLAN";
pub const PACK_VERSION: u32 = 3;
pub const AXES_MAGIC: &[u8; 4] = b"AXES";
/// Magic, version, two lengths and the SHA-256: the JSON starts here.
pub const PACK_HEADER: usize = 48;
/// The scene the leg mirror (calibration-mirror) is built from.
pub const LEG_SCENE: &str = "examples/full-robot/measured-actuator-integration/browser-control-400hz/scene.json";
/// Desired-reference samples per second stored in a plan.
const DESIRED_HZ: f64 = 200.;
/// The governor check: steps and their period.
const CHECK_STEPS: usize = 150;
const CHECK_DT: f64 = 0.02;
/// How far past its taught poses a multi-turn motor may be when the operator
/// confirms its turn: the places a reading allows lie in the travel widened by this.
pub const TURN_MARGIN_COUNTS: i32 = 256;
/// Plan flag: the motor's poses span more than one encoder turn.
pub const FLAG_MULTI_TURN: u8 = 1;
/// Axes-table flag: the calibration's lower pose is the high-count end.
pub const FLAG_REVERSED: u8 = 2;
/// Links of the leg being drawn share this prefix.
const LEG_PREFIX: &str = "+X |";

#[derive(Clone, Debug)]
pub struct PackOptions {
    /// Fraction of each motor's measured capability (as the panel's effort).
    pub effort: f64,
    /// Samples per second of the stored plan and preview.
    pub sample_hz: f64,
    /// Supply voltage the registry limits are evaluated at.
    pub supply_v: f64,
    /// Repository paths of compiled gaits; empty means the catalog's first `max_gaits`.
    pub gaits: Vec<String>,
    pub max_gaits: usize,
    /// A gait-run record whose `bindings` give each motor's CAD joint and
    /// polarity; None means the newest run in the leg's output.
    pub bindings_from: Option<PathBuf>,

}
impl Default for PackOptions {
    fn default() -> Self {
        Self { effort: 0.5, sample_hz: 25., supply_v: 11.1, gaits: vec![], max_gaits: 12, bindings_from: None }
    }
}

pub struct GaitPack {
    pub json: Value,
    pub binary: Vec<u8>,
}
impl GaitPack {
    /// The file the ESP32 stores: header, JSON, plans.
    pub fn to_bytes(&self) -> Vec<u8> {
        let json = serde_json::to_vec(&self.json).expect("pack JSON serializes");
        let mut digest = Sha256::new();
        digest.update(&json);
        digest.update(&self.binary);
        let mut out = Vec::with_capacity(PACK_HEADER + json.len() + self.binary.len());
        out.extend_from_slice(PACK_MAGIC);
        out.extend_from_slice(&PACK_VERSION.to_le_bytes());
        out.extend_from_slice(&(json.len() as u32).to_le_bytes());
        out.extend_from_slice(&(self.binary.len() as u32).to_le_bytes());
        out.extend_from_slice(&digest.finalize());
        out.extend_from_slice(&json);
        out.extend_from_slice(&self.binary);
        out
    }
}

/// One stored plan, decoded (for tests and for checking a pack read back).
#[derive(Debug, PartialEq)]
pub struct Plan {
    pub period_s: f32,
    pub axes: Vec<PlanAxis>,
    /// samples × axes of desired counts over one cycle.
    pub desired: Vec<Vec<f32>>,
    pub check_dt_s: f32,
    /// check steps × axes of governed counts.
    pub check: Vec<Vec<f32>>,
}
#[derive(Debug, PartialEq)]
pub struct PlanAxis {
    pub id: u8,
    pub window: (i32, i32),
    pub governor_period_s: f32,
    pub max_speed_counts_s: f32,
    pub max_acceleration_counts_s2: f32,
    pub response_rate_per_s: f32,
    /// [`DriveMode::register`]: 0 servo position, 1 servo speed.
    pub drive: u8,
    pub flags: u8,
    /// The panel's hold deadband: a speed-mode motor this close to a resting goal is not driven.
    pub hold_tolerance_counts: f32,
    pub turn_margin_counts: i32,
}
const AXIS_BYTES: usize = 36;
impl Plan {
    pub fn encode(&self) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(PLAN_MAGIC);
        b.extend_from_slice(&self.period_s.to_le_bytes());
        b.extend_from_slice(&(self.desired.len() as u16).to_le_bytes());
        b.push(self.axes.len() as u8);
        b.push(0);
        for a in &self.axes {
            b.extend_from_slice(&[a.id, a.drive, a.flags, 0]);
            b.extend_from_slice(&a.window.0.to_le_bytes());
            b.extend_from_slice(&a.window.1.to_le_bytes());
            for v in [a.governor_period_s, a.max_speed_counts_s, a.max_acceleration_counts_s2, a.response_rate_per_s, a.hold_tolerance_counts] {
                b.extend_from_slice(&v.to_le_bytes());
            }
            b.extend_from_slice(&a.turn_margin_counts.to_le_bytes());
        }
        for row in &self.desired {
            for v in row {
                b.extend_from_slice(&v.to_le_bytes());
            }
        }
        b.extend_from_slice(&(self.check.len() as u16).to_le_bytes());
        b.extend_from_slice(&[0, 0]);
        b.extend_from_slice(&self.check_dt_s.to_le_bytes());
        for row in &self.check {
            for v in row {
                b.extend_from_slice(&v.to_le_bytes());
            }
        }
        b
    }
    pub fn decode(b: &[u8]) -> R<Self> {
        let get = |at: usize, n: usize| b.get(at..at + n).ok_or_else(|| "plan is truncated".to_string());
        if get(0, 4)? != PLAN_MAGIC {
            return Err("not a plan".into());
        }
        let f32_at = |at: usize| -> R<f32> { Ok(f32::from_le_bytes(get(at, 4)?.try_into().unwrap())) };
        let i32_at = |at: usize| -> R<i32> { Ok(i32::from_le_bytes(get(at, 4)?.try_into().unwrap())) };
        let u16_at = |at: usize| -> R<usize> { Ok(u16::from_le_bytes(get(at, 2)?.try_into().unwrap()) as usize) };
        let period_s = f32_at(4)?;
        let n = u16_at(8)?;
        let k = get(10, 1)?[0] as usize;
        let mut axes = Vec::new();
        for a in 0..k {
            let at = 12 + AXIS_BYTES * a;
            axes.push(PlanAxis { id: get(at, 1)?[0], window: (i32_at(at + 4)?, i32_at(at + 8)?), governor_period_s: f32_at(at + 12)?,
                max_speed_counts_s: f32_at(at + 16)?, max_acceleration_counts_s2: f32_at(at + 20)?, response_rate_per_s: f32_at(at + 24)?,
                drive: get(at + 1, 1)?[0], flags: get(at + 2, 1)?[0], hold_tolerance_counts: f32_at(at + 28)?, turn_margin_counts: i32_at(at + 32)? });
        }
        let mut at = 12 + AXIS_BYTES * k;
        let rows = |at: &mut usize, count: usize| -> R<Vec<Vec<f32>>> {
            let mut out = Vec::new();
            for _ in 0..count {
                out.push((0..k).map(|a| f32_at(*at + 4 * a)).collect::<R<Vec<_>>>()?);
                *at += 4 * k;
            }
            Ok(out)
        };
        let desired = rows(&mut at, n)?;
        let steps = u16_at(at)?;
        let check_dt_s = f32_at(at + 4)?;
        at += 8;
        let check = rows(&mut at, steps)?;
        if b.len() != at {
            return Err("plan length disagrees with its header".into());
        }
        Ok(Self { period_s, axes, desired, check_dt_s, check })
    }
}

/// Split a pack file into its JSON and plans, checking the header and digest.
pub fn read_pack(bytes: &[u8]) -> R<(Value, Vec<u8>)> {
    if bytes.len() < PACK_HEADER || &bytes[0..4] != PACK_MAGIC {
        return Err("not a robot-link gait pack".into());
    }
    let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    if word(4) as u32 != PACK_VERSION {
        return Err(format!("pack version {} is not {PACK_VERSION}", word(4)));
    }
    let (j, b) = (word(8), word(12));
    if bytes.len() != PACK_HEADER + j + b {
        return Err("pack length disagrees with its header".into());
    }
    let mut digest = Sha256::new();
    digest.update(&bytes[PACK_HEADER..]);
    if digest.finalize().as_slice() != &bytes[16..PACK_HEADER] {
        return Err("pack digest does not match".into());
    }
    let json = serde_json::from_slice(&bytes[PACK_HEADER..PACK_HEADER + j]).map_err(|e| format!("pack JSON: {e}"))?;
    Ok((json, bytes[PACK_HEADER + j..].to_vec()))
}

fn sha256_file(path: &Path) -> R<String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect())
}
fn round(v: f64, digits: i32) -> f64 {
    let k = 10f64.powi(digits);
    (v * k).round() / k
}
fn rel(path: &Path) -> String {
    path.strip_prefix(repo_root()).unwrap_or(path).display().to_string()
}

/// The newest gait run in the leg's output (its bindings are the leg mirror's).
fn newest_gait_run(cfg: &Config) -> R<PathBuf> {
    let dir = cfg.output.join("gait-runs");
    let mut runs: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    runs.sort();
    runs.pop().ok_or(format!("{}: no gait runs to take the leg's joint bindings from; pass a bindings file", dir.display()))
}

/// The leg as drawn: every link of the leg as its CAD collision mesh (vertex
/// clustered to `FIGURE_CELL_M`), placed by the kinematic mirror. Each link's
/// pose relative to its tree parent depends on one motor at most (hip
/// output on the hip servo, thigh and worm on the worm servo, crank, curved
/// link and crosshead on the foot servo), so the pack stores, per link, that
/// relative transform sampled over its motor's range, and the page composes
/// the chain for any combination of motor angles. `figure_check` measures what
/// composing loses against solving the mirror directly.
struct Figure {
    json: Value,
}
/// The figure covers each motor's taught travel widened by this fraction of
/// it, or by FIGURE_MARGIN_COUNTS if more.
const FIGURE_MARGIN: f64 = 0.15;
const FIGURE_MARGIN_COUNTS: f64 = 300.;
/// Mesh simplification cell, m.
const FIGURE_CELL_M: f64 = 0.003;
/// Samples of each motor's range in a link's table.
const FIGURE_SAMPLES: usize = 81;
/// The composed figure must match a direct mirror solve this well (mm, any vertex).
const FIGURE_TOLERANCE_MM: f64 = 2.0;

/// Row-major 3×4 [R | t].
type Rt = [f64; 12];
fn rt_of(p: &crate::session::LinkPose) -> Rt {
    let r = &p.rotation;
    [r[0][0], r[0][1], r[0][2], p.position_m[0], r[1][0], r[1][1], r[1][2], p.position_m[1], r[2][0], r[2][1], r[2][2], p.position_m[2]]
}
fn rt_mul(a: &Rt, b: &Rt) -> Rt {
    let mut o = [0.; 12];
    for i in 0..3 {
        for j in 0..3 {
            o[4 * i + j] = (0..3).map(|k| a[4 * i + k] * b[4 * k + j]).sum();
        }
        o[4 * i + 3] = (0..3).map(|k| a[4 * i + k] * b[4 * k + 3]).sum::<f64>() + a[4 * i + 3];
    }
    o
}
fn rt_inv(a: &Rt) -> Rt {
    let mut o = [0.; 12];
    for i in 0..3 {
        for j in 0..3 {
            o[4 * i + j] = a[4 * j + i];
        }
    }
    for i in 0..3 {
        o[4 * i + 3] = -(0..3).map(|k| o[4 * i + k] * a[4 * k + 3]).sum::<f64>();
    }
    o
}
fn rt_apply(a: &Rt, p: &[f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| (0..3).map(|k| a[4 * i + k] * p[k]).sum::<f64>() + a[4 * i + 3])
}
fn rt_lerp(a: &Rt, b: &Rt, f: f64) -> Rt {
    std::array::from_fn(|k| a[k] + (b[k] - a[k]) * f)
}
/// Vertex clustering: vertices snapped to a `cell` grid and merged (their mean
/// kept), triangles that collapse or repeat dropped.
fn simplify(vertices: &[[f64; 3]], triangles: &[[usize; 3]], cell: f64) -> (Vec<[f64; 3]>, Vec<[usize; 3]>) {
    let mut index = std::collections::HashMap::new();
    let mut sums: Vec<([f64; 3], f64)> = Vec::new();
    let map: Vec<usize> = vertices.iter().map(|v| {
        let key = v.map(|c| (c / cell).round() as i64);
        *index.entry(key).or_insert_with(|| { sums.push(([0.; 3], 0.)); sums.len() - 1 })
    }).collect();
    for (v, &m) in vertices.iter().zip(&map) {
        for k in 0..3 {
            sums[m].0[k] += v[k];
        }
        sums[m].1 += 1.;
    }
    let out_v: Vec<[f64; 3]> = sums.iter().map(|(s, n)| s.map(|c| c / n)).collect();
    let mut seen = std::collections::HashSet::new();
    let out_t = triangles.iter().filter_map(|t| {
        let t = t.map(|i| map[i]);
        if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
            return None;
        }
        let mut key = t;
        key.sort();
        seen.insert(key).then_some(t)
    }).collect();
    (out_v, out_t)
}

/// Build the figure: the leg's links, each one's tree parent and driving
/// motor, its sampled relative transform, and the composition check.
fn build_figure(scene: &Scene, mirror: &mut KinematicMirror, home: &[f64], motors: &[(u8, usize, f64, f64)], taught: &[(f64, f64)]) -> R<Figure> {
    let robot = &scene.robot;
    let posed: Vec<String> = mirror.pose(home)?.poses.into_iter().map(|p| p.name).collect();
    let leg: Vec<&sim_domain_robot::model::Link> = robot.links.iter().filter(|l| l.name.starts_with(LEG_PREFIX) && posed.contains(&l.name)).collect();
    let names: Vec<String> = leg.iter().map(|l| l.name.clone()).collect();
    // Tree parent among the leg's links (None: the body, held still by the mirror).
    let parent: Vec<Option<usize>> = names.iter().map(|n| {
        robot.joints.iter().find(|j| &j.child == n && !j.is_loop()).and_then(|j| j.parent.as_ref()).and_then(|p| names.iter().position(|m| m == p))
    }).collect();
    // Parents before children.
    let mut order: Vec<usize> = Vec::new();
    while order.len() < names.len() {
        let before = order.len();
        for k in 0..names.len() {
            if !order.contains(&k) && parent[k].is_none_or(|p| order.contains(&p)) {
                order.push(k);
            }
        }
        if order.len() == before {
            return Err("the leg's links do not form a tree".into());
        }
    }
    let pose_at = |mirror: &mut KinematicMirror, q: &[f64]| -> R<Vec<Rt>> {
        let p = mirror.pose(q)?;
        names.iter().map(|n| p.poses.iter().find(|x| &x.name == n).map(rt_of).ok_or(format!("mirror pose has no link {n}"))).collect()
    };
    let relative = |world: &[Rt], k: usize| -> Rt {
        match parent[k] {
            Some(p) => rt_mul(&rt_inv(&world[p]), &world[k]),
            None => world[k],
        }
    };
    let home_world = pose_at(mirror, home)?;
    // Each motor swept alone from home (the mirror continues along its branch).
    let mut tables: Vec<Vec<Vec<Rt>>> = Vec::new(); // motor -> sample -> link -> relative
    for &(_, coordinate, lo, hi) in motors {
        let mut rows = Vec::new();
        for s in 0..FIGURE_SAMPLES {
            let mut q = home.to_vec();
            q[coordinate] = lo + (hi - lo) * s as f64 / (FIGURE_SAMPLES - 1) as f64;
            let world = pose_at(mirror, &q).map_err(|e| format!("figure: motor coordinate {coordinate} at {:.3} rad: {e}", q[coordinate]))?;
            rows.push((0..names.len()).map(|k| relative(&world, k)).collect());
        }
        pose_at(mirror, home)?;
        tables.push(rows);
    }
    // Which motor moves each link relative to its parent.
    let spread = |m: usize, k: usize| -> f64 {
        let first = tables[m][0][k];
        tables[m].iter().map(|r| r[k].iter().zip(&first).map(|(a, b)| (a - b).abs()).fold(0., f64::max)).fold(0., f64::max)
    };
    let mut driver: Vec<Option<usize>> = Vec::new();
    for k in 0..names.len() {
        let moving: Vec<usize> = (0..motors.len()).filter(|&m| spread(m, k) > 1e-6).collect();
        match moving.as_slice() {
            [] => driver.push(None),
            [m] => driver.push(Some(*m)),
            _ => return Err(format!("{}: moves with more than one motor relative to its parent; the figure cannot be composed", names[k])),
        }
    }
    let rel_at = |k: usize, angles: &[f64]| -> Rt {
        match driver[k] {
            None => relative(&home_world, k),
            Some(m) => {
                let (_, _, lo, hi) = motors[m];
                let x = ((angles[m] - lo) / (hi - lo)).clamp(0., 1.) * (FIGURE_SAMPLES - 1) as f64;
                let i = (x.floor() as usize).min(FIGURE_SAMPLES - 2);
                rt_lerp(&tables[m][i][k], &tables[m][i + 1][k], x - i as f64)
            }
        }
    };
    let compose = |angles: &[f64]| -> Vec<Rt> {
        let mut world = vec![[0.; 12]; names.len()];
        for &k in &order {
            let r = rel_at(k, angles);
            world[k] = match parent[k] {
                Some(p) => rt_mul(&world[p], &r),
                None => r,
            };
        }
        world
    };
    // Meshes, simplified.
    let meshes: Vec<(Vec<[f64; 3]>, Vec<[usize; 3]>)> = leg.iter().map(|l| simplify(&l.collision.vertices, &l.collision.triangles, FIGURE_CELL_M)).collect();
    // The check: random motor combinations, composed vs solved, every vertex.
    let mut worst: f64 = 0.;
    let mut seed: u64 = 0x9e3779b97f4a7c15;
    let mut rand = || { seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17; (seed >> 11) as f64 / (1u64 << 53) as f64 };
    const CHECKS: usize = 40;
    for _ in 0..CHECKS {
        let angles: Vec<f64> = motors.iter().map(|&(_, _, lo, hi)| lo + (hi - lo) * rand()).collect();
        let mut q = home.to_vec();
        for (m, &(_, c, _, _)) in motors.iter().enumerate() {
            q[c] = angles[m];
        }
        // From home each time, as the sweeps were: the mirror follows its branch along the way.
        pose_at(mirror, home)?;
        let solved = pose_at(mirror, &q).map_err(|e| format!("figure check at motor angles {angles:.3?} rad: {e}"))?;
        let composed = compose(&angles);
        for k in 0..names.len() {
            for v in &meshes[k].0 {
                let (a, b) = (rt_apply(&solved[k], v), rt_apply(&composed[k], v));
                worst = worst.max(((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt() * 1000.);
            }
        }
    }
    pose_at(mirror, home)?;
    if worst > FIGURE_TOLERANCE_MM {
        return Err(format!("the composed leg figure differs from the mirror by {worst:.2} mm (limit {FIGURE_TOLERANCE_MM} mm)"));
    }
    // Bounds of every view over the taught travel's corners, padded: what the
    // leg can do, at a scale that keeps it readable (a motor outside its
    // travel may run off the edge).
    let views = [("side", [1., 0., 0.], [0., 0., 1.]), ("top", [1., 0., 0.], [0., 1., 0.])];
    let mut bounds = vec![[f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY]; views.len()];
    for corner in 0..(1usize << motors.len()) {
        let angles: Vec<f64> = taught.iter().enumerate().map(|(m, &(lo, hi))| if corner >> m & 1 == 1 { hi } else { lo }).collect();
        let world = compose(&angles);
        for k in 0..names.len() {
            for v in &meshes[k].0 {
                let w = rt_apply(&world[k], v);
                for (b, (_, u, vv)) in bounds.iter_mut().zip(&views) {
                    let (x, y) = (w[0] * u[0] + w[1] * u[1] + w[2] * u[2], w[0] * vv[0] + w[1] * vv[1] + w[2] * vv[2]);
                    *b = [b[0].min(x), b[1].max(x), b[2].min(y), b[3].max(y)];
                }
            }
        }
    }
    for b in &mut bounds {
        let pad = 0.08 * (b[1] - b[0]).max(b[3] - b[2]);
        *b = [b[0] - pad, b[1] + pad, b[2] - pad, b[3] + pad];
    }
    let r4 = |v: f64| round(v, 4);
    let r6 = |v: f64| round(v, 6);
    let links: Vec<Value> = names.iter().enumerate().map(|(k, n)| {
        let (v, t) = &meshes[k];
        let mut row = json!({"name": n, "parent": parent[k], "motor": driver[k].map(|m| motors[m].0),
            "vertices": v.iter().flat_map(|p| p.map(r4)).collect::<Vec<_>>(), "triangles": t.iter().flatten().collect::<Vec<_>>()});
        match driver[k] {
            None => row["rel"] = json!(relative(&home_world, k).map(r6)),
            Some(m) => row["table"] = json!(tables[m].iter().map(|r| r[k].map(r6)).collect::<Vec<_>>()),
        }
        row
    }).collect();
    Ok(Figure { json: json!({
        "motors": motors.iter().map(|&(id, _, lo, hi)| json!({"id": id, "rad": [lo, hi], "samples": FIGURE_SAMPLES})).collect::<Vec<_>>(),
        "order": order, "links": links,
        "views": views.iter().zip(&bounds).map(|((n, u, v), b)| json!({"name": n, "u": u, "v": v, "bounds": b.map(r4)})).collect::<Vec<_>>(),
        "check": {"combinations": CHECKS, "max_error_mm": round(worst, 3), "tolerance_mm": FIGURE_TOLERANCE_MM,
            "method": "every simplified mesh vertex, the figure composed from the sampled per-link transforms against the kinematic mirror solved directly, at random motor angles inside the sampled ranges"},
        "mesh_cell_m": FIGURE_CELL_M,
    })})
}

/// Build the pack for the leg described by the calibration server config at `config_path`.
pub fn build_gait_pack(config_path: &Path, options: &PackOptions) -> R<GaitPack> {
    if !(options.effort > 0. && options.effort <= 1.) || !(options.sample_hz >= 5. && options.sample_hz <= 100.) || !(options.supply_v > 0.) {
        return Err("effort must be in (0, 1], sample rate 5-100 Hz, supply positive".into());
    }
    let cfg: Config = serde_json::from_slice(&std::fs::read(config_path).map_err(|e| format!("{}: {e}", config_path.display()))?)
        .map_err(|e| format!("{}: {e}", config_path.display()))?;
    let cal_path = cfg.output.join("calibration.json");
    let calibration: Calibration = serde_json::from_slice(&std::fs::read(&cal_path).map_err(|e| format!("{}: {e}", cal_path.display()))?)
        .map_err(|e| format!("{}: {e}", cal_path.display()))?;
    let runs = match &options.bindings_from {
        Some(p) => p.clone(),
        None => newest_gait_run(&cfg)?,
    };
    let run: Value = serde_json::from_slice(&std::fs::read(&runs).map_err(|e| format!("{}: {e}", runs.display()))?).map_err(|e| format!("{}: {e}", runs.display()))?;
    let scene_path = repo_root().join(LEG_SCENE);
    let scene: Scene = serde_json::from_slice(&std::fs::read(&scene_path).map_err(|e| format!("{}: {e}", scene_path.display()))?)
        .map_err(|e| format!("{}: {e}", scene_path.display()))?;
    let scene_robot = scene.clone();
    let mut mirror = KinematicMirror::new(scene, 0.2)?;
    let coordinates = mirror.coordinates();
    let home: Vec<f64> = coordinates.iter().map(|c| c.home).collect();

    // Motors the ESP32 may command, with each motor's binding.
    let mut bindings: Vec<LegBinding> = Vec::new();
    let mut axes: Vec<AxisCalibration> = Vec::new();
    let (mut axis_rows, mut excluded) = (Vec::new(), Vec::new());
    for (id, role) in &cfg.roles {
        let bound = run["bindings"].as_array().and_then(|b| b.iter().find(|x| x["id"] == *id));
        let a = calibration.axes.get(id).cloned().unwrap_or_default();
        let reason = if bound.is_none() {
            Some(format!("{} binds no CAD joint", rel(&runs)))
        } else if a.disabled {
            Some("disabled in the calibration record".into())
        } else if a.lower.is_none() || a.upper.is_none() {
            Some("both poses are not taught".into())
        } else if a.reference.is_none() {
            Some("no sim alignment saved".into())
        } else {
            multi_turn_misfit(&a)
        };
        if let Some(reason) = reason {
            excluded.push(json!({"id": id, "role": role, "reason": reason}));
            continue;
        }
        let bound = bound.unwrap();
        let joint = bound["joint"].as_str().ok_or("binding has no joint")?.to_string();
        let cad = coordinates.iter().find(|c| c.joint == joint).ok_or(format!("CAD has no motor joint {joint}"))?;
        let binding = LegBinding {
            id: *id,
            joint: joint.clone(),
            polarity: bound["polarity"].as_f64().ok_or("binding has no polarity")?,
            reference_counts: a.reference.unwrap() as f64,
            // As the panel: the saved alignment pose wins over the CAD home.
            home_rad: a.reference_joint_rad.unwrap_or(cad.home),
        };
        binding.validate()?;
        let (lo, hi) = a.encoder_bounds();
        let (wlo, whi) = taught_window(&a);
        let multi = is_multi_turn(&a);
        axis_rows.push(json!({"id": id, "role": role, "joint": joint, "polarity": binding.polarity, "reference_counts": binding.reference_counts,
            "home_rad": binding.home_rad, "taught_counts": [lo, hi], "window_counts": [wlo, whi],
            "window_rad": [binding.joint_rad(wlo).min(binding.joint_rad(whi)), binding.joint_rad(wlo).max(binding.joint_rad(whi))],
            "multi_turn": multi, "drive": if multi { "servo_speed" } else { "servo_position" },
            // The calibration's pose names: a reversed motor's lower pose is its high-count end.
            "reversed": a.reversed(), "lower_pose_counts": a.lower, "upper_pose_counts": a.upper,
            "travel_counts": [lo.unwrap().min(hi.unwrap()), lo.unwrap().max(hi.unwrap())],
            "turn_margin_counts": if multi { TURN_MARGIN_COUNTS } else { 0 }}));
        bindings.push(binding);
        axes.push(a);
    }
    if bindings.is_empty() {
        return Err(format!("no motor can be commanded from the ESP32: {}", excluded.iter().map(|e| format!("{}: {}", e["role"].as_str().unwrap_or(""), e["reason"].as_str().unwrap_or(""))).collect::<Vec<_>>().join("; ")));
    }

    // The figure: each commandable motor swept over its taught travel widened
    // by FIGURE_MARGIN (so a motor found outside it still draws where it is).
    let motors: Vec<(u8, usize, f64, f64)> = bindings.iter().zip(&axes).map(|(b, a)| {
        let (lo, hi) = a.encoder_bounds();
        let (lo, hi) = (lo.unwrap().min(hi.unwrap()) as f64, lo.unwrap().max(hi.unwrap()) as f64);
        let margin = (FIGURE_MARGIN * (hi - lo)).max(FIGURE_MARGIN_COUNTS);
        let (x, y) = (b.joint_rad(lo - margin), b.joint_rad(hi + margin));
        (b.id, coordinates.iter().position(|c| c.joint == b.joint).unwrap(), x.min(y), x.max(y))
    }).collect();
    let taught: Vec<(f64, f64)> = bindings.iter().zip(&axes).map(|(b, a)| {
        let (lo, hi) = taught_window(a);
        let (x, y) = (b.joint_rad(lo), b.joint_rad(hi));
        (x.min(y), x.max(y))
    }).collect();
    let figure = build_figure(&scene_robot, &mut mirror, &home, &motors, &taught)?;

    // Gaits.
    let paths: Vec<String> = if options.gaits.is_empty() {
        super::gait::gait_catalog()?["gaits"].as_array().cloned().unwrap_or_default().iter()
            .filter_map(|g| g["path"].as_str().map(str::to_string)).take(options.max_gaits).collect()
    } else {
        options.gaits.clone()
    };
    let mut gait_rows = Vec::new();
    let mut binary = axes_table(&bindings, &axes);
    for path in &paths {
        let compiled = gait_with_governor(path)?;
        let gait = Gait::from_compiled(&compiled, path)?;
        let n = (gait.info.period_s * options.sample_hz).round().max(8.) as usize;
        let dt = gait.info.period_s / n as f64;
        let bound: Vec<(LegBinding, AxisCalibration)> = bindings.iter().cloned().zip(axes.iter().cloned()).filter(|(b, _)| gait.index(&b.joint).is_some()).collect();
        let (gb, ga): (Vec<LegBinding>, Vec<AxisCalibration>) = bound.into_iter().unzip();
        let mut reasons = gait_misfit(&cfg, &gait, &gb, &ga)?;
        if gb.is_empty() {
            reasons.push("the gait drives none of the commandable motors".into());
        }
        let mut governed = GovernedGait::new(gait.clone());
        let (limits, _registry) = govern_leg(&cfg, &mut governed, &gb, &ga, options.effort, options.supply_v)?;
        // The preview's governed curve: one cycle after WARM cycles from rest
        // (the live governor on the leg settles to about this).
        const WARM: usize = 8;
        let desired0 = governed.desired(0.)?;
        let check_start = governed.clone();
        for b in &gb {
            let i = gait.index(&b.joint).unwrap();
            governed.start_from(i, desired0[i]);
        }
        let mut steady = Vec::new();
        for k in 0..WARM * n {
            let out = governed.step(k as f64 * dt, dt, 1.)?;
            if k >= (WARM - 1) * n {
                steady.push(out);
            }
        }
        let steady = &steady[..];
        // Plots: every leg joint the gait has, raw and (for commandable motors) governed, in degrees.
        let leg_joints: Vec<&String> = gait.info.joints.iter().filter(|j| j.starts_with(LEG_PREFIX)).collect();
        let plots: Vec<Value> = leg_joints.iter().map(|j| {
            let i = gait.index(j).unwrap();
            let desired: Vec<f64> = (0..n).map(|k| gait.sample(k as f64 * dt).map(|q| round(q[i].to_degrees(), 2))).collect::<R<_>>()?;
            let motor = gb.iter().find(|b| &&b.joint == j);
            let command = motor.map(|_| steady.iter().map(|o| round(o[i].0.to_degrees(), 2)).collect::<Vec<_>>());
            Ok(json!({"joint": j, "id": motor.map(|b| b.id), "desired_deg": desired, "command_deg": command}))
        }).collect::<R<_>>()?;
        // Figure frames from the raw gait: each commandable motor's joint angle (CAD home where the gait has none).
        let mut frames = Vec::new();
        for k in 0..n {
            let q = gait.sample(k as f64 * dt)?;
            frames.push(motors.iter().map(|&(_, c, _, _)| round(gait.index(&coordinates[c].joint).map_or(home[c], |i| q[i]), 5)).collect::<Vec<_>>());
        }
        let mut row = json!({"name": gait.info.name.rsplit('/').nth(1).unwrap_or(path), "path": path, "period_s": gait.info.period_s, "samples": n, "frames_rad": frames,
            "nominal_speed_m_s": gait.info.nominal_speed_m_s, "governor": gait.info.governor, "limits": limits, "plots": plots,
            "playable": reasons.is_empty(), "reasons": reasons});
        if reasons_ok(&row) {
            let index: Vec<usize> = gb.iter().map(|b| gait.index(&b.joint).unwrap()).collect();
            // Desired reference, densely sampled so linear interpolation stays
            // within a count of the spline.
            let dense = (gait.info.period_s * DESIRED_HZ).round().max(16.) as usize;
            let mut desired = Vec::new();
            for k in 0..dense {
                let q = governed.desired(gait.info.period_s * k as f64 / dense as f64)?;
                desired.push(gb.iter().zip(&index).map(|(b, &i)| b.counts(q[i]) as f32).collect());
            }
            // The check: the panel's governor from the first desired value, at rest.
            let mut reference = check_start.clone();
            for &i in &index {
                reference.start_from(i, desired0[i]);
            }
            let mut check = Vec::new();
            for k in 0..CHECK_STEPS {
                let out = reference.step(k as f64 * CHECK_DT, CHECK_DT, 1.)?;
                check.push(gb.iter().zip(&index).map(|(b, &i)| b.counts(out[i].0) as f32).collect());
            }
            let plan = Plan {
                period_s: gait.info.period_s as f32,
                axes: gb.iter().zip(&ga).zip(&index).map(|((b, a), &i)| {
                    let (lo, hi) = a.encoder_bounds();
                    let c = governed.config(i).unwrap();
                    let multi = is_multi_turn(a);
                    PlanAxis { id: b.id, window: (lo.unwrap().min(hi.unwrap()), lo.unwrap().max(hi.unwrap())), governor_period_s: c.period_s as f32,
                        max_speed_counts_s: (c.maximum_speed_rad_s / RAD) as f32, max_acceleration_counts_s2: (c.maximum_acceleration_rad_s2 / RAD) as f32,
                        response_rate_per_s: c.response_rate_per_s as f32,
                        drive: if multi { DriveMode::ServoSpeed } else { DriveMode::ServoPosition }.register(),
                        flags: if multi { FLAG_MULTI_TURN } else { 0 },
                        // As the panel passes it to servo_command.
                        hold_tolerance_counts: cfg.sweep_tuning.hold_deadband_counts.unwrap_or(0.) as f32,
                        turn_margin_counts: if multi { TURN_MARGIN_COUNTS } else { 0 } }
                }).collect(),
                desired,
                check_dt_s: CHECK_DT as f32,
                check,
            };
            let bytes = plan.encode();
            row["plan"] = json!({"offset": binary.len(), "length": bytes.len(), "ids": gb.iter().map(|b| b.id).collect::<Vec<_>>(), "desired_samples": dense,
                "needs_turn": gb.iter().zip(&ga).filter(|(_, a)| is_multi_turn(a)).map(|(b, _)| b.id).collect::<Vec<_>>()});
            binary.extend(bytes);
        }
        gait_rows.push(row);
    }

    let json = json!({
        "format": "robot-link gait pack", "version": PACK_VERSION,
        "fidelity": "Kinematic preview: the CAD collision meshes placed by the kinematic mirror (no physics, no contact). Plans are the desired reference and governor the Rust panel would command; the ESP32 runs the governor live. The preview's governed curve is one cycle after eight from rest.",
        "effort": options.effort, "sample_hz": options.sample_hz, "supply_v": options.supply_v,
        "fixture": cfg.fixture,
        "source": {
            "config": rel(config_path), "config_sha256": sha256_file(config_path)?,
            "calibration": rel(&cal_path), "calibration_sha256": sha256_file(&cal_path)?,
            "bindings_from": rel(&runs), "bindings_sha256": sha256_file(&runs)?,
            "scene": LEG_SCENE, "scene_sha256": sha256_file(&scene_path)?,
            "registry": "examples/actuators/hx30hm/accepted/registry.json",
            "registry_sha256": sha256_file(&repo_root().join("examples/actuators/hx30hm/accepted/registry.json"))?,
        },
        "axes": axis_rows, "excluded": excluded,
        "figure": figure.json,
        "gaits": gait_rows,
    });
    Ok(GaitPack { json, binary })
}
/// A run played by the ESP32 (its `GET /api/gait_log`), as a record beside
/// the panel's gait runs: the samples, per-motor tracking of the command the
/// ESP32 sent and of the gait's desired reference, and where it came from.
/// Statistics count only samples after the approach (gait time > 0).
pub fn esp32_run_record(log: &Value, pack: &Value, firmware: &str) -> R<Value> {
    let rows = log["samples"].as_array().ok_or("gait log has no samples")?;
    let mut per: std::collections::BTreeMap<u64, Vec<(f64, f64, f64)>> = Default::default();
    for r in rows {
        let r = r.as_array().ok_or("gait log sample is not a row")?;
        let f = |i: usize| r.get(i).and_then(Value::as_f64).ok_or("gait log sample is short");
        if f(1)? > 0. {
            per.entry(f(2)? as u64).or_default().push((f(3)?, f(4)?, f(5)?));
        }
    }
    let rms = |v: &[f64]| (v.iter().map(|e| e * e).sum::<f64>() / v.len().max(1) as f64).sqrt();
    let peak = |v: &[f64]| v.iter().fold(0f64, |m, e| m.max(e.abs()));
    let statistics: serde_json::Map<String, Value> = per.iter().map(|(id, s)| {
        let to_command: Vec<f64> = s.iter().map(|(c, _, a)| a - c).collect();
        let to_desired: Vec<f64> = s.iter().map(|(_, d, a)| a - d).collect();
        (id.to_string(), json!({"samples": s.len(), "tracking_rms_counts": round(rms(&to_command), 2), "tracking_peak_counts": round(peak(&to_command), 2),
            "desired_rms_counts": round(rms(&to_desired), 2), "desired_peak_counts": round(peak(&to_desired), 2)}))
    }).collect();
    Ok(json!({
        "version": 1, "host": "robot-link ESP32-P4 (firmware/robot-link)", "firmware": firmware,
        "gait": log["gait"], "outcome": log["result"], "stop_verified": log["stop_verified"], "speed_scale": log["speed"],
        "drive_mode": pack["axes"].as_array().map(|a| a.iter().map(|x| (x["id"].to_string(), x["drive"].clone())).collect::<serde_json::Map<_, _>>()),
        "effort": pack["effort"], "supply_v": pack["supply_v"], "pack_source": pack["source"],
        "axes": pack["axes"], "excluded": pack["excluded"],
        "statistics": statistics,
        "columns": log["columns"], "samples": log["samples"], "samples_total": log["samples_total"],
        "scope": "Suspended leg (no ground contact). The ESP32 runs the gait's governor live and streams servo position goals (servo speed with a position trim for a multi-turn motor, whose counts include the turns the operator confirmed); tracking is the measured encoder minus the goal it sent (and minus the gait's desired reference). Samples are at the ESP32's telemetry rate, about 20 Hz per motor; the log keeps the last 2048.",
    }))
}
/// The pack's axes table: each commandable motor's taught travel (low, high
/// counts, turns included) and multi-turn flag.
fn axes_table(bindings: &[LegBinding], axes: &[AxisCalibration]) -> Vec<u8> {
    let mut b = AXES_MAGIC.to_vec();
    b.extend_from_slice(&[bindings.len() as u8, 0, 0, 0]);
    for (bd, a) in bindings.iter().zip(axes) {
        let (lo, hi) = a.encoder_bounds();
        let (lo, hi) = (lo.unwrap().min(hi.unwrap()), lo.unwrap().max(hi.unwrap()));
        let multi = is_multi_turn(a);
        let flags = if multi { FLAG_MULTI_TURN } else { 0 } | if a.reversed() { FLAG_REVERSED } else { 0 };
        b.extend_from_slice(&[bd.id, flags, 0, 0]);
        for v in [lo, hi, if multi { TURN_MARGIN_COUNTS } else { 0 }] {
            b.extend_from_slice(&v.to_le_bytes());
        }
    }
    b
}
/// Whether the motor's taught poses or alignment lie beyond one encoder turn.
fn is_multi_turn(a: &AxisCalibration) -> bool {
    a.coordinate_session.is_some() || a.reference_session.is_some()
        || [a.lower, a.upper, a.reference].iter().flatten().any(|c| !(0..=4095).contains(c))
}
/// Why a multi-turn motor cannot be driven from the ESP32, if it cannot. Its
/// poses and alignment must share one turn count: the alignment inside the
/// taught travel (where the count is unambiguous), and the travel short
/// enough that a reading allows at most two places.
fn multi_turn_misfit(a: &AxisCalibration) -> Option<String> {
    if !is_multi_turn(a) {
        return None;
    }
    let (lo, hi) = (a.lower?.min(a.upper?), a.lower?.max(a.upper?));
    let reference = a.reference?;
    if a.reference_session.is_some() && a.reference_session != a.coordinate_session {
        Some("its sim alignment was saved in a different tracking session from its poses; re-save the alignment".into())
    } else if !(lo..=hi).contains(&reference) {
        Some(format!("its sim alignment ({reference}) lies outside its taught travel ({lo}..{hi}), so its turn is not tied to the poses; re-save the alignment inside the travel"))
    } else if hi - lo + 2 * TURN_MARGIN_COUNTS >= 2 * 4096 {
        Some(format!("its taught travel ({} counts) is two turns or more: a reading would allow three places", hi - lo))
    } else {
        None
    }
}
/// Promote poses taught on the ESP32's page (its `GET /api/calibration`)
/// into the leg's `calibration.json`, the one source the next pack is built
/// from. Only motors the ESP32 marks changed are touched, and each must still
/// have the travel this calibration had when the ESP32's pack was built (else
/// the pack is stale: rebuild and push it first). A pose with an open end is
/// refused. Widening a motor's travel raises a limit, so it needs `reason`.
/// The previous file is kept as `calibration-<ms>-before-esp32.json` and the
/// change is recorded in `esp32-poses-<ms>.json` beside it; returns that record.
pub fn promote_esp32_poses(config_path: &Path, esp32: &Value, reason: Option<&str>) -> R<Value> {
    let cfg: Config = serde_json::from_slice(&std::fs::read(config_path).map_err(|e| format!("{}: {e}", config_path.display()))?)
        .map_err(|e| format!("{}: {e}", config_path.display()))?;
    let cal_path = cfg.output.join("calibration.json");
    let before = std::fs::read(&cal_path).map_err(|e| format!("{}: {e}", cal_path.display()))?;
    let mut calibration: Calibration = serde_json::from_slice(&before).map_err(|e| format!("{}: {e}", cal_path.display()))?;
    let motors = esp32["motors"].as_array().ok_or("the ESP32's calibration has no motors")?;
    let pair = |v: &Value, what: &str, id: u64| -> R<(i32, i32)> {
        let a = v.as_array().filter(|a| a.len() == 2).ok_or(format!("motor {id}: {what} is not [low, high]"))?;
        let n = |x: &Value| x.as_i64().and_then(|x| i32::try_from(x).ok()).ok_or(format!("motor {id}: {what} is not counts"));
        Ok((n(&a[0])?, n(&a[1])?))
    };
    let mut changes = Vec::new();
    for m in motors.iter().filter(|m| m["changed"] == true) {
        let id = m["id"].as_u64().ok_or("a motor has no id")?;
        if m["open"].as_array().is_some_and(|o| o.iter().any(|x| x == true)) {
            return Err(format!("motor {id} has an open end on the ESP32: set that pose before promoting"));
        }
        let (pack_lo, pack_hi) = pair(&m["pack"], "pack travel", id)?;
        let (lo, hi) = pair(&m["taught"], "taught travel", id)?;
        if lo >= hi {
            return Err(format!("motor {id}: taught travel {lo}..{hi} is empty"));
        }
        let axis = calibration.axes.get_mut(&(id as u8)).ok_or(format!("motor {id} is not in {}", cal_path.display()))?;
        let (a, b) = axis.encoder_bounds();
        let (old_lo, old_hi) = match (a, b) {
            (Some(a), Some(b)) => (a.min(b), a.max(b)),
            _ => return Err(format!("motor {id} has an untaught pose in {}; teach it in the panel", cal_path.display())),
        };
        if (old_lo, old_hi) != (pack_lo, pack_hi) {
            return Err(format!("motor {id}: the ESP32's pack was built from travel {pack_lo}..{pack_hi}, but {} now says {old_lo}..{old_hi}; rebuild and push the pack, then teach again", cal_path.display()));
        }
        let previous = json!({"lower": axis.lower, "upper": axis.upper});
        // Keep the motor's direction: a reversed motor stores its high end as `lower`.
        if axis.reversed() {
            (axis.lower, axis.upper) = (Some(hi), Some(lo));
        } else {
            (axis.lower, axis.upper) = (Some(lo), Some(hi));
        }
        axis.validate()?;
        let widened = lo < old_lo || hi > old_hi;
        changes.push(json!({"id": id, "role": axis.role, "previous": previous, "now": {"lower": axis.lower, "upper": axis.upper},
            "travel_counts": {"previous": [old_lo, old_hi], "now": [lo, hi]}, "widened": widened}));
    }
    if changes.is_empty() {
        return Err("the ESP32 has no changed poses to promote".into());
    }
    let widened = changes.iter().any(|c| c["widened"] == true);
    let reason = reason.map(str::trim).filter(|r| !r.is_empty());
    if widened && reason.is_none() {
        return Err("a promoted pose widens a motor's travel, which raises a limit: give the reason (--reason)".into());
    }
    calibration.validate()?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e| e.to_string())?.as_millis();
    let backup = cfg.output.join(format!("calibration-{stamp}-before-esp32.json"));
    std::fs::write(&backup, &before).map_err(|e| format!("{}: {e}", backup.display()))?;
    let record = json!({
        "version": 1, "kind": "esp32 taught poses promoted",
        "source": "robot-link ESP32 page (GET /api/calibration): poses set by the operator at the leg, the reading of each motor's encoder (with its confirmed turns for a multi-turn motor)",
        "esp32_pack_sha256": esp32["pack_sha256"], "calibration": rel(&cal_path), "previous_file": rel(&backup),
        "changes": changes, "raises_a_limit": widened, "reason": reason,
        "next": "rebuild the gait pack from this calibration and push it (leg_gait_pack ... --push); the ESP32 drops each taught pose the new pack matches",
    });
    let record_path = cfg.output.join(format!("esp32-poses-{stamp}.json"));
    std::fs::write(&record_path, serde_json::to_vec_pretty(&record).unwrap()).map_err(|e| format!("{}: {e}", record_path.display()))?;
    // As the panel writes it.
    std::fs::write(&cal_path, serde_json::to_vec_pretty(&calibration).map_err(|e| e.to_string())?).map_err(|e| format!("{}: {e}", cal_path.display()))?;
    Ok(json!({"record": rel(&record_path), "calibration": rel(&cal_path), "changes": record["changes"], "raises_a_limit": widened}))
}
fn reasons_ok(row: &Value) -> bool {
    row["playable"] == true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plan_round_trips_and_pack_digest_guards_it() {
        let axis = |id, window, drive, flags| PlanAxis { id, window, governor_period_s: 0.02, max_speed_counts_s: 900.5, max_acceleration_counts_s2: 12000., response_rate_per_s: 10.,
            drive, flags, hold_tolerance_counts: 16., turn_margin_counts: if flags != 0 { TURN_MARGIN_COUNTS } else { 0 } };
        let plan = Plan {
            period_s: 0.8,
            axes: vec![axis(1, (2514, 3329), 0, 0), axis(2, (-1407, 3116), 1, FLAG_MULTI_TURN)],
            desired: vec![vec![2600., 2400.], vec![2610., 2399.5], vec![2620.25, 2398.]],
            check_dt_s: 0.02,
            check: vec![vec![2600., 2400.], vec![2601., 2400.]],
        };
        let bytes = plan.encode();
        assert_eq!(bytes.len(), 12 + 2 * 36 + 3 * 2 * 4 + 8 + 2 * 2 * 4);
        assert_eq!(Plan::decode(&bytes).unwrap(), plan);
        let pack = GaitPack { json: json!({"gaits": [{"plan": {"offset": 0, "length": bytes.len()}}]}), binary: bytes.clone() };
        let file = pack.to_bytes();
        let (j, b) = read_pack(&file).unwrap();
        assert_eq!(b, bytes);
        assert_eq!(j["gaits"][0]["plan"]["length"], bytes.len());
        let mut bad = file.clone();
        *bad.last_mut().unwrap() ^= 1;
        assert!(read_pack(&bad).unwrap_err().contains("digest"));
    }
    #[test]
    fn multi_turn_motors_need_one_turn_count_for_poses_and_alignment() {
        let worm = AxisCalibration { lower: Some(3116), upper: Some(-1407), reference: Some(1081), coordinate_session: Some("1".into()), ..Default::default() };
        assert!(is_multi_turn(&worm));
        assert_eq!(multi_turn_misfit(&worm), None);
        let knee = AxisCalibration { lower: Some(2514), upper: Some(3329), reference: Some(3082), ..Default::default() };
        assert!(!is_multi_turn(&knee) && multi_turn_misfit(&knee).is_none());
        let outside = AxisCalibration { reference: Some(3500), ..worm.clone() };
        assert!(multi_turn_misfit(&outside).unwrap().contains("outside its taught travel"));
        let other_session = AxisCalibration { reference: Some(-500), reference_session: Some("2".into()), ..worm.clone() };
        assert!(multi_turn_misfit(&other_session).unwrap().contains("different tracking session"));
        let long = AxisCalibration { lower: Some(-4000), upper: Some(4000), reference: Some(0), ..worm };
        assert!(multi_turn_misfit(&long).unwrap().contains("three places"));
    }
    #[test]
    fn esp32_poses_are_promoted_with_direction_kept_and_widening_needs_a_reason() {
        let dir = std::env::temp_dir().join(format!("esp32-poses-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut cfg: Value = serde_json::from_slice(&std::fs::read(repo_root().join("examples/actuators/hx30hm/hardware/2026-09-21-leg-calibration/server.json")).unwrap()).unwrap();
        cfg["output"] = json!(dir);
        let config = dir.join("server.json");
        std::fs::write(&config, serde_json::to_vec(&cfg).unwrap()).unwrap();
        let mut cal = Calibration::default();
        cal.axes.insert(1, AxisCalibration { role: "knee".into(), lower: Some(2514), upper: Some(3329), reference: Some(3082), ..Default::default() });
        cal.axes.insert(2, AxisCalibration { role: "worm".into(), lower: Some(3116), upper: Some(-1407), reference: Some(1081), coordinate_session: Some("1".into()), ..Default::default() });
        std::fs::write(dir.join("calibration.json"), serde_json::to_vec(&cal).unwrap()).unwrap();
        let esp32 = |motors: Value| json!({"pack_sha256": "ab", "motors": motors});
        let read = || -> Calibration { serde_json::from_slice(&std::fs::read(dir.join("calibration.json")).unwrap()).unwrap() };
        // A narrower knee needs no reason.
        let out = promote_esp32_poses(&config, &esp32(json!([{"id": 1, "changed": true, "open": [false, false], "pack": [2514, 3329], "taught": [2600, 3300]}])), None).unwrap();
        assert_eq!(out["raises_a_limit"], false);
        assert_eq!((read().axes[&1].lower, read().axes[&1].upper), (Some(2600), Some(3300)));
        // The pack must have been built from the travel the file has now.
        let stale = promote_esp32_poses(&config, &esp32(json!([{"id": 1, "changed": true, "open": [false, false], "pack": [2514, 3329], "taught": [2500, 3300]}])), Some("x"));
        assert!(stale.unwrap_err().contains("rebuild and push the pack"));
        // The reversed worm keeps its high end as `lower`; widening it needs a reason.
        let wider = esp32(json!([{"id": 2, "changed": true, "open": [false, false], "pack": [-1407, 3116], "taught": [-1500, 3116]}]));
        assert!(promote_esp32_poses(&config, &wider, None).unwrap_err().contains("--reason"));
        let out = promote_esp32_poses(&config, &wider, Some("operator moved the worm's lower stop")).unwrap();
        assert_eq!(out["raises_a_limit"], true);
        let worm = &read().axes[&2];
        assert_eq!((worm.lower, worm.upper, worm.reversed(), worm.coordinate_session.as_deref()), (Some(3116), Some(-1500), true, Some("1")));
        // An open end is refused.
        let open = esp32(json!([{"id": 1, "changed": true, "open": [false, true], "pack": [2600, 3300], "taught": [2600, 8000000]}]));
        assert!(promote_esp32_poses(&config, &open, Some("x")).unwrap_err().contains("open end"));
        assert_eq!(std::fs::read_dir(&dir).unwrap().filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().starts_with("esp32-poses-")).count() >= 1, true);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
