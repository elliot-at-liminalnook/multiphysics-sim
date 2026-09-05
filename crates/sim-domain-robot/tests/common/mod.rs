//! Fixtures: box links with analytic signed-distance grids, and a runtime
//! around one `robot.articulated` element.
#![allow(dead_code)]

use sim_compile::Runtime;
use sim_core::{BehaviorId, BehaviorRegistry, ModelWorld, PortId, StateId};
use sim_domain_robot::model::*;
use sim_domain_robot::{register_model, Articulated, Generalized, Options, ARTICULATED};
use sim_dynamics::Integrator;
use sim_solve::NewtonConfig;
use std::collections::BTreeMap;
use std::sync::Arc;

pub fn registry() -> BehaviorRegistry {
    let mut r = BehaviorRegistry::default();
    sim_domain_robot::register(&mut r).unwrap();
    sim_domain_rotational::elements::register(&mut r).unwrap();
    sim_domain_electrical::elements::register(&mut r).unwrap();
    sim_domain_thermal::register(&mut r).unwrap();
    sim_domain_control::elements::register(&mut r).unwrap();
    r
}

pub fn box_sdf(half: [f64; 3], cell: f64, pad: usize) -> Sdf {
    let dims = [0, 1, 2].map(|k| ((2.0 * half[k] / cell).round() as usize) + 1 + 2 * pad);
    let origin = [0, 1, 2].map(|k| -half[k] - pad as f64 * cell);
    let mut values = Vec::with_capacity(dims[0] * dims[1] * dims[2]);
    for ix in 0..dims[0] {
        for iy in 0..dims[1] {
            for iz in 0..dims[2] {
                let p = [
                    origin[0] + ix as f64 * cell,
                    origin[1] + iy as f64 * cell,
                    origin[2] + iz as f64 * cell,
                ];
                let q = [
                    p[0].abs() - half[0],
                    p[1].abs() - half[1],
                    p[2].abs() - half[2],
                ];
                let outside =
                    (q[0].max(0.0).powi(2) + q[1].max(0.0).powi(2) + q[2].max(0.0).powi(2)).sqrt();
                let inside = q[0].max(q[1]).max(q[2]).min(0.0);
                values.push(outside + inside);
            }
        }
    }
    Sdf {
        origin,
        cell,
        dims,
        values,
    }
}

/// A box link of full size `size` centred on `com` (link frame = COM).
pub fn box_link(name: &str, size: [f64; 3], mass: f64, com: [f64; 3], ground: bool) -> Link {
    let h = [size[0] / 2.0, size[1] / 2.0, size[2] / 2.0];
    let mut vertices = Vec::new();
    for sx in [-1.0, 1.0] {
        for sy in [-1.0, 1.0] {
            for sz in [-1.0, 1.0] {
                vertices.push([sx * h[0], sy * h[1], sz * h[2]]);
            }
        }
    }
    let hull = vertices.clone();
    for k in 0..3 {
        for s in [-1.0, 1.0] {
            let mut p = [0.0; 3];
            p[k] = s * h[k];
            vertices.push(p);
        }
    }
    let ixx = mass / 12.0 * (size[1] * size[1] + size[2] * size[2]);
    let iyy = mass / 12.0 * (size[0] * size[0] + size[2] * size[2]);
    let izz = mass / 12.0 * (size[0] * size[0] + size[1] * size[1]);
    let cell = (size[0].min(size[1]).min(size[2]) / 6.0).max(1e-4);
    Link {
        name: name.into(),
        id: name.into(),
        members: vec![],
        material: "pla".into(),
        ground,
        mass,
        com,
        inertia: [[ixx, 0.0, 0.0], [0.0, iyy, 0.0], [0.0, 0.0, izz]],
        bbox: vec![[-h[0], -h[1], -h[2]], [h[0], h[1], h[2]]],
        collision: Collision {
            vertices,
            triangles: vec![],
            hull,
            sdf: Some(box_sdf(h, cell, 2)),
        },
        flex: None,
        print: None,
    }
}

pub fn joint(
    name: &str,
    kind: &str,
    parent: Option<&str>,
    child: &str,
    origin: [f64; 3],
    axis: [f64; 3],
) -> Joint {
    Joint {
        name: name.into(),
        id: name.into(),
        kind: kind.into(),
        parent: parent.map(|p| p.into()),
        child: child.into(),
        origin,
        axis,
        limits: None,
        home: 0.0,
        physics: JointPhysics::default(),
        fastened: None,
        motor: None,
    }
}

pub fn pla() -> Material {
    let mut m = Material::default();
    m.id = "pla".into();
    m.name = "PLA".into();
    m.friction.insert(
        "world".into(),
        FrictionPair {
            r#static: 0.4,
            kinetic: 0.3,
        },
    );
    m.friction.insert(
        "pla".into(),
        FrictionPair {
            r#static: 0.4,
            kinetic: 0.3,
        },
    );
    m
}

