//! `robot.articulated`: the whole robot as one element — a floating (or
//! grounded) base, tree joints in minimal coordinates, loop-closing
//! constraints, modal flexibility per link, and contact computed from the
//! links' own geometry against the floor and each other.
//!
//! The equations are inverse dynamics in residual form, as
//! `multibody.chain` does in the plane: given the states and their rates
//! (base twist and its rate, joint speeds and their rates, modal
//! coordinates), a forward pass gives every link's pose, twist and
//! acceleration, a backward Newton–Euler pass gives the force and moment
//! each joint transmits, and each joint's port absorbs the torque its
//! motion demands beyond what its own friction, stops and springs supply.
//!
//! Ports: `frame.base` (owned), `joint.<name>` (rotational; ball joints
//! expose `joint.<name>.x/.y/.z`), `slide.<name>` (translational for
//! prismatic joints), `temperature.<link>` signal inputs (mount temperature
//! softening a flexible link), `contact.<link>` signal outputs (normal
//! force sum) and `imu.<sensor>.<ax|ay|az|gx|gy|gz>` signal outputs
//! (sampled inertial sensors).
//!
//! Parameters: `model` (handle from [`crate::register_model`]), `gravity`
//! multiplier (default 1), `planar` (1 confines the base to the model's
//! planar hint with a stiff penalty), `flex` (0 disables modal
//! flexibility), `contact` (0 disables geometry contact), `loop.alpha`
//! (Baumgarte rate, default 100), `initial.joint.<name>.angle/.speed`.

use crate::math::{frame_from_z, m3, quat, quat_rate, rot_axis, rot_vec, v, M, V};
use crate::model::{Flex, Friction, PhysicalModel, Sdf, Terrain};
use crate::sdf::{contact_vertices, keyed_normal, local_bounds};
use nalgebra::{UnitQuaternion, Vector3};
use sim_core::{
    acausal, param, param_or, signal_in, signal_out, Behavior, BehaviorDescriptor, BehaviorRegistry, ConnectorKind, Context, Provision, QuantityKind, RegistryError, StateDeclaration, View,
};
use std::collections::BTreeMap;
use std::sync::Arc;

pub const ARTICULATED: &str = "robot.articulated";

type Params = BTreeMap<String, f64>;

pub const BASE_STATES: usize = 13;
/// Contact vertices kept per link.
pub const CONTACT_VERTICES: usize = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DofKind {
    Revolute,
    Prismatic,
}

/// One degree of freedom of a joint: an axis in the joint frame, either a
/// port on the node (the angle is the node's across value) or an internal
/// state (compliant fixed joints).
#[derive(Clone, Debug)]
pub struct Dof {
    pub name: String,
    pub kind: DofKind,
    pub axis: usize,
    /// Port index in the element's port list, when this DOF has one.
    pub port: Option<usize>,
    pub q_state: Option<usize>,
    pub qd_state: usize,
    pub lower: Option<f64>,
    pub upper: Option<f64>,
    pub home: f64,
    pub friction: Friction,
    /// Spring to rest (compliant fixed joints): `(k, c)`.
    pub spring: Option<(f64, f64)>,
    pub stop_k: f64,
    pub stop_c: f64,
    pub initial_angle: f64,
    pub initial_speed: f64,
}

#[derive(Clone, Debug)]
pub struct JointC {
    pub name: String,
    pub parent: usize,
    pub child: usize,
    /// Joint origin from the parent's COM, parent frame.
    pub r_pj: V,
    /// Joint frame orientation in the parent frame (export pose).
    pub r_j: M,
    /// Child COM from the joint origin, joint frame.
    pub r_jc: V,
    /// Child orientation relative to the joint frame.
    pub r_jc_rot: M,
    pub dofs: Vec<Dof>,
    /// Which of the parent's flex boundary frames this joint rides on.
    pub flex_boundary: Option<usize>,
    /// Radius around the joint origin where parent/child contact is ignored.
    pub band: f64,
    pub pin_radius: f64,
    pub contact_length: f64,
    pub allowable_pressure: f64,
    pub shear_capacity: Option<f64>,
    pub is_fixed: bool,
}

#[derive(Clone, Debug)]
pub struct FlexC {
    pub normalization: crate::model::ModalNormalization,
    pub modes: usize,
    pub mass: Vec<f64>,
    pub stiffness: Vec<f64>,
    pub damping: Vec<f64>,
    pub boundary_points: Vec<V>,
    /// `[mode][boundary][6]` relative to the inboard boundary.
    pub shapes: Vec<Vec<[f64; 6]>>,
    pub participation: Vec<[f64; 6]>,
    pub state: usize,
    pub temperature_signal: Option<usize>,
    pub softening: crate::model::Softening,
    pub stress_cells: Vec<V>,
    pub stress_per_mode: Vec<Vec<[f64; 6]>>,
}

#[derive(Clone, Debug)]
pub struct LinkC {
    pub name: String,
    pub mass: f64,
    pub inertia: M,
    pub com0: V,
    pub material: String,
    pub yield_strength: f64,
    pub contact: Vec<V>,
    pub lo: V,
    pub hi: V,
    pub sdf: Option<Sdf>,
    pub flex: Option<FlexC>,
    pub parent_joint: Option<usize>,
    pub children: Vec<usize>,
    pub bristle_state: usize,
    pub contact_signal: Option<usize>,
    pub floor_mu: (f64, f64),
    /// Per other link: contact vertices that already sit inside it at the
    /// export pose (a pin in its hole, a shaft in its bore): assembly, not collision.
    pub excluded: BTreeMap<usize, Vec<bool>>,
    pub grounded: bool,
}

#[derive(Clone, Debug)]
pub struct LoopC {
    pub name: String,
    pub a: usize,
    pub b: usize,
    pub r_a: V,
    pub r_b: V,
    /// Axis rows: `(e1, e2)` in a's frame and the axis in b's frame.
    pub axis: Option<(V, V, V)>,
    pub lambda_state: usize,
    pub rows: usize,
}

#[derive(Clone, Debug)]
pub struct TransmissionC {
    pub name: String,
    pub driver: usize,
    pub driven: usize,
    pub ratio: f64,
    pub lambda_state: usize,
}

#[derive(Clone, Debug)]
pub struct ImuC {
    pub name: String,
    pub link: usize,
    pub point: V,
    pub axes: M,
    pub period: f64,
    pub latency: f64,
    pub noise: [f64; 2],
    pub bias0: [f64; 6],
    pub bias_walk: f64,
    pub quant: [f64; 2],
    pub range: [f64; 2],
    pub state: usize,
    pub signals: [usize; 6],
    pub stream: u64,
}

/// A tree root: the ground link (pinned) or a floating body.
#[derive(Clone, Debug)]
pub struct BaseC {
    pub link: usize,
    pub state: usize,
    pub grounded: bool,
    pub p0: V,
}

#[derive(Clone, Debug)]
pub struct CableC {
    pub name: String,
    pub a: usize,
    pub r_a: V,
    pub b: usize,
    pub r_b: V,
    pub length: f64,
    pub mass: f64,
    pub stiffness: f64,
    pub damping: f64,
}

/// The compiled robot; also usable outside the runtime for kinematics and
/// reporting (`evaluate`).
#[derive(Clone, Debug)]
pub struct Articulated {
    pub model: Arc<PhysicalModel>,
    pub links: Vec<LinkC>,
    /// Tree joints in forward order (parents before children).
    pub joints: Vec<JointC>,
    pub loops: Vec<LoopC>,
    pub transmissions: Vec<TransmissionC>,
    pub imus: Vec<ImuC>,
    pub cables: Vec<CableC>,
    pub root: usize,
    pub grounded: bool,
    pub bases: Vec<BaseC>,
    pub gravity: V,
    pub base0: (V, UnitQuaternion<f64>),
    pub initial_twist: [f64; 6],
    pub planar: Option<(V, V)>,
    pub contact_on: bool,
    pub loop_alpha: f64,
    /// Constraint-force mixing: a tiny compliance that keeps redundant loop rows solvable.
    pub loop_cfm: f64,
    pub loop_angular_cfm: f64,
    pub floor_k: f64,
    pub floor_c: f64,
    /// Hunt–Crossley factor (s/m): `F = k·d·(1 − α·v_separation)`.
    pub restitution_damping: f64,
    pub terrain: Option<Terrain>,
    pub floor_z: f64,
    pub state_count: usize,
    pub warnings: Vec<String>,
    /// Port index of every DOF, by port name.
    pub port_names: Vec<String>,
    pub signal_out_names: Vec<String>,
    pub signal_in_names: Vec<String>,
    pub ambient_c: f64,
    pub link_k: f64,
}

/// Everything read from states, rates and ports for one evaluation.
#[derive(Clone, Debug, Default)]
pub struct Generalized {
    pub states: Vec<f64>,
    pub rates: Vec<f64>,
    /// Angle (or slide) of each DOF, by DOF order over all joints.
    pub q: Vec<f64>,
    pub qd: Vec<f64>,
    pub qdd: Vec<f64>,
    pub temperatures: Vec<f64>,
}

#[derive(Clone, Debug)]
pub struct LinkKin {
    pub r: M,
    pub p: V,
    pub w: V,
    pub vel: V,
    pub alpha: V,
    pub acc: V,
}

#[derive(Clone, Debug, Default)]
pub struct JointReaction {
    /// Force transmitted to the child (world).
    pub f: V,
    /// Moment about the joint point (world).
    pub n: V,
    pub point: V,
    /// Axis in world and the torque/force each DOF must supply.
    pub axes: Vec<V>,
    pub tau_needed: Vec<f64>,
    pub tau_passive: Vec<f64>,
}

