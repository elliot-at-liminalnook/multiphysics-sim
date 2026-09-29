# Leg characterization campaign

The campaign measures the leg's motors, transmissions and whole leg in stages,
with several safety layers. It fits every quantity with uncertainty and turns
the fits into an actuator profile and a gait-search patch.

- **Plan and status:** [PLAN.md](PLAN.md).
- **Code:** `crates/sim-runtime/src/acquisition/characterization.rs` (library).
- **Simulated run:** `examples/run_characterization.rs`.
- **Hardware rig:** `BusRig` in `calibration_serial.rs`.
- **Operator action:** the calibration server's `campaign` action and the
  **Characterization campaign** panel section.

Everything here was run **in simulation only**. The hardware path has been
exercised against the simulated FPGA and servos on a pseudo-terminal, not
against the leg. No value here is a hardware measurement.

## Run it

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo build --release -p sim-runtime --example run_characterization
target/release/examples/run_characterization \
  examples/actuators/hx30hm/hardware/characterization-campaign/plan.sim.json \
  examples/actuators/hx30hm/hardware/characterization-campaign/results-sim
cargo test --release -p sim-runtime --test characterization          # 8 tests, ~3 s
cargo test --release -p sim-runtime --lib campaign_runs_on_the_bus_rig # bus rig, real time, ~50 s
node web/tests/calibration-bench-e2e.mjs                             # server + panel + pty bench, ~6 min
```

`samples.json` (about 50 MB) is written by the example but is not kept here.
Rerun the example to regenerate it.

## What the simulated run does

1. **Hidden truth.** The simulated leg (`virtual_bench`) is built from motor
   models the campaign does not see:
   - knee: sticky;
   - worm: 10 counts of backlash;
   - belt/hip: cosine gravity load and 40 counts per duty of compliance.

   The bench also models:
   - shared supply sag;
   - a supply current sensor;
   - winding heating;
   - joint-side angle.
2. **Prior.** The prior is what the CAD profile claims: 5.51 rad/s no-load,
   0.85% friction duty, no backlash and no gravity.
3. **Rehearsal.** The whole plan runs on the prior first. All stages completed
   on the prior (2,161 simulated s).
4. **Campaign.** The campaign runs on the truth, and each open-loop segment is
   checked against the rehearsal.
   - Stages A–G and J run per axis; H, I and K follow.
   - The stage H joint combinations were vetted by the CAD kinematic mirror
     plus sampled inter-link penetration, at 0.5 mm tolerance.
   - Result: 2,259 simulated s, 3.1 s of wall time, no stage stopped by a gate.
5. **Fit, replay, promotion and ranking** follow (below).

## Results (`results-sim/`)

Fitted vs hidden truth. Uncertainty comes from the K repeats, with a 5% floor;
it is 25% where a stage ran once.

| Quantity | Knee fit / truth | Worm fit / truth | Belt fit / truth |
|---|---|---|---|
| speed gain (counts/s per duty) | 2650 / 2900 | 3274 / 3290 | 2984 / 3030 |
| step time constant (s) | 0.068 / 0.070 | 0.061 / 0.061 | 0.062 / 0.068 |
| small-signal time constant, E (s) | 0.055 / 0.070 | 0.045 / 0.061 | 0.050 / 0.068 |
| backlash, B (counts) | 3.95 / 4 | 10.4 / 10 | 6.1 / 6 |
| compliance, B (counts per duty) | 20 / 20 | 8 / 8 | 40 / 40 |
| gravity amplitude / zero, A | — | — | 0.0299 / 0.03, 1098 / 1100 |
| moving friction, A (duty) | 0.085 / 0.070 | 0.058 / 0.045 | 0.073 / 0.060 |
| thermal time constant, J (s) | 108 / 108 | 108 / 108 | 108 / 108 |

Replay: RMS position error when the recorded open-loop segments are replayed
through each model.

| Axis | CAD prior | Fitted | Truth (floor) |
|---|---|---|---|
| knee | 126 | 17 | 2.2 |
| worm | 121 | 12 | 2.9 |
| belt/hip | 190 | 10 | 3.3 |

Other results:

- **Stage H, all motors together:** minimum supply 11.04 V from 11.8 V (6.5%
  sag). Holding axes were disturbed by at most 12 counts while another moved.
- **Stage I:** the qualified gait baseline's +X leg, replayed in the air at
  half speed, tracked at 10–21 counts RMS (52–60 peak).
- **Gait-search patch** (`promotion.json` → `gait_search_patch`):
  - governor speed upper limit 2.98 rad/s;
  - governor acceleration upper limit 6.93 rad/s²;
  - per-coordinate reference speed limits: foot 2.98, hip 3.39, worm 3.79 rad/s.

  These are 0.8 × the full-drive speed (fitted gain beyond moving friction) and
  0.5 × the measured acceleration. They are **not** the speed reached inside
  the taught travel, which is limited by travel, not by the motor. Apply them
  only after replay agreement on hardware and promotion to CAD.
- **Test ranking** (uncertainty × gait sensitivity), top entries:
  1. worm gravity amplitude (near zero, so the relative uncertainty is large);
  2. the constant-load terms;
  3. knee backlash.

  On hardware, the ranking directs repeat time.

## Known weak fits and limits (simulation)

- **Moving friction reads 20–30% high.** Stage A's up/down difference includes
  a speed-dependent share at 40–160 counts/s.
- **Knee speed gain is about 9% low.** The knee's taught range is 464 counts,
  so the high-duty steps stop early. Steps shorter than 2.5 time constants are
  now rejected rather than extrapolated.
- **The small-signal time constant is about 25% low.** Friction makes the
  small-signal response faster than the large-step one. The phase fit is
  within 6° RMS and no longer sits at the grid edge.
- **Thermal resistance is a lower bound only.** Supply power includes every
  axis and the mechanical output.
- **Stage F fits the simulated servo's position loop,** not the real firmware's.
- **Stages E and fast G** need FPGA-side capture on hardware (the serial loop
  is 30–60 ms). `plan.hardware.json` omits E and J for the first hardware pass
  and has no known load (B skips compliance). The leg has no joint-side angle
  or supply-current sensor yet, so backlash and compliance are simulation-only
  for now.

## Fixes this work made to shared code

- **Short identification steps.** A first-order fit with dead time
  (`motor_identification::fit_step_with`) gets the step speed from steps that
  end before steady state. It is used by the campaign only; the Tune button
  keeps its own fit. Over the simulated serial bus it recovers the worm's gain
  as 3,316 vs 3,290.
- **Sine sweep phase sign.** Stage E's phase sign was inverted, and its input
  is now the whole applied duty.
- **Sine sweep amplitude.** The probe amplitude is at least 1.5 × the breakaway
  measured in D.
- **Thermal fit.** Stage J fits a first-order rise instead of timing 63% of a
  rise that had not plateaued (34 s, now 108 s).
- **Preflight cool-down.** Every stage now cools the axis first if it starts
  above the cool-down temperature.
