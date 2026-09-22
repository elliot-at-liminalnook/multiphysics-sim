//! Saved subsystem definitions and the element catalog, as one library.
//! A library file carries a definition plus every definition it places, so it
//! can be imported into any document. Importing records the file and its hash.
use crate::document::*;
use crate::resolve::{element_top_ports, Resolver};
use crate::commands::describe;
use crate::SystemError;
use serde::{Deserialize, Serialize};
use sim_core::{BehaviorRegistry, PortSchema};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryFile {
    pub schema: String,
    /// The definition this file offers.
    pub definition: String,
    /// It and every definition it places.
    pub definitions: BTreeMap<String, Definition>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LibraryEntry {
    pub id: String,
    pub label: String,
    pub description: String,
    pub interface: Option<String>,
    pub path: String,
    pub content_hash: String,
    pub ports: BTreeMap<String, Option<String>>,
}

/// One registry element, as the palette and inspectors show it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ElementEntry {
    pub component_type: String,
    pub display_name: String,
    pub domain: String,
    pub ports: BTreeMap<String, String>,
    pub parameters: Vec<ParameterEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParameterEntry {
    pub name: String,
    pub unit: String,
    pub required: bool,
    pub default: Option<f64>,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
}

/// Elements whose ports are fixed (no wildcard families), for the palette.
pub fn elements(registry: &BehaviorRegistry) -> Vec<ElementEntry> {
    let mut out = Vec::new();
    for descriptor in registry.descriptors() {
        if descriptor.ports.iter().any(|p| p.name.contains('*')) {
            continue;
        }
        let Ok(ports) = element_top_ports(registry, &descriptor.type_id.0, &BTreeSet::new()) else { continue };
        out.push(ElementEntry {
            component_type: descriptor.type_id.0.clone(),
            display_name: descriptor.display_name.to_string(),
            domain: descriptor.type_id.0.split('.').next().unwrap_or("").to_string(),
            ports: ports.iter().map(|(k, v)| (k.clone(), describe(v))).collect(),
            parameters: descriptor
                .parameters
                .iter()
                .flatten()
                .filter(|p| !p.implementation_reference)
                .map(|p| ParameterEntry { name: p.name.clone(), unit: p.unit.clone(), required: p.required, default: p.default, minimum: p.minimum, maximum: p.maximum })
                .collect(),
        });
    }
    out
}

/// Every definition `id` places, transitively, including itself.
pub fn closure(document: &SystemDocument, id: &str) -> Result<BTreeMap<String, Definition>, SystemError> {
    let mut out = BTreeMap::new();
    let mut stack = vec![id.to_string()];
    while let Some(next) = stack.pop() {
        if out.contains_key(&next) {
            continue;
        }
        let d = document.definitions.get(&next).ok_or_else(|| SystemError::Invalid(format!("unknown definition `{next}`")))?.clone();
        for i in d.instances.values() {
            if let InstanceKind::Subsystem { definition } = &i.kind {
                stack.push(definition.clone());
            }
        }
        out.insert(next, d);
    }
    Ok(out)
}

pub fn file_name(id: &str) -> String {
    format!("{id}.definition.json")
}

/// Save a definition (and what it places) to `<library>/<id>.definition.json`.
pub fn save(document: &SystemDocument, id: &str, library: &Path) -> Result<PathBuf, SystemError> {
    let mut definitions = closure(document, id)?;
    for d in definitions.values_mut() {
        d.source = None;
        // Reference images depend on this document's assets; they stay behind.
        d.references.clear();
    }
    let file = LibraryFile { schema: LIBRARY_SCHEMA.into(), definition: id.into(), definitions };
    std::fs::create_dir_all(library)?;
    let path = library.join(file_name(id));
    crate::store::write_atomic(&path, &serde_json::to_vec_pretty(&file)?)?;
    Ok(path)
}

pub fn read(path: &Path) -> Result<(LibraryFile, String), SystemError> {
    let bytes = std::fs::read(path)?;
    let file: LibraryFile = serde_json::from_slice(&bytes)?;
    if file.schema != LIBRARY_SCHEMA || !file.definitions.contains_key(&file.definition) {
        return Err(SystemError::Invalid(format!("{} is not a `{LIBRARY_SCHEMA}` file", path.display())));
    }
    Ok((file, blake3::hash(&bytes).to_hex().to_string()))
}

/// Definitions to add to a document to import a library file.
pub fn import(path: &Path) -> Result<BTreeMap<String, Definition>, SystemError> {
    let (file, hash) = read(path)?;
    let mut definitions = file.definitions;
    for d in definitions.values_mut() {
        d.source = Some(LibrarySource { path: path.display().to_string(), content_hash: hash.clone() });
    }
    Ok(definitions)
}

pub fn list(library: &Path, registry: &BehaviorRegistry) -> Result<Vec<LibraryEntry>, SystemError> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(library) else { return Ok(out) };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.to_string_lossy().ends_with(".definition.json")).collect();
    paths.sort();
    for path in paths {
        let (file, hash) = read(&path)?;
        let mut doc = SystemDocument::new("library");
        doc.definitions.extend(file.definitions.clone());
        let resolver = Resolver::new(&doc, registry);
        let d = &file.definitions[&file.definition];
        let mut ports = BTreeMap::new();
        for p in d.ports.keys() {
            let schema = resolver.boundary_schema(&file.definition, p, &mut BTreeSet::new()).ok().flatten();
            ports.insert(p.clone(), schema.as_ref().map(describe));
        }
        out.push(LibraryEntry {
            id: file.definition.clone(),
            label: d.label.clone(),
            description: d.description.clone(),
            interface: d.interface.clone(),
            path: path.display().to_string(),
            content_hash: hash,
            ports,
        });
    }
    Ok(out)
}

