# Buffered frame capture, A4 version 2

This is an **offline firmware candidate**, not a commissioned hardware protocol.
It retains the original A3/v1 START and TERMINAL records and replaces the seven
per-transaction records plus duplicate raw replies with one frame packet. Existing
normal acquisition validation still rejects periods below 10 ms; separate named
offline compile/review APIs allow 2.5–10 ms for the three-selected-axis study.

All integers are little endian. Outer packets use the existing UART header and
additive complement checksum; a complete compact packet has 30–255 bytes. A
transport reader must accept the larger packet size before using this candidate.
The current live acquisition application has not been switched to this format.
The candidate advertises capability bit `0x20` for batched capture (capabilities
39 for its fixed-gain profile, 35 without that profile).

| Byte offset | Size | Field |
| --- | ---: | --- |
| 0 | 2 | `FF FF` |
| 2 | 1 | Stream ID 253 |
| 3 | 1 | Total packet length minus 4 |
| 4 | 1 | Envelope status, zero |
| 5 | 1 | Version 2 |
| 6 | 1 | Tag A4 |
| 7 | 1 | Record type 1, frame batch |
| 8 | 4 | Run ID |
| 12 | 2 | Frame index |
| 14 | 8 | Scheduled frame origin in 50 MHz device ticks |
| 22 | 1 | Raw byte count, 0–128 |
| 23 | 1 | Event count, 0–7 |
| 24 | 2 | Selected motor mask, IDs 4–12, at most three selected |
| 26 | 1 | Bit 0 partial frame; bit 1 raw overflow |
| 27 | 2 | Dropped raw byte count; saturates at 65535 |
| 29 | variable | Raw motor-UART bytes, once, including unmatched/error/partial data |
| after raw | variable | Compact events in acquisition order |
| final | 1 | Complement of sum of bytes 2 through previous byte |

Each event starts with 13 bytes:

| Event-relative offset | Size | Field |
| --- | ---: | --- |
| 0 | 1 | Kind: 1 feedback, 2 control, 3 audit |
| 1 | 1 | Motor ID or 254 for combined control |
| 2 | 1 | Feedback sequence (zero for other kinds) |
| 3 | 3 | Request tick offset from frame origin |
| 6 | 3 | Completion tick offset from frame origin |
| 9 | 1 | Exclusive raw-byte end offset captured at event completion |
| 10 | 1 | Outcome |
| 11 | 1 | Servo error byte |
| 12 | 1 | Reported payload width |

Only control events append payload: two wire-encoded PWM bytes per selected axis,
in increasing ID order. The Rust decoder reconstructs the canonical nine slots,
zeroing unselected slots. Feedback/audit payloads are recovered from the raw
packet ending at the referenced offset; header, ID, width, error, and checksum
must match. Extra raw bytes remain available as diagnostics. Failed audits retain
the original raw register values. Sequence numbers describe accepted UART
observations, not internal encoder refresh or sensor age.

Three normal feedback replies occupy 63 raw bytes, and three audit replies occupy
36. Seven event headers occupy 91 bytes, and three PWM values occupy six bytes.
With the 30-byte frame envelope/checksum, a normal frame is **226 bytes**, versus
439 previously. Nothing is gained by dropping audits or measurement fields.

## Buffering and ownership

`experiment_frame_log.v` is a reusable logging module. The integrated candidate
in `firmware/src/bridge_control.v` connects it to the existing shared sequencer,
controller, supervisor, event encoder and UARTs. A copy is also provided in the
FPGA project's `servo/src/experiment_frame_log.v` for reuse; the production bridge
and its default build are not switched to this candidate.

The logger snapshots raw data and copies event metadata one byte per clock into a
current frame backed by two synchronous byte memories. It builds one packet
byte/checksum per two clocks and retains up to four
complete output packets in byte-addressed block RAM. A synchronous indexed read
port feeds the host FIFO at one byte per three clocks, outside motor scheduling.
Normal packet building needs about 9.1 us after metadata capture; it runs outside the
read/compute/write transaction path. A new raw byte during finalization restarts
that build so it is not silently omitted. A stored running origin avoids a
frame-index multiplier. Terminal emission flushes incomplete frame evidence first.

The motor scheduler no longer waits for the computer-UART FIFO's 128-byte reserve
or pending output logs. Logging copies can overlap autonomous motor traffic;
host-originated transactions still respect packet boundaries and bus ownership.
The original quiet-time guards, command/feedback watchdogs, drive/travel limits,
checksum checks, and per-frame audits remain.

When output banks fill, recorder credit eventually stops new transactions and the
existing deadline/supervisor path stops drive. Once the host drains, all buffered
events and terminal evidence remain readable. More than 128 raw bytes in a frame
raises a latched fault, retains the prefix, and records the dropped-byte count.
The raw decoder can inspect such an artifact, but the shared audit rejects it as
incomplete. A finite buffer cannot promise lossless capture of an unbounded stream.

Rust `fpga_batch::Frame::decode` validates the transport and retains diagnostics;
`decode_events` verifies run/mask/frame identity against START before expanding
into the existing `fpga_events` arithmetic, cadence, coverage and stop auditor.
The explicitly named offline-rate APIs reuse that auditor without relaxing the
normal acquisition or upload-validation paths.
