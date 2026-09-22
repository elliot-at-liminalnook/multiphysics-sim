//! One command set for every editor: both viewers, REST and the CLI apply the
//! same commands with the same validation. `at` is always an instance path;
//! commands edit the definition placed there (see [`Resolver::definition_id_at`]).
use crate::document::*;
use crate::flatten::{mul, rotate};
use crate::resolve::Resolver;
use crate::SystemError;
use serde::{Deserialize, Serialize};
use sim_core::{BehaviorRegistry, PortSchema};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    SetTitle { title: String },
    /// Record (or clear) the numerical settings this system runs with.
    SetRunSettings { run: Option<RunSettings> },
    AddInstance {
        #[serde(default)]
        at: String,
        name: String,
        instance: InstanceSpec,
    },
    RemoveInstance {
        #[serde(default)]
        at: String,
        name: String,
    },
    RenameInstance {
        #[serde(default)]
        at: String,
        name: String,
        new_name: String,
    },
    SetLabel {
        #[serde(default)]
        at: String,
        name: String,
        label: String,
    },
    MoveInstance {
        #[serde(default)]
        at: String,
        name: String,
        placement: Placement,
    },
    SetParameter {
        #[serde(default)]
        at: String,
        name: String,
        parameter: String,
        /// `null` clears the binding (the default applies again).
        binding: Option<ParameterBinding>,
    },
    SetAppearance {
        #[serde(default)]
        at: String,
        name: String,
        appearance: Option<Appearance>,
    },
    /// Join terminals. Existing nets that share a terminal are merged.
    Connect {
        #[serde(default)]
        at: String,
        terminals: Vec<Terminal>,
        #[serde(default)]
        label: String,
    },
    /// Remove one terminal from its net.
    Disconnect {
        #[serde(default)]
        at: String,
        terminal: Terminal,
    },
    AddBoundaryPort {
        #[serde(default)]
        at: String,
        name: String,
        #[serde(default)]
        port: BoundaryPort,
        /// Optionally join it to an inner terminal in the same step.
        #[serde(default)]
        connect: Option<Terminal>,
    },
    RemoveBoundaryPort {
        #[serde(default)]
        at: String,
        name: String,
    },
    DeclareParameter {
        #[serde(default)]
        at: String,
        name: String,
        declaration: ParameterDecl,
    },
    RemoveParameter {
        #[serde(default)]
        at: String,
        name: String,
    },
    /// Move selected instances into a new definition placed as one instance.
    /// Nets that cross the selection become boundary ports.
    Group {
        #[serde(default)]
        at: String,
        instances: Vec<String>,
        name: String,
        definition: String,
        #[serde(default)]
        label: String,
    },
    /// Inline a subsystem instance's contents into its parent.
    Ungroup {
        #[serde(default)]
        at: String,
        name: String,
    },
    /// Replace an instance's implementation, keeping its connections.
    /// The replacement must provide every connected port with the same type.
    Swap {
        #[serde(default)]
        at: String,
        name: String,
        kind: InstanceKind,
        /// Keep parameter bindings the replacement also declares.
        #[serde(default = "default_true")]
        keep_parameters: bool,
    },
    /// Give one instance its own copy of a shared definition.
    MakeUnique {
        #[serde(default)]
        at: String,
        name: String,
        definition: String,
    },
    /// Add definitions (for example from a library file).
    AddDefinitions { definitions: BTreeMap<String, Definition> },
    RemoveDefinition { id: String },
    SetDefinitionInfo {
        id: String,
        #[serde(default)]
        label: Option<String>,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        interface: Option<Option<String>>,
    },
    RegisterAsset { id: String, asset: Asset },
    SetReference {
        #[serde(default)]
        at: String,
        id: String,
        reference: ReferenceImage,
    },
    /// Scale about the first picked point so the two points are `distance` apart.
    CalibrateReference {
        #[serde(default)]
        at: String,
        id: String,
        first: [f32; 3],
        second: [f32; 3],
        distance: f32,
    },
    RemoveReference {
        #[serde(default)]
        at: String,
        id: String,
    },
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Outcome {
    /// Definition that changed, and how many placements share it.
    pub definition: Option<String>,
    pub shared_by: usize,
    pub message: String,
}

