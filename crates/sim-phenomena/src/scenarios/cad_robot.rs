//! A robot from the CAD tool: `robocad` exports a `*.simrobot.json` with
//! every body's mass, centre of mass, planar inertia and section outline,
//! and the revolute joints between them; this builds the planar
//! multibody — a root body (fixed if the CAD calls it `ground`) with
//! serial chains of links hanging off it through the joints, servos on
//! every joint held to the CAD pose by a PD loop on the seam, and
//! compliant point contacts under the root and at every chain tip — and
//! runs it. The viewer (`sim-app --scene cad --model file`) draws the
//! outlines in their simulated poses and rebuilds whenever the file
//! changes, which closes the loop: edit in CAD, save, watch.

use crate::world::{damped_runtime, registry};
pub use super::cad_physical::{run_physical, BuildOptions, PhysicalRobot};
pub use sim_domain_robot::PhysicalModel;
use serde::Deserialize;
use sim_compile::Runtime;
use sim_core::{BehaviorId, BehaviorRegistry, FnCoupler, ModelWorld, StateId};
use sim_domain_control::external::EXTERNAL;
use sim_domain_multibody::chain::CHAIN;
use sim_domain_multibody::contact as ct;
use sim_domain_sensing as sense;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Deserialize)]
pub struct CadBody {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub material: Option<String>,
    pub mass_kg: f64,
    /// Centre of mass in the working plane (mm).
    pub com: [f64; 2],
    #[serde(default)]
    pub inertia_zz: f64,
    #[serde(default)]
    pub bbox: Vec<Vec<f64>>,
    #[serde(default)]
    pub outline: Vec<Vec<[f64; 2]>>,
    #[serde(default)]
    pub ground: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CadMotor {
    pub name: String,
    #[serde(default)]
    pub spec: String,
    /// N·m at the joint (gear ratio applied).
    pub stall_torque: f64,
    /// rad/s at the joint.
    pub no_load_speed: f64,
    #[serde(default)]
    pub rotor_inertia: f64,
}

fn default_joint_type() -> String {
    "revolute".to_owned()
}

#[derive(Clone, Debug, Deserialize)]
pub struct CadJoint {
    pub name: String,
    pub child: String,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub pivot2: [f64; 2],
    #[serde(default = "default_joint_type", rename = "type")]
    pub kind: String,
    /// `[lower, upper]` in rad, either may be null.
    #[serde(default)]
    pub limits: Option<Vec<Option<f64>>>,
    #[serde(default)]
    pub motor: Option<CadMotor>,
    #[serde(default)]
    pub damping: f64,
    /// +1 when the joint axis points along the plane normal, −1 against it.
    #[serde(default = "one")]
    pub axis_sign: f64,
}

fn one() -> f64 {
    1.0
}

#[derive(Clone, Debug, Deserialize)]
pub struct CadModel {
    pub bodies: Vec<CadBody>,
    #[serde(default)]
    pub joints: Vec<CadJoint>,
    #[serde(default)]
    pub source: Option<String>,
}

impl CadModel {
    pub fn load(path: &str) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))
    }

    fn body(&self, name: &str) -> Option<&CadBody> {
        self.bodies.iter().find(|b| b.name == name)
    }
}

/// One link of a chain as built: which CAD body, the pivot it hangs from
/// (world, m) and its length along the link.
#[derive(Clone, Debug)]
pub struct LinkInfo {
    pub body: usize,
    pub joint_name: String,
    pub length: f64,
    /// The CAD outline's offset from the pivot, in the link frame (m), and
    /// the link frame's angle relative to the CAD pose (rad).
    pub cad_angle: f64,
    pub pivot_cad: [f64; 2],
}

#[derive(Clone, Debug)]
pub struct ChainInfo {
    pub behavior: BehaviorId,
    pub links: Vec<LinkInfo>,
    pub angles: Vec<StateId>,
    pub tip: [StateId; 2],
}

