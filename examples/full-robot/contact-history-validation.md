# Contact-history queries without unused force solves

The shared articulated library now evaluates contact-history rates directly.
It omits the inverse-dynamics and joint-reaction work previously performed by
the full evaluation used for the implicit history update. Physical equations,
geometry, controller, tolerances, and event handling are unchanged.

## Dependency contract

`Articulated::prepare_contact_history_rates` returns an immutable evaluator.
For the bristle friction model it prepares link kinematics and contact geometry,
then calls the original contact law for every query. Changed joint/base/modal
position or velocity refreshes kinematics; changed poses refresh geometry.
History values and contact forces are never cached. Acceleration changes alone
do not alter contact-history rates. The full prepared evaluation and the new
contact-only evaluator share their motion-dependency predicate.

For regularized Coulomb friction, the existing history states do not contribute
to friction forces. They follow `rate = -200 * history` both on the floor and in
flight. The contact-only evaluator calls the same shared decay function as the
full force evaluation, without constructing geometry or kinematics. When
contact is disabled, the existing zero-rate behavior is preserved. Grounded
links remain excluded. The original zero/one probes, affine-history solve and
final original-rate residual check are retained.

This shortcut is valid because of the selected library component's equations,
not because of the robot's identity or a numerical zero observed at one pose.
Actual foot/contact forces still use fresh geometry, velocity and load in
dynamics preparation. A future friction model must explicitly implement its
history dependencies rather than inheriting a memoryless assumption.

## Validation

The selected Rust suites pass 53 tests, with one pre-existing experimental SDF
derivative test explicitly ignored. History rates are compared bit-for-bit
against full evaluation while perturbing states, rates, joint coordinates,
temperatures, and signed zeros. Existing fixtures cover flex, sequential joint
axes, compliant boundaries, heightfields, pair contacts, touchdown/liftoff,
velocity reversal and return to the prepared point after rejected probes.
Both contact laws and disabled contact are covered. Independent sliding,
energy-dissipation, loaded motor, implicit stepping and replay tests also pass.

Both complete 2.8 s robot runs, at 0.25 and 0.125 ms, match the frozen previous
binary exactly in every sampled frame, terminal frame, hybrid event/subdivision
record, solve diagnostic, accepted contact segment and integrated impulse.
This preserves the parent sampled-lift result and all its limitations; it does
not fix the existing local/simultaneous solver discrepancy or establish walking.

| Development profile | Previous 0.25 ms | Optimized 0.25 ms |
|---|---:|---:|
| Contact-history work | 16.24 s | 0.39 s |
| Closure mapping | 39.32 s | 38.37 s |
| Dynamics preparation | 18.36 s | 18.56 s |
| Component equations | 10.76 s | 10.45 s |

These development runs overlap other jobs. They identify eliminated work but
are not an isolated wall-time speedup claim. The separate ABBA benchmark runs
the frozen previous/optimized binaries on identical complete-motion inputs,
with no other assistant-launched heavy jobs overlapping. Its machine-readable
results are in `contact-history-status.json`. Host background work is not
controlled, and two measurements per method on one workload do not establish
general speedup or realtime performance.

The sequential ABBA timings are 84.56, 71.51, 70.95 and 84.52 s. Means are
84.54 s before and 71.23 s after: **1.187× faster, or 15.7% less wall time**,
on the recorded Intel i9-9980HK host. Every benchmark run also passes exact
trajectory/contact/event comparison. Throughput is 0.0393 simulated seconds
per wall second, still about 25.4 times slower than realtime. Timings cover the
session's stepping work, not build/startup or writing the JSON capture.

## Browser and delivery

The browser bundle uses the same changed Rust library. Validation runs the full
281-frame `robot-point-final-refresh` motion against its native reference,
checks exact browser replay/reset and invalid-request preservation, and runs
the maintained viewer interaction suite. The summary script requires both
reports to pass. Existing controller configurations and readiness labels remain
unchanged; this optimization introduces no new controller preset.

All 281 native/WASM frames pass the existing 1e-7 absolute-entry diagnostic
(maximum 7.34e-8 N), with exact replay/reset and invalid-request preservation.
All 21 viewer checks pass on the 19-preset bundle, and the robot view was
visually inspected. The browser motion took 119.38 s for 2.8 simulated seconds
while other validation work overlapped; the main thread remained responsive
but the longest simulation chunk was 1.92 s. Responsiveness of controls is not
realtime controller latency.

The shareable build is
`runs/interactive/robot-lab-contact-history-2026-09-07.zip`.
The source/binary snapshot is under `runs/interactive/contact-history`.
The installed viewer and unsaved CAD session are not reloaded by these tests.

## Reproduce

```sh
node examples/full-robot/prepare_contact_history.mjs
cargo test --locked -p sim-domain-robot --test jacobian --test embedded_step --test embedded_motor --test regularized_friction -p sim-runtime --test embedded_session
cargo build --locked --release -p sim-runtime --example integrate_embedding
target/release/examples/integrate_embedding runs/full-robot/learning/point-feedback/scene.json runs/full-robot/learning/contact-history/base.config.json > runs/full-robot/learning/contact-history/base.execution.json
```

Repeat the last command for `refined`. Preserve the previous source/binary
snapshot before rebuilding. Once other heavy jobs finish,
`benchmark_contact_history.mjs` executes four sequential complete runs and
checks exact physical/numerical records. Rebuild the WASM bundle and run
`web/tests/embedded.mjs` and `web/tests/viewer.mjs`; package the validated bundle,
then run `summarize_contact_history.mjs`. The summary rechecks captures and
records artifact hashes; ignored outputs alone are not the durable baseline.

Closure mapping is now the largest measured component. Further performance
work should investigate that repeated preparation while preserving all
original linkage/rank checks. Task-specific accuracy, deployable observations,
walking, policy learning and controller-routed WASD remain required.
