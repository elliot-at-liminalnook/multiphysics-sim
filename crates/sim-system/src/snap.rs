//! Snapping: what can attach to a port, and the commands that attach it.
//!
//! For each port of a placed instance, [`suggestions`] lists every registry
//! element and saved/document definition with a port that may share a net
//! with it (same connector kind; signal output ↔ input of one quantity).
//! Curated companions from the component notes (`pairs_with`) rank first.
//! Candidates that would break the solver's structure, such as a second rigid
//! inertia on a shaft that already has one, are kept but carry a `conflict`
//! explaining what to put between them instead.
//!
//! [`snap`] turns one choice into ordinary shared commands (import, add,
//! connect) with a placement next to the source part, coaxial for shafts, so
//! the result is one undoable edit in every editor.
use crate::commands::describe;
use crate::document::*;
use crate::library;
use crate::resolve::{element_ports, element_top_ports, Resolver};
use crate::{Command, SystemError};
use serde::Serialize;
use sim_core::{BehaviorRegistry, PortSchema};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Candidate {
    pub kind: InstanceKind,
    pub label: String,
    /// One-line summary from the component notes or definition description.
    pub summary: String,
    /// The candidate's port that would join the net.
    pub port: String,
    /// Library file to import first, when the definition is not in the document.
    pub library_path: Option<String>,
    /// Listed as a typical companion by either side's notes.
    pub recommended: bool,
    /// Why attaching here would not compile as is, and what to do instead.
    pub conflict: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PortSuggestions {
    pub port: String,
    pub schema: String,
    /// Terminals already on this port's net (besides the port itself).
    pub connected_to: Vec<String>,
    pub candidates: Vec<Candidate>,
}

/// Whether a port of type `a` can share a net with a port of type `b`.
pub fn joinable(a: &PortSchema, b: &PortSchema) -> bool {
    match (a, b) {
        (PortSchema::Acausal(x), PortSchema::Acausal(y)) => x == y,
        (PortSchema::SignalOut(x), PortSchema::SignalIn(y)) | (PortSchema::SignalIn(x), PortSchema::SignalOut(y)) => x == y,
        _ => false,
    }
}

/// Acausal ports of an element on which it provides a lane (owns the node's
/// speed state, like an inertia or a mass). Found by instantiating the
/// equations with placeholder values; the values never reach a model.
pub fn providing_ports(registry: &BehaviorRegistry, component_type: &str) -> BTreeSet<String> {
    let Ok(descriptor) = registry.get(&component_type.into()) else { return BTreeSet::new() };
    let Some(equations) = descriptor.equations else { return BTreeSet::new() };
    let acausal: Vec<&str> = descriptor.ports.iter().filter(|p| matches!(p.schema, PortSchema::Acausal(_))).map(|p| p.name).collect();
    if acausal.iter().any(|p| p.contains('*')) {
        return BTreeSet::new();
    }
    let params: BTreeMap<String, f64> = descriptor
        .parameters
        .iter()
        .flatten()
        .filter(|p| !p.name.contains('*') && !p.name.starts_with("initial."))
        .map(|p| (p.name.clone(), p.default.unwrap_or(if p.integer { p.minimum.unwrap_or(1.) } else { 1.0 })))
        .collect();
    let Ok(behavior) = equations(&params) else { return BTreeSet::new() };
    behavior.provides().iter().filter_map(|p| acausal.get(p.port).map(|s| s.to_string())).collect()
}

fn notes_of(registry: &BehaviorRegistry, component_type: &str) -> Option<&'static sim_core::ComponentNotes> {
    registry.get(&component_type.into()).ok().and_then(|d| d.notes)
}