#[derive(Clone, Debug)]
pub struct ContactPoint {
    pub link: usize,
    pub other: Option<usize>,
    pub point: V,
    pub force: V,
    pub penetration: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Evaluation {
    pub links: Vec<LinkKin>,
    pub joints: Vec<JointReaction>,
    pub contacts: Vec<ContactPoint>,
    pub base_wrench: Vec<[f64; 6]>,
    /// Per link, per mode: the modal force.
    pub modal_force: Vec<Vec<f64>>,
    pub loop_rows: Vec<f64>,
    pub bristle_rates: Vec<f64>,
    pub contact_normal: Vec<f64>,
    pub joint_points: Vec<V>,
}

/// Build-time options.
#[derive(Clone, Debug)]
pub struct Options {
    pub gravity_scale: f64,
    pub planar: bool,
    pub flex: bool,
    pub contact: bool,
    pub loop_alpha: f64,
    pub loop_cfm: f64,
    pub loop_angular_cfm: f64,
    /// Modes kept per flexible link (the lowest ones), and a frequency cap.
    pub flex_modes: usize,
    pub flex_max_hz: f64,
    pub initial_angles: BTreeMap<String, f64>,
    pub initial_speeds: BTreeMap<String, f64>,
    /// Base twist at t = 0: `vx, vy, vz, wx, wy, wz`.
    pub initial_twist: [f64; 6],
    /// Base position offset at t = 0 (added to the root link's export COM).
    pub initial_offset: [f64; 3],
}
impl Default for Options {
    fn default() -> Self {
        Self { gravity_scale: 1.0, planar: false, flex: true, contact: true, loop_alpha: 100.0, loop_cfm: 1e-6, loop_angular_cfm: 1e-6, flex_modes: 4, flex_max_hz: 500.0, initial_angles: BTreeMap::new(), initial_speeds: BTreeMap::new(), initial_twist: [0.0; 6], initial_offset: [0.0; 3] }
    }
}

fn smooth_sign(x: f64, eps: f64) -> f64 {
    (x / eps).tanh()
}

/// Friction torque opposing `qd` (returned with its sign).
pub fn friction_torque(f: &Friction, qd: f64) -> f64 {
    let vs = f.stribeck_speed.max(1e-6);
    let e = (-(qd / vs).powi(2)).exp();
    let magnitude = f.coulomb * (1.0 + (f.static_ratio - 1.0) * e) + f.stribeck * e;
    -(magnitude * smooth_sign(qd, 1e-3) + f.viscous * qd)
}

impl Articulated {
    pub fn new(model: Arc<PhysicalModel>, opts: &Options) -> Result<Self, String> {
        if model.links.is_empty() {
            return Err("the model has no links".into());
        }
        let mut warnings = Vec::new();
        let n = model.links.len();
        // Root: the ground link, else the heaviest link nobody's child.
        let children: Vec<&str> = model.joints.iter().filter(|j| !j.is_loop()).map(|j| j.child.as_str()).collect();
        let root = model.links.iter().position(|l| l.ground).unwrap_or_else(|| {
            let mut best = None::<usize>;
            for (i, l) in model.links.iter().enumerate() {
                if children.contains(&l.name.as_str()) {
                    continue;
                }
                if best.map(|b| l.mass > model.links[b].mass).unwrap_or(true) {
                    best = Some(i);
                }
            }
            best.unwrap_or(0)
        });
        let grounded = model.links[root].ground;
        // Links.
        let mut links: Vec<LinkC> = Vec::with_capacity(n);
        for l in &model.links {
            let mat = model.material_of(l);
            let (lo, hi) = local_bounds(l);
            let contact = contact_vertices(&l.collision, CONTACT_VERTICES);
            let sdf = l.collision.sdf.clone().filter(|s| s.is_valid());
            links.push(LinkC {
                name: l.name.clone(),
                mass: l.mass.max(1e-6),
                inertia: {
                    let i = m3(l.inertia);
                    // Guard against a degenerate tensor.
                    let floor = 1e-9 * l.mass.max(1e-6);
                    M::new(i[(0, 0)].max(floor), i[(0, 1)], i[(0, 2)], i[(1, 0)], i[(1, 1)].max(floor), i[(1, 2)], i[(2, 0)], i[(2, 1)], i[(2, 2)].max(floor))
                },
                com0: v(l.com),
                material: l.material.clone(),
                yield_strength: mat.yield_strength,
                contact,
                lo,
                hi,
                sdf,
                flex: None,
                parent_joint: None,
                children: Vec::new(),
                bristle_state: 0,
                contact_signal: None,
                floor_mu: model.friction_between(&l.material, "world"),
                excluded: BTreeMap::new(),
                grounded: l.ground,
            });
        }
        // Vertices inside another link at the export pose belong to the
        // assembly (pins in holes, shafts in bores, overlapping solids in a
        // quick CAD model) and never collide with that link.
        for i in 0..n {
            for j in 0..n {
                if i == j {
                    continue;
                }
                let Some(sdf) = &links[j].sdf else { continue };
                let flags: Vec<bool> = links[i].contact.iter().map(|c| sdf.sample(links[i].com0 + c - links[j].com0).0 < 0.0).collect();
                if flags.iter().any(|f| *f) {
                    links[i].excluded.insert(j, flags);
                }
            }
        }
        // Tree order by BFS from every root over tree joints: the ground
        // link (or heaviest orphan) first, then every other link nobody's
        // child — loose parts and a robot standing on a separate ground link.
        let mut order: Vec<usize> = Vec::new(); // joint indices into model.joints
        let mut visited = vec![false; n];
        let mut roots = vec![root];
        let bfs = |start: usize, order: &mut Vec<usize>, visited: &mut Vec<bool>, warnings: &mut Vec<String>| {
            visited[start] = true;
            let mut frontier = vec![start];
            while let Some(li) = frontier.pop() {
                let name = model.links[li].name.clone();
                let mut outgoing: Vec<usize> = model
                    .joints
                    .iter()
                    .enumerate()
                    .filter(|(_, j)| !j.is_loop() && j.parent.as_deref().map(|p| p == name).unwrap_or(li == root && j.parent.is_none()))
                    .map(|(k, _)| k)
                    .collect();
                outgoing.sort();
                for k in outgoing {
                    let j = &model.joints[k];
                    let Some(ci) = model.link_index(&j.child) else {
                        warnings.push(format!("joint {} names an unknown child {}", j.name, j.child));
                        continue;
                    };
                    if visited[ci] {
                        warnings.push(format!("joint {}: {} already has a parent; the joint is ignored (declare it as loop_revolute to close a loop)", j.name, j.child));
                        continue;
                    }
                    visited[ci] = true;
                    order.push(k);
                    frontier.insert(0, ci);
                }
            }
        };
        bfs(root, &mut order, &mut visited, &mut warnings);
        loop {
            let next = (0..n).find(|&i| !visited[i] && !children.contains(&model.links[i].name.as_str()));
            let Some(r) = next else { break };
            roots.push(r);
            bfs(r, &mut order, &mut visited, &mut warnings);
        }
        for (i, l) in model.links.iter().enumerate() {
            if !visited[i] {
                warnings.push(format!("link {} is only reachable through a joint whose parent is missing; it is left out", l.name));
            }
        }
        // Joints with world parent attach to the root.
        let mut joints: Vec<JointC> = Vec::new();
        let mut ports: Vec<String> = Vec::new();
        for &k in &order {
            let j = &model.joints[k];
            let parent = j.parent.as_deref().and_then(|p| model.link_index(p)).unwrap_or(root);
            let child = model.link_index(&j.child).unwrap();
            let axis = {
                let a = v(j.axis);
                if a.norm() < 1e-9 { Vector3::z() } else { a.normalize() }
            };
            let r_j = frame_from_z(axis);
            let origin = v(j.origin);
            let r_pj = origin - links[parent].com0;
            let r_jc = r_j.transpose() * (links[child].com0 - origin);
            let outboard_mass = subtree_mass(&model, child);
            let lever = (links[child].com0 - origin).norm().max(0.01);
            let i_out = outboard_mass * lever * lever + m3(model.links[child].inertia)[(2, 2)].abs();
            let stop_k = (1.0e4 * i_out).max(0.5);
            let stop_c = 2.0 * (stop_k * i_out).sqrt();
            let mut dofs = Vec::new();
            let mut mk = |name: String, kind: DofKind, axis: usize, with_port: bool, lower: Option<f64>, upper: Option<f64>, spring: Option<(f64, f64)>| {
                let port = if with_port {
                    ports.push(name.clone());
                    Some(ports.len() - 1)
                } else {
                    None
                };
                let initial_angle = opts.initial_angles.get(&name).copied().unwrap_or(0.0);
                let initial_speed = opts.initial_speeds.get(&name).copied().unwrap_or(0.0);
                dofs.push(Dof { name, kind, axis, port, q_state: None, qd_state: 0, lower, upper, home: j.home, friction: j.physics.friction.clone(), spring, stop_k, stop_c, initial_angle, initial_speed });
            };
            let limits = j.limits.map(|l| (Some(l[0].min(l[1])), Some(l[0].max(l[1])))).unwrap_or((None, None));
            match j.kind.as_str() {
                "revolute" => mk(format!("joint.{}", j.name), DofKind::Revolute, 2, true, limits.0, limits.1, None),
                "continuous" => mk(format!("joint.{}", j.name), DofKind::Revolute, 2, true, None, None, None),
                "prismatic" => mk(format!("slide.{}", j.name), DofKind::Prismatic, 2, true, limits.0, limits.1, None),
                "ball" => {
                    for (a, s) in ["x", "y", "z"].iter().enumerate() {
                        mk(format!("joint.{}.{s}", j.name), DofKind::Revolute, a, true, None, None, None);
                    }
                }
                "fixed" => {
                    let st = &j.physics.stiffness;
                    let fast = j.fastened.as_ref().map(|f| f.stiffness * f.count.max(1.0)).unwrap_or(0.0);
                    let k_lin = [st.radial.max(fast), st.radial.max(fast), st.axial.max(fast)];
                    let k_rot = j.fastened.as_ref().map(|f| (f.stiffness * f.count.max(1.0) * f.pattern_radius.powi(2)).max(st.bending)).unwrap_or(st.bending);
                    let m_c = links[child].mass;
                    let i_c = links[child].inertia[(2, 2)].abs().max(1e-9);
                    let z = j.physics.damping_ratio.max(0.02);
                    for (a, s) in ["x", "y", "z"].iter().enumerate() {
                        let k = k_lin[a].max(1.0);
                        mk(format!("fixed.{}.t{s}", j.name), DofKind::Prismatic, a, false, None, None, Some((k, 2.0 * z * (k * m_c).sqrt())));
                    }
                    for (a, s) in ["x", "y", "z"].iter().enumerate() {
                        let k = k_rot.max(1e-3);
                        mk(format!("fixed.{}.r{s}", j.name), DofKind::Revolute, a, false, None, None, Some((k, 2.0 * z * (k * i_c).sqrt())));
                    }
                }
                other => {
                    warnings.push(format!("joint {}: unknown type {other}, treated as revolute", j.name));
                    mk(format!("joint.{}", j.name), DofKind::Revolute, 2, true, limits.0, limits.1, None);
                }
            }
            let band = (3.0 * j.physics.hole_radius.max(j.physics.pin_radius)).max(j.physics.contact_length).max(0.01);
            links[child].parent_joint = Some(joints.len());
            links[parent].children.push(joints.len());
            joints.push(JointC {
                name: j.name.clone(),
                parent,
                child,
                r_pj,
                r_j,
                r_jc,
                r_jc_rot: r_j.transpose(),
                dofs,
                flex_boundary: None,
                band,
                pin_radius: j.physics.pin_radius,
                contact_length: j.physics.contact_length,
                allowable_pressure: j.physics.bearing.allowable_pressure,
                shear_capacity: j.fastened.as_ref().map(|f| f.shear_capacity * f.count.max(1.0)),
                is_fixed: j.kind == "fixed",
            });
        }
        // State layout.
        let mut state = BASE_STATES;
        for jc in &mut joints {
            for d in &mut jc.dofs {
                if d.port.is_none() {
                    d.q_state = Some(state);
                    state += 1;
                }
                d.qd_state = state;
                state += 1;
            }
        }
        // Extra bases (loose parts) after the DOFs; base 0 owns `frame.base`.
        let mut bases = vec![BaseC { link: root, state: 0, grounded, p0: links[root].com0 + v(opts.initial_offset) }];
        for &r in roots.iter().skip(1) {
            bases.push(BaseC { link: r, state, grounded: model.links[r].ground, p0: links[r].com0 });
            state += BASE_STATES;
        }
        // Flex.
        let temp_names: Vec<String> = {
            let mut names: Vec<String> = model.links.iter().filter(|l| l.flex.is_some() && opts.flex).map(|l| l.name.clone()).collect();
            names.sort();
            names
        };
        if opts.flex {
            for (li, l) in model.links.iter().enumerate() {
                let Some(f) = &l.flex else { continue };
                if f.modes == 0 || f.boundary_shapes.len() < f.modes || f.participation.len() < f.modes {
                    if f.modes > 0 {
                        warnings.push(format!("link {}: flex block is incomplete and is ignored", l.name));
                    }
                    continue;
                }
                if f.normalization == crate::model::ModalNormalization::Unspecified {
                    warnings.push(format!("link {}: modal normalization is unspecified; amplitudes are reported in modal coordinates, not SI displacement", l.name));
                }
                let keep = (0..f.modes).filter(|&m| m < opts.flex_modes && f.frequencies_hz.get(m).map(|hz| *hz <= opts.flex_max_hz).unwrap_or(true)).count().max(1).min(f.modes);
                let flex = compile_flex(f, li, &joints, &links, state, temp_names.iter().position(|t| t == &l.name), keep);
                state += 2 * flex.modes;
                // Which boundary each outboard joint rides on.
                for (ji, jc) in joints.iter_mut().enumerate() {
                    if jc.parent != li {
                        continue;
                    }
                    let _ = ji;
                    let origin = jc.r_pj; // parent frame = link frame
                    let mut best = None::<(usize, f64)>;
                    for (bi, bf) in f.boundary_frames.iter().enumerate() {
                        let named = bf.name == jc.name;
                        let d = (v(bf.point) - origin).norm();
                        let score = if named { -1.0 } else { d };
                        if best.map(|(_, s)| score < s).unwrap_or(true) {
                            best = Some((bi, score));
                        }
                    }
                    if let Some((bi, score)) = best {
                        if score < 0.0 || score < 0.05 {
                            jc.flex_boundary = Some(bi);
                        }
                    }
                }
                links[li].flex = Some(flex);
            }
        }
        // Loops.
        let mut loops = Vec::new();
        for j in model.joints.iter().filter(|j| j.is_loop()) {
            let (Some(a), Some(b)) = (j.parent.as_deref().and_then(|p| model.link_index(p)), model.link_index(&j.child)) else {
                warnings.push(format!("loop joint {} names unknown links", j.name));
                continue;
            };
            let origin = v(j.origin);
            let axis = {
                let a = v(j.axis);
                if a.norm() < 1e-9 { Vector3::z() } else { a.normalize() }
            };
            let rows = if j.kind == "loop_revolute" { 5 } else { 3 };
            let r_j = frame_from_z(axis);
            let axis_rows = if rows == 5 { Some((r_j.column(0).into_owned(), r_j.column(1).into_owned(), axis)) } else { None };
            loops.push(LoopC { name: j.name.clone(), a, b, r_a: origin - links[a].com0, r_b: origin - links[b].com0, axis: axis_rows, lambda_state: state, rows });
            state += rows;
        }
        // Ideal belt/gear couplings act on joint coordinates, including shafts
        // with different axes. Their equal-and-opposite generalized work avoids
        // applying a gearbox ratio twice through the motor model.
        let coordinates: Vec<_> = joints.iter().flat_map(|j| j.dofs.iter().map(move |d| (j.name.as_str(), d))).collect();
        let mut transmissions = Vec::new();
        let mut connected: Vec<usize> = (0..coordinates.len()).collect();
        for t in &model.transmissions {
            if !t.ratio.is_finite() || t.ratio.abs() < 1e-12 { return Err(format!("{}: transmission ratio must be finite and nonzero", t.name)); }
            let index = |name: &str| -> Result<usize, String> {
                let found: Vec<_> = coordinates.iter().enumerate().filter(|(_, (n, _))| *n == name).collect();
                if found.len() != 1 || !matches!(found[0].1.1.kind, DofKind::Revolute) { return Err(format!("{}: transmission needs a single rotational coordinate on {name}", t.name)); }
                Ok(found[0].0)
            };
            let driver = index(&t.driver_joint)?;
            let driven = index(&t.driven_joint)?;
            let root_of = |mut i: usize, roots: &[usize]| { while roots[i] != i { i = roots[i]; } i };
            let a = root_of(driver, &connected); let b = root_of(driven, &connected);
            if a == b { return Err(format!("{}: redundant transmission cycle", t.name)); }
            connected[a] = b;
            transmissions.push(TransmissionC { name: t.name.clone(), driver, driven, ratio: t.ratio, lambda_state: state });
            state += 1;
        }
        // Floor stick: one bristle patch (x, y, twist) per moving link.
        for l in &mut links {
            l.bristle_state = state;
            if !l.grounded {
                state += 3;
            }
        }
        // Contact signals (sorted by link name) and IMU signals.
        let mut contact_names: Vec<(String, usize)> = links.iter().enumerate().map(|(i, l)| (l.name.clone(), i)).collect();
        contact_names.sort();
        let mut signal_out_names = Vec::new();
        for (k, (name, li)) in contact_names.iter().enumerate() {
            links[*li].contact_signal = Some(k);
            signal_out_names.push(format!("contact.{name}"));
        }
        let mut imu_specs: Vec<&crate::model::Sensor> = model.sensors.iter().filter(|s| s.kind == "imu").collect();
        imu_specs.sort_by(|a, b| a.name.cmp(&b.name));
        let mut imus = Vec::new();
        for (k, s) in imu_specs.iter().enumerate() {
            let Some(li) = model.link_index(&s.link) else {
                warnings.push(format!("sensor {} names an unknown link {}", s.name, s.link));
                continue;
            };
            let axes = m3(s.axes);
            let signal = signal_out_names.len();
            for c in ["ax", "ay", "az", "gx", "gy", "gz"] {
                signal_out_names.push(format!("imu.{}.{c}", s.name));
            }
            imus.push(ImuC {
                name: s.name.clone(),
                link: li,
                point: v(s.point),
                axes,
                period: 1.0 / s.rate_hz.max(1.0),
                latency: 0.0,
                noise: [s.noise.accel, s.noise.gyro],
                bias0: [s.bias.accel[0], s.bias.accel[1], s.bias.accel[2], s.bias.gyro[0], s.bias.gyro[1], s.bias.gyro[2]],
                bias_walk: s.bias_walk,
                quant: [s.quantization.accel, s.quantization.angle],
                range: [if s.range.accel > 0.0 { s.range.accel * 9.81 } else { f64::INFINITY }, if s.range.gyro > 0.0 { s.range.gyro } else { f64::INFINITY }],
                state,
                signals: std::array::from_fn(|c| signal + c),
                stream: (1000 + k as u64) ^ model.uncertainty.seed.wrapping_mul(0x9e3779b97f4a7c15),
            });
            state += 16;
        }
        // Cables.
        let mut cables = Vec::new();
        for c in &model.cables {
            let (Some(a), Some(b)) = (model.link_index(&c.from.link), model.link_index(&c.to.link)) else {
                warnings.push(format!("cable {} names unknown links", c.name));
                continue;
            };
            let pa = v(c.from.point);
            let pb = v(c.to.point);
            let rest = (links[b].com0 + pb - (links[a].com0 + pa)).norm();
            cables.push(CableC { name: c.name.clone(), a, r_a: pa, b, r_b: pb, length: if c.length > 0.0 { c.length } else { rest }, mass: c.mass, stiffness: c.stiffness, damping: c.damping });
        }
        let planar = if opts.planar { model.planar.as_ref().map(|p| (v(p.normal).normalize(), v(p.origin))) } else { None };
        let gravity = v(model.gravity) * opts.gravity_scale;
        let base0 = (bases[0].p0, UnitQuaternion::identity());
        let signal_in_names = temp_names.iter().map(|t| format!("temperature.{t}")).collect();
        Ok(Self {
            model: model.clone(),
            links,
            joints,
            loops,
            transmissions,
            imus,
            cables,
            root,
            grounded,
            bases,
            gravity,
            base0,
            initial_twist: opts.initial_twist,
            planar,
            contact_on: opts.contact,
            loop_alpha: opts.loop_alpha,
            loop_cfm: opts.loop_cfm,
            loop_angular_cfm: opts.loop_angular_cfm,
            floor_k: model.world.floor_stiffness,
            floor_c: model.world.floor_damping,
            restitution_damping: (model.world.floor_damping / model.world.floor_stiffness).clamp(0.2, 3.0),
            terrain: model.world.terrain.clone(),
            floor_z: model.world.floor_z,
            state_count: state,
            warnings,
            port_names: ports,
            signal_out_names,
            signal_in_names,
            ambient_c: model.world.ambient_c,
            link_k: model.world.floor_stiffness,
        }
        .with_port_order())
    }

