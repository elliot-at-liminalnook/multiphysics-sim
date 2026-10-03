//! Flatten a hierarchical document into the shared flat `ModelWorld`.
//!
//! Every flattened element keeps its instance path as a persistent identity
//! (`driver/bridge/q1`); every subsystem instance becomes a nested description
//! group, so selection, graphs and live values map back to the right level.
//! Boundary ports are aliases: nets are merged across levels with a union-find
//! and each merged net becomes one `ModelWorld` connection.
use crate::document::*;
use crate::resolve::{Finding, Resolver};
use crate::SystemError;
use sim_core::{BehaviorId, BehaviorRegistry, ModelWorld, PortId};
use sim_inspect::GroupDescription;
use sim_inspect::model::{ComponentIdentity, IdentityBindings};
use sim_inspect::spatial::{GeometryProvenance, SpatialDescription, SpatialPart, SpatialShape, SPATIAL_VERSION};
use std::collections::BTreeMap;

pub struct Flattened {
    pub model: ModelWorld,
    pub identities: IdentityBindings,
    /// Content hash of the document this model was flattened from.
    pub source_hash: String,
    pub revision: u64,
    /// Element path → behavior.
    pub components: BTreeMap<String, BehaviorId>,
    /// `path#port` → port, for every element port.
    pub ports: BTreeMap<String, PortId>,
    /// Leaf parts with world placement, for the physical view.
    pub parts: Vec<SpatialPart>,
    /// Subsystem instance path → world placement.
    pub subsystems: BTreeMap<String, WorldPlacement>,
    pub findings: Vec<Finding>,
    /// Hosted root instances (`SystemDocument::links`), left out of `model`:
    /// the host runs them from their linked files.
    pub hosted: BTreeMap<String, Hosted>,
    /// The root nets that join hosted instances (the authored wiring the
    /// host checks against what it runs); not connections in `model`.
    pub hosted_nets: Vec<Net>,
}

/// A hosted instance as authored: its type, linked file and the parameter
/// values the document gives it (for `control.external`, the `sense.*` /
/// `act.*` names that declare its port members).
#[derive(Debug, Clone, PartialEq)]
pub struct Hosted {
    pub component_type: String,
    pub label: String,
    /// `FileLink::path`, relative to the system file's directory.
    pub path: String,
    pub parameters: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldPlacement {
    pub position: [f32; 3],
    pub rotation_xyzw: [f32; 4],
}

impl WorldPlacement {
    pub const IDENTITY: Self = Self { position: [0.; 3], rotation_xyzw: [0., 0., 0., 1.] };
    pub fn then(&self, local: &Placement) -> Self {
        Self {
            position: add(self.position, rotate(self.rotation_xyzw, local.position)),
            rotation_xyzw: normalize(mul(self.rotation_xyzw, local.rotation_xyzw)),
        }
    }
}

pub fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let [x, y, z, w] = q;
    let u = [x, y, z];
    let uv = cross(u, v);
    let uuv = cross(u, uv);
    [v[0] + 2. * (w * uv[0] + uuv[0]), v[1] + 2. * (w * uv[1] + uuv[1]), v[2] + 2. * (w * uv[2] + uuv[2])]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}
fn normalize(q: [f32; 4]) -> [f32; 4] {
    let n = q.iter().map(|v| v * v).sum::<f32>().sqrt();
    if n > 0. { q.map(|v| v / n) } else { [0., 0., 0., 1.] }
}

struct UnionFind {
    parent: Vec<usize>,
}
impl UnionFind {
    fn find(&mut self, x: usize) -> usize {
        let mut root = x;
        while self.parent[root] != root {
            root = self.parent[root];
        }
        let mut x = x;
        while self.parent[x] != root {
            let next = self.parent[x];
            self.parent[x] = root;
            x = next;
        }
        root
    }
    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.parent[b] = a;
        }
    }
}

