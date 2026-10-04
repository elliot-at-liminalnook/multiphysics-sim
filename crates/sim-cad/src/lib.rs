//! Owned RoboCAD archives, direct OCCT geometry, and physical derivations.
pub mod archive;
mod component;
pub mod edit;
pub use edit::Edit;
pub use archive::ArchiveDocument;
pub mod geometry;
pub mod kernel;
pub mod annotations;
pub mod saved_views;
pub mod nodes;
pub mod sketch;
pub mod ops;
pub mod robotics;
pub mod references;
pub mod export;
pub mod import;
pub mod stamp;
pub mod mass;

/// Hash the actual production source bytes, rather than a hand-maintained label.
pub fn production_source_identity() -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    for source in [
        include_bytes!("lib.rs").as_slice(),
        include_bytes!("archive.rs").as_slice(),
        include_bytes!("edit.rs").as_slice(),
        include_bytes!("geometry.rs").as_slice(),
        include_bytes!("kernel.rs").as_slice(),
        include_bytes!("annotations.rs").as_slice(),
        include_bytes!("saved_views.rs").as_slice(),
        include_bytes!("nodes.rs").as_slice(),
        include_bytes!("sketch.rs").as_slice(),
        include_bytes!("ops.rs").as_slice(),
        include_bytes!("robotics.rs").as_slice(),
        include_bytes!("references.rs").as_slice(),
        include_bytes!("export.rs").as_slice(),
        include_bytes!("stamp.rs").as_slice(),
        include_bytes!("import.rs").as_slice(),
        include_bytes!("component.rs").as_slice(),
        include_bytes!("../native/bridge.cpp").as_slice(),
        include_bytes!("../native/ops.cpp").as_slice(),
        include_bytes!("../build.rs").as_slice(),
        include_bytes!("../Cargo.toml").as_slice(),
    ] {
        h.update((source.len() as u64).to_le_bytes());
        h.update(source);
    }
    format!("sha256:{:x}", h.finalize())
}
