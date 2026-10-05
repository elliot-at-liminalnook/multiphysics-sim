//! Conservative display-envelope checks, independent of simulation compilation.
use crate::flatten::{WorldPlacement, default_appearance, rotate};
use crate::{Command, InstanceKind, Resolver, SystemDocument, SystemError, Terminal};
use serde::Serialize;
use sim_core::{BehaviorRegistry, ConnectorKind, PortSchema};
use sim_inspect::spatial::SpatialShape;
use std::collections::{BTreeMap, BTreeSet};

pub const POLICY: &str = "display_envelopes: reject new or increased overlap; allow touching, unchanged/reduced existing overlap, authored internals of newly placed subsystems, and direct rotational/translational port mates. Uses world AABBs of authored/default display shapes, not CAD mesh collision or physics contact. Electrical/signal links do not permit overlap.";
const EPS: f32 = 1e-6;

#[derive(Debug, Clone, Serialize)]
pub struct Bounds {
    pub path: String,
    pub min_m: [f32; 3],
    pub max_m: [f32; 3],
    #[serde(skip)]
    ids: Vec<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Conflict {
    pub first: Bounds,
    pub second: Bounds,
    pub penetration_m: [f32; 3],
}
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub allowed: bool,
    pub policy: &'static str,
    pub bounds_frame: &'static str,
    pub tolerance_m: f32,
    pub conflicts: Vec<Conflict>,
}
impl Report {
    pub fn require_allowed(&self) -> Result<(), SystemError> {
        if self.allowed {
            return Ok(());
        }
        Err(SystemError::Invalid(format!(
            "display placement overlaps unrelated parts: {}. Move apart or explicitly attach compatible mechanical ports. Display envelopes only; source placement unchanged.",
            self.conflicts
                .iter()
                .take(8)
                .map(|c| format!("{} / {}", c.first.path, c.second.path))
                .collect::<Vec<_>>()
                .join(", ")
        )))
    }
}

/// Transactions that can create new display interference. Metadata edits and
/// regrouping an unchanged assembly do not reinterpret an existing layout.
pub fn needs_check(commands: &[Command]) -> bool {
    commands.iter().any(|c| {
        matches!(
            c,
            Command::AddInstance { .. }
                | Command::MoveInstance { .. }
                | Command::Swap { .. }
                | Command::SetAppearance { .. }
        )
    })
}

pub fn preview(
    doc: &SystemDocument,
    registry: &BehaviorRegistry,
    commands: &[Command],
) -> Result<Report, SystemError> {
    let mut after = doc.clone();
    crate::apply(&mut after, registry, commands)?;
    check(doc, &after, registry)
}

