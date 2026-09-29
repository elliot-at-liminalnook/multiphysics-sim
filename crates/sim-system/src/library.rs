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
    pub icon: String,
    pub component_type: String,
    pub display_name: String,
    pub domain: String,
    pub ports: BTreeMap<String, String>,
    pub parameters: Vec<ParameterEntry>,
    /// Learning notes from the registry, when the component has them.
    pub notes: Option<NotesEntry>,
}

/// Serializable view of [`sim_core::ComponentNotes`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NotesEntry {
    pub summary: String,
    pub category: String,
    pub explanation: String,
    pub equations: Vec<String>,
    pub tradeoffs: String,
    pub limits: String,
    pub pairs_with: Vec<String>,
    /// Derived values at the default/typical parameters.
    pub derived: Vec<sim_core::DerivedValue>,
}

impl NotesEntry {
    pub fn from_notes(notes: &sim_core::ComponentNotes, parameters: &BTreeMap<String, f64>) -> Self {
        Self {
            summary: notes.summary.into(),
            category: notes.category.into(),
            explanation: notes.explanation.into(),
            equations: notes.equations.iter().map(|e| e.to_string()).collect(),
            tradeoffs: notes.tradeoffs.into(),
            limits: notes.limits.into(),
            pairs_with: notes.pairs_with.iter().map(|e| e.to_string()).collect(),
            derived: notes.derive(parameters),
        }
    }
}

