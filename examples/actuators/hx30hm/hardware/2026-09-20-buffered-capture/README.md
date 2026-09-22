# Buffered motor capture and control-rate study

Implemented an offline FPGA candidate that separates measurement capture from
computer-link serialization and sends raw reply data once in a compact frame.
Shared Rust decoding/auditing supports it. **No FPGA was flashed, no motor was
commanded, and no live viewer, CAD parameter, or production controller was changed.**

## Transport result

Compared with the retained unmodified 1 Mbaud logger baseline:

| Metric | Original | Buffered/batched candidate |
| --- | ---: | ---: |
| Bytes per normal three-motor frame | 439 | **226** |
| Typical complete transaction window | 2.930 ms | **1.946 ms** |
| Typical inter-transaction gaps | 1.103 ms | **0.119 ms** |
| Typical first-feedback-completion to command completion | 0.898 ms | **0.876 ms** |
| Motor link / computer link | 1 / 1 Mbaud | **1 / 1 Mbaud** |
| Reads, combined control writes, audits per frame | 3 / 1 / 3 | **3 / 1 / 3** |

The candidate removes about **89% of gap time** and **49% of output bytes** at the
original link speed. The main gain is cycle budget: most old logging waits were
after command transmission. It does not shorten immediate feedback-to-command
delay by the whole millisecond. Internal sensor age remains unknown.

Normal complete UART simulations pass at 100, 200 and 400 Hz, 25 frames each, with
full-scale positive/negative synthetic controller outputs. Every transaction and
audit is retained, and final modeled drive is zero with torque disabled. These
are finite transport tests with stationary mock encoders, not gait or physical
endurance tests. Commands use the same row vectors at different cadences for this
transport stress test; the separate physics comparison keeps physical reference
duration fixed.

At 200 Hz, 226 bytes take at least 2.26 ms of host wire time per 5 ms period; at
400 Hz they consume about 90% of the 2.5 ms period before ancillary traffic. That
makes 200 Hz a much more comfortable next qualification target. Buffering does
not remove the sustained bandwidth limit.

## Failure behavior and verification

The integrated UART cases cover missing feedback, command-watchdog expiry,
explicit STOP, wrong PWM audit, a stalled host transmitter, corrupt reply CRC,
and a partial reply. The shared Rust auditor checks controller arithmetic,
reference identity, every timestamp, coverage, readbacks and terminal results.
Failed runs remain incomplete and unscored. Corrupt and partial raw bytes remain
in the batch rather than being replaced by clean synthetic replies.

The isolated overflow test pushes 140 raw bytes into the 128-byte frame capacity:
the prefix is intact, the exact 12-byte loss is declared, fault is asserted, and
START/partial-frame/TERMINAL records drain in order. A second mode checks event
copy progress, exact metadata, and partial-frame flushing. Both are included in CI. Rust
tests additionally reject truncated packets, corrupted outer/inner checksums,
invalid raw references, altered frame/run/mask identity, hidden partial frames,
and overflow artifacts; existing legacy event/upload tests remain in place.
CI also rebuilds the complete UART simulation, runs all ten transport cases,
and checks module connections with the synthesis front end.

`results/summary.json` contains the integrated cases and timing metrics, each
case directory retains the full UART bytes and direct FPGA event observations,
and `firmware/` retains the actual simulation candidate. `PROTOCOL.md` describes
format, capacity, ordering, and deployment boundaries. `verify.mjs` reruns the
23 Rust tests and two unit modes, decodes all ten UART cases, and reconciles every
event's timestamp/kind/ID against direct RTL observations. Its
`results/verification.json` retains tool versions and source/input/artifact hashes.
The first wide combinational
packet-builder prototype is retained under `results/wide-builder-prototype`; the
final builder serializes byte/checksum work across clocks instead. Intermediate
versions and their area reports are retained in `parallel-event-copy-prototype/`
and `register-queue-prototype/`. The latter's UART run caught a missing event-copy
advance; that failure log is retained, the advance is fixed, and the second unit
mode prevents that regression. The overflow
fixture's initial handshake bug is retained as `overflow-fixture-before-handshake-fix.v`;
the corrected fixture waits for an actual ready/valid clock edge.
Whole-image synthesis also caught a simulation-only hierarchical signal reference;
`experiment_session` now exposes the event-capture pulse through an explicit port.
The rejected synthesis log is retained; subsequent synthesis uses `-noautowire`
and `check -assert` so an undriven capture signal cannot be accepted.

The final logger's standalone synthesis uses **2,323 LUTs, 1,037 flip-flops and
three block-memory primitives**, down from 38,191 LUTs in the parallel metadata
prototype. The intermediate register-backed frame buffers used 6,756 LUTs and
made the complete design exceed the board's 23,040-LUT budget by 714 LUTs; that
rejected result is retained under `results/register-frame-buffers-prototype/`.
Serial byte operations and memory-backed capture/output buffers remove that area
overhead. Packet assembly now takes about 9.1 us, outside motor scheduling, and
retains the same measurement fields.