/// Apply commands in order. All succeed and the result validates, or the
/// document is left unchanged.
pub fn apply(document: &mut SystemDocument, registry: &BehaviorRegistry, commands: &[Command]) -> Result<Vec<Outcome>, SystemError> {
    let mut draft = document.clone();
    let mut outcomes = Vec::new();
    for (index, command) in commands.iter().enumerate() {
        let outcome = apply_one(&mut draft, registry, command).map_err(|e| SystemError::Command { index, message: e.to_string() })?;
        outcomes.push(outcome);
    }
    Resolver::new(&draft, registry).validate().map_err(|e| SystemError::Command { index: commands.len().saturating_sub(1), message: e.to_string() })?;
    draft.revision = document.revision + 1;
    *document = draft;
    Ok(outcomes)
}

fn definition_mut<'a>(document: &'a mut SystemDocument, id: &str) -> Result<&'a mut Definition, SystemError> {
    document.definitions.get_mut(id).ok_or_else(|| SystemError::Invalid(format!("unknown definition `{id}`")))
}

fn name_ok(name: &str) -> Result<(), SystemError> {
    if valid_name(name) { Ok(()) } else { Err(SystemError::Invalid(format!("`{name}` is not a valid name (letters, digits, `_`, `-`)"))) }
}

fn outcome(document: &SystemDocument, registry: &BehaviorRegistry, definition: &str, message: String) -> Outcome {
    Outcome { definition: Some(definition.into()), shared_by: Resolver::new(document, registry).placements(definition), message }
}