    /// The parameters that create this element's ports and seed its joints.
    pub fn port_parameters(&self) -> Vec<(String, f64)> {
        let mut out = Vec::new();
        for (k, p) in self.port_names.iter().enumerate() {
            out.push((p.clone(), k as f64));
        }
        for s in &self.signal_in_names {
            out.push((s.clone(), 0.0));
        }
        for s in &self.signal_out_names {
            out.push((s.clone(), 0.0));
        }
        out
    }

    pub fn dofs(&self) -> impl Iterator<Item = (&JointC, &Dof)> {
        self.joints.iter().flat_map(|j| j.dofs.iter().map(move |d| (j, d)))
    }

    /// Names of every state, in state order (for id lookup by the wrapper).
    pub fn state_names(&self) -> Vec<String> {
        self.states().into_iter().map(|s| s.name).collect()
    }

    pub fn floor_height(&self, x: f64, y: f64) -> f64 {
        self.terrain.as_ref().map(|t| t.height(x, y)).unwrap_or(self.floor_z)
    }

    fn base_of(&self, g: &Generalized, b: usize) -> (V, UnitQuaternion<f64>, V, V, V, V) {
        let o = self.bases[b].state;
        let s = &g.states[o..o + BASE_STATES];
        let r = &g.rates[o..o + BASE_STATES];
        let p = V::new(s[0], s[1], s[2]);
        let q = quat(s[3], s[4], s[5], s[6]);
        let vel = V::new(s[7], s[8], s[9]);
        let w = V::new(s[10], s[11], s[12]);
        let acc = V::new(r[7], r[8], r[9]);
        let alpha = V::new(r[10], r[11], r[12]);
        (p, q, vel, w, acc, alpha)
    }

