# Buffered host path for the FPGA experiment bridge

The `BUFFER_HOST=1` option in `bridge_control.v` now receives complete host packets
while the servo bus is busy. The existing default remains `BUFFER_HOST=0`.
This is the host-input part of autonomous bus arbitration; the experiment
scheduler is **not yet connected** to this bridge.

`host_packet_queue.v` holds two ordinary packets and a separate priority STOP
slot. Packets become visible only after their complete length and checksum have
been validated. Backpressure cannot expose a partial packet. Maximum packet size
is 64 bytes; a partial packet times out after a 10 ms inter-byte gap at 50 MHz.

A valid local STOP, or a valid known-ID DISARM, immediately injects a canonical
STOP into the existing supervisor. It clears older queued work and cannot be
blocked by a full normal queue. The original packet remains in the priority slot
for its status response. Later malformed traffic cannot erase that pending STOP.
Bad checksums/lengths, partial-packet timeouts and ordinary queue overflow pulse
the existing bridge-fault latch and discard queued normal commands.

The bridge accepts queued packets only when the bus is released, its reply
reservation and quiet interval have elapsed, stop processing gaps have elapsed,
and no local response is being assembled. Every ordinary packet still passes
through `hx_safety`; every A1 command still uses the existing compiled controller
and all its checks. The queue cannot renew leases or rearm motors on its own.
The application protocol still expects one normal host transaction at a time;
this buffer is for bounded concurrent bus activity and priority stop delivery,
not an untagged host RPC pipeline.

A STOP does not truncate a packet already on the servo wire. An unstarted packet
is discarded when priority STOP/fault arrives. The existing independent stop
serializer retains reply turnaround, line-quiet requirements, spaced PWM-zero /
torque-off packets and TX release. A transmitted stop is not proof of mechanical
stationarity or power isolation.

## Verification commands

```sh
make sim-buffered-control
make sim-control
make bridge-buffered
```

These targets do not program hardware. The buffered variant uses its own
`impl/bridge_buffered.fs` output. Queue tests cover partial/stalled packets, priority
STOP in a full queue, malformed data, overflow, timeout, retained STOP, 64-byte
packets, simultaneous full dequeue/enqueue and DISARM priority.

UART tests run the same controller and safety scenarios in both host modes.
A directed buffered test sends STOP during an active servo telemetry reply and
requires an immediate supervisor stop, intact telemetry and status, delivered
zero/off packets, no armed motors and no bus contention. Shared Rust/RTL arithmetic
parity remains covered by 4,096 vectors.

## Remaining integration

Connect the scheduler's complete-packet request source at the bridge's idle
arbitration point, with host STOP/heartbeat/status priority. Add verified trajectory
storage, strict audit-response matching, device-clock data transport and the Rust
capture adapter. Then verify the combined image and commission it on hardware.
This buffered bridge alone does not provide a faster autonomous control loop or
new evidence of motor-model accuracy.