fn apply_one(document: &mut SystemDocument, registry: &BehaviorRegistry, command: &Command) -> Result<Outcome, SystemError> {
    let at_definition = |document: &SystemDocument, at: &str| Resolver::new(document, registry).definition_id_at(at);
    match command {
        Command::SetTitle { title } => {
            document.title = title.clone();
            Ok(Outcome { definition: None, shared_by: 0, message: "Renamed system".into() })
        }
        Command::SetRunSettings { run } => {
            if let Some(r) = run {
                let positive = |v: Option<f64>| v.is_none_or(|v| v.is_finite() && v > 0.);
                if !(r.interval.is_finite() && r.interval > 0.) || !positive(r.absolute_tolerance) || !positive(r.relative_tolerance) || r.max_iterations == Some(0) {
                    return Err(SystemError::Invalid("run settings need a positive step, positive tolerances and at least one iteration".into()));
                }
            }
            document.run = run.clone();
            Ok(Outcome { definition: None, shared_by: 0, message: "Updated run settings".into() })
        }
        Command::AddInstance { at, name, instance } => {
            name_ok(name)?;
            let id = at_definition(document, at)?;
            let d = definition_mut(document, &id)?;
            if d.instances.contains_key(name) {
                return Err(SystemError::Invalid(format!("`{id}` already has an instance `{name}`")));
            }
            d.instances.insert(name.clone(), instance.clone());
            Ok(outcome(document, registry, &id, format!("Added {name}")))
        }
        Command::RemoveInstance { at, name } => {
            let id = at_definition(document, at)?;
            let d = definition_mut(document, &id)?;
            d.instances.remove(name).ok_or_else(|| SystemError::Invalid(format!("no instance `{name}` in `{id}`")))?;
            for net in &mut d.nets {
                net.terminals.retain(|t| t.instance() != Some(name));
            }
            d.nets.retain(|n| n.terminals.len() >= 2);
            Ok(outcome(document, registry, &id, format!("Removed {name}")))
        }
        Command::RenameInstance { at, name, new_name } => {
            name_ok(new_name)?;
            let id = at_definition(document, at)?;
            let d = definition_mut(document, &id)?;
            if d.instances.contains_key(new_name) {
                return Err(SystemError::Invalid(format!("`{id}` already has an instance `{new_name}`")));
            }
            let instance = d.instances.remove(name).ok_or_else(|| SystemError::Invalid(format!("no instance `{name}` in `{id}`")))?;
            d.instances.insert(new_name.clone(), instance);
            for t in d.nets.iter_mut().flat_map(|n| n.terminals.iter_mut()) {
                if let Terminal::Port { instance, .. } = t {
                    if instance == name {
                        *instance = new_name.clone();
                    }
                }
            }
            Ok(outcome(document, registry, &id, format!("Renamed {name} to {new_name}")))
        }
        Command::SetLabel { at, name, label } => {
            let id = at_definition(document, at)?;
            instance_mut(document, &id, name)?.label = label.clone();
            Ok(outcome(document, registry, &id, format!("Labeled {name}")))
        }
        Command::MoveInstance { at, name, placement } => {
            let id = at_definition(document, at)?;
            instance_mut(document, &id, name)?.placement = placement.clone();
            Ok(outcome(document, registry, &id, format!("Moved {name}")))
        }
        Command::SetParameter { at, name, parameter, binding } => {
            let id = at_definition(document, at)?;
            let instance = instance_mut(document, &id, name)?;
            match binding {
                Some(b) => {
                    instance.parameters.insert(parameter.clone(), b.clone());
                }
                None => {
                    instance.parameters.remove(parameter);
                }
            }
            Ok(outcome(document, registry, &id, format!("Set {name}.{parameter}")))
        }
        Command::SetAppearance { at, name, appearance } => {
            let id = at_definition(document, at)?;
            instance_mut(document, &id, name)?.appearance = appearance.clone();
            Ok(outcome(document, registry, &id, format!("Restyled {name}")))
        }
        Command::Connect { at, terminals, label } => {
            if terminals.len() < 2 {
                return Err(SystemError::Invalid("connect needs at least two terminals".into()));
            }
            let id = at_definition(document, at)?;
            let d = definition_mut(document, &id)?;
            let mut merged = Net { label: label.clone(), terminals: Vec::new() };
            let mut kept = Vec::new();
            for net in d.nets.drain(..) {
                if net.terminals.iter().any(|t| terminals.contains(t)) {
                    if merged.label.is_empty() {
                        merged.label = net.label.clone();
                    }
                    merged.terminals.extend(net.terminals);
                } else {
                    kept.push(net);
                }
            }
            for t in terminals {
                if !merged.terminals.contains(t) {
                    merged.terminals.push(t.clone());
                }
            }
            kept.push(merged.clone());
            d.nets = kept;
            Resolver::new(document, registry).check_net(&id, &merged)?;
            Ok(outcome(document, registry, &id, format!("Connected {}", merged.terminals.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(", "))))
        }
        Command::Disconnect { at, terminal } => {
            let id = at_definition(document, at)?;
            let d = definition_mut(document, &id)?;
            let before = d.nets.iter().map(|n| n.terminals.len()).sum::<usize>();
            for net in &mut d.nets {
                net.terminals.retain(|t| t != terminal);
            }
            d.nets.retain(|n| n.terminals.len() >= 2);
            if before == d.nets.iter().map(|n| n.terminals.len()).sum::<usize>() {
                return Err(SystemError::Invalid(format!("{terminal} is not connected")));
            }
            Ok(outcome(document, registry, &id, format!("Disconnected {terminal}")))
        }
        Command::AddBoundaryPort { at, name, port, connect } => {
            name_ok(name)?;
            let id = at_definition(document, at)?;
            let d = definition_mut(document, &id)?;
            if d.ports.contains_key(name) {
                return Err(SystemError::Invalid(format!("`{id}` already has a boundary port `{name}`")));
            }
            d.ports.insert(name.clone(), port.clone());
            if let Some(t) = connect {
                apply_one(document, registry, &Command::Connect { at: at.clone(), terminals: vec![Terminal::boundary(name), t.clone()], label: String::new() })?;
            }
            Ok(outcome(document, registry, &id, format!("Added boundary port {name}")))
        }
        Command::RemoveBoundaryPort { at, name } => {
            let id = at_definition(document, at)?;
            let d = definition_mut(document, &id)?;
            d.ports.remove(name).ok_or_else(|| SystemError::Invalid(format!("`{id}` has no boundary port `{name}`")))?;
            let own = Terminal::boundary(name);
            for net in &mut d.nets {
                net.terminals.retain(|t| t != &own);
            }
            d.nets.retain(|n| n.terminals.len() >= 2);
            // Placements of this definition lose their connections to it.
            let parents: Vec<(String, String)> = placements_of(document, &id);
            for (parent, instance) in parents {
                let p = definition_mut(document, &parent)?;
                for net in &mut p.nets {
                    net.terminals.retain(|t| t != &Terminal::port(&instance, name));
                }
                p.nets.retain(|n| n.terminals.len() >= 2);
            }
            Ok(outcome(document, registry, &id, format!("Removed boundary port {name}")))
        }
        Command::DeclareParameter { at, name, declaration } => {
            name_ok(name)?;
            let id = at_definition(document, at)?;
            definition_mut(document, &id)?.parameters.insert(name.clone(), declaration.clone());
            Ok(outcome(document, registry, &id, format!("Declared parameter {name}")))
        }
        Command::RemoveParameter { at, name } => {
            let id = at_definition(document, at)?;
            let d = definition_mut(document, &id)?;
            d.parameters.remove(name).ok_or_else(|| SystemError::Invalid(format!("`{id}` has no parameter `{name}`")))?;
            Ok(outcome(document, registry, &id, format!("Removed parameter {name}")))
        }
        Command::Group { at, instances, name, definition, label } => group(document, registry, at, instances, name, definition, label),
        Command::Ungroup { at, name } => ungroup(document, registry, at, name),
        Command::Swap { at, name, kind, keep_parameters } => swap(document, registry, at, name, kind, *keep_parameters),
        Command::MakeUnique { at, name, definition } => {
            if !valid_definition_id(definition) || document.definitions.contains_key(definition) {
                return Err(SystemError::Invalid(format!("`{definition}` is not a new, valid definition id")));
            }
            let id = at_definition(document, at)?;
            let InstanceKind::Subsystem { definition: shared } = instance_mut(document, &id, name)?.kind.clone() else {
                return Err(SystemError::Invalid(format!("`{name}` is an element; only subsystems share definitions")));
            };
            let mut copy = document.definitions[&shared].clone();
            copy.label = format!("{} (unique)", copy.label);
            copy.source = None;
            document.definitions.insert(definition.clone(), copy);
            instance_mut(document, &id, name)?.kind = InstanceKind::Subsystem { definition: definition.clone() };
            Ok(outcome(document, registry, definition, format!("{name} now uses its own copy `{definition}`")))
        }
        Command::AddDefinitions { definitions } => {
            for (id, d) in definitions {
                if !valid_definition_id(id) {
                    return Err(SystemError::Invalid(format!("invalid definition id `{id}`")));
                }
                match document.definitions.get(id) {
                    Some(existing) if existing == d => {}
                    Some(_) => return Err(SystemError::Invalid(format!("definition `{id}` already exists with different contents; make it unique or remove it first"))),
                    None => {
                        document.definitions.insert(id.clone(), d.clone());
                    }
                }
            }
            Ok(Outcome { definition: None, shared_by: 0, message: format!("Added {} definition(s)", definitions.len()) })
        }
        Command::RemoveDefinition { id } => {
            if id == &document.root || !placements_of(document, id).is_empty() {
                return Err(SystemError::Invalid(format!("`{id}` is the root or still placed")));
            }
            document.definitions.remove(id).ok_or_else(|| SystemError::Invalid(format!("unknown definition `{id}`")))?;
            Ok(Outcome { definition: None, shared_by: 0, message: format!("Removed definition {id}") })
        }
        Command::SetDefinitionInfo { id, label, description, interface } => {
            let d = definition_mut(document, id)?;
            if let Some(l) = label {
                d.label = l.clone();
            }
            if let Some(t) = description {
                d.description = t.clone();
            }
            if let Some(i) = interface {
                d.interface = i.clone();
            }
            Ok(outcome(document, registry, id, format!("Updated {id}")))
        }
        Command::RegisterAsset { id, asset } => {
            if let Some(existing) = document.assets.get(id) {
                if existing.path != asset.path || existing.bytes != asset.bytes {
                    return Err(SystemError::Invalid(format!("asset `{id}` already registered with different contents")));
                }
            }
            document.assets.insert(id.clone(), asset.clone());
            Ok(Outcome { definition: None, shared_by: 0, message: format!("Registered asset {id}") })
        }
        Command::SetReference { at, id, reference } => {
            name_ok(id)?;
            let def = at_definition(document, at)?;
            definition_mut(document, &def)?.references.insert(id.clone(), reference.clone());
            Ok(outcome(document, registry, &def, format!("Placed reference {id}")))
        }
        Command::CalibrateReference { at, id, first, second, distance } => {
            let def = at_definition(document, at)?;
            let reference = definition_mut(document, &def)?.references.get_mut(id).ok_or_else(|| SystemError::Invalid(format!("no reference `{id}`")))?;
            let current = ((first[0] - second[0]).powi(2) + (first[1] - second[1]).powi(2) + (first[2] - second[2]).powi(2)).sqrt();
            if !(current > 1e-9 && distance.is_finite() && *distance > 0.) {
                return Err(SystemError::Invalid("pick two distinct points and give a positive distance".into()));
            }
            let factor = distance / current;
            reference.width *= factor;
            // Keep the first picked point fixed on screen.
            for k in 0..3 {
                reference.origin[k] = first[k] + factor * (reference.origin[k] - first[k]);
            }
            Ok(outcome(document, registry, &def, format!("Calibrated {id} by ×{factor:.4}")))
        }
        Command::RemoveReference { at, id } => {
            let def = at_definition(document, at)?;
            definition_mut(document, &def)?.references.remove(id).ok_or_else(|| SystemError::Invalid(format!("no reference `{id}`")))?;
            Ok(outcome(document, registry, &def, format!("Removed reference {id}")))
        }
    }
}