    /// Read the generalized coordinates from a residual context.
    fn read_ctx(&self, ctx: &Context) -> Generalized {
        let states = ctx.states().to_vec();
        let rates = ctx.state_rates().to_vec();
        let mut q = Vec::new();
        let mut qd = Vec::new();
        let mut qdd = Vec::new();
        for (_, d) in self.dofs() {
            let angle = match d.port {
                Some(p) => ctx.across(p),
                None => states[d.q_state.unwrap()],
            };
            q.push(angle);
            qd.push(states[d.qd_state]);
            qdd.push(rates[d.qd_state]);
        }
        let temperatures = (0..self.signal_in_names.len()).map(|k| ctx.signal_in(k)).collect();
        Generalized { states, rates, q, qd, qdd, temperatures }
    }

    fn read_view(&self, view: &View) -> Generalized {
        let states: Vec<f64> = (0..self.state_count).map(|k| view.state(k)).collect();
        let rates = vec![0.0; self.state_count];
        let mut q = Vec::new();
        let mut qd = Vec::new();
        for (_, d) in self.dofs() {
            q.push(match d.port {
                Some(p) => view.across(p),
                None => states[d.q_state.unwrap()],
            });
            qd.push(states[d.qd_state]);
        }
        let qdd = vec![0.0; q.len()];
        let temperatures = (0..self.signal_in_names.len()).map(|k| view.signal_in(k)).collect();
        Generalized { states, rates, q, qd, qdd, temperatures }
    }

    /// Build a `Generalized` from plain values (for the wrapper).
    pub fn generalized(&self, states: Vec<f64>, rates: Vec<f64>, port_angles: &[f64], temperatures: Vec<f64>) -> Generalized {
        let mut q = Vec::new();
        let mut qd = Vec::new();
        let mut qdd = Vec::new();
        for (_, d) in self.dofs() {
            q.push(match d.port {
                Some(p) => port_angles.get(p).copied().unwrap_or(0.0),
                None => states[d.q_state.unwrap()],
            });
            qd.push(states[d.qd_state]);
            qdd.push(rates.get(d.qd_state).copied().unwrap_or(0.0));
        }
        let temperatures = if temperatures.len() == self.signal_in_names.len() { temperatures } else { vec![self.ambient_c + 273.15; self.signal_in_names.len()] };
        Generalized { states, rates, q, qd, qdd, temperatures }
    }

    fn flex_deflection(&self, link: &LinkC, boundary: usize, g: &Generalized) -> (V, V, V, V, V, V) {
        // (u, θ, u̇, θ̇, ü, θ̈) of a boundary frame in the link frame.
        let mut out = [V::zeros(); 6];
        if let Some(f) = &link.flex {
            for m in 0..f.modes {
                let eta = g.states[f.state + m];
                let etad = g.states[f.state + f.modes + m];
                let etadd = g.rates[f.state + f.modes + m];
                let s = f.shapes[m][boundary];
                let du = V::new(s[0], s[1], s[2]);
                let dt = V::new(s[3], s[4], s[5]);
                out[0] += du * eta;
                out[1] += dt * eta;
                out[2] += du * etad;
                out[3] += dt * etad;
                out[4] += du * etadd;
                out[5] += dt * etadd;
            }
        }
        (out[0], out[1], out[2], out[3], out[4], out[5])
    }

