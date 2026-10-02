//! Study validation and acknowledgment adapter above shared filesystem publication.
use super::Study;
use crate::publication::{self, Hooks, NoHooks, Policy};
use std::path::Path;

impl Study {
    /// New immutable evidence; only confirmed publication acknowledges success.
    pub fn save_new(&self, path: &Path) -> Result<(), String> {
        self.save_new_with(path, &NoHooks)
    }
    /// Same production path with deterministic filesystem-stage observation/injection.
    pub fn save_new_with(&self, path: &Path, hooks: &dyn Hooks) -> Result<(), String> {
        self.validate()?;
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        self.input_contents.publish_with(path, hooks)?;
        publication::publish_with(path, &bytes, Policy::ImmutableNew, hooks).into_result()
    }
    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path)
            .map_err(|e| format!("study.source {}: {e}", path.display()))?;
        Self::load_bytes(path, &bytes)
    }
    /// Captured manifest bytes resolve companions beside the original source.
    pub fn load_bytes(path: &Path, bytes: &[u8]) -> Result<Self, String> {
        let mut study: Self = serde_json::from_slice(bytes)
            .map_err(|e| format!("study.source {}: {e}", path.display()))?;
        study.validate()?;
        study.input_contents.hydrate(path)?;
        Ok(study)
    }
    /// Exact input bytes require the report's sibling `.study-inputs` directory.
    pub fn export_html_new(&self, path: &Path) -> Result<(), String> {
        self.export_html_new_with(path, &NoHooks)
    }
    pub fn export_html_new_with(&self, path: &Path, hooks: &dyn Hooks) -> Result<(), String> {
        let html = self.render_html()?;
        self.input_contents.publish_with(path, hooks)?;
        publication::publish_with(path, html.as_bytes(), Policy::ImmutableNew, hooks).into_result()
    }
}