fn instance_mut<'a>(document: &'a mut SystemDocument, definition: &str, name: &str) -> Result<&'a mut InstanceSpec, SystemError> {
    definition_mut(document, definition)?.instances.get_mut(name).ok_or_else(|| SystemError::Invalid(format!("no instance `{name}` in `{definition}`")))
}

/// (parent definition, instance name) for every instance placing `id`.
pub fn placements_of(document: &SystemDocument, id: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (parent, d) in &document.definitions {
        for (name, i) in &d.instances {
            if matches!(&i.kind, InstanceKind::Subsystem { definition } if definition == id) {
                out.push((parent.clone(), name.clone()));
            }
        }
    }
    out
}

fn unique_name(taken: &BTreeSet<String>, wanted: &str) -> String {
    let base: String = wanted.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
    let base = if base.is_empty() { "port".to_string() } else { base };
    if !taken.contains(&base) {
        return base;
    }
    (2..).map(|n| format!("{base}_{n}")).find(|c| !taken.contains(c)).unwrap()
}

fn group(document: &mut SystemDocument, registry: &BehaviorRegistry, at: &str, selected: &[String], name: &str, definition: &str, label: &str) -> Result<Outcome, SystemError> {
    name_ok(name)?;
    if !valid_definition_id(definition) || document.definitions.contains_key(definition) {
        return Err(SystemError::Invalid(format!("`{definition}` is not a new, valid definition id")));
    }
    if selected.is_empty() {
        return Err(SystemError::Invalid("select at least one instance to group".into()));
    }
    let parent_id = Resolver::new(document, registry).definition_id_at(at)?;
    let parent = document.definitions[&parent_id].clone();
    let chosen: BTreeSet<&String> = selected.iter().collect();
    for s in &chosen {
        if !parent.instances.contains_key(*s) {
            return Err(SystemError::Invalid(format!("no instance `{s}` in `{parent_id}`")));
        }
    }
    if parent.instances.contains_key(name) && !chosen.contains(&name.to_string()) {
        return Err(SystemError::Invalid(format!("`{parent_id}` already has an instance `{name}`")));
    }
    let inside = |t: &Terminal| t.instance().is_some_and(|i| chosen.contains(&i.to_string()));
    // New definition centered on the selection.
    let n = chosen.len() as f32;
    let mut center = [0f32; 3];
    for s in &chosen {
        for (k, c) in center.iter_mut().enumerate() {
            *c += parent.instances[*s].placement.position[k] / n;
        }
    }
    let mut group = Definition::new(if label.is_empty() { name } else { label });
    for s in &chosen {
        let mut instance = parent.instances[*s].clone();
        for k in 0..3 {
            instance.placement.position[k] -= center[k];
        }
        // Parameters inherited from the parent become parameters of the group.
        for binding in instance.parameters.values() {
            if let ParameterBinding::Parameter { parameter } = binding {
                if let Some(decl) = parent.parameters.get(parameter) {
                    group.parameters.insert(parameter.clone(), decl.clone());
                }
            }
        }
        group.instances.insert((*s).clone(), instance);
    }
    let mut outer_nets = Vec::new();
    let mut taken = BTreeSet::new();
    for net in &parent.nets {
        let (ins, outs): (Vec<Terminal>, Vec<Terminal>) = net.terminals.iter().cloned().partition(|t| inside(t));
        if outs.is_empty() {
            group.nets.push(net.clone());
        } else if ins.is_empty() {
            outer_nets.push(net.clone());
        } else {
            let wanted = if !net.label.is_empty() {
                net.label.clone()
            } else if let Terminal::Port { instance, port } = &ins[0] {
                format!("{instance}_{}", port.replace('.', "_"))
            } else {
                "port".into()
            };
            let bp = unique_name(&taken, &wanted);
            taken.insert(bp.clone());
            group.ports.insert(bp.clone(), BoundaryPort { label: net.label.clone(), schema: None });
            let mut inner = ins.clone();
            inner.push(Terminal::boundary(&bp));
            group.nets.push(Net { label: net.label.clone(), terminals: inner });
            let mut outer = outs.clone();
            outer.push(Terminal::port(name, &bp));
            outer_nets.push(Net { label: net.label.clone(), terminals: outer });
        }
    }
    // Declare boundary types now, while the inner connection proves them.
    document.definitions.insert(definition.to_string(), group.clone());
    let ports: Vec<String> = group.ports.keys().cloned().collect();
    for bp in ports {
        let schema = Resolver::new(document, registry).boundary_schema(definition, &bp, &mut BTreeSet::new())?;
        document.definitions.get_mut(definition).unwrap().ports.get_mut(&bp).unwrap().schema = schema;
    }
    let mut placed = InstanceSpec::subsystem(definition);
    placed.label = label.to_string();
    placed.placement.position = center;
    for parameter in document.definitions[definition].parameters.keys() {
        placed.parameters.insert(parameter.clone(), ParameterBinding::Parameter { parameter: parameter.clone() });
    }
    let p = definition_mut(document, &parent_id)?;
    for s in &chosen {
        p.instances.remove(*s);
    }
    p.instances.insert(name.to_string(), placed);
    p.nets = outer_nets;
    Ok(outcome(document, registry, &parent_id, format!("Grouped {} instance(s) into {name} ({definition})", chosen.len())))
}

