# Active goal status

The four-part goal remains active. First optimizer seed is complete; the second
seed, a declared independent-seed extension, and every finalist validation remain
outstanding. No hardware was used.

## Current authoritative state

- Comparison process confirmed live: tool session **63943**, OS PID **73427**.
- Deferred extension/check process confirmed live: tool session **74523**.
- Earlier deferred sessions 97621 and 41260 were deliberately terminated while
  waiting, before builds/tests. Do not restart them.
- Both second-seed baselines completed at exactly 0.100108004964 m/s, matching
  both first-seed baselines. Latest log confirms 29 Bayesian and at least 28
  CMA-ES attempts completed for seed 2302; attempt 28 for CMA-ES is running.
- The original study retains its 132-attempt budget. A separate 66-attempt
  paired extension gives 198 total attempts; all failures and costs are retained.
- Progress: /tmp/gait-comparison-progress.log; stdout:
  /tmp/gait-comparison-result.log. Trial receipts are authoritative; logs and
  live-summary.json are observations only. Poll the tool handles before declaring
  either process stopped. Never restart because a log or observation is stale.

## Completed first seed: 2301

| Method | Attempts | Failed | Best m/s | Improvement | Wall minutes | Gain m/s per wall hour |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Bayesian | 33 | 21 | 0.101825562 | 1.72% | 24.219 | 0.004255 |
| CMA-ES | 33 | 16 | 0.105864912 | 5.75% | 33.184 | 0.010409 |

Both were charged 118.8 simulation seconds, including all failures. Actual
simulated time was 43.2 s for Bayesian and 61.2 s for CMA-ES. Bayesian made
7 actual log-EI proposals after 9 Latin-hypercube and 16 local-bootstrap
proposals plus the common baseline. Do not label bootstrap as acquisition.

Best candidates are Bayesian attempt 29 and CMA-ES attempt 23. Worst RMS/peak
tracking was approximately 1.46/3.19 degrees and 1.90/3.94 degrees respectively.
These are **3.6-second reduced-model search results**, not established robust
gaits. seed-2301.summary.json records the complete first-seed receipt summary;
full independent capture-score recomputation is queued after the entire study.

## Frozen search identity and running jobs

The source used to build the live search is archived in library-source.tar.gz.
Search host/qualifier are comparison-host.bin and qualifier.bin. Frozen physics
hosts run-environment.bin and audit-geometry.bin are retained for finalist runs.
Their embedded source-hash constants match the search source; verify emitted
capture runtime identities when executing finalist cases as well.

The initial baseline passed all declared detailed/reduced fidelity gates, with
1.1880794983x stepping speedup. Evidence is qualification/qualification.json.
Frozen-source checks: 24 regression tests passed; widened phase-domain test
passed again. Read regression-tests.log, domain-regression-test.log and build.log.

**Current library source differs from the frozen search source.** New offline
validation/report APIs have been authored. Do not rebuild and resume this study
with new source. If continuation becomes necessary after a confirmed terminal
process, use the archived host and identical config/identity, and diagnose any
partial attempt before acting.

Deferred session 74523 runs finish-comparison-and-checks.sh. It waits for
PID 73427 to exit, verifies the original 132 receipts, runs the declared paired
extension with archived comparison-host.bin, and verifies its 66 receipts. It
then runs focused seed/variant/geometry/fidelity/search tests and builds helper
CLIs. After success it creates search-report/ and independent-seed-report/ and
archives the helper source. It does not run finalists.
**Do not duplicate that build/report job. Keep helper source stable until it runs.**

## Seed overlap and declared extension

The archived Bayesian bootstrap/acquisition used master_seed + attempt. Seeds
2301 and 2302 therefore reused shifted streams: their nine successful initial
training observations and first acquisition proposal were identical. Evidence:
seed-overlap-evidence.json. Seed 2302 is retained as a diagnostic, not an
independent replication; its entire cost remains part of the study accounting.

seed-separation-extension.json declares seed 1002301 with 33 attempts per method,
the same archived executable, limits, model and horizon. Its additive seed range
does not overlap the originals. Independent comparisons use 2301 and 1002301.
The current reusable library now hashes master seed, attempt and proposal
purpose separately; tests are queued. This fix is not used to alter the frozen
timed studies. Extension logs: /tmp/gait-comparison-extension-progress.log and
/tmp/gait-comparison-extension-result.log.

## Authored but not yet tested

- experiment.prepare_validation: explicit physical sensitivity, extended horizon,
  source-action prefix preservation and explicit packet-sequence renewal.
- experiment.refine_timestep: physics-step refinement preserving command/task
  cadence, horizon and physical settings.
- experiment.evaluate_geometry: sampled clearance/penetration gates bound to
  actual capture hash and every capture timestamp.
- Thin preparation/evaluation CLIs and report_gait_search with stepwise incumbent
  plot, proposal-method counts, failure costs and finalist paths.
- Four variant tests and two geometry tests. Formatting/shell syntax passed;
  compilation and execution remain pending, to avoid competing with timed search.

## Required remaining work

1. Finish all 33 attempts per algorithm for seed 2302, then the declared paired
   extension for seed 1002301. Preserve the overlap diagnostic and every cost.
2. Observe queued test/build/report results and fix real failures. Preserve the
   separate preparer/evaluator identity and archived physics identity.
3. Select/deduplicate finalists by detailed-spec identity, retaining all
   algorithm/seed aliases. Recheck reduced-versus-detailed behavior for finalists.
4. Execute every case in validation-cases.json: nominal 15 s, lower/higher fixed
   supply, motor-constant/resistance changes, lower friction, combined changes,
   and nominal 30 s. Use frozen physics hosts and retain all rejected captures.
5. Complete geometry/contact/tracking and half-timestep checks in VALIDATION.md.
   Numerical endpoint evaluation is authored but untested; cross-scenario reporting remains to finish.
6. Verify report plot/artifacts and audit all four goal requirements before
   marking the goal complete. No final robustness or sim-to-real claim yet.

Read-only validation-preflight.json confirms numeric case paths and that all
12 motors bind the altered family with no per-unit deviations. It is configuration
inspection only. Voltage sensitivities are fixed-voltage cases, not battery sag.
Earlier chronological notes are preserved in STATUS-history-through-seed1.md.

## Numerical endpoint evaluator added during seed 2302

The previous deferred checker (97621, PID 74066) was deliberately terminated
while waiting, before any build/test started. Search session 63943/PID 73427
was not interrupted. Checker 41260 was subsequently replaced while still waiting
by session **74523**, which includes `evaluate_timestep_captures` in its build.

Shared `numerical_validation` uses existing capture validation and command
reconstruction; only physics step/count/report stride may differ. It rejects
model/runtime/seed/input/start-state mismatches, preserves differences and checks
completion plus explicit body-distance/position/up-z and actuated-position
budgets. Two additional fidelity integration tests are authored. Formatting and
shell syntax passed; compilation and execution are still pending.

The second-seed broad initial sampling completed without a better candidate.
Poll current receipts for updated counts, and do not duplicate deferred checks.

`finalist-profile.json` now retains the exact search profile for matched
detailed/reduced finalist requalification with the archived qualifier. It keeps
detailed motor dynamics and the original timestep, simplifying mechanism-solver
work only. VALIDATION.md also requires equal-duration nominal baseline controls
(15 and 30 seconds) for gain claims; reuse existing finalist runs if identical.
This prevents startup/horizon differences from masquerading as optimizer gains.
