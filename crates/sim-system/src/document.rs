//! The versioned, hierarchical system document (`sim.system/1`).
//!
//! A document holds named subsystem *definitions*. Each definition declares
//! typed boundary ports and parameters, and contains *instances* (registry
//! elements or other definitions) joined by *nets*. Placing a definition many
//! times shares it, like a linked CAD component; editing its contents edits
//! every placement until it is made unique.
use serde::{Deserialize, Serialize};
use sim_core::PortSchema;
use sim_inspect::Provenance;
use sim_inspect::spatial::SpatialShape;
use std::collections::BTreeMap;

pub const SCHEMA: &str = "sim.system/1";
pub const LIBRARY_SCHEMA: &str = "sim.system-library/1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemDocument {
    /// Display-only discussions; never compiled into physics.
    #[serde(default, skip_serializing_if = "crate::display::Discussions::is_empty")]
    pub discussions: crate::display::Discussions,
    pub schema: String,
    pub title: String,
    /// Incremented by every applied edit, undo and redo.
    pub revision: u64,
    /// Definition placed at the top of the hierarchy.
    pub root: String,
    pub definitions: BTreeMap<String, Definition>,
    /// Content-addressed files next to the document (reference images).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub assets: BTreeMap<String, Asset>,
    /// Numerical settings this system is validated with, recorded so every
    /// runner (viewers, CLI, tests) reproduces the same run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<RunSettings>,
    /// Saved comparisons and sweeps, rerunnable from the file alone.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub studies: BTreeMap<String, Study>,
    /// The realtime (interactive/browser) profile and its measured error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub realtime: Option<RealtimeProfile>,
    /// Hosted instances: a root-definition element whose implementation
    /// comes from a file, keyed by its instance name (see [`FileLink`] and
    /// [`HOSTED_TYPES`]). Set with `Command::LinkFile`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub links: BTreeMap<String, FileLink>,
}

/// A file a hosted instance runs from. A hosted instance's implementation
/// parameters (the robot's model handle, the external seam's period, the
/// drive limiter's limits) are never typed into the document: the host reads
/// them from the linked file at run time, so measured and derived values have
/// one source. Hosted instances are left out of the flattened model
/// ([`crate::Flattened::hosted`] lists them) and run on the shared drive host
/// (`sim_runtime::system_robot`); their nets join only other hosted instances
/// and are kept as the authored wiring.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileLink {
    /// Relative to the system file's directory, `/`-separated; never absolute.
    pub path: String,
}

/// The component types an instance can be hosted as, with the file suffix
/// its link must have: the robot model, its controller binding and its
/// drive profile.
pub const HOSTED_TYPES: [(&str, &str); 3] = [("robot.articulated", ".simrobot.json"), ("control.external", ".controller.json"), ("control.drive_limiter", ".drive.json")];

/// The file suffix a hosted instance of `component_type` links to, or None
/// when that type cannot be hosted.
pub fn hosted_suffix(component_type: &str) -> Option<&'static str> {
    HOSTED_TYPES.iter().find(|(t, _)| *t == component_type).map(|(_, s)| *s)
}

/// How this system runs in realtime: every part's realtime model (notes
/// overrides, realtime counterpart definitions) at a coarser step, with the
/// error against the detailed model measured and published.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RealtimeProfile {
    pub interval: f64,
    pub integrator: IntegratorChoice,
    /// Observables the error is measured on.
    pub observe: Vec<String>,
    /// Simulated seconds of the comparison.
    pub duration: f64,
    /// Published bound: per observable, the largest error allowed relative
    /// to that observable's range in the detailed run.
    #[serde(default)]
    pub bound: f64,
    /// Per-observable bounds that differ from `bound`, with the reason in `notes`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bounds: BTreeMap<String, f64>,
    /// What the realtime models give up (published with the bound).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// Last measurement, with the content hash it was measured on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measured: Option<FidelityMeasurement>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FidelityMeasurement {
    pub content_hash: String,
    /// Largest error relative to range, per observable.
    pub errors: BTreeMap<String, f64>,
    /// Simulated seconds per wall second (native release build) for each model.
    pub detailed_speed: f64,
    pub realtime_speed: f64,
    pub host: String,
}

/// A comparison of alternatives or a parameter sweep over one instance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Study {
    /// Level holding the instance ("" = top).
    #[serde(default)]
    pub at: String,
    pub instance: String,
    pub kind: StudyKind,
    /// Simulated seconds per variant.
    pub duration: f64,
    /// Observables to record (readable keys or IDs; substring match).
    pub observe: Vec<String>,
    /// Metrics for the trade-off table.
    #[serde(default)]
    pub metrics: Vec<Metric>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StudyKind {
    /// Run once with the current implementation and once per alternative.
    Compare { alternatives: Vec<InstanceKind> },
    /// Run once per value of one parameter of the instance (or, for a
    /// subsystem, of an element inside it: `parameter = "mesh/worm_starts"`).
    Sweep { parameter: String, values: Vec<f64> },
}

