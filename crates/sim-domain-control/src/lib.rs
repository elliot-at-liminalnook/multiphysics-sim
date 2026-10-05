//! Directed control elements and external controllers.

pub mod elements;
pub mod pwm;


pub mod trajectory;
pub mod motion_parameters;
pub mod periodic_drift;
pub mod planar;
pub mod planar_prediction;
pub mod contact_phase;
pub mod smooth_return;
pub mod motion_clock;

pub mod stepping;
pub mod support_preload;
pub mod angle_integral;
pub mod load_damping;
pub mod command_lease;
pub mod heading;
pub mod neural;
pub mod policy_search;
pub mod distillation;
pub mod ppo;
pub mod optimization;

pub mod contact_slip;
pub mod displacement;

pub mod pulse;

pub mod pwm_feedback;
pub mod fixed_pd;
pub mod sampled_fixed_pd;
pub mod reference_governor;
pub mod motion_primitives;
pub mod gait_script;
pub mod maneuver_script;
pub mod pose_script;

pub mod adaptive_braking;
pub mod drive;
pub mod notes;
