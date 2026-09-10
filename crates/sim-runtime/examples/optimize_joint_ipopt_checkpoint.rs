//! Separately named executable so existing optimization processes stay intact.
#[path = "optimize_joint_ipopt.rs"]
mod runner;
fn main() {
    runner::main();
}
