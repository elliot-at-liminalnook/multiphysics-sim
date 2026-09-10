# Fixed-speed feasibility restoration and repeated residual targeting

Dense planning force error falls from **1.837 N to .319 N** at the same fixed
cycle displacement and duration. The matched continued search on the old grid
ends at .840 N. The targeted candidate passes the dense torque tolerance, but
still fails force, moment, slip and collision requirements. **No qualified gait,
new measured speed, online controller or physical maximum is established.**

## Shared method

`ContactImplicitPlanner::restore_feasibility` reuses the existing shared bounded,
damped Gauss–Newton least-squares solver. It selects normalized base-balance,
actuator-envelope violation and excess point-penetration rows from the shared
physical report. It removes their time-quadrature weights so each checked sample
has equal weight. Task tracking, effort and sliding-work costs are excluded from
this restoration objective, but all remain available in the independent report.
Contact and actuator physics are not recomputed in a separate implementation.
The shared partition validates report dimensions and sample times.

This is a separate feasibility-restoration phase, not a new implementation of
IDTO's dogleg or a complete composite-step SQP solver. The broader separation of
feasibility and task improvement is established in constrained optimization; for
example, [Chen, Qiu and Jiao (2013)](https://www.aimsciences.org/article/doi/10.3934/jimo.2013.9.391)
describe normal-only steps for feasibility and normal/tangential steps for
objective improvement. Our bounded damped-least-squares experiment does not
inherit that algorithm's convergence guarantees.

The native `restore_contact_feasibility` example consumes a strict, explicit
recipe. The robot-specific recipe fixes both cycle displacement variables at
the parent's values and retains its .39608749 s duration. There are 574 free
parameters in the 32-control periodic basis. The planned displacement rate stays
.2009265099 m/s; this is a trajectory property, not runtime walking speed.
CAD, contact law, target references, joint bounds and physical gates remain
unchanged. The first run uses the parent's 80 check times, 30 iterations,
200,000 evaluation budget, 1e-8 derivative probe, initial damping 1 and scaling
exponent 1/4. No derivative refinement was requested.

## Matched comparisons

The first pair starts from `equality32-refined-original.result.json` with the
same fixed bounds and least-squares settings. The full-objective control retains
time-weighted tracking, effort, sliding-work and physical penalty costs. The
restoration variant changes both residual selection and sample weighting;
this experiment does not isolate those two choices from each other.

The second pair starts from the first restoration result. One continues on the
same 80 times; the other retains all 80 and adds the worst missed violating
sample from each of 32 intervals, giving 112 times. Both have the same bounds,
initial curve and 30-iteration search settings. Every comparison below uses the
same independent 512 physical samples and 513 CAD geometry poses.

| Trial | Dense force error N | Dense moment error Nm | Dense minimum torque margin Nm | Dense loaded slip |
| --- | ---: | ---: | ---: | ---: |
| Parent equality search | 1.837033 | .679743 | −.038309 | 90.452% |
| Full-objective control | 1.323620 | .460352 | −.030566 | 87.150% |
| First restoration | .885016 | .368907 | −.034405 | 90.980% |
| Continued restoration, 80 times | .840282 | .399944 | −.048110 | 90.630% |
| Targeted restoration, 112 times | **.319302** | **.118825** | **−.000369** | 89.906% |

All four new searches reach the iteration limit, not stationarity or certified
feasibility. Objective costs are not comparable across different residual sets
or grids. Sampled planning errors alone are misleading: the continued run's
80-point force error is .080868 N, below the targeted run's 112-point .132058 N,
yet its dense force error is much worse.

The targeted dense torque margin meets the −.001 Nm tolerance. Its .113 mm
maximum sampled floor penetration meets the 1 mm gate. Force and moment still
exceed .05 N and .02 Nm; about 16.91 µm sampled interlink overlap and 89.9% loaded
slip also fail. The full-objective control has somewhat lower slip, which makes
clear that improved balance is not sufficient for an acceptable gait. Browser
presets and detailed runtime contact are unchanged.

The remaining worst force peak is at 4.642 ms, an unsampled time on the 112-point
grid. The next two largest peaks are also missed. Their locations and errors
are saved in `restore32-remaining-peaks.json`. Further residual-driven sampling
and motion-basis checks remain justified; the nonzero error is not evidence of
a physical speed ceiling. Sliding and interlink clearance must also be addressed
before any candidate is promoted or transferred to an online controller.

## Repeated numerical sampling and verification

`prepare_targeted_collocation.mjs` now accepts an existing nonuniform grid,
retains every current node, and adds one worst missed normalized force, moment,
torque or penetration violation per selected interval. Its input audit must be
uniform, denser and at the same period. It preserves the recipe type, so it can
prepare both full-objective and restoration runs without adding incompatible
fields. The optional iteration argument defaults to the previous 60.

The legacy uniform-grid case reproduces its complete numerical recipe and
selected peaks exactly. The new nonuniform case verifies all old nodes, fixed
bounds and initial positions are preserved. This is residual-based selection of
numerical check times, not a prescribed foot-contact sequence.

All 14 contact-implicit integration tests pass. The added analytic test checks
that the restoration partition excludes task/sliding costs, rejects mismatched
sample times, preserves the fixed initial state, and restores a frictionless,
unactuated body without inventing force to follow an impossible acceleration
request. The robot reducer additionally checks every selected penetration,
balance and motor residual against the native final search vector.

```sh
node examples/full-robot/contact-implicit/summarize_restoration.mjs
```

This reproduces `restore32-summary.json`, verifies the paired inputs and fixed
displacements, checks exact independent final reports, and checks identical
physical frames at shared times in the dense audits. Loaded slip uses the
previous force-weighted, at-least-1-N loaded-foot measure divided by body XY path.

Recipes, completed native outputs, logs, test/build results, selector regression
and separate binary identities are preserved. The full-objective control uses
the verified executable recorded in `equality32-build-identities.json`; the new
restoration build is separately identified in `restore32-build-identities.json`.
This work begins at `f28a1cf`; no ignored run directory is the sole baseline.