/// Parameter values of an element: its explicit values, else the notes'
/// typical values, else declared defaults. For derived-value displays.
pub fn effective_parameters(registry: &BehaviorRegistry, component_type: &str, explicit: &BTreeMap<String, f64>) -> BTreeMap<String, f64> {
    let mut out = BTreeMap::new();
    if let Ok(d) = registry.get(&component_type.into()) {
        for p in d.parameters.iter().flatten() {
            if let Some(v) = p.default {
                out.insert(p.name.clone(), v);
            }
        }
        for (k, v) in d.notes.map(|n| n.typical).unwrap_or_default() {
            out.insert(k.to_string(), *v);
        }
    }
    out.extend(explicit.iter().map(|(k, v)| (k.clone(), *v)));
    out
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParameterEntry {
    pub name: String,
    /// Help text from the component notes.
    pub help: String,
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
            icon: sim_core::icons::resolve(descriptor.notes.map(|n|n.icon).unwrap_or(""), &descriptor.type_id.0).into(),
            component_type: descriptor.type_id.0.clone(),
            display_name: descriptor.display_name.to_string(),
            // A noted category (Actuators, Transmissions…) wins over the type prefix.
            domain: descriptor.notes.map(|n| n.category).filter(|c| !c.is_empty()).map(str::to_string).unwrap_or_else(|| descriptor.type_id.0.split('.').next().unwrap_or("").to_string()),
            ports: ports.iter().map(|(k, v)| (k.clone(), describe(v))).collect(),
            parameters: descriptor
                .parameters
                .iter()
                .flatten()
                .filter(|p| !p.implementation_reference)
                .map(|p| ParameterEntry { name: p.name.clone(), help: descriptor.notes.and_then(|n| n.parameter_help(&p.name)).unwrap_or_default().to_string(), unit: p.unit.clone(), required: p.required, default: p.default, minimum: p.minimum, maximum: p.maximum })
                .collect(),
            notes: descriptor.notes.map(|n| NotesEntry::from_notes(n, &effective_parameters(registry, &descriptor.type_id.0, &BTreeMap::new()))),
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

/// Result of publishing a definition to the library.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Published {
    pub id: String,
    pub path: String,
    pub version: u32,
    pub content_hash: String,
    /// False when the library already held exactly these contents.
    pub changed: bool,
}

fn comparable(mut d: Definition) -> Definition {
    d.source = None;
    d.version = None;
    d.references.clear();
    d
}

/// Save `id` (and what it places) to the library as a new version when its
/// contents differ from the published file; otherwise leave the file alone.
/// Nested definitions are published too, each with its own version.
pub fn publish(document: &SystemDocument, id: &str, library: &Path) -> Result<Vec<Published>, SystemError> {
    let definitions = closure(document, id)?;
    let mut out = Vec::new();
    // Publish nested definitions first so their files exist before parents.
    // Children before parents, so a parent records its children's versions.
    let mut order: Vec<String> = Vec::new();
    fn visit(document: &SystemDocument, id: &str, order: &mut Vec<String>) {
        if order.iter().any(|o| o == id) {
            return;
        }
        if let Some(d) = document.definitions.get(id) {
            for i in d.instances.values() {
                if let InstanceKind::Subsystem { definition } = &i.kind {
                    visit(document, definition, order);
                }
            }
        }
        order.push(id.to_string());
    }
    visit(document, id, &mut order);
    let _ = &definitions;
    for def_id in &order {
        let mut sub = SystemDocument::new("publish");
        sub.definitions = document.definitions.clone();
        let existing = read(&library.join(file_name(def_id))).ok();
        let previous = existing.as_ref().and_then(|(f, _)| f.definitions.get(def_id.as_str()).cloned());
        let unchanged = previous.as_ref().is_some_and(|_| {
            let mine: BTreeMap<String, Definition> = closure(&sub, def_id).unwrap_or_default().into_iter().map(|(k, v)| (k, comparable(v))).collect();
            let theirs: BTreeMap<String, Definition> = existing.as_ref().unwrap().0.definitions.clone().into_iter().map(|(k, v)| (k, comparable(v))).collect();
            mine == theirs
        });
        if unchanged {
            let (file, hash) = existing.unwrap();
            out.push(Published { id: def_id.clone(), path: library.join(file_name(def_id)).display().to_string(), version: file.definitions[def_id.as_str()].version.unwrap_or(1), content_hash: hash, changed: false });
            continue;
        }
        let version = previous.and_then(|p| p.version).unwrap_or(0) + 1;
        let mut defs = closure(&sub, def_id)?;
        for (k, d) in defs.iter_mut() {
            d.source = None;
            d.references.clear();
            if k == def_id {
                d.version = Some(version);
            } else if let Some(p) = out.iter().find(|p| &p.id == k) {
                d.version = Some(p.version);
            }
        }
        let file = LibraryFile { schema: LIBRARY_SCHEMA.into(), definition: def_id.clone(), definitions: defs };
        std::fs::create_dir_all(library)?;
        let path = library.join(file_name(def_id));
        let bytes = serde_json::to_vec_pretty(&file)?;
        crate::store::write_atomic(&path, &bytes)?;
        out.push(Published { id: def_id.clone(), path: path.display().to_string(), version, content_hash: blake3::hash(&bytes).to_hex().to_string(), changed: true });
    }
    // Library files that bundle a changed definition (a gearmotor file carries
    // its motor) get the new copy and a new version too, so documents that
    // imported them see the change.
    let changed: BTreeMap<String, Definition> = out
        .iter()
        .filter(|p| p.changed)
        .filter_map(|p| read(Path::new(&p.path)).ok().and_then(|(f, _)| f.definitions.get(&p.id).cloned().map(|d| (p.id.clone(), d))))
        .collect();
    if !changed.is_empty() {
        let mut dependents = Vec::new();
        if let Ok(entries) = std::fs::read_dir(library) {
            for e in entries.flatten() {
                let path = e.path();
                if !path.to_string_lossy().ends_with(".definition.json") || out.iter().any(|p| Path::new(&p.path) == path) {
                    continue;
                }
                let Ok((mut file, _)) = read(&path) else { continue };
                let mut touched = false;
                for (id, d) in &changed {
                    if let Some(bundled) = file.definitions.get_mut(id) {
                        if comparable(bundled.clone()) != comparable(d.clone()) || bundled.version != d.version {
                            *bundled = d.clone();
                            touched = true;
                        }
                    }
                }
                if touched {
                    let top = file.definition.clone();
                    let version = file.definitions[&top].version.unwrap_or(0) + 1;
                    file.definitions.get_mut(&top).unwrap().version = Some(version);
                    let bytes = serde_json::to_vec_pretty(&file)?;
                    crate::store::write_atomic(&path, &bytes)?;
                    dependents.push(Published { id: top, path: path.display().to_string(), version, content_hash: blake3::hash(&bytes).to_hex().to_string(), changed: true });
                }
            }
        }
        out.extend(dependents);
    }
    Ok(out)
}

/// A definition imported from a library file that has since changed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Stale {
    pub id: String,
    pub path: String,
    pub imported_hash: String,
    pub current_hash: String,
    pub imported_version: Option<u32>,
    pub current_version: Option<u32>,
}

/// A recorded library path: absolute as is, relative to `base` otherwise
/// (the directory the tools run from, normally the repository root).
pub fn source_path(path: &str, base: &Path) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() { p.to_path_buf() } else { base.join(p) }
}

