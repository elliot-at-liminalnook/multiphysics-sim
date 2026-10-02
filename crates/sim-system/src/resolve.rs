//! Port signatures, terminal types, net compatibility and document validation.
//! Validation separates hard errors (the document is inconsistent and an edit
//! producing it is rejected) from findings (incomplete but inspectable work).
use crate::document::*;
use crate::SystemError;
use serde::Serialize;
use sim_core::{BehaviorRegistry, ModelWorld, PortSchema};
use std::collections::{BTreeMap, BTreeSet};

/// Ports of one registry element, including composite members (`plug.thermal`)
/// and the wildcard family members its parameter names create.
pub fn element_ports(
    registry: &BehaviorRegistry,
    component_type: &str,
    parameter_names: &BTreeSet<String>,
) -> Result<BTreeMap<String, PortSchema>, SystemError> {
    let mut scratch = ModelWorld::default();
    let instance = scratch
        .part(registry, "signature", component_type, parameter_names.iter().map(|n| (n.as_str(), 0.0)))
        .map_err(|e| SystemError::Invalid(format!("{component_type}: {e}")))?;
    Ok(instance.ports.iter().map(|(name, id)| (name.clone(), scratch.ports[*id].schema.clone())).collect())
}

/// Top-level (non-member) ports of an element, for connection menus.
pub fn element_top_ports(registry: &BehaviorRegistry, component_type: &str, parameter_names: &BTreeSet<String>) -> Result<BTreeMap<String, PortSchema>, SystemError> {
    let all = element_ports(registry, component_type, parameter_names)?;
    let members: BTreeSet<String> = all.keys().filter(|k| all.keys().any(|p| p != *k && k.starts_with(&format!("{p}.")))).cloned().collect();
    Ok(all.into_iter().filter(|(k, _)| !members.contains(k)).collect())
}

pub struct Resolver<'a> {
    pub document: &'a SystemDocument,
    pub registry: &'a BehaviorRegistry,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Finding {
    pub code: String,
    pub message: String,
    /// Instance path the finding concerns, when it concerns one.
    pub subject: Option<String>,
}

impl<'a> Resolver<'a> {
    pub fn new(document: &'a SystemDocument, registry: &'a BehaviorRegistry) -> Self {
        Self { document, registry }
    }

