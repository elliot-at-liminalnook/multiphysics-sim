//! Printable parts: the print registry (one source for printer, filament and
//! printed-joint values), a layer-aware voxel stress check with loads that can
//! come from a simulation run, joint capacities across seams, print-settings
//! planning and promotion of measured coupon results.
//!
//! Units: meshes and positions in millimetres (as CAD exports them); forces
//! in newtons, stresses in pascals, moments in newton-metres.

pub mod analyze;
pub mod fe;
pub mod joints;
pub mod mesh;
pub mod plan;
pub mod promote;
pub mod registry;
pub mod sha256;
pub mod study;
pub mod voxel;