/// Suggestions for every port of `at/name`.
pub fn suggestions(document: &SystemDocument, registry: &BehaviorRegistry, library_dir: Option<&Path>, at: &str, name: &str) -> Result<Vec<PortSuggestions>, SystemError> {
    let resolver = Resolver::new(document, registry);
    let parent_id = resolver.definition_id_at(at)?;
    let parent = resolver.definition(&parent_id)?;
    let spec = parent.instances.get(name).ok_or_else(|| SystemError::Invalid(format!("no instance `{name}` in `{parent_id}`")))?;
    let ports = resolver.instance_ports(spec)?;
    let top: Vec<(&String, &Option<PortSchema>)> = ports.iter().filter(|(p, _)| !ports.keys().any(|q| q != *p && p.starts_with(&format!("{q}.")))).collect();
    let source_type = match &spec.kind {
        InstanceKind::Element { component_type } => Some(component_type.clone()),
        InstanceKind::Subsystem { .. } => None,
    };
    let source_notes = source_type.as_deref().and_then(|t| notes_of(registry, t));

    // Everything that could be placed: elements, document and library definitions.
    struct Offer {
        kind: InstanceKind,
        label: String,
        summary: String,
        ports: BTreeMap<String, PortSchema>,
        library_path: Option<String>,
        /// Element types a subsystem places (directly or nested).
        contains: BTreeSet<String>,
    }
    let mut offers = Vec::new();
    for descriptor in registry.descriptors() {
        if descriptor.ports.iter().any(|p| p.name.contains('*')) {
            continue;
        }
        let Ok(ports) = element_top_ports(registry, &descriptor.type_id.0, &BTreeSet::new()) else { continue };
        offers.push(Offer {
            kind: InstanceKind::Element { component_type: descriptor.type_id.0.clone() },
            label: descriptor.display_name.to_string(),
            summary: descriptor.notes.map(|n| n.summary.to_string()).unwrap_or_default(),
            ports,
            library_path: None,
            contains: BTreeSet::new(),
        });
    }
    let forbidden: BTreeSet<String> = document.definitions.keys().filter(|id| library::closure(document, id).map(|c| c.contains_key(&parent_id)).unwrap_or(true)).cloned().collect();
    let definition_offer = |id: &str, d: &Definition, doc: &SystemDocument, library_path: Option<String>| {
        let r = Resolver::new(doc, registry);
        let ports = d.ports.keys().filter_map(|p| r.boundary_schema(id, p, &mut BTreeSet::new()).ok().flatten().map(|s| (p.clone(), s))).collect();
        let contains = doc
            .definitions
            .iter()
            .filter(|(k, _)| library::closure(doc, id).map(|c| c.contains_key(*k)).unwrap_or(false))
            .flat_map(|(_, d)| d.instances.values())
            .filter_map(|i| match &i.kind {
                InstanceKind::Element { component_type } => Some(component_type.clone()),
                _ => None,
            })
            .collect();
        Offer { kind: InstanceKind::Subsystem { definition: id.to_string() }, label: d.label.clone(), summary: d.description.clone(), ports, library_path, contains }
    };
    for (id, d) in &document.definitions {
        if !forbidden.contains(id) && *id != document.root {
            offers.push(definition_offer(id, d, document, None));
        }
    }
    if let Some(dir) = library_dir {
        for entry in library::list(dir, registry)? {
            if document.definitions.contains_key(&entry.id) {
                continue;
            }
            let (file, _) = library::read(Path::new(&entry.path))?;
            let mut doc = SystemDocument::new("library");
            doc.definitions.extend(file.definitions.clone());
            offers.push(definition_offer(&entry.id, &file.definitions[&entry.id], &doc, Some(entry.path.clone())));
        }
    }

    let mut out = Vec::new();
    for (port, schema) in top {
        let Some(schema) = schema else { continue };
        let own = Terminal::port(name, port);
        let net = parent.nets.iter().find(|n| n.terminals.contains(&own));
        let connected_to: Vec<String> = net.map(|n| n.terminals.iter().filter(|t| **t != own).map(|t| t.to_string()).collect()).unwrap_or_default();
        // Does this node already have a lane provider (an inertia or mass)?
        let provider = net
            .map(|n| n.terminals.clone())
            .unwrap_or_else(|| vec![own.clone()])
            .iter()
            .find_map(|t| match t {
                Terminal::Port { instance, port } => match parent.instances.get(instance).map(|i| &i.kind) {
                    Some(InstanceKind::Element { component_type }) if providing_ports(registry, component_type).contains(port) => Some(instance.clone()),
                    _ => None,
                },
                Terminal::Boundary { .. } => None,
            });
        let mut candidates = Vec::new();
        for offer in &offers {
            let rank_type = match &offer.kind {
                InstanceKind::Element { component_type } => Some(component_type.as_str()),
                InstanceKind::Subsystem { .. } => None,
            };
            let mut fitting: Vec<&String> = offer.ports.iter().filter(|(_, s)| joinable(schema, s)).map(|(p, _)| p).collect();
            if fitting.is_empty() {
                continue;
            }
            // Prefer a port named like ours (shaft ↔ shaft, input for an output).
            fitting.sort_by_key(|p| (**p != *port, !matches!(p.as_str(), "shaft" | "input" | "worm" | "screw" | "p" | "axis" | "a")));
            let chosen = fitting[0].clone();
            let recommended = rank_type.is_some_and(|t| source_notes.is_some_and(|n| n.pairs_with.contains(&t)))
                || source_type.as_deref().is_some_and(|s| rank_type.and_then(|t| notes_of(registry, t)).is_some_and(|n| n.pairs_with.contains(&s)))
                || offer.contains.iter().any(|t| source_notes.is_some_and(|n| n.pairs_with.contains(&t.as_str())) && !t.ends_with("ground") && !t.contains("sensor"));
            let conflict = match (&provider, rank_type) {
                (Some(existing), Some(t)) if providing_ports(registry, t).contains(&chosen) => Some(format!(
                    "{existing} already sets this node's motion; a second rigid body on the same node is the same body. Add its inertia to {existing}, or join them through a coupling (rotational.spring / translational.spring)."
                )),
                _ => None,
            };
            candidates.push(Candidate { kind: offer.kind.clone(), label: offer.label.clone(), summary: offer.summary.clone(), port: chosen, library_path: offer.library_path.clone(), recommended, conflict });
        }
        let order = |c: &Candidate| -> (bool, bool, usize, String) {
            let t = match &c.kind {
                InstanceKind::Element { component_type } => component_type.clone(),
                InstanceKind::Subsystem { definition } => definition.clone(),
            };
            let curated = source_notes.and_then(|n| n.pairs_with.iter().position(|p| *p == t)).unwrap_or(usize::MAX);
            (c.conflict.is_some(), !c.recommended, curated, c.label.clone())
        };
        candidates.sort_by_key(order);
        out.push(PortSuggestions { port: port.clone(), schema: describe(schema), connected_to, candidates });
    }
    Ok(out)
}

