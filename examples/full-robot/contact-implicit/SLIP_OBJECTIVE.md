# Shared slip shaping reduces sliding but does not produce a qualified gait

The new shared Rust slip objective reduces independently audited loaded slip
from **89.52% to 70.18%** at the same planned displacement rate. Stronger shaping
also increases balance error. No trial passes the .05 N force, .02 Nm moment,
−.001 Nm torque-margin, 5% loaded-slip and zero sampled-overlap requirements
together. **No faster validated gait, online controller or physical maximum is
established.**

## Shared component and planner integration

`sim-domain-control::contact_slip` implements the
[derived sampled bound](SAMPLED_SLIP_BOUND.md). Its registry descriptor,
`control.contact_slip`, exposes the same validation and typed ports to component
consumers. Required parameters are duration (s), displacement (m) and load
threshold (N). Inputs are a positive sample weight (s), point/group normal loads
(N) and two tangential material velocities (m/s); outputs are dimensionless
residual components. It computes an objective contribution, not a contact force.

The focused example is runnable with:

```sh
cargo run -p sim-domain-control --example contact_slip
```

Its constant loaded-sliding case has bound 1, while stationary loaded contact
plus unloaded swing has bound 0. These are declared analytic observations, not
a robot simulation.

`ContactSlipObjective` requires an explicit group name for every marker, the load
threshold, target ratio and residual scale. This robot's recipe explicitly maps
its 96 markers to the four authored foot-crosshead links. The planner obtains
normal loads and material velocities from its existing shared physical report.
It computes both the original loaded-slip statistic S and the conservative
upper bound B for each group. Duration and displacement come from the analytic
periodic motion; nonpositive displacement or incompatible reports are rejected.

The optional shaping residual per group is

    max(B - target_ratio, 0) / ratio_scale.

`restore_feasibility_with_slip` appends these group residuals to the existing
balance, motor-violation and penetration objective. The original
`restore_feasibility` API delegates with no slip objective. Explicit `None` and
the old entry point produce identical results in the regression.

The new `within_sampled_slip_limit` flag checks **S**, not B. The stronger bound
is a shaping surrogate and does not replace the original acceptance gate.
Planning feasibility, slip, dense-time coverage, collisions and runtime behavior
remain separate checks; solver stationarity is insufficient. The analytical
bound applies to the supplied sampled observations, not unobserved times or
hardware contact.

## Matched robot study

All four searches start from `restore32-targeted2.result.json`, with identical
CAD, contact law, initial motion, 32 controls, 144 check times, 574 free variables,
bounds, fixed displacement, .39608749 s duration, and 30-iteration least-squares
settings. Planned displacement rate is .2009265099 m/s for every trial. Only the
optional slip objective and its scale differ. Target ratio is .05 and the
loaded-force threshold remains 1 N.

All rows below use independent 512-point physical/slip audits and 513 sampled
CAD geometry poses. A smaller residual scale means stronger slip shaping.

| Shaping scale | Dense slip | Dense force error N | Dense moment error Nm | Dense torque margin Nm | Max overlap µm |
| --- | ---: | ---: | ---: | ---: | ---: |
| None | 89.520% | .176938 | .068240 | −.042223 | 17.20 |
| .10 | 77.908% | .185829 | .066418 | −.025380 | 17.01 |
| .05 | 72.873% | .215390 | .069810 | −.002322 | 16.65 |
| .02 | 70.175% | .388272 | .138269 | −.008774 | 16.32 |

The strongest weight gains only about 2.7 percentage points of slip over the
intermediate .05 weight while substantially worsening force and moment error.
This is a measured tradeoff, not evidence that continually increasing the
weight will solve the gait. Every new trial reaches the iteration limit; none
establishes stationarity, a local physical optimum or a global speed ceiling.
Costs with different shaping weights are not directly comparable.

The force-thresholded contact timings change without prescribed stance flags.
For example, the +Y foot's loaded fraction falls from .8594 to .8047 and the −Y
foot's from .7422 to .6738 between the control and strongest shaping. Each foot
still has one thresholded touchdown and liftoff per cycle. This is a local
change in contact timing, not a demonstrated new gait family. The remaining
70–73% worst-foot slip is far above the 5% requirement.

The .05 trial is a useful optimization checkpoint for further coupled balance
and slip work, but is not promoted. Further work should preserve or restore
physical balance while reducing sliding, inspect the remaining overlap pairs,
and assess temporal coverage and other initial motions. Increasing objective
weight alone does not satisfy these requirements.

## Verification and evidence

- All 62 control-library tests and 16 contact-planning integration tests pass.
  New cases cover constant/zero slip, unloaded swing, mixed load/velocity
  weighting, invalid signals/parameters, typed registry ports, residual
  composition and explicit grouping.
- An analytic translating frictionless body has valid balance but unit slip.
  With all positions fixed the optimizer reports stationarity, while the slip
  acceptance flag correctly remains false. This guards against mistaking a
  stationary objective for a usable gait.
- On the recorded initial 512-point curve, the new Rust physics and geometry
  reports match the previous reports exactly. Rust slip metrics agree with the
  prior independent JavaScript derivation to 6.66e-16.
- Each final report is independently reproduced. The reducer verifies every
  physical and added shaping residual, exact physical frames at shared dense
  times, fixed displacement, paired inputs and independent dense slip metrics.

```sh
node examples/full-robot/contact-implicit/summarize_slip_objective.mjs
```

This reproduces `slip32-summary.json`, including original slip, the conservative
bound, contact-load fractions and thresholded contact events for every foot.
Control dense audits explicitly request the slip diagnostic without adding it
retroactively to their optimization objective.

Recipes, completed native outputs, audits, logs, focused example output and
source/binary identities are preserved. The turn starts at `e32b26a`. New
source is shared Rust; no authored CAD property or browser preset is changed.
The contact model remains an explicit, uncalibrated planning approximation, and
the results do not claim measured runtime speed or sim-to-real accuracy.
