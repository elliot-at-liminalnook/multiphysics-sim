//! CAD parity contracts shared by headless tooling and future CAD replacements.
//! No UI or graphics ownership; current adapters still depend on Python/OCCT.
pub mod compare;
pub mod contract;
pub mod gates;
#[cfg(not(target_arch = "wasm32"))]
pub mod isolation;
#[cfg(not(target_arch = "wasm32"))]
pub mod native;
#[cfg(not(target_arch = "wasm32"))]
pub mod publication;
#[cfg(not(target_arch = "wasm32"))]
pub mod runner;

#[cfg(not(target_arch = "wasm32"))]
pub mod process;

#[cfg(not(target_arch = "wasm32"))]
mod native_observations;

#[cfg(not(target_arch = "wasm32"))]
pub mod owned_path;
