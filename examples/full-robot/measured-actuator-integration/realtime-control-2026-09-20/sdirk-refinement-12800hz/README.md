# SDIRK refinement remains unqualified

The full three-second 12800 Hz SDIRK2 run completes in 74.128094 s. Halving
the previous 6400 Hz step fails the unchanged physical comparison gates:
peak motor-angle difference 0.010859 rad (0.622 degrees), link-origin difference
4.977 mm, current difference 0.15277 A, contact-force difference 8.5205 N,
and different contact identities. Sample/application counts match. This is
pairwise simulation sensitivity, not measured hardware error; neither run is
a converged reference. The earlier zero-voltage short-window convergence does
not extend to this controlled three-second case.

`comparison.json` is a timestep-sensitivity result, separate from the qualified
same-method Jacobian reuse receipt in `../stage-proposal-reuse/step-1/`.
Reproduce the comparison without overwriting solver-optimization receipts:

```sh
node ../compare-refinement.mjs stage-proposal-reuse/step-1/candidate sdirk-refinement-12800hz/reference sdirk-refinement-12800hz/comparison
```

The root-qualified performance recipe remains BE6400 with its existing explicit
timestep-convergence limitation. No gate or selected timestep changed.
