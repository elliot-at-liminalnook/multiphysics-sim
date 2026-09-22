# Offline controller tracking — 15 September 2026

The hardware is disconnected. This campaign improves and tests controller behavior
in the shared Rust simulation; it does not establish physical tracking accuracy.
No motor commands, flashing, CAD parameter promotion, or live-viewer activation
were performed in this offline phase.

## Main result

**Gain tuning helps, but the saved full-size gait is not physically trackable by
these provisional motor models.** Its hip, worm, and foot references demand peak
speeds of 911, 2,983, and 1,398 degrees/s respectively. The largest 20 ms changes
are 18.2, 59.7, and 28.0 degrees. These come from the retained gait trace, not a new
measurement. Raising simulated drive does not make those changes disappear.

With full drive available, the selected shared gains improve the smaller motions:

| Nominal test | Original RMS, IDs 10 / 11 / 12 | Selected RMS, IDs 10 / 11 / 12 |
| --- | --- | --- |
| Move to 5.625 degrees and hold | 0.292 / 0.280 / 0.283° | 0.230 / 0.225 / 0.224° |
| Reversals at 2.7 Hz, ±5.625 degrees | 1.218 / 1.094 / 1.115° | 1.089 / 0.976 / 0.991° |
| Increasing-frequency motion | 1.015 / 0.895 / 0.912° | 0.886 / 0.773 / 0.790° |
| Gait at 9% angular amplitude | 0.911 / 1.769 / 0.878° | 0.766 / 1.404 / 0.721° |
| Original full-size gait | 8.905 / 20.007 / 14.613° | 9.266 / 19.311 / 14.195° |

The final half-second of the selected full-drive hold has zero sampled encoder
span in all three motor estimates. That means steady simulated encoder readings,
not zero physical shaft error. The full-size hip gait regresses after tuning;
this is retained, not promoted as a successful gait controller.

[Tracking plots](../controller-tracking-full-drive-simulation/tracking-comparison.png)
show the failures alongside the improvements.

## Drive and gain comparisons

The search freezes the plant and searches only FPGA-supported power-of-two gains.
It uses two deterministic coordinate-descent starts; this is a bounded search,
not a claim of global optimality. Its objective is worst-model training RMS plus
0.15 times peak error. Selection uses a composite ramp/hold/reversal waveform;
the five validation patterns above are not used for selection.

| Simulated PWM ceiling | Selected Kp / Kd / Kv, Q8 integers | Interpretation |
| --- | --- | --- |
| 100/1000, 10% | 1024 / 4096 / 4096 | Small training improvement; hold and some gait cases regress. Not promoted. |
| 350/1000, 35% | 4096 / 1024 / 4096 | Modest improvement; full gait remains poor. |
| 1000/1000, 100% | 4096 / 4096 / 4096 | Better small-motion tracking; full gait remains poor. Simulation candidate only. |

The original gains are 4096 / 0 / 4096. Each comparison keeps its drive ceiling
fixed. Full-drive results live in the
[separate campaign](../controller-tracking-full-drive-simulation/README.md).
The hardware/browser settings were not changed to any of these candidates.

Per-motor choices were also selected from the training search and checked
independently. They are not consistently better on validation; for example,
motor 11's full-drive training choice uses Kd=2048 but performs worse than the
shared Kd=4096 on all five nominal validation patterns. Per-motor plant differences
remain explicit. The current streamed FPGA source has shared group gains, not
per-axis gain storage.

## Feasible reference experiments

A new registered Rust component, `control.reference_governor`, constrains command
speed and acceleration with explicit state and units. It preserves the requested
signal separately. Its bounds apply before integer encoder quantization.

At 40 degrees/s and 200 degrees/s², using the selected full-drive controller,
tracking of the newly limited command is **0.552 / 0.396 / 0.443° RMS** for the
three estimates. Peak errors are **1.055 / 0.791 / 0.791°**. However, error against
the original gait remains **14.284 / 21.691 / 14.805° RMS**. The modified commands
are not an accepted walking gait: foot placement, contact timing, balance, and
travel have not been tested with this governor.

[Reference-envelope plot](../controller-tracking-full-drive-simulation/reference-envelope.png)
and its raw traces show both error definitions. Speed limits from 20 to 220
degrees/s were tested with acceleration limits equal to five times those values
per second. These are experimental command limits, not measured motor ratings.

## Validation and reproducibility

- 162 gain configurations across four provisional model variants; no plant fitting.
- 240 original/selected validation cases: holds, reversals, chirps, 9%-amplitude
  gait, and full gait, at nominal conditions and a combined adverse scenario.