struct Builder<'a> {
    resolver: Resolver<'a>,
    registry: &'a BehaviorRegistry,
    model: ModelWorld,
    identities: IdentityBindings,
    components: BTreeMap<String, BehaviorId>,
    ports: BTreeMap<String, PortId>,
    /// Terminal keys: `path#port` for element ports, `path@port` for boundary aliases.
    keys: BTreeMap<String, usize>,
    uf: UnionFind,
    parts: Vec<SpatialPart>,
    subsystems: BTreeMap<String, WorldPlacement>,
    source_hash: String,
    hosted: BTreeMap<String, Hosted>,
}

impl Builder<'_> {
    fn key(&mut self, key: String) -> usize {
        if let Some(k) = self.keys.get(&key) {
            return *k;
        }
        let k = self.uf.parent.len();
        self.uf.parent.push(k);
        self.keys.insert(key, k);
        k
    }

    fn terminal_key(&self, path: &str, definition: &Definition, terminal: &Terminal) -> String {
        match terminal {
            Terminal::Boundary { boundary } => format!("{path}@{boundary}"),
            Terminal::Port { instance, port } => {
                let child = join_path(path, instance);
                match definition.instances.get(instance).map(|i| &i.kind) {
                    Some(InstanceKind::Subsystem { .. }) => format!("{child}@{port}"),
                    _ => format!("{child}#{port}"),
                }
            }
        }
    }

    fn expand(&mut self, path: &str, definition_id: &str, env: &BTreeMap<String, f64>, frame: WorldPlacement, group: Option<String>, depth: usize) -> Result<(), SystemError> {
        if depth > 64 {
            return Err(SystemError::Invalid("hierarchy deeper than 64 levels".into()));
        }
        let definition = self.resolver.definition(definition_id)?.clone();
        for (name, instance) in &definition.instances {
            let here = join_path(path, name);
            let world = frame.then(&instance.placement);
            // A hosted root instance is not compiled: the host runs it from its linked file.
            if let (Some(link), InstanceKind::Element { component_type }) = (self.resolver.document.links.get(name).filter(|_| definition_id == self.resolver.document.root), &instance.kind) {
                let mut parameters = BTreeMap::new();
                for (parameter, binding) in &instance.parameters {
                    parameters.insert(parameter.clone(), resolve_binding(binding, env, &here, parameter)?);
                }
                self.hosted.insert(name.clone(), Hosted { component_type: component_type.clone(), label: instance.label.clone(), path: link.path.clone(), parameters });
                continue;
            }
            match &instance.kind {
                InstanceKind::Element { component_type } => {
                    let mut values = Vec::new();
                    for (parameter, binding) in &instance.parameters {
                        values.push((parameter.clone(), resolve_binding(binding, env, &here, parameter)?));
                    }
                    let object = self.model.add_object(if instance.label.is_empty() { name.clone() } else { instance.label.clone() });
                    let created = self
                        .model
                        .instantiate(self.registry, object, component_type, values.iter().map(|(k, v)| (k.as_str(), *v)))
                        .map_err(|e| SystemError::Invalid(format!("{here}: {e}")))?;
                    self.identities.components.insert(
                        created.behavior,
                        ComponentIdentity { id: here.clone(), persistent: true, source: None, cad: None, group: group.clone() },
                    );
                    self.components.insert(here.clone(), created.behavior);
                    for (port, id) in &created.ports {
                        let key = format!("{here}#{port}");
                        self.key(key.clone());
                        self.ports.insert(key, *id);
                    }
                    let appearance = instance.appearance.clone().unwrap_or_else(|| default_appearance(component_type));
                    self.parts.push(SpatialPart {
                        id: format!("part/{here}"),
                        component: here.clone(),
                        label: if instance.label.is_empty() { name.clone() } else { instance.label.clone() },
                        provenance: GeometryProvenance::Illustrative { explanation: "System-builder placement and display shape; not CAD geometry and not a physics input".into() },
                        position: world.position,
                        rotation_xyzw: world.rotation_xyzw,
                        exploded_offset: [0.; 3],
                        color_srgb: appearance.color_srgb,
                        shape: appearance.shape,
                        model: appearance.model,
                    });
                }
                InstanceKind::Subsystem { definition: child } => {
                    let child_definition = self.resolver.definition(child)?;
                    let mut child_env = BTreeMap::new();
                    for (parameter, decl) in &child_definition.parameters {
                        let value = match instance.parameters.get(parameter) {
                            Some(binding) => resolve_binding(binding, env, &here, parameter)?,
                            None => decl.default.ok_or_else(|| SystemError::Invalid(format!("{here} needs parameter `{parameter}` ({})", decl.unit)))?,
                        };
                        child_env.insert(parameter.clone(), value);
                    }
                    self.identities.groups.insert(
                        here.clone(),
                        GroupDescription { id: here.clone(), label: format!("{} · {}", if instance.label.is_empty() { name } else { &instance.label }, child_definition.label), parent: group.clone() },
                    );
                    self.subsystems.insert(here.clone(), world);
                    for port in child_definition.ports.keys() {
                        self.key(format!("{here}@{port}"));
                    }
                    self.expand(&here, child, &child_env, world, Some(here.clone()), depth + 1)?;
                }
            }
        }
        for net in &definition.nets {
            let keys: Vec<String> = net.terminals.iter().map(|t| self.terminal_key(path, &definition, t)).collect();
            let ids: Vec<usize> = keys.into_iter().map(|k| self.key(k)).collect();
            for pair in ids.windows(2) {
                self.uf.union(pair[0], pair[1]);
            }
        }
        Ok(())
    }
}

