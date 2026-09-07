# Body and foot observations for controller development

`sim-runtime::task_observation::TaskObserver` adds explicitly named ideal-state
observations to the existing sampled Rhai contract. It resolves link identities
and marker offsets against the declared CAD hash. No sensors, masses or contact
parameters are added to the robot. The current robot export has no declared
sensors, so this is a teacher/diagnostic interface, not a deployable estimator.

The opt-in `policy.task_observations` config is
`examples/full-robot/task-observations.json`. The robot uses the chassis link as
its moving reference frame and the four previously declared foot markers. The
45 additional scalar channels have these meanings:

| Channel group | Units and frame | Meaning |
| --- | --- | --- |
| `body.gravity_direction.{x,y,z}` | dimensionless, body axes | Unit gravity direction; describes tilt, not yaw. It is not raw accelerometer output. |
| `body.linear_velocity.{x,y,z}` | m/s, body axes | Absolute world velocity of the body COM, resolved in body axes. |
| `body.angular_velocity.{x,y,z}` | rad/s, body axes | Absolute angular velocity, resolved in body axes. |
| `marker.<id>.position.{x,y,z}` | m, body COM and axes | Marker position relative to the moving body. |
| `marker.<id>.velocity.{x,y,z}` | m/s, body COM and axes | Time derivative of that relative position, including body rotation. |
| `marker.<id>.floor_force_world.{x,y,z}` | N, world axes | Summed floor force on the marker's entire link; internal contacts excluded. Not a force at the marker point. |

Marker offsets are metres from the exported link COM in link-local axes. The
same point/COM convention is used by the existing tracking tools. Relative
velocity uses both bodies' translation and rotation. In particular, a point
rigidly fixed to a rotating reference body has zero relative velocity. Nonzero
marker velocity does not itself prove sliding at a contact patch.

Observations are sampled at the policy's simulation-time boundary and held in
telemetry until its next sample. The viewer shows the observation timestamp
separately from the current displayed physics time. Reading observations does
not advance sensor clocks, mutate contact memory, or change the physics. Floor
forces trigger a fresh shared rigid-body/contact evaluation only at policy
samples; they are not cached across changes in physical state.

## Reproduction and evidence

First prepare the existing joint-feedback experiment. Then:

```sh
node examples/full-robot/prepare_task_observations.mjs
cargo test --locked -p sim-runtime --test task_observation --test embedded_session
cargo run --locked --release -p sim-runtime --example integrate_embedding -- \
  runs/full-robot/learning/task-observations/scene.json \
  runs/full-robot/learning/task-observations/config.json \
  > runs/full-robot/learning/task-observations/execution.json
```

The preparation tool captures unchanged robot/world/controller inputs plus the
explicit observation config and records their hashes. It retains gain 0.5 and
the prior motion; the robot's controller does not yet use these new signals for
landing or balance.

Two observation tests check an independent numerical time derivative of a
rotating-frame point, fixed-point invariance, gravity projection, named frames
and units, floor-force summation, internal-contact exclusion, and invalid link,
marker and provenance rejection. Seven session tests include a Rhai controller
whose output actually depends on a marker observation, with changed inputs and
replay in different chunk sizes. The browser fixture repeats this controller
probe with explicitly synthetic test-only provenance; it is not a CAD artifact.
These checks are included in the existing CI path; remote CI was not run here.

The full native experiment completes 1.6 seconds in 91.05 wall seconds on this
run. All 161 sampled physical frames match the original gain-0.5 experiment
exactly after excluding the deliberately expanded policy telemetry. This proves
observation invariance for this trajectory, not hardware sensing accuracy or a
new speedup. Preserve `observation-invariance.json` with the capture.

Build the WASM bundle with `web/build-viewer.mjs` and select
**Quadruped · body and foot observations**. Expand the inspector's **Body and
foot observations** section. It shows current sampled body/foot signals, and
save/replay restores the same readings. `web/tests/viewer.mjs` exercises this
full robot preset when its artifacts are present. `web/tests/embedded.mjs`
compares every native/WASM reporting frame and exact browser replay/reset.

## Remaining work

This provides information for task-space policies; it does not implement one.
A landing controller still needs explicit foothold references, support/contact
qualification, phase transitions and failure handling. Dynamic balance and
feasible step planning remain separate tasks. Deployable control needs declared
hardware sensors, their rates/noise/latency, and an estimator; ideal gravity,
velocity and contact forces must not silently become student-policy inputs.

The complete 1.6 s browser run passes all 161 sampled frames with maximum absolute
entry difference 4.565e-9 N in a floor-force observation. Replay and reset are
exact apart from wall-time instrumentation; invalid requests and changed recipes
preserve state. The browser run takes 104.75 wall seconds, with a worst 10 ms
simulation chunk of 2.269 wall seconds under concurrent work. UI tests pass for
the expanded readout, sample timestamp, save/replay, previous controllers,
loading/error/cancellation and narrow layouts. The fixture's observation-driven
Rhai probe also passes in WASM. Exact artifacts are listed in
`task-observation-status.json`.
