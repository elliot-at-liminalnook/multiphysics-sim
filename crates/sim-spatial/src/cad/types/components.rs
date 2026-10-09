//! Reusable CAD definitions and service-owned preparation. All calls block;
//! the viewer runs them through `jobs`, never on its frame thread.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ComponentStamp {
    pub document_id: String,
    pub expected_revision: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ComponentParameter {
    pub value: Value,
    pub unit: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<Value>,
    pub provenance: String,
    pub description: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ComponentPort {
    pub kind: String,
    pub label: String,
    pub source_id: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ComponentVariant {
    pub definition_id: String,
    pub parameter_bindings: BTreeMap<String, Value>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ComponentNested {
    pub definition_id: String,
    pub parameter_bindings: BTreeMap<String, Value>,
    pub overrides: BTreeMap<String, Value>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
/// `ComponentDefinition.descriptor`, not an invented archive manifest.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ComponentDefinition {
    pub id: String,
    pub name: String,
    pub revision: u64,
    pub description: String,
    pub parameters: BTreeMap<String, ComponentParameter>,
    pub features: Vec<Value>,
    pub ports: BTreeMap<String, ComponentPort>,
    pub variants: BTreeMap<String, ComponentVariant>,
    pub default_variant: Option<String>,
    pub nested: BTreeMap<String, ComponentNested>,
    pub provenance: Value,
    pub frame: Value,
    pub node_count: usize,
    pub dependencies: Vec<String>,
    pub targets: Vec<ComponentTarget>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ComponentTarget {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub component_member: Option<Value>,
    pub component_instance: Option<Value>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ComponentCatalogue {
    pub version: u64,
    pub features: Value,
    pub definitions: Vec<ComponentDefinition>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ComponentRecipes {
    pub recipes: BTreeMap<String, Value>,
    pub features: Value,
    pub units: Vec<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ComponentLibrary {
    pub path: String,
    pub files: Vec<ComponentLibraryFile>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ComponentLibraryFile {
    pub path: String,
    pub name: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentJobState {
    Pending,
    Running,
    Ready,
    Applied,
    Failed,
    Cancelled,
}
impl ComponentJobState {
    pub fn terminal(&self) -> bool {
        matches!(self, Self::Applied | Self::Failed | Self::Cancelled)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ComponentJobStatus {
    pub id: String,
    #[serde(default)]
    pub operation: String,
    pub state: ComponentJobState,
    #[serde(default)]
    pub stage: String,
    #[serde(default)]
    pub done: u64,
    #[serde(default)]
    pub total: u64,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub log_path: Option<String>,
    #[serde(default)]
    pub result: Value,
    pub document_id: String,
    pub revision: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ComponentStarted {
    pub job: ComponentJobStatus,
}

/// Named authoritative Ops, with explicit typed operation boundaries. Recipe
/// expressions stay values because the Python dimensional grammar owns them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ComponentOperation {
    Make {
        ids: Vec<String>,
        name: String,
        origin: [f64; 3],
    },
    Create {
        ids: Vec<String>,
        name: String,
        origin: [f64; 3],
    },
    Parametric {
        name: String,
        shape: String,
    },
    Place {
        definition_id: String,
        placement: Value,
        overrides: BTreeMap<String, Value>,
        bindings: BTreeMap<String, Option<String>>,
        name: String,
        variant: Option<String>,
    },
    Defaults {
        definition_id: String,
        parameters: BTreeMap<String, ComponentParameter>,
        features: Vec<Value>,
        nested: BTreeMap<String, ComponentNested>,
        family_variants: Option<BTreeMap<String, ComponentVariant>>,
    },
    Overrides {
        instance_id: String,
        overrides: BTreeMap<String, Value>,
        placement: Option<Value>,
    },
    Detach {
        instance_id: String,
    },
    Import {
        path: String,
    },
    Export {
        definition_id: String,
        path: String,
    },
    Family {
        name: String,
        variants: BTreeMap<String, ComponentVariant>,
        parameters: BTreeMap<String, ComponentParameter>,
        default_variant: Option<String>,
    },
    LinkFamily {
        instance_id: String,
        definition_id: String,
        variant: String,
        overrides: BTreeMap<String, Value>,
    },
    Transform {
        ids: Vec<String>,
        translation: [f64; 3],
        axis: [f64; 3],
        angle_deg: f64,
        center: Option<[f64; 3]>,
        scale: f64,
    },
}
impl ComponentOperation {
    pub fn op_name(&self) -> &'static str {
        match self {
            Self::Make { .. } => "make_component",
            Self::Create { .. } => "create_component",
            Self::Parametric { .. } => "new_parametric_component",
            Self::Place { .. } => "place_component",
            Self::Defaults { .. } => "set_component_parameters",
            Self::Overrides { .. } => "set_component_overrides",
            Self::Detach { .. } => "detach_component",
            Self::Import { .. } => "import_component",
            Self::Export { .. } => "export_component",
            Self::Family { .. } => "create_component_family",
            Self::LinkFamily { .. } => "link_component_family",
            Self::Transform { .. } => "transform_components",
        }
    }
    pub fn kwargs(&self) -> Value {
        let mut value = serde_json::to_value(self).expect("component operation is JSON");
        value
            .as_object_mut()
            .expect("tagged operation")
            .remove("operation");
        // The catalogue's nested identity is read-only. Ops accepts only
        // bindings and defaults here, and rejects a descriptor copied whole.
        if let Self::Defaults { nested, .. } = self {
            value["nested"] = json!(
                nested
                    .iter()
                    .map(|(id, n)| (
                        id.clone(),
                        json!({"parameter_bindings":n.parameter_bindings,"overrides":n.overrides})
                    ))
                    .collect::<BTreeMap<_, _>>()
            );
        }
        value
    }
}