pub struct CadRobot {
    pub runtime: Runtime,
    pub model: CadModel,
    pub root: usize,
    pub root_fixed: bool,
    /// x, y, θ, vx, vy, ω of the root body (m, rad).
    pub root_ids: [StateId; 6],
    pub chains: Vec<ChainInfo>,
    /// Joint targets held by the PD loop, in chain order (rad), shared with the seam.
    pub targets: Arc<Mutex<Vec<f64>>>,
    pub joint_names: Vec<String>,
    pub seam: Option<BehaviorId>,
    /// Ground plane height in the CAD's frame (mm): the lowest point of the model.
    pub floor_mm: f64,
    pub scale: f64,
    pub warnings: Vec<String>,
}

const SCALE: f64 = 1.0e-3; // mm → m

/// Serial chains from the joint graph: each child of the root starts a chain
/// that follows single children onward.
fn chains_of(model: &CadModel, root: &str) -> Vec<Vec<CadJoint>> {
    let mut by_parent: HashMap<String, Vec<CadJoint>> = HashMap::new();
    for j in &model.joints {
        let parent = j.parent.clone().unwrap_or_else(|| root.to_string());
        by_parent.entry(parent).or_default().push(j.clone());
    }
    let mut chains = Vec::new();
    let mut stack: Vec<CadJoint> = by_parent.get(root).cloned().unwrap_or_default();
    while let Some(first) = stack.pop() {
        let mut chain = vec![first.clone()];
        let mut current = first.child.clone();
        loop {
            let children = by_parent.get(&current).cloned().unwrap_or_default();
            if children.len() == 1 {
                chain.push(children[0].clone());
                current = children[0].child.clone();
            } else {
                // A branch: further children start their own chains from this link's body.
                for c in children {
                    stack.push(c);
                }
                break;
            }
        }
        chains.push(chain);
    }
    chains
}

