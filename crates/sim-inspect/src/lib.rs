//! Graphics-independent inspection and recording contracts. Descriptions carry
//! physical identity; frames carry committed observations; diagram state owns
//! presentation only. Unknown values and provenance are never invented.
use serde::{Deserialize, Serialize};
use sim_core::definitions::{DefinitionCatalog, DefinitionId, DefinitionRegistry, SignalType};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub mod annotations;
pub mod model;
pub mod selection;
pub mod plot;
pub mod spatial;
pub mod live;
pub mod animation;
#[cfg(feature = "runtime")]
pub mod runtime;

pub const SAMPLE_FRAME_VERSION: u32 = 2;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceReference {
    pub artifact_hash: String,
    pub path: String,
    pub line: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CadReference {
    pub artifact_hash: String,
    pub body_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Provenance {
    Unspecified,
    Measured { source: SourceReference },
    Estimated { explanation: String },
    Derived { rule: String, inputs_hash: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterValue {
    pub value: f64,
    pub unit: Option<String>,
    pub provenance: Provenance,
    pub uncertainty: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentDescription {
    pub id: String,
    pub label: String,
    pub component_type: String,
    pub group: Option<String>,
    pub source: Option<SourceReference>,
    pub cad: Option<CadReference>,
    /// False for IDs reconstructed from source positions or labels. They are
    /// scoped to the captured source and can change when that source is edited.
    pub persistent_identity: bool,
    pub parameters: BTreeMap<String, ParameterValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PortKind {
    Unresolved { declared_type: String },
    Physical { connector: DefinitionId },
    SignalInput { signal_type: SignalType },
    SignalOutput { signal_type: SignalType },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortDescription {
    pub id: String,
    pub component: String,
    pub name: String,
    pub schema: PortKind,
    pub composite_parent: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetDescription {
    pub id: String,
    /// Includes all terminals; a net is a hyperedge, not fabricated pairs.
    pub ports: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupDescription {
    pub id: String,
    pub label: String,
    pub parent: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ObservationLocation {
    State {
        component: String,
        state: String,
    },
    Across {
        port: String,
        lane: String,
    },
    Through {
        port: String,
        lane: String,
    },
    Signal {
        port: String,
    },
    Diagnostic {
        component: Option<String>,
        name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Availability {
    Available,
    Unavailable { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservableDescriptor {
    pub id: String,
    pub label: String,
    pub quantity: DefinitionId,
    pub location: ObservationLocation,
    pub sign_convention: Option<String>,
    pub coordinate_frame: Option<String>,
    pub availability: Availability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Diagnostic {
    pub code: String,
    pub message: String,
    pub subject: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemDescription {
    pub version: u32,
    /// Hash of this immutable description, excluding this field itself.
    pub id: String,
    pub source_hash: String,
    pub model_revision: u64,
    pub definitions: DefinitionCatalog,
    pub components: BTreeMap<String, ComponentDescription>,
    pub ports: BTreeMap<String, PortDescription>,
    pub nets: BTreeMap<String, NetDescription>,
    pub groups: BTreeMap<String, GroupDescription>,
    pub observables: BTreeMap<String, ObservableDescriptor>,
    /// Authoring/compilation errors can be shown on a structurally valid graph.
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Error, PartialEq, Eq)]
#[error("inspection contract: {0}")]
pub struct InspectionError(pub String);

impl SystemDescription {
    pub fn seal(&mut self) -> Result<(), InspectionError> {
        self.validate_structure()?;
        self.id = self.content_hash()?;
        Ok(())
    }

    fn content_hash(&self) -> Result<String, InspectionError> {
        let mut copy = self.clone();
        copy.id.clear();
        Ok(
            blake3::hash(&serde_json::to_vec(&copy).map_err(|e| InspectionError(e.to_string()))?)
                .to_hex()
                .to_string(),
        )
    }

    pub fn validate(&self) -> Result<(), InspectionError> {
        self.validate_structure()?;
        ensure(
            self.id == self.content_hash()?,
            "description content hash mismatch",
        )
    }

    fn validate_structure(&self) -> Result<(), InspectionError> {
        ensure(
            self.version == SCHEMA_VERSION,
            "unsupported description version",
        )?;
        ensure(!self.source_hash.is_empty(), "missing source identity")?;
        let frozen = DefinitionRegistry::from_catalog(self.definitions.clone())
            .and_then(|r| r.freeze())
            .map_err(|e| InspectionError(e.to_string()))?;
        for (id, component) in &self.components {
            ensure(
                !id.is_empty() && id == &component.id,
                "component key/id mismatch",
            )?;
            if let Some(group) = &component.group {
                ensure(self.groups.contains_key(group), "missing component group")?;
            }
            for parameter in component.parameters.values() {
                ensure(parameter.value.is_finite(), "nonfinite parameter")?;
                ensure(
                    parameter
                        .uncertainty
                        .is_none_or(|v| v.is_finite() && v >= 0.),
                    "invalid uncertainty",
                )?;
            }
        }
        for (id, port) in &self.ports {
            ensure(!id.is_empty() && id == &port.id, "port key/id mismatch")?;
            ensure(
                self.components.contains_key(&port.component),
                "missing port owner",
            )?;
            let mut ancestor = port.composite_parent.as_ref();
            let mut visited = BTreeSet::from([id]);
            while let Some(parent) = ancestor {
                ensure(visited.insert(parent), "cyclic composite parent")?;
                let p = self.ports.get(parent).ok_or_else(|| InspectionError("invalid composite parent".into()))?;
                ensure(p.component == port.component, "invalid composite parent owner")?;
                ancestor = p.composite_parent.as_ref();
            }
            match &port.schema {
                PortKind::Unresolved { declared_type } => ensure(
                    !declared_type.is_empty(),
                    "unresolved port needs declared type",
                )?,
                PortKind::Physical { connector } => {
                    frozen
                        .connector_handle(connector)
                        .map_err(|e| InspectionError(e.to_string()))?;
                }
                PortKind::SignalInput { signal_type } | PortKind::SignalOutput { signal_type } => {
                    if let SignalType::Quantity(q) = signal_type {
                        frozen
                            .quantity_handle(q)
                            .map_err(|e| InspectionError(e.to_string()))?;
                    }
                }
            }
        }
        for (id, net) in &self.nets {
            ensure(!id.is_empty() && id == &net.id, "net key/id mismatch")?;
            ensure(!net.ports.is_empty(), "empty net")?;
            let mut seen = BTreeSet::new();
            for p in &net.ports {
                ensure(
                    self.ports.contains_key(p) && seen.insert(p),
                    "missing or repeated net terminal",
                )?;
            }
            // Physical incompatibility is a compiler diagnostic, not a reason
            // to make an invalid authored system impossible to inspect.
        }
        for (id, group) in &self.groups {
            ensure(!id.is_empty() && id == &group.id, "group key/id mismatch")?;
            let mut seen = BTreeSet::from([id]);
            let mut parent = group.parent.as_ref();
            while let Some(p) = parent {
                ensure(seen.insert(p), "cyclic groups")?;
                let g = self
                    .groups
                    .get(p)
                    .ok_or_else(|| InspectionError("missing parent group".into()))?;
                parent = g.parent.as_ref();
            }
        }
        for (id, observable) in &self.observables {
            ensure(
                !id.is_empty() && id == &observable.id,
                "observable key/id mismatch",
            )?;
            frozen
                .quantity_handle(&observable.quantity)
                .map_err(|e| InspectionError(e.to_string()))?;
            match &observable.location {
                ObservationLocation::State { component, .. }
                | ObservationLocation::Diagnostic {
                    component: Some(component),
                    ..
                } => ensure(
                    self.components.contains_key(component),
                    "missing observation owner",
                )?,
                ObservationLocation::Across { port, lane }
                | ObservationLocation::Through { port, lane } => {
                    let port = self
                        .ports
                        .get(port)
                        .ok_or_else(|| InspectionError("missing observation port".into()))?;
                    let PortKind::Physical { connector } = &port.schema else {
                        return Err(InspectionError(
                            "physical observation needs resolved physical port".into(),
                        ));
                    };
                    let layout = frozen
                        .layout(
                            frozen
                                .connector_handle(connector)
                                .map_err(|e| InspectionError(e.to_string()))?,
                        )
                        .unwrap();
                    let variable = match &observable.location {
                        ObservationLocation::Across { .. } => layout
                            .lanes
                            .iter()
                            .map(|l| &l.across)
                            .find(|v| &v.name == lane),
                        _ => layout
                            .lanes
                            .iter()
                            .filter_map(|l| l.through.as_ref())
                            .find(|v| &v.name == lane),
                    }
                    .ok_or_else(|| InspectionError("missing observed lane".into()))?;
                    ensure(
                        variable.quantity == observable.quantity,
                        "observed lane quantity mismatch",
                    )?;
                }
                ObservationLocation::Signal { port } => {
                    let port = self
                        .ports
                        .get(port)
                        .ok_or_else(|| InspectionError("missing observation port".into()))?;
                    match &port.schema {
                        PortKind::SignalInput { signal_type }
                        | PortKind::SignalOutput { signal_type } => {
                            if let SignalType::Quantity(q) = signal_type {
                                ensure(
                                    q == &observable.quantity,
                                    "observed signal quantity mismatch",
                                )?;
                            }
                        }
                        _ => {
                            return Err(InspectionError(
                                "signal observation needs signal port".into(),
                            ));
                        }
                    }
                }
                ObservationLocation::Diagnostic {
                    component: None, ..
                } => (),
            }
        }
        Ok(())
    }
}

fn ensure(condition: bool, message: &str) -> Result<(), InspectionError> {
    if condition {
        Ok(())
    } else {
        Err(InspectionError(message.into()))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "quality", rename_all = "snake_case", deny_unknown_fields)]
pub enum SampleValue {
    Committed { value: f64, sample_time: f64 },
    AcceptedStage { value: f64, sample_time: f64, step_start: f64, step_end: f64 },
    Unavailable { reason: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SampleFrame {
    pub version: u32,
    pub description_id: String,
    pub model_revision: u64,
    pub run_id: String,
    pub generation: u64,
    pub sequence: u64,
    pub step: u64,
    pub time: f64,
    pub values: BTreeMap<String, SampleValue>,
}

impl SampleFrame {
    pub fn validate(&self, description: &SystemDescription) -> Result<(), InspectionError> {
        ensure(self.version == 1 || self.version == SAMPLE_FRAME_VERSION, "unsupported frame version")?;
        ensure(
            self.description_id == description.id
                && self.model_revision == description.model_revision,
            "frame belongs to another description/revision",
        )?;
        ensure(
            !self.run_id.is_empty() && self.time.is_finite() && self.time >= 0.,
            "invalid frame identity/time",
        )?;
        for (id, sample) in &self.values {
            let observation = description
                .observables
                .get(id)
                .ok_or_else(|| InspectionError(format!("unknown observable {id}")))?;
            match sample {
                SampleValue::Committed { value, sample_time } => {
                    ensure(
                        observation.availability == Availability::Available,
                        "unavailable observable carries a value",
                    )?;
                    ensure(
                        value.is_finite()
                            && sample_time.is_finite()
                            && *sample_time >= 0.
                            && *sample_time <= self.time,
                        "invalid observation value/time",
                    )?;
                }
                SampleValue::AcceptedStage { value, sample_time, step_start, step_end } => {
                    ensure(self.version >= 2, "stage values require frame version 2")?;
                    ensure(observation.availability == Availability::Available, "unavailable observable carries a value")?;
                    ensure(value.is_finite() && sample_time.is_finite() && step_start.is_finite() && step_end.is_finite()
                        && *step_start >= 0. && step_start < step_end && step_start <= sample_time
                        && sample_time <= step_end && *step_end <= self.time, "invalid accepted-stage interval or value")?;
                }
                SampleValue::Unavailable { reason } => ensure(
                    !reason.trim().is_empty(),
                    "unavailable sample needs a reason",
                )?,
            }
        }
        Ok(())
    }
}

/// Live transport gate; recorded playback validates frames without this
/// monotonicity constraint, because moving the playback cursor is intentional.
#[derive(Debug)]
pub struct FrameGate {
    run_id: String,
    generation: u64,
    last: Option<(u64, u64, f64)>,
}
impl FrameGate {
    pub fn new(run_id: String, generation: u64) -> Self {
        Self {
            run_id,
            generation,
            last: None,
        }
    }
    pub fn accept(
        &mut self,
        description: &SystemDescription,
        frame: &SampleFrame,
    ) -> Result<(), InspectionError> {
        frame.validate(description)?;
        ensure(
            frame.run_id == self.run_id && frame.generation == self.generation,
            "stale worker generation or run",
        )?;
        if let Some((sequence, step, time)) = self.last {
            ensure(
                frame.sequence > sequence && frame.step >= step && frame.time >= time,
                "out-of-order live frame",
            )?;
        }
        self.last = Some((frame.sequence, frame.step, frame.time));
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}
impl Point {
    fn finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagramState {
    pub version: u32,
    pub description_id: String,
    pub revision: u64,
    pub positions: BTreeMap<String, Point>,
    pub pinned: BTreeSet<String>,
    pub collapsed: BTreeSet<String>,
    pub camera: Point,
    pub zoom: f32,
    pub plot_observables: BTreeSet<String>,
}
impl DiagramState {
    pub fn new(description: &SystemDescription) -> Self {
        Self {
            version: SCHEMA_VERSION,
            description_id: description.id.clone(),
            revision: 0,
            positions: BTreeMap::new(),
            pinned: BTreeSet::new(),
            collapsed: BTreeSet::new(),
            camera: Point { x: 0., y: 0. },
            zoom: 1.,
            plot_observables: BTreeSet::new(),
        }
    }
    pub fn validate(&self, description: &SystemDescription) -> Result<(), InspectionError> {
        ensure(
            self.version == SCHEMA_VERSION && self.description_id == description.id,
            "layout belongs to another description/version",
        )?;
        ensure(
            self.camera.finite() && self.zoom.is_finite() && self.zoom > 0.,
            "invalid camera",
        )?;
        for (id, point) in &self.positions {
            ensure(
                description.components.contains_key(id) && point.finite(),
                "invalid node position",
            )?;
        }
        for id in &self.pinned {
            ensure(
                self.positions.contains_key(id),
                "pinned node has no position",
            )?;
        }
        for id in &self.collapsed {
            ensure(
                description.groups.contains_key(id),
                "unknown collapsed group",
            )?;
        }
        for id in &self.plot_observables {
            ensure(
                description.observables.contains_key(id),
                "unknown plot observable",
            )?;
        }
        Ok(())
    }
}
