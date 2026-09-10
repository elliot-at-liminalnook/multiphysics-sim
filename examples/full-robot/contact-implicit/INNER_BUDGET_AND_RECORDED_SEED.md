# Inner solve budget and a recorded gait warm-start

More inner iterations do not resolve the fast motion's failures. A separate
recorded-gait warm-start survives its first balance fit with passing dense
sampled slip, torque, penetration and collision checks, but still fails force
and moment balance. It is a more useful starting point, not a qualified new
controller or a faster runtime gait.

Two distinct questions follow the failed first inequality experiment: whether
short inner solves caused premature penalty escalation, and whether the local
search was trapped near an intrinsically poor slipping motion. These experiments
keep the speed-improvement goal intact; a lower-speed warm-start is preparation
for subsequent speed continuation, not a replacement objective.

## More inner work at the original planned rate

`inequality32-inner30.recipe.json` is identical to `inequality32.recipe.json`
except provenance and the inner LM iteration cap, increased from ten to thirty.
It retains three outer iterations, the same penalty update controls, the same
144 check times and the same fixed +45° projected planned rate of 0.2009265099
m/s. The original inner solves all reached their caps with large gradients,
so they did not provide a converged minimizer before the penalty changed.

This comparison tests additional computation at each penalty, not a new
physical model. Ninety allowed LM iterations and thirty allowed LM iterations
are deliberately unequal budgets. Neither a lower objective nor a stationary
subproblem would prove a physical maximum or stable walking.

## Recorded low-slip motion

The existing `diagonal68-minimal-observations.native.json` capture is verified
against `contact-planning/runtime-evidence/evidence-v4-index.json` (SHA-256
`b7741bd17102a3381e81e5b35a79258150bc3adc11c0159f65106c7550775285`).
The source controller requests 0.0678067849 m/s with a 0.6597773086 s motion
period. Sampling starts at 2 s, during steady forward command, and covers one
period entirely before the stop command.

The existing shared Rust `sample_capture_seed` converts captured body poses
and independent joint coordinates into a numerical periodic seed. It linearly
interpolates the recorded samples, removes non-XY endpoint drift and recenters
XY. These samples become 32 cubic controls, so the resulting curve is smoothed,
not an exact replay. No contact flags, stance timings or force allocation enter
the planner. The original capture's controller is phase-based; using its motion
as an initial guess does not demonstrate discovery of a new contact family.

`prepare_recorded_periodic_trial.mjs` now has an explicit optional
`--fix-recorded-displacement` mode. It fixes the numerical cycle displacement to
the sampled record while keeping all other bounds strict. The source recipe's
reference XY origin was aligned with its existing fixed control gauge, avoiding
a 0.236/0.664 mm discrepancy between an old task reference and optimized control
origin. This is a declared flat-floor translation of the numerical reference,
not a widened joint limit or altered CAD property.

An initial attempt used an old sampler executable, which correctly rejected the
newer `periodic_collocation_phases` field. The sampler was rebuilt against the
current schema; the field was not discarded. The raw seed, original and aligned
sampling recipes, failure note and build identities preserve this preparation.

The dense seed audit has 512 physical samples and 513 geometry poses. It reports
2.5403% worst-foot loaded slip, no sampled overlap and 0.28690 Nm minimum torque
margin. However, force error is 15.7838 N and moment error 2.97179 Nm: runtime
motion is not automatically feasible under the smoothed planning curve/contact
approximation. The largest force error is mainly vertical. The -X foot's RMS
surrogate is 10.237%, despite actual slip of 2.5403%; this illustrates why the
surrogate target remains sufficient but not necessary and cannot replace actual
slip acceptance.

`recorded68-restore.recipe.json` starts from that same recorded seed and fixed
displacement. It uses one thirty-iteration inequality AL subproblem with initial
penalty one, before any outer escalation. Its grid is uniform with 128 times,
rather than retaining targeted nodes selected for the unrelated .201 motion.
The final independent audit uses 512 times. Physical properties, actuator bounds,
the actual slip gate and collision acceptance are unchanged.

## Final results

| Motion / dense audit | Planned rate m/s | Force error N | Moment error Nm | Minimum torque margin Nm | Loaded slip | Overlap µm |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Original fast AL, 3×10 inner iterations | 0.200927 | 0.200148 | 0.071828 | -0.002948 | 72.65% | 16.65 |
| Fast AL, 3×30 inner iterations | 0.200927 | 0.210106 | 0.071180 | -0.003452 | 72.18% | 16.60 |
| Recorded seed, before fitting | 0.067063 | 15.783751 | 2.971792 | 0.286896 | 2.54% | 0 |
| Recorded seed, one 30-iteration fit | 0.067063 | 0.210964 | 0.123757 | 0.220302 | 4.73% | 0 |

The longer fast search uses 103,451 evaluations rather than 34,500. All its
inner solves still hit their caps. Its small slip change is not a qualification
gain: force and torque are worse on the dense grid, and all previously failed
physical gates remain failed. This is evidence against simply spending more
iterations on this local initialization, not a proof that the speed is impossible.

The recorded-seed fit uses 34,488 evaluations and also stops at its iteration
cap. It reduces dense force error by about 98.7% and moment error by about 95.8%,
while retaining actual loaded slip below 5%, positive torque margin and zero
sampled overlap. Dense point penetration is 0.108705 mm. Force must still fall
below 0.05 N and moment below 0.02 Nm; their present values fail those gates.
These rates are displacement of planned curves, not new measured walking speeds.

The next useful continuation should preserve the recorded motion's favorable
slip/contact properties while improving its balance fit, then increase speed
and validate through the shared runtime. This evidence does not justify further
claiming progress from tiny changes around the 72%-slipping initialization.
Neither path establishes continuous collision clearance, runtime stability,
browser responsiveness or the robot's physical speed maximum.

## Reproduction and verification

All optimizations use the unchanged executable and sources identified by
`inequality32-build-identities.json` from commit `d0b1743`. The separate sampler
rebuild is identified in `recorded68-sampler-identities.json`. No runtime or
browser controller is promoted by these offline experiments.

`verify_inequality_evidence.mjs` shares the existing final-report, physical
inequality, multiplier, budget, fixed-displacement and dense-frame checks across
experiments. Refactoring it leaves the original `inequality32-summary.json`
reproducible byte-for-byte. `summarize_inner_budget_and_seed.mjs` additionally
checks the matched inner-budget inputs, the recorded seed and strict bounds,
and the explicit grid-only change in the restoration configuration.

Run `optimize_contact_inequalities SCENE surface-markers.json RECIPE` for each
recipe, then `audit_contact_implicit SCENE surface-markers.json RECIPE RESULT
SUBDIVISIONS --pairs` with 4 subdivisions for coarse checks and 16 for dense.
The recorded initial seed uses `recorded68.initial.json` in place of an optimizer
result. Run the two summarizers with Node from the repository root.
