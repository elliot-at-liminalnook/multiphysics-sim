//! Engineer-owned analysis metadata. Never input to the compiler or CAD model.
use crate::{Selection, projection::NodeSource};
use serde::{Deserialize, Serialize};
use sim_inspect::{DiagramState, GroupDescription, SystemDescription};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum Target {
    Component(String),
    Group(String),
    Net(String),
}
impl Target {
    pub fn key(&self) -> String {
        match self {
            Self::Component(id) => format!("component/{id}"),
            Self::Group(id) => format!("group/{id}"),
            Self::Net(id) => format!("net/{id}"),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Annotation {
    pub target: Target,
    pub label: Option<String>,
    pub text: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisGroup {
    pub id: String,
    pub label: String,
    pub members: BTreeSet<String>,
    pub color: [u8; 3],
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedView {
    pub state: DiagramState,
    pub selected: Option<Selection>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workspace {
    pub version: u32,
    pub description_id: String,
    pub annotations: BTreeMap<String, Annotation>,
    pub groups: BTreeMap<String, AnalysisGroup>,
    pub views: BTreeMap<String, SavedView>,
    pub collapsed: BTreeSet<String>,
    pub focus: Option<NodeSource>,
    pub dimmed_domains: BTreeSet<String>,
    pub next_group: u64,
}
impl Workspace {
    pub fn new(d: &SystemDescription) -> Self {
        Self {
            version: 1,
            description_id: d.id.clone(),
            annotations: BTreeMap::new(),
            groups: BTreeMap::new(),
            views: BTreeMap::new(),
            collapsed: d.groups.keys().cloned().collect(),
            focus: None,
            dimmed_domains: BTreeSet::new(),
            next_group: 1,
        }
    }
    pub fn annotation(&self, target: &Target) -> Option<&Annotation> {
        self.annotations.get(&target.key())
    }
    pub fn validate(&self, d: &SystemDescription) -> Result<(), String> {
        if self.version != 1 || self.description_id != d.id {
            return Err("Workspace belongs to another captured model or format version".into());
        }
        if self.next_group == 0 || self.next_group == u64::MAX {
            return Err("Invalid next analysis-group identity".into());
        }
        let mut assigned = BTreeMap::new();
        for (id, group) in &self.groups {
            if id != &group.id
                || !id.starts_with("analysis/group/")
                || d.groups.contains_key(id)
                || group.label.trim().is_empty()
                || group.members.is_empty()
            {
                return Err(
                    "Analysis groups need a unique ID, a name and at least one component".into(),
                );
            }
            for component in &group.members {
                if !d.components.contains_key(component) {
                    return Err(format!(
                        "Unknown component in group {}: {component}",
                        group.label
                    ));
                }
                if let Some(other) = assigned.insert(component, &group.label) {
                    return Err(format!(
                        "{} already belongs to group {other}; groups must be disjoint",
                        d.components[component].label
                    ));
                }
            }
        }
        let valid = |target: &Target| match target {
            Target::Component(id) => d.components.contains_key(id),
            Target::Net(id) => d.nets.contains_key(id),
            Target::Group(id) => d.groups.contains_key(id) || self.groups.contains_key(id),
        };
        for (key, note) in &self.annotations {
            if key != &note.target.key() || !valid(&note.target) {
                return Err(
                    "Annotation references a missing component, group or connection".into(),
                );
            }
        }
        if self
            .collapsed
            .iter()
            .any(|id| !d.groups.contains_key(id) && !self.groups.contains_key(id))
        {
            return Err("Workspace references an unknown collapsed group".into());
        }
        if self.focus.as_ref().is_some_and(|source| {
            !valid(&match source {
                NodeSource::Component(id) => Target::Component(id.clone()),
                NodeSource::Group(id) => Target::Group(id.clone()),
            })
        }) {
            return Err("Workspace focus no longer exists".into());
        }
        // Each view is validated against its regenerated projection before use.
        // Reject non-finite coordinates even in currently inactive views.
        for (id, view) in &self.views {
            if id != &view.state.description_id
                || !view.state.zoom.is_finite()
                || view.state.zoom <= 0.
                || !view.state.camera.x.is_finite()
                || !view.state.camera.y.is_finite()
                || view
                    .state
                    .positions
                    .values()
                    .any(|p| !p.x.is_finite() || !p.y.is_finite())
            {
                return Err("Invalid saved diagram view".into());
            }
        }
        Ok(())
    }
    /// A presentation-only clone. Physical components, ports and nets retain IDs;
    /// custom groups replace membership only in this clone, never in the capture.
    pub fn presentation(&self, d: &SystemDescription) -> Result<SystemDescription, String> {
        self.validate(d)?;
        let mut view = d.clone();
        for group in self.groups.values() {
            view.groups.insert(
                group.id.clone(),
                GroupDescription {
                    id: group.id.clone(),
                    label: group.label.clone(),
                    parent: None,
                },
            );
            for id in &group.members {
                let component = view.components.get_mut(id).unwrap();
                if let Some(label) = component
                    .group
                    .as_ref()
                    .and_then(|g| d.groups.get(g))
                    .map(|g| &g.label)
                {
                    if let Some(short) = component.label.strip_prefix(&format!("{label}.")) {
                        component.label = short.into();
                    }
                }
                component.group = Some(group.id.clone());
            }
        }
        for annotation in self.annotations.values() {
            if let Some(label) = &annotation.label {
                match &annotation.target {
                    Target::Component(id) => {
                        view.components.get_mut(id).unwrap().label = label.clone()
                    }
                    Target::Group(id) => view.groups.get_mut(id).unwrap().label = label.clone(),
                    Target::Net(_) => {}
                }
            }
        }
        Ok(view)
    }
}

#[derive(Clone)]
pub struct Journal {
    pub current: Workspace,
    undo: Vec<Workspace>,
    redo: Vec<Workspace>,
}
impl Journal {
    pub fn new(workspace: Workspace) -> Self {
        Self {
            current: workspace,
            undo: vec![],
            redo: vec![],
        }
    }
    pub fn commit(&mut self, next: Workspace, d: &SystemDescription) -> Result<(), String> {
        next.validate(d)?;
        if next != self.current {
            self.undo.push(self.current.clone());
            if self.undo.len() > 100 {
                self.undo.remove(0);
            }
            self.redo.clear();
            self.current = next;
        }
        Ok(())
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn undo(&mut self) -> bool {
        if let Some(previous) = self.undo.pop() {
            self.redo
                .push(std::mem::replace(&mut self.current, previous));
            true
        } else {
            false
        }
    }
    pub fn redo(&mut self) -> bool {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.current, next));
            true
        } else {
            false
        }
    }
}
