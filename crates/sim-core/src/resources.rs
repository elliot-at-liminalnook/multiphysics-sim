//! Content-addressed resources: data an element needs that is not a scalar
//! parameter, such as a robot's whole physical model.
//!
//! Element factories take scalar parameters only, so a resource is named by
//! a key: the first 52 bits of the BLAKE3 hash of its text, an integer that
//! an `f64` parameter holds exactly. The text itself travels with the model
//! ([`crate::ModelWorld::resources`]), so a model world can be serialised and
//! rebuilt in another process. Compiling installs a world's resources into
//! this process's cache ([`crate::ModelWorld::install_resources`]); a factory
//! reads the cache by key ([`get`]). The cache holds each distinct text once.
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// The largest key: keys are 52-bit integers, exact in an `f64`.
pub const MAX_KEY: u64 = (1 << 52) - 1;

fn cache() -> &'static Mutex<HashMap<u64, Arc<str>>> {
    static CACHE: OnceLock<Mutex<HashMap<u64, Arc<str>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The key of `text`.
pub fn key_of(text: &str) -> u64 {
    let hash = blake3::hash(text.as_bytes());
    let mut word = [0u8; 8];
    word.copy_from_slice(&hash.as_bytes()[..8]);
    u64::from_le_bytes(word) & MAX_KEY
}

/// A key as an element parameter holds it, or None for anything that is
/// not one (non-finite, negative, fractional or too large).
pub fn key_from_parameter(value: f64) -> Option<u64> {
    (value.is_finite() && value >= 0.0 && value.fract() == 0.0 && value <= MAX_KEY as f64).then_some(value as u64)
}

/// Make `text` available to factories in this process; its key. Refused
/// when another text already has the same key (a hash collision), so a
/// model never silently reads someone else's resource.
pub fn install(text: &str) -> Result<u64, String> {
    let key = key_of(text);
    let mut cache = cache().lock().unwrap_or_else(|p| p.into_inner());
    match cache.get(&key) {
        Some(existing) if &**existing != text => Err(format!("resource key {key} is already held by different content")),
        Some(_) => Ok(key),
        None => {
            cache.insert(key, Arc::from(text));
            Ok(key)
        }
    }
}

/// The text installed under `key`.
pub fn get(key: u64) -> Option<Arc<str>> {
    cache().lock().unwrap_or_else(|p| p.into_inner()).get(&key).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_exact_parameters_and_content_decides_them() {
        let a = install("{\"a\":1}").unwrap();
        assert_eq!(install("{\"a\":1}").unwrap(), a);
        assert_ne!(key_of("{\"a\":2}"), a);
        assert!(a <= MAX_KEY);
        assert_eq!(key_from_parameter(a as f64), Some(a));
        for bad in [f64::NAN, f64::INFINITY, -1.0, 0.5, (MAX_KEY as f64) * 4.0] {
            assert_eq!(key_from_parameter(bad), None);
        }
        assert_eq!(get(a).as_deref(), Some("{\"a\":1}"));
    }
}
