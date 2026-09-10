//! Event-boundary fix build; preserve executables pinned by running searches.
#[path = "optimize_joint_ipopt.rs"]
mod runner;
fn main() {
    runner::main();
}
