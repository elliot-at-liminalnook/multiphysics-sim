# Predicting long-run displacement from short physical runs

The speed objective is still full-horizon net displacement divided by duration,
without sampled falls. The new screen estimates that same displacement from a
short prefix to prioritize expensive simulations. It does not qualify a gait or
replace the full-duration evaluations, and adds no heading/slip/tracking penalty.

`sim-domain-control::planar_prediction` fits a constant body-frame planar twist
from timestamped x/y/heading observations. It takes the SE(2) logarithm of each
relative pose, sums the integrated twists, and divides by elapsed time. This
couples translation with rotation and handles wrapped heading measurements.
Future endpoints use the existing shared `stepping::advance_planar` integral.
All times, positions, velocities and headings retain seconds, metres, m/s and
radians. Samples must be frequent enough that heading changes by less than pi
between observations; rotations hidden between samples cannot be recovered.

This is an empirical kinematic surrogate. It contains no new contact dynamics,
actuator approximation or fall prediction, and is separate from the learned
20/100/200 ms dynamics models. Its constant-motion assumption can fail during
transients, changing contact patterns or long-run drift. Different fitting
windows are retained to expose sensitivity, not treated as calibrated confidence
intervals. Future measured positions enter scoring only, never fitting.

## Completed checks

The Rust tests recover straight motion, both turn directions, lateral motion,
near-zero turning, wrapped headings, nonuniform sample intervals and transformed
world origins. An analytic circular trajectory has substantial motion but zero
net displacement after one revolution. Invalid times, nonfinite data and backward
prediction are rejected. Three tests pass; the release CLI build completes.

The same 5–20 second fitting window gives these nominal 300-second estimates:

| Candidate | Predicted net speed | Measured net speed | Endpoint error | Straight-line endpoint error |
| --- | ---: | ---: | ---: | ---: |
| Teacher | 0.510723 m/s | 0.514338 m/s | 11.230 m | 34.961 m |
| Phase seed 0 | 0.000918 m/s | 0.001322 m/s | 0.614 m | 15.549 m |
| Affine evaluation 003 | 0.005339 m/s | 0.006103 m/s | 0.248 m | 32.421 m |

The latter two captures are new 20-second input replays with the same seed 1901,
full-episode configuration and action prefixes as their completed 300-second
runs. Their replay inputs are authored recipes, not claimed prior observations.
Only the requested prefix completes; the recorded full episode remains 300 s.
The teacher uses its existing 60-second native capture. The measurements and
all nine window comparisons are in `planar-speed-prediction-v1.json`.

Using teacher seconds 20–60 improves its 300-second estimate to 0.513235m/s,
with 1.177m endpoint error versus 27.753m for linear extrapolation. Earlier windows
have materially larger endpoint errors: this is not numerical convergence or
proof that the model generalizes to new gaits.

Phase seed0 travels along its sampled path at 0.322m/s during seconds 5–20,
while turning at about −0.670rad/s. Affine003 travels along its sampled path at
0.146m/s and turns about 0.0399rad/s. These prefixes explain why appreciable
motion can accompany very little net progress; no path-length reward is added.

## Model-guided experiment selection

`examples/interactive/run_planar_speed_screen.mjs` reuses the existing Rust
trajectory transformer, seeded design generator, environment replay and fitted
planar predictor. It preserves the CAD/world/actuator definitions and the declared
300-second task, simulates 20 seconds, then predicts the 300-second endpoint from
both 5–20 and10–20 second windows. The larger predicted speed prioritizes further
simulation; it is an optimistic heuristic, not a probabilistic upper bound.
Physical falls and numerical failures remain recorded failed outcomes.

A two-candidate 2 s workflow check validates materialization, seeded proposals,
physical replay and prediction output. Its first version mislabeled prefix speed
by using the runtime's declared 300-second denominator. The corrected diagnostic
uses actual prefix duration; all 202 physical frames, task transitions and
predicted rankings remain exactly unchanged. Both versions are preserved.

The active batch is `runs/planar-screen20-v1`, specified by
`runs/planar-screen-inputs-v1/experiment.json`: baseline plus 16 Latin-hypercube
candidates, seed 2301, two at a time. It explores the same eight amplitude, lead,
command-speed and tracking-gain variables as the existing full-horizon search.
The baseline must reproduce 10.235359974362822m over 20 s within 1e-8 m before the
batch proceeds. The finite initial domain is not a physical limit. A STOP file
in the output directory requests cancellation between batches.

Only new promising proposals should proceed to full 300 s measurement and matching
fidelity checks. No model-guided speed or sample-efficiency gain has been shown.
The existing full-duration amplitude and phase searches continue independently.
`predictive-screen-study-evidence-v1.json` preserves terminal diagnostics and
immutable live inputs; growing outputs are excluded from that snapshot.