    /// Forward and backward passes, contacts, constraints: the whole
    /// evaluation at one instant.
    pub fn evaluate_with(&self, g: &Generalized, contact: bool) -> Evaluation {
        let n = self.links.len();
        let mut kin: Vec<Option<LinkKin>> = vec![None; n];
        for b in 0..self.bases.len() {
            let (p0, q0, v0, w0, a0, al0) = self.base_of(g, b);
            kin[self.bases[b].link] = Some(LinkKin { r: q0.to_rotation_matrix().into_inner(), p: p0, w: w0, vel: v0, alpha: al0, acc: a0 });
        }
        let mut joint_points = vec![V::zeros(); self.joints.len()];
        let mut joint_axes: Vec<Vec<V>> = vec![Vec::new(); self.joints.len()];
        let mut joint_frames: Vec<M> = vec![M::identity(); self.joints.len()];
        let mut dof_index = 0usize;
        for (ji, j) in self.joints.iter().enumerate() {
            let pk = kin[j.parent].clone().expect("parents are evaluated first");
            let parent = &self.links[j.parent];
            // Joint frame on the (possibly deflected) parent.
            let (u, th, ud, thd, udd, thdd) = match j.flex_boundary {
                Some(b) => self.flex_deflection(parent, b, g),
                None => (V::zeros(), V::zeros(), V::zeros(), V::zeros(), V::zeros(), V::zeros()),
            };
            let r_off = pk.r * (j.r_pj + u);
            let mut o = pk.p + r_off;
            let mut rot = pk.r * rot_vec(th) * j.r_j;
            let w_flex = pk.r * thd;
            let mut w = pk.w + w_flex;
            let mut alpha = pk.alpha + pk.r * thdd + pk.w.cross(&w_flex);
            let v_flex = pk.r * ud;
            let mut vel = pk.vel + pk.w.cross(&r_off) + v_flex;
            let mut acc = pk.acc + pk.alpha.cross(&r_off) + pk.w.cross(&pk.w.cross(&r_off)) + pk.r * udd + 2.0 * pk.w.cross(&v_flex);
            let mut axes = Vec::with_capacity(j.dofs.len());
            for d in &j.dofs {
                let e = rot.column(d.axis).into_owned();
                let (q, qd, qdd) = (g.q[dof_index], g.qd[dof_index], g.qdd[dof_index]);
                dof_index += 1;
                axes.push(e);
                match d.kind {
                    DofKind::Revolute => {
                        rot = rot_axis(e, q) * rot;
                        alpha += e * qdd + w.cross(&(e * qd));
                        w += e * qd;
                    }
                    DofKind::Prismatic => {
                        let d_w = e * q;
                        o += d_w;
                        acc += e * qdd + 2.0 * w.cross(&(e * qd)) + alpha.cross(&d_w) + w.cross(&w.cross(&d_w));
                        vel += e * qd + w.cross(&d_w);
                    }
                }
            }
            joint_points[ji] = o;
            joint_axes[ji] = axes;
            joint_frames[ji] = rot;
            let d = rot * j.r_jc;
            let r_c = rot * j.r_jc_rot;
            let p_c = o + d;
            let v_c = vel + w.cross(&d);
            let a_c = acc + alpha.cross(&d) + w.cross(&w.cross(&d));
            kin[j.child] = Some(LinkKin { r: r_c, p: p_c, w, vel: v_c, alpha, acc: a_c });
        }
        let links: Vec<LinkKin> = kin.into_iter().map(|k| k.unwrap_or(LinkKin { r: M::identity(), p: V::zeros(), w: V::zeros(), vel: V::zeros(), alpha: V::zeros(), acc: V::zeros() })).collect();
        // External forces (world, about each COM).
        let mut f_ext = vec![V::zeros(); n];
        let mut t_ext = vec![V::zeros(); n];
        for (i, l) in self.links.iter().enumerate() {
            f_ext[i] += self.gravity * l.mass;
        }
        let mut contacts = Vec::new();
        let mut bristle_rates = vec![0.0; g.states.len()];
        let mut contact_normal = vec![0.0; n];
        if contact {
            self.contacts(g, &links, &mut f_ext, &mut t_ext, &mut contacts, &mut bristle_rates, &mut contact_normal);
        }
        // Cables.
        for c in &self.cables {
            let (ka, kb) = (&links[c.a], &links[c.b]);
            let ra = ka.r * c.r_a;
            let rb = kb.r * c.r_b;
            let pa = ka.p + ra;
            let pb = kb.p + rb;
            let d = pb - pa;
            let len = d.norm().max(1e-9);
            let dir = d / len;
            let rate = (kb.vel + kb.w.cross(&rb) - ka.vel - ka.w.cross(&ra)).dot(&dir);
            let stretch = len - c.length;
            let tension = if stretch > 0.0 { (c.stiffness * stretch / c.length.max(1e-6) + c.damping * rate).max(0.0) } else { 0.0 };
            let f = dir * tension;
            let weight = self.gravity * (0.5 * c.mass);
            f_ext[c.a] += f + weight;
            t_ext[c.a] += ra.cross(&(f + weight));
            f_ext[c.b] += -f + weight;
            t_ext[c.b] += rb.cross(&(-f + weight));
        }
        // Loop constraint forces and rows.
        let mut loop_rows = Vec::new();
        for lp in &self.loops {
            let (ka, kb) = (&links[lp.a], &links[lp.b]);
            let ra = ka.r * lp.r_a;
            let rb = kb.r * lp.r_b;
            let lam = &g.states[lp.lambda_state..lp.lambda_state + lp.rows];
            let f = V::new(lam[0], lam[1], lam[2]);
            f_ext[lp.b] += f;
            t_ext[lp.b] += rb.cross(&f);
            f_ext[lp.a] -= f;
            t_ext[lp.a] -= ra.cross(&f);
            let pa = ka.p + ra;
            let pb = kb.p + rb;
            let va = ka.vel + ka.w.cross(&ra);
            let vb = kb.vel + kb.w.cross(&rb);
            let aa = ka.acc + ka.alpha.cross(&ra) + ka.w.cross(&ka.w.cross(&ra));
            let ab = kb.acc + kb.alpha.cross(&rb) + kb.w.cross(&kb.w.cross(&rb));
            let al = self.loop_alpha;
            let phi = pb - pa;
            let dphi = vb - va;
            let ddphi = ab - aa;
            for k in 0..3 {
                loop_rows.push(ddphi[k] + 2.0 * al * dphi[k] + al * al * phi[k] + self.loop_cfm * lam[k]);
            }
            if let Some((e1, e2, ax)) = &lp.axis {
                let a_w = kb.r * ax;
                let da = kb.w.cross(&a_w);
                let dda = kb.alpha.cross(&a_w) + kb.w.cross(&kb.w.cross(&a_w));
                for (row, e_l) in [(3usize, e1), (4, e2)] {
                    let e_w = ka.r * e_l;
                    let de = ka.w.cross(&e_w);
                    let dde = ka.alpha.cross(&e_w) + ka.w.cross(&ka.w.cross(&e_w));
                    let phi = e_w.dot(&a_w);
                    let dphi = de.dot(&a_w) + e_w.dot(&da);
                    let ddphi = dde.dot(&a_w) + 2.0 * de.dot(&da) + e_w.dot(&dda);
                    loop_rows.push(ddphi + 2.0 * al * dphi + al * al * phi + self.loop_angular_cfm * lam[row]);
                    let torque = a_w.cross(&e_w) * lam[row];
                    t_ext[lp.b] += torque;
                    t_ext[lp.a] -= torque;
                }
            }
        }
        let mut transmission_torque = vec![0.0; g.q.len()];
        for t in &self.transmissions {
            let lambda = g.states[t.lambda_state];
            let phi = g.q[t.driver] - t.ratio * g.q[t.driven];
            let velocity = g.qd[t.driver] - t.ratio * g.qd[t.driven];
            let acceleration = g.qdd[t.driver] - t.ratio * g.qdd[t.driven];
            loop_rows.push(acceleration + 2.0 * self.loop_alpha * velocity + self.loop_alpha.powi(2) * phi + self.loop_angular_cfm * lambda);
            transmission_torque[t.driver] += lambda;
            transmission_torque[t.driven] -= t.ratio * lambda;
        }
        // Planar penalty on floating bases.
        if let Some((nrm, org)) = &self.planar {
            for b in self.bases.iter().filter(|b| !b.grounded) {
                let (p0, q0, v0, w0, _, _) = self.base_of(g, self.bases.iter().position(|x| x.link == b.link).unwrap());
                let m_total = subtree_mass(&self.model, b.link);
                let k = 1.0e4 * m_total;
                let c = 2.0 * (k * m_total).sqrt();
                let off = (p0 - org).dot(nrm);
                let vn = v0.dot(nrm);
                f_ext[b.link] -= nrm * (k * off + c * vn);
                let rv = q0.scaled_axis();
                let rperp = rv - nrm * rv.dot(nrm);
                let wperp = w0 - nrm * w0.dot(nrm);
                let i_total = self.links[b.link].inertia.trace() / 3.0 + m_total * 0.01;
                let kr = 1.0e4 * i_total;
                let cr = 2.0 * (kr * i_total).sqrt();
                t_ext[b.link] -= rperp * kr + wperp * cr;
            }
        }
        // Backward pass.
        let mut f_link = vec![V::zeros(); n];
        let mut n_link = vec![V::zeros(); n];
        for (i, l) in self.links.iter().enumerate() {
            let k = &links[i];
            let i_w = k.r * l.inertia * k.r.transpose();
            f_link[i] = l.mass * k.acc - f_ext[i];
            n_link[i] = i_w * k.alpha + k.w.cross(&(i_w * k.w)) - t_ext[i];
        }
        let mut joints: Vec<JointReaction> = vec![JointReaction::default(); self.joints.len()];
        let mut sum_f = vec![V::zeros(); n]; // force the link must transmit to children
        let mut sum_n = vec![V::zeros(); n]; // their moments about the link's COM
        for (ji, j) in self.joints.iter().enumerate().rev() {
            let c = j.child;
            let o = joint_points[ji];
            let f = f_link[c] + sum_f[c];
            let nmom = n_link[c] + (links[c].p - o).cross(&f_link[c]) + sum_n[c] + (links[c].p - o).cross(&sum_f[c]);
            joints[ji].f = f;
            joints[ji].n = nmom;
            joints[ji].point = o;
            sum_f[j.parent] += f;
            sum_n[j.parent] += nmom + (o - links[j.parent].p).cross(&f);
        }
        let base_wrench: Vec<[f64; 6]> = self
            .bases
            .iter()
            .map(|b| {
                let f = f_link[b.link] + sum_f[b.link];
                let nm = n_link[b.link] + sum_n[b.link];
                [f.x, f.y, f.z, nm.x, nm.y, nm.z]
            })
            .collect();
        // Joint torques: needed vs passive.
        let mut dof_index = 0usize;
        for (ji, j) in self.joints.iter().enumerate() {
            let axes = joint_axes[ji].clone();
            let mut needed = Vec::with_capacity(j.dofs.len());
            let mut passive = Vec::with_capacity(j.dofs.len());
            for (k, d) in j.dofs.iter().enumerate() {
                let e = axes[k];
                let tau = match d.kind {
                    DofKind::Revolute => e.dot(&joints[ji].n),
                    DofKind::Prismatic => e.dot(&joints[ji].f),
                };
                let (q, qd) = (g.q[dof_index], g.qd[dof_index]);
                dof_index += 1;
                let mut pas = friction_torque(&d.friction, qd) + transmission_torque[dof_index - 1];
                let qm = q + d.home;
                if let Some(hi) = d.upper {
                    if qm > hi {
                        pas -= d.stop_k * (qm - hi) + d.stop_c * qd.max(0.0);
                    }
                }
                if let Some(lo) = d.lower {
                    if qm < lo {
                        pas -= d.stop_k * (qm - lo) + d.stop_c * qd.min(0.0);
                    }
                }
                if let Some((ks, cs)) = d.spring {
                    pas -= ks * q + cs * qd;
                }
                needed.push(tau);
                passive.push(pas);
            }
            joints[ji].axes = axes;
            joints[ji].tau_needed = needed;
            joints[ji].tau_passive = passive;
        }
        // Modal forces.
        let mut modal_force = vec![Vec::new(); n];
        for (li, l) in self.links.iter().enumerate() {
            let Some(f) = &l.flex else { continue };
            let k = &links[li];
            let a_loc = k.r.transpose() * (k.acc - self.gravity);
            let al_loc = k.r.transpose() * k.alpha;
            let mut out = vec![0.0; f.modes];
            for m in 0..f.modes {
                let p = f.participation[m];
                let mut fm = -(p[0] * a_loc.x + p[1] * a_loc.y + p[2] * a_loc.z + p[3] * al_loc.x + p[4] * al_loc.y + p[5] * al_loc.z);
                for &ji in &l.children {
                    if let Some(b) = self.joints[ji].flex_boundary {
                        let s = f.shapes[m][b];
                        let fb = k.r.transpose() * (-joints[ji].f);
                        let nb = k.r.transpose() * (-joints[ji].n);
                        fm += s[0] * fb.x + s[1] * fb.y + s[2] * fb.z + s[3] * nb.x + s[4] * nb.y + s[5] * nb.z;
                    }
                }
                out[m] = fm;
            }
            modal_force[li] = out;
        }
        Evaluation { links, joints, contacts, base_wrench, modal_force, loop_rows, bristle_rates, contact_normal, joint_points }
    }