fn resolve_binding(binding: &ParameterBinding, env: &BTreeMap<String, f64>, path: &str, parameter: &str) -> Result<f64, SystemError> {
    match binding {
        ParameterBinding::Value { value, .. } => Ok(*value),
        ParameterBinding::Parameter { parameter: from } => env
            .get(from)
            .copied()
            .ok_or_else(|| SystemError::Invalid(format!("{path}.{parameter} inherits `{from}`, which has no value"))),
    }
}

/// Display shape and color by component family. Presentation only. The
/// shape approximates the bounds of the family's catalog model (see
/// `library/models/catalog.json`); viewers without the catalog draw it.
/// Abstract elements (thermal nodes, sources, ground) are small markers.
pub fn default_appearance(component_type: &str) -> Appearance {
    let domain = component_type.split('.').next().unwrap_or("");
    let color = match domain {
        "electrical" => [0.80, 0.52, 0.30],
        "thermal" => [0.86, 0.36, 0.28],
        "rotational" | "translational" | "multibody" => [0.62, 0.66, 0.72],
        "control" | "sensing" => [0.33, 0.52, 0.86],
        "robot" => [0.40, 0.62, 0.52],
        _ => [0.60, 0.60, 0.64],
    };
    let b = |x: f32, y: f32, z: f32| SpatialShape::Box { size: [x, y, z] };
    let shape = match component_type {
        "electrical.mosfet" => b(0.010, 0.0185, 0.0046),
        "electrical.capacitor" => SpatialShape::Cylinder { radius: 0.004, length: 0.012 },
        "electrical.resistor" | "bridge.thermistor" | "electrical.diode" => b(0.013, 0.004, 0.0027),
        "electrical.inductor" => b(0.012, 0.0082, 0.012),
        "control.pwm" | "control.h_bridge_pwm" | "control.pi" => b(0.006, 0.0018, 0.005),
        "electrical.voltage_sense" => b(0.003, 0.0012, 0.0023),
        "robot.battery" => b(0.066, 0.0185, 0.057),
        "robot.motor_unit" => b(0.054, 0.0435, 0.020),
        "bridge.brushed_motor" | "bridge.motor" => SpatialShape::Cylinder { radius: 0.012, length: 0.041 },
        "rotational.inertia" => SpatialShape::Cylinder { radius: 0.015, length: 0.010 },
        "part.stepper_motor" => b(0.0423, 0.064, 0.0423),
        "part.bldc_motor" => SpatialShape::Cylinder { radius: 0.016, length: 0.034 },
        "part.propeller" => b(0.254, 0.008, 0.018),
        "part.drive_wheel" => SpatialShape::Cylinder { radius: 0.04, length: 0.024 },
        "part.timing_belt" => SpatialShape::Cylinder { radius: 0.008, length: 0.016 },
        "part.rack_pinion" => b(0.08, 0.01, 0.03),
        "part.solenoid" => b(0.02, 0.038, 0.016),
        "part.coreless_motor" | "part.brushed_motor_eq" => SpatialShape::Cylinder { radius: 0.012, length: 0.041 },
        "robot.h_bridge" | "robot.switchable_h_bridge" | "actuator.pwm_driver" => b(0.04, 0.01, 0.03),
        _ => SpatialShape::Sphere { radius: 0.0025 },
    };
    Appearance { shape, color_srgb: color, model: None }
}

