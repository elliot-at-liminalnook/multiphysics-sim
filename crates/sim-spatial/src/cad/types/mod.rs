//! The CAD panels' data types: the document tree and node summaries, the
//! selection, meshes, robot, physical, print, component, composition,
//! experiment, motion, thread, view and file shapes. They kept RoboCAD's
//! JSON (its `.rcad` manifest and the answers its service gave), so files
//! and the REST surface stay compatible; nothing here talks to a service:
//! CAD runs in process (`crate::cad::local`, `sim_cad`).
//!
//! Reads are tolerant: unknown fields are ignored, missing or `null` ones
//! take their defaults, non-finite numbers read as absent.
pub mod candidates;
pub mod components;
pub mod composition;
pub mod experiments;
pub mod files;
pub mod motion;
pub mod organize;
pub mod physical;
pub mod print;
pub mod references;
pub mod robot;
pub mod section;
pub mod sketch;
pub mod system_link;
pub mod threads;
pub mod types;
pub mod views;

pub use components::*;
pub use files::*;
pub use physical::*;
pub use print::*;
pub use references::*;
pub use robot::*;
pub use section::*;
pub use sketch::{PlaneFrame, SketchCall, SketchCurve, SketchGeometry, Uv, check_calls, plane_of};
pub use system_link::*;
pub use threads::*;
pub use types::*;
pub use views::*;

/// The display tessellation tolerance a mesh is drawn at when its node
/// names none (mm).
pub const MESH_TOLERANCE: f64 = 0.1;

/// Why a CAD operation was refused or failed (the in-process editor's
/// errors; the shape RoboCAD's client errors had, so callers read them alike).
#[derive(Clone, Debug, PartialEq)]
pub struct CadError {
    /// `local` for the in-process editor.
    pub method: &'static str,
    pub route: String,
    /// 404 when the thing asked for does not exist (a job, a node).
    pub status: Option<u16>,
    pub message: String,
}
impl CadError {
    /// A refusal or failure of the in-process CAD editor.
    pub fn local(message: impl Into<String>) -> CadError {
        CadError { method: "local", route: String::new(), status: None, message: message.into() }
    }
    /// The thing asked for does not exist.
    pub fn not_found(&self) -> bool {
        self.status == Some(404)
    }
}
impl std::fmt::Display for CadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for CadError {}
impl From<String> for CadError {
    fn from(message: String) -> CadError {
        CadError::local(message)
    }
}
