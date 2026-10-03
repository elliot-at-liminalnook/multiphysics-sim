//! Owned RoboCAD archives, direct OCCT geometry, and physical derivations.
pub mod archive;
mod component;
pub use archive::ArchiveDocument;
pub mod geometry;
pub mod mass;

/// Hash the actual production source bytes, rather than a hand-maintained label.
pub fn production_source_identity() -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    for source in [
        include_bytes!("lib.rs").as_slice(),
        include_bytes!("archive.rs").as_slice(),
        include_bytes!("geometry.rs").as_slice(),
        include_bytes!("component.rs").as_slice(),
        include_bytes!("../native/bridge.cpp").as_slice(),
        include_bytes!("../build.rs").as_slice(),
        include_bytes!("../Cargo.toml").as_slice(),
    ] {
        h.update((source.len() as u64).to_le_bytes());
        h.update(source);
    }
    format!("sha256:{:x}", h.finalize())
}
