# Gait generation with provisional motors

**2026-09-19:** The latest [contact-shape campaign](contact-shape-2026-09-19/README.md)
co-designs stance, foot paths and contact timing with detailed motors and reference
limits active. Its best ten-second result is **1.071 m forward**, with **1.604°
worst-motor RMS tracking error** and repeated clearance of every foot. It also
passes thirty seconds (3.268 m forward), stop/reverse, timestep sensitivity and
exact replay. This qualifies a flat-ground simulation baseline; no browser or
hardware promotion is claimed. This follows the [actuator-bounded campaign](actuator-bounded-2026-09-19/README.md).
The historical results below use different controller recipes and limits.

The 0.264° historical bench criterion no longer blocks gait experiments. This
pilot uses the twelve-motor CAD revision and shared Rust FPGA controller/motor
dynamics. Model errors remain visible; no fit is promoted to calibrated status.

Start from the historical command schedule, including packet sequence refreshes.
The existing 50 ms motor windows used default zero motion commands, so they prove
motor integration and numerical consistency, not execution of the walking gait.
An attempted 2 s default-command capture was cancelled before producing a report
when this distinction was found; it is not gait evidence.

The first search varies requested pace (25–100% of historical command) and tracking
feedback (-1 to 1), retaining the original trajectory shape. Forward speed and yaw
rate scale together. These are authored search ranges, not physical limits.
Baseline uses the exact historical command values. The initial 2 s horizon is a
screening pilot; finalists need longer tests and the matched 300 s qualification
before comparison with the historical 0.5736 m/s result. Only complete, non-fallen
trials can receive a score. Failed preparation, integration and falls are retained.

Supply voltages remain explicitly imposed. The shared runtime can now accept CAD
power branches, but this robot has no measured battery/wiring profile. Loaded-leg
behavior, physical-unit assignments and calibrated current are also unresolved.
The current CAD profile uses 100 Hz feedback and a **35% PWM command limit**;
supplies are held at 11.1 V and command latency is estimated as zero. These are
the existing provisional recipe, not a claim about the motors' maximum capability.
Gait comparisons retain them unchanged. Revisit controller authority and electrical
scenarios as explicit CAD/configuration comparisons, preserving these baselines.

From the repository root:

```sh
gait_run=examples/full-robot/measured-actuator-integration/gait-generation/reproduction
node examples/full-robot/measured-actuator-integration/gait-generation/prepare_search.mjs 2 "$gait_run"
cargo build --locked -p sim-runtime --features bayesian --example search_motion
target/debug/examples/search_motion init "$gait_run/pilot.spec.json" "$gait_run/pilot.settings.json" "$gait_run/pilot-journal"
target/debug/examples/search_motion advance "$gait_run/pilot-journal" 400 4 "$gait_run/CANCEL"
```

The journal directory must be fresh for `init`. `advance` resumes committed
checkpoints and preserves trials. Creating `CANCEL` stops at an action boundary.
Preparation hashes input artifacts; each Rust journal binds the runtime source
identity, full scene/configuration, seed, task and candidate settings. Do not
regenerate the spec over an active experiment. Change horizon in a separate run.

For a new horizon or timestep, use a fresh output directory:

```sh
node examples/full-robot/measured-actuator-integration/gait-generation/prepare_search.mjs 10 NEW-DIRECTORY
node examples/full-robot/measured-actuator-integration/gait-generation/prepare_search.mjs 2 ANOTHER-DIRECTORY - 2
```

An optional third argument supplies a JSON object with the selected `pace_scale`
and `tracking_gain`. The final argument divides the nominal timestep by 1 or 2.
Preparation refuses to overwrite existing experiment inputs. Both cases still
need their own `init`, then `advance` with a trial budget of 1 for a single
comparison. The nominal ten-second and two-second half-step experiments are complete.
The numerical protocol was saved before starting the half-step evaluation.

The first four-trial pilot is complete. All four finished 2 s without falling:

| Pace multiplier | Tracking gain | Net travel | Speed over 2 s |
| --- | --- | --- | --- |
| 1.0000 (baseline) | -0.4566 | 0.212443 m | 0.106221 m/s |
| 0.7462 | 0.0816 | 0.228120 m | 0.114060 m/s |
| 0.9985 | 0.6662 | 0.194422 m | 0.097211 m/s |
| 0.3387 | -0.6939 | 0.251633 m | 0.125817 m/s |

