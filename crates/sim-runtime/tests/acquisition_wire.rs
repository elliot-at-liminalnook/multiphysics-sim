//! Independent-language fixture: bytes came from the Verilog UART transmitter,
//! not from a Rust serializer. Includes a write ACK, invalid replies and loss.
use sim_runtime::acquisition::{BusTransaction, FrameDecoder};
#[test]
fn verilog_wire_fixture_preserves_timing_commands_failures_and_loss() {
    let bytes = include_bytes!("../../../examples/actuators/hx30hm/acquisition/fixture.bin");
    for chunk in [1, 7, 62, 4096] {
        let mut decoder = FrameDecoder::default();
        let mut records = vec![];
        for part in bytes.chunks(chunk) {
            for f in decoder.feed(part) {
                records.push(BusTransaction::from_frame(&f).unwrap());
            }
        }
        assert_eq!(decoder.statistics.crc_errors, 0);
        assert_eq!(records.len(), 10);
        assert_eq!(records.last().unwrap().transaction_id, 34);
        assert_eq!(records.last().unwrap().device_dropped_total, 25);
        assert!(
            records
                .iter()
                .all(|r| r.device_id == 2 && r.window.clock_hz == 50_000_000)
        );
        assert!(
            records
                .iter()
                .all(|r| r.window.completion_tick > r.window.request_tick)
        );
        let command = records.iter().find(|r| r.instruction == 3).unwrap();
        assert_eq!(command.request, [0x2e, 1, 0x80]);
        assert!(command.reply.is_empty());
        let reads: Vec<_> = records
            .iter()
            .filter(|r| r.instruction == 2 && r.outcome == 0)
            .collect();
        assert_eq!(reads.len(), 7);
        assert!(reads.iter().all(|r| r.reply == [0x34, 0x12]));
        assert_eq!(records.iter().filter(|r| r.outcome != 0).count(), 2);
        assert!(
            records
                .iter()
                .filter(|r| r.outcome != 0)
                .all(|r| r.reply.is_empty())
        );
    }
}