pub fn empty_model() -> PhysicalModel {
    let mut m: PhysicalModel = serde_json::from_str(r#"{"version": 3}"#).unwrap();
    m.materials.insert("pla".into(), pla());
    m
}

pub struct Rig {
    pub runtime: Runtime,
    pub art: Articulated,
    pub behavior: BehaviorId,
    pub ports: Vec<PortId>,
    pub state_ids: Vec<StateId>,
    pub port_ids: Vec<StateId>,
}

impl Rig {
    /// Build a runtime with only the articulated element (its ports left
    /// open) and the given scalar parameters.
    pub fn new(model: PhysicalModel, extra: &[(&str, f64)], integrator: Integrator) -> Rig {
        let registry = registry();
        let opts = options_from(extra);
        let art = Articulated::new(Arc::new(model.clone()), &opts).unwrap();
        let handle = register_model(model);
        let mut params: Vec<(&'static str, f64)> = vec![("model", handle)];
        for (k, v) in art.port_parameters() {
            params.push((Box::leak(k.into_boxed_str()), v));
        }
        for (k, v) in extra {
            params.push((Box::leak(k.to_string().into_boxed_str()), *v));
        }
        let mut m = ModelWorld::default();
        let inst = m.part(&registry, "robot", ARTICULATED, params).unwrap();
        let mut ports = vec![inst.port("frame.base")];
        m.connect([inst.port("frame.base")]);
        for name in &art.port_names {
            let p = inst.port(Box::leak(name.clone().into_boxed_str()));
            m.connect([p]);
            ports.push(p);
        }
        for name in &art.signal_out_names {
            m.connect([inst.port(Box::leak(name.clone().into_boxed_str()))]);
        }
        for (k, name) in art.signal_in_names.iter().enumerate() {
            let amb = m
                .part(
                    &registry,
                    &format!("ambient{k}"),
                    sim_domain_control::elements::CONSTANT,
                    [("value", 293.15)],
                )
                .unwrap();
            m.connect([
                amb.port("value"),
                inst.port(Box::leak(name.clone().into_boxed_str())),
            ]);
        }
        let runtime = Runtime::new(m, &registry, integrator).expect("compiles");
        let state_ids = art
            .state_names()
            .iter()
            .map(|n| runtime.state_id(inst.behavior, n))
            .collect();
        let port_ids = ports
            .iter()
            .skip(1)
            .map(|p| runtime.across_id(*p))
            .collect();
        Rig {
            runtime,
            art,
            behavior: inst.behavior,
            ports,
            state_ids,
            port_ids,
        }
    }

    pub fn generalized(&self) -> Generalized {
        let states: Vec<f64> = self
            .state_ids
            .iter()
            .map(|id| self.runtime.get(*id))
            .collect();
        let rates = vec![0.0; states.len()];
        let mut port_angles = vec![0.0];
        port_angles.extend(self.port_ids.iter().map(|id| self.runtime.get(*id)));
        self.art.generalized(states, rates, &port_angles, vec![])
    }

    pub fn state(&self, name: &str) -> f64 {
        let k = self
            .art
            .state_names()
            .iter()
            .position(|n| n == name)
            .unwrap_or_else(|| panic!("state {name}"));
        self.runtime.get(self.state_ids[k])
    }

    pub fn angle(&self, port: usize) -> f64 {
        self.runtime.get(self.port_ids[port])
    }
}

pub fn options_from(extra: &[(&str, f64)]) -> Options {
    let mut opts = Options::default();
    let map: BTreeMap<String, f64> = extra.iter().map(|(k, v)| (k.to_string(), *v)).collect();
    opts.contact = map.get("contact").copied().unwrap_or(1.0) > 0.5;
    opts.flex = map.get("flex").copied().unwrap_or(1.0) > 0.5;
    opts.planar = map.get("planar").copied().unwrap_or(0.0) > 0.5;
    for (k, v) in &map {
        if let Some(rest) = k.strip_prefix("initial.") {
            if let Some(n) = rest.strip_suffix(".angle") {
                opts.initial_angles.insert(n.into(), *v);
            }
            if let Some(n) = rest.strip_suffix(".speed") {
                opts.initial_speeds.insert(n.into(), *v);
            }
        }
    }
    for (k, name) in ["vx", "vy", "vz", "wx", "wy", "wz"].iter().enumerate() {
        opts.initial_twist[k] = map
            .get(&format!("initial.base.{name}"))
            .copied()
            .unwrap_or(0.0);
    }
    opts
}

pub fn midpoint() -> Integrator {
    Integrator::ImplicitMidpoint(NewtonConfig {
        max_iterations: 40,
        min_line_search: 1.0 / 4096.0,
        ..NewtonConfig::default()
    })
}
pub fn euler() -> Integrator {
    Integrator::BackwardEuler(NewtonConfig {
        max_iterations: 40,
        min_line_search: 1.0 / 4096.0,
        ..NewtonConfig::default()
    })
}
