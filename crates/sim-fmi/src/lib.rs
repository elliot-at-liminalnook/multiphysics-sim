//! FMI 3.0 Co-Simulation FMUs as blocks (docs/architecture/composition.md,
//! "FMI 3 profile").
//!
//! - [`Fmu::load`] reads an archive, checks it against the supported
//!   profile (refusing by name what is outside it), extracts it privately.
//! - [`Fmu::summary`] / [`Fmu::interface`] say what it offers: inputs,
//!   outputs and parameters with units and the quantities they carry.
//! - [`add_block`] declares an FMU block in a model; [`bind`] instantiates
//!   every FMU block of a runtime (one instance per block, nothing shared)
//!   after checking the artifact's SHA-256 and every port and parameter.
//! - [`pack`] builds an FMU from C sources (the test fixtures, a user's own
//!   controller).
//!
//! The C ABI is written here from the standard's headers (`abi`); the model
//! description is read leniently with `roxmltree` (`description`). The
//! maintained `fmi` crate was evaluated first: its strict schema rejects
//! the standard `unit`/`quantity` attributes on variables, so FMUs written
//! by common tools would not load; `fmi-sys` needs libclang to generate
//! bindings for a dozen functions.

pub mod abi;
pub mod block;
pub mod description;
pub mod fmu;
pub mod pack;
pub mod units;

pub use block::FmuBlock;
pub use fmu::{Binding, Fmu, Summary, platform, sha256_hex};

use sim_core::{BlockTiming, ImplementationRef, Instance, ModelWorld, QuantityKind};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum FmiError {
    /// The file is not a readable FMU.
    #[error("{0}")]
    Archive(String),
    /// The FMU falls outside the supported profile.
    #[error("{fmu}: not supported: {}", reasons.join("; "))]
    Unsupported { fmu: String, reasons: Vec<String> },
    /// A block's ports or parameters do not match the FMU.
    #[error("{0}")]
    Binding(String),
    /// Instantiating or calling the FMU failed.
    #[error("{0}")]
    Instance(String),
}

/// Declare a block run by `fmu` in `world`: its interface is the FMU's
/// (ports typed by `kinds` where given, else by unit), the implementation
/// reference records `path` (as the model will store it), the artifact's
/// SHA-256 and the `parameters` to set. Checked against the FMU now.
pub fn add_block(world: &mut ModelWorld, name: &str, fmu: &Fmu, path: &str, kinds: &BTreeMap<String, QuantityKind>, timing: BlockTiming, parameters: BTreeMap<String, f64>) -> Result<Instance, FmiError> {
    let interface = fmu.interface(kinds)?;
    fmu.check(name, &interface, &parameters)?;
    world
        .add_block(name, interface, timing, ImplementationRef::Fmi3 { path: path.to_owned(), sha256: fmu.sha256.clone(), parameters })
        .map_err(|e| FmiError::Binding(e.to_string()))
}

/// Loaded FMUs by resolved path, shared by the blocks that use one.
#[derive(Default)]
pub struct Cache {
    loaded: BTreeMap<PathBuf, Fmu>,
}

impl Cache {
    pub fn load(&mut self, path: &Path) -> Result<&Fmu, FmiError> {
        if !self.loaded.contains_key(path) {
            let fmu = Fmu::load(path)?;
            self.loaded.insert(path.to_owned(), fmu);
        }
        Ok(&self.loaded[path])
    }
}

/// Instantiate and bind every FMU block of `runtime`. Relative paths are
/// resolved against `base` (the model file's directory). Each block gets
/// its own instance; the artifact must be the one the model recorded
/// (SHA-256), every port and parameter must match. Returns how many blocks
/// were bound.
pub fn bind(runtime: &mut sim_compile::Runtime, base: &Path, cache: &mut Cache) -> Result<usize, FmiError> {
    let blocks: Vec<sim_core::BlockDecl> = runtime.model.blocks.clone();
    let mut bound = 0;
    for decl in blocks {
        let ImplementationRef::Fmi3 { path, sha256, parameters } = &decl.implementation else { continue };
        let resolved = if Path::new(path).is_absolute() { PathBuf::from(path) } else { base.join(path) };
        let fmu = cache.load(&resolved)?;
        if !sha256.is_empty() && *sha256 != fmu.sha256 {
            return Err(FmiError::Binding(format!(
                "block `{}`: {} is not the FMU the model was built with (SHA-256 {}…, the model records {}…): re-add it to accept the new artifact",
                decl.name,
                resolved.display(),
                &fmu.sha256[..12],
                &sha256[..sha256.len().min(12)]
            )));
        }
        let binding = fmu.check(&decl.name, &decl.interface, parameters)?;
        let block = FmuBlock::new(fmu, &decl.name, decl.interface.clone(), binding)?;
        runtime.bind_block(&decl.name, Box::new(block)).map_err(|e| FmiError::Binding(e.to_string()))?;
        bound += 1;
    }
    Ok(bound)
}