fn ungroup(document: &mut SystemDocument, registry: &BehaviorRegistry, at: &str, name: &str) -> Result<Outcome, SystemError> {
    let parent_id = Resolver::new(document, registry).definition_id_at(at)?;
    let parent = document.definitions[&parent_id].clone();
    let instance = parent.instances.get(name).ok_or_else(|| SystemError::Invalid(format!("no instance `{name}` in `{parent_id}`")))?.clone();
    let InstanceKind::Subsystem { definition: child_id } = &instance.kind else {
        return Err(SystemError::Invalid(format!("`{name}` is an element; there is nothing to ungroup")));
    };
    let child = document.definitions[child_id].clone();
    let mut taken: BTreeSet<String> = parent.instances.keys().filter(|k| *k != name).cloned().collect();
    let mut renamed = BTreeMap::new();
    for inner in child.instances.keys() {
        let new = if taken.contains(inner) { unique_name(&taken, &format!("{name}_{inner}")) } else { inner.clone() };
        taken.insert(new.clone());
        renamed.insert(inner.clone(), new);
    }
    let map = |t: &Terminal| match t {
        Terminal::Port { instance, port } => Terminal::port(&renamed[instance], port),
        b => b.clone(),
    };
    let mut result = parent.clone();
    result.instances.remove(name);
    for (inner, spec) in &child.instances {
        let mut spec = spec.clone();
        let offset = rotate(instance.placement.rotation_xyzw, spec.placement.position);
        for k in 0..3 {
            spec.placement.position[k] = instance.placement.position[k] + offset[k];
        }
        spec.placement.rotation_xyzw = mul(instance.placement.rotation_xyzw, spec.placement.rotation_xyzw);
        for binding in spec.parameters.values_mut() {
            if let ParameterBinding::Parameter { parameter } = binding.clone() {
                *binding = match instance.parameters.get(&parameter) {
                    Some(b) => b.clone(),
                    None => ParameterBinding::value(child.parameters.get(&parameter).and_then(|d| d.default).ok_or_else(|| {
                        SystemError::Invalid(format!("cannot ungroup: `{parameter}` has neither a value on {name} nor a default"))
                    })?),
                };
            }
        }
        result.instances.insert(renamed[inner].clone(), spec);
    }
    // Each boundary port joins the outer net that reaches it to the inner net behind it.
    let mut nets: Vec<Net> = Vec::new();
    let mut outer = parent.nets.clone();
    for inner in &child.nets {
        let boundaries: Vec<String> = inner.terminals.iter().filter_map(|t| if let Terminal::Boundary { boundary } = t { Some(boundary.clone()) } else { None }).collect();
        let mut terminals: Vec<Terminal> = inner.terminals.iter().filter(|t| t.instance().is_some()).map(map).collect();
        for bp in boundaries {
            let alias = Terminal::port(name, &bp);
            if let Some(pos) = outer.iter().position(|n| n.terminals.contains(&alias)) {
                let net = outer.remove(pos);
                terminals.extend(net.terminals.into_iter().filter(|t| t != &alias));
            }
        }
        nets.push(Net { label: inner.label.clone(), terminals });
    }
    for net in outer {
        let terminals: Vec<Terminal> = net.terminals.into_iter().filter(|t| t.instance() != Some(name)).collect();
        nets.push(Net { label: net.label, terminals });
    }
    // Nets that met through the same outer net are one net now.
    let mut merged: Vec<Net> = Vec::new();
    for net in nets.into_iter().filter(|n| n.terminals.len() >= 2) {
        if let Some(existing) = merged.iter_mut().find(|m| m.terminals.iter().any(|t| net.terminals.contains(t))) {
            for t in net.terminals {
                if !existing.terminals.contains(&t) {
                    existing.terminals.push(t);
                }
            }
        } else {
            merged.push(net);
        }
    }
    result.nets = merged;
    document.definitions.insert(parent_id.clone(), result);
    Ok(outcome(document, registry, &parent_id, format!("Ungrouped {name} into {} instance(s)", child.instances.len())))
}