    #[allow(clippy::too_many_arguments)]
    fn contacts(&self, g: &Generalized, links: &[LinkKin], f_ext: &mut [V], t_ext: &mut [V], out: &mut Vec<ContactPoint>, bristle_rates: &mut [f64], normal_sum: &mut [f64]) {
        let n = self.links.len();
        // World boxes for the broad phase.
        let boxes: Vec<(V, V)> = self.links.iter().zip(links).map(|(l, k)| world_box(l, k)).collect();
        let sigma0 = self.floor_k;
        let up = V::z();
        for (i, l) in self.links.iter().enumerate() {
            let k = &links[i];
            // Floor / terrain: every vertex below the surface carries a normal
            // force; the link's stick-slip is one bristle patch shared by
            // them (translation and twist about the normal).
            if !l.grounded {
                let zs = l.bristle_state;
                let z = V::new(g.states[zs], g.states[zs + 1], 0.0);
                let z_twist = g.states[zs + 2];
                let mut total = 0.0;
                let mut touching: Vec<(V, V, f64, V)> = Vec::new(); // point, offset, normal force, velocity
                for c in &l.contact {
                    let r = k.r * c;
                    let pt = k.p + r;
                    let depth = self.floor_height(pt.x, pt.y) - pt.z;
                    if depth <= 0.0 {
                        continue;
                    }
                    let vp = k.vel + k.w.cross(&r);
                    let vn = vp.dot(&up);
                    let fn_ = (self.floor_k * depth * (1.0 - self.restitution_damping * vn)).max(0.0);
                    total += fn_;
                    touching.push((pt, r, fn_, vp));
                }
                if total > 0.0 {
                    let sigma1 = 2.0 * (sigma0 * l.mass).sqrt();
                    let mut centroid = V::zeros();
                    let mut vt = V::zeros();
                    for (pt, _, fn_, vp) in &touching {
                        let w = fn_ / total;
                        centroid += pt * w;
                        vt += (vp - up * vp.dot(&up)) * w;
                    }
                    let rg2 = touching.iter().map(|(pt, _, fn_, _)| (pt - centroid).norm_squared() * fn_ / total).sum::<f64>().max(1e-8);
                    let (mus, muk) = l.floor_mu;
                    let speed = vt.norm();
                    let mu = muk + (mus - muk) * (-(speed / 0.01).powi(2)).exp();
                    let gv = mu * total + 1.0e-3;
                    let zd = vt - z * (sigma0 * speed / gv);
                    let ft = -(z * sigma0 + zd * sigma1);
                    let wn = k.w.dot(&up);
                    let k_twist = sigma0 * rg2;
                    let g_twist = mu * total * rg2.sqrt() + 1.0e-6;
                    let zd_twist = wn - z_twist * (k_twist * wn.abs() / g_twist);
                    let torque = -(k_twist * z_twist + sigma1 * rg2 * zd_twist);
                    bristle_rates[zs] = zd.x;
                    bristle_rates[zs + 1] = zd.y;
                    bristle_rates[zs + 2] = zd_twist;
                    for (pt, r, fn_, _) in &touching {
                        let share = ft * (fn_ / total);
                        let force = up * *fn_ + share;
                        f_ext[i] += up * *fn_;
                        t_ext[i] += r.cross(&(up * *fn_));
                        out.push(ContactPoint { link: i, other: None, point: *pt, force, penetration: *fn_ / self.floor_k });
                    }
                    f_ext[i] += ft;
                    t_ext[i] += (centroid - k.p).cross(&ft) + up * torque;
                    normal_sum[i] += total;
                } else {
                    bristle_rates[zs] = -z.x * 200.0;
                    bristle_rates[zs + 1] = -z.y * 200.0;
                    bristle_rates[zs + 2] = -z_twist * 200.0;
                }
            }
            for (ci, c) in l.contact.iter().enumerate() {
                let r = k.r * c;
                let pt = k.p + r;
                let vp = k.vel + k.w.cross(&r);
                // Other links' distance fields.
                for j in 0..n {
                    if j == i || self.links[j].sdf.is_none() {
                        continue;
                    }
                    if l.excluded.get(&j).map(|f| f[ci]).unwrap_or(false) {
                        continue;
                    }
                    let (lo, hi) = &boxes[j];
                    if pt.x < lo.x || pt.y < lo.y || pt.z < lo.z || pt.x > hi.x || pt.y > hi.y || pt.z > hi.z {
                        continue;
                    }
                    // Neighbours through a joint: skip the band around it.
                    if let Some(band) = self.neighbour_band(i, j) {
                        if (pt - band.0).norm() < band.1 {
                            continue;
                        }
                    }
                    let kj = &links[j];
                    let local = kj.r.transpose() * (pt - kj.p);
                    let (phi, grad) = self.links[j].sdf.as_ref().unwrap().sample(local);
                    if phi >= 0.0 {
                        continue;
                    }
                    let depth = -phi;
                    let nrm = kj.r * grad;
                    let v_rel = vp - (kj.vel + kj.w.cross(&(pt - kj.p)));
                    let vn = v_rel.dot(&nrm);
                    let fn_ = (self.link_k * depth * (1.0 - self.restitution_damping * vn)).max(0.0);
                    let vt = v_rel - nrm * vn;
                    let (_, muk) = self.model.friction_between(&l.material, &self.links[j].material);
                    let ft = -vt * (muk * fn_ / (vt.norm() + 1e-3));
                    let force = nrm * fn_ + ft;
                    f_ext[i] += force;
                    t_ext[i] += r.cross(&force);
                    f_ext[j] -= force;
                    t_ext[j] -= (pt - kj.p).cross(&force);
                    normal_sum[i] += fn_;
                    normal_sum[j] += fn_;
                    out.push(ContactPoint { link: i, other: Some(j), point: pt, force, penetration: depth });
                }
            }
        }
    }

    /// `(joint point at export, band radius)` when links `i` and `j` share a joint.
    fn neighbour_band(&self, i: usize, j: usize) -> Option<(V, f64)> {
        for jc in &self.joints {
            if (jc.parent == i && jc.child == j) || (jc.parent == j && jc.child == i) {
                let origin = self.links[jc.parent].com0 + jc.r_pj;
                return Some((origin, jc.band));
            }
        }
        for lp in &self.loops {
            if (lp.a == i && lp.b == j) || (lp.a == j && lp.b == i) {
                return Some((self.links[lp.a].com0 + lp.r_a, 0.01));
            }
        }
        // Joint origins move with the parent; use the export pose band as an
        // approximation (links near a joint stay near it).
        None
    }

    /// Von Mises stress at each stress cell of a link for modal amplitudes `eta`.
    pub fn stress(&self, link: usize, eta: &[f64]) -> Vec<f64> {
        let Some(f) = &self.links[link].flex else { return Vec::new() };
        let cells = f.stress_cells.len();
        let mut out = vec![0.0; cells];
        for c in 0..cells {
            let mut s = [0.0; 6];
            for m in 0..f.modes.min(eta.len()) {
                if let Some(sm) = f.stress_per_mode.get(m).and_then(|v| v.get(c)) {
                    for k in 0..6 {
                        s[k] += sm[k] * eta[m];
                    }
                }
            }
            let (xx, yy, zz, xy, yz, xz) = (s[0], s[1], s[2], s[3], s[4], s[5]);
            out[c] = (0.5 * ((xx - yy).powi(2) + (yy - zz).powi(2) + (zz - xx).powi(2)) + 3.0 * (xy * xy + yz * yz + xz * xz)).sqrt();
        }
        out
    }

    /// Link poses `(R, p)` from state values and port angles (for viewers).
    pub fn poses(&self, g: &Generalized) -> Vec<(M, V)> {
        let e = self.evaluate_kinematics_only(g);
        e.iter().map(|k| (k.r, k.p)).collect()
    }

    pub fn evaluate_kinematics_only(&self, g: &Generalized) -> Vec<LinkKin> {
        self.evaluate_with(g, false).links
    }

    pub fn evaluate(&self, g: &Generalized) -> Evaluation {
        self.evaluate_with(g, self.contact_on)
    }

