//! Model context, without compilation or simulation. Call `build` on a worker.
//! Resolution and net merging use the same authoring path as the runtime.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sim_core::BehaviorRegistry;
use sim_system::{InstanceKind, SystemDocument};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Request {
    pub discussion: Option<String>,
    pub targets: Vec<String>,
}

pub fn build(
    document: &SystemDocument,
    registry: &BehaviorRegistry,
    request: &Request,
) -> Result<Value, String> {
    let mut doc = document.clone();
    sim_system::display::refresh(&mut doc);
    let thread = request
        .discussion
        .as_ref()
        .map(|id| {
            doc.discussions
                .threads
                .get(id)
                .ok_or_else(|| format!("unknown discussion {id}"))
        })
        .transpose()?;
    let all = sim_system::display::paths(&doc);
    for path in &request.targets {
        if !all.contains_key(path) {
            return Err(format!("unknown model target {path}"));
        }
    }
    let mut targets: BTreeSet<String> = request.targets.iter().cloned().collect();
    let mut missing = Vec::new();
    if let Some(t) = thread {
        for target in t
            .targets
            .iter()
            .chain(t.comments.iter().flat_map(|c| &c.links))
        {
            if target.missing {
                missing.push(target);
            } else {
                targets.insert(target.path.clone());
            }
        }
    }
    // An unlinked discussion is model-wide; a deleted target is not silently
    // retargeted to an unrelated instance that happens to reuse its name.
    let model_wide = targets.is_empty() && missing.is_empty();
    let mut focus: BTreeSet<String> = all
        .keys()
        .filter(|path| {
            model_wide
                || targets
                    .iter()
                    .any(|t| *path == t || path.starts_with(&format!("{t}/")))
        })
        .cloned()
        .collect();
    let flat = sim_system::flatten(&doc, registry).map_err(|e| e.to_string());
    let description = flat.as_ref().map_err(Clone::clone).and_then(|flat| {
        sim_inspect::model::describe(
            &flat.model,
            registry,
            &flat.source_hash,
            flat.revision,
            &flat.identities,
        )
        .map(|m| m.description)
        .map_err(|e| e.to_string())
    });
    let mut neighbors = BTreeSet::new();
    let mut nets = BTreeMap::new();
    if let Ok(d) = &description {
        for (id, net) in &d.nets {
            if net
                .ports
                .iter()
                .filter_map(|p| d.ports.get(p))
                .any(|p| focus.contains(&p.component))
            {
                nets.insert(id.clone(), net);
                for p in net.ports.iter().filter_map(|p| d.ports.get(p)) {
                    if !focus.contains(&p.component) {
                        neighbors.insert(p.component.clone());
                    }
                }
            }
        }
    }
    let requested_count = focus.len() + neighbors.len();
    let included: BTreeSet<_> = focus
        .iter()
        .chain(neighbors.iter())
        .take(128)
        .cloned()
        .collect();
    focus.retain(|p| included.contains(p));
    neighbors.retain(|p| included.contains(p));
    let mut authored = BTreeMap::new();
    let mut definitions = BTreeMap::new();
    let mut types = BTreeSet::new();
    let resolver = sim_system::Resolver::new(&doc, registry);
    for path in &included {
        // Carry parent bindings too: they explain parameter inheritance and
        // preserve authored provenance that the flattened numeric model omits.
        let mut current = path.as_str();
        loop {
            let (parent, name) = current.rsplit_once('/').unwrap_or(("", current));
            if let Ok(id) = resolver.definition_id_at(parent) {
                let def = &doc.definitions[&id];
                if let Some(instance) = def.instances.get(name) {
                    authored.insert(current.to_owned(), json!({
                        "definition":id,"instance":instance,
                        "json_pointer":format!("/definitions/{}/instances/{}", pointer(&id), pointer(name)),
                        "lineage":all.get(current).map(|x| &x.0),
                    }));
                    definitions.insert(id.clone(), def);
                    if let InstanceKind::Element { component_type } = &instance.kind {
                        types.insert(component_type.clone());
                    }
                    if let InstanceKind::Subsystem { definition } = &instance.kind {
                        if let Some(def) = doc.definitions.get(definition) {
                            definitions.insert(definition.clone(), def);
                        }
                    }
                }
            }
            if parent.is_empty() {
                break;
            }
            current = parent;
        }
    }
    let mut knowledge = BTreeMap::new();
    for ty in types {
        if let Ok(d) = registry.get(&ty.as_str().into()) {
            // Defaults describe the contract, never replace an unknown model value.
            let parameters: Vec<_> = d
                .parameters
                .iter()
                .flatten()
                .filter(|p| !p.implementation_reference)
                .map(|p| {
                    json!({"name":p.name,"unit":p.unit,"required":p.required,"default":p.default,
                    "minimum":p.minimum,"maximum":p.maximum,
                    "help":d.notes.and_then(|n| n.parameter_help(&p.name))})
                })
                .collect();
            knowledge.insert(ty, json!({"display_name":d.display_name,"parameters":parameters,
                "notes":d.notes.map(|n| json!({"summary":n.summary,"explanation":n.explanation,
                    "equations":n.equations,"tradeoffs":n.tradeoffs,"limits":n.limits,"pairs_with":n.pairs_with}))}));
        }
    }
    let resolved = description.as_ref().ok().map(|d| {
        let components: BTreeMap<_,_> = d.components.iter().filter(|(p,_)| included.contains(*p)).collect();
        let ports: BTreeMap<_,_> = d.ports.iter().filter(|(_,p)| included.contains(&p.component)).collect();
        let observables: BTreeMap<_,_> = d.observables.iter().filter(|(_,o)| {
            use sim_inspect::ObservationLocation::*;
            match &o.location {
                State{component,..} | Diagnostic{component:Some(component),..} => included.contains(component),
                Across{port,..}|Through{port,..}|Signal{port} => ports.contains_key(port),
                Diagnostic{component:None,..} => true,
            }
        }).collect();
        json!({"components":components,"ports":ports,"nets":nets,"observables":observables,
            "quantity_and_connector_definitions":d.definitions,"diagnostics":d.diagnostics,
            "parameter_provenance_note":"Resolved numeric values use the shared model factory. Authored provenance, uncertainty and inherited bindings are in authored_instances and definitions; unspecified is not measured."})
    });
    Ok(json!({
        "schema":"sim.model-context/v1", "read_only":true,
        "discussion":thread,
        "source":{"title":doc.title,"revision":doc.revision,"content_hash":document.content_hash()},
        "scope":{"discussion":request.discussion,"targets":targets,"model_wide":model_wide,
            "missing_targets":missing,"focus":focus,"connected_neighbors":neighbors,
            "included_instances":included.len(),"omitted_instances":requested_count-included.len(),
            "limit":128,"followup":"system_context with targets [instance/path] inspects omitted or neighboring parts"},
        "authored_instances":authored,"definitions":definitions,"component_knowledge":knowledge,
        "resolved_model":resolved,"resolution_error":description.err(),
        "findings":resolver.findings(),
        "run_settings":doc.run,"realtime_profile":doc.realtime,"studies":doc.studies,
        "display":{"semantics":sim_system::display::SEMANTICS,"length_unit":"m","coordinate_frame":"right_handed_y_up",
            "parts":flat.as_ref().ok().map(|f| f.parts.iter().filter(|p| included.contains(&p.component)).collect::<Vec<_>>())},
        "evidence":{"model_resolution":"Authoring snapshot, not proof of successful compilation, execution or physical calibration",
            "measurements":"No live values are inferred. Inspect viewer_status and REST /v1/description plus /v1/measurements; validate description identity/revision and current model before using samples.",
            "unknowns":"Absent physical properties, units, provenance, uncertainty or CAD references remain unknown. Display geometry cannot supply them."}
    }))
}