- Nominal: 100 Hz, 11.8 V, 45°C, 2 ms sample-to-command delay, authored unloaded
  inertia. Adverse: 10.4 V, 55°C, 8 ms delay, five times the fixture load inertia.
  These are declared scenarios; the delay and load values are not new measurements.
- 36 timestep comparisons, from 250 to 125 microseconds. Maximum change in RMS
  tracking score was 0.0141°, below the declared 0.1° numerical check. This checks
  numerical consistency, not physical model accuracy.
- Three selected traces replay exactly, including encoder samples and PWM.
- 12 electrically coupled tests cover three and nine simultaneously reversing
  shafts, three drive ceilings, and two shared-wire resistances. The nine-motor
  scenario repeats the three provisional estimates; it does not assign calibrated
  models to physical motors 4–9.
- 17 focused runtime tests and the new governor test pass: integer-law parity,
  own feedback, zero-drive behavior, delay, cancellation, timestep sensitivity,
  single/group equivalence, and current/power accounting. Existing broader FPGA
  transaction evidence is retained in the hardware campaign.

Tracking RMS compares simulated encoder angle to the requested angle at the same
observation time. There is no fitted time shift. The previous-applied-target RMS
is saved separately, as are saturation, peak error, final error, and tail span.
Only use tail span as a hold-oscillation metric when the target is actually held.
All saved traces are explicitly `simulation_only`, never acquisition recordings.

`inputs.json` retains both complete families and the source gait; `search.json`
retains every candidate; `validation.json` indexes raw traces. `robustness/` holds
timestep, cadence, per-unit, governed-reference, and coupled electrical results.

Reproduce from the repository root, using **new** output directories:

```sh
cargo build --locked -p sim-runtime --example tune_motor_tracking --example check_motor_tracking --example motor_tracking_envelope
RAYON_NUM_THREADS=1 target/debug/examples/tune_motor_tracking examples/actuators/hx30hm/hardware/2026-09-14-three-motor-characterization/baseline-family.json examples/full-robot/measured-actuator-integration/voltage-conditioning/candidate-family.json examples/full-robot/measured-actuator-integration/browser-hardware/reference-trace.json NEW_OUTPUT_DIR
RAYON_NUM_THREADS=1 target/debug/examples/check_motor_tracking NEW_OUTPUT_DIR
# Append 1000 to the tuning command for a separate full-drive campaign.
RAYON_NUM_THREADS=1 target/debug/examples/motor_tracking_envelope NEW_FULL_DRIVE_OUTPUT_DIR
RAYON_NUM_THREADS=1 cargo test --locked -p sim-runtime --test tracking_design --test fpga_controller --test actuator_group
cargo test --locked -p sim-domain-control reference_governor
```

An interrupted search can resume only with identical frozen inputs and before
validation begins. The source snapshots and hashes accompany the results.
One Rayon worker avoids excessive small-system parallel overhead on this host;
the physics equations and timestep are unchanged.

## Electrical result and limits

With a hypothetical 11.8 V ideal source, 0.35 Ω shared feed, 0.1 Ω branches, and
0.2 A auxiliary load, the nine-motor full-drive reversal scenario reaches a
minimum shared-bus voltage of **9.687 V**, peak source current **6.038 A**, and
peak bus power **58.489 W**. These peaks need not occur at the same instant.
All instantaneous voltage/current/power traces are retained. This is an explicit
wiring hypothesis, not a bench measurement, battery rating, or supply recommendation.

The plant families remain provisional: their measured-vs-simulated angle errors
are approximately 1–2° on earlier validation patterns. Current is uncalibrated;
these electrical predictions cannot be treated as measured amps or watts.
The scenarios do not model a supply's CV/CC transition, battery depletion, BMS,
or thermal rise. Temperatures are imposed. Backlash stays at the families'
unmeasured zero assumption. Fixture-inertia variation is not a loaded robot leg.

## What remains

1. Use realistic actuator dynamics and speed/acceleration constraints **inside**
   gait generation; score original requested motion, motor response, foot placement,
   and balance together. Do not silently filter an already accepted ideal gait.
2. Choose a gait and tolerances that pass those complete robot checks. The new
   governor is available through the shared registry but has not been inserted
   into the live WASD controller or promoted into CAD.
3. Once hardware returns, commission 100 Hz live control, repeat identical small
   references, and refine each motor model against measurements. The streamed
   FPGA image still fails routing/timing; no new image was flashed.
4. Calibrate actual joint zeros, directions, transmissions, travel, drive limits,
   and loaded behavior when a physical leg/fixture exists. Simulation alone cannot
   establish these or qualify real-leg operation.