    fn imu_sample(&self, imu: &ImuC, view: &View, states: &mut [f64]) {
        let g = self.read_view(view);
        let kin = self.evaluate_kinematics_only(&g);
        let k = &kin[imu.link];
        let r = k.r * imu.point;
        let vel = k.vel + k.w.cross(&r);
        let s = imu.state;
        let prev = V::new(states[s + 6], states[s + 7], states[s + 8]);
        let first = states[s + 15] <= imu.period * 1.5 + imu.latency;
        let acc_w = if first { V::zeros() } else { (vel - prev) / imu.period };
        let specific = imu.axes * (k.r.transpose() * (acc_w - self.gravity));
        let gyro = imu.axes * (k.r.transpose() * k.w);
        let index = (states[s + 15] / imu.period).round() as u64;
        for c in 0..6 {
            // Bias random walk.
            states[s + 9 + c] += imu.bias_walk * imu.period.sqrt() * keyed_normal(imu.stream.wrapping_mul(13).wrapping_add(c as u64 + 100), index);
            let raw = if c < 3 { specific[c] } else { gyro[c - 3] };
            let noise = if c < 3 { imu.noise[0] } else { imu.noise[1] };
            let mut value = raw + imu.bias0[c] + states[s + 9 + c] + noise * keyed_normal(imu.stream.wrapping_mul(13).wrapping_add(c as u64), index);
            let qstep = if c < 3 { imu.quant[0] } else { imu.quant[1] };
            if qstep > 0.0 {
                value = (value / qstep).round() * qstep;
            }
            let range = if c < 3 { imu.range[0] } else { imu.range[1] };
            states[s + c] = value.clamp(-range, range);
        }
        states[s + 6] = vel.x;
        states[s + 7] = vel.y;
        states[s + 8] = vel.z;
    }
}

fn world_box(l: &LinkC, k: &LinkKin) -> (V, V) {
    let mut lo = V::repeat(f64::INFINITY);
    let mut hi = V::repeat(f64::NEG_INFINITY);
    for ix in 0..2 {
        for iy in 0..2 {
            for iz in 0..2 {
                let c = V::new(if ix == 0 { l.lo.x } else { l.hi.x }, if iy == 0 { l.lo.y } else { l.hi.y }, if iz == 0 { l.lo.z } else { l.hi.z });
                let p = k.p + k.r * c;
                lo = lo.inf(&p);
                hi = hi.sup(&p);
            }
        }
    }
    let pad = 1e-3;
    (lo - V::repeat(pad), hi + V::repeat(pad))
}

fn subtree_mass(model: &PhysicalModel, link: usize) -> f64 {
    let mut total = 0.0;
    let mut stack = vec![link];
    let mut seen = vec![false; model.links.len()];
    while let Some(i) = stack.pop() {
        if seen[i] {
            continue;
        }
        seen[i] = true;
        total += model.links[i].mass;
        let name = &model.links[i].name;
        for j in model.joints.iter().filter(|j| !j.is_loop() && j.parent.as_deref() == Some(name.as_str())) {
            if let Some(c) = model.link_index(&j.child) {
                stack.push(c);
            }
        }
    }
    total
}

fn compile_flex(f: &Flex, li: usize, joints: &[JointC], links: &[LinkC], state: usize, temperature_signal: Option<usize>, keep: usize) -> FlexC {
    let modes = keep;
    let nb = f.boundary_frames.len();
    // The inboard boundary: the one named after the link's parent joint,
    // else the nearest to the inboard joint origin.
    let inboard = links[li].parent_joint.and_then(|pj| {
        let jc = &joints[pj];
        // Child frame axes equal the world axes at export: com_c − origin = R_j r_jc.
        let joint_origin_child = -(jc.r_j * jc.r_jc);
        let mut best = None::<(usize, f64)>;
        for (bi, bf) in f.boundary_frames.iter().enumerate() {
            let score = if bf.name == jc.name { -1.0 } else { (v(bf.point) - joint_origin_child).norm() };
            if best.map(|(_, s)| score < s).unwrap_or(true) {
                best = Some((bi, score));
            }
        }
        best.map(|(b, _)| b)
    });
    let mut shapes = Vec::with_capacity(modes);
    for m in 0..modes {
        let row = &f.boundary_shapes[m];
        let base = inboard.and_then(|b| row.get(b)).copied().unwrap_or([0.0; 6]);
        let mut rel = Vec::with_capacity(nb);
        for b in 0..nb {
            let s = row.get(b).copied().unwrap_or([0.0; 6]);
            let point = v(f.boundary_frames[b].point);
            let inboard_point = inboard.map(|ib| v(f.boundary_frames[ib].point)).unwrap_or(V::zeros());
            // Remove the inboard frame's rigid motion (translation + small rotation about it).
            let th = V::new(base[3], base[4], base[5]);
            let rigid = V::new(base[0], base[1], base[2]) + th.cross(&(point - inboard_point));
            rel.push([s[0] - rigid.x, s[1] - rigid.y, s[2] - rigid.z, s[3] - base[3], s[4] - base[4], s[5] - base[5]]);
        }
        shapes.push(rel);
    }
    let mass: Vec<f64> = (0..modes).map(|m| f.modal_mass.get(m).copied().unwrap_or(1.0).max(1e-12)).collect();
    let stiffness: Vec<f64> = (0..modes)
        .map(|m| f.modal_stiffness.get(m).copied().unwrap_or_else(|| {
            let w = 2.0 * std::f64::consts::PI * f.frequencies_hz.get(m).copied().unwrap_or(100.0);
            mass[m] * w * w
        }))
        .collect();
    let damping: Vec<f64> = (0..modes).map(|m| 2.0 * f.damping_ratio * (stiffness[m] * mass[m]).sqrt()).collect();
    FlexC {
        normalization: f.normalization,
        modes,
        mass,
        stiffness,
        damping,
        boundary_points: f.boundary_frames.iter().map(|b| v(b.point)).collect(),
        shapes,
        participation: (0..modes).map(|m| f.participation.get(m).copied().unwrap_or([0.0; 6])).collect(),
        state,
        temperature_signal,
        softening: f.softening.clone(),
        stress_cells: f.stress_cells.iter().map(|c| v(*c)).collect(),
        stress_per_mode: f.stress_per_mode.iter().take(modes).cloned().collect(),
    }
}

impl Behavior for Articulated {
    fn owned_frame(&self) -> Option<usize> { Some(0) }
    fn states(&self) -> Vec<StateDeclaration> {
        use QuantityKind::*;
        let (p, q) = &self.base0;
        let mut out = vec![
            StateDeclaration::new("base.x", Length, p.x),
            StateDeclaration::new("base.y", Length, p.y),
            StateDeclaration::new("base.z", Length, p.z),
            StateDeclaration::new("base.qw", Dimensionless, q.w),
            StateDeclaration::new("base.qx", Dimensionless, q.i),
            StateDeclaration::new("base.qy", Dimensionless, q.j),
            StateDeclaration::new("base.qz", Dimensionless, q.k),
            StateDeclaration::new("base.vx", LinearVelocity, self.initial_twist[0]),
            StateDeclaration::new("base.vy", LinearVelocity, self.initial_twist[1]),
            StateDeclaration::new("base.vz", LinearVelocity, self.initial_twist[2]),
            StateDeclaration::new("base.wx", AngularVelocity, self.initial_twist[3]),
            StateDeclaration::new("base.wy", AngularVelocity, self.initial_twist[4]),
            StateDeclaration::new("base.wz", AngularVelocity, self.initial_twist[5]),
        ];
        for (_, d) in self.dofs() {
            if d.port.is_none() {
                out.push(StateDeclaration::new(format!("{}.q", d.name), if d.kind == DofKind::Revolute { Angle } else { Length }, d.initial_angle));
            }
            out.push(StateDeclaration::new(format!("{}.speed", d.name), if d.kind == DofKind::Revolute { AngularVelocity } else { LinearVelocity }, d.initial_speed));
        }
        for (k, b) in self.bases.iter().enumerate().skip(1) {
            let name = format!("base{k}");
            out.push(StateDeclaration::new(format!("{name}.x"), Length, b.p0.x));
            out.push(StateDeclaration::new(format!("{name}.y"), Length, b.p0.y));
            out.push(StateDeclaration::new(format!("{name}.z"), Length, b.p0.z));
            out.push(StateDeclaration::new(format!("{name}.qw"), Dimensionless, 1.0));
            for c in ["qx", "qy", "qz"] {
                out.push(StateDeclaration::new(format!("{name}.{c}"), Dimensionless, 0.0));
            }
            for c in ["vx", "vy", "vz"] {
                out.push(StateDeclaration::new(format!("{name}.{c}"), LinearVelocity, 0.0));
            }
            for c in ["wx", "wy", "wz"] {
                out.push(StateDeclaration::new(format!("{name}.{c}"), AngularVelocity, 0.0));
            }
        }
        for l in &self.links {
            if let Some(f) = &l.flex {
                let (coordinate, velocity) = f.normalization.quantities();
                for m in 0..f.modes {
                    out.push(StateDeclaration::new(format!("{}.eta{m}", l.name), coordinate, 0.0));
                }
                for m in 0..f.modes {
                    out.push(StateDeclaration::new(format!("{}.etad{m}", l.name), velocity, 0.0));
                }
            }
        }
        for lp in &self.loops {
            for k in 0..lp.rows {
                out.push(StateDeclaration::new(format!("{}.lambda{k}", lp.name), if k < 3 { Force } else { Torque }, 0.0));
            }
        }
        for t in &self.transmissions {
            out.push(StateDeclaration::new(format!("{}.lambda", t.name), Torque, 0.0));
        }
        for l in self.links.iter().filter(|l| !l.grounded) {
            out.push(StateDeclaration::new(format!("{}.bristle.x", l.name), Length, 0.0));
            out.push(StateDeclaration::new(format!("{}.bristle.y", l.name), Length, 0.0));
            out.push(StateDeclaration::new(format!("{}.bristle.twist", l.name), Angle, 0.0));
        }
        for imu in &self.imus {
            for (k, c) in ["ax", "ay", "az", "gx", "gy", "gz"].iter().enumerate() {
                out.push(StateDeclaration::new(format!("{}.{c}", imu.name), if k < 3 { LinearAcceleration } else { AngularVelocity }, 0.0));
            }
            for c in ["vx", "vy", "vz"] {
                out.push(StateDeclaration::new(format!("{}.prev.{c}", imu.name), LinearVelocity, 0.0));
            }
            for c in 0..6 {
                out.push(StateDeclaration::new(format!("{}.bias{c}", imu.name), if c < 3 { LinearAcceleration } else { AngularVelocity }, 0.0));
            }
            out.push(StateDeclaration::new(format!("{}.next_sample", imu.name), Time, imu.latency + imu.period));
        }
        debug_assert_eq!(out.len(), self.state_count);
        out
    }

    fn provides(&self) -> Vec<Provision> {
        self.dofs().filter_map(|(_, d)| d.port.map(|p| Provision { port: p, lane: 1, state: d.qd_state })).collect()
    }

    fn pinned(&self) -> Vec<(usize, usize, f64)> {
        self.dofs().filter_map(|(_, d)| d.port.map(|p| (p, 0, d.initial_angle))).collect()
    }

    fn residual(&self, ctx: &mut Context) {
        let g = self.read_ctx(ctx);
        let e = self.evaluate(&g);
        let s = &g.states;
        let r = &g.rates;
        // Bases.
        for (bi, b) in self.bases.iter().enumerate() {
            let o = b.state;
            if b.grounded {
                let target = [b.p0.x, b.p0.y, b.p0.z, 1.0, 0.0, 0.0, 0.0];
                for k in 0..7 {
                    ctx.set_state_residual(o + k, s[o + k] - target[k]);
                }
                for k in 7..13 {
                    ctx.set_state_residual(o + k, s[o + k]);
                }
            } else {
                for k in 0..3 {
                    ctx.set_state_residual(o + k, r[o + k] - s[o + 7 + k]);
                }
                let q = quat(s[o + 3], s[o + 4], s[o + 5], s[o + 6]);
                let w_body = q.inverse() * V::new(s[o + 10], s[o + 11], s[o + 12]);
                let qr = quat_rate(&q, w_body);
                let norm2 = (3..7).map(|k| s[o + k] * s[o + k]).sum::<f64>();
                for k in 0..4 {
                    ctx.set_state_residual(o + 3 + k, r[o + 3 + k] - qr[k] - 10.0 * (1.0 - norm2) * s[o + 3 + k]);
                }
                for k in 0..6 {
                    ctx.set_state_residual(o + 7 + k, e.base_wrench[bi][k]);
                }
            }
        }
        // Joints.
        let mut dof_index = 0usize;
        for (ji, j) in self.joints.iter().enumerate() {
            for (k, d) in j.dofs.iter().enumerate() {
                let through = e.joints[ji].tau_needed[k] - e.joints[ji].tau_passive[k];
                match d.port {
                    Some(p) => {
                        ctx.set_state_residual(d.qd_state, s[d.qd_state] - ctx.across_derivative(p, 0));
                        ctx.add_through(p, through);
                    }
                    None => {
                        let qs = d.q_state.unwrap();
                        ctx.set_state_residual(qs, r[qs] - s[d.qd_state]);
                        ctx.set_state_residual(d.qd_state, through);
                    }
                }
                dof_index += 1;
            }
        }
        let _ = dof_index;
        // Flex.
        for (li, l) in self.links.iter().enumerate() {
            let Some(f) = &l.flex else { continue };
            // Thermal nodes are in kelvin; the softening curve is in °C.
            let temp = f.temperature_signal.map(|k| g.temperatures[k] - 273.15).unwrap_or(self.ambient_c);
            let soft = f.softening.factor(temp);
            for m in 0..f.modes {
                let eta = s[f.state + m];
                let etad = s[f.state + f.modes + m];
                let etadd = r[f.state + f.modes + m];
                ctx.set_state_residual(f.state + m, r[f.state + m] - etad);
                // Scaled by the modal stiffness so the row is in metres like
                // the coordinate, not in newtons a million times larger.
                let scale = 1.0 / f.stiffness[m].max(1.0);
                ctx.set_state_residual(f.state + f.modes + m, scale * (f.mass[m] * etadd + f.damping[m] * etad + f.stiffness[m] * soft * eta - e.modal_force[li][m]));
            }
        }
        // Loops.
        let mut row = 0;
        for lp in &self.loops {
            for k in 0..lp.rows {
                ctx.set_state_residual(lp.lambda_state + k, e.loop_rows[row]);
                row += 1;
            }
        }
        for t in &self.transmissions {
            ctx.set_state_residual(t.lambda_state, e.loop_rows[row]);
            row += 1;
        }
        // Bristles.
        for l in self.links.iter().filter(|l| !l.grounded) {
            for k in 0..3 {
                let idx = l.bristle_state + k;
                ctx.set_state_residual(idx, r[idx] - e.bristle_rates[idx]);
            }
        }
        // IMUs: held values, previous velocity, bias and clock are constant between samples.
        for imu in &self.imus {
            for k in 0..16 {
                ctx.set_state_residual(imu.state + k, r[imu.state + k]);
            }
            for c in 0..6 {
                ctx.set_signal(imu.signals[c], s[imu.state + c]);
            }
        }
        for (li, l) in self.links.iter().enumerate() {
            if let Some(sig) = l.contact_signal {
                ctx.set_signal(sig, e.contact_normal[li]);
            }
        }
    }

