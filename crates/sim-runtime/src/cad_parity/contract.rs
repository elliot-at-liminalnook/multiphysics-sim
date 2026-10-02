//! Version one of the graphics-independent CAD parity wire contract.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
pub const SCHEMA_VERSION: u32 = 1;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SourceFile {
    pub path: String,
    pub sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub id: String,
    pub source: SourceFile,
    pub dependencies: Vec<SourceFile>,
    pub model_schema: String,
    pub document_id: String,
    pub units: String,
    pub frame: String,
    pub provenance: String,
    pub coverage: Vec<String>,
    pub unavailable: Vec<String>,
    pub scenarios: Vec<Scenario>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub id: String,
    pub operations: Vec<Step>,
    pub comparisons: Vec<ComparisonPolicy>,
    pub required_coverage: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub id: String,
    pub operation: Operation,
    pub expected: ExpectedOutcome,
}
/// Inputs are commands, never canned responses. IDs are durable CAD IDs.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Observe,
    Rename {
        node: String,
        name: String,
    },
    Undo,
    Redo,
    Op {
        name: String,
        args: Vec<Value>,
        kwargs: BTreeMap<String, Value>,
    },
    ConfigureRobot {
        revision_offset: i64,
        updates: Value,
    },
    Component {
        operation: Value,
        revision_offset: i64,
        cancel: bool,
        interfere: Option<String>,
    },
    Pose {
        positions: BTreeMap<String, f64>,
        time: f64,
        continuation: bool,
        revision_offset: i64,
    },
    Captured {
        capture: String,
        action: String,
        args: Value,
    },
    Physical {
        flex: bool,
        planar: bool,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ExpectedOutcome {
    Success,
    Rejected,
    Cancelled,
    Unsupported,
    DeliberateDifference,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    Passed,
    Failed,
    Unsupported,
    DeliberateDifference,
    Incomplete,
    Uncertain,
    Cancelled,
    NotRun,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AdapterIdentity {
    pub name: String,
    pub version: String,
    pub implementation: String,
    pub kernel: String,
    pub derivation: String,
    pub independent: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub owner: String,
    pub unit: Option<String>,
    pub frame: Option<String>,
    pub provenance: Option<String>,
    pub uncertainty: Option<Value>,
    pub value: ObservedValue,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(
    tag = "state",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ObservedValue {
    Present(Value),
    Missing(String),
    Unsupported(String),
    Invalid(String),
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub step_id: String,
    pub status: ExecutionStatus,
    pub expected: ExpectedOutcome,
    pub message: String,
    pub process_document_id: Option<String>,
    pub revision: Option<u64>,
    pub observations: BTreeMap<String, Observation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executed_at: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AdapterRun {
    pub identity: AdapterIdentity,
    pub source: SourceFile,
    pub receipts: Vec<Receipt>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Tolerance {
    pub absolute: f64,
    pub relative: f64,
    pub justification: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Metric {
    Exact,
    Numeric {
        tolerance: Tolerance,
    },
    PointSet {
        tolerance: Tolerance,
        max_points: usize,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ObservationOwners {
    pub reference: String,
    pub native: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ComparisonPolicy {
    /// Future independent adapters retain their own numerical owners. An explicit
    /// pair authorizes comparison without renaming either observation authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owners: Option<ObservationOwners>,
    pub step_id: String,
    pub path: String,
    pub family: String,
    pub unit: Option<String>,
    pub frame: Option<String>,
    pub require_provenance: bool,
    pub metric: Metric,
    pub deliberate_difference: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Diagnostic {
    pub step_id: String,
    pub path: String,
    pub family: String,
    pub status: ExecutionStatus,
    pub message: String,
    pub unit: Option<String>,
    pub absolute_error: Option<f64>,
    pub relative_error: Option<f64>,
    pub tolerance: Option<Tolerance>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum GateKind {
    ServiceContract,
    IndependentDerivation,
    IndependentKernel,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Gate {
    pub kind: GateKind,
    pub passed: bool,
    pub reasons: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Report {
    pub schema_version: u32,
    pub code_identity: String,
    pub manifest: Manifest,
    pub scenario_id: String,
    pub reference: AdapterRun,
    pub native: AdapterRun,
    pub diagnostics: Vec<Diagnostic>,
    pub gates: Vec<Gate>,
}
/// Synchronous headless adapters; callers own scheduling. Cancellation is durable
/// state, inspected between bounded network/process waits; no feature threads.
pub trait Adapter {
    fn identity(&self) -> AdapterIdentity;
    fn run(
        &mut self,
        manifest: &Manifest,
        scenario: &Scenario,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<AdapterRun, String>;
}