Final whole-image synthesis passes `check -assert` and uses **19,201 of 23,040
LUTs (83.3%)**, 9,447 flip-flops, and 12 memory primitives. It is within the logic
budget with 3,839 LUTs of headroom. `verify.mjs` checks this budget alongside the
simulation results; synthesis reports and rejected configurations remain retained.

**Whole-image placement, routing at
50 MHz, and physical operation are not established by RTL simulation.** The
existing host capture application also needs the version-2/larger-packet transport
adapter before this candidate can be commissioned. Default physical plan/upload
validation continues to enforce the previously qualified minimum period.

## Does the higher rate improve the gait?

The answer is conditional. I ran **54 shared-Rust motor simulations**, using three
existing provisional motor models, three reference patterns, two controller
families, and 100/200/400 Hz. Nine timestep comparisons passed; their largest
change in common-grid RMS was **0.00093 degrees**, against a predeclared 0.1-degree
numerical tolerance. That establishes numerical consistency, not model accuracy.

The same underlying piecewise-linear reference, duration, plant, voltage,
temperature and PWM ceiling are used at each rate. Targets undergo the same
encoder-count quantization. Scores use actual simulated shaft angle on the same
100 Hz time grid against the same reference, with no fitted time shift. A common
1.25 ms sample-to-apply delay is an explicit study assumption, not a measurement
of the servo's sensor age. No model was refitted.

For a controlled comparison, one controller family scales its derivative and
per-tick feedforward gains with rate: Kp stays 4096; Kd/Kv are 1024 at 100 Hz,
2048 at 200 Hz, and 4096 at 400 Hz. All are within the existing power-of-two
integer-law bounds. These are illustrative cadence-consistent gains, **not a new
gain search or a claim that they outperform the best tuned 100 Hz controller**.
The transport firmware still uses the original 4096/0/4096 law; the adjusted-gain
family is a separate physics comparison, not a newly tuned firmware image.

Small gait-reference RMS tracking error with those cadence-consistent gains:

| Model | 100 Hz | 200 Hz | 400 Hz |
| --- | ---: | ---: | ---: |
| ID 10 | 0.977° | 0.929° | 0.913° |
| ID 11 | 1.718° | 1.550° | 1.483° |
| ID 12 | 0.937° | 0.885° | 0.865° |

Moving to 200 Hz improves this comparison by about 5–10%; 400 Hz adds only about
2–4%. With the original unchanged 4096/0/4096 gains, higher rate makes the reversal
test worse for all three models: 100 Hz errors of 1.200/1.078/1.096° become
1.393/1.260/1.285° at 200 Hz. The per-tick law changes physical effect when cadence
changes, so blindly increasing rate is not a reliable improvement.

The full-size gait still has roughly **9–20° RMS error**, even at higher rates.
The earlier reference analysis found demands far beyond the provisional actuator
envelope. These tests do not include whole-robot foot placement, balance, or a
measured walking speed. See the retained full campaign in `gait-rate-study/`,
including all regressions, and the [prior reference-feasibility analysis](../../../../full-robot/measured-actuator-integration/controller-tracking-simulation/README.md).

## Decision

Keep the batching/decoupling change as an offline candidate and target **200 Hz
for the next tuned hardware qualification**, after transport integration and
sensor-freshness checks. Do not raise commanded gait speed on the strength of
the 400 Hz UART pass. I stopped timing optimization at the remaining bus/ownership
guards: reducing those or removing audits would change the reliability/evidence
contract for much less benefit. Further meaningful gait work should address
cadence-aware controller tuning and actuator-feasible references, with whole-robot
contact/balance validation, rather than shaving tiny logger delays.

## Reproduce

From the repository root (Node, Verilator/C++, Icarus Verilog and Rust required):

```sh
node examples/actuators/hx30hm/hardware/2026-09-20-buffered-capture/run.mjs
cargo build --locked -p sim-runtime --example compare_control_cadence --example review_fpga_batch
node examples/actuators/hx30hm/hardware/2026-09-20-buffered-capture/verify.mjs
RAYON_NUM_THREADS=1 target/debug/examples/compare_control_cadence examples/full-robot/measured-actuator-integration/controller-tracking-full-drive-simulation NEW_RATE_STUDY_DIRECTORY
```

The runner only builds/runs simulations in a fresh temporary directory. Its
explicit `HX_REUSE` option is for reusing an unchanged local build during an
investigation; fresh default execution is the reproducible qualification path.
No command above opens a serial port or flashes hardware.

For the FPGA resource check, run `yosys -Q -s synthesize.ys` from `firmware/`
with the Gowin-capable Yosys version recorded in `results/verification.json`.
This generates a synthesis report only; it does not place, route, or flash.
