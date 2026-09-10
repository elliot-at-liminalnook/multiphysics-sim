# Strict sampled-slip barrier

The barrier preserves the sampled slip pass and the independent dense audit
also passes narrowly at 4.977%. Balance still fails. A paired native diagnostic
shows that the final trajectory sits on the hard 1 N loaded-set cutoff: shifting
all controls vertically by just 1e-12 m changes sampled slip from 4.569% to
5.696%. This numerical discontinuity must be addressed before extending this
finite-difference barrier search. No new gait is qualified or promoted.

This experiment addresses the previous finite solve's loss of an initially
passing sampled slip limit. It adds a strict interior barrier while retaining
the physical model, motion bounds, actual-slip inequalities and dense audit.
It does not impose a stance sequence or establish a physical speed maximum.

## Shared method

`sim-solve::inequality_barrier::reciprocal_inequality_barrier` takes finite,
dimensionless signed inequalities `g < 0` and a finite positive weight `w`.
It returns residuals `sqrt(w)/(-g)`, contributing `w/(2*g*g)` to least-squares
cost. Boundary and exterior evaluations return errors, without clipped slack.
The existing bounded least-squares solver rejects invalid trials and shortens
invalid derivative probes. Thus accepted states stay inside the supplied
barrier domain, starting from a strictly feasible state. The method can stall
near a boundary and is not a general constrained-convergence guarantee.

The optional `ContactSlipObjective.loaded_slip_barrier_weight` appends these
residuals for the shared actual loaded-slip rows
`g = (sampled_loaded_slip_ratio - target_ratio)/ratio_scale`.
The existing RMS shaping residuals and physical inequalities remain unchanged.
The separate `constrain_loaded_slip` flag still controls whether actual-slip
rows are appended to the augmented-Lagrangian constraint vector. Weighted
restoration explicitly rejects the barrier option instead of ignoring it.
The default absent option preserves old behavior and serialized recipes.

This barrier is a numerical addition to our solver, not an assertion that it
reproduces IDTO's solver. Load-threshold crossings make the actual slip metric
piecewise smooth. Accepted optimizer states are protected only on the sampled
grid: the motion between sample times, detailed contact, runtime tracking and
browser responsiveness still require independent validation.

## Matched setup

`prepare_slip_barrier_trial.mjs` clones `sliplimit68-jacobi.recipe.json` and adds
weight `0.0001`. This is an explicit numerical experiment, not a material or
actuator parameter. Its only checkpoint change is appending the initial barrier
residuals. All multipliers, signed constraint rows, next penalty, prior norm,
motion, fixed displacement, bounds, contact/actuator configuration and solver
budgets match the control. Native warm-start validation must exactly reproduce
that extended initial residual vector before optimization starts.

There are 128 physical check times, 32 cubic controls, two additional outer
iterations of at most 30 inner iterations, and Jacobi scaling exponent 0.5.
The planned +45 degree displacement rate is fixed at 0.0670626816 m/s.
The independent dense audit uses 512 physical times and 513 geometry poses.
This low-slip feasibility experiment is a prerequisite for speed continuation,
not a replacement for the requested maximum-speed goal.

## Results and numerical diagnosis

| Dense audit | Force error N | Moment error Nm | Torque margin Nm | Actual slip | Overlap |
| --- | ---: | ---: | ---: | ---: | ---: |
| Matched control without barrier | 0.117246 | 0.042090 | +0.247133 | 7.520% | 0 |
| Strict sampled-slip barrier | 0.156734 | 0.063064 | +0.254456 | 4.977% | 0 |
| Earlier retained `warm68` | 0.129337 | 0.075612 | +0.277906 | 4.741% | 0 |

The barrier's coarse slip is 4.569%, while the dense check reaches 4.977%.
Maximum point penetration is 0.107934 mm. The force and moment limits remain
0.05 N and 0.02 Nm; both fail. This is a balance/slip tradeoff, not a qualified
improvement over the retained `warm68` seed. Preserve both diagnostic states.

