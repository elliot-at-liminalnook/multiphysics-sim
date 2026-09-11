# Offline preparation for the nine-servo PWM sweep

Servos were unplugged at the user's request. No hardware I/O, programming,
or motion was performed during this validation. The safety FPGA image is built
but **not deployed**. The old transparent image does not provide these stops.

## What is implemented and tested

- A latched FPGA supervisor for temperature, high/low voltage, uncalibrated
  internal current, device errors, missing telemetry, per-ID command leases,
  S2 emergency stop, and bridge faults. A trip broadcasts zero PWM and torque
  disable repeatedly; new drive is blocked until explicit healthy rearming.
- Wire-level UART tests check real stop bytes/checksums, bus release, replies,
  local status, and SYNC transmission receipts. The image meets 50 MHz timing.
- Shared Rust protocol and deterministic sweep scheduling, with an offline
  serial rehearsal of all nine individually and concurrently. Injected current
  faults abort escalation and attempt zero/off/relock on every ID.
- A 720-trial schedule: 12 increasing levels from 25 to 1000/1000, both signs,
  three repeats, nine individual groups followed by all nine together. The
  full-drive direction bit is the measured HX firmware 3.15 bit 10.
- Atomic checkpoints retain a completed prefix. Resume requires an identical
  plan/bench condition and a fresh preflight; interrupted trials are rerun.
- 58 signed-PWM/shared-physics cases, direction symmetry and monotonic speed
  checks, refined timestep convergence, and analytic motor/thermal regressions.

The initial 250 µs physics step was sufficient for steady speed but not sampled
startup-current peaks. At the final 3.90625 µs step, halving the preceding step
changes sampled peak current by less than 0.24%. Sampling remains 5 ms, so these
are sampled peaks, not resolved electrical startup extrema. See
[physics-validation.json](physics-validation.json).

To repeat the full software validation in a new directory, run
`validate-software.sh /path/to/sipeed-tang-primer-25k/servo NEW_OUTPUT`.
It invokes no serial or programming commands. Tests/build logs and exact
source snapshots are retained here; snapshots are provenance, not a second
maintained runtime.

## Software-only preview

From the physics-simulator root:

```sh
cargo run --release -p sim-runtime --example characterize_hx_bridge -- \
  --validate-sweep examples/actuators/hx30hm/software-validation/pwm-sweep-plan.json \
  4,5,6,7,8,9,10,11,12
```

This branch exits before opening any serial port. The checked schedule is saved
in [sweep-schedule.json](sweep-schedule.json). Minimum planned pulse/rest time is
900 seconds, excluding preflight, transport, readbacks, and cleanup.

The runner selects `control: "pwm_sweep"`, requires the FPGA safety protocol
and exact threshold profile, verifies IDs/mode/zero drive, and arms explicitly.
It issues common SYNC PWM commands and polls each channel. Explicit heartbeats
keep each command lease alive; telemetry alone cannot do so. A local `STOP`
file aborts. Trial logs retain command/zero request windows, bridge receipts,
feedback, displacement, observed speed, and final stationary checks.

The supplied 250 ms pulse, 1000 ms rest, and 1200-count home-relative travel
limit are **experimental policy**, not physical motor limits. An additional
velocity × 300 ms travel margin can stop a trial early. Full drive is reached
only if every preceding trial passes. Actual pulse timing includes transport;
a slow/lost transaction can overrun the requested duration, while the FPGA
continues enforcing its independent leases. A bounded pulse peak must not be
reported as a proven steady-state or absolute maximum speed.

For resume, copy the plan and set `resume_from` to the previous successful
`checkpoint.json`, retaining all other fields. Always use a new output directory.
Record supply, fixture, load and ambient changes in `bench_condition`; changing
conditions invalidates checkpoint reuse. Legacy `control: "pwm"` files remain
historical small-pilot plans, not supervised sweep plans.

## What still requires power or physical equipment

The next powered session starts with FPGA commissioning: profile readback and
small-drive S2, host-loss, telemetry-loss, and rearm checks, with observed
stationary output after each stop. Only then run the performance ladder.
The last confirmed supply setting was 12.6 V / 1 A; previous concurrent tests
experienced severe rail sag. This plan does not assume additional current
headroom. A voltage trip identifies a supply/harness-limited trial, not a
servo speed ceiling.

The FPGA uses servo telemetry, whose current scale, calibration, update delay,
and thermal response remain unverified. It cannot cut supply power or measure
external supply current; stop broadcasts need a working bus. Temperature
sampling does not observe winding temperature directly. The detailed protocol
and limitations are in [fpga-source/safety.md](fpga-source/safety.md).

The 58 physics cases use the existing CAD-derived motor and declared estimates,
with an explicitly assumed added inertia and ideal supply. They validate the
software and numerical path, **not sim-to-real accuracy** or concurrent supply
sag. Torque-speed curves, known-load acceleration/inertia, backlash/compliance,
current calibration, coast/brake behavior, heating/cooling, sensor sample rate,
and held-out model fitting still require measured evidence. Full characterization
is not complete merely because this unloaded PWM ladder is prepared.
