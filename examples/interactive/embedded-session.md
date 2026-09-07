# Incremental mechanism and motor sessions

`sim_runtime::embedded::EmbeddedSession` owns the CAD-derived physical runtime,
independent-coordinate state, motor/driver/servo banks, firmware memory, motion
clock, and implicit solver workspace. `advance(n)` advances the existing fixed
nominal steps. Calling it from a browser, headless runner or future learning host
does not change the timestep or controller clock.

The `integrate_embedding` example is now a thin headless host for this session.
Its previous mechanical, direct-winding, driver, scheduled-servo and motion-gated
paths moved into the library. The experiment config explicitly selects its
controller path. With `config.policy` absent, the reference/servo path does not
execute `Scene.controller`. With it present, the same existing `RhaiController`
executes the captured program at `Scene.period_s`, using a typed named contract.
The separate detailed `Session` also uses that Rhai controller implementation. Neither host adds a duplicate physics model.

```rust,ignore
let mut run = EmbeddedSession::new(scene, config, seed, CaptureMode::Full)?;
while !run.done() {
    run.advance(40)?;
    let frame = run.frame()?;
    // Render or inspect committed state without changing it.
}
let report = run.report()?;
```

`Full` retains the diagnostic history expected by the existing comparison tools.
`Latest` bounds retained history for interactive hosts; its physical step and
event processing are identical. It exposes a current frame and a versioned
recording of scene, experiment, seed, committed step count, input events and failure. It does not
pretend a discarded contact trace remains available for an impulse audit.

`prepare_replay` validates the recording and returns a fresh session plus the
number of steps to re-execute. The browser worker advances these in bounded
chunks, reports progress and yields between chunks. Cancellation terminates the
worker. Reset builds a fresh session. Browser replay rejects another scene or
controller recipe so the displayed geometry cannot silently diverge from the
loaded experiment. Errors latch at the session boundary: failed sessions cannot
continue until reset/replay. Invalid advance counts leave state unchanged.

Version 3 records the failure reason as well as the successful prefix. Replay
executes the failed attempt too, and checks its reason and committed step index.
A different failure, or an attempt that unexpectedly succeeds, produces an
explicit replay mismatch. End-of-horizon failures require no extra physics step.
Versions 1 and 2 remain readable with their original prefix-only semantics; they
cannot recover failure information that was never recorded.

The coordinate map currently reconstructs its inexpensive immutable layout at
each nominal step to avoid a self-referential owning structure. It does not rerun
a closure solve during layout construction. Solver workspace and all physical
state survive host calls. No timestep, contact law or motor constant was changed
as part of this extraction.

## Checks

```sh
cargo test --locked -p sim-runtime --test embedded_session
cargo run --locked --release -p sim-runtime --example integrate_embedding -- \
  examples/interactive/pendulum.scene.json examples/interactive/pendulum.embedded.json \
  > runs/interactive/pendulum-embedded-native.json
node web/build-viewer.mjs runs/interactive/viewer --fixture-only
node web/tests/embedded.mjs runs/interactive/viewer pendulum-embedded \
  runs/interactive/pendulum-embedded-native.json runs/interactive/pendulum-embedded-browser.json
```

Build the WASM library and matching bindings as described in `web/README.md`.
The Rust session tests cover uneven chunking, diagnostic equality, inspection
without state changes, recorded-prefix replay, invalid requests, partial-run
completion reporting, latched feedback timeouts, sampled policy observations,
bounded commands and input-event replay. Browser tests compare every
native reporting frame, verify exact browser replay/reset, and exercise invalid
requests and replay provenance checks. These are numerical portability and
lifecycle tests, not hardware calibration.

## Sampled Rhai policies

`examples/interactive/pendulum.policy.json` opts into the captured Rhai program
in the scene. `policy.observation_source` must explicitly identify
`ideal_joint_state_diagnostics`; these are privileged simulator observations,
not an invented hardware sensor model. Metadata marks the contract nondeployable.
For each independent angular coordinate, the contract exposes `.angle`,
`.angular_velocity`, `.reference`, and the commanded `.target` in declared units.
Additional bounded input channels come from `Scene.controller.inputs`.

A policy period must be an integer number of nominal steps. The controller
samples committed state before the next interval and holds its target until the
next policy sample. Registered servo firmware then samples that target on its
own clock, with its existing quantization, saturation and latency. A policy
sample at a previously serviced firmware deadline cannot retroactively change
that firmware sample. This staging differs from the continuous reference target
law; compare a zero-correction sampled policy before attributing changes to
feedback gain.

`set_inputs` validates count, finiteness and bounds before changing values. Input
changes are recorded at the committed step index, with same-step changes
coalesced. Version 2 recordings include these events; version 1 fixed-reference
recordings remain readable. Replay applies each event at its original boundary,
independent of browser frame rate or chunk sizes. The viewer restores displayed
inputs after replay. Policy failure latches the session until reset or replay.

Software target bounds are mandatory and intersect any authored CAD limits.
Invalid outputs stop the experiment rather than silently clamping. These bounds
do not establish a collision-free operating envelope or measured physical stops.
Full reports capture policy sources, configuration, typed contract, seed and
input events. Frames distinguish geometric reference from commanded target and
actual joint state. Fixed-reference captures retain their original frame format.

The pendulum-policy preset and browser CI exercise live input/replay and every
sampled native/WASM frame. The full robot feedback experiment is documented in
`../full-robot/joint-feedback-validation.md`. Walking commands and deployed
observations still need their own implementation and acceptance tests.

Optional `policy.task_observations` adds typed body/marker kinematics and
link floor-force resultants through the shared `TaskObserver`. It requires an
explicit CAD hash, named reference link, local marker offsets and an ideal-state
source declaration. Observations are sampled only on the policy clock. See
`../full-robot/task-observation-validation.md` for frame definitions and tests.