    fn guards(&self, view: &View, out: &mut Vec<f64>) {
        for imu in &self.imus {
            out.push(view.state(imu.state + 15) - view.time);
        }
    }

    fn jump(&mut self, index: usize, view: &View, states: &mut [f64]) {
        if let Some(imu) = self.imus.get(index) {
            let imu = imu.clone();
            self.imu_sample(&imu, view, states);
            states[imu.state + 15] += imu.period;
        }
    }

    fn energy(&self, view: &View) -> f64 {
        let g = self.read_view(view);
        let kin = self.evaluate_kinematics_only(&g);
        let mut e = 0.0;
        for (l, k) in self.links.iter().zip(&kin) {
            let i_w = k.r * l.inertia * k.r.transpose();
            e += 0.5 * l.mass * k.vel.norm_squared() + 0.5 * k.w.dot(&(i_w * k.w)) - l.mass * self.gravity.dot(&k.p);
            if let Some(f) = &l.flex {
                for m in 0..f.modes {
                    e += 0.5 * f.stiffness[m] * g.states[f.state + m].powi(2) + 0.5 * f.mass[m] * g.states[f.state + f.modes + m].powi(2);
                }
            }
        }
        let mut di = 0;
        for (_, d) in self.dofs() {
            if let Some((k, _)) = d.spring {
                e += 0.5 * k * g.q[di].powi(2);
            }
            di += 1;
        }
        e
    }
}

fn articulated(p: &Params) -> Result<Box<dyn Behavior>, sim_core::EquationError> {
    let handle = param(p, "model")?;
    let model = crate::model::model_by_handle(handle).ok_or_else(|| sim_core::EquationError::InvalidParameter("model".into(), "no model registered under this handle".into()))?;
    let mut opts = Options { gravity_scale: param_or(p, "gravity", 1.0), planar: param_or(p, "planar", 0.0) > 0.5, flex: param_or(p, "flex", 1.0) > 0.5, contact: param_or(p, "contact", 1.0) > 0.5, loop_alpha: param_or(p, "loop.alpha", 100.0), loop_cfm: param_or(p, "loop.cfm.translation", 1e-6), loop_angular_cfm: param_or(p, "loop.cfm.rotation", 1e-6), flex_modes: param_or(p, "flex.modes", 4.0) as usize, flex_max_hz: param_or(p, "flex.max_hz", 500.0), ..Options::default() };
    for (k, name) in ["vx", "vy", "vz", "wx", "wy", "wz"].iter().enumerate() {
        opts.initial_twist[k] = param_or(p, &format!("initial.base.{name}"), 0.0);
    }
    for (k, name) in ["x", "y", "z"].iter().enumerate() {
        opts.initial_offset[k] = param_or(p, &format!("initial.base.{name}"), 0.0);
    }
    for (k, val) in p {
        if let Some(rest) = k.strip_prefix("initial.") {
            if rest.starts_with("base.") {
                continue;
            }
            if let Some(name) = rest.strip_suffix(".angle") {
                opts.initial_angles.insert(name.to_owned(), *val);
            } else if let Some(name) = rest.strip_suffix(".speed") {
                opts.initial_speeds.insert(name.to_owned(), *val);
            } else if let Some(name) = rest.strip_suffix(".position").filter(|name| name.starts_with("slide.")) {
                opts.initial_angles.insert(name.to_owned(), *val);
            } else if let Some(name) = rest.strip_suffix(".velocity").filter(|name| name.starts_with("slide.")) {
                opts.initial_speeds.insert(name.to_owned(), *val);
            }
        }
    }
    let a = Articulated::new(model, &opts).map_err(|e| sim_core::EquationError::InvalidParameter("model".into(), e))?;
    // The port list the compiler will build must match ours.
    let mut declared: Vec<&String> = p.keys().filter(|k| ["joint.", "slide.", "temperature.", "contact.", "imu."].iter().any(|prefix| k.starts_with(prefix))).collect();
    declared.sort();
    let mut ours: Vec<&String> = a.port_names.iter().chain(&a.signal_in_names).chain(&a.signal_out_names).collect();
    ours.sort();
    if declared != ours {
        return Err(sim_core::EquationError::InvalidParameter("ports".into(), format!("ports {declared:?} do not match the model's ports {ours:?}; use `Articulated::port_parameters`")));
    }
    Ok(Box::new(a))
}

impl Articulated {
    /// Renumber DOF ports to the compiler's order (family members sorted by
    /// name: `joint.*` first, then `slide.*`).
    pub fn with_port_order(mut self) -> Self {
        let mut names: Vec<String> = self.port_names.clone();
        let mut joints: Vec<String> = names.iter().filter(|n| n.starts_with("joint.")).cloned().collect();
        joints.sort();
        let mut slides: Vec<String> = names.iter().filter(|n| n.starts_with("slide.")).cloned().collect();
        slides.sort();
        names = joints;
        names.extend(slides);
        // Port 0 is frame.base.
        for j in &mut self.joints {
            for d in &mut j.dofs {
                if d.port.is_some() {
                    let k = names.iter().position(|n| *n == d.name).expect("dof port name");
                    d.port = Some(1 + k);
                }
            }
        }
        self.port_names = names;
        // Typed IMU families are declared axis-first. Keep the physics state
        // packed per sensor, and map each channel explicitly to its port slot.
        self.signal_out_names.sort_by(|a, b| {
            let key = |name: &str| if name.starts_with("imu.") {
                format!("1.{}.{}", name.rsplit('.').next().unwrap_or(""), name)
            } else { format!("0.{name}") };
            key(a).cmp(&key(b))
        });
        for imu in &mut self.imus {
            imu.signals = ["ax", "ay", "az", "gx", "gy", "gz"].map(|axis|
                self.signal_out_names.iter().position(|name| *name == format!("imu.{}.{axis}", imu.name)).expect("IMU signal"));
        }
        self
    }
}

pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    use sim_core::ParameterDeclaration as P;
    let mut parameters = vec![
        P::required("model", "handle").integer(0., 9007199254740991.), P::optional("gravity", "1", 1.),
        P::optional("planar", "1", 0.).integer(0., 1.), P::optional("flex", "1", 1.).integer(0., 1.), P::optional("contact", "1", 1.).integer(0., 1.),
        P::optional("loop.alpha", "1/s", 100.).nonnegative(), P::optional("loop.cfm.translation", "1/kg", 1e-6).nonnegative(),
        P::optional("loop.cfm.rotation", "1/(kg·m²)", 1e-6).nonnegative(), P::optional("flex.modes", "modes", 4.).integer(1., 1024.),
        P::optional("flex.max_hz", "Hz", 500.).positive(), P::alternative("joint.*", "index").integer(0., 9007199254740991.),
        P::alternative("slide.*", "index").integer(0., 9007199254740991.),
        P::alternative("temperature.*", "1").integer(0., 0.), P::alternative("contact.*", "1").integer(0., 0.),
    ];
    for axis in ["ax", "ay", "az", "gx", "gy", "gz"] {
        parameters.push(P::alternative(format!("imu.*.{axis}"), "1").integer(0., 0.));
    }
    for (axes, unit) in [(&["x", "y", "z"][..], "m"), (&["vx", "vy", "vz"][..], "m/s"), (&["wx", "wy", "wz"][..], "rad/s")] {
        parameters.extend(axes.iter().map(|axis| P::optional(format!("initial.base.{axis}"), unit, 0.)));
    }
    parameters.extend([P::optional("initial.joint.*.angle", "rad", 0.), P::optional("initial.joint.*.speed", "rad/s", 0.),
        P::optional("initial.slide.*.position", "m", 0.), P::optional("initial.slide.*.velocity", "m/s", 0.)]);
    registry.register(BehaviorDescriptor::new(
        ARTICULATED,
        "Articulated robot from a CAD physical description",
        vec![
            acausal("frame.base", ConnectorKind::Frame),
            acausal("joint.*", ConnectorKind::Rotational),
            acausal("slide.*", ConnectorKind::Translational),
            signal_in("temperature.*", QuantityKind::Temperature),
            signal_out("contact.*", QuantityKind::Force),
            signal_out("imu.*.ax", QuantityKind::LinearAcceleration),
            signal_out("imu.*.ay", QuantityKind::LinearAcceleration),
            signal_out("imu.*.az", QuantityKind::LinearAcceleration),
            signal_out("imu.*.gx", QuantityKind::AngularVelocity),
            signal_out("imu.*.gy", QuantityKind::AngularVelocity),
            signal_out("imu.*.gz", QuantityKind::AngularVelocity),
        ],
        articulated,
    ).with_parameters(parameters))
}