/// A number computed from one recorded observable over a time window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metric {
    pub label: String,
    pub observable: String,
    pub reduce: Reduce,
    /// Window start and end, seconds (None = whole run).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<[f64; 2]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reduce {
    Final,
    Mean,
    Max,
    Min,
    /// Peak absolute value.
    Peak,
    /// Last value minus first value in the window.
    Change,
    /// Time integral (e.g. energy from power).
    Integral,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegratorChoice {
    ImplicitMidpoint,
    BackwardEuler,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunSettings {
    pub integrator: IntegratorChoice,
    /// Fixed step, seconds.
    pub interval: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub absolute_tolerance: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relative_tolerance: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_iterations: Option<usize>,
    /// Why these settings (e.g. switching edges need an L-stable method).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    /// Display-only workplane in this definition's local frame.
    #[serde(default, skip_serializing_if = "crate::display::Grid::is_default")]
    pub grid: crate::display::Grid,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub icon: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Implementation family. Definitions sharing an interface are offered as
    /// alternatives for each other when they also satisfy the port contract.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interface: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ports: BTreeMap<String, BoundaryPort>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub parameters: BTreeMap<String, ParameterDecl>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub instances: BTreeMap<String, InstanceSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nets: Vec<Net>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub references: BTreeMap<String, ReferenceImage>,
    /// Display shape when this definition is shown as one closed box.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<Appearance>,
    /// Library file this definition was imported from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<LibrarySource>,
    /// Published library version (incremented when the published contents change).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
    /// Definition with the same ports that is this one's realtime model
    /// (e.g. an averaged bridge for a switching one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub realtime: Option<String>,
}

impl Definition {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            grid: Default::default(), icon: String::new(),
            label: label.into(),
            description: String::new(),
            interface: None,
            ports: BTreeMap::new(),
            parameters: BTreeMap::new(),
            instances: BTreeMap::new(),
            nets: Vec::new(),
            references: BTreeMap::new(),
            appearance: None,
            source: None,
            version: None,
            realtime: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibrarySource {
    pub path: String,
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct BoundaryPort {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    /// Declared type. Required for a port nothing inside connects to yet;
    /// otherwise it must agree with the inner connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<PortSchema>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterDecl {
    pub unit: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<f64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstanceSpec {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub display_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    pub kind: InstanceKind,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub parameters: BTreeMap<String, ParameterBinding>,
    #[serde(default, skip_serializing_if = "Placement::is_default")]
    pub placement: Placement,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<Appearance>,
}

impl InstanceSpec {
    pub fn new(kind: InstanceKind) -> Self {
        Self { display_id: String::new(), label: String::new(), kind, parameters: BTreeMap::new(), placement: Placement::default(), appearance: None }
    }
    pub fn element(component_type: &str) -> Self {
        Self::new(InstanceKind::Element { component_type: component_type.into() })
    }
    pub fn subsystem(definition: &str) -> Self {
        Self::new(InstanceKind::Subsystem { definition: definition.into() })
    }
    pub fn with(mut self, parameter: &str, value: f64) -> Self {
        self.parameters.insert(parameter.into(), ParameterBinding::value(value));
        self
    }
    pub fn inherit(mut self, parameter: &str, from: &str) -> Self {
        self.parameters.insert(parameter.into(), ParameterBinding::Parameter { parameter: from.into() });
        self
    }
    pub fn at(mut self, position: [f32; 3]) -> Self {
        self.placement.position = position;
        self
    }
    pub fn labeled(mut self, label: &str) -> Self {
        self.label = label.into();
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InstanceKind {
    /// A registered behavior (resistor, motor unit, thermal capacitance, …).
    Element { component_type: String },
    /// Another definition in this document.
    Subsystem { definition: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParameterBinding {
    /// A value in the unit the component declares. `unit`, when given, must
    /// equal that declared unit; no silent conversion happens.
    Value {
        value: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unit: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provenance: Option<Provenance>,
        /// One standard deviation, in the same unit, when the value was fitted or measured.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        uncertainty: Option<f64>,
    },
    /// Use a parameter of the enclosing definition.
    Parameter { parameter: String },
}

impl ParameterBinding {
    pub fn value(value: f64) -> Self {
        Self::Value { value, unit: None, provenance: None, uncertainty: None }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    /// Meters, in the enclosing definition's frame (right-handed, +Y up).
    #[serde(default)]
    pub position: [f32; 3],
    #[serde(default = "identity_rotation")]
    pub rotation_xyzw: [f32; 4],
    /// Schematic position in diagram units, when the author placed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schematic: Option<[f32; 2]>,
}

fn identity_rotation() -> [f32; 4] {
    [0., 0., 0., 1.]
}

impl Default for Placement {
    fn default() -> Self {
        Self { position: [0.; 3], rotation_xyzw: identity_rotation(), schematic: None }
    }
}

impl Placement {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Appearance {
    pub shape: SpatialShape,
    pub color_srgb: [f32; 3],
    /// Display model ID from the model catalog, overriding the component
    /// type's default model (for example a finned heatsink on a thermal mass).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// One net: every terminal in it is joined. A net is a hyperedge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Net {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    pub terminals: Vec<Terminal>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Terminal {
    /// A port of an instance in this definition. Composite members use
    /// dotted names such as `plug.thermal`.
    Port { instance: String, port: String },
    /// One of this definition's own boundary ports.
    Boundary { boundary: String },
}

impl Terminal {
    pub fn port(instance: &str, port: &str) -> Self {
        Self::Port { instance: instance.into(), port: port.into() }
    }
    pub fn boundary(name: &str) -> Self {
        Self::Boundary { boundary: name.into() }
    }
    pub fn instance(&self) -> Option<&str> {
        match self {
            Self::Port { instance, .. } => Some(instance),
            Self::Boundary { .. } => None,
        }
    }
}

impl std::fmt::Display for Terminal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Port { instance, port } => write!(f, "{instance}.{port}"),
            Self::Boundary { boundary } => write!(f, "boundary {boundary}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceView {
    /// Placed on a plane in the physical assembly (meters).
    Spatial,
    /// Drawn behind the schematic (diagram units).
    Schematic,
}

/// A reference image. It is presentation only and never feeds the physics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceImage {
    pub label: String,
    /// Key into [`SystemDocument::assets`].
    pub asset: String,
    pub view: ReferenceView,
    /// Center of the image. Spatial: meters. Schematic: diagram units (z = 0).
    pub origin: [f32; 3],
    /// Plane normal and in-plane horizontal axis (spatial view only).
    #[serde(default = "default_normal")]
    pub normal: [f32; 3],
    #[serde(default = "default_x_axis")]
    pub x_axis: [f32; 3],
    /// Displayed width; height follows the image's aspect ratio.
    pub width: f32,
    pub opacity: f32,
    #[serde(default)]
    pub locked: bool,
    #[serde(default = "yes")]
    pub visible: bool,
}

fn default_normal() -> [f32; 3] {
    [0., 1., 0.]
}
fn default_x_axis() -> [f32; 3] {
    [1., 0., 0.]
}
fn yes() -> bool {
    true
}

impl ReferenceImage {
    pub fn height(&self, asset: &Asset) -> f32 {
        self.width * asset.height_px as f32 / asset.width_px.max(1) as f32
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    /// Relative to the document's directory.
    pub path: String,
    pub media_type: String,
    pub width_px: u32,
    pub height_px: u32,
    pub bytes: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub original_name: String,
}

impl SystemDocument {
    pub fn new(title: &str) -> Self {
        let mut definitions = BTreeMap::new();
        definitions.insert("root".to_string(), Definition::new(title));
        Self { discussions: Default::default(), schema: SCHEMA.into(), title: title.into(), revision: 0, root: "root".into(), definitions, assets: BTreeMap::new(), run: None, studies: BTreeMap::new(), realtime: None, links: BTreeMap::new() }
    }

    /// The file a root instance is hosted from, when it is linked.
    pub fn link(&self, instance: &str) -> Option<&FileLink> {
        self.links.get(instance)
    }

    /// Whether `instance` of definition `definition` is hosted: links name root instances only.
    pub fn hosted(&self, definition: &str, instance: &str) -> bool {
        definition == self.root && self.links.contains_key(instance)
    }

    /// Canonical content hash, independent of pretty printing and revision.
    pub fn content_hash(&self) -> String {
        let mut copy = self.clone();
        copy.revision = 0;
        let value = serde_json::to_value(&copy).expect("documents serialize");
        blake3::hash(&serde_json::to_vec(&value).expect("values serialize")).to_hex().to_string()
    }
}

/// An instance path from the root: `""` is the root definition itself,
/// `"driver/bridge"` is instance `bridge` inside instance `driver`.
pub fn split_path(path: &str) -> Vec<&str> {
    path.split('/').filter(|s| !s.is_empty()).collect()
}

pub fn join_path(parent: &str, name: &str) -> String {
    if parent.is_empty() { name.to_string() } else { format!("{parent}/{name}") }
}

/// Names of instances, ports, parameters and definitions.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        && !name.starts_with('-')
}

/// Definition IDs may be namespaced with dots (`drivers.h_bridge_mosfet`).
pub fn valid_definition_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.split('.').all(valid_name)
}
