//! Discoverable, deterministic library operations. These are explicit calls,
//! not acausal equation elements or a second simulation runtime.
use crate::{BehaviorRegistry, QuantityKind, definitions::DefinitionId};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    /// Dotted request/result path; [] denotes each array entry.
    pub path: String,
    pub quantity: Option<QuantityKind>,
    pub unit: String,
    pub shape: String,
}
impl Field {
    pub fn quantity(path: &str, quantity: QuantityKind, shape: &str) -> Self {
        Self {
            path: path.into(),
            unit: quantity.unit().into(),
            quantity: Some(quantity),
            shape: shape.into(),
        }
    }
    /// Structured or mixed-coordinate data; units must be explained explicitly.
    pub fn structured(path: &str, unit: &str, shape: &str) -> Self {
        Self {
            path: path.into(),
            quantity: None,
            unit: unit.into(),
            shape: shape.into(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Descriptor {
    pub id: DefinitionId,
    pub description: String,
    pub inputs: Vec<Field>,
    pub outputs: Vec<Field>,
    pub assumptions: Vec<String>,
}
impl Descriptor {
    pub fn new(
        name: &str,
        description: &str,
        inputs: Vec<Field>,
        outputs: Vec<Field>,
        assumptions: &[&str],
    ) -> Self {
        Self {
            id: DefinitionId::new(name, 1),
            description: description.into(),
            inputs,
            outputs,
            assumptions: assumptions.iter().map(|s| s.to_string()).collect(),
        }
    }
    fn validate(&self) -> Result<(), String> {
        if self.id.name.trim().is_empty()
            || self.id.version == 0
            || self.description.trim().is_empty()
        {
            return Err("primitive needs a name, positive schema version and description".into());
        }
        for fields in [&self.inputs, &self.outputs] {
            let mut paths = std::collections::BTreeSet::new();
            for f in fields {
                if f.path.is_empty()
                    || f.unit.is_empty()
                    || f.shape.is_empty()
                    || !paths.insert(&f.path)
                    || f.quantity.as_ref().is_some_and(|q| q.unit() != f.unit)
                {
                    return Err("primitive fields need unique paths, units and shapes".into());
                }
            }
        }
        Ok(())
    }
}
type Call = dyn Fn(Value) -> Result<Value, String> + Send + Sync;
#[derive(Clone)]
pub(crate) struct Entry {
    descriptor: Descriptor,
    call: Arc<Call>,
}
impl std::fmt::Debug for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.descriptor.fmt(f)
    }
}
pub(crate) type Entries = BTreeMap<DefinitionId, Entry>;

impl BehaviorRegistry {
    /// Input deserialization, domain validation and execution share one Rust
    /// function. No script/inspector-specific implementation of the algorithm.
    pub fn register_primitive<I, O>(
        &mut self,
        descriptor: Descriptor,
        call: fn(I) -> Result<O, String>,
    ) -> Result<(), String>
    where
        I: DeserializeOwned + 'static,
        O: Serialize + 'static,
    {
        descriptor.validate()?;
        let definitions = self.frozen_definitions().map_err(|e| e.to_string())?;
        for field in descriptor.inputs.iter().chain(&descriptor.outputs) {
            if let Some(quantity) = &field.quantity {
                quantity.validate(definitions).map_err(|e| e.to_string())?;
            }
        }
        if self.primitives.contains_key(&descriptor.id) {
            return Err("duplicate primitive identity".into());
        }
        let key = descriptor.id.clone();
        self.primitives.insert(
            key,
            Entry {
                descriptor,
                call: Arc::new(move |value| {
                    let input = serde_json::from_value(value).map_err(|e| e.to_string())?;
                    serde_json::to_value(call(input)?).map_err(|e| e.to_string())
                }),
            },
        );
        Ok(())
    }
    pub fn primitive_descriptors(&self) -> impl Iterator<Item = &Descriptor> {
        self.primitives.values().map(|entry| &entry.descriptor)
    }
    pub fn call_primitive(&self, id: &DefinitionId, input: Value) -> Result<Value, String> {
        let entry = self
            .primitives
            .get(id)
            .ok_or("unknown primitive identity/version")?;
        (entry.call)(input)
    }
}
