//! Separate executable keeps earlier live searches on their recorded binary.
#[path = "optimize_joint_ipopt.rs"]
mod runner;
fn main() { runner::main(); }
