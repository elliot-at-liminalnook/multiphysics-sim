//! Native structural validation; dimensional expressions and geometry remain
//! authoritative in RoboCAD. Errors carry a field/identity path.
use super::{ComponentsState, form::definition};
use crate::cad::CadDocument;
use sim_runtime::cad_client::{ComponentOperation, ComponentParameter};
use std::collections::BTreeMap;

pub(super) fn validate_operation(
    op: &ComponentOperation,
    st: &ComponentsState,
    doc: &CadDocument,
) -> Result<(), String> {
    let nodes = &doc.doc.as_ref().ok_or("components.document: unread")?.nodes;
    let node = |id: &str| {
        nodes
            .iter()
            .find(|n| n.id == id)
            .ok_or_else(|| format!("components.nodes.{id}: missing node"))
    };
    let name = |s: &str| -> Result<(), String> {
        if s.trim().is_empty() {
            Err("components.name: enter a component name".into())
        } else {
            Ok(())
        }
    };
    let parameter_specs = |p: &BTreeMap<String, ComponentParameter>| -> Result<(), String> {
        for (key, p) in p {
            if key.is_empty()
                || !key.chars().enumerate().all(|(i, c)| {
                    c == '_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit())
                })
            {
                return Err(format!(
                    "components.parameters.{key}: invalid parameter name"
                ));
            }
            if !st
                .recipes
                .as_ref()
                .ok_or("components.recipes: authoritative units have not been read")?
                .units
                .contains(&p.unit)
            {
                return Err(format!(
                    "components.parameters.{key}.unit: unsupported unit {}",
                    p.unit
                ));
            }
            if st.recipes.as_ref().is_some_and(|r| r.units.contains(key))
                || ["pi", "tau"].contains(&key.as_str())
            {
                return Err(format!(
                    "components.parameters.{key}: reserved parameter name"
                ));
            }
            if !["measured", "derived", "estimated"].contains(&p.provenance.as_str()) {
                return Err(format!(
                    "components.parameters.{key}.provenance: choose measured, derived or estimated"
                ));
            }
            if !(p.value.is_number() || p.value.is_string()) {
                return Err(format!(
                    "components.parameters.{key}.value: expected number or dimensional expression"
                ));
            }
        }
        Ok(())
    };
    use ComponentOperation as O;
    match op {
        O::Make {
            ids,
            name: n,
            origin,
        }
        | O::Create {
            ids,
            name: n,
            origin,
        } => {
            name(n)?;
            finite(origin, "origin")?;
            if ids.is_empty() {
                return Err("components.selection: select nodes to capture".into());
            }
            let mut captured = std::collections::BTreeSet::new();
            let mut todo = ids.clone();
            while let Some(id) = todo.pop() {
                if !captured.insert(id.clone()) {
                    continue;
                }
                let n = node(&id)?;
                todo.extend(n.children.iter().cloned());
            }
            for id in &captured {
                let n = node(id)?;
                if let Some(owner) = n
                    .component_member
                    .as_ref()
                    .and_then(|m| m["instance_id"].as_str())
                    && !captured.contains(owner)
                {
                    return Err(format!(
                        "components.selection.{id}: capture the whole linked occurrence, not individual linked members"
                    ));
                }
            }
        }
        O::Parametric { name: n, shape } => {
            name(n)?;
            if !["box", "cylinder"].contains(&shape.as_str()) {
                return Err("components.shape: choose box or cylinder".into());
            }
        }
        O::Place {
            definition_id,
            placement,
            overrides,
            bindings,
            name: n,
            variant,
        } => {
            name(n)?;
            let d = definition(st, definition_id)?;
            transform(placement, "placement")?;
            for key in overrides.keys() {
                if !d.parameters.contains_key(key) {
                    return Err(format!("components.overrides.{key}: unknown parameter"));
                }
            }
            for (key, value) in bindings {
                let p = d
                    .ports
                    .get(key)
                    .ok_or_else(|| format!("components.bindings.{key}: unknown typed port"))?;
                let id = value.as_deref().ok_or_else(|| {
                    format!("components.bindings.{key}: no matching {} node", p.kind)
                })?;
                if node(id)?.kind != p.kind {
                    return Err(format!(
                        "components.bindings.{key}: {id} must be {}",
                        p.kind
                    ));
                }
            }
            for key in d.ports.keys() {
                if !bindings.contains_key(key) {
                    return Err(format!("components.bindings.{key}: required typed port"));
                }
            }
            if let Some(v) = variant
                && !d.variants.contains_key(v)
            {
                return Err(format!("components.variant.{v}: missing family variant"));
            }
        }
        O::Defaults {
            definition_id,
            parameters,
            features,
            nested,
            family_variants,
        } => {
            let d = definition(st, definition_id)?;
            parameter_specs(parameters)?;
            for (i, feature) in features.iter().enumerate() {
                let kind = feature["kind"]
                    .as_str()
                    .ok_or_else(|| format!("components.features.{i}.kind: required recipe kind"))?;
                if !st
                    .catalogue
                    .as_ref()
                    .is_some_and(|(_, c)| c.features.get(kind).is_some())
                {
                    return Err(format!(
                        "components.features.{i}.kind: unsupported recipe {kind}"
                    ));
                }
                let target = feature["node"].as_str().ok_or_else(|| {
                    format!("components.features.{i}.node: required target identity")
                })?;
                if !(target == "*" && kind == "assembly_placement") {
                    let node = d.targets.iter().find(|n| n.id == target).ok_or_else(|| {
                        format!("components.features.{i}.node: missing definition target {target}")
                    })?;
                    if node.component_member.is_some() {
                        return Err(format!(
                            "components.features.{i}.node: map parameters to the nested component instead of editing its derived members"
                        ));
                    }
                }
            }
            for (id, mapping) in nested {
                let original = d.nested.get(id).ok_or_else(|| {
                    format!("components.nested.{id}: must target an immediate child occurrence")
                })?;
                if mapping.definition_id != original.definition_id {
                    return Err(format!(
                        "components.nested.{id}.definition_id: changing topology through parameter mappings is unsupported"
                    ));
                }
            }
            if let Some(variants) = family_variants {
                if variants.keys().ne(d.variants.keys()) {
                    return Err("components.variants: variant names are read-only in Edit defaults; create a new family to change membership".into());
                }
                for (name, v) in variants {
                    if d.variants
                        .get(name)
                        .is_none_or(|old| old.definition_id != v.definition_id)
                    {
                        return Err(format!(
                            "components.variants.{name}.definition_id: read-only in Edit defaults"
                        ));
                    }
                    let target = definition(st, &v.definition_id)?;
                    if !target.variants.is_empty() {
                        return Err(format!(
                            "components.variants.{name}: a variant must be an assembly definition"
                        ));
                    }
                }
            }
        }
        O::Overrides {
            instance_id,
            overrides,
            placement,
        } => {
            let n = node(instance_id)?;
            let instance = n.component_instance.as_ref().ok_or_else(|| {
                format!("components.occurrence.{instance_id}: select a component occurrence")
            })?;
            if n.component_member.is_some() && placement.is_some() {
                return Err(format!(
                    "components.occurrence.{instance_id}.placement: move nested components through the parent definition"
                ));
            }
            let id = instance["definition_id"]
                .as_str()
                .ok_or("components.occurrence.definition_id: missing")?;
            let d = definition(st, id)?;
            for key in overrides.keys() {
                if !d.parameters.contains_key(key) {
                    return Err(format!("components.overrides.{key}: unknown parameter"));
                }
            }
            if let Some(p) = placement {
                transform(p, "placement")?
            }
        }
        O::Detach { instance_id } => {
            let n = node(instance_id)?;
            if n.component_instance.is_none() {
                return Err("components.detach: select a linked occurrence".into());
            }
            if n.component_member.is_some() {
                return Err("components.detach: detach the outer occurrence first".into());
            }
        }
        O::Import { path } => library_path(path)?,
        O::Export {
            definition_id,
            path,
        } => {
            definition(st, definition_id)?;
            library_path(path)?;
        }
        O::Family {
            name: n,
            variants,
            parameters,
            default_variant,
        } => {
            name(n)?;
            parameter_specs(parameters)?;
            if variants.is_empty() {
                return Err("components.variants: a family requires at least one variant".into());
            }
            for (key, v) in variants {
                if !definition(st, &v.definition_id)?.variants.is_empty() {
                    return Err(format!(
                        "components.variants.{key}: must use an assembly definition"
                    ));
                }
            }
            if let Some(v) = default_variant
                && !variants.contains_key(v)
            {
                return Err(format!("components.default_variant.{v}: missing variant"));
            }
        }
        O::LinkFamily {
            instance_id,
            definition_id,
            variant,
            ..
        } => {
            let n = node(instance_id)?;
            if n.component_member.is_some() {
                return Err(
                    "components.link_family: link a top-level occurrence before nesting it".into(),
                );
            }
            let d = definition(st, definition_id)?;
            let target = d
                .variants
                .get(variant)
                .ok_or_else(|| format!("components.variants.{variant}: select a family variant"))?;
            if n.component_instance
                .as_ref()
                .and_then(|i| i["definition_id"].as_str())
                != Some(target.definition_id.as_str())
            {
                return Err("components.link_family: occurrence must already use the selected variant definition".into());
            }
        }
        O::Transform {
            ids,
            translation,
            axis,
            angle_deg,
            scale,
            center,
        } => {
            finite(translation, "translation")?;
            finite(axis, "axis")?;
            if !angle_deg.is_finite() || !scale.is_finite() || *scale != 1.0 {
                return Err("components.transform: linked occurrences support finite rigid transforms only (scale 1)".into());
            }
            if let Some(c) = center {
                finite(c, "center")?
            }
            if ids.is_empty() {
                return Err("components.transform: select linked occurrences".into());
            }
            for id in ids {
                let n = node(id)?;
                if n.component_member.is_some() || n.component_instance.is_none() {
                    return Err(format!(
                        "components.transform.{id}: move linked members through the outer occurrence"
                    ));
                }
                if n.locked {
                    return Err(format!("components.transform.{id}: occurrence is locked"));
                }
            }
        }
    }
    Ok(())
}
fn finite(v: &[f64; 3], path: &str) -> Result<(), String> {
    if v.iter().all(|x| x.is_finite()) {
        Ok(())
    } else {
        Err(format!("components.{path}: values must be finite"))
    }
}
fn library_path(path: &str) -> Result<(), String> {
    if !std::path::Path::new(path).is_absolute() || !path.ends_with(".rcomp") {
        Err("components.path: choose an absolute *.rcomp path".into())
    } else {
        Ok(())
    }
}
fn transform(v: &serde_json::Value, path: &str) -> Result<(), String> {
    for key in ["translation", "axis"] {
        let a = v[key]
            .as_array()
            .ok_or_else(|| format!("components.{path}.{key}: expected vector"))?;
        if a.len() != 3 || a.iter().any(|x| x.as_f64().is_none_or(|v| !v.is_finite())) {
            return Err(format!(
                "components.{path}.{key}: expected three finite values"
            ));
        }
    }
    if v["angle_deg"].as_f64().is_none_or(|v| !v.is_finite()) || v["scale"].as_f64() != Some(1.0) {
        return Err(format!(
            "components.{path}: rigid placement requires finite angle and scale 1"
        ));
    }
    Ok(())
}
