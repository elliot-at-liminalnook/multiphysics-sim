# One-second loaded pose hold

This experiment extends the retracted common-height startup in
`embedded-integration.md` to one second. All 12 registered servo controllers hold
the explicitly authored initial motor targets, with the same 1 kHz sampling,
11.1 V supply and imposed 293.15 K winding temperature. The rigid mechanisms,
backlash events, motor/driver laws and experimental regularized floor law remain
unchanged. It is a loaded hold, not a learned balance controller or a walking
experiment. The CAD properties and reduced contact/actuator behavior are not
hardware calibrated.

All four nominal timestep choices complete. Each servo executes exactly 1,000
ticks, and no internal part contact is reported in any accepted segment.

| Nominal step | Native time for 1 simulated second | Maximum sampled foot difference from 0.125 ms | Maximum sampled duty difference |
| --- | ---: | ---: | ---: |
| 0.125 ms | 45.02 s | comparison reference | comparison reference |
| 0.25 ms | 30.47 s | 0.228 mm | 0.0910 |
| 0.5 ms | 25.40 s | 0.447 mm | 0.1587 |
| 1 ms | 14.23 s | 0.497 mm | 0.1567 |

These are single exploratory CPU measurements, not paired matched-error
speedups. Desktop load/thermal behavior is uncontrolled, and larger-step runs
may overlap small diagnostic builds. The reference is the finest tested
one-second run, not a continuous-time or hardware truth. The reporting grid is
10 ms; the earlier 100 ms run used 2 ms samples and found a 0.235 mm difference
at 0.25/0.125 ms. A smaller observed maximum on a sparser grid is not improved
accuracy.

At 0.25 ms, the body's final downward displacement is 0.327 mm, maximum
horizontal displacement is 0.061 mm and maximum sampled tilt is 0.0230 degrees.
At 0.125 ms those maxima are 0.336 mm, 0.065 mm and 0.0202 degrees. Both finish
with approximately 39.007 N upward floor support, close to the 3.976 kg model's
weight. They remain nearly upright in this hold; this is not a static-equilibrium
certificate or evidence of disturbance recovery.

The feet themselves move by up to 0.947 mm (0.25 ms) and 0.973 mm (0.125 ms)
relative to their initial positions. This physical simulated displacement is a
different quantity from the difference between timestep choices. Settling and
regularized-contact creep must be assessed against the task and real-foot
behavior, not hidden by a small inter-run comparison. Largest differences in
10 ms integrated contact-force windows are approximately 0.0345, 0.0351 and
0.0329 N s for 0.25, 0.5 and 1 ms versus the 0.125 ms run. Duty and contact
impulse disagreement remains even where the body looks still.

No larger timestep is promoted on foot-marker agreement alone. Next acceptance
cases need commanded stepping, loaded tracking, coupled reach/clearance margins,
and balance/recovery checks; hardware measurements must calibrate uncertain
parameters. The full planning/teacher/student/robust-learning objective remains
unfinished.

## Reproduction and shared diagnostics

The CAD revision and regularized-floor scene derivation are recorded in
`embedded-integration.md`. Use the four `mechanical-servo-hold-1s-*.json` recipes
with the same release executable. For example:

```sh
cargo run --locked --release -p sim-runtime --example integrate_embedding -- runs/full-robot/learning/servo-regularized-floor.scene.json examples/full-robot/mechanical-servo-hold-1s-coarse.json > runs/full-robot/learning/servo-hold-1s-coarse.json
cargo run --locked --release -p sim-runtime --example summarize_hold -- runs/full-robot/learning/servo-hold-1s-coarse.json examples/full-robot/hold-1s-metrics.json > runs/full-robot/learning/servo-hold-1s-coarse-posture.json
cargo run --locked --release -p sim-runtime --example compare_embedding -- runs/full-robot/learning/servo-hold-1s-coarse.json runs/full-robot/learning/servo-hold-1s-refined.json examples/full-robot/foot-markers.json > runs/full-robot/learning/servo-hold-1s-comparison.json
```

`sim_runtime::posture::summarize_hold` is shared Rust code accepting ordinary
`EpisodeFrame`s. It uses the existing marker transform, declared local/world up
directions and explicit interval/sample coverage. It reports body tilt, vertical
and horizontal displacement, and each marker's maximum/final displacement from
the initial frame. It chooses no acceptance thresholds. The optional CAD hash
guard protects marker provenance; failed/incomplete intervals, invalid rotations,
ambiguous body names and inadequate sample coverage are rejected. Synthetic
translation/rotation and invalid-evidence checks run in CI; the module also
compiles for WASM. `hold-1s-status.json` records results, input/capture hashes and
source/binary snapshots for the experiment and reporting tool.
