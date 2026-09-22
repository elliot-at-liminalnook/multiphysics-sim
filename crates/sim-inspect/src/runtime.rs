//! Bind inspection identities once to a compiled runtime; sample without equations.
use crate::model::{IdentityBindings, ModelInspection};
use crate::*;
use sim_compile::{
    Runtime,
    observation::{FlowBinding, ObservationError, ValueBinding, ValueObservation},
};
use sim_core::{BehaviorRegistry, PortId};
use std::sync::Arc;

#[derive(Clone)]
enum Binding {
    Value(ValueBinding),
    Flow(FlowBinding),
    Unavailable(String),
}
/// Runtime capabilities are sealed once, independently of which values a viewer
/// subscribes to. Static CAD descriptions remain unchanged.
pub struct RuntimeInspection {
    pub description: Arc<SystemDescription>,
    bindings: BTreeMap<String, Binding>,
}
impl RuntimeInspection {
    pub fn new(
        runtime: &Runtime,
        registry: &BehaviorRegistry,
        source_hash: &str,
        revision: u64,
        identities: &IdentityBindings,
    ) -> Result<Self, InspectionError> {
        ensure(
            registry
                .frozen_definitions()
                .map_err(|e| InspectionError(e.to_string()))?
                .fingerprint()
                == runtime.definitions.fingerprint(),
            "registry differs from compiled runtime",
        )?;
        let inspection =
            model::describe(&runtime.model, registry, source_hash, revision, identities)?;
        Self::bind(runtime, inspection)
    }
    fn bind(runtime: &Runtime, inspection: ModelInspection) -> Result<Self, InspectionError> {
        let ports: BTreeMap<_, _> = inspection
            .ports
            .iter()
            .map(|(id, name)| (name.as_str(), *id))
            .collect();
        let mut description = inspection.description;
        let mut bindings = BTreeMap::new();
        for (id, observable) in &mut description.observables {
            let binding = (|| -> Result<Binding, String> {
                let port = |name: &str| -> Result<PortId, String> {
                    ports
                        .get(name)
                        .copied()
                        .ok_or("missing runtime port".into())
                };
                let lane = |pid, name: &str, through: bool| -> Result<usize, String> {
                    let sim_core::PortSchema::Acausal(kind) = &runtime.model.ports[pid].schema
                    else {
                        return Err("not a physical port".into());
                    };
                    kind.resolve(&runtime.definitions)
                        .map_err(|e| e.to_string())?
                        .lanes
                        .iter()
                        .position(|l| {
                            if through {
                                l.through.as_ref().is_some_and(|v| v.name == name)
                            } else {
                                l.across.name == name
                            }
                        })
                        .ok_or("missing runtime lane".into())
                };
                match &observable.location {
                    ObservationLocation::State { .. } => runtime
                        .bind_value(*inspection.states.get(id).ok_or("missing declared state")?)
                        .map(Binding::Value)
                        .map_err(|e| e.to_string()),
                    ObservationLocation::Across {
                        port: name,
                        lane: name_lane,
                    } => {
                        let pid = port(name)?;
                        runtime
                            .bind_across(pid, lane(pid, name_lane, false)?)
                            .map(Binding::Value)
                            .map_err(|e| e.to_string())
                    }
                    ObservationLocation::Through {
                        port: name,
                        lane: name_lane,
                    } => {
                        let pid = port(name)?;
                        runtime
                            .bind_through(pid, lane(pid, name_lane, true)?)
                            .map(Binding::Flow)
                            .map_err(|e| e.to_string())
                    }
                    ObservationLocation::Signal { port: name } => runtime
                        .bind_signal(port(name)?)
                        .map(Binding::Value)
                        .map_err(|e| e.to_string()),
                    ObservationLocation::Diagnostic { .. } => {
                        Err("Optional diagnostic has no runtime provider".into())
                    }
                }
            })();
            let binding = match binding {
                Ok(binding) => {
                    let quantity = match &binding {
                        Binding::Value(b) => b.quantity(),
                        Binding::Flow(b) => b.quantity(),
                        _ => unreachable!(),
                    };
                    let wildcard = match &observable.location {
                        ObservationLocation::Signal { port } => matches!(
                            &description.ports[port].schema,
                            PortKind::SignalInput {
                                signal_type: SignalType::Any
                            } | PortKind::SignalOutput {
                                signal_type: SignalType::Any
                            }
                        ),
                        _ => false,
                    };
                    if wildcard {
                        observable.quantity = quantity.clone();
                    }
                    if quantity != &observable.quantity {
                        Binding::Unavailable(
                            "Runtime quantity differs from inspection metadata".into(),
                        )
                    } else {
                        binding
                    }
                }
                Err(reason) => Binding::Unavailable(reason),
            };
            observable.availability = match &binding {
                Binding::Unavailable(reason) => Availability::Unavailable {
                    reason: reason.clone(),
                },
                _ => Availability::Available,
            };
            bindings.insert(id.clone(), binding);
        }
        description.seal()?;
        Ok(Self {
            description: Arc::new(description),
            bindings,
        })
    }
    pub fn subscribe<'a>(
        &self,
        ids: impl IntoIterator<Item = &'a str>,
    ) -> Result<Subscription, InspectionError> {
        let mut selected = BTreeMap::new();
        for id in ids {
            let binding = self
                .bindings
                .get(id)
                .ok_or_else(|| InspectionError(format!("unknown observable {id}")))?;
            selected.insert(id.to_owned(), binding.clone());
        }
        Ok(Subscription {
            description: self.description.clone(),
            bindings: selected,
        })
    }
}
pub struct Subscription {
    description: Arc<SystemDescription>,
    bindings: BTreeMap<String, Binding>,
}
/// The session owns run/generation/sequence and fixed simulation step identity.
pub struct FrameStamp<'a> {
    pub run_id: &'a str,
    pub generation: u64,
    pub sequence: u64,
    pub step: u64,
}
impl Subscription {
    pub fn len(&self) -> usize {
        self.bindings.len()
    }
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }
    pub fn sample(
        &self,
        runtime: &Runtime,
        stamp: FrameStamp<'_>,
    ) -> Result<SampleFrame, InspectionError> {
        let mut values = BTreeMap::new();
        for (id, binding) in &self.bindings {
            let value = match binding {
                Binding::Unavailable(reason) => SampleValue::Unavailable {
                    reason: reason.clone(),
                },
                _ => {
                    let observed =
                        match binding {
                            Binding::Value(binding) => runtime.read_value(binding),
                            Binding::Flow(binding) => runtime.read_flow(binding).map(|v| {
                                ValueObservation::AcceptedStage {
                                    value: v.value,
                                    sample_time: v.evaluation_time,
                                    step_start: v.step_start,
                                    step_end: v.step_end,
                                }
                            }),
                            _ => unreachable!(),
                        };
                    match observed {
                        Ok(ValueObservation::Endpoint { value, sample_time }) => {
                            SampleValue::Committed { value, sample_time }
                        }
                        Ok(ValueObservation::AcceptedStage {
                            value,
                            sample_time,
                            step_start,
                            step_end,
                        }) => SampleValue::AcceptedStage {
                            value,
                            sample_time,
                            step_start,
                            step_end,
                        },
                        Err(ObservationError::ForeignBinding) => {
                            return Err(InspectionError(
                                "subscription belongs to another runtime".into(),
                            ));
                        }
                        Err(error) => SampleValue::Unavailable {
                            reason: error.to_string(),
                        },
                    }
                }
            };
            values.insert(id.clone(), value);
        }
        let frame = SampleFrame {
            version: SAMPLE_FRAME_VERSION,
            description_id: self.description.id.clone(),
            model_revision: self.description.model_revision,
            run_id: stamp.run_id.into(),
            generation: stamp.generation,
            sequence: stamp.sequence,
            step: stamp.step,
            time: runtime.time,
            values,
        };
        frame.validate(&self.description)?;
        Ok(frame)
    }
}
