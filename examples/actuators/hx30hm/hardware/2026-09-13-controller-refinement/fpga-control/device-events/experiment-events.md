# Device-clock recording stream — bridge wiring pending

`experiment_event_stream.v` now joins the transaction core to the bounded event
serializer. It keeps START, motor events and terminal evidence ordered while the
host is blocked. Run identity is frozen at START; the bank/run lock remains held
until the terminal packet is consumed. STOP must not reset the recorder. An event
already accepted by the serializer remains intact after its source withdraws.

This is tested with the transaction core, not yet connected to `bridge_control` or
programmed. No new physical cadence, stopping or tracking accuracy is established.

## Wire format, version 1

Each packet is `FF FF FD LEN 00 payload checksum`. ID FD separates device records
from ordinary motor replies and bridge status. LEN equals payload bytes plus two;
checksum is the inverted byte sum from ID through the final payload byte. Packets
are 37–64 bytes, and all integers are little endian.

| Payload offset | Field |
| --- | --- |
| 0 | Version 1 |
| 1 | Tag A3 |
| 2 | Kind: 0 START, 1 telemetry, 2 transmitted PWM, 3 audit, 4 terminal |
| 3–6 | Host-assigned run ID, frozen for this run |
| 7–8 | Frame index |
| 9 | Motor ID 4–12; FE for global records |
| 10 | Outcome: zero success; audit 1 failure; terminal scheduler result 0–8 |
| 11 | Supervisor sample sequence for telemetry; otherwise zero |
| 12–19 | Request ticks |
| 20–27 | Completion ticks |
| 28 | Device status/error byte for audit; otherwise zero |
| 29 | Actual reported data width |
| 30 | Retained data width |
| 31 onward | Raw data below |

- START retains plan CRC32, motor mask, frame count, period and clock frequency
  (4/2/2/4/4 bytes). Both timestamps equal the device clock at the accepted START.
- Telemetry retains the exact 15 bytes from 0x38. Its sequence must also have
  passed the independent supervisor. Voltage stays in its reported units and
  current stays raw; internal measurement age is unknown.
- Control retains nine exact transmitted register words (18 bytes), including
  PWM direction bits and canonical zero for unselected axes. Completion means the
  final UART byte finished, not the servo's internal switching instant.
- Audit retains the six torque/PWM bytes from 0x28. Malformed matching replies keep
  their actual width, error and first min(width,15) bytes with outcome 1. The two
  width fields make truncation explicit. The surrounding raw-byte capture must
  retain the complete malformed/partial traffic as well.
- Terminal retains stop-request ticks, stop-pair completion ticks, interrupted
  request ticks, interrupted kind/ID/presence (8/8/8/1/1/1 bytes). The last three
  fields and interrupted timestamp are zero when absent. Common request ticks are
  the run start; common completion ticks are stop-pair completion. A terminal does
  not prove mechanical stationarity.

The single packet bank is filled atomically, then scanned for checksum. There is
no second payload-to-packet copy bank. All 59 complete-run fixture packets remain
byte-identical after this resource reduction. Both implementations' logs remain
in the simulator archive; this optimization changes encoding latency, not bytes.

## Bridge integration contract

The bridge must validate the sealed source/start pose, reset controller history
consistently with Rust, reserve recorder capacity, and deliver the same accepted
START edge to recorder and transaction core. Pass the current device clock to
the recorder at that edge. Reject another START while either core or recorder is
locked; never overwrite the original run identity with a rejected request.

Connect recorder `event_ready` to transaction-core credit, without bypassing it.
Recorder faults feed the independent stop path. A pending terminal has separate
storage so physical stopping does not wait for host output. Pending motor events
drain before that terminal. Complete event packets and raw servo traffic must not
interleave at byte level on the host stream; reserve whole-packet capacity and use
a host rate sufficient for all records. A nine-motor frame contains 910 event
bytes before raw UART traffic, beyond 115200-baud capacity at 40–50 ms. A 1-Mbaud
host link is the integration target, not a measured present capability.

## Rust validation and batch review

`controller_refinement::fpga_events` retains the sealed upload and raw packets.
Review checks source/run identity, packet shape/checksum, complete ordered motor
coverage, fixed frame windows, sequence progression (including wrap), exact shared
controller arithmetic on recorded feedback, PWM readback, and terminal timing.
Every valid incomplete prefix remains unscored. Missing/reordered/mixed/corrupt
events and false success are rejected. Failed audit payloads remain available.

The report contains source/capture hashes and its limited completion scope.
`completed` means protocol coverage and stop-pair transmission; it does not mean
model acceptance, accurate electrical measurements or verified physical stopping.
This format must still be connected to serial acquisition, post-stop physical
checks and the existing viewer/simulation recording adapter.

In the simulator workspace:

```
cargo test -p sim-runtime --test fpga_events --test fpga_upload
cargo run -p sim-runtime --example review_controller -- review-device-capture CAPTURE_JSON NEW_REPORT
```

In the servo repository:

```
make sim-experiment-events sim-experiment-event-stream sim-experiment-recording
make check-experiment-events check-experiment-event-stream
```

Three serializer groups, three stream-lifecycle groups and nine transaction-plus-
recorder groups pass. The original nine transaction groups still pass without the
recorder. Serializer and stream Gowin synthesis pass. Six Rust event groups and
seven existing upload groups pass; real RTL serializer output supplies the Rust
fixture. The fixture is synthetic, contains no measured motor motion, and does not
exercise full bridge UART arbitration or integrated routed timing.