/// A replacement that satisfies an instance's connected ports.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Alternative {
    pub kind: InstanceKind,
    pub label: String,
    /// Library file to import first, when the definition is not in the document yet.
    pub library_path: Option<String>,
    pub same_interface: bool,
}

/// Implementations that can replace `at/name` without breaking its nets:
/// registry elements, document definitions and library definitions.
pub fn alternatives(document: &SystemDocument, registry: &BehaviorRegistry, library: Option<&Path>, at: &str, name: &str) -> Result<Vec<Alternative>, SystemError> {
    let resolver = Resolver::new(document, registry);
    let parent_id = resolver.definition_id_at(at)?;
    let parent = resolver.definition(&parent_id)?;
    let current = parent.instances.get(name).ok_or_else(|| SystemError::Invalid(format!("no instance `{name}` in `{parent_id}`")))?;
    let ports = resolver.instance_ports(current)?;
    let used: BTreeMap<String, Option<PortSchema>> = parent
        .nets
        .iter()
        .flat_map(|n| &n.terminals)
        .filter_map(|t| match t {
            Terminal::Port { instance, port } if instance == name => Some((port.clone(), ports.get(port).cloned().flatten())),
            _ => None,
        })
        .collect();
    let current_interface = match &current.kind {
        InstanceKind::Subsystem { definition } => resolver.definition(definition)?.interface.clone(),
        InstanceKind::Element { component_type } => Some(component_type.clone()),
    };
    let fits = |offered: &BTreeMap<String, Option<PortSchema>>| used.iter().all(|(p, s)| matches!((s, offered.get(p)), (Some(a), Some(Some(b))) if a == b));
    let mut out = Vec::new();
    for descriptor in registry.descriptors() {
        if descriptor.ports.iter().any(|p| p.name.contains('*')) || current.kind == (InstanceKind::Element { component_type: descriptor.type_id.0.clone() }) {
            continue;
        }
        let Ok(offered) = crate::resolve::element_ports(registry, &descriptor.type_id.0, &BTreeSet::new()) else { continue };
        let offered = offered.into_iter().map(|(k, v)| (k, Some(v))).collect();
        if fits(&offered) {
            out.push(Alternative {
                kind: InstanceKind::Element { component_type: descriptor.type_id.0.clone() },
                label: descriptor.display_name.to_string(),
                library_path: None,
                same_interface: current_interface.as_deref() == Some(descriptor.type_id.0.as_str()),
            });
        }
    }
    let mut add_definition = |id: &str, d: &Definition, doc: &SystemDocument, library_path: Option<String>| {
        if current.kind == (InstanceKind::Subsystem { definition: id.to_string() }) {
            return;
        }
        let r = Resolver::new(doc, registry);
        let offered: BTreeMap<String, Option<PortSchema>> = d.ports.keys().map(|p| (p.clone(), r.boundary_schema(id, p, &mut BTreeSet::new()).ok().flatten())).collect();
        if fits(&offered) {
            out.push(Alternative {
                kind: InstanceKind::Subsystem { definition: id.to_string() },
                label: d.label.clone(),
                library_path,
                same_interface: d.interface.is_some() && d.interface == current_interface,
            });
        }
    };
    // Definitions that would create a cycle (the parent, or anything placing it) are excluded.
    let forbidden: BTreeSet<String> = document
        .definitions
        .keys()
        .filter(|id| closure(document, id).map(|c| c.contains_key(&parent_id)).unwrap_or(true))
        .cloned()
        .collect();
    for (id, d) in &document.definitions {
        if !forbidden.contains(id) {
            add_definition(id, d, document, None);
        }
    }
    if let Some(library) = library {
        for entry in list(library, registry)? {
            if document.definitions.contains_key(&entry.id) {
                continue;
            }
            let (file, _) = read(Path::new(&entry.path))?;
            let mut doc = SystemDocument::new("library");
            doc.definitions.extend(file.definitions.clone());
            add_definition(&entry.id, &file.definitions[&entry.id], &doc, Some(entry.path.clone()));
        }
    }
    out.sort_by(|a, b| b.same_interface.cmp(&a.same_interface).then(a.label.cmp(&b.label)));
    Ok(out)
}