    pub fn definition(&self, id: &str) -> Result<&'a Definition, SystemError> {
        self.document.definitions.get(id).ok_or_else(|| SystemError::Invalid(format!("unknown definition `{id}`")))
    }

    /// The definition whose contents are edited at an instance path.
    pub fn definition_id_at(&self, path: &str) -> Result<String, SystemError> {
        let mut id = self.document.root.clone();
        for name in split_path(path) {
            let definition = self.definition(&id)?;
            let instance = definition
                .instances
                .get(name)
                .ok_or_else(|| SystemError::Invalid(format!("no instance `{name}` in `{id}` (path `{path}`)")))?;
            match &instance.kind {
                InstanceKind::Subsystem { definition } => id = definition.clone(),
                InstanceKind::Element { component_type } => {
                    return Err(SystemError::Invalid(format!("`{name}` is a {component_type} element, not a subsystem; swap it for a subsystem to implement it further")));
                }
            }
        }
        Ok(id)
    }

    /// Number of instances, anywhere in the hierarchy under the root, that
    /// place this definition. Editing it affects all of them.
    pub fn placements(&self, definition: &str) -> usize {
        fn count(r: &Resolver, current: &str, target: &str, depth: usize) -> usize {
            if depth > 64 {
                return 0;
            }
            let Ok(d) = r.definition(current) else { return 0 };
            d.instances
                .values()
                .map(|i| match &i.kind {
                    InstanceKind::Subsystem { definition } => (definition == target) as usize + count(r, definition, target, depth + 1),
                    _ => 0,
                })
                .sum()
        }
        if definition == self.document.root { 1 } else { count(self, &self.document.root, definition, 0) }
    }

    /// Port types an instance exposes: element ports, or a subsystem's
    /// boundary ports (inferred from inside when not declared).
    pub fn instance_ports(&self, instance: &InstanceSpec) -> Result<BTreeMap<String, Option<PortSchema>>, SystemError> {
        match &instance.kind {
            InstanceKind::Element { component_type } => {
                let names = instance.parameters.keys().cloned().collect();
                Ok(element_ports(self.registry, component_type, &names)?.into_iter().map(|(k, v)| (k, Some(v))).collect())
            }
            InstanceKind::Subsystem { definition } => {
                let d = self.definition(definition)?;
                let mut out = BTreeMap::new();
                for name in d.ports.keys() {
                    out.insert(name.clone(), self.boundary_schema(definition, name, &mut BTreeSet::new())?);
                }
                Ok(out)
            }
        }
    }

    /// A boundary port's type: its declaration, else what it joins inside.
    pub fn boundary_schema(&self, definition: &str, port: &str, visiting: &mut BTreeSet<(String, String)>) -> Result<Option<PortSchema>, SystemError> {
        let d = self.definition(definition)?;
        let declared = d.ports.get(port).ok_or_else(|| SystemError::Invalid(format!("`{definition}` has no boundary port `{port}`")))?.schema.clone();
        if declared.is_some() {
            return Ok(declared);
        }
        if !visiting.insert((definition.to_string(), port.to_string())) || visiting.len() > 256 {
            return Ok(None);
        }
        let own = Terminal::boundary(port);
        for net in d.nets.iter().filter(|n| n.terminals.contains(&own)) {
            for terminal in &net.terminals {
                if terminal == &own {
                    continue;
                }
                if let Some(schema) = self.terminal_schema(definition, terminal, visiting)? {
                    return Ok(Some(schema));
                }
            }
        }
        Ok(None)
    }

    pub fn terminal_schema(&self, definition: &str, terminal: &Terminal, visiting: &mut BTreeSet<(String, String)>) -> Result<Option<PortSchema>, SystemError> {
        let d = self.definition(definition)?;
        match terminal {
            Terminal::Boundary { boundary } => self.boundary_schema(definition, boundary, visiting),
            Terminal::Port { instance, port } => {
                let spec = d.instances.get(instance).ok_or_else(|| SystemError::Invalid(format!("net refers to missing instance `{instance}` in `{definition}`")))?;
                match &spec.kind {
                    InstanceKind::Element { component_type } => {
                        let names = spec.parameters.keys().cloned().collect();
                        let ports = element_ports(self.registry, component_type, &names)?;
                        ports.get(port).cloned().map(Some).ok_or_else(|| {
                            SystemError::Invalid(format!("{component_type} `{instance}` has no port `{port}`; it has {:?}", ports.keys().collect::<Vec<_>>()))
                        })
                    }
                    InstanceKind::Subsystem { definition: child } => self.boundary_schema(child, port, visiting),
                }
            }
        }
    }

    /// Hard validation: everything referenced exists, names are valid, the
    /// subsystem graph is acyclic, parameters are known with matching units,
    /// and every net joins compatible port types.
    pub fn validate(&self) -> Result<(), SystemError> {
        for d in self.document.definitions.values() {d.grid.validate()?; if !d.icon.is_empty()&&!sim_core::icons::NAMES.contains(&d.icon.as_str()){return Err(SystemError::Invalid("unknown display icon".into()));}}
        for (id,t) in &self.document.discussions.threads {crate::display::validate_thread(t)?;if id!=&t.id{return Err(SystemError::Invalid("thread key does not match its ID".into()));}}

        let doc = self.document;
        if doc.schema != SCHEMA {
            return Err(SystemError::Invalid(format!("unsupported schema `{}`; expected `{SCHEMA}`", doc.schema)));
        }
        self.definition(&doc.root)?;
        for (id, d) in &doc.definitions {
            if !valid_definition_id(id) {
                return Err(SystemError::Invalid(format!("invalid definition id `{id}`")));
            }
            for name in d.ports.keys().chain(d.parameters.keys()).chain(d.instances.keys()).chain(d.references.keys()) {
                if !valid_name(name) {
                    return Err(SystemError::Invalid(format!("invalid name `{name}` in `{id}`")));
                }
            }
            for (name, p) in &d.parameters {
                if p.default.is_some_and(|v| !v.is_finite()) {
                    return Err(SystemError::Invalid(format!("parameter `{name}` of `{id}` has a nonfinite default")));
                }
            }
            for (name, instance) in &d.instances {
                self.validate_instance(id, d, name, instance)?;
            }
            let mut seen = BTreeSet::new();
            for net in &d.nets {
                if net.terminals.is_empty() {
                    return Err(SystemError::Invalid(format!("empty net in `{id}`")));
                }
                for t in &net.terminals {
                    if !seen.insert(t.clone()) {
                        return Err(SystemError::Invalid(format!("terminal {t} appears in two nets of `{id}`; merge them")));
                    }
                    if let Terminal::Boundary { boundary } = t {
                        if !d.ports.contains_key(boundary) {
                            return Err(SystemError::Invalid(format!("net refers to missing boundary port `{boundary}` of `{id}`")));
                        }
                    }
                }
                self.check_net(id, net)?;
            }
            for (name, reference) in &d.references {
                if !doc.assets.contains_key(&reference.asset) {
                    return Err(SystemError::Invalid(format!("reference `{name}` uses missing asset `{}`", reference.asset)));
                }
                let finite = reference.origin.iter().chain(&reference.normal).chain(&reference.x_axis).all(|v| v.is_finite());
                if !finite || !(reference.width.is_finite() && reference.width > 0.) || !(0.0..=1.0).contains(&reference.opacity) {
                    return Err(SystemError::Invalid(format!("reference `{name}` needs finite placement, positive width and opacity in [0, 1]")));
                }
            }
        }
        self.check_acyclic()?;
        Ok(())
    }

    fn validate_instance(&self, definition_id: &str, definition: &Definition, name: &str, instance: &InstanceSpec) -> Result<(), SystemError> {
        let p = &instance.placement;
        if !p.position.iter().chain(&p.rotation_xyzw).all(|v| v.is_finite())
            || (p.rotation_xyzw.iter().map(|v| v * v).sum::<f32>() - 1.).abs() > 1e-3
            || p.schematic.is_some_and(|s| !s.iter().all(|v| v.is_finite()))
        {
            return Err(SystemError::Invalid(format!("`{name}` in `{definition_id}` needs a finite placement and a unit rotation")));
        }
        for (parameter, binding) in &instance.parameters {
            if let ParameterBinding::Parameter { parameter: from } = binding {
                if !definition.parameters.contains_key(from) {
                    return Err(SystemError::Invalid(format!("`{name}.{parameter}` inherits `{from}`, which `{definition_id}` does not declare")));
                }
            }
            if let ParameterBinding::Value { value, .. } = binding {
                if !value.is_finite() {
                    return Err(SystemError::Invalid(format!("`{name}.{parameter}` is not finite")));
                }
            }
        }
        match &instance.kind {
            InstanceKind::Element { component_type } => {
                let descriptor = self
                    .registry
                    .get(&component_type.as_str().into())
                    .map_err(|_| SystemError::Invalid(format!("`{name}` uses unregistered component `{component_type}`")))?;
                if let Some(declared) = &descriptor.parameters {
                    for (parameter, binding) in &instance.parameters {
                        let Some(decl) = declared.iter().find(|d| d.name == *parameter || wildcard_match(&d.name, parameter)) else {
                            return Err(SystemError::Invalid(format!("{component_type} has no parameter `{parameter}` (instance `{name}`)")));
                        };
                        if let ParameterBinding::Value { unit: Some(unit), value, .. } = binding {
                            if unit != &decl.unit {
                                return Err(SystemError::Invalid(format!("`{name}.{parameter}` is given in `{unit}` but {component_type} declares `{}`; values are not converted", decl.unit)));
                            }
                            check_bounds(name, parameter, *value, decl)?;
                        } else if let ParameterBinding::Value { value, .. } = binding {
                            check_bounds(name, parameter, *value, decl)?;
                        }
                    }
                }
            }
            InstanceKind::Subsystem { definition: child } => {
                let d = self.definition(child)?;
                for (parameter, binding) in &instance.parameters {
                    let Some(decl) = d.parameters.get(parameter) else {
                        return Err(SystemError::Invalid(format!("subsystem `{child}` declares no parameter `{parameter}` (instance `{name}`)")));
                    };
                    if let ParameterBinding::Value { unit: Some(unit), .. } = binding {
                        if unit != &decl.unit {
                            return Err(SystemError::Invalid(format!("`{name}.{parameter}` is given in `{unit}` but `{child}` declares `{}`", decl.unit)));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn check_acyclic(&self) -> Result<(), SystemError> {
        fn visit(r: &Resolver, id: &str, stack: &mut Vec<String>) -> Result<(), SystemError> {
            if stack.iter().any(|s| s == id) {
                return Err(SystemError::Invalid(format!("definition cycle: {} -> {id}", stack.join(" -> "))));
            }
            stack.push(id.to_string());
            for instance in r.definition(id)?.instances.values() {
                if let InstanceKind::Subsystem { definition } = &instance.kind {
                    visit(r, definition, stack)?;
                }
            }
            stack.pop();
            Ok(())
        }
        for id in self.document.definitions.keys() {
            visit(self, id, &mut Vec::new())?;
        }
        Ok(())
    }

    /// Type-check one net. Physical terminals must share a connector (a plain
    /// port may join the matching member of a composite); a signal net has at
    /// most one output and every input reads its quantity.
    pub fn check_net(&self, definition: &str, net: &Net) -> Result<(), SystemError> {
        let mut schemas = Vec::new();
        for t in &net.terminals {
            if let Some(s) = self.terminal_schema(definition, t, &mut BTreeSet::new())? {
                schemas.push((t, s));
            }
        }
        compatible(&schemas).map_err(|e| SystemError::Invalid(format!("net in `{definition}`: {e}")))
    }

    /// Incomplete-but-valid findings for review and the viewers.
    pub fn findings(&self) -> Vec<Finding> {
        let mut out = Vec::new();
        self.findings_at("", &self.document.root, &mut out, 0);
        out
    }

    fn findings_at(&self, path: &str, definition: &str, out: &mut Vec<Finding>, depth: usize) {
        let Ok(d) = self.definition(definition) else { return };
        if depth > 64 {
            return;
        }
        let connected: BTreeSet<&Terminal> = d.nets.iter().flat_map(|n| &n.terminals).collect();
        for (name, instance) in &d.instances {
            let here = join_path(path, name);
            if let Ok(ports) = self.instance_ports(instance) {
                let top: Vec<&String> = ports.keys().filter(|k| !ports.keys().any(|p| p != *k && k.starts_with(&format!("{p}.")))).collect();
                for port in top {
                    // Unused outputs are fine; they stay observable.
                    if matches!(ports.get(port), Some(Some(PortSchema::SignalOut(_)))) {
                        continue;
                    }
                    let members_connected = connected.iter().any(|t| matches!(t, Terminal::Port { instance: i, port: p } if i == name && p.starts_with(&format!("{port}."))));
                    if !connected.contains(&Terminal::port(name, port)) && !members_connected {
                        out.push(Finding { code: "unconnected_port".into(), message: format!("{here}.{port} is not connected"), subject: Some(here.clone()) });
                    }
                }
            }
            match &instance.kind {
                InstanceKind::Element { component_type } => {
                    if let Ok(descriptor) = self.registry.get(&component_type.as_str().into()) {
                        for decl in descriptor.parameters.iter().flatten() {
                            if decl.required && !decl.name.contains('*') && !instance.parameters.contains_key(&decl.name) {
                                out.push(Finding { code: "missing_parameter".into(), message: format!("{here} needs `{}` ({})", decl.name, decl.unit), subject: Some(here.clone()) });
                            }
                        }
                    }
                }
                InstanceKind::Subsystem { definition: child } => {
                    if let Ok(cd) = self.definition(child) {
                        for (p, decl) in &cd.parameters {
                            if decl.default.is_none() && !instance.parameters.contains_key(p) {
                                out.push(Finding { code: "missing_parameter".into(), message: format!("{here} needs `{p}` ({})", decl.unit), subject: Some(here.clone()) });
                            }
                        }
                    }
                    self.findings_at(&here, child, out, depth + 1);
                }
            }
        }
        for port in d.ports.keys() {
            if !connected.contains(&Terminal::boundary(port)) {
                out.push(Finding { code: "open_boundary".into(), message: format!("boundary port {port} of `{definition}` joins nothing inside"), subject: (!path.is_empty()).then(|| path.to_string()) });
            }
        }
    }
}

fn wildcard_match(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        Some((prefix, suffix)) => name.starts_with(prefix) && name.ends_with(suffix) && name.len() > prefix.len() + suffix.len(),
        None => false,
    }
}

fn check_bounds(name: &str, parameter: &str, value: f64, decl: &sim_core::ParameterDeclaration) -> Result<(), SystemError> {
    let below = decl.minimum.is_some_and(|m| if decl.exclusive_minimum { value <= m } else { value < m });
    let above = decl.maximum.is_some_and(|m| value > m);
    if below || above {
        return Err(SystemError::Invalid(format!(
            "`{name}.{parameter}` = {value} is outside its declared range{}{}",
            decl.minimum.map(|m| format!(" (minimum {m}{})", if decl.exclusive_minimum { ", exclusive" } else { "" })).unwrap_or_default(),
            decl.maximum.map(|m| format!(" (maximum {m})")).unwrap_or_default()
        )));
    }
    if decl.integer && value.fract() != 0. {
        return Err(SystemError::Invalid(format!("`{name}.{parameter}` must be an integer")));
    }
    Ok(())
}

/// Whether these terminal types may share one net.
pub fn compatible(schemas: &[(&Terminal, PortSchema)]) -> Result<(), String> {
    crate::composition::validate_port_schemas(schemas)
}
