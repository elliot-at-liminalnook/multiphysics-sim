//! Directed control elements and external controllers.

pub mod elements;
pub mod external;

pub mod trajectory;
pub mod motion_parameters;
pub mod periodic_drift;
pub mod planar_prediction;
pub mod contact_phase;
pub mod smooth_return;
pub mod motion_clock;

pub mod stepping;
pub mod support_preload;
pub mod angle_integral;
pub mod load_damping;
pub mod command_lease;
pub mod neural;
pub mod policy_search;
pub mod distillation;
pub mod ppo;
pub mod optimization;

pub mod contact_slip;
pub mod displacement;
