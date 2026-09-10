# Shared task progress

`sim_domain_control::displacement::measure` computes net endpoint displacement
and its position gradient on explicitly selected Cartesian axes. The same
calculation is available as `control.net_displacement` in the component registry
(length inputs/output, dimensionless gradient and axis parameters), and as
`net_displacement(origin, position, axes)` in Rhai.

An environment can select a named CAD link and world axes:

```json
"progress": {"link": "chassis", "axes": "xy"}
```

Each reward is the change in net displacement from reset, in metres. The
undiscounted sum equals endpoint distance. Divide by the **full configured
duration** for sustained net speed. Failed and incomplete evaluations have no
eligible completed-episode score; their progress remains diagnostic.

Failure uses separately authored `termination_bounds` over typed observations.
There are no implicit upright, ground-contact, slip or gait restrictions. These
bounds are sampled at action endpoints. They do not yet replace every legacy
fall condition: CAD hull clearance needs a reusable observation or failure rule.
The existing `speed` task retains its previous fall semantics.

The progress objective excludes other reward terms. Robot properties stay in
CAD; selecting progress axes or failure bounds does not alter physical limits.
`pendulum.progress.environment.json` is a focused XZ example using the production
environment, with no failure bounds. It is not a second locomoting robot.

## Verified checkpoint

- Two displacement tests cover all axis selections, analytic gradients,
  translation invariance, invalid inputs, registry units and legacy XY arithmetic.
- The Rhai test covers units, execution and transactional rejection.
- Runtime library tests (21) and environment tests (11) passed, including exact
  replay, raw-session physics agreement, explicit failure bounds, and rejection
  of failed or incomplete scores.
- The fastest quadruped's first 0.1 s retains exact native frames, transitions
  and recorded inputs against the previous runtime, excluding wall time. Moving
  that capture to `progress` preserves physics, rewards, distance and speed.
- Production WASM worker and replay agree with native on the quadruped's first
  0.1 s and pendulum's first 0.06 s: no differences outside 1e-7 absolute plus
  1e-8 relative tolerance across 1,267,086 numeric comparisons.

`shared-progress-evidence-v1.json` durably references the captures, inputs, build
identities and reports. `web/tests/progress-native.mjs` checks the extraction;
`web/tests/progress.mjs` checks native/WASM parity and held-input replay. CI runs
the Rust/Rhai tests and pendulum native/WASM case. The live user browser bundle
is unchanged.

This checkpoint establishes an extraction and short runtime parity, not sustained
browser speed, realtime fidelity, or the full cross-robot learning objective.
See [the full delivery ledger](shared-robot-learning-plan.md) for remaining work.
