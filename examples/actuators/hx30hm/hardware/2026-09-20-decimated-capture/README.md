# 400 Hz control, 200 Hz normal logging

Simulation-only candidate extending `../2026-09-20-buffered-capture`. The reusable
logger has an explicit `LOG_STRIDE` parameter (default 1; candidate selects 2).
Feedback reads, controller updates, PWM writes and register audits still execute
every 2.5 ms. Only normal completed odd frames are omitted from the computer log.
Even frames, diagnostic/fault frames, partial frames, START and TERMINAL remain.

## RTL results

- 25 complete 400 Hz cycles, 175 motor transactions, 13 retained normal frames
  (0, 2, …, 24), 12 explicitly unlogged frames.
- Three-motor transaction window: 1.94562 ms; 0.55438 ms period headroom.
- 226 bytes per retained frame; steady **45.2%** occupancy of the 1 Mbaud 8N1 host
  link, compared with 90.4% for full 400 Hz logging. Faults add records. These are
  wire timings, not measured USB/OS latency.
- All eight complete UART cases pass: normal, missing feedback, command watchdog,
  STOP, wrong audit on an odd frame, stalled host, bad CRC, partial reply. The
  watchdog fixture retains 200 Hz to exceed its lease within 25 frames.
- Both isolated logger cases pass: exact raw overflow/loss and ordered partial
  flushing/event copy. All 26 shared Rust decoder/event/upload tests pass.
- Complete synthesis: **19,210 / 23,040 LUTs**, 9,448 flip-flops and 12 memory
  primitives. `check -assert` passes. Placement/routing and 50 MHz timing remain
  unqualified. No serial hardware was opened or FPGA flashed.

`results/verification.json` reconciles retained packets against independent RTL
transaction events and records source/evidence hashes. CI rebuilds the eight UART
cases, two logger cases and synthesis hierarchy check; the generic-runtime job
runs the Rust tests. Raw UART and failed-run evidence remain in `results/`.

## Evidence and deployment boundary

The strict full-capture Rust reviewer rejects stride-2 packets. Explicit
`review_fpga_batch ... --sampled-two` review validates framing, identities,
order/cadence, retained readbacks, required even frames and stop evidence, but
reports `complete_controller_evidence: false`. It never invents missing samples
or claims full-cycle arithmetic review. Diagnostic frames add to the nominal
200 Hz logging cadence. Overflow still faults; guards and bus audits remain.

The host acquisition adapter still needs larger batched packets and explicit
stride handling. Normal physical upload limits remain unchanged. Three-motor
simulation does not qualify a twelve-motor physical bus. This is not a commissioned
hardware mode. See `PROTOCOL.md` for the explicit stride contract.

The browser profile is at
`examples/full-robot/measured-actuator-integration/browser-control-400hz/`:
400 Hz shared integer motor control, 200 Hz native capture profile, and 50 Hz outer
motion policy/live observations, with provisional bench-fit candidates.

## Reproduce

From the repository root:

```sh
node examples/actuators/hx30hm/hardware/2026-09-20-decimated-capture/run.mjs
cargo build --locked -p sim-runtime --example review_fpga_batch
cargo test --locked -p sim-runtime --test fpga_batch --test fpga_events --test fpga_upload --test fpga_sampled_capture > examples/actuators/hx30hm/hardware/2026-09-20-decimated-capture/results/rust-tests.log 2>&1
```

Run both Icarus commands in the CI job, retaining output in
`results/logger-tests.log`. From `firmware/`, run:

```sh
yosys -Q -s synthesize.ys > ../results/full-image-synthesis.log
```

Finally run `node examples/actuators/hx30hm/hardware/2026-09-20-decimated-capture/verify.mjs`
from the repository root. The verifier checks retained receipts/current sources;
the preceding commands regenerate those receipts. No command accesses hardware.
