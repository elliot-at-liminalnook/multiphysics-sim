//! Offline audit of streamed FPGA references against device-clock evidence.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 2 {
        return Err("review_live_fpga CAPTURE_JSON (offline only)".into());
    }
    let capture: sim_runtime::controller_refinement::live_stream::Capture =
        serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let review = capture.review()?;
    println!("{}", serde_json::to_string_pretty(&review)?);
    Ok(())
}
