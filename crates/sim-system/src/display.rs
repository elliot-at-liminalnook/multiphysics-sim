//! Authored display layout and discussions. None of this changes physics.
//! SI metres in the enclosing definition's right-handed, Y-up frame.
use crate::{Command, InstanceKind, Placement, Resolver, SystemDocument, SystemError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SEMANTICS: &str = "display_only: placement, grid, pins and comments do not change CAD geometry, joints, contact, mass, inertia or simulation initial conditions";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Plane {
    Xy,
    #[default]
    Xz,
    Yz,
}
impl Plane {
    pub fn axes(self) -> (usize, usize, usize) {
        match self {
            Self::Xy => (0, 1, 2),
            Self::Xz => (0, 2, 1),
            Self::Yz => (1, 2, 0),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Grid {
    pub visible: bool,
    pub snap: bool,
    pub spacing_m: f32,
    pub origin_m: [f32; 3],
    pub plane: Plane,
}
impl Default for Grid {
    fn default() -> Self {
        Self {
            visible: true,
            snap: true,
            spacing_m: 0.01,
            origin_m: [0.; 3],
            plane: Plane::Xz,
        }
    }
}
impl Grid {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
    pub fn validate(&self) -> Result<(), SystemError> {
        if !self.spacing_m.is_finite()
            || self.spacing_m < 1e-6
            || self.spacing_m > 1000.
            || !self.origin_m.iter().all(|v| v.is_finite())
        {
            return Err(SystemError::Invalid(
                "display grid needs finite origin and spacing in 0.000001..1000 metres".into(),
            ));
        }
        Ok(())
    }
    pub fn point(&self, mut p: [f32; 3], snap: bool) -> Result<[f32; 3], SystemError> {
        self.validate()?;
        if !p.iter().all(|v| v.is_finite()) {
            return Err(SystemError::Invalid(
                "display position must be finite metres".into(),
            ));
        }
        if snap {
            for (v, o) in p.iter_mut().zip(self.origin_m) {
                *v = o + ((*v - o) / self.spacing_m).round() * self.spacing_m;
            }
        }
        Ok(p)
    }
}

/// Preview and commit use this identical function. One snapped delta preserves
/// selection offsets (parts do not collapse independently onto grid points).
pub fn moves(
    doc: &SystemDocument,
    registry: &sim_core::BehaviorRegistry,
    at: &str,
    names: &[String],
    position_m: [f32; 3],
    snap: bool,
) -> Result<Vec<Command>, SystemError> {
    let resolver = Resolver::new(doc, registry);
    let id = resolver.definition_id_at(at)?;
    let d = &doc.definitions[&id];
    let first = names
        .first()
        .and_then(|n| d.instances.get(n))
        .ok_or_else(|| SystemError::Invalid("select at least one existing instance".into()))?;
    let target = d.grid.point(position_m, snap)?;
    let delta: [f32; 3] = std::array::from_fn(|i| target[i] - first.placement.position[i]);
    names
        .iter()
        .map(|name| {
            let i = d
                .instances
                .get(name)
                .ok_or_else(|| SystemError::Invalid(format!("unknown instance {name}")))?;
            let mut placement = i.placement.clone();
            for k in 0..3 {
                placement.position[k] += delta[k];
            }
            Ok(Command::MoveInstance {
                at: at.into(),
                name: name.clone(),
                placement,
            })
        })
        .collect()
}

/// Threads, comments and their persistence rules are shared with the
/// schematic and lessons (`sim-annotate`); here they anchor to instances by
/// lineage so renames and moves keep them attached.
pub type Discussions = sim_annotate::Discussions<Target>;
pub type Thread = sim_annotate::Thread<Target>;
pub type Comment = sim_annotate::Comment<Target>;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub path: String,
    #[serde(default)]
    pub label: String,
    /// Stable instance lineage, retained when a target is deleted.
    #[serde(default)]
    pub lineage: Vec<String>,
    #[serde(default)]
    pub missing: bool,
}

pub fn paths(doc: &SystemDocument) -> BTreeMap<String, (Vec<String>, Placement)> {
    fn walk(
        doc: &SystemDocument,
        def: &str,
        prefix: &str,
        lineage: &[String],
        out: &mut BTreeMap<String, (Vec<String>, Placement)>,
        depth: usize,
    ) {
        if depth > 64 {
            return;
        }
        let Some(d) = doc.definitions.get(def) else {
            return;
        };
        for (name, i) in &d.instances {
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let mut chain = lineage.to_vec();
            chain.push(identity(doc, def, name, i));
            out.insert(path.clone(), (chain.clone(), i.placement.clone()));
            if let InstanceKind::Subsystem { definition } = &i.kind {
                walk(doc, definition, &path, &chain, out, depth + 1);
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(doc, &doc.root, "", &[], &mut out, 0);
    out
}
fn identity(doc: &SystemDocument, def: &str, name: &str, i: &crate::InstanceSpec) -> String {
    if !i.display_id.is_empty() {
        return i.display_id.clone();
    }
    blake3::hash(format!("{}:{def}:{name}", doc.root).as_bytes()).to_hex()[..24].into()
}
pub fn assign_ids(doc: &mut SystemDocument) {
    let assignments: Vec<_> = doc
        .definitions
        .iter()
        .flat_map(|(d, def)| {
            def.instances
                .iter()
                .filter(|(_, i)| i.display_id.is_empty())
                .map(|(n, i)| (d.clone(), n.clone(), identity(doc, d, n, i)))
        })
        .collect();
    for (d, n, id) in assignments {
        doc.definitions
            .get_mut(&d)
            .unwrap()
            .instances
            .get_mut(&n)
            .unwrap()
            .display_id = id;
    }
}
pub fn bind(doc: &SystemDocument, path: &str) -> Result<Target, SystemError> {
    let all = paths(doc);
    let (lineage, _) = all
        .get(path)
        .ok_or_else(|| SystemError::Invalid(format!("unknown discussion target {path}")))?;
    Ok(Target {
        path: path.into(),
        label: path.rsplit('/').next().unwrap_or(path).into(),
        lineage: lineage.clone(),
        missing: false,
    })
}
/// Every instance path with its lineage and placement: the index system
/// discussion anchors resolve against.
pub type PathIndex = BTreeMap<String, (Vec<String>, Placement)>;
/// Match identity, not a reused name. Shared-definition occurrences are
/// distinguished by surviving ancestor identities. Ambiguity stays missing.
impl sim_annotate::Anchor for Target {
    type Index = PathIndex;
    fn validate(&self) -> Result<(), String> {
        if self.path.is_empty()
            || self.path.len() > 4096
            || self.label.len() > 1000
            || self.lineage.len() > 65
        {
            return Err("invalid discussion reference".into());
        }
        Ok(())
    }
    fn label(&self) -> String {
        self.label.clone()
    }
    fn missing(&self) -> bool {
        self.missing
    }
    fn refresh(&mut self, all: &PathIndex) -> bool {
        let t = self;
        if t.lineage.is_empty() {
            if let Some((chain, _)) = all.get(&t.path) {
                t.lineage = chain.clone();
            }
        }
        let mut candidates: Vec<_> = all
            .iter()
            .filter(|(_, (chain, _))| chain.last() == t.lineage.last() && !t.lineage.is_empty())
            .map(|(path, (chain, _))| {
                (
                    chain.iter().filter(|id| t.lineage.contains(id)).count(),
                    path,
                    chain,
                )
            })
            .collect();
        candidates.sort_by(|a, b| b.0.cmp(&a.0));
        if let Some((_, path, chain)) = candidates
            .first()
            .filter(|(score, _, _)| candidates.get(1).is_none_or(|c| c.0 < *score))
        {
            t.path = (*path).clone();
            t.lineage = (*chain).clone();
            t.missing = false;
            true
        } else {
            t.missing = true;
            false
        }
    }
}
pub fn refresh(doc: &mut SystemDocument) {
    let all = paths(doc);
    sim_annotate::refresh(&mut doc.discussions, &all);
}
pub fn validate_thread(t: &Thread) -> Result<(), SystemError> {
    sim_annotate::validate_thread(t).map_err(SystemError::Invalid)
}

/// Hash used to decide whether presentation edits require rebuilding geometry.
/// Notes, grid settings and identity metadata cannot restart a simulation.
pub fn scene_hash(document: &SystemDocument) -> String {
    let mut d = document.clone();
    d.discussions = Discussions::default();
    for def in d.definitions.values_mut() {
        def.grid = Grid::default();
        def.icon.clear();
        for i in def.instances.values_mut() {
            i.display_id.clear();
        }
    }
    d.content_hash()
}

/// Readable text for hosts that render the persistent link chips separately.
pub fn plain_comment(body: &str) -> String {
    sim_annotate::plain_comment(body)
}

/// Compact UI timestamp; the stored timestamp remains an exact UTC instant.
pub fn relative_time(timestamp: &str) -> String {
    sim_annotate::relative_time(timestamp)
}
