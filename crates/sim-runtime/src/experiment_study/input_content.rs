//! Exact source bytes owned by Study; receipts contain only bounded references.
//! Capture, hydration and publication are worker/job operations, never frame work.
use serde::{Deserialize, Serialize};
use crate::publication::{self, Hooks, NoHooks, Outcome, Policy, Stage};
use std::{collections::BTreeMap, path::{Path, PathBuf}, sync::Arc};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentRef {
    pub version: u32,
    pub blake3: String,
    pub byte_length: u64,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Store {
    #[serde(default)]
    pub references: BTreeMap<String, ContentRef>,
    /// Pending and hydrated bytes share allocations across immutable snapshots.
    #[serde(skip)]
    pub contents: BTreeMap<String, Arc<Vec<u8>>>,
}
impl Store {
    pub fn capture(&mut self, bytes: Vec<u8>) -> ContentRef {
        let blake3 = blake3::hash(&bytes).to_hex().to_string();
        let reference = ContentRef { version: 1, blake3: blake3.clone(), byte_length: bytes.len() as u64 };
        self.references.entry(blake3.clone()).or_insert_with(|| reference.clone());
        self.contents.entry(blake3).or_insert_with(|| Arc::new(bytes));
        reference
    }
    /// Cheap metadata/Arc union. No hashing, serialization or disk work.
    pub fn merge(&mut self, other: &Self) -> Result<(), String> {
        other.validate()?;
        for (hash, reference) in &other.references {
            if self.references.get(hash).is_some_and(|existing| existing != reference) {
                return Err(format!("input_contents.references.{hash}: conflicting immutable identity"));
            }
        }
        self.references.extend(other.references.clone());
        for (hash, bytes) in &other.contents { self.contents.entry(hash.clone()).or_insert_with(|| bytes.clone()); }
        Ok(())
    }
    pub fn validate(&self) -> Result<(), String> {
        for (hash, reference) in &self.references {
            if reference.version != 1 || hash != &reference.blake3 || hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
                return Err(format!("input_contents.references.{hash}: invalid version or lowercase BLAKE3 identity"));
            }
        }
        Ok(())
    }
    /// Only recognized envelopes are interpreted; legacy raw and opaque payloads remain untouched.
    pub fn validate_envelope(&self, value: &serde_json::Value, path: &str) -> Result<(), String> {
        let Some(raw) = value.get("content_ref") else { return Ok(()); };
        let reference: ContentRef = serde_json::from_value(raw.clone()).map_err(|e| format!("{path}.content_ref: {e}"))?;
        if self.references.get(&reference.blake3) != Some(&reference) {
            return Err(format!("{path}.content_ref: missing or conflicting input_contents reference {}", reference.blake3));
        }
        Ok(())
    }
    pub fn resolve(&self, hash: &str) -> Result<&[u8], String> {
        if !self.references.contains_key(hash) { return Err(format!("input_contents.references.{hash}: missing reference")); }
        self.contents.get(hash).map(|bytes| bytes.as_slice()).ok_or_else(|| format!("input_contents.{hash}: missing recoverable content; load through Study::load"))
    }
    pub fn artifact_path(manifest: &Path, hash: &str) -> PathBuf {
        manifest.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new(".")).join(".study-inputs").join(format!("{hash}.bin"))
    }
    fn verify(reference: &ContentRef, bytes: &[u8], path: &Path) -> Result<(), String> {
        if bytes.len() as u64 != reference.byte_length || blake3::hash(bytes).to_hex().to_string() != reference.blake3 {
            return Err(format!("input_contents.{}: corrupt artifact {} (length or BLAKE3 mismatch)", reference.blake3, path.display()));
        }
        Ok(())
    }
    pub fn hydrate(&mut self, manifest: &Path) -> Result<(), String> {
        self.validate()?;
        for (hash, reference) in &self.references {
            let path = Self::artifact_path(manifest, hash);
            let bytes = std::fs::read(&path).map_err(|e| format!("input_contents.{hash}: missing/unreadable artifact {}: {e}", path.display()))?;
            Self::verify(reference, &bytes, &path)?;
            self.contents.insert(hash.clone(), Arc::new(bytes));
        }
        Ok(())
    }
    /// Publish content first, manifest last. Existing content is verified and never overwritten.
    /// A failed manifest publication leaves recoverable bytes in this owner and immutable artifacts intact.
    pub fn publish(&self, manifest: &Path) -> Result<(), String> {
        self.publish_with(manifest, &NoHooks)
    }
    pub fn publish_with(&self, manifest: &Path, hooks: &dyn Hooks) -> Result<(), String> {
        self.validate()?;
        for (hash, reference) in &self.references {
            let path = Self::artifact_path(manifest, hash);
            let bytes = self.resolve(hash)?;
            Self::verify(reference, bytes, &path)?;
            let result = match std::fs::read(&path) {
                Ok(existing) => {
                    Self::verify(reference, &existing, &path)?;
                    // Readable identity is insufficient: synchronize reused evidence too.
                    publication::confirm_existing_with(&path, hooks)
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    match publication::publish_with(&path, bytes, Policy::ImmutableNew, hooks) {
                        Outcome::Unpublished(failure)
                            if failure.stage == Stage::Publish
                                && failure.kind == std::io::ErrorKind::AlreadyExists => {
                            // Only the immutable destination race permits verified reuse.
                            // Other failures, especially visible-unconfirmed ones, stay errors.
                            if failure.cleanup_error.is_some() {
                                return Outcome::Unpublished(failure).into_result().map_err(|e|
                                    format!("input_contents.{hash}: publication {}: {e}", path.display()));
                            }
                            let existing = std::fs::read(&path).map_err(|e|
                                format!("input_contents.{hash}: concurrent artifact {}: {e}", path.display()))?;
                            Self::verify(reference, &existing, &path)?;
                            publication::confirm_existing_with(&path, hooks)
                        }
                        outcome => outcome,
                    }
                }
                Err(error) => return Err(format!("input_contents.{hash}: unreadable artifact {}: {error}", path.display())),
            };
            result.into_result().map_err(|e| format!("input_contents.{hash}: publication {}: {e}", path.display()))?;
        }
        Ok(())
    }
}
