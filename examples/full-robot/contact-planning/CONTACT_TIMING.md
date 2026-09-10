# Contact-relative force timing

TOWR's ideas remain unexhausted. Its [spline holder](https://github.com/ethz-adrl/towr/blob/master/towr/src/spline_holder.cc)
connects motion and force splines to phase durations; [duration updates](https://github.com/ethz-adrl/towr/blob/master/towr/src/phase_durations.cc)
notify those splines. Our existing force curves already stretched with each
foot's own stance duration. However, the additional knots introduced by
`with_event_aligned_linear_forces` were aligned to *other feet's* transitions
only at initialization. Subsequent timing changes could move load-transfer
windows away from those knots.

The shared Rust planner now optionally attaches force knots to contact events,
or to a fixed fraction between adjacent events. Every motion probe resolves the
times again, including force initialization, full and cached evaluation,
analytic force derivatives, optimizer decoding and returned candidates. Force
coefficients remain independent optimization variables. The old JSON format
omits this optional metadata and preserves its previous evaluation behavior.

This is an extension motivated by phase-dependent splines, not an implementation
of all TOWR features. It explicitly holds the initial cyclic ordering of contact
events fixed. Simultaneous or reordered events are rejected; their separation
must exceed 1e-10 normalized phase for numerical validity. That restriction is a
local search domain, not a physical timing limit. Different orderings and
simultaneous-event families still need separate parameterizations/searches.
The existing one-stance/one-swing model, numerical motion derivatives and dense
inequality augmented-Lagrangian solver remain limitations.

## Analytic and CAD checks

- Two overlapping supports, with unchanged force coefficients: changing phase
  and duty fractions leaves a **0.1400022645** maximum normalized load error with
  fixed knots; contact-relative knots give **9.9920072e-16**. This isolates a
  representation error; it is not a robot speed bound.
- The same test checks initial force fidelity, zero endpoints, unchanged force
  coefficients, event-order and coincident-event rejection, invalid endpoint
  bindings, serialization, cyclic phase shifts and period scaling.
- On the actual 443-variable, broader-height robot recipe, initial force curves
  change by at most **2.1316282e-14 N**, and normalized knot times by at most
  **1.1102230e-16**. No force coefficient, variable bound, body/foot motion,
  physical parameter, speed objective or solver setting changes.
- The prior native full CAD report is identical when read and compared through
  JSON. The starting speed remains **0.2117102645 m/s**, force error
  **12.5245238484 N**, moment error **3.0667182083 Nm**, and minimum torque margin
  **-0.2695338587 Nm**. It remains infeasible.
- A +0.0001 phase perturbation of foot 1 deliberately leaves serialized knot
  times stale. Resolving inside the evaluator, explicitly materializing them,
  replaying the cache, and clearing metadata on the materialized fixed reference
  all produce byte-identical native reports.
- At this perturbed timing, all **1,098** force-column central differences pass
  across three distinct cases (original curved body, constant body, alternate
  interpolation). Maximum scaled error is **9.06274975e-9**, no fallback columns,
  and all six independently recomputed CAD probes match the cache.

`bind_joint_contact_timing` uses the shared library to produce the bound recipe
and audit. `prepare_joint_contact_timing.mjs` verifies unchanged inputs and
extracts `joint-timed-speed.recipe.json` plus its initial report and timing probe.
Extraction through JavaScript canonicalizes signed zeros. The 8,000-evaluation
contact-relative speed search is now running with the separately built optimizer
in `target/gait-contact-timing`; **no completed result is recorded here yet**.
The older comparison optimizer in `target/gait-exploration` remains unchanged.
The new executable's full initial report matches the prepared report after
canonicalizing 18 signed-zero differences; every other value is identical.
`joint-timed-speed-launch.json` records the live session, source/build/compiler
identities, arguments and inputs. Its result and log remain untracked until the
process finishes; this launch is not a speed or feasibility claim.

## Completed searches and support diagnostics

The eight-body-control, original-force-basis search finished at the penalty
limit: 7,181 evaluations, 48 accepted inner steps, speed **0.025 m/s**, force
error **0.46195638 N**, moment error **0.12831447 Nm**, torque margin
**-0.07142617 Nm**, maximum normalized inequality **8.23912760**. No feasible
candidate was found. Its original launch/build identities and final result are
preserved; this is not evidence of a physical speed ceiling.

Two previously completed duty-0.75 multistarts were also checked with independent
point forces at each instant, omitting temporal force curves, friction and motor
limits. The constant-mean-body start's final motion excludes balance at 18/160
frames (maximum floating-point dual lower bound **6.84870844**, required <=1).
The reference-body start excludes 32/160 (maximum **15.11006678**). Independent
CAD reconstruction error is zero in both. Thus those fixed body motions/contact
schedules cannot be rescued by adjusting force curves alone. Body/foot/timing
optimization remains necessary; these diagnostics do not establish a global
physical limit or claim that all contact patterns have been tried.

The static aligned-force and wider-height searches were still running when
this evidence was prepared. No new runtime or browser gait is promoted.
