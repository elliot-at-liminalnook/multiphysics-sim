//! What a live session is launched from: the captured model, its settings
//! and the authoring identities bound to it. Plain data (also in browsers);
//! the process transport lives in `system_worker`.
use crate::system_session::SessionConfig;
use serde::{Deserialize, Serialize};
use sim_core::{BehaviorRegistry, ModelWorld};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Launch {
    pub version: u32,
    pub run_id: String,
    pub model: ModelWorld,
    pub source_hash: String,
    pub revision: u64,
    pub config: SessionConfig,
    #[serde(default)]
    pub binding: Option<SourceBinding>,
    /// The system file's directory, which the model's FMU paths are relative to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
}
/// Authoring identities scoped to one exact model capture, never guessed by a UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceBinding {
    pub model_hash: String,
    pub description_id: String,
    pub identities: sim_inspect::model::IdentityBindings,
}
impl Launch {
    pub fn model_hash(&self) -> Result<String, String> {
        // Canonical JSON object ordering, independent of pretty printing.
        let value = serde_json::to_value(&self.model).map_err(|e| e.to_string())?;
        Ok(
            blake3::hash(&serde_json::to_vec(&value).map_err(|e| e.to_string())?)
                .to_hex()
                .to_string(),
        )
    }
    pub fn validate_binding(&self, registry: &BehaviorRegistry) -> Result<(), String> {
        if let Some(binding) = &self.binding {
            if self.model_hash()? != binding.model_hash {
                return Err("live model capture hash mismatch".into());
            }
            let d = sim_inspect::model::describe(
                &self.model,
                registry,
                &self.source_hash,
                self.revision,
                &binding.identities,
            )
            .map_err(|e| e.to_string())?
            .description;
            if d.id != binding.description_id {
                return Err("live model does not match authored description".into());
            }
        }
        Ok(())
    }
}