impl CadRobot {
    /// `bandwidth_hz` and `damping_ratio` size each joint's PD from the
    /// inertia it carries (everything outboard of the joint), so a 30 g
    /// printed link and a 3 kg arm are both held critically.
    pub fn build(model: CadModel, registry: &BehaviorRegistry, bandwidth_hz: f64, damping_ratio: f64) -> Result<Self, String> {
        if model.bodies.is_empty() {
            return Err("the model has no bodies".into());
        }
        let mut warnings = Vec::new();
        let model = {
            let mut m = model;
            m.joints.retain(|j| {
                match j.kind.as_str() {
                    "revolute" | "continuous" => true,
                    "fixed" => false,
                    other => {
                        warnings.push(format!("joint {} is {other}: the planar simulator has revolute joints only, it is treated as fixed", j.name));
                        false
                    }
                }
            });
            m
        };
        let root = model
            .bodies
            .iter()
            .position(|b| b.ground)
            .unwrap_or_else(|| {
                // The heaviest body that is nobody's child.
                let children: Vec<&str> = model.joints.iter().map(|j| j.child.as_str()).collect();
                let mut best = 0;
                for (i, b) in model.bodies.iter().enumerate() {
                    if !children.contains(&b.name.as_str()) && b.mass_kg > model.bodies[best].mass_kg.max(if children.contains(&model.bodies[best].name.as_str()) { -1.0 } else { model.bodies[best].mass_kg }) {
                        best = i;
                    }
                }
                best
            });
        let root_body = &model.bodies[root];
        let root_fixed = root_body.ground;
        let floor_mm = model.bodies.iter().flat_map(|b| b.outline.iter().flatten().map(|p| p[1]).chain(b.bbox.get(0).and_then(|lo| lo.get(2)).copied())).fold(f64::INFINITY, f64::min);
        let floor_mm = if floor_mm.is_finite() { floor_mm } else { 0.0 };
        let mut m = ModelWorld::default();
        let mass = if root_fixed { 1.0e6 } else { root_body.mass_kg.max(1.0e-3) };
        let inertia = if root_fixed { 1.0e6 } else { root_body.inertia_zz.max(1.0e-6) };
        let body = m
            .part(registry, "root", ct::PLANAR_RIGID_BODY, [
                ("mass", mass), ("inertia", inertia), ("gravity", if root_fixed { 0.0 } else { 9.81 }),
                ("initial.x", root_body.com[0] * SCALE), ("initial.y", (root_body.com[1] - floor_mm) * SCALE),
            ])
            .unwrap();
        let mut body_ports = vec![body.port("frame")];
        // Contacts under the root: the two lowest outline points (or bbox corners).
        if !root_fixed {
            let pts: Vec<[f64; 2]> = root_body.outline.iter().flatten().copied().collect();
            let mut lows: Vec<[f64; 2]> = if pts.len() >= 2 { pts.clone() } else { vec![[root_body.com[0] - 20.0, floor_mm], [root_body.com[0] + 20.0, floor_mm]] };
            lows.sort_by(|a, b| a[1].total_cmp(&b[1]));
            let left = lows.iter().take(6).min_by(|a, b| a[0].total_cmp(&b[0])).copied().unwrap();
            let right = lows.iter().take(6).max_by(|a, b| a[0].total_cmp(&b[0])).copied().unwrap();
            for (k, p) in [left, right].iter().enumerate() {
                let foot = m.part(registry, &format!("root.contact{k}"), ct::POINT_PLANE_COMPLIANT, [("px", (p[0] - root_body.com[0]) * SCALE), ("py", (p[1] - root_body.com[1]) * SCALE), ("friction", 0.8), ("stiffness", 2.0e5), ("damping", 2.0e3)]).unwrap();
                body_ports.push(foot.port("frame"));
            }
        }
        let chain_specs = chains_of(&model, &root_body.name);
        let mut joint_names = Vec::new();
        let mut seam_params: Vec<(&'static str, f64)> = vec![("period", 2.0e-3)];
        let mut chains_built = Vec::new();
        let mut wiring: Vec<(sim_core::Instance, Vec<(String, sim_core::PortId)>)> = Vec::new();
        // Per-joint gains from the outboard inertia about the joint's pivot,
        // plus the joint's limits, motor torque/speed caps and damping.
        let mut gains: Vec<(f64, f64)> = Vec::new();
        let mut caps: Vec<(Option<f64>, Option<f64>, f64, f64, f64, f64)> = Vec::new(); // lower, upper, stall, no-load speed, damping, axis sign
        for (ci, spec) in chain_specs.iter().enumerate() {
            let mut params: Vec<(&'static str, f64)> = vec![("gravity", 9.81)];
            let first_pivot = spec[0].pivot2;
            params.push(("ax", (first_pivot[0] - root_body.com[0]) * SCALE));
            params.push(("ay", (first_pivot[1] - root_body.com[1]) * SCALE));
            let mut links = Vec::new();
            let mut previous_angle = 0.0;
            for (li, joint) in spec.iter().enumerate() {
                // Everything from this joint outward, about this pivot.
                let mut outboard = 0.0;
                for later in &spec[li..] {
                    if let Some(b) = model.body(&later.child) {
                        let d = ((b.com[0] - joint.pivot2[0]).hypot(b.com[1] - joint.pivot2[1])) * SCALE;
                        outboard += b.inertia_zz.max(1.0e-7) + b.mass_kg * d * d;
                    }
                }
                let omega = 2.0 * std::f64::consts::PI * bandwidth_hz;
                let rotor = joint.motor.as_ref().map(|m| m.rotor_inertia).unwrap_or(0.0);
                let kp = (outboard + rotor) * omega * omega;
                let kd = 2.0 * damping_ratio * (kp * (outboard + rotor)).sqrt();
                gains.push((kp, kd));
                let (lower, upper) = match &joint.limits {
                    Some(l) if l.len() == 2 => (l[0], l[1]),
                    _ => (None, None),
                };
                let (stall, speed) = joint.motor.as_ref().map(|m| (m.stall_torque, m.no_load_speed)).unwrap_or((f64::INFINITY, f64::INFINITY));
                caps.push((lower, upper, stall, speed, joint.damping, joint.axis_sign));
                let child = model.body(&joint.child).ok_or_else(|| format!("joint {} names an unknown body {}", joint.name, joint.child))?;
                let body_index = model.bodies.iter().position(|b| b.name == joint.child).unwrap();
                let pivot = joint.pivot2;
                // The link runs from this pivot to the next pivot, or to the outline point farthest from the pivot.
                let end = if li + 1 < spec.len() {
                    spec[li + 1].pivot2
                } else {
                    child
                        .outline
                        .iter()
                        .flatten()
                        .copied()
                        .max_by(|a, b| ((a[0] - pivot[0]).hypot(a[1] - pivot[1])).total_cmp(&(b[0] - pivot[0]).hypot(b[1] - pivot[1])))
                        .unwrap_or([child.com[0] * 2.0 - pivot[0], child.com[1] * 2.0 - pivot[1]])
                };
                let length = ((end[0] - pivot[0]).hypot(end[1] - pivot[1])).max(1.0) * SCALE;
                let abs_angle = (end[1] - pivot[1]).atan2(end[0] - pivot[0]);
                let rel_angle = abs_angle - previous_angle;
                previous_angle = abs_angle;
                let along = ((child.com[0] - pivot[0]) * abs_angle.cos() + (child.com[1] - pivot[1]) * abs_angle.sin()) * SCALE;
                let name: &'static str = Box::leak(joint.name.clone().into_boxed_str());
                params.push((Box::leak(format!("joint.{name}").into_boxed_str()), li as f64));
                params.push((Box::leak(format!("link{li}.length").into_boxed_str()), length));
                params.push((Box::leak(format!("link{li}.mass").into_boxed_str()), child.mass_kg.max(1.0e-4)));
                params.push((Box::leak(format!("link{li}.com").into_boxed_str()), along.clamp(0.0, length)));
                params.push((Box::leak(format!("link{li}.inertia").into_boxed_str()), child.inertia_zz.max(1.0e-7)));
                params.push((Box::leak(format!("initial.joint.{name}.angle").into_boxed_str()), rel_angle));
                links.push(LinkInfo { body: body_index, joint_name: joint.name.clone(), length, cad_angle: abs_angle, pivot_cad: pivot });
                joint_names.push(joint.name.clone());
                seam_params.push((Box::leak(format!("sense.{name}.angle").into_boxed_str()), 0.0));
                seam_params.push((Box::leak(format!("sense.{name}.speed").into_boxed_str()), 0.0));
                seam_params.push((Box::leak(format!("act.{name}.torque").into_boxed_str()), 0.0));
            }
            let chain = m.part(registry, &format!("chain{ci}"), CHAIN, params).unwrap();
            body_ports.push(chain.port("base"));
            let tip = m.part(registry, &format!("chain{ci}.tip"), ct::POINT_PLANE_COMPLIANT, [("friction", 0.8), ("stiffness", 2.0e5), ("damping", 2.0e3)]).unwrap();
            m.connect([chain.port("tip"), tip.port("frame")]);
            let joint_ports: Vec<(String, sim_core::PortId)> = spec.iter().map(|j| (j.name.clone(), chain.port(Box::leak(format!("joint.{}", j.name).into_boxed_str())))).collect();
            wiring.push((chain, joint_ports));
            chains_built.push(links);
        }
        m.connect(body_ports);
        let seam = if joint_names.is_empty() { None } else { Some(m.part(registry, "controller", EXTERNAL, seam_params).unwrap()) };
        let mut across_ids: Vec<Vec<sim_core::PortId>> = Vec::new();
        for (chain, joint_ports) in &wiring {
            let mut ids = Vec::new();
            for (name, port) in joint_ports {
                let servo = m.part(registry, &format!("{name}.servo"), sense::SERVO, [("bandwidth", 100.0), ("torque_limit", 50.0)]).unwrap();
                let encoder = m.part(registry, &format!("{name}.encoder"), sense::ENCODER, []).unwrap();
                let tacho = m.part(registry, &format!("{name}.tacho"), sense::TACHOMETER, []).unwrap();
                m.connect([*port, servo.port("shaft"), encoder.port("shaft"), tacho.port("shaft")]);
                let seam = seam.as_ref().unwrap();
                m.connect([encoder.port("angle"), seam.port(Box::leak(format!("sense.{name}.angle").into_boxed_str()))]);
                m.connect([tacho.port("speed"), seam.port(Box::leak(format!("sense.{name}.speed").into_boxed_str()))]);
                m.connect([seam.port(Box::leak(format!("act.{name}.torque").into_boxed_str())), servo.port("command")]);
                m.connect([servo.port("current")]);
                ids.push(*port);
            }
            let _ = chain;
            across_ids.push(ids);
        }
        let mut runtime = damped_runtime(m, registry);
        let root_ids = ["x", "y", "theta", "vx", "vy", "omega"].map(|n| runtime.state_id(body.behavior, n));
        let mut chains = Vec::new();
        for ((chain, _), (links, ids)) in wiring.iter().zip(chains_built.into_iter().zip(across_ids)) {
            let angles = ids.iter().map(|p| runtime.across_id(*p)).collect();
            let tip = [runtime.state_id(chain.behavior, "tip.x"), runtime.state_id(chain.behavior, "tip.y")];
            chains.push(ChainInfo { behavior: chain.behavior, links, angles, tip });
        }
        // The PD loop on the seam holds every joint at its target.
        let initial: Vec<f64> = chains.iter().flat_map(|c| c.angles.iter().map(|id| runtime.get(*id))).collect();
        let targets = Arc::new(Mutex::new(initial.clone()));
        if let Some(seam) = &seam {
            let contract = runtime.contract(seam.behavior);
            let index = |names: &[sim_core::Channel], name: &str| names.iter().position(|c| c.name == name).expect("seam channel");
            let angle: Vec<usize> = joint_names.iter().map(|n| index(&contract.sensors, &format!("{n}.angle"))).collect();
            let speed: Vec<usize> = joint_names.iter().map(|n| index(&contract.sensors, &format!("{n}.speed"))).collect();
            let torque: Vec<usize> = joint_names.iter().map(|n| index(&contract.actuators, &format!("{n}.torque"))).collect();
            let held = targets.clone();
            let gains = gains.clone();
            let caps = caps.clone();
            let home: Vec<f64> = initial.clone();
            runtime
                .attach(
                    seam.behavior,
                    Box::new(FnCoupler(move |_t: f64, s: &[f64], a: &mut [f64]| {
                        let t = held.lock().unwrap_or_else(|p| p.into_inner());
                        for j in 0..angle.len() {
                            let (kp, kd) = gains[j];
                            let (lower, upper, stall, no_load, damping, sign) = caps[j];
                            let q = s[angle[j]];
                            let w = s[speed[j]];
                            // Limits are given in the joint's own sense; the chain angle
                            // is measured from the CAD pose along the plane normal.
                            let edges = match (lower, upper) {
                                (Some(lo), Some(hi)) => {
                                    let (a, b) = (home[j] + sign * lo, home[j] + sign * hi);
                                    (Some(a.min(b)), Some(a.max(b)))
                                }
                                (Some(lo), None) => if sign > 0.0 { (Some(home[j] + lo), None) } else { (None, Some(home[j] - lo)) },
                                (None, Some(hi)) => if sign > 0.0 { (None, Some(home[j] + hi)) } else { (Some(home[j] - hi), None) },
                                _ => (None, None),
                            };
                            let target = match edges {
                                (Some(a), Some(b)) => t[j].clamp(a, b),
                                (Some(a), None) => t[j].max(a),
                                (None, Some(b)) => t[j].min(b),
                                _ => t[j],
                            };
                            let mut torque_cmd = kp * (target - q) - (kd + damping) * w;
                            // A DC motor's torque falls linearly with speed toward no-load.
                            if stall.is_finite() {
                                let available = stall * (1.0 - (w.abs() / no_load).min(1.0)).max(0.0);
                                let limit = if w * torque_cmd > 0.0 { available } else { stall };
                                torque_cmd = torque_cmd.clamp(-limit, limit);
                            }
                            // Soft end stops: a stiff spring past the limit.
                            let stop = 10.0 * kp;
                            if let Some(edge) = edges.0 {
                                if q < edge { torque_cmd += stop * (edge - q); }
                            }
                            if let Some(edge) = edges.1 {
                                if q > edge { torque_cmd += stop * (edge - q); }
                            }
                            a[torque[j]] = torque_cmd.clamp(-500.0, 500.0);
                        }
                    })),
                )
                .map_err(|e| e.to_string())?;
        }
        for w in &warnings {
            eprintln!("cad model: {w}");
        }
        Ok(Self { runtime, model, root, root_fixed, root_ids, chains, targets, joint_names, seam: seam.map(|s| s.behavior), floor_mm, scale: SCALE, warnings })
    }

    pub fn advance(&mut self, duration: f64) -> Result<(), String> {
        self.runtime.advance(duration, 1.0e-3).map_err(|e| e.to_string())
    }

    /// World pose (x, y in m, θ) of every CAD body, root first then per link.
    pub fn poses(&self) -> Vec<(usize, [f64; 2], f64)> {
        let rt = &self.runtime;
        let root = [rt.get(self.root_ids[0]), rt.get(self.root_ids[1])];
        let theta = rt.get(self.root_ids[2]);
        let mut out = vec![(self.root, root, theta)];
        let root_com = self.model.bodies[self.root].com;
        for chain in &self.chains {
            let first = chain.links[0].pivot_cad;
            let (c, s) = (theta.cos(), theta.sin());
            let off = [(first[0] - root_com[0]) * SCALE, (first[1] - root_com[1]) * SCALE];
            let mut pivot = [root[0] + c * off[0] - s * off[1], root[1] + s * off[0] + c * off[1]];
            let mut phi = theta;
            for (li, link) in chain.links.iter().enumerate() {
                phi += rt.get(chain.angles[li]);
                // The body's CAD frame is rotated by (phi − cad_angle) about its pivot.
                let rot = phi - link.cad_angle;
                let (rc, rs) = (rot.cos(), rot.sin());
                let com_off = [(self.model.bodies[link.body].com[0] - link.pivot_cad[0]) * SCALE, (self.model.bodies[link.body].com[1] - link.pivot_cad[1]) * SCALE];
                let com = [pivot[0] + rc * com_off[0] - rs * com_off[1], pivot[1] + rs * com_off[0] + rc * com_off[1]];
                out.push((link.body, com, rot));
                pivot = [pivot[0] + link.length * phi.cos(), pivot[1] + link.length * phi.sin()];
            }
        }
        out
    }

    /// Outline polylines of every body in world metres, for drawing.
    pub fn outlines(&self) -> Vec<(usize, Vec<[f64; 2]>)> {
        let mut out = Vec::new();
        for (bi, com, rot) in self.poses() {
            let b = &self.model.bodies[bi];
            let (c, s) = (rot.cos(), rot.sin());
            for loop_ in &b.outline {
                let pts = loop_
                    .iter()
                    .map(|p| {
                        let d = [(p[0] - b.com[0]) * SCALE, (p[1] - b.com[1]) * SCALE];
                        [com[0] + c * d[0] - s * d[1], com[1] + s * d[0] + c * d[1]]
                    })
                    .collect();
                out.push((bi, pts));
            }
            if b.outline.is_empty() {
                // No section through the COM: draw the bbox in the plane.
                if b.bbox.len() == 2 {
                    let (lo, hi) = (&b.bbox[0], &b.bbox[1]);
                    let corners = [[lo[0], lo[2]], [hi[0], lo[2]], [hi[0], hi[2]], [lo[0], hi[2]], [lo[0], lo[2]]];
                    let pts = corners
                        .iter()
                        .map(|p| {
                            let d = [(p[0] - b.com[0]) * SCALE, (p[1] - b.com[1]) * SCALE];
                            [com[0] + c * d[0] - s * d[1], com[1] + s * d[0] + c * d[1]]
                        })
                        .collect();
                    out.push((bi, pts));
                }
            }
        }
        out
    }

    pub fn set_target(&self, joint: usize, angle: f64) {
        let mut t = self.targets.lock().unwrap_or_else(|p| p.into_inner());
        if joint < t.len() {
            t[joint] = angle;
        }
    }

    pub fn joint_angles(&self) -> Vec<f64> {
        self.chains.iter().flat_map(|c| c.angles.iter().map(|id| self.runtime.get(*id))).collect()
    }
}

/// Either generation of exported model: the planar summary (v2) or the
/// physical description (v3).
pub enum AnyRobot {
    Planar(CadRobot),
    Physical(PhysicalRobot),
}

/// The `version` field of an exported file (2 when absent).
pub fn file_version(path: &str) -> Result<u32, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
    Ok(value.get("version").and_then(|v| v.as_u64()).unwrap_or(2) as u32)
}

impl AnyRobot {
    pub fn load(path: &str, opts: &BuildOptions) -> Result<Self, String> {
        let registry = registry();
        if file_version(path)? >= 3 {
            let model = PhysicalModel::load(path)?;
            Ok(Self::Physical(PhysicalRobot::build(model, &registry, opts)?))
        } else {
            let model = CadModel::load(path)?;
            Ok(Self::Planar(CadRobot::build(model, &registry, 6.0, 1.0)?))
        }
    }
    pub fn advance(&mut self, duration: f64) -> Result<(), String> {
        match self {
            Self::Planar(r) => r.advance(duration),
            Self::Physical(r) => r.advance(duration),
        }
    }
    pub fn time(&self) -> f64 {
        match self {
            Self::Planar(r) => r.runtime.time,
            Self::Physical(r) => r.runtime.time,
        }
    }
    pub fn joint_names(&self) -> Vec<String> {
        match self {
            Self::Planar(r) => r.joint_names.clone(),
            Self::Physical(r) => r.joint_names.iter().map(|n| n.trim_start_matches("joint.").trim_start_matches("slide.").to_owned()).collect(),
        }
    }
    pub fn joint_angles(&self) -> Vec<f64> {
        match self {
            Self::Planar(r) => r.joint_angles(),
            Self::Physical(r) => r.joint_angles(),
        }
    }
    pub fn targets(&self) -> Vec<f64> {
        match self {
            Self::Planar(r) => r.targets.lock().unwrap_or_else(|p| p.into_inner()).clone(),
            Self::Physical(r) => r.targets.lock().unwrap_or_else(|p| p.into_inner()).clone(),
        }
    }
    pub fn set_target(&self, joint: usize, angle: f64) {
        match self {
            Self::Planar(r) => r.set_target(joint, angle),
            Self::Physical(r) => r.set_target(joint, angle),
        }
    }
}

/// Headless: load, build, hold the pose for `seconds`, report. A v3 file
/// goes through the physical builder and writes its results beside it.
pub fn run_file(path: &str, seconds: f64) -> Result<String, String> {
    if file_version(path)? >= 3 {
        return run_physical(path, seconds, &BuildOptions::default(), None);
    }
    let model = CadModel::load(path)?;
    let registry = registry();
    let mut robot = CadRobot::build(model, &registry, 6.0, 1.0)?;
    let mut lines = vec![format!("{} bodies, {} joints, root `{}`{}", robot.model.bodies.len(), robot.joint_names.len(), robot.model.bodies[robot.root].name, if robot.root_fixed { " (fixed)" } else { "" })];
    let steps = (seconds / 0.1).ceil() as usize;
    for k in 0..=steps {
        if k > 0 {
            robot.advance(0.1)?;
        }
        let rt = &robot.runtime;
        let root = [rt.get(robot.root_ids[0]), rt.get(robot.root_ids[1]), rt.get(robot.root_ids[2])];
        let joints: Vec<String> = robot.joint_names.iter().zip(robot.joint_angles()).map(|(n, a)| format!("{n}={:.3}", a)).collect();
        lines.push(format!("t={:.1}s root=({:.4}, {:.4}, {:.3}) {}", rt.time, root[0], root[1], root[2], joints.join(" ")));
    }
    Ok(lines.join("\n"))
}
