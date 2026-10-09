//! Typed revision-guarded source composition operations; blocking job calls.
use serde::{Deserialize, Serialize};
use serde_json::Value;
pub use sim_system::composition::{CadComponent, CadConnection, CadEndpoint, CadGraph};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphSnapshot {
    pub revision: u64,
    #[serde(default)]
    pub document_id: Option<String>,
    pub graph: CadGraph,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphEdit {
    pub revision: u64,
    #[serde(default)]
    pub document_id: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    pub graph: CadGraph,
}
pub use sim_system::composition::{ImportedComponent, Recipe, SystemType};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImportedSnapshot {
    pub run_id: String,
    pub revision: Option<u64>,
    pub state: String,
    pub error: Option<String>,
    pub stale: bool,
    #[serde(default)]
    pub metadata_stale: bool,
    pub imported: Vec<ImportedComponent>,
    pub resolved: Option<Value>,
    #[serde(default)]
    pub guard_document_id: Option<String>,
    #[serde(default)]
    pub guard_revision: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum GraphCommand {
    Add { component: CadComponent },
    Update { id: String, component: CadComponent },
    Remove { id: String },
    Connect { ports: Vec<CadEndpoint> },
    Open { id: String },
    Replace { graph: CadGraph },
}

