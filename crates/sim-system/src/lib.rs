//! Hierarchical system documents for the shared multiphysics runtime.
//!
//! - [`document`]: the versioned `sim.system/1` file format.
//! - [`resolve`]: port types, validation and review findings.
//! - [`commands`]: the one edit command set (both viewers, REST, CLI).
//! - [`store`]: file-backed editing with a shared undo/redo journal.
//! - [`flatten`]: hierarchy → flat `ModelWorld` with path identities.
//! - [`library`]: saved definitions, the element palette, swap alternatives.
//! - [`snap`]: what attaches to a port, and the commands that attach it.
//! - [`assets`]: content-addressed reference images.
pub mod assets;
pub mod commands;
pub mod composition;
pub mod document;
pub mod flatten;
pub mod library;
pub mod resolve;
pub mod snap;
pub mod profile;
pub mod store;

pub use commands::{apply, Command, Outcome};
pub use document::*;
pub use flatten::{flatten, Flattened};
pub use resolve::{Finding, Resolver};
pub use store::SystemStore;

#[derive(Debug, thiserror::Error)]
pub enum SystemError {
    #[error("{0}")]
    Invalid(String),
    #[error("command {index}: {message}")]
    Command { index: usize, message: String },
    #[error("the system changed (revision {found}, expected {expected}); reload and retry")]
    Stale { expected: u64, found: u64 },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub mod display;

pub mod display_overlap;
