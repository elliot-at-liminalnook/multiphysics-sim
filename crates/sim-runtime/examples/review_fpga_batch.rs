//! Decode raw offline UART capture through the shared batch/event implementation.
use serde_json::json;
use sim_runtime::controller_refinement::{fpga_batch, fpga_events};
use std::{fs, path::Path};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let sampled = args.len() == 4 && args[3] == "--sampled-two";
    if args.len() != 3 && !sampled {
        return Err("review_fpga_batch UART_HEX OUTPUT_JSON [--sampled-two]".into());
    }
    let bytes = fs::read_to_string(&args[1])?
        .split_whitespace()
        .map(|s| u8::from_str_radix(s, 16))
        .collect::<Result<Vec<_>, _>>()?;
    let mut at = 0;
    let mut packets = Vec::new();
    let mut frames = Vec::new();
    let mut count = 0;
    while at < bytes.len() {
        if at + 4 > bytes.len() {
            return Err("Partial UART header".into());
        }
        let length = usize::from(bytes[at + 3]) + 4;
        if length < 6 || at + length > bytes.len() {
            return Err("Partial UART packet".into());
        }
        let p = bytes[at..at + length].to_vec();
        if p[..2] != [255, 255] || p[2..].iter().fold(0u8, |s, b| s.wrapping_add(*b)) != 255 {
            return Err("UART packet checksum/header".into());
        }
        if p[2] == 253 {
            if fpga_batch::is_batch(&p) {
                frames.push(fpga_batch::Frame::decode(&p)?);
            }
            packets.push(p);
        }
        count += 1;
        at += length;
    }
    let sampled_capture = if sampled {
        Some(fpga_batch::decode_sampled_capture(&packets, 2)?)
    } else {
        None
    };
    let events = if let Some(c) = &sampled_capture {
        std::iter::once(c.start.clone())
            .chain(c.frames.iter().flat_map(|f| f.events.clone()))
            .chain(std::iter::once(c.terminal.clone()))
            .collect()
    } else {
        fpga_batch::decode_events(&packets)?
    };
    let terminal = events.last().ok_or("Missing stream")?;
    if terminal.kind != fpga_events::Kind::Terminal {
        return Err("Missing terminal".into());
    }
    let output = json!({"simulation_only":true,"uart_blake3":blake3::hash(&bytes).to_hex().to_string(),
        "uart_packet_count":count,"transport_packets":packets,"frames":frames,"events":events,
        "terminal_result":terminal.outcome,"sampled_capture":sampled_capture});
    fs::write(Path::new(&args[2]), serde_json::to_vec_pretty(&output)?)?;
    Ok(())
}