pub fn flatten(document: &SystemDocument, registry: &BehaviorRegistry) -> Result<Flattened, SystemError> {
    let resolver = Resolver::new(document, registry);
    resolver.validate()?;
    let findings = resolver.findings();
    let root = resolver.definition(&document.root)?;
    let mut env = BTreeMap::new();
    for (parameter, decl) in &root.parameters {
        env.insert(parameter.clone(), decl.default.ok_or_else(|| SystemError::Invalid(format!("root parameter `{parameter}` needs a default")))?);
    }
    let mut builder = Builder {
        resolver: Resolver::new(document, registry),
        registry,
        model: ModelWorld::default(),
        identities: IdentityBindings::default(),
        components: BTreeMap::new(),
        ports: BTreeMap::new(),
        keys: BTreeMap::new(),
        uf: UnionFind { parent: Vec::new() },
        parts: Vec::new(),
        subsystems: BTreeMap::new(),
        source_hash: document.content_hash(),
        hosted: BTreeMap::new(),
    };
    builder.expand("", &document.root, &env, WorldPlacement::IDENTITY, None, 0)?;
    // Group element ports by merged net; each set with two or more element
    // ports becomes one connection. Composite ports connect member-wise.
    let mut sets: BTreeMap<usize, Vec<PortId>> = BTreeMap::new();
    let keys: Vec<(String, usize)> = builder.keys.iter().map(|(k, v)| (k.clone(), *v)).collect();
    for (key, index) in keys {
        if let Some(port) = builder.ports.get(&key).copied() {
            let root = builder.uf.find(index);
            sets.entry(root).or_default().push(port);
        }
    }
    for ports in sets.into_values() {
        if ports.len() >= 2 {
            builder.model.connect(ports);
        }
    }
    for (i, _) in builder.model.connections.iter().enumerate() {
        builder.identities.connections.insert(i, format!("net/{i}"));
    }
    // Root nets on hosted instances join hosted instances only (`Resolver::check_net`):
    // their terminals have no element port above, so they made no connection.
    let hosted_nets = root.nets.iter().filter(|n| n.terminals.iter().any(|t| t.instance().is_some_and(|i| document.hosted(&document.root, i)))).cloned().collect();
    Ok(Flattened {
        hosted: builder.hosted,
        hosted_nets,
        source_hash: builder.source_hash,
        revision: document.revision,
        model: builder.model,
        identities: builder.identities,
        components: builder.components,
        ports: builder.ports,
        parts: builder.parts,
        subsystems: builder.subsystems,
        findings,
    })
}

impl Flattened {
    /// Physical presentation of every leaf element, bound to a description.
    pub fn spatial(&self, description_id: &str, title: &str) -> SpatialDescription {
        SpatialDescription {
            version: SPATIAL_VERSION,
            description_id: description_id.into(),
            title: title.into(),
            length_unit: "m".into(),
            coordinate_frame: "right_handed_y_up".into(),
            provenance: "System-builder placements (illustrative display geometry; physics comes from the system file)".into(),
            parts: self.parts.clone(),
        }
    }
}
