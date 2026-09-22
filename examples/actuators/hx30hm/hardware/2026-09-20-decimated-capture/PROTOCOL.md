# Explicit alternate-frame capture

Extends [the buffered baseline](../2026-09-20-buffered-capture/PROTOCOL.md).
Packet offsets, raw/event layouts, checksums and A3 START/TERMINAL remain unchanged.

A4/v2 byte 26 flags:

| Bit | Meaning |
| --- | --- |
| 0 | Partial frame |
| 1 | Raw overflow |
| 2 | Logging stride 2 (otherwise stride 1) |
| 3 | Diagnostic: failed event/device error, recorder fault or unexpected raw length |

Capability `0x40` advertises stride 2; fixed-gain capabilities are 103 (`0x67`),
including the existing `0x20` batched-capture bit. Unknown flag bits are rejected.

Frame indices and origin ticks count control cycles, not retained records.
Normal frames 0, 2, 4, … are retained with unchanged 2.5 ms control period in START.
Odd diagnostic/partial frames remain in chronological position. Terminal emission
flushes incomplete frame evidence first. It is not a 200 Hz controller.

`fpga_batch::decode_events` rejects stride 2 to preserve full-capture audit rules.
`decode_sampled_capture(packets, 2)` explicitly selects sampled review, reports
intentionally unlogged indices and always marks full controller evidence false.
It rejects missing required even frames, reordering, inconsistent identity or
stride, ordinary unexpected odd frames, hidden partials, raw loss and an audit
mislabeled successful despite disagreeing readback. Failed runs stay failed. A
terminal before the first transaction may have no frame packets.

An ordinary odd frame is not retroactively retained because a later frame fails.
Decimation is an explicit evidence tradeoff; use stride 1 for complete controller
history and arithmetic reconstruction. No fault/stop guard is removed.
