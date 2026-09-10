# Surface contact and schedule-free initial guesses

Previous goal turn made progress in commit `0da0a7f`. This turn replaces the
planner's one-point-per-foot approximation with the exact compiled foot contact
sample locations used by the shared detailed runtime. It does not change CAD,
runtime contact physics, the old browser bundles, or the active speed goal.

Subsequent [runtime validation](RUNTIME_TRACKING.md) found and corrected material-pair friction/dissipation mismatches and demonstrated excessive sliding. The planning gates below alone do not qualify these paths as gaits.

## Physical correction

`tracking::compiled_surface_markers` validates the CAD identity and explicit link
selection, then exports all compiled contact vertices in link-COM coordinates.
`export_surface_markers` uses that shared API. The four selected CAD foot links
produce 96 points. These remain sampled geometry, not an exact CAD surface.
The runtime uses floor stiffness **per sample**, so the planner retains that
interpretation. The smooth normal law and pointwise friction still differ from
the runtime's friction patch, and no model equivalence is asserted.

The initial pose is the first knot in the earlier audited path with zero sampled
self-overlap (knot 1). Only its pose is retained. The base moves upward by the
explicitly recorded .157698447 mm, using the independently measured minimum
floor clearance, to start 20 µm above the plane. CAD and floor heights are not
edited. All initial velocities are zero. An independent audit confirms zero
self-overlap and the requested clearance at all 13 repeated initial poses.
The planner's minimum surface gap agrees with the runtime geometry audit.

A tilted four-point analytic test checks edge forces, nonzero moments and exact
agreement with runtime sample clearances. It also rejects wrong CAD, duplicate
links and missing links. All four contact-implicit integration tests pass,
including the existing gravity/free-flight, unprescribed touchdown and registry
checks. The release examples build successfully.

## Optimization evidence

Both initial guesses have the same fixed initial state, .25 m/s +45° task,
objective, bounds, 12 intervals/.39608749 s horizon, actuator envelopes and
contact continuation. Only future position guesses differ. The stationary guess
repeats the initial pose; the translating guess moves the body along the task
reference while holding joint coordinates constant. Neither prescribes foot
contacts, lift events, gait phases or a joint-motion cycle.

The contact schedule is k=2,000/10,000/50,000/200,000 N/m per point and smoothing
3/1/.1/.01 mm, with 24 iterations per stage. Each final model is then continued
for up to 100 iterations with the same weights, bounds and physical gates.
Hessian scaling remains 1/4. These are numerical search budgets, not speed limits.

| Trial | Planned mean diagonal rate m/s | Force error N | Moment error Nm | Minimum torque margin Nm | Planning gates |
| --- | ---: | ---: | ---: | ---: | --- |
| Stationary seed, continuation | .046022 | .059439 | .026352 | .103474 | Fail |
| Stationary seed, refinement | .055747 | .020972 | .008915 | .095400 | Pass |
| Translating seed, continuation | .184060 | 2.116891 | 1.902176 | -.553647 | Fail |
| Translating seed, refinement | .183028 | .009410 | .004334 | -.007529 | Pass within declared tolerance |

The stationary refinement passes the final planning gates and shows zero sampled
interlink overlap over 61 poses. Its maximum floor penetration is .114164 mm,
below the 1 mm development threshold. The terminal diagonal velocity is about
.202 m/s, substantially above its average during this short start from rest.
Thresholded per-foot forces switch without a supplied schedule. These are force
diagnostics, not proof of completed swing/lift events or a repeatable gait.
Its cost continues falling at the iteration limit; it is not stationary.

The translating continuation has zero sampled self-overlap and .152175 mm
maximum floor penetration, but its balance and torque failures disqualify it.
Its higher displacement rate is not a measured speed record. All independent
audits reproduce final planning reports exactly. The summaries include per-foot
forces, coordinate ranges, terminal velocity and exact matched-input checks.

The translating refinement retains .183028 m/s mean planned motion while passing
the declared planning tolerances. Its torque margin is slightly negative
(-.007529 Nm versus the -.01 Nm gate), not strictly within the estimated motor
envelope. This must remain visible in further validation. Its 61-pose audit shows
zero sampled self-overlap and .128742 mm maximum floor penetration. Terminal
diagonal velocity is only .071302 m/s: this short accelerating/decelerating path
does not establish sustained speed. It also hits the iteration limit, with no
claim of convergence or global optimality. No path has run in the detailed
controller/runtime yet and none has replaced the .21 browser preview.

## Next requirements

The improved geometry and feasible local path justify extending the horizon and
testing detailed runtime tracking. The .396 s path cannot establish sustained
speed, periodicity, terminal viability or responsive steering/reverse/stopping.
Before larger horizon searches, reuse unchanged local time-stencil evaluations:
finite differences currently recompute every frame although each residual frame
depends on at most three position knots. That optimization must preserve exact
uncached residuals and retain the independent audit path. Stronger constrained
balance handling remains an option if necessary; current feasibility is no longer
blocked solely by the old missing surface contacts.

Retain the .25 m/s task target and explore multiple numerical initial guesses
without imposing a contact sequence. Validate any retained motion in the detailed
shared runtime, including collision, slip, timestep sensitivity, actuator tracking,
command loss and actual browser performance before replacing the experimental
.21 preview. No global physical or calibrated hardware speed maximum is proved.

## Replay

`surface-selection.json` declares the four CAD links and frame. Run
`export_surface_markers SCENE surface-selection.json` to produce markers, then
`prepare_surface_trial.mjs` to prepare the stationary recipe. It derives only
configuration from existing Rust geometry evidence and refuses to overwrite.
Each retained recipe includes all input positions, so direct optimizer replay
does not require rerunning preparation. Use `optimize_contact_implicit SCENE
surface-markers.json RECIPE` and `audit_contact_implicit SCENE surface-markers.json
RECIPE RESULT 5`, then `summarize_surface_trial.mjs NAME`.

`evidence-v2-index.json` records current source, input, binary and result identities.
Older manifests remain commit-scoped and unchanged. The source scene remains
durably recoverable through the existing speed-ceiling evidence archive chain.
