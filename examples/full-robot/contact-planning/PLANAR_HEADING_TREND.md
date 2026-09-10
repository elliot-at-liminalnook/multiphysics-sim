# Separating repeated yaw motion from sustained turning

The first planar predictor was too sensitive to the fitting-window endpoints.
On the teacher's same 20-second capture, the 5–20 and 10–20 second fits predicted
300-second net speeds of 0.510723 and 0.295027 m/s. The actual 300-second result is
0.514338 m/s. Averaging interval heading changes telescopes to an endpoint
heading difference, so repeated yaw motion can masquerade as long-run turning.

The shared `PlanarTrendPrediction` now unwraps recorded headings and estimates
heading rate by least-squares regression against elapsed time. It also uses the
fitted mean heading at the end of the window for extrapolation. The existing
body-frame translation fit and exact planar integration remain unchanged. The
original method remains available and pinned for the running screen; this is a
separate, reproducible comparison.

Only past poses enter either fit. No future state, measured endpoint, guessed
contact pattern, heading penalty or actuator change enters the controller or
candidate objective. Future positions are evaluation labels only. The trend
is still an empirical constant-motion assumption with finite-window bias, not
a neural dynamics model or a physical bound.

## Matched predictions

| Case and fitting window | Original predicted speed | Trend predicted speed | Measured 300 s speed | Original endpoint error | Trend endpoint error |
| --- | ---: | ---: | ---: | ---: | ---: |
| Teacher,5–20 s |0.510723|0.512784|0.514338|11.230 m|1.284 m|
| Teacher,10–20 s |0.295027|0.508391|0.514338|160.302 m|12.187 m|
| Phase seed 0,5–20 s |0.000918|0.001277|0.001322|0.614 m|0.0155 m|
| Affine003,5–20 s |0.005339|0.004856|0.006103|0.248 m|0.374 m|

Speeds are in m/s. The teacher's two-window spread shrinks from 0.21570 to
0.004393 m/s. Improvement is not universal: affine 003's5–20 s endpoint error
increases, although its other windows improve. The teacher's 20–60 s endpoint
error also increases slightly, from 1.177 to 1.220 m. All 11 matched cases and both
methods are retained in `planar-heading-trend-v1.json`; no favorable-window
selection is hidden.

Five Rust tests pass, covering straight and turning trajectories, wrapped and
nonuniform heading observations, transformed origins and long clock offsets,
invalid data, closed circles, and repeated heading-observation error. The
repeated-error case verifies over 10x improvement in 300-second endpoint error;
it does not assert zero regression bias. Its initial draft included an
unsupported absolute 1 m expectation, which failed and was removed while retaining
the improvement and analytic checks. The original draft and failure are archived.
The unchanged endpoint method exactly reproduces all 11 prior fitted twists and
predicted poses after the new code is built.

## Using the model to choose experiments

`rescore_planar_speed_screen.mjs` refits completed screen data without resimulating
physics. It preserves failed prefixes and puts estimated 300-second scores in a
separate optimizer context, explicitly marking them as unqualified for full
physical speed. They cannot be mixed with measured full-duration outcomes.
The maximum prediction across the declared windows remains an optimistic
selection heuristic; it is not a calibrated confidence bound.

The rescorer passes a two-candidate workflow check using the completed short
smoke data. The 16-candidate physical batch now completes: ten new candidates
survive the 20-second prefix and six fall. Together with the baseline, eleven
completed prefixes feed the GP; failures remain excluded and preserved. None
of the new candidates is predicted faster than the baseline.

Constrained LogEI with a constant-mean Matern-5/2 GP, normalized inputs and seed
2401 selects a new eight-parameter proposal. Its full 300-second physical test
is running at `runs/planar-guided300-v1`. The proposal is an exploration choice,
not a qualified physical improvement.

`run_adaptive_planar_speed.mjs` automates the next twelve sequential proposal,
20-second simulation, heading-trend fit and GP-update steps. It uses the exact
seed-observation predictor and physical recipe, and keeps estimated scores in
their separate context. STOP requests cancellation between evaluations. Its
first proposal and materialized physical recipe exactly reproduce the standalone
full-horizon candidate. Each later proposal consumes the accumulated outcomes.
The live root is `runs/adaptive-planar20-v1`; the recipe is
`runs/adaptive-planar-inputs-v1/experiment.json`.

No model-guided speed gain or sample-efficiency gain has yet been established.
`planar-guided-search-v1.json` records the completed screen and pending tests;
`planar-heading-trend-evidence-v1.json` preserves terminal evidence and immutable
live inputs, excluding growing logs and outputs.

The first adaptive prefix now completes without a sampled fall at 0.008132 m/s;
its estimated 300-second speed is 0.001625 m/s, so it is not a predicted gain.
Its full-duration run remains active to test the extrapolation. The next GP
proposal demonstrably consumes this new observation: eighteen accumulated rows,
twelve completed-prefix training rows and six preserved failures. The feedback
check verifies that future position labels are absent from screen requests.
