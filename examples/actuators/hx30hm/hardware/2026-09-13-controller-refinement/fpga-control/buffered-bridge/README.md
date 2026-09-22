# Buffered experiment bridge milestone

The [buffered host path](buffered-bridge.md) is now integrated as an optional mode
of the existing controller bridge. Its source, FPGA image and checks are frozen here.
The image passed final routed timing at **60.68 MHz** against a **50 MHz** target.
It has **not been loaded** onto the FPGA and does not yet run autonomous experiments.

Eight queue cases passed. Wire-level tests cover STOP arriving during a servo
reply, intact telemetry/status, no bus contention and delivered zero/torque-off.
Controller arithmetic, signed PWM, all-nine commands, freshness and travel gating
pass in buffered and retained modes. Both modes passed the UART safety scenarios;
4,096 shared Rust/RTL arithmetic vectors also pass.

This implements host packet buffering, bus-idle admission and priority STOP,
not the second arbitration source. Still required: connect the scheduler's
transaction requests, verified trajectory storage, torque/PWM audit decoding,
device-clock data transport and the Rust acquisition adapter. Then commission the
combined image and take fresh motor measurements. Existing physical accuracy
failures remain unchanged; no new hardware measurements were taken here.

In the hardware repository, reproduce with `make sim-buffered-control`,
`make sim-control` and `make bridge-buffered`. None programs a board.
