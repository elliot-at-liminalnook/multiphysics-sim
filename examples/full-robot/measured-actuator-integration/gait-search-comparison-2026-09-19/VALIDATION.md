# Finalist validation procedure

Prepared while the matched search is running, before finalist validation runs.
This document and the case files are a procedure, not passing evidence.

## Selection and execution

After all four algorithm/seed groups complete their declared budgets,
`report_gait_search` recomputes saved capture scores and selects each group's
best eligible trial. Keep ties deterministic by retaining the earlier attempt.
Deduplicate identical **detailed experiment specifications**, retaining every
algorithm/seed alias; equal speeds alone do not establish identical candidates.

Before long-horizon validation, recheck each unique finalist with the archived
qualifier and the unchanged `finalist-profile.json` extracted from the comparison
config. Its existing `prepare <detailed-spec> <profile> <fresh-directory> --fresh`
and `qualify <directory>` commands save matched detailed/reduced captures,
identity, mechanical/electrical error budgets and stepping performance. Run with
`RAYON_NUM_THREADS=1` and retain rejected qualification results as well.

For speed-gain claims, compare equal-duration detailed episodes. Include nominal
15-second and 30-second baseline controls; reuse them if the original baseline
is selected as a finalist. A longer episode's smaller startup fraction must not
be counted as an optimizer improvement over a 3.6-second baseline. These controls
do not add optimizer attempts or change any search/acceptance limits.

Run every unique finalist through all seven `validation-cases.json` cases:
nominal 15 seconds; 10.5 V; 12.0 V; altered motor constants; lower floor friction;
the combined perturbation; and nominal 30 seconds. Each shorter sensitivity
case lasts 15 seconds. These are explicit sensitivity assumptions, not measured
confidence intervals. Supply voltage is held fixed in each voltage case; these
cases do not establish battery-sag accuracy.

`prepare_validation_case` preserves the original action prefix and explicitly
renews the packet sequence while holding the final command for the extension.
Keep its preparation receipt and source/prepared hashes. Execute specs through
the archived `run-environment.bin` to retain the physics implementation used by
the search. Record that executable's hash and the capture's runtime identity.
Offline preparers/evaluators have a newer source identity, which must be retained
separately. Never silently resume the search using rebuilt physics.

## Acceptance checks

`motion-gates.json` copies the search gates unchanged: complete episode, no fall
or body-floor contact, body up-z at least 0.9, per-motor RMS tracking at most
5 degrees and peak tracking at most 15 degrees. Speed remains signed displacement
along the declared forward direction divided by completed duration. Report every
case's speed and tracking, including rejected cases.

`geometry-gates.json` additionally requires each authored sliding foot to cross
up through 3 mm clearance at least twice after the first second, and maximum
sampled inter-link penetration of 1 mm. These geometric thresholds retain the
earlier contact-shape validation protocol. Run archived `audit-geometry.bin`
against each capture, then `evaluate_validation_capture` with both gate files.
The evaluator requires the actual capture-byte hash and every capture frame's
timestamp to match the geometry report. Sampled geometry cannot prove the
absence of collisions between recorded frames.

Also perform a nominal detailed timestep check with half the physics step,
twice the step count and twice the report stride, holding controller/task clocks,
actions, seed, model, initial state and episode duration fixed. Retain both
captures and their exact configuration differences. Retain the earlier numerical
budgets: net-distance difference <= 5 mm; endpoint body-position difference <=
10 mm; endpoint body-up-z difference <= 0.01; endpoint actuated-joint difference
<= 1 degree. Both runs must independently pass motion and geometry checks.
Failure remains a numerical-validation failure; do not loosen budgets afterward.

Use `prepare_timestep_case` for this refinement and
`evaluate_timestep_captures` with `numerical-gates.json` to evaluate it. The shared
`experiment.evaluate_timestep` primitive checks runtime/model/task/seed identity,
matching initial state, frame clocks and reconstructed held actions before
applying the endpoint budgets. It preserves exact configuration differences and
the CLI records both capture hashes and the separate evaluator identity. Both
captures must still independently pass motion and geometry acceptance.

## Search interpretation

Failures consume the declared attempt and reserved simulation slot. Report both
reserved and actual simulated seconds, failure reasons, and cumulative wall cost.
The incumbent plot changes only on completion of a trial, not gradually between
trials. Two optimizer seeds are descriptive evidence, not a general ranking.

Count `bayesian_log_ei` proposals separately from initial Latin-hypercube and
local-feasibility-bootstrap proposals. A group that never reaches Bayesian
acquisition does not demonstrate Bayesian optimization performance. Preserve the
original study and, if needed, declare a matched extension before continuing;
do not rename bootstrap samples or exclude their costs.

## Pending implementation verification

The validation preparer, geometry evaluator and report helper are authored but
have not yet been built or tested. Build and run their focused tests after the
timed search finishes, to avoid adding a competing build workload. No finalist
capture or passing long-horizon/robustness result exists yet.