/// Commands that place `candidate` at `at`, named `new_name`, next to
/// `from_instance` and join `candidate.port` to `from_instance.from_port`.
pub fn snap(document: &SystemDocument, registry: &BehaviorRegistry, at: &str, from_instance: &str, from_port: &str, candidate: &Candidate, new_name: &str) -> Result<Vec<Command>, SystemError> {
    let resolver = Resolver::new(document, registry);
    let parent_id = resolver.definition_id_at(at)?;
    let parent = resolver.definition(&parent_id)?;
    let source = parent.instances.get(from_instance).ok_or_else(|| SystemError::Invalid(format!("no instance `{from_instance}`")))?;
    let mut commands = Vec::new();
    if let Some(path) = &candidate.library_path {
        commands.push(Command::AddDefinitions { definitions: library::import(Path::new(path))? });
    }
    let schema = resolver.instance_ports(source)?.get(from_port).cloned().flatten();
    let mechanical = matches!(&schema, Some(PortSchema::Acausal(k)) if matches!(k.name(), n if n.contains("rotational") || n.contains("translational")));
    // Shafts continue along the source's axis (its local +Y); other kinds sit beside it.
    let step: [f32; 3] = if mechanical { crate::flatten::rotate(source.placement.rotation_xyzw, [0., 0.025, 0.]) } else { [0.025, 0., 0.] };
    let occupied: Vec<[f32; 3]> = parent.instances.values().map(|i| i.placement.position).collect();
    let free = (1..40)
        .map(|k| {
            let s = k as f32;
            [source.placement.position[0] + step[0] * s, source.placement.position[1] + step[1] * s, source.placement.position[2] + step[2] * s]
        })
        .find(|p| occupied.iter().all(|q| (0..3).map(|i| (p[i] - q[i]).powi(2)).sum::<f32>() > 0.012f32.powi(2)))
        .unwrap_or(source.placement.position);
    let mut spec = starter(registry, &candidate.kind, &candidate.label).at(free);
    if mechanical {
        spec.placement.rotation_xyzw = source.placement.rotation_xyzw;
    }
    // Defaults the element cannot run without are left for the user: the
    // inspector flags required parameters, nothing is silently invented.
    commands.push(Command::AddInstance { at: at.to_string(), name: new_name.to_string(), instance: spec });
    commands.push(Command::Connect { at: at.to_string(), terminals: vec![Terminal::port(from_instance, from_port), Terminal::port(new_name, &candidate.port)], label: String::new() });
    Ok(commands)
}

/// A new instance of `kind` with the notes' typical values for its
/// parameters, each recorded as an estimate so it is never mistaken for a
/// measured property of the user's part.
pub fn starter(registry: &BehaviorRegistry, kind: &InstanceKind, label: &str) -> InstanceSpec {
    let mut spec = InstanceSpec::new(kind.clone()).labeled(label);
    if let InstanceKind::Element { component_type } = kind {
        if let Some(notes) = notes_of(registry, component_type) {
            for (parameter, value) in notes.typical {
                spec.parameters.insert(
                    parameter.to_string(),
                    ParameterBinding::Value {
                        value: *value,
                        unit: None,
                        provenance: Some(sim_inspect::Provenance::Estimated { explanation: format!("typical {parameter} from the {component_type} notes; replace with your part's value") }),
                        uncertainty: None,
                    },
                );
            }
        }
    }
    spec
}

/// The ports of a registry element with their types, for library cards.
pub fn element_port_types(registry: &BehaviorRegistry, component_type: &str) -> BTreeMap<String, String> {
    element_ports(registry, component_type, &BTreeSet::new()).map(|p| p.iter().map(|(k, v)| (k.clone(), describe(v))).collect()).unwrap_or_default()
}
