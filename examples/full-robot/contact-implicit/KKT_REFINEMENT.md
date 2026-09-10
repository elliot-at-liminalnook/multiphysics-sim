# Residual refinement removes the reproduced linear failure

The original exact-L1 search now passes the linear solve that previously stopped
it. The first 23 accepted iterations exactly reproduce the old recorded history.
At iteration 23, one residual correction reduces relative KKT error from
1.478030277e-6 to 2.139746142e-10, below the unchanged 1e-6 requirement. The run
then completes its 30-iteration budget. **This is a verified numerical repair,
not a qualified gait or a speed improvement.**

## Shared solver change

After an inaccurate whitened KKT solve, compute residuals in the original
unscaled equations used by that linear solve:

- dual: H p + Aᵀ λ + g;
- primal: A p + c.

Reuse the same Cholesky and whitened-constraint SVD factors to solve the KKT
correction with the negative residual as right-hand side. Add the correction
and recheck the original equations. At most three improving corrections are
retained. A correction that worsens the checked error is discarded. Neither
linear nor physical tolerances change, and inconsistent constraints still fail.
Here "original" distinguishes the KKT equations from the factored equations;
H, A and g already include the optimizer's variable scaling.

Reports now separate normalized primal and dual errors, preserve the initial
error and number of successful refinements, and record the full Newton step
norm. Nonfinite diagnostics cannot pass through a maximum operation unnoticed.
The example's scope string also distinguishes configured dogleg from exact-L1
globalization.

## Experiments and physical limits

All runs use the same CAD, 96 foot-surface samples, contact law, 32 periodic
controls, 80 optimization samples, numerical target and bounds documented in
[EQUALITY_CONSTRAINTS.md](EQUALITY_CONSTRAINTS.md). Audits use 512 physical samples
and 513 CAD geometry poses. Values below are planned motion, not measured runtime
walking.

| Trial | Dense force error N | Dense moment error Nm | Dense torque margin Nm | Dense loaded slip | Planned rate m/s |
| --- | ---: | ---: | ---: | ---: | ---: |
| Original exact-L1, stopped at linear failure | 1.837673 | .681857 | −.031938 | 90.852% | .201115 |
| Original recipe with refinement, 30 iterations | 1.837033 | .679743 | −.038309 | 90.452% | .200927 |
| Eight-iteration restart, either executable | 1.836954 | .680378 | −.037069 | 90.490% | .200889 |

The new run's sampled force error decreases only from .170793 to .170422 N.
Dense force and moment error remain much larger than the .05 N/.02 Nm gates;
the dense torque violation worsens. About 15.4 µm sampled overlap and roughly
90% loaded slip also remain. No candidate is promoted, no browser controller
is replaced, and no physical speed maximum has been established.

The separate restart experiment uses the exact last accepted motion from the
old failure as its initial input. Both old and new executables complete eight
iterations with bit-identical positions and physical reports; neither invokes
refinement. Restarting is therefore not a reliable reproduction of the earlier
failure. The original-recipe replay supplies the stronger evidence above.
Restart also resets trust/merit state and re-encodes positions; this experiment
does not isolate which reset causes its different numerical path.

The new diagnostics identify a second issue: restart full Newton steps have
scaled norms 69.5–135.9 while end-of-iteration radii are .025–.1. Original replay
iteration 23 has norm 98.46 and radius .05. This indicates severe shortening of
the full constrained direction and helps explain slow feasibility progress;
it is not evidence of a physical speed limit. A useful next algorithmic test
is a separate trust-region feasibility step minimizing linearized base-balance
error, followed by task optimization, rather than merely increasing iteration
budgets or weakening the gates. Dense contact transitions, slipping, collisions,
bound handling and eventual runtime entry/tracking still need work.

## Reproduction and checks

`equality32-refined-build-identities.json` records the new source/binary hashes
and the preserved old executable hash. The old code is recoverable from
`050c0a7`; no old binary needs to remain in `/tmp` to reproduce its build.
`equality32-refined.recipe.json` is the common eight-iteration restart recipe;
`equality32-refined-original.recipe.json` exactly matches the original experiment
except provenance. Logs/results retain all three completed runs.

20 shared solver tests and 13 contact-implicit integration tests pass. The new
ill-conditioned equality regression checks closure against known solutions,
exercises successful refinement, and verifies that unattainable strict accuracy
still reports failure. The existing inconsistent-constraint, bound, nonfinite,
nonlinear and unactuated-body checks continue to pass.

Run:

```sh
node examples/full-robot/contact-implicit/summarize_kkt_refinement.mjs
```

This reproduces `equality32-refined-summary.json`, verifies matching original
recipes, counts the exact 23-iteration history prefix, checks identical restart
outputs, and checks exact independent reports and same-time dense frames. Slip
uses the previously documented force-weighted loaded-foot measure. Native
optimizations and audits are terminal before this evidence is indexed.
