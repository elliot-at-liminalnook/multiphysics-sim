# Landing error at recorded plan time

The support-qualified return establishes sustained sampled support, but says
nothing about where a foot landed. The shared `motion_tracking::compare_motion`
diagnostic now compares each physical frame with the geometric plan at its
recorded reference time. A pause repeats the reference pose while physical
motion continues. Final holding samples remain visible. The tool refuses missing
exact reference samples, backwards/accelerated phase clocks, incomplete endpoints
and mismatched CAD/trajectory/initial-placement metadata. It does not fit a time
warp or interpolate the mechanism to make an error smaller.

The report separates world-marker error from marker error in the moving body's
axes. The former includes body drift and is relevant to a foothold; the latter
isolates motion relative to the body. They are different vectors in different
frames and their magnitudes must not be subtracted to infer body drift.

## Measured results

Both saved support-checkpoint runs were compared with the original geometric
plan, using its CAD-derived foot marker offsets and exact samples.

| At the 1.110 s support qualification | 0.25 ms step | 0.125 ms step |
| --- | ---: | ---: |
| Moving foot world XY placement error | 2.016 mm | 2.153 mm |
| Moving foot full world position error | 2.037 mm | 2.173 mm |
| Moving foot position error relative to body | 1.140 mm | 1.268 mm |

At the end of the 2 s experiment, world XY error is still 1.112 / 1.245 mm.
The maximum full world error during the entire motion is 3.734 / 3.723 mm,
occurring during swing at 0.690 s. The support feet each remain below 0.88 mm
maximum sampled world-position error. These are marker-to-plan distances, not
contact penetration; marker location and the lowest collision contact can differ.

The results establish why support qualification alone cannot certify a landing.
They do not establish that 2 mm is unacceptable for every task: allowable error
must come from the actual foothold margin, with reserves for sensing and hardware
uncertainty. The next controller experiment needs explicit task-space placement
references and budgets, including body motion, before repeating this into steps.
The current motion returns the foot to its original intended location; it is
not yet a forward walking stride.

## Reproduction and compatibility

```sh
cargo test --locked -p sim-runtime --test motion_tracking --test tracking
cargo build --locked --release -p sim-runtime --example compare_motion
target/release/examples/compare_motion runs/full-robot/learning/landing-checkpoint/execution.json runs/full-robot/learning/hip-grid-plan-16mm-lift-5mm.json examples/full-robot/foot-markers.json 'Robot | Chassis and hip mounts' > runs/full-robot/learning/landing-checkpoint/tracking.json
target/release/examples/compare_motion runs/full-robot/learning/landing-checkpoint/refined.execution.json runs/full-robot/learning/hip-grid-plan-16mm-lift-5mm.json examples/full-robot/foot-markers.json 'Robot | Chassis and hip mounts' > runs/full-robot/learning/landing-checkpoint/refined.tracking.json
```

Report version 2 adds per-sample actual/reference/error vectors in both frames.
Each marker summary now includes a `frame` field (`world` or `body_relative`);
consumers of the former body-only output must select `body_relative` explicitly.
RMS weights reporting samples equally, including waiting and final hold, so it
must not be compared directly with a shorter experiment's RMS without matching
the interval. Existing fixed-reference captures still align on simulation time.

Three focused motion tests cover pause/final-hold alignment, visible body drift
and real linkage error, fixed-reference compatibility, and invalid provenance or
coverage. The five existing tracking tests also pass. The new tests are included
in browser CI's native checks; no remote CI run is claimed.

This changes offline measurement, not physics or controller execution. The tested
support-qualified WASM preset remains available in the local viewer. Its previous
native/browser validation remains the evidence for that unchanged controller.
`phase-tracking-status.json` records the diagnostic inputs, code and result hashes.
