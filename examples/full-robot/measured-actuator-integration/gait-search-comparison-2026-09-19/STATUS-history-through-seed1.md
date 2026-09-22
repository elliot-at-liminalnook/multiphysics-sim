# Active goal status

The goal remains active; the optimizer comparison and finalist validations are
not complete. This turn made implementation progress and produced verified
qualification plus a successful first dynamic comparison trial.

- Shared Rust preparation, CMA-ES, comparison accounting and capture gates built.
- 24 regression tests passed; final widened phase-template test passed again.
- Final source captured in `library-source.tar.gz`; native comparison host and
  qualifier retained as `comparison-host.bin` and `qualifier.bin`.
- New 80% cadence baseline passed all unchanged reduced/detailed fidelity gates;
  measured stepping speedup 1.1880794983x.
- Comparison running under tool exec session **63943**. Poll that handle before
  deciding whether it stopped; do not restart based solely on this file.
- Progress log: `/tmp/gait-comparison-progress.log`; stdout:
  `/tmp/gait-comparison-result.log`.
- Launch command: `RAYON_NUM_THREADS=1 target/release/examples/compare_gait_search
  examples/full-robot/measured-actuator-integration/gait-search-comparison-2026-09-19/comparison-config.json
  examples/full-robot/measured-actuator-integration/gait-search-comparison-2026-09-19/comparison
  132 examples/full-robot/measured-actuator-integration/gait-search-comparison-2026-09-19/CANCEL`.
- First sealed receipt: `comparison/2301-Bayesian-000/trial.json`. Baseline passes,
  signed speed 0.10010800496 m/s; 98.78 wall seconds including preparation/capture.
  CMA-ES's matched baseline is running at this observation.

## Next actions

1. Monitor the confirmed comparison process and inspect sealed receipts. Keep
   all failures. If a host failure occurs, diagnose it against the actual process
   and partial attempt directory; never silently overwrite/restart evidence.
2. Complete all 33 attempts for each algorithm and both seeds. Count actual
   Bayesian acquisition versus LHS/local-bootstrap proposals separately. Do not
   claim a meaningful Bayesian comparison if no Bayesian proposals were made.
3. Select eligible finalists per algorithm/seed and run detailed 15-second
   validation plus nominal 30-second episodes. `sensitivity-plan.json` explicitly
   lists correct registry-family resistance/torque/back-EMF and floor-friction
   paths plus per-servo supply-voltage overrides. Preserve CAD source and receipts.
4. Finish geometry/contact/tracking and timestep checks; save plots/tables for
   speed versus cumulative wall hours, failure counts and scenario performance.
5. Audit the full four-part goal before marking it complete. No goal status
   update has been made because the comparison and finalist evidence are pending.

Avoid editing library source while collecting these comparisons: qualification
and replay are bound to the archived source identity. CLI-only helpers can call
existing shared APIs; adding new library APIs later requires explicit source
provenance and appropriate matched revalidation. Hardware remains untouched.

## Validation tooling prepared during the live comparison

The running comparison still uses the archived, tested executable/source above.
New offline helper source has now been authored; it is **not built or tested
yet**, to avoid running a build workload alongside the timed comparison:

- `experiment_variants` shared primitive and `prepare_validation_case` CLI:
  explicit voltage/motor/friction numeric edits, original-value receipts,
  extended horizons with an explicitly renewed command-sequence tail.
- `report_gait_search` CLI: uses shared accounting and capture gates to verify
  saved scores, select finalists and write tables plus speed-versus-wall-time SVG.
- `tests/experiment_variants.rs`: source-preservation, override-receipt,
  horizon/sequence and malformed-edit cases; pending execution.

These additions change the on-disk library source hash. **Do not rebuild and
resume the live comparison with the new source.** Use the retained
`comparison-host.bin` if a completed process later needs continuation, and keep
its original config/identity. `run-environment.bin` and `audit-geometry.bin` now
retain the same search-era runtime for finalist simulation and geometry audits.
The later helper build must record its separate preparer/report identity; its
output experiment specs can execute through the archived physics runtime.

Once the comparison ends, build/test the helpers, then execute the actual
finalist scenarios. The helper code being present does not satisfy the goal's
validation requirement. `live-summary.json` is an observed partial count only;
trial receipts and the confirmed live process remain authoritative.

## Finalist checks prepared before validation

`VALIDATION.md`, `motion-gates.json` and `geometry-gates.json` now state the
long-horizon, sensitivity, geometry and timestep procedure. Voltage cases set
10.5/12 V exactly. Geometry evaluation now requires every capture timestamp,
preventing a shortened audit from passing. The report plot uses step changes
for incumbents. New helper source remains unbuilt/untested pending completion
of the timed search. No finalist validation has run. See `live-summary.json`
for the latest partial receipt count; the live comparison remains session 63943.

## Deferred helper checks queued

Search session **63943** was confirmed live again, OS PID **73427**. First-seed
Bayesian attempt 20 completed (six successful captures including baseline), and
CMA-ES attempt 20 is simulating; no baseline improvement yet.

Shared `experiment.refine_timestep` and thin `prepare_timestep_case` now prepare
matched numerical-refinement experiments without altering commands, model or
elapsed horizon. Four variant tests and two geometry tests are authored; new
tests remain unexecuted. Rust formatting checks and shell syntax checks passed.

Deferred check session **97621** is live, waiting for PID 73427 to exit. It runs
`check-validation-tools-after-search.sh`, first verifying all 132 declared trial
receipts, then focused helper/fidelity/search tests and release helper builds.
Only after those pass does it generate `search-report/`. It does not restart any
search or run finalist simulations. **Poll both sessions; do not duplicate this
build/report job.** If search or checks fail, inspect the real exit and retained
logs before acting. Keep helper source stable while this queued check is pending.

Next actual physics work remains selection/deduplication of finalists followed
by every case in `VALIDATION.md`, using archived physics executables. Numerical
endpoint evaluation and final cross-scenario reporting still need completion.

Read-only sensitivity preflight completed against the starting detailed spec:
all numeric case paths exist, all 12 motors bind the altered family, and no
per-unit deviations override it. `validation-preflight.json` records before/after
values and explicitly does not claim executed runtime sensitivity. Both live
sessions were polled again; helper checks still await search termination.

## First short-run improvement observed

CMA-ES seed 2301 attempt 23 reached 0.105864911947 m/s (+5.75% relative
to the 0.100108004964 m/s baseline). Worst motor RMS was about 1.90 degrees
and peak 3.94 degrees over 3.6 seconds. `first-improvement.json` points to the
full trial and evaluation. This does not establish long-run or physical-variation
robustness; full-budget comparison and finalist validation remain required.

Bayesian seed 2301 reached nine successful observations after attempt 25.
Attempt 26 is the first sealed `bayesian_log_ei` proposal; it failed bounded
kinematic preparation and retained its full attempt/cost. This is actual
Bayesian acquisition evidence, distinct from local feasibility bootstrapping.
Seed 2302 and finalist validation remain outstanding.
