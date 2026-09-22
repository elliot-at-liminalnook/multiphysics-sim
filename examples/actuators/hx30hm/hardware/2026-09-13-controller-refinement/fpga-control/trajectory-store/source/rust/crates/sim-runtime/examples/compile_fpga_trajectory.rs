//! Offline artifact/vector generation only; no FPGA upload or motor operation.
use sim_runtime::controller_refinement::{
    fpga::Plan,
    fpga_upload::{HEADER_BYTES, Upload},
};
use std::{io::Write, path::Path};
fn hex_le(bytes: &[u8]) -> String {
    bytes.iter().rev().map(|b| format!("{b:02x}")).collect()
}
fn write_new(dir: &Path, name: &str, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dir.join(name))?
        .write_all(bytes)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 {
        return Err(
            "Usage: compile_fpga_trajectory PLAN_JSON HOMES_JSON NEW_DIRECTORY (offline only)"
                .into(),
        );
    }
    let plan: Plan = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let homes: [u16; 9] = serde_json::from_slice(&std::fs::read(&args[2])?)?;
    let upload = Upload::compile(&plan, homes)?;
    let bytes = upload.canonical_bytes()?;
    let dir = Path::new(&args[3]);
    std::fs::create_dir(dir)?;
    write_new(dir, "upload.json", &serde_json::to_vec_pretty(&upload)?)?;
    write_new(
        dir,
        "header.hex",
        format!("{}\n", hex_le(&bytes[..HEADER_BYTES])).as_bytes(),
    )?;
    let rows = bytes[HEADER_BYTES..]
        .chunks_exact(36)
        .map(|r| format!("{}\n", hex_le(r)))
        .collect::<String>();
    write_new(dir, "rows.hex", rows.as_bytes())?;
    write_new(
        dir,
        "crc.hex",
        format!("{:08x}\n", upload.plan_crc32).as_bytes(),
    )?;
    let packets = upload
        .packets
        .iter()
        .map(|packet| {
            let mut padded = [0u8; 64];
            padded[..packet.len()].copy_from_slice(packet);
            format!("{}\n", hex_le(&padded))
        })
        .collect::<String>();
    let lengths = upload
        .packets
        .iter()
        .map(|packet| format!("{:02x}\n", packet.len()))
        .collect::<String>();
    write_new(dir, "packets.hex", packets.as_bytes())?;
    write_new(dir, "lengths.hex", lengths.as_bytes())?;
    println!(
        "{} frames compiled; prototype protocol only, no hardware accessed",
        plan.targets.len()
    );
    Ok(())
}
