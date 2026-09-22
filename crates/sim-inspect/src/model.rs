//! Topology extraction from the shared model. No stepping, geometry mutation,
//! or name-based state lookup happens here. Authoring adapters retain their IDs.
use crate::*;
use sim_core::{
    BehaviorId, BehaviorRegistry, ModelWorld, PortId, PortSchema, definitions::builtins,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentIdentity {
    pub id: String,
    pub persistent: bool,
    pub source: Option<SourceReference>,
    pub cad: Option<CadReference>,
    pub group: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityBindings {
    #[serde(with = "identity_entries")]
    pub components: BTreeMap<BehaviorId, ComponentIdentity>,
    pub connections: BTreeMap<usize, String>,
    pub groups: BTreeMap<String, GroupDescription>,
}

/// Binding keys are local to this exact ModelWorld. Persist authoring IDs, not
/// these keys, and rebuild this map whenever the model is rebuilt.
#[derive(Debug)]
pub struct ModelInspection {
    pub description: SystemDescription,
    pub components: BTreeMap<BehaviorId, String>,
    pub ports: BTreeMap<PortId, String>,
    pub states: BTreeMap<String, sim_core::StateId>,
}

pub fn describe(
    model: &ModelWorld,
    registry: &BehaviorRegistry,
    source_hash: &str,
    model_revision: u64,
    identities: &IdentityBindings,
) -> Result<ModelInspection, InspectionError> {
    let definitions = registry
        .definitions()
        .map_err(|e| InspectionError(e.to_string()))?;
    // Validate standalone authored metadata as well as compiled models.
    for (_, entry) in model.state.iter() {
        entry
            .quantity
            .validate(&definitions)
            .map_err(|e| InspectionError(e.to_string()))?;
    }
    for (_, port) in &model.ports {
        if let PortSchema::SignalIn(quantity) | PortSchema::SignalOut(quantity) = &port.schema {
            quantity
                .validate(&definitions)
                .map_err(|e| InspectionError(e.to_string()))?;
        }
    }
    let mut description = SystemDescription {
        version: SCHEMA_VERSION,
        id: String::new(),
        source_hash: source_hash.into(),
        model_revision,
        definitions: definitions.catalog().clone(),
        components: BTreeMap::new(),
        ports: BTreeMap::new(),
        nets: BTreeMap::new(),
        groups: identities.groups.clone(),
        observables: BTreeMap::new(),
        diagnostics: Vec::new(),
    };
    let mut components = BTreeMap::new();
    let mut states = BTreeMap::new();
    let mut occurrences = BTreeMap::<(String, String), usize>::new();
    for (key, behavior) in &model.behaviors {
        let object = model
            .objects
            .get(behavior.object)
            .ok_or_else(|| InspectionError("behavior references missing object".into()))?;
        let identity = identities.components.get(&key);
        let occurrence = occurrences
            .entry((object.name.clone(), behavior.kind.0.clone()))
            .or_default();
        let id = identity.map(|i| i.id.clone()).unwrap_or_else(|| {
            let stable_input =
                serde_json::to_vec(&(source_hash, &object.name, &behavior.kind.0, *occurrence))
                    .unwrap();
            format!("source/{}", blake3::hash(&stable_input).to_hex())
        });
        *occurrence += 1;
        ensure(
            !description.components.contains_key(&id),
            "duplicate component authoring identity",
        )?;
        let descriptor = registry.get(&behavior.kind).ok();
        if descriptor.is_none() {
            description.diagnostics.push(Diagnostic {
                code: "unregistered_component".into(),
                message: format!("Component type {} is unavailable", behavior.kind.0),
                subject: Some(id.clone()),
            });
        }
        description.components.insert(
            id.clone(),
            ComponentDescription {
                id: id.clone(),
                label: object.name.clone(),
                component_type: behavior.kind.0.clone(),
                group: identity.and_then(|i| i.group.clone()),
                source: identity.and_then(|i| i.source.clone()),
                cad: identity.and_then(|i| i.cad.clone()),
                persistent_identity: identity.is_some_and(|i| i.persistent),
                parameters: behavior
                    .parameters
                    .iter()
                    .filter(|(name, _)| {
                        !descriptor
                            .and_then(|d| d.parameters.as_ref())
                            .is_some_and(|parameters| {
                                parameters
                                    .iter()
                                    .any(|p| &p.name == *name && p.implementation_reference)
                            })
                    })
                    .map(|(name, q)| {
                        let unit = descriptor
                            .and_then(|d| d.parameters.as_ref())
                            .and_then(|p| p.iter().find(|p| &p.name == name))
                            .map(|p| p.unit.clone());
                        (
                            name.clone(),
                            ParameterValue {
                                value: q.value_si,
                                unit,
                                provenance: Provenance::Unspecified,
                                uncertainty: None,
                            },
                        )
                    })
                    .collect(),
            },
        );
        for state in &behavior.state {
            let entry = model
                .state
                .entry(*state)
                .map_err(|e| InspectionError(e.to_string()))?;
            // The compiler's display label includes the object name. Bind the
            // state's local declaration name so a CAD rename preserves identity.
            let prefix = format!("{}.", object.name);
            let local_name = entry
                .declaration_name
                .as_deref()
                .unwrap_or_else(|| entry.name.strip_prefix(&prefix).unwrap_or(&entry.name));
            let oid = format!("{id}/state/{}", encoded(local_name));
            states.insert(oid.clone(), *state);
            ensure(
                !description.observables.contains_key(&oid),
                "duplicate component state name",
            )?;
            description.observables.insert(
                oid.clone(),
                ObservableDescriptor {
                    id: oid,
                    label: entry.name.clone(),
                    quantity: builtins::quantity_id(&entry.quantity),
                    location: ObservationLocation::State {
                        component: id.clone(),
                        state: local_name.into(),
                    },
                    sign_convention: None,
                    coordinate_frame: None,
                    availability: Availability::Available,
                },
            );
        }
        components.insert(key, id);
    }
    let mut ports = BTreeMap::new();
    for (key, port) in &model.ports {
        let component = components
            .get(&port.owner)
            .ok_or_else(|| InspectionError("port references missing behavior".into()))?;
        let id = format!("{component}/port/{}", encoded(&port.name));
        ensure(
            !description.ports.contains_key(&id),
            "duplicate port name on component",
        )?;
        let schema = match &port.schema {
            PortSchema::Acausal(kind) => PortKind::Physical {
                connector: kind.definition_id(),
            },
            // Legacy dimensionless wildcard behavior is explicitly identified
            // in the inspection schema during the typed-signal migration.
            PortSchema::SignalIn(kind) => PortKind::SignalInput {
                signal_type: signal_type(kind),
            },
            PortSchema::SignalOut(kind) => PortKind::SignalOutput {
                signal_type: signal_type(kind),
            },
        };
        description.ports.insert(
            id.clone(),
            PortDescription {
                id: id.clone(),
                component: component.clone(),
                name: port.name.clone(),
                schema,
                composite_parent: None,
            },
        );
        ports.insert(key, id.clone());
        if let PortSchema::Acausal(kind) = &port.schema {
            let layout = definitions
                .layout(
                    definitions
                        .connector_handle(&kind.definition_id())
                        .map_err(|e| InspectionError(e.to_string()))?,
                )
                .unwrap();
            for lane in &layout.lanes {
                for (variable, through) in std::iter::once((&lane.across, false))
                    .chain(lane.through.iter().map(|v| (v, true)))
                {
                    let side = if through { "through" } else { "across" };
                    let oid = format!("{id}/{side}/{}", encoded(&variable.name));
                    description.observables.insert(
                        oid.clone(),
                        ObservableDescriptor {
                            id: oid,
                            label: format!(
                                "{}.{}.{}",
                                object_label(model, port.owner),
                                port.name,
                                variable.name
                            ),
                            quantity: variable.quantity.clone(),
                            location: if through {
                                ObservationLocation::Through {
                                    port: id.clone(),
                                    lane: variable.name.clone(),
                                }
                            } else {
                                ObservationLocation::Across {
                                    port: id.clone(),
                                    lane: variable.name.clone(),
                                }
                            },
                            sign_convention: through.then(|| "Positive into this component".into()),
                            coordinate_frame: None,
                            availability: if through {
                                Availability::Unavailable {
                                    reason: "Accepted-state flow capture has not been implemented"
                                        .into(),
                                }
                            } else {
                                Availability::Available
                            },
                        },
                    );
                }
            }
        } else {
            let kind = match &port.schema {
                PortSchema::SignalIn(q) | PortSchema::SignalOut(q) => q,
                _ => unreachable!(),
            };
            let oid = format!("{id}/signal");
            description.observables.insert(
                oid.clone(),
                ObservableDescriptor {
                    id: oid,
                    label: format!("{}.{}", object_label(model, port.owner), port.name),
                    quantity: builtins::quantity_id(kind),
                    location: ObservationLocation::Signal { port: id },
                    sign_convention: None,
                    coordinate_frame: None,
                    availability: Availability::Available,
                },
            );
        }
    }
    for (key, port) in &model.ports {
        if let Some((parent, _)) = port.member_of {
            let parent = ports
                .get(&parent)
                .ok_or_else(|| InspectionError("missing composite parent".into()))?;
            description
                .ports
                .get_mut(&ports[&key])
                .unwrap()
                .composite_parent = Some(parent.clone());
        }
    }
    for (index, connection) in model.connections.iter().enumerate() {
        let mut terminals = connection
            .ports
            .iter()
            .map(|p| {
                ports
                    .get(p)
                    .cloned()
                    .ok_or_else(|| InspectionError("net references missing port".into()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        terminals.sort();
        let id = identities
            .connections
            .get(&index)
            .cloned()
            .unwrap_or_else(|| {
                format!(
                    "net/{}",
                    blake3::hash(&serde_json::to_vec(&terminals).unwrap()).to_hex()
                )
            });
        ensure(
            !description.nets.contains_key(&id),
            "duplicate connection identity",
        )?;
        description.nets.insert(
            id.clone(),
            NetDescription {
                id,
                ports: terminals,
            },
        );
    }
    for key in identities.components.keys() {
        ensure(
            model.behaviors.contains_key(*key),
            "identity mapping belongs to another model",
        )?;
    }
    for index in identities.connections.keys() {
        ensure(
            *index < model.connections.len(),
            "connection identity index is invalid",
        )?;
    }
    description.seal()?;
    Ok(ModelInspection {
        description,
        components,
        ports,
        states,
    })
}

fn encoded(s: &str) -> String {
    s.as_bytes().iter().map(|b| format!("{b:02x}")).collect()
}
fn signal_type(q: &sim_core::QuantityKind) -> SignalType {
    if *q == sim_core::QuantityKind::Dimensionless {
        SignalType::Any
    } else {
        SignalType::Quantity(builtins::quantity_id(q))
    }
}
fn object_label(model: &ModelWorld, behavior: BehaviorId) -> &str {
    &model.objects[model.behaviors[behavior].object].name
}

// Slotmap keys are meaningful only inside an exact captured ModelWorld. Serialize
// entries as pairs; the launch contract checks the captured model hash before use.
mod identity_entries {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        map: &BTreeMap<BehaviorId, ComponentIdentity>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        map.iter().collect::<Vec<_>>().serialize(s)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        d: D,
    ) -> Result<BTreeMap<BehaviorId, ComponentIdentity>, D::Error> {
        let pairs = Vec::<(BehaviorId, ComponentIdentity)>::deserialize(d)?;
        let len = pairs.len();
        let map: BTreeMap<_, _> = pairs.into_iter().collect();
        if len != map.len() {
            return Err(serde::de::Error::custom("duplicate identity key"));
        }
        Ok(map)
    }
}
