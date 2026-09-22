//! Numeric readout of retained, accepted solver contributions.
use crate::Runtime;
use sim_core::{PortId, PortSchema, StateId};

/// A through contribution evaluated in an accepted implicit step. For midpoint,
/// this is the solver's stage (differential midpoint with solved algebraic values),
/// not an instantaneous endpoint flow. The stochastic draw is the solved draw.
#[derive(Debug, Clone, PartialEq)]
pub struct FlowObservation {
    pub value: f64,
    pub evaluation_time: f64,
    pub step_start: f64,
    pub step_end: f64,
    /// Maximum absolute unscaled residual in the captured full system. Rows
    /// may have different units; this is diagnostic, not a universal tolerance.
    pub residual_max: f64,
}
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ObservationError {
    #[error("observation binding belongs to a different runtime")]
    ForeignBinding,
    #[error("unknown physical port or through lane")]
    UnknownLane,
    #[error(
        "no matching accepted solver evaluation is retained; capture must be enabled before stepping, and this integrator/event state must be supported"
    )]
    Unavailable,
}
/// Validated numeric binding, local to one runtime instance. Metadata is resolved
/// once; sampling performs no definition lookup or string matching.
#[derive(Clone, Debug)]
pub struct FlowBinding {
    identity: std::sync::Arc<()>,
    island: usize,
    slot: usize,
    lane: usize,
    quantity: sim_core::definitions::DefinitionId,
    unit: String,
}
impl FlowBinding {
    pub fn quantity(&self) -> &sim_core::definitions::DefinitionId {
        &self.quantity
    }
    pub fn unit(&self) -> &str {
        &self.unit
    }
}
impl Runtime {
    pub fn set_observation_capture(&self, enabled: bool) {
        for island in &self.islands {
            island.system.set_observation_capture(enabled);
        }
    }
    pub fn bind_through(&self, port: PortId, lane: usize) -> Result<FlowBinding, ObservationError> {
        let Some(p) = self.model.ports.get(port) else {
            return Err(ObservationError::UnknownLane);
        };
        let PortSchema::Acausal(kind) = &p.schema else {
            return Err(ObservationError::UnknownLane);
        };
        let layout = kind
            .resolve(&self.definitions)
            .map_err(|_| ObservationError::UnknownLane)?;
        let variable = layout
            .lanes
            .get(lane)
            .and_then(|lane| lane.through.as_ref())
            .ok_or(ObservationError::UnknownLane)?;
        let quantity = variable.quantity.clone();
        let unit = self
            .definitions
            .quantity(
                self.definitions
                    .quantity_handle(&quantity)
                    .map_err(|_| ObservationError::UnknownLane)?,
            )
            .unwrap()
            .canonical_unit
            .clone();
        self.islands
            .iter()
            .enumerate()
            .find_map(|(island, simulation)| {
                simulation
                    .system
                    .flow_binding(port, lane)
                    .map(|(slot, lane)| FlowBinding {
                        identity: self.observation_identity.clone(),
                        island,
                        slot,
                        lane,
                        quantity: quantity.clone(),
                        unit: unit.clone(),
                    })
            })
            .ok_or(ObservationError::UnknownLane)
    }
    pub fn read_flow(&self, binding: &FlowBinding) -> Result<FlowObservation, ObservationError> {
        if !std::sync::Arc::ptr_eq(&binding.identity, &self.observation_identity) {
            return Err(ObservationError::ForeignBinding);
        }
        let island = self
            .islands
            .get(binding.island)
            .ok_or(ObservationError::ForeignBinding)?;
        if self.observation_committed_times.get(binding.island) != Some(&island.time) {
            return Err(ObservationError::Unavailable);
        }
        island
            .system
            .captured_flow(island.time, &island.state, binding.slot, binding.lane)
            .ok_or(ObservationError::Unavailable)
    }
    /// Convenience lookup. Continuous subscribers should retain `bind_through`.
    pub fn observe_through(
        &self,
        port: PortId,
        lane: usize,
    ) -> Result<FlowObservation, ObservationError> {
        self.read_flow(&self.bind_through(port, lane)?)
    }
}

/// Endpoint values are retained differential states. Algebraic and derived
/// values come from a matched accepted stage, with that stage's actual time.
#[derive(Debug, Clone, PartialEq)]
pub enum ValueObservation {
    Endpoint {
        value: f64,
        sample_time: f64,
    },
    AcceptedStage {
        value: f64,
        sample_time: f64,
        step_start: f64,
        step_end: f64,
    },
}
#[derive(Debug, Clone)]
pub struct ValueBinding {
    identity: std::sync::Arc<()>,
    island: usize,
    index: usize,
    state: StateId,
    endpoint: bool,
    quantity: sim_core::definitions::DefinitionId,
    unit: String,
}
impl ValueBinding {
    pub fn quantity(&self) -> &sim_core::definitions::DefinitionId {
        &self.quantity
    }
    pub fn unit(&self) -> &str {
        &self.unit
    }
}
impl Runtime {
    pub fn bind_value(&self, state: StateId) -> Result<ValueBinding, ObservationError> {
        let entry = self
            .model
            .state
            .entry(state)
            .map_err(|_| ObservationError::UnknownLane)?;
        self.islands
            .iter()
            .enumerate()
            .find_map(|(island, simulation)| {
                simulation
                    .system
                    .state_ids
                    .iter()
                    .position(|id| *id == state)
                    .map(|index| ValueBinding {
                        identity: self.observation_identity.clone(),
                        island,
                        index,
                        state,
                        endpoint: simulation.system.endpoint_value(index),
                        quantity: entry.quantity.definition_id(),
                        unit: entry.quantity.unit().into(),
                    })
            })
            .ok_or(ObservationError::UnknownLane)
    }
    pub fn bind_across(&self, port: PortId, lane: usize) -> Result<ValueBinding, ObservationError> {
        let id = self
            .islands
            .iter()
            .find_map(|island| {
                island
                    .system
                    .port_lanes
                    .get(&port)
                    .and_then(|lanes| lanes.get(lane))
                    .and_then(|index| island.system.state_ids.get(*index))
            })
            .ok_or(ObservationError::UnknownLane)?;
        self.bind_value(*id)
    }
    pub fn bind_signal(&self, port: PortId) -> Result<ValueBinding, ObservationError> {
        let id = self
            .islands
            .iter()
            .find_map(|island| {
                island
                    .system
                    .port_signal
                    .get(&port)
                    .and_then(|index| island.system.state_ids.get(*index))
            })
            .ok_or(ObservationError::UnknownLane)?;
        self.bind_value(*id)
    }
    pub fn read_value(&self, binding: &ValueBinding) -> Result<ValueObservation, ObservationError> {
        if !std::sync::Arc::ptr_eq(&binding.identity, &self.observation_identity) {
            return Err(ObservationError::ForeignBinding);
        }
        let island = self
            .islands
            .get(binding.island)
            .ok_or(ObservationError::ForeignBinding)?;
        let time = *self
            .observation_committed_times
            .get(binding.island)
            .ok_or(ObservationError::Unavailable)?;
        if binding.endpoint {
            let value = self
                .model
                .state
                .get(binding.state)
                .map_err(|_| ObservationError::UnknownLane)?;
            if !value.is_finite() {
                return Err(ObservationError::Unavailable);
            }
            Ok(ValueObservation::Endpoint {
                value,
                sample_time: time,
            })
        } else if time == island.time {
            island
                .system
                .captured_value(time, &island.state, binding.index)
                .ok_or(ObservationError::Unavailable)
        } else {
            Err(ObservationError::Unavailable)
        }
    }
}
