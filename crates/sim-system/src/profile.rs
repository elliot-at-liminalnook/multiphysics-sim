//! The realtime profile of a system: the same topology with every part's
//! realtime model. Elements take their notes' realtime parameter values;
//! subsystems whose definition names a realtime counterpart are swapped to
//! it; the run uses the profile's step and integrator. Everything goes
//! through the shared commands, so the result is an ordinary document.
use crate::document::*;
use crate::{Command, SystemError};
use sim_core::BehaviorRegistry;

/// Every (level path, instance) in the hierarchy, parents first.
fn walk(document: &SystemDocument, definition: &str, path: &str, out: &mut Vec<(String, String, InstanceKind)>, depth: usize) {
    if depth > 32 {
        return;
    }
    let Some(d) = document.definitions.get(definition) else { return };
    for (name, spec) in &d.instances {
        out.push((path.to_string(), name.clone(), spec.kind.clone()));
        if let InstanceKind::Subsystem { definition } = &spec.kind {
            walk(document, definition, &crate::join_path(path, name), out, depth + 1);
        }
    }
}

/// The realtime variant of `document` (unchanged when it has no profile).
pub fn realtime(document: &SystemDocument, registry: &BehaviorRegistry) -> Result<SystemDocument, SystemError> {
    let Some(profile) = document.realtime.clone() else { return Ok(document.clone()) };
    let mut doc = document.clone();
    // Swap subsystems to their realtime counterparts first (their contents change).
    let mut swaps = Vec::new();
    let mut all = Vec::new();
    walk(&doc, &doc.root.clone(), "", &mut all, 0);
    for (at, name, kind) in &all {
        if let InstanceKind::Subsystem { definition } = kind {
            if let Some(counterpart) = doc.definitions.get(definition).and_then(|d| d.realtime.clone()) {
                swaps.push(Command::Swap { at: at.clone(), name: name.clone(), kind: InstanceKind::Subsystem { definition: counterpart }, keep_parameters: true });
            }
        }
    }
    // Only swap the outermost occurrence; nested ones disappear with it.
    let mut applied: Vec<String> = Vec::new();
    for c in swaps {
        let Command::Swap { at, name, .. } = &c else { continue };
        let path = crate::join_path(at, name);
        if applied.iter().any(|p| path.starts_with(&format!("{p}/"))) {
            continue;
        }
        crate::apply(&mut doc, registry, std::slice::from_ref(&c))?;
        applied.push(path);
    }
    // Element realtime parameter values. Definitions are shared, so each
    // definition's instances are edited once (at its first placement).
    let mut all = Vec::new();
    walk(&doc, &doc.root.clone(), "", &mut all, 0);
    let mut seen = std::collections::BTreeSet::new();
    let mut edits = Vec::new();
    for (at, name, kind) in &all {
        let InstanceKind::Element { component_type } = kind else { continue };
        let Ok(parent) = crate::Resolver::new(&doc, registry).definition_id_at(at) else { continue };
        if !seen.insert((parent, name.clone())) {
            continue;
        }
        let Some(notes) = registry.get(&component_type.as_str().into()).ok().and_then(|d| d.notes) else { continue };
        for (parameter, value) in notes.realtime {
            edits.push(Command::SetParameter { at: at.clone(), name: name.clone(), parameter: parameter.to_string(), binding: Some(ParameterBinding::value(*value)) });
        }
    }
    crate::apply(&mut doc, registry, &edits)?;
    let mut run = doc.run.clone().unwrap_or(RunSettings { integrator: profile.integrator, interval: profile.interval, absolute_tolerance: None, relative_tolerance: None, max_iterations: None, rationale: String::new() });
    run.interval = profile.interval;
    run.integrator = profile.integrator;
    run.rationale = format!("Realtime profile of: {}", run.rationale);
    doc.run = Some(run);
    doc.title = format!("{} (realtime)", doc.title);
    Ok(doc)
}
