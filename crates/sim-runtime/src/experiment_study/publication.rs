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
        let bytes = Self::read_source_bytes(path)?;
        Self::load_bytes(path, &bytes)
    }
    /// Auto-detect captured portable bytes; JSON resolves original sibling companions.
    pub fn load_bytes(path: &Path, bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > super::portable::MAX_ARTIFACT_BYTES {
            return Err("study.portable.size: source exceeds 256 MiB".into());
        }
        let mut study = if bytes.starts_with(super::portable::MAGIC) {
            super::portable::decode(bytes)?
        } else {
            super::portable::json_bounds(bytes, super::portable::MAX_MANIFEST_BYTES)?;
            super::portable::manifest_preflight(bytes, false)?;
            let mut study: Self = serde_json::from_slice(bytes)
                .map_err(|e| format!("study.source {}: {e}", path.display()))?;
            // Legacy manifests retain their companion-only hydration contract.
            study.input_contents.hydrate(path)?;
            study
        };
        // Worker-only decoding restores diagnostic terminals for the existing review.
        super::refinement::terminal::cache(&mut study)?;
        study.validate()?;
        Ok(study)
    }
    /// Bounded capture reader shared by all Study-source consumers.
    pub fn read_source_bytes(path: &Path) -> Result<Vec<u8>, String> {
        super::portable::read_bounded(path, super::portable::MAX_ARTIFACT_BYTES)
    }
    /// Diagnostic projection only: does not authorize object hydration or Study
    /// attachment. Call load_bytes for membership, hash and semantic validation.
    pub fn manifest_value(bytes: &[u8]) -> Result<serde_json::Value, String> {
        let manifest = super::portable::manifest_slice(bytes)?;
        serde_json::from_slice(manifest).map_err(|e| format!("study.source.manifest: {e}"))
    }
    /// Self-contained immutable artifact; no companion publication or source edits.
    pub fn portable_bytes(&self) -> Result<Vec<u8>, String> {
        super::portable::encode(self)
    }
    pub fn save_portable_new(&self, path: &Path) -> Result<(), String> {
        self.save_portable_new_with(path, &NoHooks)
    }
    pub fn save_portable_new_with(&self, path: &Path, hooks: &dyn Hooks) -> Result<(), String> {
        let bytes = self.portable_bytes()?;
        publication::publish_with(path, &bytes, Policy::ImmutableNew, hooks).into_result()
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
