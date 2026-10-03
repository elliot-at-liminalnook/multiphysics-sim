//! Command-line compatibility adapter for the shared finite acquisition behavior.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    sim_runtime::hardware::bench::acquisition::cli(std::env::args().collect())
}
