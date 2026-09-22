//! Shared, graphics-independent selection. Bundles retain every source identity.
use crate::{InspectionError, SystemDescription, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[cfg(unix)]
pub mod native;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SelectionTarget {
    #[default]
    None,
    Components {
        ids: BTreeSet<String>,
    },
    Ports {
        ids: BTreeSet<String>,
    },
    Nets {
        ids: BTreeSet<String>,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectionDetails {
    pub components: BTreeSet<String>,
    pub ports: BTreeSet<String>,
    pub nets: BTreeSet<String>,
}

impl SelectionTarget {
    pub fn component(id: impl Into<String>) -> Self {
        Self::Components {
            ids: BTreeSet::from([id.into()]),
        }
    }
    pub fn net(id: impl Into<String>) -> Self {
        Self::Nets {
            ids: BTreeSet::from([id.into()]),
        }
    }
    pub fn validate(&self, d: &SystemDescription) -> Result<(), InspectionError> {
        let (ids, valid) = match self {
            Self::None => return Ok(()),
            Self::Components { ids } => (ids, ids.iter().all(|id| d.components.contains_key(id))),
            Self::Ports { ids } => (ids, ids.iter().all(|id| d.ports.contains_key(id))),
            Self::Nets { ids } => (ids, ids.iter().all(|id| d.nets.contains_key(id))),
        };
        ensure(
            !ids.is_empty() && valid,
            "empty or unknown selection identity",
        )
    }

    pub fn resolve(&self, d: &SystemDescription) -> Result<SelectionDetails, InspectionError> {
        self.validate(d)?;
        let mut result = SelectionDetails::default();
        match self {
            Self::None => return Ok(result),
            Self::Components { ids } => {
                result.components = ids.clone();
                result.ports.extend(
                    d.ports
                        .values()
                        .filter(|p| ids.contains(&p.component))
                        .map(|p| p.id.clone()),
                );
            }
            Self::Ports { ids } => {
                result.ports = ids.clone();
            }
            Self::Nets { ids } => {
                result.nets = ids.clone();
                result
                    .ports
                    .extend(ids.iter().flat_map(|id| d.nets[id].ports.iter().cloned()));
            }
        }
        // A composite selection includes its declared descendant terminals.
        loop {
            let descendants: Vec<_> = d
                .ports
                .values()
                .filter(|p| {
                    p.composite_parent
                        .as_ref()
                        .is_some_and(|parent| result.ports.contains(parent))
                })
                .map(|p| p.id.clone())
                .collect();
            let old = result.ports.len();
            result.ports.extend(descendants);
            if old == result.ports.len() {
                break;
            }
        }
        result
            .components
            .extend(result.ports.iter().map(|id| d.ports[id].component.clone()));
        if !matches!(self, Self::Nets { .. }) {
            result.nets.extend(
                d.nets
                    .values()
                    .filter(|n| n.ports.iter().any(|p| result.ports.contains(p)))
                    .map(|n| n.id.clone()),
            );
        }
        Ok(result)
    }
}