fn swap(document: &mut SystemDocument, registry: &BehaviorRegistry, at: &str, name: &str, kind: &InstanceKind, keep_parameters: bool) -> Result<Outcome, SystemError> {
    let parent_id = Resolver::new(document, registry).definition_id_at(at)?;
    let parent = document.definitions[&parent_id].clone();
    let current = parent.instances.get(name).ok_or_else(|| SystemError::Invalid(format!("no instance `{name}` in `{parent_id}`")))?.clone();
    let mut replacement = current.clone();
    replacement.kind = kind.clone();
    replacement.appearance = None;
    if !keep_parameters {
        replacement.parameters.clear();
    }
    // Keep only parameters the replacement declares.
    let declared: Option<BTreeSet<String>> = match kind {
        InstanceKind::Element { component_type } => registry
            .get(&component_type.as_str().into())
            .map_err(|_| SystemError::Invalid(format!("unregistered component `{component_type}`")))?
            .parameters
            .as_ref()
            .map(|p| p.iter().map(|d| d.name.clone()).collect()),
        InstanceKind::Subsystem { definition } => Some(
            document.definitions.get(definition).ok_or_else(|| SystemError::Invalid(format!("unknown definition `{definition}`")))?.parameters.keys().cloned().collect(),
        ),
    };
    if let Some(declared) = declared {
        replacement.parameters.retain(|k, _| declared.contains(k));
    }
    let resolver = Resolver::new(document, registry);
    let old_ports = resolver.instance_ports(&current)?;
    let new_ports = resolver.instance_ports(&replacement)?;
    let used: BTreeSet<String> = parent
        .nets
        .iter()
        .flat_map(|n| &n.terminals)
        .filter_map(|t| match t {
            Terminal::Port { instance, port } if instance == name => Some(port.clone()),
            _ => None,
        })
        .collect();
    let mut problems = Vec::new();
    for port in &used {
        match (old_ports.get(port).cloned().flatten(), new_ports.get(port)) {
            (_, None) => problems.push(format!("the replacement has no port `{port}`")),
            (Some(old), Some(Some(new))) if &old != new => problems.push(format!("`{port}` changes type ({}) → ({})", describe(&old), describe(new))),
            (Some(_), Some(None)) => problems.push(format!("the replacement's `{port}` has no proven type (declare it)")),
            _ => {}
        }
    }
    if !problems.is_empty() {
        return Err(SystemError::Invalid(format!("cannot swap {name}: {}", problems.join("; "))));
    }
    definition_mut(document, &parent_id)?.instances.insert(name.to_string(), replacement);
    Ok(outcome(document, registry, &parent_id, format!("Swapped {name} to {}", kind_label(kind))))
}

pub fn describe(schema: &PortSchema) -> String {
    match schema {
        PortSchema::Acausal(k) => k.name().to_string(),
        PortSchema::SignalIn(q) => format!("signal in {q:?}"),
        PortSchema::SignalOut(q) => format!("signal out {q:?}"),
    }
}

pub fn kind_label(kind: &InstanceKind) -> String {
    match kind {
        InstanceKind::Element { component_type } => component_type.clone(),
        InstanceKind::Subsystem { definition } => format!("subsystem {definition}"),
    }
}
