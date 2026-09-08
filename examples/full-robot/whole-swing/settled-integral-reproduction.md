# Reproduce the frozen integral teacher

The versioned scene, task and recipe archive are sufficient to reconstruct the
five original cases. This path does not read prior `runs/` artifacts. Build
the current shared Rust runtime, then use a fresh output directory:

```sh
cargo build --locked --release -p sim-runtime --example run_environment --example evaluate_lift
node examples/full-robot/whole-swing/reproduce_settled_integral.mjs runs/integral-regression
node examples/full-robot/hybrid-speed/run.mjs runs/integral-regression runs/integral-regression/status.json
node examples/interactive/audit_walking_suite.mjs runs/integral-regression/plan.json runs/integral-regression/status.json runs/integral-regression/integrity.json
node examples/full-robot/whole-swing/collect_settled_integral_regression.mjs runs/integral-regression/plan.json runs/integral-regression/status.json runs/integral-regression/integrity.json runs/integral-regression/results
node -e 'require("node:assert/strict")(require("./runs/integral-regression/results-summary.json").declared_suite_passed)'
```

The new plan records the current binary and all input hashes. The collector
retains full outcomes or explicitly labelled accepted prefixes, checks the
three stop endpoints in each longer command case, and compares minute-long
trajectories at 1.25/0.625 ms. The five original reconstructed configuration
objects and complete action arrays were checked for exact equality. Source
hashes identify local raw captures separately from the versioned measured
results; raw captures are not bundled in the repository.

The workflow `walking-reference.yml` runs this regression on later changes.
These cases were held out from selection of the original integral candidate;
they are revealed regression cases for subsequent development. Passing them
does not supply new held-out, terrain, browser or hardware evidence.