/// Imported definitions whose library file changed (or vanished: skipped).
pub fn stale(document: &SystemDocument, base: &Path) -> Vec<Stale> {
    let mut out = Vec::new();
    for (id, d) in &document.definitions {
        let Some(source) = &d.source else { continue };
        let Ok((file, hash)) = read(&source_path(&source.path, base)) else { continue };
        if hash != source.content_hash {
            out.push(Stale { id: id.clone(), path: source.path.clone(), imported_hash: source.content_hash.clone(), current_hash: hash, imported_version: d.version, current_version: file.definitions.get(id).and_then(|x| x.version) });
        }
    }
    out
}

/// Commands that bring every stale import up to its library file: update
/// the definitions the document has, add the ones it lacks.
pub fn sync(document: &SystemDocument, base: &Path) -> Result<Vec<crate::Command>, SystemError> {
    let paths: BTreeSet<String> = stale(document, base).into_iter().map(|s| s.path).collect();
    let mut update = BTreeMap::new();
    let mut add = BTreeMap::new();
    for path in paths {
        for (id, mut d) in import(&source_path(&path, base))? {
            // Keep the path as the document recorded it (relative stays relative).
            if let Some(s) = &mut d.source {
                s.path = path.clone();
            }
            // A definition imported from another file stays owned by that file.
            let owned_elsewhere = document.definitions.get(&id).and_then(|x| x.source.as_ref()).is_some_and(|s| s.path != path);
            if owned_elsewhere && !update.contains_key(&id) {
                continue;
            }
            if document.definitions.contains_key(&id) { update.insert(id, d) } else { add.insert(id, d) };
        }
    }
    let mut commands = Vec::new();
    if !add.is_empty() {
        commands.push(crate::Command::AddDefinitions { definitions: add });
    }
    if !update.is_empty() {
        commands.push(crate::Command::UpdateDefinitions { definitions: update });
    }
    Ok(commands)
}

/// Where a definition is placed, across system files: (file, placements).
pub fn where_used(files: &[PathBuf], id: &str) -> Vec<(String, usize)> {
    files
        .iter()
        .filter_map(|f| {
            let doc: SystemDocument = serde_json::from_slice(&std::fs::read(f).ok()?).ok()?;
            let n = crate::commands::placements_of(&doc, id).len();
            (n > 0).then(|| (f.display().to_string(), n))
        })
        .collect()
}

/// System files under `dir`, recursively (for where-used).
pub fn system_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                if !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.') || n == "target") {
                    stack.push(p);
                }
            } else if p.to_string_lossy().ends_with(".system.json") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// Commands that set an instance's parameters from a CAD derivation
/// (`sim.cad-physics/1`), each recorded as derived (rule + hash of its
/// inputs and the CAD file), declared or estimated as the file says.
pub fn cad_physics_commands(at: &str, instance: &str, record: &serde_json::Value) -> Result<Vec<crate::Command>, SystemError> {
    if record["schema"] != "sim.cad-physics/1" {
        return Err(SystemError::Invalid("not a sim.cad-physics/1 record".into()));
    }
    let cad_hash = record["cad"]["sha256"].as_str().unwrap_or("").to_string();
    let params = record["parameters"].as_object().ok_or_else(|| SystemError::Invalid("record has no parameters".into()))?;
    let mut out = Vec::new();
    for (name, p) in params {
        let value = p["value"].as_f64().ok_or_else(|| SystemError::Invalid(format!("{name}: no value")))?;
        let rule = p["provenance"]["rule"].as_str().unwrap_or("").to_string();
        let provenance = match p["provenance"]["kind"].as_str() {
            Some("derived") => sim_inspect::Provenance::Derived { rule: format!("{rule} (CAD {})", record["cad"]["path"].as_str().unwrap_or("?")), inputs_hash: blake3::hash(format!("{cad_hash}:{}", p["provenance"]["inputs"]).as_bytes()).to_hex().to_string() },
            _ => sim_inspect::Provenance::Estimated { explanation: format!("{rule} (from CAD {})", record["cad"]["path"].as_str().unwrap_or("?")) },
        };
        out.push(crate::Command::SetParameter { at: at.to_string(), name: instance.to_string(), parameter: name.clone(), binding: Some(ParameterBinding::Value { value, unit: None, provenance: Some(provenance), uncertainty: None }) });
    }
    Ok(out)
}
