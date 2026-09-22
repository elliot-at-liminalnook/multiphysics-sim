//! Offline all-axis prediction through one shared physical supply.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use sim_runtime::controller_refinement::{calibration, fpga, fpga_group};
    use std::{fs, io::Write};
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() != 6 {
        return Err(
            "review_fpga_group RECORDING FAMILY SUPPLY_SCENARIO replay|closed-loop NEW_OUTPUT"
                .into(),
        );
    }
    let r: fpga::Recording = serde_json::from_slice(&fs::read(&args[1])?)?;
    let family: calibration::Family = serde_json::from_slice(&fs::read(&args[2])?)?;
    let scenario: fpga_group::SupplyScenario = serde_json::from_slice(&fs::read(&args[3])?)?;
    let closed = match args[4].as_str() {
        "closed-loop" => true,
        "replay" => false,
        _ => return Err("Unknown prediction mode".into()),
    };
    let mut last = 0;
    let result = fpga_group::predict(
        &r,
        &family,
        &scenario,
        closed,
        &std::sync::atomic::AtomicBool::new(false),
        |n, _| {
            if n / 100 > last {
                last = n / 100;
                eprintln!("Prediction {}%", last * 10);
            }
        },
    )?;
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&args[5])?;
    file.write_all(&serde_json::to_vec_pretty(&result)?)?;
    Ok(())
}