The last candidate improves short-window travel by 18.45%; it is selected for
longer validation, not accepted as a sustained improvement. Its largest endpoint
joint-tracking error is 12.13°, versus baseline 61.71°. These are endpoint errors,
not peaks or time averages. The ten-second baseline completes without falling:
0.816106 m, or 0.081611 m/s. The selected candidate also completes ten seconds
without falling: 1.346374 m, or 0.134637 m/s, a 64.98% improvement in net travel
under matched conditions. Its largest endpoint tracking error is 13.65°.
See `ten-second-comparison.json` and `selected-10s/recording.json` for evidence
and the saved replay input. Its full ten-second replay matches the saved endpoint
state, task result and runtime input recording exactly. Its ten-second half-step
check also passes: travel differs by 0.451 mm, body position by 5.68 mm, and the
largest joint-angle difference is 0.154°. These horizons cannot
be compared directly with the historical 300 s speed.

The shared compiled-geometry audit reveals an important limitation: one foot's
sampled clearance never exceeds 1.01 mm. Greater net travel therefore does not
yet establish robust stepping. The largest sampled inter-link overlap is 0.044 mm.
The audit covers 501 poses, 20 ms apart, and does not certify continuous collision
freedom or manufacturing clearance. See `selected-10s/geometry-summary.json`.

The baseline half-timestep check passes its predeclared endpoint bounds: travel
differs by 0.465 mm, body position by 0.472 mm, and the largest actuated-joint
angle difference is 0.1323°. This establishes short-run numerical agreement only.
See `numerical-screening-protocol.json` and `timestep-comparison.json`.

A separate `full-authority/robot.rcad` increases the CAD controller command limit
from 350 to 1000 permille. All 114 geometry entries survive unchanged and the
profile round trip passes native validation. Physical export and unchanged-physics
checks pass. All four matched two-second cases finish without falls; the original
gait is fastest at 0.387144 m/s. It also completes ten seconds at 0.450412 m/s
(4.504124 m net travel). Its exact release replay and sampled geometry audit are
complete: each foot attains a peak clearance between 12 and 42 mm, and the largest
sampled inter-link overlap is 0.069 mm. The ten-second half-step check passes:
net travel changes by 2.98 mm, endpoint body position by 7.96 mm and the largest
actuated-joint angle by 0.238°. Continuous clearance remains unproven.

Two Bayesian follow-ups also finish. The best reaches 0.429551 m/s over two seconds
at pace 0.867202 and tracking gain -0.873099. Its ten-second validation reaches
only 0.160519 m/s, so it does not replace the original gait. Exact replay passes.
The full trace confirms actual slowing, not just a curved path: sampled path speed
drops from 0.442 m/s during the first two seconds to 0.106 m/s during the last two.
The original gait retains 0.487 m/s sampled path speed in that last window and is
completes a separate 30-second run in `full-authority-30s/` without falling:
12.653113 m net travel, or 0.421770 m/s. Its recording is saved; this longer horizon
does not yet have separate replay, geometry or half-step qualification.
This remains an uncalibrated simulation scenario;
loaded torque/current still require physical measurements.

The release build matches all 501 recorded frames/transitions of the 35% winner
and both ten-second endpoint records. It runs the matched captured case about
1.65 times faster, but ten simulated seconds still take roughly 214-216 wall
seconds in these exploratory measurements. This is not realtime or browser
qualification. See `release-performance.json`. Subsequent runs use
`target/release/examples/search_motion` after `cargo build --release --locked
-p sim-runtime --features bayesian --example search_motion --example run_environment`.

The preparer accepts an optional final base-spec path to extend another CAD
scenario. It preserves that scenario's physical model, parameterization and
controller, and verifies the historical command contract/prefix before extending
the episode. Existing experiment files are never overwritten.

`summarize_trace.mjs CAPTURE OUTPUT` aggregates saved Rust observations into
two-second motion windows, per-motor sampled tracking/saturation statistics and
conditional electrical predictions. It checks the sample grid, named actuator
bindings and agreement with native net distance. These are descriptive diagnostics,
not new acceptance gates: 20 ms samples can miss peaks, and imposed-supply energy
is a trapezoidal estimate rather than a battery-state observation. Both full-authority
ten-second captures have `trace-summary.json` reports.

The next priority is gait robustness under nonideal actuator conditions. Separate
CAD scenarios now cover estimated delay and internal gearbox play; battery/wiring,
motor variation and dynamic thermal integration follow. See [scenario plan](robustness/PLAN.md).

`results.json` summarizes committed checkpoints; `search.log` may contain newer
progress. Refresh with the following command, optionally appending a run folder:

```sh
node examples/full-robot/measured-actuator-integration/gait-generation/summarize_search.mjs
```