struct Layout {
    bounds: Vec<Bounds>,
    mates: Vec<(String, String)>,
}
fn layout(doc: &SystemDocument, registry: &BehaviorRegistry) -> Result<Layout, SystemError> {
    let mut doc = doc.clone();
    crate::display::assign_ids(&mut doc);
    let mut out = Layout {
        bounds: vec![],
        mates: vec![],
    };
    fn walk(
        doc: &SystemDocument,
        registry: &BehaviorRegistry,
        id: &str,
        path: &str,
        ids: &[String],
        frame: WorldPlacement,
        out: &mut Layout,
        depth: usize,
    ) -> Result<(), SystemError> {
        if depth > 64 {
            return Err(SystemError::Invalid("display hierarchy too deep".into()));
        }
        let d = doc
            .definitions
            .get(id)
            .ok_or_else(|| SystemError::Invalid(format!("unknown definition {id}")))?;
        let join = |name: &str| {
            if path.is_empty() {
                name.to_string()
            } else {
                format!("{path}/{name}")
            }
        };
        let resolver = Resolver::new(doc, registry);
        for net in &d.nets {
            let mut physical = Vec::new();
            for t in &net.terminals {
                if let Terminal::Port { instance, .. } = t {
                    if let Some(PortSchema::Acausal(k)) =
                        resolver.terminal_schema(id, t, &mut BTreeSet::new())?
                    {
                        if k == ConnectorKind::Rotational || k == ConnectorKind::Translational {
                            physical.push(join(instance));
                        }
                    }
                }
            }
            for i in 0..physical.len() {
                for j in i + 1..physical.len() {
                    out.mates.push((physical[i].clone(), physical[j].clone()));
                }
            }
        }
        for (name, spec) in &d.instances {
            let p = join(name);
            let f = frame.then(&spec.placement);
            let mut lineage = ids.to_vec();
            lineage.push(spec.display_id.clone());
            match &spec.kind {
                InstanceKind::Subsystem { definition } => {
                    walk(doc, registry, definition, &p, &lineage, f, out, depth + 1)?
                }
                InstanceKind::Element { component_type } => {
                    let shape = spec
                        .appearance
                        .clone()
                        .unwrap_or_else(|| default_appearance(component_type))
                        .shape;
                    let half = match shape {
                        SpatialShape::Box { size } => size.map(|v| v.abs() * 0.5),
                        SpatialShape::Cylinder { radius, length } => {
                            [radius.abs(), length.abs() * 0.5, radius.abs()]
                        }
                        SpatialShape::Sphere { radius } => [radius.abs(); 3],
                    };
                    if !half.iter().all(|v| v.is_finite() && *v > 0.) {
                        return Err(SystemError::Invalid(format!(
                            "{p}: display envelope requires positive finite dimensions"
                        )));
                    }
                    let axes: [[f32; 3]; 3] = std::array::from_fn(|i| {
                        let mut v = [0.; 3];
                        v[i] = half[i];
                        rotate(f.rotation_xyzw, v)
                    });
                    let extent: [f32; 3] =
                        std::array::from_fn(|k| axes.iter().map(|a| a[k].abs()).sum());
                    out.bounds.push(Bounds {
                        path: p,
                        min_m: std::array::from_fn(|k| f.position[k] - extent[k]),
                        max_m: std::array::from_fn(|k| f.position[k] + extent[k]),
                        ids: lineage,
                    });
                }
                // Blocks are code; a generated assembly's own geometry comes from its source.
                InstanceKind::Generated { .. } | InstanceKind::Block { .. } => {}
            }
        }
        Ok(())
    }
    walk(
        &doc,
        registry,
        &doc.root,
        "",
        &[],
        WorldPlacement::IDENTITY,
        &mut out,
        0,
    )?;
    Ok(out)
}
fn overlap(a: &Bounds, b: &Bounds) -> Option<[f32; 3]> {
    let p = std::array::from_fn(|k| a.max_m[k].min(b.max_m[k]) - a.min_m[k].max(b.min_m[k]));
    p.iter().all(|v| *v > EPS).then_some(p)
}
fn under(path: &str, ancestor: &str) -> bool {
    path == ancestor
        || path
            .strip_prefix(ancestor)
            .is_some_and(|s| s.starts_with('/'))
}

/// Existing interference may remain or decrease, so old authored assemblies can
/// be repaired incrementally. A rigid translation preserves its internal pairs.
pub fn check(
    before: &SystemDocument,
    after: &SystemDocument,
    registry: &BehaviorRegistry,
) -> Result<Report, SystemError> {
    let old = layout(before, registry)?;
    let new = layout(after, registry)?;
    let old_by_id: BTreeMap<_, _> = old.bounds.iter().map(|b| (b.ids.clone(), b)).collect();
    let old_prefixes: BTreeSet<_> = old
        .bounds
        .iter()
        .flat_map(|b| (1..=b.ids.len()).map(|n| b.ids[..n].to_vec()))
        .collect();
    let mut conflicts = vec![];
    for i in 0..new.bounds.len() {
        for j in i + 1..new.bounds.len() {
            let a = &new.bounds[i];
            let b = &new.bounds[j];
            let Some(p) = overlap(a, b) else { continue };
            if new.mates.iter().any(|(x, y)| {
                (under(&a.path, x) && under(&b.path, y)) || (under(&a.path, y) && under(&b.path, x))
            }) {
                continue;
            }
            // A library subsystem's existing internal layout is authored together.
            let common = a.ids.iter().zip(&b.ids).take_while(|(x, y)| x == y).count();
            if common > 0 && !old_prefixes.contains(&a.ids[..common]) {
                continue;
            }
            if let (Some(oa), Some(ob)) = (old_by_id.get(&a.ids), old_by_id.get(&b.ids)) {
                if let Some(previous) = overlap(oa, ob) {
                    if p.iter().zip(previous).all(|(n, o)| *n <= o + EPS) {
                        continue;
                    }
                }
            }
            conflicts.push(Conflict {
                first: a.clone(),
                second: b.clone(),
                penetration_m: p,
            });
        }
    }
    Ok(Report {
        allowed: conflicts.is_empty(),
        policy: POLICY,
        bounds_frame: "world_metres",
        tolerance_m: EPS,
        conflicts,
    })
}
