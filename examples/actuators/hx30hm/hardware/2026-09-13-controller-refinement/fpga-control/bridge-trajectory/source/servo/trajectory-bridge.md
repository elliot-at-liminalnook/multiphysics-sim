# Upload-enabled bridge and transaction assembly

`bridge_control` now has an optional `TRAJECTORY_ENABLE` profile, requiring
`BUFFER_HOST=1` and `SAFETY_ENABLE=1`. It handles A2 CONFIG/ROW/SEAL locally,
returns verified progress, and preserves the existing supervised A1 controller.
The profile has not been loaded on hardware. It has **no autonomous START**.

`make bridge-trajectory` builds this separate image. Programming is not part of
the target. The first integration failed physical placement at 77% reported LUT
utilization; reducing datapath widths to bounds already enforced by the bridge
passes its arithmetic and UART checks. Packet-length remainder/subtraction logic
also has an equivalent, smaller-width formulation; full supervisor RTL equivalence
passes at all 1,157 comparison points. The compact image now passes final routed
timing at **53.71 MHz** against **50 MHz**. The earlier 43.18 MHz placement estimate
was preliminary; use the final routed result. This is not hardware commissioning.

## Upload exchange and replies

Use the [canonical upload format](trajectory-store.md). Additional A2 payload
`[4]` queries capability/progress/last rejection without changing the plan. Every
CONFIG/ROW/SEAL/STATUS receives a 31-byte reply, including rejected operations:

`FF FF FE 1B 00 payload[25] checksum`

| Payload offset | Field |
| --- | --- |
| 0 | Protocol version = 1 |
| 1 | Trajectory tag = A2 |
| 2 | Echoed operation: 0 config, 1 row, 2 seal, 4 status |
| 3 | Last upload rejected: 0 or 1 |
| 4 | Sealed plan valid: 0 or 1 |
| 5 | Verification busy: 0 or 1 |
| 6–7 | Selected motor mask, little endian |
| 8–9 | Frame count |
| 10–11 | Rows written |
| 12–15 | Period in device clocks |
| 16–19 | Computed plan CRC32 |
| 20–23 | Clock frequency: 50,000,000 Hz |
| 24 | Capabilities = 1: upload/status only |

CONFIG/ROW replies follow the store's acceptance edge. SEAL replies wait until
whole-plan verification ends and include the computed CRC. A new mutating request
clears the previous rejection before its own result; STATUS retains it. Store or
host-parser faults also trip the existing supervisor. Uploads cannot arm, renew
leases, compute PWM or reach the servo wire.

The host must exchange one request/reply at a time, check version/capability and
verify each acknowledged operation, row count and final CRC. Do not treat a row
receipt or queued packet as a successfully sealed plan. The Rust shared library
provides `Receipt`, `status_request` and `transfer` for these checks; transfer
retains raw replies and reports cancellation/rejection/timeout without retrying
or continuing an incomplete upload. Transport adapters retain transaction timing.
Successful upload does not establish motor readiness, firmware programming
identity, autonomous execution support or mechanical accuracy.

The buffered input path reserves room for a full upload reply before dispatch.
FIFO writes cannot overwrite unread bytes. STOP's immediate supervisor path
remains independent of reply credit; retained responses drain in order. No safety
threshold or watchdog interval was increased.

## Preparing scheduler transactions

`src/experiment_packet.v` assembles the scheduler's telemetry READ 0x38/15,
existing A1 controller packet, and torque/PWM audit READ 0x28/6. It calculates the
checksum over successive cycles to avoid a large combinational checksum chain.
`request_ready` only acknowledges the source when the complete packet is accepted
by the downstream bridge. The source must hold kind/ID/row/gains stable until that
handshake. Deasserting request validity cancels unfinished or stalled assembly.
Unknown kinds and unselected/foreign motors fault without producing a packet.
This builder is tested but not yet connected to the bridge scheduler source.

## Verification

- `make sim-bridge-trajectory`: UART upload acceptance/rejection, completed seal,
  capability query, STOP during a status reply, FIFO credit under a query burst,
  no leaked motor commands or arming, and retained controller/supervisor behavior.
- `make sim-experiment-packet`: Rust-generated A1 row agreement, reads/audits for
  all nine IDs, stable packet backpressure, cancellation and invalid requests.
- `make sim-buffered-control sim-control`: unchanged-profile queue, UART/supervisor
  and 4,096 original Rust/RTL controller vectors.
- The Rust compiler now also emits `fixed_pd_bounded_tb.v`: 4,096 vectors compare
  the narrowed bridge bindings against full generated arithmetic and Rust, including
  extreme bounds and unsaturated quantization. The bridge still rejects full-width
  out-of-range values before any narrowed result can be transmitted.
- The compact packet-length checker is formally equivalent to the preceding
  supervisor, including the command-forwarding and watchdog outputs. All 128
  possible length values also pass direct word/byte shape comparison.
- `cargo test -p sim-runtime --test fpga_upload`: receipt decoding from captured
  RTL UART bytes, ordered transfer, rejection/cancellation and provenance checks.

Next integration: scheduler request arbitration, immutable row addressing,
validated telemetry/audit correlation, final-wire completion, bounded timestamp
transport, Rust acquisition and complete image commissioning before new motion.