The two outer iterations consume 72,581 evaluations and both inner solves hit
their 30-iteration caps. The outer penalties are 1 and 1, unlike the control's
1 and 10: the same adaptive penalty rule responds to different residual
histories. The next checkpoint penalty is 10. Rejected evaluations total
3,763, and final damping reaches 1.457e9 with a large projected gradient.
There is no stationarity or physical-infeasibility certificate.

At 0.5309145529936997 s, the -X foot carries 0.9999999999553548 N, just below
the hard 1 N cutoff used by the actual loaded-slip metric. Two independent
Rust audits translate every cubic control and the periodic endpoint vertically
by +/-1e-12 m, preserving joints, phase grid, horizontal displacement and bounds:

| Numerical probe | Foot load N | Sampled actual slip | RMS bound |
| --- | ---: | ---: | ---: |
| Up 1e-12 m | 0.999999921259 | 4.56941181% | 27.12430279% |
| Down 1e-12 m | 1.000000078652 | 5.69565938% | 27.12430382% |

The actual metric jumps by 1.126 percentage points while force-balance error
changes by only 1.13e-6 N. The RMS bound remains numerically continuous but is
too conservative for this already low-slip trajectory. These probes establish
a sampled loaded-set discontinuity at the final state. They do not claim
picometre CAD accuracy or prove the cause of every rejected evaluation.

The next solver work should handle this cutoff explicitly, for example through
contact-event-aware integration or a tighter continuous sufficient slip bound,
while retaining the independent actual-slip gate. Merely lowering barrier
weight or increasing the iteration cap does not resolve this discontinuity.
Dense slip is already close to its limit, so a coarse pass cannot justify
promotion to runtime walking or speed continuation by itself.

One specific next option is the continuous mean-slip upper bound
`L = sum(dt * sum(fn * |vt|) / max(group_load, N0)) / body_path`.
Above the cutoff its integrand equals the actual metric; below it, the added
term is nonnegative. Therefore `actual <= L`. Cauchy-Schwarz and
`body_path >= displacement` also give `L <= RMS_bound` on the same quadrature.
Unlike the hard cutoff, this expression is continuous, though it still has
norm/max kinks. It is not yet integrated into a Rust optimizer or controller.

An explicitly diagnostic reduction of the recorded Rust audit data gives a
maximum `L` of 3.217% for the original recorded seed, 5.563% after its first
restoration, 5.806% for `warm68`, and 6.954% for this barrier result on their
respective coarse grids. Thus the original recorded seed supplies an interior
starting point for a potential continuous-bound barrier, while the later
states do not. Its large balance error remains to be restored. This is a
concrete follow-up supported by the audits, not another qualified gait claim.

## Verification and reproduction

All 29 shared solver tests and 16 planner tests pass. The scalar barrier test
drives toward an exterior unconstrained optimum, verifies rejected unsafe
trials, and checks the resulting interior stationary equation. Domain tests
reject boundary, nonfinite and invalid-weight inputs. The planner test rejects
100% slip even when force balance passes, checks exact barrier residuals, and
verifies that the weighted restoration API cannot silently discard the option.

The new binary exactly reproduces the previous parent physical, slip and full
geometry audit. All four earlier inequality summaries remain byte-for-byte
reproducible using the extended shared verifier. Build and source identities,
test logs, checkpoint preparation, recipes and independent audits are retained.
Two initial invocations supplied a policy config in the capture-marker slot;
both exited during input parsing before any physics evaluation. Their terminal
failure evidence is retained, and the corrected commands use `surface-markers.json`.

Run `optimize_contact_inequalities SCENE surface-markers.json RECIPE`, then
`audit_contact_implicit SCENE surface-markers.json RECIPE RESULT 4 --pairs`.
Repeat the audit with the dense recipe and 16 subdivisions. Use the shared
constrained scene recorded in the evidence index. Run
`summarize_slip_barrier_trial.mjs` from the repository root to verify matched
inputs, checkpoints, barrier residuals, multiplier updates and dense metrics.
