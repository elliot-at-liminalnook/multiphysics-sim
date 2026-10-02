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
        // Refuse before any filesystem publication unless the exact serialized
        // representation and known diagnostic content pass the shared reopen path.
        let mut recovered = Self::decode_legacy_manifest(path, &bytes)?;
        recovered.input_contents = self.input_contents.clone();
        recovered.input_contents.verify_all(path)?;
        Self::finish_load(recovered)?;
        self.input_contents.publish_with(path, hooks)?;
        publication::publish_with(path, &bytes, Policy::ImmutableNew, hooks).into_result()
    }
    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = Self::read_source_bytes(path)?;
        Self::load_bytes(path, &bytes)
    }
    /// Auto-detect captured portable bytes; JSON resolves original sibling companions.
    pub fn load_bytes(path: &Path, bytes: &[u8]) -> Result<Self, String> {
        let study = if bytes.starts_with(super::portable::MAGIC) {
            super::portable::decode(bytes)?
        } else {
            let mut study = Self::decode_legacy_manifest(path, bytes)?;
            study.input_contents.hydrate(path)?;
            study
        };
        Self::finish_load(study)
    }
    fn decode_legacy_manifest(path: &Path, bytes: &[u8]) -> Result<Self, String> {
        // Historical JSON uses serde's established acceptance rules, not the
        // portable container's resource limits or duplicate-key policy.
        serde_json::from_slice(bytes).map_err(|e| format!("study.source {}: {e}", path.display()))
    }
    fn finish_load(mut study: Self) -> Result<Self, String> {
        super::refinement::terminal::cache(&mut study)?;
        study.validate()?;
        Ok(study)
    }
    /// Portable sources are bounded before allocation; historical JSON retains
    /// its established reader. Detection uses the same open handle as the read.
    pub fn read_source_bytes(path: &Path) -> Result<Vec<u8>, String> {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = std::fs::File::open(path).map_err(|e| format!("study.source {}: {e}", path.display()))?;
        let mut prefix = [0u8; 8];
        let mut used = 0;
        while used < prefix.len() {
            let n = file.read(&mut prefix[used..]).map_err(|e| e.to_string())?;
            if n == 0 { break; }
            used += n;
        }
        file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        if used == prefix.len() && &prefix == super::portable::MAGIC {
            let limit = super::portable::MAX_ARTIFACT_BYTES;
            if file.metadata().map_err(|e| e.to_string())?.len() > limit as u64 {
                return Err("study.portable.size: source exceeds 256 MiB".into());
            }
            file.take(limit as u64 + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
            if bytes.len() > limit { return Err("study.portable.size: source exceeds 256 MiB".into()); }
        } else {
            file.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        }
        Ok(bytes)
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
