# Autonomous transaction core — bridge integration pending

`experiment_transactions.v` joins the device-clock scheduler, packet builder and
raw-reply adapter. It is synthesizable and tested together, but is **not wired into
bridge_control, programmed, or physically commissioned**. It contains no second
controller law and does not arm motors or renew command leases.

At START it captures the motor mask and controller gains. The external sealed
trajectory bank supplies the indexed target/delta row. Its lock must include this
core's `bank_locked` signal and the outer logger's pending start/terminal records.
A start identity, matching plan CRC and current starting encoder positions must
be validated by the outer bridge before START is offered. No START opcode is
implemented by the current upload-only bridge.

## Bus and evidence contract

The core emits complete telemetry READ, A1 controller and torque/PWM audit packets.
`packet_ready` must mean the bridge can own the entire transaction on a quiet bus.
It must also reserve logger capacity. Host motor traffic cannot interleave with an
autonomous run; buffered STOP, heartbeat and status need separate arbitration.
All writes, including the internally generated A1 batch, still pass the ordinary
controller travel checks and independent supervisor.

Only echo-gated servo bytes are fed to the reply adapter. It retains the first 15
payload bytes without voltage/current conversion, together with actual width,
status and identity. Checksummed matching replies of wrong shape or error status
fail. Foreign, corrupt, duplicate and unsolicited replies cannot complete a read.
An inter-byte timeout discards partial parser state without acknowledging the
request. No retries occur. The bridge must drain prior bus activity before a run;
the servo protocol has no transaction sequence token that can distinguish a late
duplicate from a new response to the same address and ID.

Telemetry advances the scheduler only after **both** the matching complete raw
reply and a changed healthy supervisor sample sequence are present. Raw data alone
cannot replace supervisor acceptance; sequence changes alone cannot manufacture a
recorded sample. The completion tick includes the association pipeline delay and
is not a claim about the servo's internal ADC/encoder sample time.

`control_wire_done` must be asserted only after the final byte of the PWM batch
finishes transmission. `transmitted_pwm` holds the exact nine on-wire register
words, including direction bits and zero for unselected axes. The core freezes
them at completion. Each subsequent audit requires torque enabled and exact PWM
word equality. This confirms register readback, not applied torque or mechanical
stationarity. `stop_pair_wire_done` likewise means both stop packets have finished.

Completed events retain frame, motor, outcome, sequence, request/completion ticks
and actual raw data: 15 telemetry bytes, 18 transmitted PWM bytes or six audit
bytes. The data remains stable while the event is held, including after STOP.
Logger backpressure prevents new transactions; lost credit during a transaction
or held event stops the run. Rejected/truncated packets must also be retained by
the outer raw-byte capture. A malformed overlong payload is flagged with its full
width but only its first 15 bytes are available on this core's event port.

Scheduler result 8 now identifies transaction-adapter failures; result 7 retains
torque/PWM audit failure. Neither is silently converted to a successful frame.

## Verification

```
make sim-experiment-transactions sim-experiment-reply sim-experiment-scheduler
make check-experiment-transactions
```

Nine integrated test groups exercise two complete nine-motor frames, sparse
selection, frozen gains, exact transmitted-PWM audits, missing raw/accepted
feedback independently, error-status replies, logging backpressure and cancellation. Seven raw-parser
groups cover all IDs, signed PWM, bad status/shape/checksum/identity, truncation,
duplicates, cancellation and invalid ownership. Sixteen scheduler groups cover
its deadlines, supervision, stop completion and adapter faults.

Integrated Gowin synthesis passes. The test bridge models packet ownership and
wire completion; these tests do not drive the physical UART or real motors.

## Remaining work

Connect this core to bridge arbitration and immutable row addressing; implement
the start/terminal identity and timestamp stream with bounded capacity; connect
Rust acquisition and the existing review format. Test the complete UART/controller/
supervisor path and final routed timing, then commission independent stopping and
measure zero-command cadence before collecting fresh bounded motion trials.