fn pointer(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_system::*;
    fn fixture() -> (SystemDocument, BehaviorRegistry) {
        let mut registry = BehaviorRegistry::default();
        sim_domain_electrical::elements::register(&mut registry).unwrap();
        let mut doc = SystemDocument::new("Inherited resistor");
        let mut child = Definition::new("Branch");
        child.parameters.insert(
            "r".into(),
            ParameterDecl {
                unit: "Ω".into(),
                default: Some(100.),
                description: "branch resistance".into(),
            },
        );
        let mut resistor = InstanceSpec::element("electrical.resistor");
        resistor.parameters.insert(
            "resistance".into(),
            ParameterBinding::Parameter {
                parameter: "r".into(),
            },
        );
        child.instances.insert("resistor".into(), resistor);
        child.ports.insert("in".into(), BoundaryPort::default());
        child.nets.push(Net {
            label: "input".into(),
            terminals: vec![Terminal::boundary("in"), Terminal::port("resistor", "p")],
        });
        doc.definitions.insert("branch".into(), child);
        let mut branch = InstanceSpec::subsystem("branch");
        branch.parameters.insert(
            "r".into(),
            ParameterBinding::Value {
                value: 220.,
                unit: Some("Ω".into()),
                provenance: Some(sim_inspect::Provenance::Estimated {
                    explanation: "bench estimate".into(),
                }),
                uncertainty: Some(2.),
            },
        );
        let root = doc.definitions.get_mut(&doc.root).unwrap();
        root.instances.insert("branch".into(), branch);
        root.instances.insert(
            "source".into(),
            InstanceSpec::element("electrical.voltage_source").with("voltage", 12.),
        );
        root.instances.insert(
            "other".into(),
            InstanceSpec::element("electrical.resistor").with("resistance", 330.),
        );
        root.instances.insert(
            "unrelated".into(),
            InstanceSpec::element("electrical.resistor").with("resistance", 999.),
        );
        root.nets.push(Net {
            label: "bus".into(),
            terminals: vec![
                Terminal::port("source", "p"),
                Terminal::port("branch", "in"),
                Terminal::port("other", "p"),
            ],
        });
        sim_system::display::assign_ids(&mut doc);
        (doc, registry)
    }
    #[test]
    fn follows_inheritance_and_cross_boundary_hyperedges_without_simulating() {
        let (doc, registry) = fixture();
        let before = doc.content_hash();
        let v = build(
            &doc,
            &registry,
            &Request {
                targets: vec!["branch".into()],
                ..Default::default()
            },
        )
        .unwrap();
        assert!(v["resolution_error"].is_null(), "{v}");
        assert_eq!(
            v["resolved_model"]["components"]["branch/resistor"]["parameters"]["resistance"]["value"],
            220.
        );
        assert_eq!(
            v["authored_instances"]["branch"]["instance"]["parameters"]["r"]["uncertainty"],
            2.
        );
        assert_eq!(
            v["authored_instances"]["branch"]["instance"]["parameters"]["r"]["provenance"]["explanation"],
            "bench estimate"
        );
        assert_eq!(
            v["scope"]["connected_neighbors"],
            json!(["other", "source"])
        );
        assert!(v["resolved_model"]["components"].get("unrelated").is_none());
        assert!(
            v["resolved_model"]["nets"]
                .as_object()
                .unwrap()
                .values()
                .any(|n| n["ports"].as_array().unwrap().len() == 3)
        );
        assert!(
            !v["component_knowledge"]["electrical.resistor"]["notes"]["equations"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(
            v["findings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["code"] == "unconnected_port")
        );
        assert_eq!(before, doc.content_hash());
    }
    #[test]
    fn reports_resolution_failure_and_rejects_unknown_paths() {
        let (mut doc, registry) = fixture();
        doc.definitions
            .get_mut("branch")
            .unwrap()
            .instances
            .get_mut("resistor")
            .unwrap()
            .parameters
            .insert(
                "resistance".into(),
                ParameterBinding::Parameter {
                    parameter: "missing".into(),
                },
            );
        let v = build(
            &doc,
            &registry,
            &Request {
                targets: vec!["branch/resistor".into()],
                ..Default::default()
            },
        )
        .unwrap();
        assert!(v["resolution_error"].as_str().unwrap().contains("missing"));
        assert!(v["resolved_model"].is_null());
        assert!(!v["authored_instances"]["branch/resistor"].is_null());
        assert!(
            build(
                &doc,
                &registry,
                &Request {
                    targets: vec!["nonexistent".into()],
                    ..Default::default()
                }
            )
            .is_err()
        );
    }
    #[test]
    fn deleted_annotation_target_is_not_rebound_to_reused_name() {
        let (mut doc, registry) = fixture();
        let target = sim_system::display::bind(&doc, "branch").unwrap();
        doc.discussions.threads.insert(
            "note".into(),
            sim_system::display::Thread {
                id: "note".into(),
                title: "Review".into(),
                resolved: false,
                targets: vec![target],
                comments: vec![],
                pin_m: None,
                view: None,
            },
        );
        doc.definitions
            .get_mut(&doc.root)
            .unwrap()
            .instances
            .get_mut("branch")
            .unwrap()
            .display_id = "replacement".into();
        let v = build(
            &doc,
            &registry,
            &Request {
                discussion: Some("note".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(v["scope"]["missing_targets"].as_array().unwrap().len(), 1);
        assert_eq!(v["scope"]["model_wide"], false);
        assert_eq!(v["scope"]["included_instances"], 0);
    }
}
